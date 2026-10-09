//! What is drawn on a sheet, as boxes: the things a field, a label or a power symbol being placed has to keep clear of.
//!
//! Each box is what eeschema gives the item (the same boxes `eda_kicad::sch_overlap` measures): a symbol's body and its pin lines
//! and pin texts, a power symbol with its Value, a label with its flag, a wire, a junction, a no-connect flag. A symbol's own fields
//! are not here: they are what is being placed.

use eda_model::ir::{NetLabel, Point, SchematicSection};
use eda_model::ConstraintModel;

use crate::hier::kit::{label_rect, nc_rect, outward, power_rect, Rect};
use crate::symgeom::SymbolGeom;

/// One thing on the sheet and the box it takes.
#[derive(Debug, Clone)]
pub struct Obstacle {
    /// The reference of the symbol it belongs to, else empty.
    pub owner: String,
    pub rect: Rect,
}

/// The way a label's text runs on from its anchor, away from the wire that ends there (a label no wire ends at reads to the right).
pub fn label_run_dir(sch: &SchematicSection, l: &NetLabel) -> (i64, i64) {
    let mut from = None;
    for w in &sch.wires {
        for (i, p) in w.pts.iter().enumerate() {
            if *p != l.at {
                continue;
            }
            // the neighbour along the wire, the other way from the label
            let q = if i + 1 < w.pts.len() { Some(w.pts[i + 1]) } else { None }.or(if i > 0 { Some(w.pts[i - 1]) } else { None });
            if let Some(q) = q.filter(|q| *q != l.at) {
                from = Some(((q.x - l.at.x).signum(), (q.y - l.at.y).signum()));
            }
        }
    }
    match from {
        Some((fx, fy)) => (-fx, -fy),
        None => (1, 0),
    }
}

fn thin(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x) - 100, a.y.min(b.y) - 100, a.x.max(b.x) + 100, a.y.max(b.y) + 100)
}

/// Every drawn item of `sch` but the fields of its symbols.
pub fn of_section(sch: &SchematicSection, model: &ConstraintModel) -> Vec<Obstacle> {
    let mut out = of_section_but_wires(sch, model);
    for w in &sch.wires {
        for pair in w.pts.windows(2) {
            out.push(Obstacle { owner: String::new(), rect: thin(pair[0], pair[1]) });
        }
    }
    out
}

/// [`of_section`] without the wires and buses: what the fields of a symbol being placed by hand keep clear of, apart from the wires, which they
/// are measured against segment by segment.
pub fn of_section_but_wires(sch: &SchematicSection, model: &ConstraintModel) -> Vec<Obstacle> {
    let mut out = Vec::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let mut sym = sym.clone();
        if sym.lib_id.is_empty() {
            sym.lib_id = format!("eda:{}", sym.id);
        }
        let resolved = model.real_symbol_of_instance(sch, &sym, part);
        let geom = SymbolGeom::of(&sym, part, resolved.as_ref());
        if let Some(b) = geom.body {
            out.push(Obstacle { owner: sym.id.clone(), rect: outward(b) });
        }
        for (r, p) in geom.pin_rects() {
            if p.length >= 0.0 {
                out.push(Obstacle { owner: sym.id.clone(), rect: outward(r) });
            }
        }
    }
    for l in &sch.labels {
        let hierarchical = matches!(l.kind, eda_model::ir::LabelKind::Hierarchical { .. } | eda_model::ir::LabelKind::Global { .. });
        out.push(Obstacle { owner: String::new(), rect: label_rect(l.at, label_run_dir(sch, l), &l.net, hierarchical) });
    }
    for p in &sch.power_symbols {
        out.push(Obstacle { owner: String::new(), rect: power_rect(p.at, p.rot, &p.net, &p.lib_id) });
    }
    for n in &sch.no_connects {
        out.push(Obstacle { owner: String::new(), rect: nc_rect(n.at) });
    }
    for j in &sch.junctions {
        out.push(Obstacle { owner: String::new(), rect: Rect::new(j.at.x - 400, j.at.y - 400, j.at.x + 400, j.at.y + 400) });
    }
    out
}
