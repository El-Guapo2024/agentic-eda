//! eda-gates — schematic-section geometry gates.
//!
//! `check_schematic` re-derives, from `eda_engine::geometry` (the same
//! module the engine and renderer use), each symbol's box + port layout,
//! and checks the `Design`'s schematic section against it: wires must be
//! orthogonal, on-grid, terminate exactly at a pin's stub tip, and never
//! cut through a symbol's box interior; symbol boxes must not overlap each
//! other. A separate Warn-only check counts wire/wire crossings between
//! different nets, for a future layout scorer.

pub mod pcb;
pub use pcb::{check_placement, check_routing};

use std::collections::BTreeMap;

use eda_engine::geometry;
use eda_layout::{Node as LayoutNode, Point as LPoint, Port};
use eda_model::ir::{Design, Point as IrPoint, SchematicSection, SymbolInstance};
use eda_model::{CheckResult, CheckStatus, ConstraintModel, Part};

fn to_lpoint(p: IrPoint) -> LPoint {
    LPoint { x: p.x, y: p.y }
}

/// A symbol's re-derived box + port geometry, keyed by symbol id.
struct SymGeo<'a> {
    #[allow(dead_code)]
    part: &'a Part,
    top_left: LPoint,
    width: i64,
    height: i64,
    node: LayoutNode,
    /// pin index (in `part.pins`) -> port index, mirroring
    /// `geometry::build_ports`'s own return.
    pin_port: Vec<Option<usize>>,
}

impl<'a> SymGeo<'a> {
    fn build(sym: &SymbolInstance, part: &'a Part) -> Self {
        let (width, height) = geometry::node_size(part.pins.len());
        let (ports, pin_port): (Vec<Port>, _) = geometry::build_ports(part, width, height);
        let node = LayoutNode { id: 0, width, height, ports };
        Self { part, top_left: to_lpoint(sym.at), width, height, node, pin_port }
    }

    fn bounds(&self) -> (i64, i64, i64, i64) {
        (self.top_left.x, self.top_left.x + self.width, self.top_left.y, self.top_left.y + self.height)
    }

    fn stub_tip_for_pin_number(&self, pin_number: &str) -> Option<LPoint> {
        let pin_idx = self.part.pins.iter().position(|p| p.number == pin_number)?;
        let port_idx = self.pin_port.get(pin_idx).copied().flatten()?;
        Some(geometry::stub_tip(&self.node, self.top_left, port_idx))
    }
}

fn build_geos<'a>(sch: &SchematicSection, model: &'a ConstraintModel) -> BTreeMap<String, SymGeo<'a>> {
    let mut geos = BTreeMap::new();
    for sym in &sch.symbols {
        if let Some(part) = model.part(&sym.id) {
            geos.insert(sym.id.clone(), SymGeo::build(sym, part));
        }
    }
    geos
}

fn split_pin_ref(pin_ref: &str) -> (&str, &str) {
    pin_ref.split_once('.').unwrap_or((pin_ref, ""))
}

/// Runs every schematic geometry gate against `design.schematic`, re-using
/// `model` to recompute each symbol's box/port layout the same way the
/// engine and renderer do. Returns one `CheckResult` per finding, plus a
/// `Pass` for any check that found nothing to fail/warn about, and always
/// exactly one `schematic_wire_crossing_count` result (Warn, informational).
pub fn check_schematic(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let mut results = Vec::new();
    let Some(sch) = design.schematic.as_ref() else {
        results.push(CheckResult::fail("schematic_wire_through_symbol", "design", "design has no schematic section"));
        return results;
    };
    let geos = build_geos(sch, model);

    check_offgrid(sch, &mut results);
    check_orthogonal(sch, &mut results);
    check_symbol_overlap(&geos, &mut results);
    check_wire_through_symbol(sch, &geos, &mut results);
    check_endpoint_off_pin(sch, &geos, &mut results);
    check_wire_overlap(sch, &mut results);
    check_crossing_count(sch, &mut results);
    check_text_overlap(sch, model, &mut results);

    results
}

