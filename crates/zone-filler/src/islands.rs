//! Isolated copper islands: `CN_CONNECTIVITY_ALGO::FillIsolatedIslandsMap` (`pcbnew/connectivity/connectivity_algo.cpp`).
//!
//! A fragment of a zone's fill is an *island* when the cluster of copper it belongs to has no pad. The cluster is the set of
//! same-net items that touch one another, across layers: pads, vias (every layer they span) and tracks, and the fragments of
//! every zone of the net -- a fragment is connected to a pad, via or track that touches it (`CN_VISITOR::checkZoneItemConnection`:
//! the item's anchor inside the fragment, or its shape colliding with it), and to another zone's fragment on the same layer
//! when a vertex of one lies inside the other (`checkZoneZoneConnection`). A fragment that reaches a pad only through a track,
//! a via, or a fragment of another zone is therefore not an island; one that touches nothing but a lone track or via is.
//!
//! A zone with no net has nothing to connect to: `FillIsolatedIslandsMap` reports its first polygon only (a quirk kept as it is).

use crate::hatch::{chain_bounds, even_odd, segments_intersect};
use crate::shape::{self, Shape};
use crate::FillInput;
use eda_clipper2::Point64;
use eda_model::ir::Zone;
use eda_shape_poly_set::{LineChain, ShapePolySet};
use std::collections::HashMap;

/// The edges of one polygon outline bucketed by their y range, for point-in-polygon and edge-crossing queries.
struct EdgeIndex {
    edges: Vec<(Point64, Point64)>,
    stripes: Vec<Vec<u32>>,
    y_min: i64,
    stripe_height: i64,
    bbox: (i64, i64, i64, i64),
}

impl EdgeIndex {
    fn new(chain: &LineChain) -> EdgeIndex {
        let bbox = chain_bounds(chain);
        let n = chain.len();
        let edges: Vec<(Point64, Point64)> = (0..n).map(|i| (chain[i], chain[(i + 1) % n])).filter(|(a, b)| a != b).collect();
        let stripe_count = ((edges.len() as f64).sqrt() as usize).clamp(1, 4096);
        let y_range = (bbox.3 - bbox.1).max(1);
        let stripe_height = ((y_range + stripe_count as i64 - 1) / stripe_count as i64).max(1);
        let mut stripes = vec![Vec::new(); stripe_count];
        for (i, (a, b)) in edges.iter().enumerate() {
            let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
            let s0 = (((lo - bbox.1) / stripe_height) as usize).min(stripe_count - 1);
            let s1 = (((hi - bbox.1) / stripe_height) as usize).min(stripe_count - 1);
            for s in s0..=s1 {
                stripes[s].push(i as u32);
            }
        }
        EdgeIndex { edges, stripes, y_min: bbox.1, stripe_height, bbox }
    }

    fn stripe_of(&self, y: i64) -> usize {
        (((y - self.y_min) / self.stripe_height).max(0) as usize).min(self.stripes.len() - 1)
    }

    /// Even-odd containment of `pt` in the outline.
    fn contains(&self, pt: Point64) -> bool {
        if pt.x < self.bbox.0 || pt.x > self.bbox.2 || pt.y < self.bbox.1 || pt.y > self.bbox.3 {
            return false;
        }
        let mut inside = false;
        for &i in &self.stripes[self.stripe_of(pt.y)] {
            let (p1, p2) = self.edges[i as usize];
            if (p1.y >= pt.y) == (p2.y >= pt.y) {
                continue;
            }
            let d = (p2.x - p1.x) as f64 * (pt.y - p1.y) as f64 / (p2.y - p1.y) as f64;
            if ((pt.x - p1.x) as f64) < d.trunc() {
                inside = !inside;
            }
        }
        inside
    }

