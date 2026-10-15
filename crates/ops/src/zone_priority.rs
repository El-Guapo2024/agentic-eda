//! Auto-Assign Priorities: the Zone Manager's button, a port of `AutoAssignZonePriorities` (`pcbnew/zone_utils.cpp`).
//!
//! Copper pours that overlap on a layer take turns: the one with the higher priority keeps its copper and the other is knocked out of it, so which wins decides where the
//! copper goes. KiCad works the order out from what the overlap holds -- the pads and vias of each net inside it -- rather than leaving it to uuids:
//!
//!   1. the pairs of zones (copper pours only, no rule area, no teardrop) that share a copper layer and overlap;
//!   2. two overlapping zones of the *same net* are cooperative: they end up with one priority, the highest any of the group got;
//!   3. for two of different nets, the one with more of its own pads and vias inside the overlap wins. Where neither has any, or the counts are within 20% of each other,
//!      the smaller zone wins (a small pour inside a big one is the exception to it), and equal areas say nothing;
//!   4. the winners are ordered (Kahn's topological sort, edges from the weaker rule first, the zones' current priorities breaking the ties) and numbered from the top:
//!      the first gets `n - 1`, the last 0. A cycle's zones come last, in their current order.
//!
//! The dialog stages the result and applies it on OK (`edit_zone` per zone), as KiCad's does with its clones of the zones.

use std::collections::BTreeMap;

use eda_connectivity::geom::{point_in_polygon, polygons_overlap};
use eda_drc::board::DrcBoard;
use eda_model::ir::{Design, Point, Zone};
use eda_model::ConstraintModel;
use eda_shape_poly_set::ShapePolySet;

/// `SIMILARITY_THRESHOLD`: two counts within this fraction of the larger are not different enough to decide by.
const SIMILARITY_THRESHOLD: f64 = 0.20;

/// One overlapping pair of eligible zones (indices into the eligible list) and the copper layers they share.
struct Pair {
    a: usize,
    b: usize,
    shared: Vec<String>,
}

/// `ZONE_PRIORITY_EDGE`: `higher` goes above `lower`; `count_diff` is how many more pads and vias it has in the overlap, `from_area` that the size decided.
struct Edge {
    higher: usize,
    lower: usize,
    count_diff: usize,
    from_area: bool,
}

fn bbox(outline: &[Point]) -> (i64, i64, i64, i64) {
    outline.iter().fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)))
}

fn area(outline: &[Point]) -> f64 {
    let twice: i128 = (0..outline.len()).map(|i| outline[i].x as i128 * outline[(i + 1) % outline.len()].y as i128 - outline[(i + 1) % outline.len()].x as i128 * outline[i].y as i128).sum();
    (twice as f64 / 2.0).abs()
}

fn ring(outline: &[Point]) -> ShapePolySet {
    ShapePolySet::from_outline(outline.iter().map(|p| eda_clipper2::Point64::new(p.x, p.y)).collect())
}

/// `SHAPE_POLY_SET::Contains`: inside an outline of the set and in none of its holes.
fn contains(set: &ShapePolySet, p: Point) -> bool {
    let pts = |chain: &Vec<eda_clipper2::Point64>| chain.iter().map(|q| Point { x: q.x, y: q.y }).collect::<Vec<_>>();
    set.polys.iter().any(|poly| point_in_polygon(p, &pts(&poly[0])) && !poly[1..].iter().any(|hole| point_in_polygon(p, &pts(hole))))
}

/// `findOverlappingPairs`: pairs of eligible zones that share a copper layer, whose boxes meet and whose outlines overlap.
fn overlapping_pairs(zones: &[&Zone]) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for i in 0..zones.len() {
        let box_a = bbox(&zones[i].outline);
        for j in i + 1..zones.len() {
            let shared: Vec<String> = zones[i].copper_layers().into_iter().filter(|l| zones[j].is_on_layer(l)).map(String::from).collect();
            if shared.is_empty() {
                continue;
            }
            let box_b = bbox(&zones[j].outline);
            if box_b.0 > box_a.2 || box_b.2 < box_a.0 || box_b.1 > box_a.3 || box_b.3 < box_a.1 {
                continue;
            }
            if polygons_overlap(&zones[i].outline, &zones[j].outline) {
                pairs.push(Pair { a: i, b: j, shared });
            }
        }
    }
    pairs
}