/// Estimated ref/value/net-label text boxes (using the same
/// `geometry::text_bbox` estimator the renderer uses to place them), so this
/// gate and `eda-render`'s placement always agree on what counts as
/// clearance. Only the ref/value/net-label text is modeled here (pin-name
/// text sits tight against the box interior at fixed offsets and is not a
/// realistic collision source in practice).
fn collect_text_boxes(sch: &SchematicSection, model: &ConstraintModel) -> Vec<(String, geometry::TextBox)> {
    use geometry::{text_bbox, HAnchor};
    let mut out = Vec::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let (width, height) = geometry::node_size(part.pins.len());
        let x = sym.at.x as f64 / 1000.0;
        let y = sym.at.y as f64 / 1000.0;
        let w = width as f64 / 1000.0;
        let h = height as f64 / 1000.0;

        // ref: above-left, start-anchored (mirrors eda-render's ref_y=-0.3)
        out.push((format!("{}:ref", sym.id), text_bbox(x, y - 0.3, &sym.id, 1.6, HAnchor::Start)));

        // value: below-left, start-anchored (mirrors eda-render's value_y)
        let value = match (&part.value, &part.mpn) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(m)) => Some(m.clone()),
            (None, None) => None,
        };
        if let Some(val) = value {
            // Mirrors eda-render: value sits at the box's bottom-right
            // (value_x = w+1.6, value_y = h+1.8).
            out.push((format!("{}:value", sym.id), text_bbox(x + w + 1.6, y + h + 1.8, &val, 1.4, HAnchor::Start)));
        }
    }
    for l in &sch.labels {
        let x = l.at.x as f64 / 1000.0;
        let y = l.at.y as f64 / 1000.0;
        let upper = l.net.to_uppercase();
        let (ty, font) = if upper.starts_with("GND") || upper.starts_with("AGND") || upper.starts_with("VSS") {
            (y + 0.5 + 0.5 + 0.8, 1.3) // drop(0.5) + d3(drop+0.5) + ty(d3+0.8), mirroring render's ground-glyph text baseline
        } else if upper.starts_with('V') || upper.starts_with('+') || upper == "VCC" || upper == "VDD" {
            (y - 1.6 - 0.4, 1.3)
        } else {
            (y - 1.8, 1.3)
        };
        out.push((format!("label:{}@{},{}", l.net, l.at.x, l.at.y), text_bbox(x, ty, &l.net, font, HAnchor::Middle)));
    }
    out
}

fn check_text_overlap(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let boxes = collect_text_boxes(sch, model);
    let mut ok = true;
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            if boxes[i].1.overlaps(&boxes[j].1) {
                out.push(CheckResult {
                    check: "schematic_text_overlap".into(),
                    status: CheckStatus::Warn,
                    location: Some(format!("{}/{}", boxes[i].0, boxes[j].0)),
                    hint: Some("estimated text bounding boxes overlap".into()),
                });
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_text_overlap"));
    }
}

const GRID: i64 = geometry::GRID;