    /// Does a polygon (closed outline) touch this outline's area: a vertex of it inside, or an edge crossing an edge here
    /// (or, the other way round, an edge of this one inside it: the small polygon holds a vertex of the big one).
    fn collides(&self, other: &LineChain, other_bbox: (i64, i64, i64, i64)) -> bool {
        if other_bbox.2 < self.bbox.0 || other_bbox.0 > self.bbox.2 || other_bbox.3 < self.bbox.1 || other_bbox.1 > self.bbox.3 {
            return false;
        }
        if other.iter().any(|&p| self.contains(p)) {
            return true;
        }
        let n = other.len();
        let (s0, s1) = (self.stripe_of(other_bbox.1), self.stripe_of(other_bbox.3));
        for s in s0..=s1 {
            for &ei in &self.stripes[s] {
                let (a, b) = self.edges[ei as usize];
                if a.x.max(b.x) < other_bbox.0 || a.x.min(b.x) > other_bbox.2 || a.y.max(b.y) < other_bbox.1 || a.y.min(b.y) > other_bbox.3 {
                    continue;
                }
                for j in 0..n {
                    if segments_intersect(a, b, other[j], other[(j + 1) % n]) {
                        return true;
                    }
                }
            }
        }
        // the other polygon entirely around a vertex of this outline
        let probe = self.edges.first().map(|e| e.0);
        probe.is_some_and(|p| p.x >= other_bbox.0 && p.x <= other_bbox.2 && p.y >= other_bbox.1 && p.y <= other_bbox.3 && even_odd(other, p))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pad,
    Via,
    Track,
    Fragment,
}

struct Node<'a> {
    kind: Kind,
    net: &'a str,
    /// A pad, via or track names its layers; a fragment its zone's one.
    layers: Vec<&'a str>,
    bbox: (i64, i64, i64, i64),
    outline: LineChain,
    anchors: Vec<Point64>,
    /// (zone index, polygon index) of a fragment.
    fragment: Option<(usize, usize)>,
    /// For a fragment: a stripe index over its outline (and holes' edges are not needed: a fractured outline has none).
    index: Option<EdgeIndex>,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

fn layers_overlap(a: &[&str], b: &[&str]) -> bool {
    a.iter().any(|l| b.contains(l))
}

fn bbox_hit(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0.max(b.0) <= a.2.min(b.2) && a.1.max(b.1) <= a.3.min(b.3)
}

/// Whether two item outlines (small convex-ish polygons: pads, vias, tracks) touch.
fn outlines_touch(a: &Node, b: &Node) -> bool {
    if !bbox_hit(a.bbox, b.bbox) {
        return false;
    }
    if a.anchors.iter().any(|&p| even_odd(&b.outline, p)) || b.anchors.iter().any(|&p| even_odd(&a.outline, p)) {
        return true;
    }
    let (na, nb) = (a.outline.len(), b.outline.len());
    for i in 0..na {
        for j in 0..nb {
            if segments_intersect(a.outline[i], a.outline[(i + 1) % na], b.outline[j], b.outline[(j + 1) % nb]) {
                return true;
            }
        }
    }
    false
}

/// For every zone of `zones` (each with its fill, on its own layer), the indices of the polygons of the fill that are
/// isolated islands (`ISOLATED_ISLANDS::m_IsolatedOutlines`).
pub fn find_isolated_islands(zones: &[(&Zone, &ShapePolySet)], input: &FillInput, max_error: i64) -> Vec<Vec<usize>> {
    let mut result: Vec<Vec<usize>> = vec![Vec::new(); zones.len()];
    let mut nodes: Vec<Node> = Vec::new();

    // Items with a net: pads, vias, tracks.
    for pad in &input.pads {
        let Some(net) = pad.net.as_deref() else { continue };
        let outline = shape::exact_polygon(&pad.copper, max_error);
        if outline.len() < 3 {
            continue;
        }
        let (b0, b1, b2, b3) = shape::bounds(&pad.copper);
        let centre = pad.geometry.map(|g| g.center).unwrap_or(Point64::new((b0 + b2) / 2, (b1 + b3) / 2));
        nodes.push(Node { kind: Kind::Pad, net, layers: pad.layers.iter().map(String::as_str).collect(), bbox: (b0, b1, b2, b3), outline, anchors: vec![centre], fragment: None, index: None });
    }
    for via in &input.vias {
        let Some(net) = via.net.as_deref() else { continue };
        let shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
        let outline = shape::exact_polygon(&shape, max_error);
        let layers: Vec<&str> = via.layer_order.iter().map(String::as_str).filter(|l| via.is_on_layer(l)).collect();
        nodes.push(Node { kind: Kind::Via, net, layers, bbox: shape::bounds(&shape), outline, anchors: vec![via.at], fragment: None, index: None });
    }
    for track in &input.tracks {
        let Some(net) = track.net.as_deref() else { continue };
        let shape = Shape::Stadium { a: track.a, b: track.b, r: track.width / 2 };
        let outline = shape::exact_polygon(&shape, max_error);
        nodes.push(Node { kind: Kind::Track, net, layers: vec![track.layer.as_str()], bbox: shape::bounds(&shape), outline, anchors: vec![track.a, track.b], fragment: None, index: None });
    }
    let first_fragment = nodes.len();
    // Zone fragments.
    for (zi, (zone, fill)) in zones.iter().enumerate() {
        if zone.net.is_empty() {
            // a zone with no net is "not in connectivity": its first polygon is reported as isolated
            if !fill.is_empty() {
                result[zi].push(0);
            }
            continue;
        }
        for (pi, poly) in fill.polys.iter().enumerate() {
            let outline = &poly[0];
            if outline.len() < 3 {
                continue;
            }
            nodes.push(Node {
                kind: Kind::Fragment,
                net: zone.net.as_str(),
                layers: vec![zone.layer.as_str()],
                bbox: chain_bounds(outline),
                outline: outline.clone(),
                anchors: Vec::new(),
                fragment: Some((zi, pi)),
                index: Some(EdgeIndex::new(outline)),
            });
        }
    }

    // Union-find within each net.
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    let mut by_net: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        by_net.entry(n.net).or_default().push(i);
    }
    for members in by_net.values() {
        for (ai, &a) in members.iter().enumerate() {
            for &b in &members[ai + 1..] {
                let (na, nb) = (&nodes[a], &nodes[b]);
                if !bbox_hit(na.bbox, nb.bbox) || !layers_overlap(&na.layers, &nb.layers) {
                    continue;
                }
                let connected = match (na.kind, nb.kind) {
                    (Kind::Fragment, Kind::Fragment) => {
                        // `checkZoneZoneConnection`: a vertex of one inside the other, different zones only.
                        na.fragment.map(|f| f.0) != nb.fragment.map(|f| f.0) && (nb.outline.iter().any(|&p| na.index.as_ref().is_some_and(|ix| ix.contains(p))) || na.outline.iter().any(|&p| nb.index.as_ref().is_some_and(|ix| ix.contains(p))))
                    }
                    (Kind::Fragment, _) => item_touches_fragment(nb, na),
                    (_, Kind::Fragment) => item_touches_fragment(na, nb),
                    _ => outlines_touch(na, nb),
                };
                if connected {
                    union(&mut parent, a, b);
                }
            }
        }
    }

