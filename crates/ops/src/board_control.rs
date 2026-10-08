//! Board control: the board-wide verbs of KiCad's `BOARD_EDITOR_CONTROL`
//! (`pcbnew/tools/board_editor_control.cpp`) that edit `design.json`: merging
//! zones, a zone's fill priority, the drill/place file origin and Repair
//! Board. Each is a pure function over the design; [`crate::Board::apply`]
//! calls them for `Cmd::MergeZones`, `Cmd::SetZonePriority`,
//! `Cmd::SetAuxOrigin` and `Cmd::RepairBoard`, so each lands as one undo step.
//!
//! What is ported, and where this differs from the source:
//!  * `ZoneMerge` / `mergeZones` / `BOARD::TestZoneIntersection`: the same
//!    filters (same net, same rule-area-ness, same layer), the same
//!    "intersects an already chosen zone" test (a touching edge or a corner
//!    inside), the same refusal when the union is more than one outline
//!    ("insufficient overlap"), the highest priority wins. A zone here is one
//!    ring on one layer, so a union with a hole is stored fractured (a slit
//!    ring), as `Cmd::ZoneCutout` stores one.
//!  * `ZonePriority{MoveToTop,Raise,Lower,MoveToBottom}`: the overlap test
//!    (`getOverlappingZones`), the priority map and the cascade search
//!    (`findCascadeZones`) as written; a move that would change nothing is
//!    an error here ("nothing to do"), where KiCad quietly does nothing, so an
//!    empty undo step is never pushed.
//!  * `RepairBoard`: duplicate item ids get new ones (`RepairDuplicateItemUuids`)
//!    and a net an item uses but the netlist lacks is added back
//!    ("Orphaned net re-parented"). The report lines are KiCad's.

use eda_clipper2::Point64;
use eda_model::ir::{Design, Point, Zone};
use eda_model::{CheckResult, Net};
use eda_shape_poly_set::ShapePolySet;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, location: &str, hint: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, hint)]
}

// ------------------------------------------------------------------ geometry

