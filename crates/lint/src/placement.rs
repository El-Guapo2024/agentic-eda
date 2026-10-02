//! Placement-quality checks. None of these has a KiCad equivalent:
//! `placement_proximity`, `placement_decoupling`, `placement_stub_crossings`,
//! `placement_board_use`, `placement_net_compactness`,
//! `placement_edge_connector`, `placement_refdes_clear`. They read only the
//! placement (courtyards, pad centres, the nets and rules in the model),
//! never copper, so they are cheap enough to run on every step of the
//! constructive placer and on every studio refresh.
//!
//! The check names, thresholds, messages and fix hints are the ones
//! `eda_gates::pcb` has always used (the gates filter on the names); this
//! module moved here from the DRC crate unchanged so nothing downstream
//! notices.

use crate::finding::{Check, Finding, FixHint, Item};
use eda_model::footprint::{edge_connector_gap, is_edge_connector};
use eda_model::ir::{Design, Point, Um};
use eda_model::{is_free_two_pin, ConstraintModel, PlacementRule};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy)]
struct Rect(Um, Um, Um, Um);

impl Rect {
    fn gap(&self, o: &Rect) -> f64 {
        let dx = (self.0 - o.2).max(o.0 - self.2).max(0) as f64;
        let dy = (self.1 - o.3).max(o.1 - self.3).max(0) as f64;
        (dx * dx + dy * dy).sqrt()
    }
    fn overlap_area(&self, o: &Rect) -> i128 {
        let w = (self.2.min(o.2) - self.0.max(o.0)).max(0) as i128;
        let h = (self.3.min(o.3) - self.1.max(o.1)).max(0) as i128;
        w * h
    }
}

fn orient(a: Point, b: Point, c: Point) -> i128 {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128
}

/// Max gap, µm, between an edge connector's courtyard and the nearest
/// board edge. The placer keeps a 600 µm routing margin around every
/// courtyard, so a connector it pushes flush sits ~600 µm in.
pub const EDGE_CONNECTOR_MAX_GAP_UM: i64 = 1500;
/// Board-use limits: the union of all courtyards must be centred to
/// within this fraction of the board dimension (|left − right| ≤ f·W) …
pub const BOARD_USE_MAX_IMBALANCE: f64 = 0.15;
/// … and must span at least this fraction of each board dimension.
pub const BOARD_USE_MIN_SPAN: f64 = 0.5;
/// Least share of the board the parts' own courtyards may cover, as a
/// fraction of `solver.fit_board_utilization`. Span and imbalance only ask
/// whether the parts are centred and spread; a board three times larger
/// than it needs passes both, because shelf packing spreads parts right
/// across it. This asks the remaining question -- whether the board was
/// actually fitted to the parts -- and only where the intent asked for a
/// fit. A board whose outline is fixed by an enclosure sets
/// `fit_board_utilization: 0` and is exempt.
pub const BOARD_USE_MIN_DENSITY_FRACTION: f64 = 0.5;
/// See [`eda_model::DECOUPLING_MAX_GAP_UM`].
pub const DECOUPLING_MAX_GAP_UM: i64 = eda_model::DECOUPLING_MAX_GAP_UM;
/// A net's pad bounding-box half-perimeter may not exceed this multiple of
/// its lower bound, `2·sqrt(Σ member courtyard areas)` — roughly the HPWL
/// the net would have with its members packed touching.
pub const NET_COMPACTNESS_MAX_RATIO: f64 = 1.6;
/// Nets whose HPWL is below this, µm, are never called spread out.
pub const NET_COMPACTNESS_FLOOR_UM: i64 = 6000;
/// Nets with more members than this (GND, rails) are plane/fill nets that
/// legitimately span the board; compactness is not judged on them. Nor is
/// it judged on nets touching an edge connector: where those run is set
/// by which edge the connector took (`placement_edge_connector`).
pub const NET_COMPACTNESS_MAX_MEMBERS: usize = 6;
/// Max number of crossing pairs of 2-pin nets (straight pad-to-pad
/// stubs), as a fraction of the 2-pin net count, counting only pairs
/// where *both* stubs end on a free 2-pin part (R/C/D/L…) — the crossing
/// a reviewer sees as "swap those two parts", which trading the two free
/// parts' places always removes. A stub between an IC and a connector, or
/// to a pin on the far column of an IC, may have no crossing-free spot
/// left once the IC's decoupling ring is placed; that is a two-layer
/// routing matter, not a placement defect.
pub const STUB_CROSSING_MAX_RATIO: f64 = 0.0;