fn check_offgrid(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for s in &sch.symbols {
        if s.at.x % GRID != 0 || s.at.y % GRID != 0 {
            out.push(CheckResult::fail("schematic_offgrid", s.id.clone(), format!("symbol position {:?} is not on the {GRID}um grid", s.at)));
            ok = false;
        }
    }
    for w in &sch.wires {
        for p in &w.pts {
            if p.x % GRID != 0 || p.y % GRID != 0 {
                out.push(CheckResult::fail("schematic_offgrid", w.net.clone(), format!("wire point {p:?} is not on the {GRID}um grid")));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_offgrid"));
    }
}

fn check_orthogonal(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            if a.x != b.x && a.y != b.y {
                out.push(CheckResult::fail("schematic_wire_not_orthogonal", w.net.clone(), format!("segment {a:?} -> {b:?} is neither horizontal nor vertical")));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_not_orthogonal"));
    }
}

fn check_symbol_overlap(geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    let ids: Vec<&String> = geos.keys().collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let (li, ri, ti, bi) = geos[ids[i]].bounds();
            let (lj, rj, tj, bj) = geos[ids[j]].bounds();
            let overlap_x = li < rj && lj < ri;
            let overlap_y = ti < bj && tj < bi;
            if overlap_x && overlap_y {
                out.push(CheckResult::fail(
                    "schematic_symbol_overlap",
                    format!("{}/{}", ids[i], ids[j]),
                    "symbol boxes overlap",
                ));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_symbol_overlap"));
    }
}

/// Axis-aligned-aware "does this segment cross the box's open interior"
/// test (diagonal segments — which `schematic_wire_not_orthogonal` already
/// flags separately — fall back to a conservative bounding-box overlap).
fn seg_crosses_box(a: LPoint, b: LPoint, left: i64, right: i64, top: i64, bottom: i64) -> bool {
    if a.y == b.y {
        let y = a.y;
        if y > top && y < bottom {
            let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
            return x1 > left && x0 < right;
        }
        false
    } else if a.x == b.x {
        let x = a.x;
        if x > left && x < right {
            let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
            return y1 > top && y0 < bottom;
        }
        false
    } else {
        let (sx0, sx1) = (a.x.min(b.x), a.x.max(b.x));
        let (sy0, sy1) = (a.y.min(b.y), a.y.max(b.y));
        sx1 > left && sx0 < right && sy1 > top && sy0 < bottom
    }
}

fn check_wire_through_symbol(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            let (a, b) = (to_lpoint(seg[0]), to_lpoint(seg[1]));
            for (id, geo) in geos {
                let (left, right, top, bottom) = geo.bounds();
                if seg_crosses_box(a, b, left, right, top, bottom) {
                    out.push(CheckResult::fail(
                        "schematic_wire_through_symbol",
                        format!("{} through {id}", w.net),
                        format!("segment {a:?} -> {b:?} cuts through {id}'s box"),
                    ));
                    ok = false;
                }
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_through_symbol"));
    }
}

fn check_endpoint_off_pin(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        let (Some(&first), Some(&last)) = (w.pts.first(), w.pts.last()) else { continue };
        let stub_tips: Vec<LPoint> = w
            .pins
            .iter()
            .filter_map(|pin_ref| {
                let (sym_id, pin_num) = split_pin_ref(pin_ref);
                geos.get(sym_id).and_then(|g| g.stub_tip_for_pin_number(pin_num))
            })
            .collect();
        if stub_tips.len() != w.pins.len() {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), "one or more of the wire's pin refs did not resolve to a symbol/pin/port"));
            ok = false;
            continue;
        }
        let first = to_lpoint(first);
        let last = to_lpoint(last);
        if !stub_tips.contains(&first) {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), format!("first point {first:?} is not at any listed pin's stub tip")));
            ok = false;
        }
        if !stub_tips.contains(&last) {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), format!("last point {last:?} is not at any listed pin's stub tip")));
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_endpoint_off_pin"));
    }
}

fn seg_orientation(a: IrPoint, b: IrPoint) -> Option<bool> {
    // Some(true) = horizontal, Some(false) = vertical, None = degenerate/diagonal.
    if a.y == b.y && a.x != b.x {
        Some(true)
    } else if a.x == b.x && a.y != b.y {
        Some(false)
    } else {
        None
    }
}