fn orient(a: Point, b: Point, c: Point) -> i128 {
    (b.x as i128 - a.x as i128) * (c.y as i128 - a.y as i128) - (b.y as i128 - a.y as i128) * (c.x as i128 - a.x as i128)
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// `SEG::Collide( aSeg, 0 )`: the segments share a point (a touch counts).
fn segments_touch(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let (o1, o2, o3, o4) = (orient(p1, p2, q1), orient(p1, p2, q2), orient(q1, q2, p1), orient(q1, q2, p2));
    if ((o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)) && ((o3 > 0 && o4 < 0) || (o3 < 0 && o4 > 0)) {
        return true;
    }
    (o1 == 0 && on_segment(p1, p2, q1)) || (o2 == 0 && on_segment(p1, p2, q2)) || (o3 == 0 && on_segment(q1, q2, p1)) || (o4 == 0 && on_segment(q1, q2, p2))
}

/// `SHAPE_LINE_CHAIN::PointInside`: even-odd crossing, a point on an edge counts as inside.
fn ring_contains(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if orient(a, b, p) == 0 && on_segment(a, b, p) {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) {
            // The crossing's x against p.x, without division: sign of (b.x-a.x)*(p.y-a.y) - (p.x-a.x)*(b.y-a.y), flipped by the edge's direction.
            let side = orient(a, b, p);
            if (b.y > a.y && side > 0) || (b.y < a.y && side < 0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn bbox(ring: &[Point]) -> (i64, i64, i64, i64) {
    ring.iter().fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)))
}

fn bboxes_meet(a: &[Point], b: &[Point]) -> bool {
    let (a, b) = (bbox(a), bbox(b));
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn edges_touch(a: &[Point], b: &[Point]) -> bool {
    (0..a.len()).any(|i| {
        let (p1, p2) = (a[i], a[(i + 1) % a.len()]);
        (0..b.len()).any(|j| segments_touch(p1, p2, b[j], b[(j + 1) % b.len()]))
    })
}

/// `BOARD::TestZoneIntersection`: same layer, bounding boxes meet, and either
/// an edge of one touches an edge of the other or a corner of one is inside
/// the other.
pub fn zones_intersect(a: &Zone, b: &Zone) -> bool {
    a.layer == b.layer && a.outline.len() >= 3 && b.outline.len() >= 3 && bboxes_meet(&a.outline, &b.outline) && (edges_touch(&a.outline, &b.outline) || b.outline.iter().any(|p| ring_contains(&a.outline, *p)) || a.outline.iter().any(|p| ring_contains(&b.outline, *p)))
}

// --------------------------------------------------------------------- merge

/// `ZoneMerge`: the zones among `ids` (in that order) that can join the first one, merged into it. Returns the id of
/// the zone that carries the result (the first zone, which keeps its settings, with the highest priority of them all).
/// A selection of fewer than two zones, or a union that is not one outline, is refused with KiCad's message.
pub fn merge_zones(design: &mut Design, ids: &[String]) -> Result<String, Vec<CheckResult>> {
    let zones: &mut Vec<Zone> = &mut design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", "zones", "the board has no zones"))?.zones;
    // The selection's zones, in order; anything that is not a zone is ignored, as in the source.
    let chosen: Vec<usize> = ids.iter().filter_map(|id| zones.iter().position(|z| &z.id == id)).collect();
    if chosen.len() < 2 {
        return Err(fail("ops_zone_merge", "zones", "select two or more zones to merge"));
    }
    let first = chosen[0];
    let mut to_merge: Vec<usize> = Vec::new();
    for &c in &chosen {
        let zone = &zones[c];
        // wxLogMessage'd and skipped in the source: a different net, rule-area-ness or layer, or no overlap.
        if zone.net != zones[first].net || zone.is_rule_area != zones[first].is_rule_area || zone.layer != zones[first].layer {
            continue;
        }
        let meets = c == first || to_merge.iter().any(|&m| zones_intersect(zone, &zones[m]));
        if meets && !to_merge.contains(&c) {
            to_merge.push(c);
        }
    }
    if to_merge.len() < 2 {
        return Err(fail("ops_zone_merge", &zones[first].id, "the selected zones do not touch, so there is nothing to merge"));
    }

    let chain = |z: &Zone| z.outline.iter().map(|p| Point64::new(p.x, p.y)).collect::<Vec<_>>();
    let mut union = ShapePolySet::from_outline(chain(&zones[to_merge[0]]));
    for &m in &to_merge[1..] {
        union.boolean_add(&ShapePolySet::from_outline(chain(&zones[m])));
    }
    union.simplify();
    // One polygon, possibly with holes: two means the overlap was a point, or inside a hole.
    if union.outline_count() != 1 {
        return Err(fail("ops_zone_merge", &zones[first].id, "Zones have insufficient overlap for merging."));
    }
    union.fracture(false);
    let ring: Vec<Point> = union.outline(0).iter().map(|p| Point { x: p.x, y: p.y }).collect();
    if ring.len() < 3 {
        return Err(fail("ops_zone_merge", &zones[first].id, "Zones have insufficient overlap for merging."));
    }

    // The most aggressive fill ordering of the lot survives.
    let priority = to_merge.iter().map(|&m| zones[m].priority).max().unwrap_or(0);
    let keep = zones[first].id.clone();
    zones[first].outline = ring;
    zones[first].priority = priority;
    let drop: BTreeSet<usize> = to_merge.iter().copied().filter(|&m| m != first).collect();
    let mut at = 0;
    zones.retain(|_| {
        let keep_it = !drop.contains(&at);
        at += 1;
        keep_it
    });
    Ok(keep)
}

// ------------------------------------------------------------------ priority

/// Which way `Cmd::SetZonePriority` moves a zone among the zones it overlaps
/// (`pcbnew.EditorControl.zonePriority{MoveToTop,Raise,Lower,MoveToBottom}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZonePriorityMove {
    Top,
    Raise,
    Lower,
    Bottom,
}

/// `getOverlappingZones`: the other copper zones on this zone's layer whose outline touches it or holds it (or is held by it).
pub fn overlapping_zones(zones: &[Zone], zi: usize) -> Vec<usize> {
    let zone = &zones[zi];
    (0..zones.len())
        .filter(|&ci| {
            let c = &zones[ci];
            ci != zi
                && !c.is_rule_area
                && !c.teardrop
                && c.layer == zone.layer
                && c.outline.len() >= 3
                && zone.outline.len() >= 3
                && bboxes_meet(&zone.outline, &c.outline)
                && (edges_touch(&zone.outline, &c.outline) || ring_contains(&zone.outline, c.outline[0]) || ring_contains(&c.outline, zone.outline[0]))
        })
        .collect()
}

/// `buildPriorityMap`: every copper zone but `exclude`, by priority.
fn priority_map(zones: &[Zone], exclude: usize) -> BTreeMap<u32, Vec<usize>> {
    let mut by: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, z) in zones.iter().enumerate() {
        if i != exclude && !z.is_rule_area && !z.teardrop {
            by.entry(z.priority).or_default().push(i);
        }
    }
    by
}

