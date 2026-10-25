//! The Net Inspector's numbers (`PCB_NET_INSPECTOR_PANEL`, `pcbnew/widgets/pcb_net_inspector_panel.cpp`): `GET /api/net_inspector`.
//!
//! One row per net of the board (`buildNetsList`: every net with a code above 0, so not the unconnected net), with KiCad's columns
//! (`buildColumns`): Name, Netclass, Total Length, Via Count, Via Length, Track Length, Die Length, Pad Count. The numbers are
//! `calculateNets`'s: `LENGTH_DELAY_CALCULATION::CalculateLengthDetails` over the net's tracks, arcs, vias and pads (`relevantConnectivityItems`)
//! with `PATH_OPTIMISATIONS { OptimiseVias, MergeTracks, OptimiseTracesInPads }` (no `InferViaInPad`), ported here function by function:
//!
//! * `optimiseVias`: a via's layers become the ones it really carries the signal between -- the layers of the lines that end in it (at its
//!   centre, or inside its pad) and of the pad it sits in (an SMD pad's copper layer, else the footprint's side): the outermost two of them,
//!   one (a stub, no height) or none;
//! * `mergeLines`: the lines that meet end to end, one to one, on one layer and with no arc in the one taken in, become one line;
//! * `OptimiseTraceInVia` / `clipLineToVia` and `OptimiseTraceInPad` / `clipLineToPad`: a merged line's end inside a via's pad is measured from
//!   the via's centre, and one that ends on a pad's position from the pad's position, both straight to where the line leaves the pad;
//! * the sums: Track Length is the lines' lengths (`SHAPE_LINE_CHAIN::Length`, an arc by its true length), Via Length the stackup height each
//!   via spans (`StackupHeight`, only with Board Setup's "Use stackup height for track length calculations", which also gates the Via Count,
//!   as KiCad's does), Die Length the pads' Pad-to-die length, Pad Count the pads; Total Length is the three lengths
//!   (`LIST_ITEM::GetTotalLength`).
//!
//! Where this differs: the pad-to-die length is read from the `(die_length ..)` a board file's pad had (the IR keeps it as one of the pad's
//! tokens, `PadMaskInfo::tokens`; a pad made here has none); a via sits in a pad (`GetConnectedPads`) when the pad's copper touches the via's
//! pad on a layer both have; a clipped end on an arc is measured along the arc's polygonisation (the 1 um `SHAPE_LINE_CHAIN` of it); the time
//! domain columns ("Show Time Domain Details") are not offered.

use eda_model::units::Nm;
use crate::board;
use eda_drc::board::DrcBoard;
use eda_drc::kimath::Shape;
use eda_model::ir::{Design, Point};
use eda_model::{ConstraintModel, PadKind};
use eda_pns::chain::ShapeLineChain;
use eda_pns::kigeom::ShapeArc;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// `LENGTH_DELAY_CALCULATION_ITEM::MERGE_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Merge {
    Unmerged,
    InUse,
    Retired,
}

/// A `LENGTH_DELAY_CALCULATION_ITEM` of type `LINE`: a track segment or an arc, then what merging made of it.
#[derive(Debug, Clone)]
struct LineItem {
    line: ShapeLineChain,
    layer: String,
    status: Merge,
    has_arc: bool,
}

/// A `LENGTH_DELAY_CALCULATION_ITEM` of type `VIA`, with the layers `optimiseVias` gives it.
#[derive(Debug, Clone)]
struct ViaItem {
    at: Point,
    radius: Nm,
    layers: (String, String),
}

/// What `CalculateLengthDetails` returns, the part the panel shows (`LENGTH_DELAY_STATS`).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LengthStats {
    pub num_pads: usize,
    pub num_vias: usize,
    pub via_length: Nm,
    pub track_length: Nm,
    pub pad_to_die_length: Nm,
    /// Track length per copper layer (`LayerLengths`).
    pub layer_lengths: BTreeMap<String, Nm>,
    /// How many tracks, arcs, vias and pads of the net there are (`relevantConnectivityItems`): a net with none has no row (`foundNets`).
    pub items: usize,
}

impl LengthStats {
    /// `LIST_ITEM::GetTotalLength`.
    pub fn total(&self) -> Nm {
        self.track_length + self.via_length + self.pad_to_die_length
    }
}

fn contains(shape: &Shape, p: Point) -> bool {
    shape.collides(&Shape::Circle { c: p, r: Nm::ZERO }, Nm::ZERO).is_some()
}