/// `computeConstraint`: which of two overlapping zones of different nets goes above the other, or neither.
fn constraint(pair: &Pair, zones: &[&Zone], board: &DrcBoard) -> Option<Edge> {
    let mut intersection = ring(&zones[pair.a].outline);
    intersection.boolean_intersection(&ring(&zones[pair.b].outline));
    if intersection.is_empty() {
        return None;
    }
    let (net_a, net_b) = (zones[pair.a].net.as_str(), zones[pair.b].net.as_str());
    let (mut count_a, mut count_b) = (0usize, 0usize);
    let mut count = |at: Point, net: Option<&str>| {
        if contains(&intersection, at) {
            match net {
                Some(n) if n == net_a => count_a += 1,
                Some(n) if n == net_b => count_b += 1,
                _ => {}
            }
        }
    };
    // A pad counts once, if it is on any of the layers the zones share (`IsOnLayer`, then `break`).
    for pad in &board.pads {
        if pair.shared.iter().any(|l| pad.layers.contains(l)) {
            count(pad.center, pad.net.as_deref());
        }
    }
    let layer_index = |l: &str| board.layers.iter().position(|x| x == l);
    for via in &board.vias {
        let (from, to) = (layer_index(&via.from_layer), layer_index(&via.to_layer));
        let on = pair.shared.iter().any(|l| match (layer_index(l), from, to) {
            (Some(i), Some(f), Some(t)) => i >= f.min(t) && i <= f.max(t),
            _ => true,
        });
        if on {
            count(via.at, via.net.as_deref());
        }
    }

    // The smaller zone goes above the bigger; the same size says nothing.
    let by_area = |count_diff: usize| {
        let (area_a, area_b) = (area(&zones[pair.a].outline), area(&zones[pair.b].outline));
        if area_a == area_b {
            return None;
        }
        let (higher, lower) = if area_a < area_b { (pair.a, pair.b) } else { (pair.b, pair.a) };
        Some(Edge { higher, lower, count_diff, from_area: true })
    };
    if count_a == 0 && count_b == 0 {
        return by_area(0);
    }
    let diff = count_a.abs_diff(count_b);
    if (diff as f64 / count_a.max(count_b) as f64) < SIMILARITY_THRESHOLD {
        return by_area(diff);
    }
    let (higher, lower) = if count_a > count_b { (pair.a, pair.b) } else { (pair.b, pair.a) };
    Some(Edge { higher, lower, count_diff: diff, from_area: false })
}

/// `assignPrioritiesFromGraph`: Kahn's algorithm, the zones nothing must go above first; the current priorities (then the zones' places) break every tie. Returns the new
/// priority of each zone: the first of the order has `n - 1`.
fn assign_from_graph(edges: &[Edge], current: &[u32]) -> Vec<u32> {
    let n = current.len();
    let mut sorted: Vec<&Edge> = edges.iter().collect();
    // The area-based (weakest) edges first, then by ascending count difference.
    sorted.sort_by(|a, b| b.from_area.cmp(&a.from_area).then(a.count_diff.cmp(&b.count_diff)));
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree = vec![0usize; n];
    for e in &sorted {
        adj[e.higher].push(e.lower);
        in_degree[e.lower] += 1;
    }
    let key = |z: &usize| (current[*z], *z);
    let mut queue: Vec<usize> = (0..n).filter(|&z| in_degree[z] == 0).collect();
    queue.sort_by_key(key);
    let mut order: Vec<usize> = Vec::with_capacity(n);
    while !queue.is_empty() {
        let z = queue.remove(0);
        order.push(z);
        let mut next = adj[z].clone();
        next.sort_by_key(key);
        for m in next {
            in_degree[m] -= 1;
            if in_degree[m] == 0 {
                queue.push(m);
            }
        }
        queue.sort_by_key(key);
    }
    // Zones stuck in a cycle come last, in their current order.
    if order.len() < n {
        let mut rest: Vec<usize> = (0..n).filter(|z| !order.contains(z)).collect();
        rest.sort_by_key(key);
        order.extend(rest);
    }
    let mut out = vec![0u32; n];
    for (i, z) in order.iter().enumerate() {
        out[*z] = (n - 1 - i) as u32;
    }
    out
}