/// Flags any two wire segments (consecutive-point pairs within different
/// nets' `Wire.pts` polylines) that are collinear (both axis-aligned,
/// sharing the same x or same y) and overlap over a positive-length range
/// (not merely touching at a shared endpoint). Same-net overlaps are
/// explicitly allowed (star hubs/branches legitimately share points or
/// segments).
fn check_wire_overlap(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut segs: Vec<(&str, IrPoint, IrPoint)> = Vec::new();
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            segs.push((w.net.as_str(), seg[0], seg[1]));
        }
    }

    let mut ok = true;
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let (net_i, ai, bi) = segs[i];
            let (net_j, aj, bj) = segs[j];
            if net_i == net_j {
                continue;
            }
            let (Some(oi), Some(oj)) = (seg_orientation(ai, bi), seg_orientation(aj, bj)) else { continue };
            if oi != oj {
                continue; // different orientation: not collinear
            }
            let overlap = if oi {
                // both horizontal: collinear iff same y.
                if ai.y != aj.y {
                    None
                } else {
                    let (x0i, x1i) = (ai.x.min(bi.x), ai.x.max(bi.x));
                    let (x0j, x1j) = (aj.x.min(bj.x), aj.x.max(bj.x));
                    let lo = x0i.max(x0j);
                    let hi = x1i.min(x1j);
                    if hi > lo {
                        Some((ai.y, lo, hi))
                    } else {
                        None
                    }
                }
            } else {
                // both vertical: collinear iff same x.
                if ai.x != aj.x {
                    None
                } else {
                    let (y0i, y1i) = (ai.y.min(bi.y), ai.y.max(bi.y));
                    let (y0j, y1j) = (aj.y.min(bj.y), aj.y.max(bj.y));
                    let lo = y0i.max(y0j);
                    let hi = y1i.min(y1j);
                    if hi > lo {
                        Some((ai.x, lo, hi))
                    } else {
                        None
                    }
                }
            };
            if overlap.is_some() {
                out.push(CheckResult::fail(
                    "schematic_wire_overlap",
                    format!("{net_i}/{net_j}"),
                    format!("segments {ai:?}->{bi:?} ({net_i}) and {aj:?}->{bj:?} ({net_j}) are collinear and overlap over a positive length"),
                ));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_overlap"));
    }
}