/// Where the segment from `inside` (in the shape) to `outside` (not) leaves the shape, to a nanometre (`SHAPE::Collide( SEG, 0, .., &loc )`).
fn exit_point(inside_pt: Point, outside: Point, inside: &dyn Fn(Point) -> bool) -> Point {
    let at = |t: f64| Point { x: inside_pt.x + Nm::kiround((outside.x - inside_pt.x).as_f64() * t), y: inside_pt.y + Nm::kiround((outside.y - inside_pt.y).as_f64() * t) };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..48 {
        let mid = (lo + hi) / 2.0;
        if inside(at(mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(hi)
}

/// `clipLineToPad` / `clipLineToVia`: from the end of `line` (`forward`: its start) that is inside, the first vertex outside; the line becomes
/// `center` -> the point where it leaves -> the rest of it. A line wholly inside is left alone.
fn clip_line(line: &ShapeLineChain, center: Point, inside: &dyn Fn(Point) -> bool, forward: bool) -> ShapeLineChain {
    let n = line.point_count() as isize;
    if n < 2 {
        return line.clone();
    }
    let (start, delta) = if forward { (0, 1) } else { (n - 1, -1) };
    let mut v = start + delta;
    while (0..n).contains(&v) {
        let p = line.c_point(v as usize);
        if !inside(p) {
            let prev = line.c_point((v - delta) as usize);
            let cross = exit_point(prev, p, inside);
            let mut out = ShapeLineChain::new();
            if forward {
                out.append_point(center);
                out.append_point(cross);
                out.append_chain(&line.slice(v, -1));
            } else {
                out.append_chain(&line.slice(0, v));
                out.append_point(cross);
                out.append_point(center);
            }
            return out;
        }
        v += delta;
    }
    line.clone() // "All points inside pad, nothing to clip"
}

/// `LENGTH_DELAY_CALCULATION::mergeShapeLineChains`: `secondary` joined to `primary` at its start or its end.
fn merge_chains(primary: &ShapeLineChain, secondary: &ShapeLineChain, at_start: bool) -> ShapeLineChain {
    let pts = secondary.points();
    if at_start {
        // the secondary's points other than the shared one, ending next to the primary's start
        let mut out = ShapeLineChain::new();
        let ordered: Vec<Point> = if pts[0] == primary.c_point(0) { pts.iter().rev().copied().collect() } else { pts.to_vec() };
        for p in &ordered[..ordered.len() - 1] {
            out.append_point(*p);
        }
        out.append_chain(primary);
        out
    } else {
        let mut out = primary.clone();
        let ordered: Vec<Point> = if pts[0] == primary.c_last_point() { pts.to_vec() } else { pts.iter().rev().copied().collect() };
        for p in &ordered[1..] {
            out.append_point(*p);
        }
        out
    }
}

/// `LENGTH_DELAY_CALCULATION::mergeLines`.
fn merge_lines(lines: &mut [LineItem]) {
    let mut at: BTreeMap<Point, BTreeSet<usize>> = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        let (a, b) = (l.line.c_point(0), l.line.c_last_point());
        at.entry(a).or_default().insert(i);
        at.entry(b).or_default().insert(i);
    }
    let remove = |at: &mut BTreeMap<Point, BTreeSet<usize>>, l: &LineItem, i: usize| {
        for p in [l.line.c_point(0), l.line.c_last_point()] {
            if let Some(s) = at.get_mut(&p) {
                s.remove(&i);
            }
        }
    };
    for primary in 0..lines.len() {
        if lines[primary].status != Merge::Unmerged {
            continue;
        }
        let l = lines[primary].clone();
        remove(&mut at, &l, primary);
        lines[primary].status = Merge::InUse;
        loop {
            let mut merged = false;
            for at_start in [true, false] {
                let pos = if at_start { lines[primary].line.c_point(0) } else { lines[primary].line.c_last_point() };
                let Some(set) = at.get(&pos) else { continue };
                if set.len() != 1 {
                    continue;
                }
                let other = *set.iter().next().unwrap();
                // "Don't merge if line is an arc", "Don't merge if lines are on different layers"
                if lines[other].has_arc || lines[other].layer != lines[primary].layer {
                    continue;
                }
                lines[other].status = Merge::Retired;
                let joined = merge_chains(&lines[primary].line, &lines[other].line, at_start);
                lines[primary].line = joined;
                let o = lines[other].clone();
                remove(&mut at, &o, other);
                merged = true;
            }
            if !merged {
                break;
            }
        }
    }
}

/// `CalculateLengthDetails` of the items of `net` on `board` (see the module doc). `copper` is the board's copper layers in stack order,
/// `die` the pad-to-die length of each pad of `board.pads` (0 where none), `height` the stackup height between two copper layers when the
/// board uses it in its lengths (`None` when it does not).
pub(crate) fn length_details(board: &DrcBoard, net: &str, copper: &[String], die: &[Nm], height: Option<&dyn Fn(&str, &str) -> i64>) -> LengthStats {
    let on = |n: &Option<String>| n.as_deref() == Some(net);
    let pads: Vec<usize> = board.pads.iter().enumerate().filter(|(_, p)| on(&p.net)).map(|(i, _)| i).collect();
    let mut lines: Vec<LineItem> = board
        .tracks
        .iter()
        .filter(|t| on(&t.net))
        .map(|t| {
            let mut line = ShapeLineChain::new();
            match t.arc_mid {
                Some(mid) => line.append_arc(&ShapeArc::new(t.a, mid, t.b, 0)),
                None => {
                    line.append_point(t.a);
                    line.append_point(t.b);
                }
            }
            LineItem { has_arc: !line.arcs().is_empty(), line, layer: t.layer.clone(), status: Merge::Unmerged }
        })
        .collect();
    let stack = |l: &str| copper.iter().position(|c| c == l).unwrap_or(usize::MAX);

    // `optimiseVias`, on the lines as they are before the merge.
    let mut vias: Vec<ViaItem> = Vec::new();
    for v in board.vias.iter().filter(|v| on(&v.net)) {
        let radius = v.diameter / 2;
        let circle = Shape::Circle { c: v.at, r: radius };
        let mut span: BTreeSet<String> = BTreeSet::new();
        for l in &lines {
            for end in [l.line.c_point(0), l.line.c_last_point()] {
                if end == v.at || contains(&circle, end) {
                    span.insert(l.layer.clone());
                }
            }
        }
        // `GetConnectedPads( pcbVia )`: the pad the via is in (its copper meets the via's on a layer of the via's)
        let (lo, hi) = { let (a, b) = (stack(&v.from_layer), stack(&v.to_layer)); (a.min(b), a.max(b)) };
        let coincident = pads.iter().map(|&i| &board.pads[i]).find(|p| {
            p.copper_groups().iter().any(|(layers, shape)| layers.iter().any(|l| (lo..=hi).contains(&stack(l))) && shape.collides(&circle, Nm::ZERO).is_some())
        });
        if let Some(p) = coincident {
            // an SMD pad -- and an edge connector pad (`PAD_ATTRIB::CONN`), which the model holds as one -- sits on its copper layer (`CuStack().begin()`)
            let side = if p.kind == PadKind::Smd { p.layers.first().cloned() } else { Some(if p.side == eda_model::ir::Side::Bottom { "B.Cu".to_string() } else { "F.Cu".to_string() }) };
            span.extend(side);
        }
        let mut cu: Vec<String> = span.into_iter().filter(|l| stack(l) != usize::MAX).collect();
        cu.sort_by_key(|l| stack(l));
        let layers = match (cu.first(), cu.last()) {
            (Some(a), Some(b)) => (a.clone(), b.clone()),
            // "Nothing connects to this via": `pcbVia->GetLayer()` twice
            _ => (v.from_layer.clone(), v.from_layer.clone()),
        };
        vias.push(ViaItem { at: v.at, radius, layers });
    }

    merge_lines(&mut lines);

    // "Clip traces inside VIA pads after merging"
    for v in &vias {
        let circle = Shape::Circle { c: v.at, r: v.radius };
        let inside = |p: Point| contains(&circle, p);
        for l in lines.iter_mut().filter(|l| l.status == Merge::InUse) {
            if l.line.point_count() < 2 {
                continue;
            }
            if inside(l.line.c_point(0)) {
                l.line = clip_line(&l.line, v.at, &inside, true);
            } else if inside(l.line.c_last_point()) {
                l.line = clip_line(&l.line, v.at, &inside, false);
            }
        }
    }
    // `optimiseTracesInPads`
    let (first, last) = (copper.first().map(String::as_str).unwrap_or("F.Cu"), copper.last().map(String::as_str).unwrap_or("B.Cu"));
    for &i in &pads {
        let p = &board.pads[i];
        for l in lines.iter_mut().filter(|l| l.status == Merge::InUse) {
            // "Only consider lines which terminate in the pad"
            if l.line.point_count() < 2 || (l.line.c_point(0) != p.center && l.line.c_last_point() != p.center) {
                continue;
            }
            if !p.layers.iter().any(|x| *x == l.layer) || p.flash(&l.layer, first, last) == eda_model::padstack::Flash::Never {
                continue;
            }
            let Some(shape) = p.copper_on(&l.layer) else { continue };
            let inside = |q: Point| contains(shape, q);
            if inside(l.line.c_point(0)) {
                l.line = clip_line(&l.line, p.center, &inside, true);
            } else if inside(l.line.c_last_point()) {
                l.line = clip_line(&l.line, p.center, &inside, false);
            }
        }
    }

    let mut stats = LengthStats { items: pads.len() + lines.len() + vias.len(), ..LengthStats::default() };
    for l in lines.iter().filter(|l| l.status != Merge::Retired) {
        let length = Nm::new(l.line.length());
        stats.track_length += length;
        *stats.layer_lengths.entry(l.layer.clone()).or_default() += length;
    }
    if let Some(height) = height {
        for v in &vias {
            stats.via_length += Nm::new(height(&v.layers.0, &v.layers.1));
            stats.num_vias += 1;
        }
    }
    for &i in &pads {
        stats.pad_to_die_length += die.get(i).copied().unwrap_or(Nm::ZERO);
        stats.num_pads += 1;
    }
    stats
}

/// The `(die_length ..)` a board file gave the pad, in nanometres (`PAD::GetPadToDieLength`): one of the pad's tokens the IR keeps.
fn die_length_of(tokens: &[String]) -> Nm {
    tokens
        .iter()
        .find_map(|t| t.trim().strip_prefix("(die_length").and_then(|r| r.trim().trim_end_matches(')').trim().parse::<f64>().ok()))
        .map(eda_model::units::from_mm)
        .unwrap_or(Nm::ZERO)
}

/// The pad-to-die length of each of `board.pads`: the k-th pad numbered N of a footprint is the k-th pad numbered N of its footprint's pads,
/// whose tokens are the footprint extra's pad at the same index.
fn die_lengths(design: &Design, model: &ConstraintModel, board: &DrcBoard) -> Vec<Nm> {
    let extras = design.drawings.as_ref().map(|d| &d.footprint_extras);
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    board
        .pads
        .iter()
        .map(|p| {
            let k = seen.entry((p.footprint_ref.clone(), p.number.clone())).or_insert(0);
            let nth = *k;
            *k += 1;
            let Some(part) = model.part(&p.footprint_ref) else { return Nm::ZERO };
            let Some(fp) = model.footprint_of(part) else { return Nm::ZERO };
            let Some(extra) = extras.and_then(|e| e.iter().find(|e| e.id == p.footprint_ref)) else { return Nm::ZERO };
            if extra.pads.len() != fp.pads.len() {
                return Nm::ZERO;
            }
            let idx = fp.pads.iter().enumerate().filter(|(_, q)| q.number == p.number).nth(nth).map(|(i, _)| i);
            idx.and_then(|i| extra.pads.get(i)).map(|m| die_length_of(&m.tokens)).unwrap_or(Nm::ZERO)
        })
        .collect()
}

/// `NETCLASS::GetHumanReadableName`: one class by its name; two "A and B", three "A, B and C", more "A, B and N more".
fn human_class_name(constituents: &[String]) -> String {
    match constituents {
        [] => "Default".to_string(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

/// The rows of every net of the board (`calculateNets( .., true )`: the zero-pad nets too, the page filters them).
pub(crate) fn rows(design: &Design, model: &ConstraintModel) -> Vec<Value> {
    let board = eda_drc::board::build(design, model);
    let copper = model.board.layers.clone();
    let die = die_lengths(design, model, &board);
    let stackup = model.stackup.clone().unwrap_or_else(|| eda_model::rules::default_stackup(copper.len(), model.board.board_thickness_nm));
    let height_fn = |a: &str, b: &str| eda_pns::tuning_path::stackup_distance_nm(&stackup, &copper, a, b);
    let height: Option<&dyn Fn(&str, &str) -> i64> = if model.board.use_height_for_length_calcs { Some(&height_fn) } else { None };
    // `BOARD::GetNetInfo()`: every net the board has -- the model's, and any a track, via or zone names that the model has not.
    let mut nets: BTreeSet<String> = model.nets.iter().map(|n| n.name.clone()).collect();
    if let Some(rt) = &design.routing {
        nets.extend(rt.tracks.iter().map(|t| t.net.clone()).chain(rt.vias.iter().map(|v| v.net.clone())).chain(rt.zones.iter().filter(|z| !z.is_rule_area).map(|z| z.net.clone())));
    }
    nets.remove("");
    nets.iter()
        .filter_map(|net| {
            let s = length_details(&board, net, &copper, &die, height);
            // `calculateNets` walks the connectivity items: a net with no track, via or pad is not listed
            if s.items == 0 {
                return None;
            }
            let class = model.board.effective_class(net);
            Some(json!({
                "name": net,
                "netclass": human_class_name(&class.constituents),
                "total_length": s.total(),
                "via_count": s.num_vias,
                "via_length": s.via_length,
                "track_length": s.track_length,
                "die_length": s.pad_to_die_length,
                "pad_count": s.num_pads,
                "layer_lengths": s.layer_lengths,
            }))
        })
        .collect()
}

/// `GET /api/net_inspector`.
pub fn net_inspector(dir: &Path) -> Value {
    match board::load(dir) {
        Ok((_, design, model)) => json!({ "ok": true, "nets": rows(&design, &model) }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

#[cfg(test)]
#[path = "net_inspector_tests.rs"]
mod tests;