fn item(desc: impl Into<String>, pos: Point, id: impl Into<String>) -> Item {
    Item { description: desc.into(), pos: (pos.x, pos.y), id: id.into() }
}
fn center(r: &Rect) -> Point {
    Point { x: (r.0 + r.2) / 2, y: (r.1 + r.3) / 2 }
}

/// Every placement-quality check over `design`/`model`. Only the
/// placement section is read, so a routed design costs no more than a
/// placement-only one.
pub fn check(design: &Design, model: &ConstraintModel) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(pl) = design.placement.as_ref() else { return out };
    if pl.outline.len() < 3 {
        return out;
    }

    let mut courtyards: BTreeMap<String, (Rect, eda_model::ir::Side)> = BTreeMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        if let Some((x0, y0, x1, y1)) = eda_model::footprint::placed_courtyard(model, part, fp) {
            courtyards.insert(fp.id.clone(), (Rect(x0, y0, x1, y1), fp.side));
        }
    }
    let bare: BTreeMap<String, Rect> = courtyards.iter().map(|(k, (r, _))| (k.clone(), *r)).collect();
    let bb = (
        pl.outline.iter().map(|p| p.x).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.x).max().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).max().unwrap_or(0),
    );

    proximity(model, &courtyards, &mut out);
    refdes_clear(model, pl, &courtyards, &mut out);
    edge_connector(model, &bare, bb, &mut out);
    board_use(&bare, bb, model.solver.fit_board_utilization, &mut out);
    decoupling(model, &bare, &mut out);
    net_compactness(pl, model, &bare, &mut out);
    stub_crossings(pl, model, &mut out);

    out
}

fn proximity(model: &ConstraintModel, courtyards: &BTreeMap<String, (Rect, eda_model::ir::Side)>, out: &mut Vec<Finding>) {
    for rule in &model.placement_rules {
        let PlacementRule::Proximity { a, b, max_mm, reason } = rule else { continue };
        let (Some((ra, _)), Some((rb, _))) = (courtyards.get(a), courtyards.get(b)) else { continue };
        let d = ra.gap(rb) / 1000.0;
        if d > *max_mm {
            let over = d - *max_mm;
            let suggest = if over < 0.5 {
                "near miss: the placer stopped short of the rule. Raise solver.place_moves_per_part, try another seed, or relax the rule's max_mm"
            } else {
                "far miss: the pair cannot get closer under the other rules. Move one of the two parts in the intent, drop what sits between them, or relax the rule's max_mm"
            };
            let reason_suffix = reason.as_deref().map(|r| format!("; {r}")).unwrap_or_default();
            let v = Finding::new(Check::PlacementProximity, format!("{d:.2} mm apart, rule allows {max_mm} mm{reason_suffix}"), vec![item(a, center(ra), a.clone()), item(b, center(rb), b.clone())]);
            out.push(v.with_fix(FixHint { mover: b.clone(), toward: a.clone(), distance_to_close_um: (over * 1000.0).round() as Um, suggested_command: suggest.into() }));
        }
    }
}

fn refdes_clear(model: &ConstraintModel, pl: &eda_model::ir::PlacementSection, courtyards: &BTreeMap<String, (Rect, eda_model::ir::Side)>, out: &mut Vec<Finding>) {
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some((x0, y0, x1, y1)) = eda_model::footprint::placed_refdes_box(model, &pl.outline, part, fp) else { continue };
        let bx = Rect(x0, y0, x1, y1);
        for (id, (r, s)) in courtyards {
            if *id == fp.id || *s != fp.side {
                continue;
            }
            let ov = bx.overlap_area(r);
            if ov > 0 {
                out.push(Finding::new(
                    Check::PlacementRefdesClear,
                    format!("refdes label of {} overlaps the courtyard of {} by {} \u{b5}m\u{b2}", fp.id, id, ov),
                    vec![item(format!("Reference of {}", fp.id), center(&bx), format!("{}.ref", fp.id)), item(format!("Footprint {id}"), center(r), id.clone())],
                ));
            }
        }
    }
}