/// `findCascadeZones`: from `from`, walk up (or down) collecting the zones at each consecutive occupied priority until a
/// free one. `viable` is false when the walk runs into 0 (or `u32::MAX`) still occupied: the shift would overflow.
fn cascade(by: &BTreeMap<u32, Vec<usize>>, from: u32, up: bool) -> (Vec<usize>, bool) {
    let (mut found, mut p) = (Vec::new(), from);
    while let Some(zs) = by.get(&p) {
        found.extend(zs);
        if up {
            if p == u32::MAX {
                return (found, false);
            }
            p += 1;
        } else {
            if p == 0 {
                return (found, false);
            }
            p -= 1;
        }
    }
    (found, true)
}

/// The four moves. The zone must be a copper zone (not a rule area or a teardrop).
pub fn set_zone_priority(design: &mut Design, id: &str, to: ZonePriorityMove) -> Result<(), Vec<CheckResult>> {
    let zones: &mut Vec<Zone> = &mut design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", id, "the board has no zones"))?.zones;
    let zi = zones.iter().position(|z| z.id == id).ok_or_else(|| fail("ops_unknown_zone", id, "no zone with this id"))?;
    if zones[zi].is_rule_area || zones[zi].teardrop {
        return Err(fail("ops_zone_priority", id, "only a copper zone has a fill priority"));
    }
    let nothing = |what: &str| fail("ops_zone_priority", id, what);
    let overlapping = overlapping_zones(zones, zi);
    let mine = zones[zi].priority;
    match to {
        ZonePriorityMove::Top => {
            let max = overlapping.iter().map(|&o| zones[o].priority).fold(mine, u32::max);
            if mine >= max {
                return Err(nothing("this zone is already above every zone it overlaps"));
            }
            let by = priority_map(zones, zi);
            // A: take the top slot and push the zones there down; B: take the one above it and push up. Fewer displaced wins.
            let (down, down_ok) = cascade(&by, max, false);
            let (up, up_ok) = if max < u32::MAX { cascade(&by, max + 1, true) } else { (Vec::new(), false) };
            if !down_ok && !up_ok {
                return Err(nothing("no free priority to move this zone to the top"));
            }
            if down_ok && (!up_ok || down.len() <= up.len()) {
                zones[zi].priority = max;
                down.iter().for_each(|&z| zones[z].priority -= 1);
            } else {
                zones[zi].priority = max + 1;
                up.iter().for_each(|&z| zones[z].priority += 1);
            }
        }
        ZonePriorityMove::Raise => {
            // The overlapping zone with the lowest priority still above ours.
            let target = overlapping.iter().copied().filter(|&o| zones[o].priority > mine).min_by_key(|&o| zones[o].priority).ok_or_else(|| nothing("this zone is already above every zone it overlaps"))?;
            let above = zones[target].priority;
            if above < u32::MAX {
                zones[zi].priority = above + 1;
            } else {
                zones[zi].priority = u32::MAX;
                zones[target].priority = mine;
            }
        }
        ZonePriorityMove::Lower => {
            let target = overlapping.iter().copied().filter(|&o| zones[o].priority < mine).max_by_key(|&o| zones[o].priority).ok_or_else(|| nothing("this zone is already below every zone it overlaps"))?;
            let below = zones[target].priority;
            if below > 0 {
                zones[zi].priority = below - 1;
            } else {
                zones[zi].priority = 0;
                zones[target].priority = mine;
            }
        }
        ZonePriorityMove::Bottom => {
            let min = overlapping.iter().map(|&o| zones[o].priority).fold(mine, u32::min);
            if mine <= min {
                return Err(nothing("this zone is already below every zone it overlaps"));
            }
            let by = priority_map(zones, zi);
            // A: take the bottom slot and push the zones there up; B: take the one below it and push down.
            let (up, up_ok) = cascade(&by, min, true);
            let (down, down_ok) = if min > 0 { cascade(&by, min - 1, false) } else { (Vec::new(), false) };
            if !up_ok && !down_ok {
                return Err(nothing("no free priority to move this zone to the bottom"));
            }
            if up_ok && (!down_ok || up.len() <= down.len()) {
                zones[zi].priority = min;
                up.iter().for_each(|&z| zones[z].priority += 1);
            } else {
                zones[zi].priority = min - 1;
                down.iter().for_each(|&z| zones[z].priority -= 1);
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------- origin

/// `DoSetDrillOrigin`: the drill/place file origin; `None` resets it to (0, 0), `DrillOrigin`'s reset branch.
pub fn set_aux_origin(design: &mut Design, at: Option<Point>) -> Result<(), Vec<CheckResult>> {
    let at = at.filter(|p| p.x != 0 || p.y != 0);
    let current = design.drawings.as_ref().and_then(|d| d.aux_origin);
    if current == at {
        return Err(fail("ops_aux_origin", "aux_origin", if at.is_some() { "the drill/place file origin is already there" } else { "the drill/place file origin is already at (0, 0)" }));
    }
    design.drawings.get_or_insert_with(Default::default).aux_origin = at;
    Ok(())
}

/// `PCB_CONTROL::DoSetGridOrigin`: the point the editing grid is anchored at; `None` (or (0, 0)) is `GridResetOrigin`'s reset.
pub fn set_grid_origin(design: &mut Design, at: Option<Point>) -> Result<(), Vec<CheckResult>> {
    let at = at.filter(|p| p.x != 0 || p.y != 0);
    let current = design.drawings.as_ref().and_then(|d| d.grid_origin);
    if current == at {
        return Err(fail("ops_grid_origin", "grid_origin", if at.is_some() { "the grid origin is already there" } else { "the grid origin is already at (0, 0)" }));
    }
    design.drawings.get_or_insert_with(Default::default).grid_origin = at;
    Ok(())
}

// -------------------------------------------------------------------- repair

/// What `RepairBoard` did, in KiCad's words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// How many problems were repaired ("%d potential problems repaired.").
    pub repaired: usize,
    /// The detail lines ("%d duplicate IDs replaced.", "Orphaned net %s re-parented.").
    pub details: Vec<String>,
}

/// `BOARD_EDITOR_CONTROL::RepairBoard`, applied to `design`: replaces duplicate item ids (`RepairDuplicateItemUuids`,
/// the first of each id keeps it) and adds back, to the board's net list, every net an item uses that the list lacks.
/// `nets` is the netlist the board is judged against (the intent's, or the schematic-derived one that replaced it).
pub fn repair_board(design: &mut Design, nets: &[Net]) -> RepairReport {
    let mut report = RepairReport::default();

    // Duplicate ids, across every kind of item that carries one: the first of an id keeps it, the rest are blanked.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut replaced = 0usize;
    let mut duplicate = |id: &str| {
        let dup = !id.is_empty() && !seen.insert(id.to_string());
        replaced += dup as usize;
        dup
    };
    if let Some(rt) = design.routing.as_mut() {
        for t in rt.tracks.iter_mut().filter(|t| duplicate(&t.id)) {
            t.id.clear();
        }
        for v in rt.vias.iter_mut().filter(|v| duplicate(&v.id)) {
            v.id.clear();
        }
        for z in rt.zones.iter_mut().filter(|z| duplicate(&z.id)) {
            z.id.clear();
        }
    }
    if let Some(dr) = design.drawings.as_mut() {
        for s in dr.shapes.iter_mut().filter(|s| duplicate(s.id())) {
            s.set_id(String::new());
        }
        for t in dr.texts.iter_mut().filter(|t| duplicate(&t.id)) {
            t.id.clear();
        }
        for g in dr.groups.iter_mut().filter(|g| duplicate(&g.id)) {
            g.id.clear();
        }
        for d in dr.dimensions.iter_mut().filter(|d| duplicate(&d.id)) {
            d.id.clear();
        }
    }
    if replaced > 0 {
        // The blanked ones get new deterministic ids, exactly as an item added by hand does.
        if let Some(rt) = design.routing.as_mut() {
            rt.assign_missing_ids();
        }
        if let Some(dr) = design.drawings.as_mut() {
            dr.assign_missing_ids();
        }
        report.repaired += replaced;
        report.details.push(format!("{replaced} duplicate IDs replaced."));
    }

    // Nets an item names but the netlist does not have.
    let known: BTreeSet<&str> = nets.iter().map(|n| n.name.as_str()).collect();
    let mut orphans: BTreeSet<String> = BTreeSet::new();
    if let Some(rt) = design.routing.as_ref() {
        for net in rt.tracks.iter().map(|t| &t.net).chain(rt.vias.iter().map(|v| &v.net)).chain(rt.zones.iter().map(|z| &z.net)) {
            if !net.is_empty() && !known.contains(net.as_str()) {
                orphans.insert(net.clone());
            }
        }
    }
    if !orphans.is_empty() {
        let mut list = nets.to_vec();
        for net in &orphans {
            list.push(Net { name: net.clone(), pins: vec![] });
            report.details.push(format!("Orphaned net {net} re-parented."));
            report.repaired += 1;
        }
        design.nets = Some(list);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn square(id: &str, layer: &str, net: &str, x: i64, y: i64, side: i64, priority: u32) -> Zone {
        Zone { id: id.into(), net: net.into(), layer: layer.into(), outline: vec![pt(x, y), pt(x + side, y), pt(x + side, y + side), pt(x, y + side)], priority, ..Default::default() }
    }

    /// Built from JSON so a field the IR gains later (with its serde default) does not break these.
    fn design(zones: Vec<Zone>) -> Design {
        let mut d: Design = serde_json::from_value(serde_json::json!({ "schema": 1, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 }, "routing": { "tracks": [], "vias": [] } })).unwrap();
        d.routing.as_mut().unwrap().zones = zones;
        d
    }

    fn zones(d: &Design) -> &Vec<Zone> {
        &d.routing.as_ref().unwrap().zones
    }

    fn priorities(d: &Design) -> Vec<(String, u32)> {
        zones(d).iter().map(|z| (z.id.clone(), z.priority)).collect()
    }

    #[test]
    fn segments_that_touch_collide_and_ones_that_miss_do_not() {
        assert!(segments_touch(pt(0, 0), pt(10, 10), pt(0, 10), pt(10, 0)), "a cross");
        assert!(segments_touch(pt(0, 0), pt(10, 0), pt(10, 0), pt(10, 10)), "end to end");
        assert!(segments_touch(pt(0, 0), pt(10, 0), pt(5, 0), pt(5, 10)), "a T");
        assert!(segments_touch(pt(0, 0), pt(10, 0), pt(5, 0), pt(15, 0)), "collinear and overlapping");
        assert!(!segments_touch(pt(0, 0), pt(10, 0), pt(11, 0), pt(15, 0)), "collinear, apart");
        assert!(!segments_touch(pt(0, 0), pt(10, 0), pt(0, 1), pt(10, 1)), "parallel");
    }

    #[test]
    fn a_point_is_inside_a_ring_the_way_pointinside_says() {
        let ring = [pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10)];
        assert!(ring_contains(&ring, pt(5, 5)));
        assert!(ring_contains(&ring, pt(10, 5)), "on an edge");
        assert!(ring_contains(&ring, pt(0, 0)), "on a corner");
        assert!(!ring_contains(&ring, pt(11, 5)));
        assert!(!ring_contains(&ring, pt(5, -1)));
        // A concave ring: the notch is outside.
        let c = [pt(0, 0), pt(10, 0), pt(10, 10), pt(6, 10), pt(6, 4), pt(4, 4), pt(4, 10), pt(0, 10)];
        assert!(!ring_contains(&c, pt(5, 8)));
        assert!(ring_contains(&c, pt(2, 8)));
    }

    #[test]
    fn two_zones_intersect_when_edges_touch_or_one_holds_the_other_and_only_on_one_layer() {
        let a = square("a", "F.Cu", "GND", 0, 0, 10_000, 0);
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", 5_000, 5_000, 10_000, 0)), "overlap");
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", 2_000, 2_000, 2_000, 0)), "inside: no edge crosses, a corner is inside");
        assert!(zones_intersect(&a, &square("b", "F.Cu", "GND", 10_000, 0, 5_000, 0)), "sharing an edge");
        assert!(!zones_intersect(&a, &square("b", "F.Cu", "GND", 20_000, 0, 5_000, 0)), "apart");
        assert!(!zones_intersect(&a, &square("b", "B.Cu", "GND", 5_000, 5_000, 10_000, 0)), "another layer");
    }

    #[test]
    fn merging_overlapping_zones_keeps_the_first_and_its_settings_and_takes_the_highest_priority() {
        let mut d = design(vec![square("z1", "F.Cu", "GND", 0, 0, 10_000, 1), square("z2", "F.Cu", "GND", 5_000, 0, 10_000, 3), square("z3", "F.Cu", "GND", 100_000, 0, 5_000, 9)]);
        let kept = merge_zones(&mut d, &["z1".into(), "z2".into(), "z3".into()]).unwrap();
        assert_eq!(kept, "z1");
        assert_eq!(priorities(&d), vec![("z1".to_string(), 3), ("z3".to_string(), 9)], "z2 is gone, z3 (apart) is untouched and was not merged");
        let merged = &zones(&d)[0];
        let (x0, y0, x1, y1) = bbox(&merged.outline);
        assert_eq!((x0, y0, x1, y1), (0, 0, 15_000, 10_000), "the union of the two squares");
        let area = {
            let mut s = ShapePolySet::from_outline(merged.outline.iter().map(|p| Point64::new(p.x, p.y)).collect());
            s.simplify();
            s.area()
        };
        assert!((area - 150_000_000.0).abs() < 1.0, "{area}");
    }

    #[test]
    fn merging_skips_zones_of_another_net_layer_or_kind_and_chains_through_a_zone_that_touches_a_chosen_one() {
        let other_net = square("n", "F.Cu", "VCC", 5_000, 0, 10_000, 0);
        let mut keepout = square("k", "F.Cu", "GND", 5_000, 0, 10_000, 0);
        keepout.is_rule_area = true;
        let mut d = design(vec![
            square("a", "F.Cu", "GND", 0, 0, 10_000, 0),
            other_net,
            keepout,
            square("b", "B.Cu", "GND", 5_000, 0, 10_000, 0),
            // c touches only b2, which is chosen before it, so c chains in through b2.
            square("b2", "F.Cu", "GND", 9_000, 0, 6_000, 0),
            square("c", "F.Cu", "GND", 14_000, 0, 6_000, 0),
        ]);
        merge_zones(&mut d, &["a".into(), "n".into(), "k".into(), "b".into(), "b2".into(), "c".into()]).unwrap();
        let left: Vec<&str> = zones(&d).iter().map(|z| z.id.as_str()).collect();
        assert_eq!(left, ["a", "n", "k", "b"], "a absorbed b2, then c (which only touches b2)");
        assert_eq!(bbox(&zones(&d)[0].outline), (0, 0, 20_000, 10_000));
    }

    #[test]
    fn merging_zones_that_do_not_touch_or_touch_only_at_a_corner_is_refused() {
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 0), square("b", "F.Cu", "GND", 50_000, 0, 10_000, 0)]);
        let e = merge_zones(&mut d, &["a".into(), "b".into()]).unwrap_err();
        assert_eq!(e[0].check, "ops_zone_merge");
        assert_eq!(zones(&d).len(), 2, "nothing changed");
        // Corner to corner: the intersection test passes (they touch), but the union is two outlines.
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 0), square("b", "F.Cu", "GND", 10_000, 10_000, 10_000, 0)]);
        let e = merge_zones(&mut d, &["a".into(), "b".into()]).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some("Zones have insufficient overlap for merging."));
        assert_eq!(zones(&d).len(), 2);
        // One zone is not a selection to merge.
        assert!(merge_zones(&mut d, &["a".into()]).is_err());
    }

    fn ring_area(ring: &[Point]) -> f64 {
        let twice: i128 = (0..ring.len()).map(|i| (ring[i].x as i128 * ring[(i + 1) % ring.len()].y as i128) - (ring[(i + 1) % ring.len()].x as i128 * ring[i].y as i128)).sum();
        (twice as f64 / 2.0).abs()
    }

    #[test]
    fn a_merge_that_closes_a_hole_stays_one_zone_with_the_hole_slit_into_its_ring() {
        // A U (a notch open at the top) and a bar across the notch's mouth: the void under the bar is a hole.
        let u = Zone { id: "u".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![pt(0, 0), pt(30_000, 0), pt(30_000, 30_000), pt(20_000, 30_000), pt(20_000, 10_000), pt(10_000, 10_000), pt(10_000, 30_000), pt(0, 30_000)], ..Default::default() };
        let bar = Zone { id: "bar".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![pt(8_000, 25_000), pt(22_000, 25_000), pt(22_000, 35_000), pt(8_000, 35_000)], ..Default::default() };
        let mut d = design(vec![u, bar]);
        merge_zones(&mut d, &["u".into(), "bar".into()]).unwrap();
        assert_eq!(zones(&d).len(), 1);
        let ring = &zones(&d)[0].outline;
        assert!(ring.len() > 8, "the hole's slit adds vertices: {ring:?}");
        // U (700e6) + bar (140e6) - their overlap (20e6); the slit has no area, and the hole is not in the U to begin with.
        assert!((ring_area(ring) - 820e6).abs() < 1.0, "{}", ring_area(ring));
    }

    #[test]
    fn priority_raise_goes_just_above_the_next_overlapping_zone_and_leaves_the_others_alone() {
        let mut d = design(vec![
            square("a", "F.Cu", "GND", 0, 0, 10_000, 1),
            square("b", "F.Cu", "GND", 5_000, 0, 10_000, 4),
            square("c", "F.Cu", "GND", 5_000, 5_000, 10_000, 7),
            square("far", "F.Cu", "GND", 90_000, 0, 1_000, 2),
        ]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d), [("a", 5), ("b", 4), ("c", 7), ("far", 2)].map(|(i, p)| (i.to_string(), p)).to_vec(), "just above b, the lowest above it; nothing else moved");
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d)[0], ("a".to_string(), 8), "then just above c");
        let e = set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap_err();
        assert_eq!(e[0].check, "ops_zone_priority", "already on top");
    }

    #[test]
    fn priority_lower_goes_just_below_the_next_overlapping_zone() {
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 9), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 4), square("c", "F.Cu", "GND", 5_000, 5_000, 10_000, 2)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d)[0].1, 3, "just below b, the highest below it");
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d)[0].1, 1, "then just below c");
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).is_err(), "already at the bottom");
    }

    #[test]
    fn raising_over_a_zone_at_the_ceiling_swaps_the_two_and_lowering_under_one_at_zero_swaps_too() {
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 5), square("b", "F.Cu", "GND", 5_000, 0, 10_000, u32::MAX)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).unwrap();
        assert_eq!(priorities(&d), vec![("a".to_string(), u32::MAX), ("b".to_string(), 5)]);
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 5), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 0)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Lower).unwrap();
        assert_eq!(priorities(&d), vec![("a".to_string(), 0), ("b".to_string(), 5)]);
    }

    #[test]
    fn move_to_top_takes_the_slot_that_displaces_fewer_zones() {
        // a (1) overlaps b (2) and c (3). Top: A takes 3 and pushes c to 2 -- but b is at 2, so b goes to 1: 2 moved.
        // B takes 4 and pushes nothing (4 is free): 0 moved. B wins.
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 1), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 2), square("c", "F.Cu", "GND", 5_000, 5_000, 10_000, 3)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Top).unwrap();
        assert_eq!(priorities(&d), [("a", 4), ("b", 2), ("c", 3)].map(|(i, p)| (i.to_string(), p)).to_vec());
        // Already on top.
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Top).is_err());
        // When the slot above is taken by a zone that does not overlap, pushing it up costs one more than pushing the
        // overlapping ones down only if fewer: a (1), b (2) overlap; far sits at 3 (not overlapping). A: take 2, push b to 1 (1 moved).
        // B: take 3, push far to 4 (1 moved). A wins a tie (down <= up).
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 1), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 2), square("far", "F.Cu", "GND", 90_000, 0, 1_000, 3)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Top).unwrap();
        assert_eq!(priorities(&d), [("a", 2), ("b", 1), ("far", 3)].map(|(i, p)| (i.to_string(), p)).to_vec(), "ties go down");
    }

    #[test]
    fn move_to_bottom_is_the_mirror_image_and_a_cascade_into_zero_is_not_viable() {
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 9), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 5), square("c", "F.Cu", "GND", 5_000, 5_000, 10_000, 4)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Bottom).unwrap();
        // A: take 4 and push c (4) to 5 and b (5) to 6: 2 moved. B: take 3, push nothing (3 is free). B wins.
        assert_eq!(priorities(&d), [("a", 3), ("b", 5), ("c", 4)].map(|(i, p)| (i.to_string(), p)).to_vec());
        // Zero is occupied below: the only way is up. a (3) overlaps b (0).
        let mut d = design(vec![square("a", "F.Cu", "GND", 0, 0, 10_000, 3), square("b", "F.Cu", "GND", 5_000, 0, 10_000, 0)]);
        set_zone_priority(&mut d, "a", ZonePriorityMove::Bottom).unwrap();
        assert_eq!(priorities(&d), [("a", 0), ("b", 1)].map(|(i, p)| (i.to_string(), p)).to_vec(), "a takes 0, b is pushed up (pushing down from -1 is not viable)");
    }

    #[test]
    fn the_priority_moves_only_apply_to_copper_zones_and_only_count_zones_on_the_same_layer() {
        let mut keepout = square("k", "F.Cu", "", 0, 0, 10_000, 0);
        keepout.is_rule_area = true;
        let mut d = design(vec![keepout, square("a", "F.Cu", "GND", 0, 0, 10_000, 0), square("b", "B.Cu", "GND", 0, 0, 10_000, 5)]);
        assert!(set_zone_priority(&mut d, "k", ZonePriorityMove::Raise).is_err(), "a rule area has no fill priority");
        assert!(set_zone_priority(&mut d, "a", ZonePriorityMove::Raise).is_err(), "b is on another layer, the keepout is not copper: nothing to rise over");
        assert!(set_zone_priority(&mut d, "nope", ZonePriorityMove::Raise).is_err());
    }

    #[test]
    fn the_drill_origin_is_set_and_reset_and_setting_what_is_there_is_refused() {
        let mut d = design(vec![]);
        set_aux_origin(&mut d, Some(pt(1_000, 2_000))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, Some(pt(1_000, 2_000)));
        assert!(set_aux_origin(&mut d, Some(pt(1_000, 2_000))).is_err());
        set_aux_origin(&mut d, None).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
        assert!(set_aux_origin(&mut d, None).is_err(), "already at (0, 0)");
        // (0, 0) is a reset, not a stored origin.
        set_aux_origin(&mut d, Some(pt(5, 5))).unwrap();
        set_aux_origin(&mut d, Some(pt(0, 0))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
    }

    #[test]
    fn the_grid_origin_is_set_and_reset_and_setting_what_is_there_is_refused() {
        let mut d = design(vec![]);
        set_grid_origin(&mut d, Some(pt(1_250, 2_500))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, Some(pt(1_250, 2_500)));
        assert!(set_grid_origin(&mut d, Some(pt(1_250, 2_500))).is_err());
        set_grid_origin(&mut d, None).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, None);
        assert!(set_grid_origin(&mut d, None).is_err(), "already at (0, 0)");
        // (0, 0) is a reset, not a stored origin, and the grid origin is not the drill/place file origin.
        set_grid_origin(&mut d, Some(pt(5, 5))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().aux_origin, None);
        set_grid_origin(&mut d, Some(pt(0, 0))).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().grid_origin, None);
    }

    #[test]
    fn repair_gives_duplicate_ids_new_ones_and_reports_kicads_lines() {
        let mut d = design(vec![square("zon_1", "F.Cu", "GND", 0, 0, 10_000, 0), square("zon_1", "B.Cu", "GND", 0, 0, 10_000, 0), square("zon_1", "F.Cu", "GND", 50_000, 0, 10_000, 0)]);
        let nets = vec![Net { name: "GND".into(), pins: vec![] }];
        let r = repair_board(&mut d, &nets);
        assert_eq!(r, RepairReport { repaired: 2, details: vec!["2 duplicate IDs replaced.".into()] });
        let ids: BTreeSet<&str> = zones(&d).iter().map(|z| z.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "all different now: {ids:?}");
        assert_eq!(zones(&d)[0].id, "zon_1", "the first keeps its id");
        assert!(zones(&d).iter().all(|z| !z.id.is_empty()));
        // Nothing left to repair.
        assert_eq!(repair_board(&mut d, &nets), RepairReport::default());
    }

    #[test]
    fn the_four_commands_read_from_json_and_apply_through_a_board() {
        use crate::{Board, Cmd};
        let model = eda_model::ConstraintModel::default();
        let mut d = design(vec![square("a", "F.Cu", "", 0, 0, 10_000, 1), square("b", "F.Cu", "", 5_000, 0, 10_000, 4), square("ghost", "F.Cu", "NOPE", 90_000, 0, 1_000, 0)]);
        d.placement = Some(serde_json::from_value(serde_json::json!({ "outline": [{ "x": 0, "y": 0 }, { "x": 1, "y": 0 }, { "x": 1, "y": 1 }], "footprints": [] })).unwrap());
        let mut board = Board::new(d, &model, 100, 300);
        let cmd = |j: serde_json::Value| serde_json::from_value::<Cmd>(j).unwrap();
        board.apply(&cmd(serde_json::json!({ "op": "set_zone_priority", "id": "a", "to": "raise" }))).unwrap();
        assert_eq!(priorities(board.design())[0], ("a".to_string(), 5));
        board.apply(&cmd(serde_json::json!({ "op": "merge_zones", "ids": ["a", "b"] }))).unwrap();
        assert_eq!(zones(board.design()).len(), 2, "b merged into a; ghost stays");
        board.apply(&cmd(serde_json::json!({ "op": "set_aux_origin", "at": { "x": 3_000, "y": 4_000 } }))).unwrap();
        assert_eq!(board.design().drawings.as_ref().unwrap().aux_origin, Some(pt(3_000, 4_000)));
        board.apply(&cmd(serde_json::json!({ "op": "set_aux_origin", "at": null }))).unwrap();
        assert_eq!(board.design().drawings.as_ref().unwrap().aux_origin, None);
        // The ghost zone's net is not in the (empty) netlist: one problem, then none.
        board.apply(&cmd(serde_json::json!({ "op": "repair_board" }))).unwrap();
        assert_eq!(board.design().nets.as_ref().unwrap()[0].name, "NOPE");
        // The next load reads the board's own netlist back (`board::load` lets `design.nets` replace the intent's).
        let design = board.into_design();
        let model = eda_model::ConstraintModel { nets: design.nets.clone().unwrap(), ..Default::default() };
        let again = Board::new(design, &model, 100, 300).apply(&Cmd::RepairBoard).unwrap_err();
        assert_eq!(again[0].hint.as_deref(), Some("No board problems found."));
    }

    #[test]
    fn repair_adds_back_a_net_that_items_use_but_the_netlist_lacks() {
        let mut d = design(vec![square("z", "F.Cu", "GHOST", 0, 0, 10_000, 0), square("y", "F.Cu", "GND", 0, 0, 10_000, 0), square("x", "F.Cu", "", 0, 0, 10_000, 0)]);
        let nets = vec![Net { name: "GND".into(), pins: vec!["U1.1".into()] }];
        let r = repair_board(&mut d, &nets);
        assert_eq!(r, RepairReport { repaired: 1, details: vec!["Orphaned net GHOST re-parented.".into()] });
        let list = d.nets.clone().unwrap();
        assert_eq!(list.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["GND", "GHOST"]);
        assert_eq!(list[0].pins, vec!["U1.1".to_string()], "the existing nets keep their pins");
        assert!(list[1].pins.is_empty());
        // The no-net zone ("") is not an orphan; a second run finds nothing.
        assert_eq!(repair_board(&mut d, &list), RepairReport::default());
    }
}