fn check_crossing_count(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    // Collect (net, segment) for every wire segment.
    let mut segs: Vec<(&str, IrPoint, IrPoint)> = Vec::new();
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            segs.push((w.net.as_str(), seg[0], seg[1]));
        }
    }

    let mut count = 0usize;
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let (net_i, ai, bi) = segs[i];
            let (net_j, aj, bj) = segs[j];
            if net_i == net_j {
                continue;
            }
            let (Some(oi), Some(oj)) = (seg_orientation(ai, bi), seg_orientation(aj, bj)) else { continue };
            if oi == oj {
                continue; // parallel segments: not counted as a crossing here
            }
            // oi==true means i is horizontal, j is vertical (or vice versa).
            let (h_a, h_b, v_a, v_b) = if oi { (ai, bi, aj, bj) } else { (aj, bj, ai, bi) };
            let hy = h_a.y;
            let (hx0, hx1) = (h_a.x.min(h_b.x), h_a.x.max(h_b.x));
            let vx = v_a.x;
            let (vy0, vy1) = (v_a.y.min(v_b.y), v_a.y.max(v_b.y));
            if vx > hx0 && vx < hx1 && hy > vy0 && hy < vy1 {
                count += 1;
            }
        }
    }

    out.push(CheckResult {
        check: "schematic_wire_crossing_count".into(),
        status: CheckStatus::Warn,
        location: None,
        hint: Some(format!("{count} wire/wire crossing(s) between different nets")),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{NetLabel, Provenance, Wire};
    use eda_model::{Pin, PinKind};

    fn provenance() -> Provenance {
        Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }
    }

    fn design(sch: SchematicSection) -> Design {
        Design { schema: 1, provenance: provenance(), schematic: Some(sch), placement: None, routing: None }
    }

    /// One part: a 2-pin passive with pins at Left(offset 1270)/Right
    /// (offset 1270), so `geometry::node_size(2)` -> width 10160, height
    /// 7620, and both ports land at grid-aligned offsets (see
    /// `geometry::distribute_offsets`).
    fn one_part_model() -> ConstraintModel {
        let r1 = Part {
            reference: "R1".into(),
            mpn: None,
            value: None,
            package: None,
            footprint: None,
            pins: vec![
                Pin { number: "1".into(), name: None, kind: PinKind::Passive },
                Pin { number: "2".into(), name: None, kind: PinKind::Passive },
            ],
        };
        ConstraintModel { parts: vec![r1], ..Default::default() }
    }

    fn r1_geo(model: &ConstraintModel, at: IrPoint) -> SymGeo<'_> {
        let sym = SymbolInstance { id: "R1".into(), at, rot: 0, mirrored: false };
        SymGeo::build(&sym, model.part("R1").unwrap())
    }

    fn r1_symbol(at: IrPoint) -> SymbolInstance {
        SymbolInstance { id: "R1".into(), at, rot: 0, mirrored: false }
    }

    // ---------------------------------------------------------- offgrid

    #[test]
    fn offgrid_passes_when_everything_on_grid() {
        let model = one_part_model();
        let sch = SchematicSection { symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_offgrid" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn offgrid_fails_on_off_grid_symbol() {
        let model = one_part_model();
        let sch = SchematicSection { symbols: vec![r1_symbol(IrPoint { x: 100, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_offgrid" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- orthogonal

    #[test]
    fn orthogonal_passes_for_hv_only_wire() {
        let model = one_part_model();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![Wire { net: "N".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }, IrPoint { x: 1270, y: 1270 }] }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_not_orthogonal" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn orthogonal_fails_for_diagonal_segment() {
        let model = one_part_model();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![Wire { net: "N".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 1270 }] }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_not_orthogonal" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- symbol_overlap

    #[test]
    fn symbol_overlap_passes_when_apart() {
        let model = one_part_model();
        let sch = SchematicSection { symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_symbol_overlap" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn symbol_overlap_fails_when_two_boxes_overlap() {
        let mut model = one_part_model();
        let r2 = Part { reference: "R2".into(), ..model.parts[0].clone() };
        model.parts.push(r2);
        let sch = SchematicSection {
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 }), SymbolInstance { id: "R2".into(), at: IrPoint { x: 1270, y: 0 }, rot: 0, mirrored: false }],
            wires: vec![],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_symbol_overlap" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- wire_through_symbol

    #[test]
    fn wire_through_symbol_passes_when_wire_stays_outside_box() {
        let model = one_part_model();
        let sch = SchematicSection {
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { net: "N".into(), pins: vec![], pts: vec![IrPoint { x: -5000, y: -5000 }, IrPoint { x: -5000, y: -6000 }] }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_through_symbol" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn wire_through_symbol_fails_when_segment_cuts_the_box() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let (left, right, top, bottom) = geo.bounds();
        let mid_y = (top + bottom) / 2;
        let sch = SchematicSection {
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { net: "N".into(), pins: vec![], pts: vec![IrPoint { x: left - 100, y: mid_y }, IrPoint { x: right + 100, y: mid_y }] }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_through_symbol" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- endpoint_off_pin

    #[test]
    fn endpoint_off_pin_passes_when_wire_starts_and_ends_on_stub_tips() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let p1 = geo.stub_tip_for_pin_number("1").unwrap();
        let p2 = geo.stub_tip_for_pin_number("2").unwrap();
        let sch = SchematicSection {
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire {
                net: "N".into(),
                pins: vec!["R1.1".into(), "R1.2".into()],
                pts: vec![IrPoint { x: p1.x, y: p1.y }, IrPoint { x: p2.x, y: p2.y }],
            }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_endpoint_off_pin" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn endpoint_off_pin_fails_when_wire_stops_short_of_the_stub_tip() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let p1 = geo.stub_tip_for_pin_number("1").unwrap();
        let p2 = geo.stub_tip_for_pin_number("2").unwrap();
        let sch = SchematicSection {
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire {
                net: "N".into(),
                pins: vec!["R1.1".into(), "R1.2".into()],
                // first point is on the box boundary, not the stub tip.
                pts: vec![IrPoint { x: p1.x + 1, y: p1.y }, IrPoint { x: p2.x, y: p2.y }],
            }],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_endpoint_off_pin" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- wire_overlap

    #[test]
    fn wire_overlap_passes_for_touching_or_offset_or_crossing_segments() {
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![
                // A: horizontal 0..1270 at y=0
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }] },
                // B: horizontal 1270..2540 at y=0 -- only touches A at a point.
                Wire { net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 2540, y: 0 }] },
                // C: horizontal at y=1270 (offset, parallel but not collinear).
                Wire { net: "C".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 1270 }, IrPoint { x: 1270, y: 1270 }] },
                // D: vertical crossing A, not collinear.
                Wire { net: "D".into(), pins: vec![], pts: vec![IrPoint { x: 600, y: -600 }, IrPoint { x: 600, y: 600 }] },
            ],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn wire_overlap_fails_for_collinear_different_net_overlap() {
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 2540, y: 0 }] },
                // Overlaps A over x in [1270, 2540].
                Wire { net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 3810, y: 0 }] },
            ],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Fail));
    }

    #[test]
    fn wire_overlap_passes_for_same_net_overlap() {
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 2540, y: 0 }] },
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 3810, y: 0 }] },
            ],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass));
    }

    // ---------------------------------------------------------- crossing_count (warn)

    #[test]
    fn crossing_count_zero_when_no_crossings() {
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }] },
                Wire { net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 5080 }, IrPoint { x: 1270, y: 5080 }] },
            ],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        let r = results.iter().find(|r| r.check == "schematic_wire_crossing_count").unwrap();
        assert_eq!(r.status, CheckStatus::Warn);
        assert!(r.hint.as_ref().unwrap().starts_with('0'));
    }

    #[test]
    fn crossing_count_counts_a_true_crossing_between_different_nets() {
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![
                Wire { net: "A".into(), pins: vec![], pts: vec![IrPoint { x: -1270, y: 0 }, IrPoint { x: 1270, y: 0 }] },
                Wire { net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: -1270 }, IrPoint { x: 0, y: 1270 }] },
            ],
            labels: vec![],
        };
        let results = check_schematic(&design(sch), &model);
        let r = results.iter().find(|r| r.check == "schematic_wire_crossing_count").unwrap();
        assert_eq!(r.status, CheckStatus::Warn);
        assert!(r.hint.as_ref().unwrap().starts_with('1'));
    }

    // ---------------------------------------------------------- misc

    #[test]
    fn empty_schematic_section_is_all_pass() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { symbols: vec![], wires: vec![], labels: vec![] };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().filter(|r| r.check != "schematic_wire_crossing_count").all(|r| r.status == CheckStatus::Pass));
    }

    #[test]
    fn net_labels_are_ignored_by_geometry_gates() {
        // Sanity: labels don't participate in any of these checks/panic the
        // gate even when off-grid.
        let model = ConstraintModel::default();
        let sch = SchematicSection {
            symbols: vec![],
            wires: vec![],
            labels: vec![NetLabel { net: "GND".into(), at: IrPoint { x: 3, y: 7 } }],
        };
        let results = check_schematic(&design(sch), &model);
        assert!(results.iter().all(|r| r.check != "schematic_offgrid" || r.status == CheckStatus::Pass));
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Pin, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }

    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, value: None, package: None, footprint: None, pins }
    }

    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// Same LDO fixture as eda-engine's and eda-render's own tests: U1 (LDO)
    /// + CIN + COUT.
    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![
                pin("1", "VIN", PinKind::Power),
                pin("2", "GND", PinKind::Ground),
                pin("3", "EN", PinKind::Signal),
                pin("4", "VOUT", PinKind::Power),
                pin("5", "NC", PinKind::Nc),
            ],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);

        ConstraintModel {
            parts: vec![u1, cin, cout],
            nets: vec![
                net("VIN", &["U1.1", "CIN.1", "U1.3"]),
                net("VOUT", &["U1.4", "COUT.1"]),
                net("GND", &["U1.2", "CIN.2", "COUT.2"]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn ldo_schematic_has_zero_fails() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "test-hash")).unwrap();
        let results = check_schematic(&design, &model);
        let fails: Vec<&CheckResult> = results.iter().filter(|r| r.status == CheckStatus::Fail).collect();
        assert!(fails.is_empty(), "expected zero Fail-status results on the LDO fixture, got: {fails:#?}");
    }

    /// `schematic_wire_overlap` on the LDO fixture: this gate previously
    /// caught a real bug here (GND and VOUT sharing a collinear channel
    /// stagger row, before `eda-layout`'s routing sized channels to fit one
    /// genuinely distinct row per hop) — proven failing at that point in
    /// the same test position this now occupies. With the routing fix
    /// landed, this must now be a clean `Pass`, same as
    /// `ldo_schematic_has_zero_fails` above.
    #[test]
    fn schematic_wire_overlap_passes_on_ldo_after_routing_fix() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "test-hash")).unwrap();
        let results = check_schematic(&design, &model);
        assert!(
            results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass),
            "expected schematic_wire_overlap to pass cleanly on the LDO fixture post-fix, got: {results:#?}"
        );
    }
}