fn edge_connector(model: &ConstraintModel, courtyards: &BTreeMap<String, Rect>, bb: (Um, Um, Um, Um), out: &mut Vec<Finding>) {
    for part in &model.parts {
        if !is_edge_connector(part) {
            continue;
        }
        let Some(r) = courtyards.get(&part.reference) else { continue };
        let gap = edge_connector_gap((r.0, r.1, r.2, r.3), bb);
        if gap > EDGE_CONNECTOR_MAX_GAP_UM {
            let v = Finding::new(
                Check::PlacementEdgeConnector,
                format!("connector courtyard is {gap} \u{b5}m from the nearest board edge (max {EDGE_CONNECTOR_MAX_GAP_UM} \u{b5}m)"),
                vec![item(&part.reference, center(r), part.reference.clone())],
            );
            out.push(v.with_fix(FixHint {
                mover: part.reference.clone(),
                toward: "nearest board edge".into(),
                distance_to_close_um: gap - EDGE_CONNECTOR_MAX_GAP_UM,
                suggested_command: "move the connector to the board edge it faces".into(),
            }));
        }
    }
}

fn board_use(courtyards: &BTreeMap<String, Rect>, bb: (Um, Um, Um, Um), fit_target: f64, out: &mut Vec<Finding>) {
    if courtyards.len() < 2 {
        return;
    }
    let u = (
        courtyards.values().map(|r| r.0).min().unwrap(),
        courtyards.values().map(|r| r.1).min().unwrap(),
        courtyards.values().map(|r| r.2).max().unwrap(),
        courtyards.values().map(|r| r.3).max().unwrap(),
    );
    let (w, h) = ((bb.2 - bb.0).max(1) as f64, (bb.3 - bb.1).max(1) as f64);
    let (l, r, t, b) = ((u.0 - bb.0) as f64, (bb.2 - u.2) as f64, (u.1 - bb.1) as f64, (bb.3 - u.3) as f64);
    let (imb_x, imb_y) = ((l - r).abs() / w, (t - b).abs() / h);
    let (span_x, span_y) = ((u.2 - u.0) as f64 / w, (u.3 - u.1) as f64 / h);
    let board_center = Point { x: (bb.0 + bb.2) / 2, y: (bb.1 + bb.3) / 2 };
    if imb_x > BOARD_USE_MAX_IMBALANCE || imb_y > BOARD_USE_MAX_IMBALANCE {
        out.push(Finding::new(
            Check::PlacementBoardUse,
            format!("parts are off-centre: margins left {l:.0}/right {r:.0}, top {t:.0}/bottom {b:.0} \u{b5}m (imbalance x {imb_x:.2}, y {imb_y:.2}; max {BOARD_USE_MAX_IMBALANCE})"),
            vec![item("board", board_center, "board")],
        ));
    }
    if fit_target > 0.0 {
        let parts_area: f64 = courtyards.values().map(|r| ((r.2 - r.0) as f64) * ((r.3 - r.1) as f64)).sum();
        let density = parts_area / (w * h);
        let floor = fit_target * BOARD_USE_MIN_DENSITY_FRACTION;
        if density < floor {
            out.push(Finding::new(
                Check::PlacementBoardUse,
                format!("parts cover {:.0}% of the board; the intent asked it fitted to {:.0}%, so anything under {:.0}% is board nobody needs", density * 100.0, fit_target * 100.0, floor * 100.0),
                vec![item("board", board_center, "board")],
            ));
        }
    }
    if span_x < BOARD_USE_MIN_SPAN || span_y < BOARD_USE_MIN_SPAN {
        out.push(Finding::new(
            Check::PlacementBoardUse,
            format!("parts span only {span_x:.2} \u{d7} {span_y:.2} of the board (min {BOARD_USE_MIN_SPAN} each way)"),
            vec![item("board", board_center, "board")],
        ));
    }
}