/// The priorities `AutoAssignZonePriorities` gives the copper pours of the board: zone id -> priority, for every pour. `None` when it changes nothing -- fewer than two pours,
/// none overlapping, or the priorities are already what it would give.
pub fn auto_assign_zone_priorities(design: &Design, model: &ConstraintModel) -> Option<BTreeMap<String, u32>> {
    let all: &[Zone] = design.routing.as_ref().map(|r| r.zones.as_slice()).unwrap_or_default();
    let zones: Vec<&Zone> = all.iter().filter(|z| !z.is_rule_area && !z.teardrop && z.is_on_copper_layer() && z.outline.len() >= 3).collect();
    if zones.len() < 2 {
        return None;
    }
    let original: Vec<u32> = zones.iter().map(|z| z.priority).collect();
    let pairs = overlapping_pairs(&zones);
    if pairs.is_empty() {
        return None;
    }

    // Overlapping zones of one net are cooperative: one group, one priority.
    let mut parent: Vec<usize> = (0..zones.len()).collect();
    fn find(parent: &mut [usize], z: usize) -> usize {
        if parent[z] != z {
            let root = find(parent, parent[z]);
            parent[z] = root;
        }
        parent[z]
    }
    for p in pairs.iter().filter(|p| zones[p.a].net == zones[p.b].net) {
        let (ra, rb) = (find(&mut parent, p.a), find(&mut parent, p.b));
        if ra != rb {
            parent[rb] = ra;
        }
    }

    let board = eda_drc::board::build(design, model);
    let edges: Vec<Edge> = pairs.iter().filter(|p| zones[p.a].net != zones[p.b].net).filter_map(|p| constraint(p, &zones, &board)).collect();
    let mut priority = if edges.is_empty() { original.clone() } else { assign_from_graph(&edges, &original) };

    // Each group takes the highest priority any of its members got, so the order the different-net edges decided holds for the whole group.
    let mut group_max: BTreeMap<usize, u32> = BTreeMap::new();
    for z in 0..zones.len() {
        let root = find(&mut parent, z);
        let max = group_max.entry(root).or_insert(0);
        *max = (*max).max(priority[z]);
    }
    for (z, p) in priority.iter_mut().enumerate() {
        *p = group_max[&find(&mut parent, z)];
    }

    (priority != original).then(|| zones.iter().zip(priority).map(|(z, p)| (z.id.clone(), p)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance, RoutingSection, Via};
    use eda_model::{BoardRules, Net, Part, Pin, PinKind};

    fn square(id: &str, net: &str, x: i64, y: i64, side: i64, priority: u32) -> Zone {
        let p = |x, y| Point { x, y };
        Zone { id: id.into(), net: net.into(), layer: "F.Cu".into(), outline: vec![p(x, y), p(x + side, y), p(x + side, y + side), p(x, y + side)], priority, ..Default::default() }
    }

    fn board(zones: Vec<Zone>, vias: Vec<Via>) -> (Design, ConstraintModel) {
        let part = Part { reference: "R1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), symbol: None, datasheet: None, pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None };
        let model = ConstraintModel { parts: vec![part], nets: vec![Net { name: "GND".into(), pins: vec![] }, Net { name: "VCC".into(), pins: vec![] }, Net { name: "AUX".into(), pins: vec![] }], board: BoardRules { layers: vec!["F.Cu".into(), "B.Cu".into()], ..Default::default() }, ..Default::default() };
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "t".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias, zones, track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        (design, model)
    }

    fn via(net: &str, x: i64, y: i64) -> Via {
        Via { id: format!("v{x}_{y}"), net: net.into(), at: Point { x, y }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
    }

    fn assigned(design: &Design, model: &ConstraintModel) -> Vec<(String, u32)> {
        auto_assign_zone_priorities(design, model).expect("a change").into_iter().collect()
    }

    #[test]
    fn the_zone_with_more_of_its_own_pads_and_vias_in_the_overlap_goes_above() {
        // GND and VCC overlap on x 5..10 mm; three GND vias and one VCC via sit in the overlap: 3 against 1, a clear win for GND whatever the sizes.
        let (d, m) = board(vec![square("gnd", "GND", 0, 0, 10_000, 0), square("vcc", "VCC", 5_000, 0, 10_000, 5)], vec![via("GND", 6_000, 2_000), via("GND", 7_000, 4_000), via("GND", 8_000, 6_000), via("VCC", 9_000, 8_000)]);
        assert_eq!(assigned(&d, &m), vec![("gnd".to_string(), 1), ("vcc".to_string(), 0)]);
    }

    #[test]
    fn with_nothing_to_count_or_counts_within_twenty_percent_the_smaller_zone_goes_above() {
        let (d, m) = board(vec![square("big", "GND", 0, 0, 20_000, 3), square("small", "VCC", 5_000, 5_000, 6_000, 0)], vec![]);
        assert_eq!(assigned(&d, &m), vec![("big".to_string(), 0), ("small".to_string(), 1)], "a small pour inside a big one is the exception to it");
        // Five vias each way is a tie: the size decides, not the counts.
        let vias = (0..5).flat_map(|i| [via("GND", 6_000 + i * 400, 6_000), via("VCC", 6_000 + i * 400, 8_000)]).collect();
        let (d, m) = board(vec![square("big", "GND", 0, 0, 20_000, 3), square("small", "VCC", 5_000, 5_000, 6_000, 0)], vias);
        assert_eq!(assigned(&d, &m), vec![("big".to_string(), 0), ("small".to_string(), 1)]);
        // Equal sizes say nothing: the priorities are left as they are.
        let (d, m) = board(vec![square("a", "GND", 0, 0, 10_000, 4), square("b", "VCC", 5_000, 0, 10_000, 4)], vec![]);
        assert!(auto_assign_zone_priorities(&d, &m).is_none());
    }

    #[test]
    fn zones_of_one_net_that_overlap_share_the_highest_priority_any_of_the_group_got() {
        // a and b are GND and overlap; c is VCC and overlaps b only, with more VCC vias in the overlap, so the edge says c above b.
        let (d, m) = board(
            vec![square("a", "GND", 0, 0, 10_000, 0), square("b", "GND", 8_000, 0, 10_000, 1), square("c", "VCC", 15_000, 0, 10_000, 2)],
            vec![via("VCC", 16_000, 2_000), via("VCC", 16_500, 4_000), via("VCC", 17_000, 6_000)],
        );
        let got = assigned(&d, &m);
        let p = |id: &str| got.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(p("a"), p("b"), "one priority for the cooperating pair: {got:?}");
        // KiCad's own outcome, kept: a has nothing to be above or below, so the sort puts it first (its priority is the lowest of the zones nothing constrains); c comes
        // next and b last, and then b is lifted to its group's top, a's. The group ends above c although the edge said c above b.
        assert_eq!((p("a"), p("b"), p("c")), (2, 2, 1), "{got:?}");

        // Where c is the one with the lowest current priority, it is taken first and stays above its group.
        let (d, m) = board(
            vec![square("a", "GND", 0, 0, 10_000, 5), square("b", "GND", 8_000, 0, 10_000, 6), square("c", "VCC", 15_000, 0, 10_000, 0)],
            vec![via("VCC", 16_000, 2_000), via("VCC", 16_500, 4_000), via("VCC", 17_000, 6_000)],
        );
        let got = assigned(&d, &m);
        let p = |id: &str| got.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!((p("c"), p("a"), p("b")), (2, 1, 1), "c first (2), a next (1), b last (0) and lifted to its group's 1: {got:?}");
    }

    #[test]
    fn a_chain_of_overlaps_is_ordered_from_the_top_and_a_cycle_keeps_its_current_order() {
        // c is smallest, then b, then a: c above b above a.
        let (d, m) = board(vec![square("a", "GND", 0, 0, 30_000, 0), square("b", "VCC", 5_000, 5_000, 20_000, 0), square("c", "AUX", 10_000, 10_000, 8_000, 0)], vec![]);
        let got = assigned(&d, &m);
        let p = |id: &str| got.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!((p("c"), p("b"), p("a")), (2, 1, 0), "{got:?}");
        // Already so: nothing to change.
        let (d, m) = board(vec![square("a", "GND", 0, 0, 30_000, 0), square("b", "VCC", 5_000, 5_000, 20_000, 1), square("c", "AUX", 10_000, 10_000, 8_000, 2)], vec![]);
        assert!(auto_assign_zone_priorities(&d, &m).is_none());
    }

    #[test]
    fn rule_areas_teardrops_zones_on_other_layers_and_disjoint_zones_are_left_alone() {
        let mut keepout = square("k", "", 0, 0, 10_000, 0);
        keepout.is_rule_area = true;
        let mut other_layer = square("o", "VCC", 0, 0, 10_000, 0);
        other_layer.layer = "B.Cu".into();
        let (d, m) = board(vec![square("a", "GND", 0, 0, 10_000, 0), keepout, other_layer, square("far", "AUX", 90_000, 0, 5_000, 0)], vec![]);
        assert!(auto_assign_zone_priorities(&d, &m).is_none());
        // One pour alone has nothing to be ordered against.
        let (d, m) = board(vec![square("a", "GND", 0, 0, 10_000, 0)], vec![]);
        assert!(auto_assign_zone_priorities(&d, &m).is_none());
    }
}