    // A cluster with no pad is orphaned: its fragments are islands.
    let mut has_pad: HashMap<usize, bool> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.kind == Kind::Pad {
            has_pad.insert(find(&mut parent, i), true);
        }
    }
    for i in first_fragment..nodes.len() {
        if let Some((zi, pi)) = nodes[i].fragment {
            let root = find(&mut parent, i);
            if !has_pad.contains_key(&root) {
                result[zi].push(pi);
            }
        }
    }
    for r in &mut result {
        r.sort_unstable();
        r.dedup();
    }
    result
}

/// `CN_VISITOR::checkZoneItemConnection`: an anchor of the item inside the fragment, else the item's shape colliding with it.
fn item_touches_fragment(item: &Node, fragment: &Node) -> bool {
    let Some(ix) = fragment.index.as_ref() else { return false };
    if item.anchors.iter().any(|&p| ix.contains(p)) {
        return true;
    }
    ix.collides(&item.outline, item.bbox)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FillPad, FillTrack};
    use eda_model::ir::Point;

    fn zone(net: &str) -> Zone {
        Zone { net: net.into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }, Point { x: 0, y: 10_000 }], ..Default::default() }
    }

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> ShapePolySet {
        ShapePolySet::from_outline(vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)])
    }

    fn pad_at(x: i64, y: i64, net: &str) -> FillPad {
        FillPad { net: Some(net.into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: x - 200, y0: y - 200, x1: x + 200, y1: y + 200 }, ..Default::default() }
    }

    #[test]
    fn a_fragment_without_a_pad_in_reach_is_an_island() {
        let z = zone("GND");
        let mut fill = rect(0, 0, 4_000, 4_000);
        fill.polys.push(rect(6_000, 6_000, 9_000, 9_000).polys.remove(0));
        let input = FillInput { pads: vec![pad_at(1_000, 1_000, "GND")], ..Default::default() };
        let islands = find_isolated_islands(&[(&z, &fill)], &input, 5);
        assert_eq!(islands, vec![vec![1]], "the second fragment touches no pad");
    }

    #[test]
    fn a_track_from_a_pad_makes_the_fragment_it_touches_connected() {
        let z = zone("GND");
        let mut fill = rect(0, 0, 4_000, 4_000);
        fill.polys.push(rect(6_000, 6_000, 9_000, 9_000).polys.remove(0));
        // a track from the pad (in fragment 0) to the second fragment
        let input = FillInput {
            pads: vec![pad_at(1_000, 1_000, "GND")],
            tracks: vec![FillTrack { net: Some("GND".into()), layer: "F.Cu".into(), a: Point64::new(1_000, 1_000), b: Point64::new(7_000, 7_000), width: 200 }],
            ..Default::default()
        };
        let islands = find_isolated_islands(&[(&z, &fill)], &input, 5);
        assert_eq!(islands, vec![Vec::<usize>::new()], "pad -> track -> fragment 1 keeps both");
    }

    #[test]
    fn a_pad_of_another_net_does_not_connect() {
        let z = zone("GND");
        let fill = rect(0, 0, 4_000, 4_000);
        let input = FillInput { pads: vec![pad_at(1_000, 1_000, "VCC")], ..Default::default() };
        assert_eq!(find_isolated_islands(&[(&z, &fill)], &input, 5), vec![vec![0]]);
    }

    #[test]
    fn a_via_alone_is_not_an_anchor() {
        let z = zone("GND");
        let fill = rect(0, 0, 4_000, 4_000);
        let input = FillInput { vias: vec![crate::FillVia { net: Some("GND".into()), at: Point64::new(1_000, 1_000), diameter: 600, drill: 300, from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), layer_order: vec!["F.Cu".into(), "B.Cu".into()] }], ..Default::default() };
        assert_eq!(find_isolated_islands(&[(&z, &fill)], &input, 5), vec![vec![0]], "no pad in the cluster");
    }

    #[test]
    fn a_fragment_overlapping_a_connected_fragment_of_another_zone_is_connected() {
        let a = zone("GND");
        let b = Zone { priority: 1, ..zone("GND") };
        let fill_a = rect(0, 0, 4_000, 4_000);
        let fill_b = rect(3_000, 3_000, 8_000, 8_000);
        let input = FillInput { pads: vec![pad_at(1_000, 1_000, "GND")], ..Default::default() };
        let islands = find_isolated_islands(&[(&a, &fill_a), (&b, &fill_b)], &input, 5);
        assert_eq!(islands, vec![Vec::<usize>::new(), Vec::<usize>::new()]);
    }
}
