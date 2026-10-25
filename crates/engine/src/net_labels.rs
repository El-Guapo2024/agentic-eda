//! A local label on every net the intent names that a sheet draws with wires alone (the user's decision of 2026-10-24, fix batch A).
//!
//! KiCad names a net after the label that drives it (`SCH_CONNECTION::recacheName`), and a net no label drives after its best pin (`Net-(D1-Pad1)`). A
//! derived sheet draws a net inside one block as wires, so the name the intent gave it (`LED1`, `USB_DP`) would be lost to KiCad, with what it meant --
//! a differential pair, a net class pattern, a rule. So, as a KiCad user would, the sheet gets a local label with the intent's name on one of the net's
//! wires: KiCad then calls the net `/<sheet path>/<name>`.
//!
//! The label sits on the interior of a horizontal run of the net's wire, reading to the right (`label_run_dir`: a label no wire ends at reads to the
//! right), on the grid, where its flag keeps clear of everything else drawn on the sheet -- symbols, their fields, other labels, power symbols, sheets,
//! other nets' wires -- and inside the drawing area; the place nearest the middle of the longest run that fits wins. Where no run has the room (a
//! resistor wired straight to an LED), the label goes at the end of a stub of one to four grid cells off the run, the run broken there with a
//! junction. A net with no such place either stays without a label (KiCad then names it after its best pin). A net that a label, a power symbol or a sheet pin names on the sheet already, and a name
//! that is KiCad's default name of a net (`Net-(..)`, `unconnected-(..)`), get none.

use eda_model::units::mm;
use eda_model::units::Nm;
use std::collections::BTreeSet;

use eda_model::ir::{Design, LabelKind, NetLabel, Point, SchematicSection};
use eda_model::ConstraintModel;

use crate::hier::kit::{label_rect, paper_named, sheet_pin_rect, Rect, G_NM};

/// Whether `name` is a name KiCad gives a net nothing drives: such a name is no label's text.
fn is_default_name(name: &str) -> bool {
    name.is_empty() || name.starts_with("Net-(") || name.starts_with("unconnected-(")
}

/// What a label being placed on `sch` keeps clear of: everything [`crate::obstacles::of_section_but_wires`] has, the symbols' fields, the sheets with
/// their fields and pins, and the wires of every net but `net`.
fn obstacles(sch: &SchematicSection, model: &ConstraintModel, net: &str) -> Vec<Rect> {
    let mut out: Vec<Rect> = crate::obstacles::of_section_but_wires(sch, model).into_iter().map(|o| o.rect).collect();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let mut sym = sym.clone();
        if sym.lib_id.is_empty() {
            sym.lib_id = format!("eda:{}", sym.id);
        }
        let resolved = model.real_symbol_of_instance(sch, &sym, part);
        let geom = crate::symgeom::SymbolGeom::of(&sym, part, resolved.as_ref());
        for f in crate::fields::symbol_fields(sch, &sym, Some(part), resolved.as_ref(), &geom).iter().filter(|f| f.visible && !f.text.is_empty()) {
            out.push(crate::fields::field_rect(f));
        }
    }
    for s in &sch.sheets {
        out.push(Rect::sized(s.at, s.size.0, s.size.1));
        for f in crate::fields::sheet_fields(sch, s) {
            out.push(crate::fields::field_rect(&f));
        }
        for p in &s.pins {
            out.push(sheet_pin_rect(p.at, p.at.x == s.at.x, &p.name));
        }
    }
    for w in sch.wires.iter().filter(|w| w.net != net) {
        for pair in w.pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            out.push(Rect::around(a, b, mm(0.1)));
        }
    }
    out
}