fn decoupling(model: &ConstraintModel, courtyards: &BTreeMap<String, Rect>, out: &mut Vec<Finding>) {
    let pairs = eda_model::decoupling_pairs(model);
    let caps: std::collections::BTreeSet<&str> = pairs.iter().map(|(c, _)| c.as_str()).collect();
    let ruled = |c: &str| model.placement_rules.iter().any(|r| matches!(r, PlacementRule::Proximity { a, b, .. } if a == c || b == c));
    for c in caps {
        if ruled(c) {
            continue;
        }
        let Some(rc) = courtyards.get(c) else { continue };
        let mut best: Option<(f64, &str)> = None;
        for (_, u) in pairs.iter().filter(|(cc, _)| cc == c) {
            let Some(ru) = courtyards.get(u.as_str()) else { continue };
            let d = rc.gap(ru);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, u));
            }
        }
        if let Some((d, u)) = best {
            if d > DECOUPLING_MAX_GAP_UM as f64 {
                let ru = courtyards[u];
                let v = Finding::new(
                    Check::PlacementDecoupling,
                    format!("decoupling capacitor is {d:.0} \u{b5}m from the IC it decouples (max {DECOUPLING_MAX_GAP_UM} \u{b5}m)"),
                    vec![item(c, center(rc), c.to_string()), item(u, center(&ru), u.to_string())],
                );
                out.push(v.with_fix(FixHint { mover: c.to_string(), toward: u.to_string(), distance_to_close_um: (d - DECOUPLING_MAX_GAP_UM as f64).round() as Um, suggested_command: format!("move {c} next to {u}") }));
            }
        }
    }
}

fn net_compactness(pl: &eda_model::ir::PlacementSection, model: &ConstraintModel, courtyards: &BTreeMap<String, Rect>, out: &mut Vec<Finding>) {
    let mut centers: HashMap<String, Point> = HashMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        if let Some(pads) = eda_model::footprint::placed_pads(model, part, fp) {
            for pad in pads {
                centers.insert(format!("{}.{}", fp.id, pad.number), pad.center);
            }
        }
    }
    for net in &model.nets {
        let pts: Vec<Point> = net.pins.iter().filter_map(|p| centers.get(p).copied()).collect();
        let members: std::collections::BTreeSet<&str> = net.pins.iter().filter_map(|p| p.split_once('.').map(|(r, _)| r)).collect();
        if pts.len() < 2 || members.len() < 2 || members.len() > NET_COMPACTNESS_MAX_MEMBERS {
            continue;
        }
        if members.iter().any(|m| model.part(m).is_some_and(is_edge_connector)) {
            continue;
        }
        let ics = members.iter().filter(|m| m.starts_with('U')).count();
        if ics > 1 {
            continue;
        }
        let touches_edge = |part: &str| {
            model
                .nets
                .iter()
                .filter(|n| n.pins.iter().any(|p| p.split_once('.').is_some_and(|(r, _)| r == part)))
                .any(|n| n.pins.iter().filter_map(|p| p.split_once('.').map(|(r, _)| r)).any(|r| model.part(r).is_some_and(is_edge_connector)))
        };
        if members.iter().any(|m| !m.starts_with('U') && touches_edge(m)) {
            continue;
        }
        let hpwl = (pts.iter().map(|p| p.x).max().unwrap() - pts.iter().map(|p| p.x).min().unwrap()) + (pts.iter().map(|p| p.y).max().unwrap() - pts.iter().map(|p| p.y).min().unwrap());
        let area: f64 = members.iter().filter_map(|m| courtyards.get(*m)).map(|r| ((r.2 - r.0) as f64) * ((r.3 - r.1) as f64)).sum();
        let bound = 2.0 * area.sqrt();
        let ratio = hpwl as f64 / bound.max(1.0);
        if hpwl > NET_COMPACTNESS_FLOOR_UM && ratio > NET_COMPACTNESS_MAX_RATIO {
            let anchor = pts[0];
            out.push(Finding::new(
                Check::PlacementNetCompactness,
                format!("net spans {hpwl} \u{b5}m HPWL over {} parts, {ratio:.2}x its packed bound {bound:.0} \u{b5}m (max {NET_COMPACTNESS_MAX_RATIO}x)", members.len()),
                vec![item(&net.name, anchor, net.name.clone())],
            ));
        }
    }
}