/// Every point of `sch` something connects at: the wires' vertices, the pins' tips, the labels, power symbols, no-connect flags, junctions and sheet pins.
/// A label or a stub there would join it.
fn connection_points(sch: &SchematicSection, model: &ConstraintModel) -> BTreeSet<Point> {
    let mut out: BTreeSet<Point> = sch.wires.iter().flat_map(|w| w.pts.iter().copied()).collect();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let resolved = model.real_symbol_of_instance(sch, sym, part);
        out.extend(crate::placed::pin_points_in(sch, sym, part, resolved.as_ref()).into_iter().map(|(_, p)| p));
    }
    out.extend(sch.labels.iter().map(|l| l.at));
    out.extend(sch.power_symbols.iter().map(|p| p.at));
    out.extend(sch.no_connects.iter().map(|n| n.at));
    out.extend(sch.junctions.iter().map(|j| j.at));
    out.extend(sch.sheets.iter().flat_map(|s| s.pins.iter().map(|p| p.at)));
    out
}

/// Whether a label or a stub may not be at `p`: something connects there ([`connection_points`]), or a wire of another net (or a bus) runs through it.
fn taken(sch: &SchematicSection, points: &BTreeSet<Point>, net: &str, p: Point) -> bool {
    points.contains(&p)
        || sch.wires.iter().filter(|w| w.net != net || w.bus).any(|w| {
            w.pts.windows(2).any(|s| {
                let (a, b) = (s[0], s[1]);
                (a.x == b.x && p.x == a.x && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)) || (a.y == b.y && p.y == a.y && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x))
            })
        })
}

/// The box of a wire run from `a` to `b`, as [`crate::obstacles`] measures a wire.
fn thin(a: Point, b: Point) -> Rect {
    Rect::around(a, b, mm(0.1))
}

/// Where a net's label goes: on its wire (`stub: None`), or at the end of a short stub drawn off its wire at `at` (the wire is broken there, with a
/// junction), the label reading on along the stub.
struct Place {
    at: Point,
    stub: Option<Point>,
}

/// The first place for `net`'s label on `sch` (see the module's doc): on a horizontal run of its wire, else on a stub of one to four grid cells off any
/// run of it, the runs the longest first and the places nearest their middle first.
fn place_for(sch: &SchematicSection, model: &ConstraintModel, net: &str, usable: &Rect) -> Option<Place> {
    let blocked = obstacles(sch, model, net);
    let points = connection_points(sch, model);
    let clear = |r: &Rect| usable.contains(r) && !blocked.iter().any(|o| o.overlaps(r));
    // the net's own wire runs a label's flag may not cross: every one but those its anchor is on (or a stub's run, which ends at `foot`)
    let own: Vec<(Point, Point)> = sch.wires.iter().filter(|w| w.net == net && !w.bus).flat_map(|w| w.pts.windows(2).map(|s| (s[0], s[1]))).collect();
    let label_clear = |at: Point, r: &Rect| -> bool {
        clear(r) && own.iter().all(|(a, b)| a == &at || b == &at || crate::nets::on_segment(at, *a, *b) || !thin(*a, *b).overlaps(r))
    };
    let mut runs: Vec<(Point, Point)> = sch.wires.iter().filter(|w| w.net == net && !w.bus).flat_map(|w| w.pts.windows(2).map(|s| (s[0], s[1]))).filter(|(a, b)| a != b && (a.x == b.x || a.y == b.y)).map(|(a, b)| if (a.x, a.y) < (b.x, b.y) { (a, b) } else { (b, a) }).collect();
    let len = |r: &(Point, Point)| (r.1.x - r.0.x) + (r.1.y - r.0.y);
    runs.sort_by(|x, y| len(y).cmp(&len(x)).then_with(|| (x.0.y, x.0.x).cmp(&(y.0.y, y.0.x))));
    // the grid points inside each run, nearest its middle first
    let inside = |(a, b): (Point, Point)| -> Vec<Point> {
        let horizontal = a.y == b.y;
        let (lo, hi) = if horizontal { (a.x, b.x) } else { (a.y, b.y) };
        let mid = (lo + hi) / 2;
        let mut ks: Vec<Nm> = ((lo / G_NM + 1)..=((hi - Nm::new(1)) / G_NM)).map(|k| G_NM * k).filter(|v| *v > lo && *v < hi).collect();
        ks.sort_by_key(|v| ((*v - mid).abs(), *v));
        ks.into_iter().map(|v| if horizontal { Point { x: v, y: a.y } } else { Point { x: a.x, y: v } }).filter(|p| !taken(sch, &points, net, *p)).collect()
    };
    for run in runs.iter().filter(|(a, b)| a.y == b.y) {
        for p in inside(*run) {
            if label_clear(p, &label_rect(p, (1, 0), net, false)) {
                return Some(Place { at: p, stub: None });
            }
        }
    }
    for k in 1..=4 {
        for run in &runs {
            let dirs: [(i64, i64); 2] = if run.0.y == run.1.y { [(0, -1), (0, 1)] } else { [(1, 0), (-1, 0)] };
            for p in inside(*run) {
                for d in dirs {
                    let end = Point { x: p.x + G_NM * (d.0 * k), y: p.y + G_NM * (d.1 * k) };
                    // nothing connects anywhere along the stub but at its foot, and its end is on no wire
                    let along = (1..=k).map(|j| Point { x: p.x + G_NM * (d.0 * j), y: p.y + G_NM * (d.1 * j) });
                    if along.into_iter().any(|q| taken(sch, &points, net, q)) || sch.wires.iter().any(|w| w.pts.windows(2).any(|s| crate::nets::on_segment(end, s[0], s[1]))) {
                        continue;
                    }
                    // the junction at the stub's foot keeps clear too
                    let junction = Rect::at(p).inflate(mm(0.5).get());
                    if clear(&thin(p, end)) && clear(&junction) && label_clear(end, &label_rect(end, d, net, false)) {
                        return Some(Place { at: end, stub: Some(p) });
                    }
                }
            }
        }
    }
    None
}

/// Label one sheet: see the module's doc.
fn label_section(sch: &mut SchematicSection, model: &ConstraintModel) {
    let named: BTreeSet<String> = sch
        .labels
        .iter()
        .map(|l| l.net.clone())
        .chain(sch.power_symbols.iter().map(|p| p.net.clone()))
        .chain(sch.sheets.iter().flat_map(|s| s.pins.iter().map(|p| p.name.clone())))
        .collect();
    let intent: BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    let nets: BTreeSet<String> = sch.wires.iter().filter(|w| !w.bus && intent.contains(w.net.as_str()) && !named.contains(&w.net) && !is_default_name(&w.net)).map(|w| w.net.clone()).collect();
    let usable = paper_named(sch.title_block.as_ref().map(|t| t.paper.as_str()).filter(|p| !p.is_empty()).unwrap_or("A4")).usable();
    for net in nets {
        let Some(place) = place_for(sch, model, &net, &usable) else { continue };
        if let Some(from) = place.stub {
            // the run is broken at the stub's foot, so that three wire ends meet there, with a junction (`SCH_SCREEN::IsExplicitJunctionNeeded`)
            if let Some(wi) = sch.wires.iter().position(|w| w.net == net && !w.bus && w.pts.windows(2).any(|s| crate::nets::on_segment(from, s[0], s[1]))) {
                let w = sch.wires[wi].clone();
                let i = w.pts.windows(2).position(|s| crate::nets::on_segment(from, s[0], s[1])).unwrap_or(0);
                let mut first = w.clone();
                first.id = String::new();
                first.pts = w.pts[..=i].iter().copied().chain(std::iter::once(from)).collect();
                let mut second = w.clone();
                second.id = String::new();
                second.pts = std::iter::once(from).chain(w.pts[i + 1..].iter().copied()).collect();
                sch.wires[wi] = first;
                sch.wires.push(second);
            }
            sch.wires.push(eda_model::ir::Wire { id: String::new(), net: net.clone(), pins: vec![], pts: vec![from, place.at], bus: false });
            sch.junctions.push(eda_model::ir::Junction { id: String::new(), at: from });
        }
        sch.labels.push(NetLabel { id: String::new(), net, at: place.at, kind: LabelKind::Local });
    }
}

/// Every sheet of `design` with a local label on each net of `model` it draws with wires alone (see the module's doc). The nets' names are the
/// intent's (what the drawing is derived from), as the wires of a derivation carry them.
pub fn label_named_nets(design: &mut Design, model: &ConstraintModel) {
    if let Some(root) = design.schematic.as_mut() {
        label_section(root, model);
    }
    for sec in design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()) {
        label_section(sec, model);
    }
}