fn stub_crossings(pl: &eda_model::ir::PlacementSection, model: &ConstraintModel, out: &mut Vec<Finding>) {
    let mut centers: HashMap<String, Point> = HashMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        if let Some(pads) = eda_model::footprint::placed_pads(model, part, fp) {
            for pad in pads {
                centers.insert(format!("{}.{}", fp.id, pad.number), pad.center);
            }
        }
    }
    let mut stubs: Vec<(&str, Point, Point, bool)> = Vec::new();
    for net in &model.nets {
        if net.pins.len() != 2 {
            continue;
        }
        let (Some(ra), Some(rb)) = (net.pins[0].split_once('.'), net.pins[1].split_once('.')) else { continue };
        if ra.0 == rb.0 {
            continue;
        }
        let (Some(&a), Some(&b)) = (centers.get(&net.pins[0]), centers.get(&net.pins[1])) else { continue };
        let free = [ra.0, rb.0].iter().any(|r| model.part(r).is_some_and(is_free_two_pin));
        stubs.push((net.name.as_str(), a, b, free));
    }
    let mut crossings = Vec::new();
    for i in 0..stubs.len() {
        for j in i + 1..stubs.len() {
            let (s, t) = (&stubs[i], &stubs[j]);
            if !(s.3 && t.3) {
                continue;
            }
            let o = [orient(s.1, s.2, t.1).signum(), orient(s.1, s.2, t.2).signum(), orient(t.1, t.2, s.1).signum(), orient(t.1, t.2, s.2).signum()];
            if o.iter().all(|v| *v != 0) && o[0] != o[1] && o[2] != o[3] {
                crossings.push((s.0, t.0, s.1));
            }
        }
    }
    let allowed = (STUB_CROSSING_MAX_RATIO * stubs.len() as f64).floor() as usize;
    if crossings.len() > allowed {
        let names = crossings.iter().map(|(a, b, _)| format!("{a}\u{d7}{b}")).collect::<Vec<_>>().join(",");
        let anchor = crossings.first().map(|(_, _, p)| *p).unwrap_or(Point { x: 0, y: 0 });
        out.push(Finding::new(
            Check::PlacementStubCrossings,
            format!("{} crossing pair(s) of 2-pin net stubs among {} nets (max {allowed})", crossings.len(), stubs.len()),
            vec![item(names.clone(), anchor, names)],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, PlacementSection, Provenance, Side};
    use eda_model::{Part, Pin, PinKind};

    fn part(r: &str) -> Part {
        Part { reference: r.into(), mpn: None, lcsc: None, datasheet: None, symbol: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None }
    }

    fn fp(id: &str, x: Um, y: Um) -> FootprintInstance {
        FootprintInstance { id: id.into(), at: Point { x, y }, rot: 0, side: Side::Top, label: Default::default() }
    }

    fn design(footprints: Vec<FootprintInstance>) -> Design {
        Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 50_000, y: 0 }, Point { x: 50_000, y: 50_000 }, Point { x: 0, y: 50_000 }], footprints, modules: vec![] }),
            routing: None,
            drawings: None,
        }
    }

    #[test]
    fn proximity_violation_carries_a_fix_hint() {
        let model = ConstraintModel {
            parts: vec![part("U1"), part("C1")],
            placement_rules: vec![PlacementRule::Proximity { a: "U1".into(), b: "C1".into(), max_mm: 3.0, reason: None }],
            ..Default::default()
        };
        let d = design(vec![fp("U1", 5_000, 5_000), fp("C1", 20_000, 5_000)]); // 15mm apart, way over 3mm
        let v = check(&d, &model);
        let hit = v.iter().find(|x| x.check == "placement_proximity").expect("proximity violation");
        let fix = hit.fix.as_ref().expect("fix hint");
        assert_eq!(fix.mover, "C1");
        assert_eq!(fix.toward, "U1");
        assert!(fix.distance_to_close_um > 0);
    }

    #[test]
    fn proximity_within_range_is_clean() {
        let model = ConstraintModel {
            parts: vec![part("U1"), part("C1")],
            placement_rules: vec![PlacementRule::Proximity { a: "U1".into(), b: "C1".into(), max_mm: 30.0, reason: None }],
            ..Default::default()
        };
        let d = design(vec![fp("U1", 5_000, 5_000), fp("C1", 6_000, 5_000)]);
        let v = check(&d, &model);
        assert!(v.iter().all(|x| x.check != "placement_proximity"), "{v:#?}");
    }
}
