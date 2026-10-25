//! The length a tuner tunes: `TOPOLOGY::AssembleTuningPath` (`pns_topology.cpp`) and `PNS_KICAD_IFACE_BASE::CalculateRoutedPathLength`
//! (`LENGTH_DELAY_CALCULATION::CalculateLengthDetails`).
//!
//! A net's tuning path starts with the line the clicked track is part of and goes on from both of its ends: through a via to the lines
//! of the net it joins (`findLinesFromVia`), until a pad (the path ends there, `walkTuningPath`) or a dead end or a junction. Its
//! length is the length of its lines -- the end of a line that finishes on a pad's centre clipped to the pad outline
//! (`OptimiseTraceInPad`), the part of a line inside a via's pad to the via's centre (`OptimiseTraceInVia`) -- plus, with *Use height for
//! length calculations*, the height of the stackup a via spans (`StackupHeight`). The pad-to-die length is not in the model.
//!
//! Lines are kept and measured in nanometres, as KiCad's `SHAPE_LINE_CHAIN::Length` is.

use eda_model::units::{mm, Nm, PCB_IU_PER_MM};
use crate::chain::ShapeLineChain;
use crate::item::{Item, ItemId, Net};
use crate::kigeom::{ki_round_nm, Seg};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::{AssembleOpts, Node};
use eda_drc::kimath::Shape;
use eda_model::ir::Point;
use eda_model::{BoardRules, Stackup};
use std::collections::HashSet;

/// What a tuning path is made of, in order from one end to the other.
#[derive(Debug, Clone)]
pub enum PathItem {
    /// A line of the net, as `NODE::AssembleLine` made it, with the pad-clipped ends of `AssembleTuningPath` already applied
    /// to its measuring copy, [`PathItem::clipped`].
    Line { line: Line, clipped: ShapeLineChain },
    /// A via of the net, between the lines on either side of it.
    Via(ItemId),
}

/// `TOPOLOGY::AssembleTuningPath`'s result: the items and the pads the path ends on (`aStartPad`, `aEndPad`).
#[derive(Debug, Clone, Default)]
pub struct TuningPath {
    pub items: Vec<PathItem>,
    pub start_pad: Option<ItemId>,
    pub end_pad: Option<ItemId>,
    /// Index of the line the path was started from, in `items`.
    pub origin: usize,
}

/// The board facts a length is measured with.
#[derive(Debug, Clone)]
pub struct LengthEnv {
    pub rules: BoardRules,
    pub stackup: Stackup,
}

impl LengthEnv {
    /// The board's rules and its stackup (the default one for its copper layers when it has none: `BuildDefaultStackupList`).
    pub fn of(model: &eda_model::ConstraintModel) -> LengthEnv {
        let rules = model.board.clone();
        let stackup = model.stackup.clone().unwrap_or_else(|| eda_model::rules::default_stackup(rules.layers.len(), mm(1.6)));
        LengthEnv { rules, stackup }
    }
}

fn all_layers() -> LayerRange {
    LayerRange::new(0, 255)
}

fn contains(shape: &Shape, p: Point) -> bool {
    shape.collides(&Shape::Circle { c: p, r: Nm::ZERO }, Nm::ZERO).is_some()
}

/// `HitTest( aPt )` among the world's pads and vias of `net`: the items whose shape holds the point.
fn hits_at(world: &Node, p: Point) -> Vec<ItemId> {
    world.overlapping(&Shape::Circle { c: p, r: Nm::ZERO }, all_layers(), &[])
}

fn same_net(a: &Net, b: &Net) -> bool {
    a == b
}

/// `clipLineToPad` / `clipLineToVia`: the end of `pts` that is inside `inside` (`forward`: the start) is cut where the line leaves it: the
/// centre, the point where the line crosses the outline, and the rest of the line from the first vertex outside. Nanometres. A line wholly
/// inside is left alone.
fn clip_line(pts: &[Point], center: Point, inside: &dyn Fn(Point) -> bool, forward: bool) -> Vec<Point> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let (start, delta): (isize, isize) = if forward { (0, 1) } else { (n as isize - 1, -1) };
    let mut v = start + delta;
    while v >= 0 && (v as usize) < n {
        if !inside(pts[v as usize]) {
            let prev = pts[(v - delta) as usize];
            let out = pts[v as usize];
            // where the segment from the last inside vertex to the first outside one leaves the shape (found to a nanometre)
            let (mut lo, mut hi) = (0.0f64, 1.0f64);
            for _ in 0..40 {
                let mid = (lo + hi) / 2.0;
                let q = Point { x: prev.x + (out.x - prev.x).scale(mid), y: prev.y + (out.y - prev.y).scale(mid) };
                if inside(q) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let cross = Point { x: prev.x + (out.x - prev.x).scale(hi), y: prev.y + (out.y - prev.y).scale(hi) };
            let mut chain = ShapeLineChain::new();
            if forward {
                chain.append_point(center);
                chain.append_point(cross);
                for p in &pts[v as usize..] {
                    chain.append_point(*p);
                }
            } else {
                for p in &pts[..=v as usize] {
                    chain.append_point(*p);
                }
                chain.append_point(cross);
                chain.append_point(center);
            }
            return chain.points().to_vec();
        }
        v += delta;
    }
    pts.to_vec() // "All points inside pad, nothing to clip"
}

/// `OptimiseTraceInPad( aLine, aPad, aLayer )` / `OptimiseTraceInVia`: a line that ends on the centre of `item` (a pad: its position; a via: any
/// point of it inside) has its end clipped to the item's outline. `layer` is the line's.
fn optimise_trace_in(chain: &mut ShapeLineChain, item: &Item, layer: i32, via: bool) {
    let (center_nm, shape, layers) = match item {
        Item::Solid(s) => (s.pos, s.shape.clone(), s.layers),
        Item::Via(v) => (v.pos, Shape::Circle { c: v.pos, r: v.diameter / 2 }, v.layers),
        Item::Segment(_) | Item::Arc(_) => return,
    };
    if !layers.overlaps_layer(layer) || chain.point_count() < 2 {
        return;
    }
    let center = center_nm;
    let first = chain.c_point(0);
    let last = chain.c_last_point();
    let inside = |p: Point| contains(&shape, p);
    if !via && first != center && last != center {
        return; // "Only consider lines which terminate in the pad"
    }
    let pts = chain.points().to_vec();
    let clipped = if inside(first) {
        clip_line(&pts, center, &inside, true)
    } else if inside(last) {
        clip_line(&pts, center, &inside, false)
    } else {
        return;
    };
    *chain = ShapeLineChain::from_points(&clipped);
}

/// The line as the nanometre chain KiCad measures and cuts: its arcs are the arcs they are (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`), the rest points.
pub(crate) fn line_chain_nm(line: &Line) -> ShapeLineChain {
    let runs = line.arc_runs();
    let mut c = ShapeLineChain::new();
    let mut i = 0;
    while i < line.pts.len() {
        if let Some(r) = runs.iter().find(|r| r.first == i) {
            let a = if r.reversed { line.arcs[r.arc].reversed() } else { line.arcs[r.arc] };
            c.append_arc(&crate::kigeom::ShapeArc::new_nm(a.start, a.mid, a.end, Nm::ZERO));
            i = r.last + 1;
            continue;
        }
        c.append_point(line.pts[i]);
        i += 1;
    }
    c
}

/// `TOPOLOGY::findLinesFromVia`: the lines of the via's net that have an end inside the via, other than the ones already `visited`.
fn lines_from_via(world: &Node, via_id: ItemId, visited: &HashSet<ItemId>) -> Vec<Line> {
    let Some(Item::Via(via)) = world.get(via_id) else { return Vec::new() };
    let shape = Shape::Circle { c: via.pos, r: via.diameter / 2 };
    let mut assembled: HashSet<ItemId> = HashSet::new();
    let mut out = Vec::new();
    for id in world.overlapping(&shape, via.layers, &[via_id]) {
        let Some(item) = world.get(id) else { continue };
        let Some((a, b)) = item.track_ends() else { continue };
        if !same_net(item.net(), &via.net) || visited.contains(&id) || assembled.contains(&id) {
            continue;
        }
        // "Make sure at least one anchor is inside the via pad"
        if !contains(&shape, a) && !contains(&shape, b) {
            continue;
        }
        let Some(line) = world.assemble_line_with(id, assemble_opts()) else { continue };
        assembled.extend(line.segment_ids.iter().copied());
        out.push(line);
    }
    out
}

/// The line `AssembleLine( seg )` gives the tuner: not through a locked segment, and it stops where the track width changes -- KiCad's call
/// lets a line cross a width change and rewrites the whole of it at the start segment's width, which would change the tracks it does not tune.
pub fn assemble_opts() -> AssembleOpts {
    AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false, stop_at_locked_joints: false }
}

struct WalkResult {
    items: Vec<PathItem>,
    length: Nm,
    end_pad: Option<ItemId>,
}

/// `TOPOLOGY::walkTuningPath`: from the end of `start` (its last point, or its first), the longest way on through vias to a pad, a dead end or a
/// junction.
fn walk_tuning_path(world: &Node, start: &Line, from_back: bool, visited: &HashSet<ItemId>) -> WalkResult {
    struct State {
        endpoint: Point,
        items: Vec<PathItem>,
        length: Nm,
        visited: HashSet<ItemId>,
    }
    let mut best = WalkResult { items: Vec::new(), length: Nm::new(-1), end_pad: None };
    let mut stack = vec![State { endpoint: if from_back { *start.pts.last().expect("a line") } else { start.pts[0] }, items: Vec::new(), length: Nm::ZERO, visited: visited.clone() }];
    let net = start.net.clone();
    let mut steps = 0;
    while let Some(mut current) = stack.pop() {
        steps += 1;
        if steps > 2000 {
            break; // (KiCad times the walk out after `m_FollowBranchTimeout`)
        }
        let hits = hits_at(world, current.endpoint);
        let pad = hits.iter().copied().find(|id| matches!(world.get(*id), Some(Item::Solid(s)) if !s.edge && same_net(&s.net, &net)) && !current.visited.contains(id));
        if let Some(pad) = pad {
            if current.length > best.length {
                best = WalkResult { items: current.items, length: current.length, end_pad: Some(pad) };
            }
            continue;
        }
        let via = hits.iter().copied().find(|id| matches!(world.get(*id), Some(Item::Via(v)) if same_net(&v.net, &net)) && !current.visited.contains(id));
        if let Some(via) = via {
            current.visited.insert(via);
            let continuations = lines_from_via(world, via, &current.visited);
            for cont in &continuations {
                let ep = current.endpoint;
                let d = |p: Point| ((p.x - ep.x).get() as i128).pow(2) + ((p.y - ep.y).get() as i128).pow(2);
                let (first, last) = (cont.pts[0], *cont.pts.last().unwrap());
                let start_near_via = d(first) <= d(last);
                let forward_endpoint = if start_near_via { last } else { first };
                let mut chain = line_chain_nm(cont);
                if let Some(item) = world.get(via) {
                    optimise_trace_in(&mut chain, item, cont.layer, true);
                }
                let mut next_items = current.items.clone();
                next_items.push(PathItem::Via(via));
                next_items.push(PathItem::Line { line: cont.clone(), clipped: chain.clone() });
                let mut next_visited = current.visited.clone();
                next_visited.extend(cont.segment_ids.iter().copied());
                stack.push(State { endpoint: forward_endpoint, items: next_items, length: current.length + chain.length_nm(), visited: next_visited });
            }
            if continuations.is_empty() && current.length > best.length {
                let mut items = current.items.clone();
                items.push(PathItem::Via(via));
                best = WalkResult { items, length: current.length, end_pad: None };
            }
        } else if current.length > best.length {
            best = WalkResult { items: current.items, length: current.length, end_pad: None };
        }
    }
    best
}

/// `TOPOLOGY::AssembleTuningPath( aRouterIface, aStart, &aStartPad, &aEndPad )` for a track segment of the world.
pub fn assemble_tuning_path(world: &Node, start: ItemId) -> Option<TuningPath> {
    let line = world.assemble_line_with(start, assemble_opts())?;
    let visited: HashSet<ItemId> = line.segment_ids.iter().copied().collect();
    let left = walk_tuning_path(world, &line, false, &visited);
    let right = walk_tuning_path(world, &line, true, &visited);

    let mut items: Vec<PathItem> = left.items.into_iter().rev().collect();
    let origin = items.len();
    items.push(PathItem::Line { clipped: line_chain_nm(&line), line });
    items.extend(right.items);

    let mut path = TuningPath { items, start_pad: left.end_pad, end_pad: right.end_pad, origin };

    // the pads the path starts and ends on, and every pad a line of it ends on: `processPad`
    let mut pads: Vec<ItemId> = Vec::new();
    pads.extend(path.start_pad);
    pads.extend(path.end_pad);
    let mut ends: Vec<Point> = Vec::new();
    for it in &path.items {
        if let PathItem::Line { line, .. } = it {
            ends.push(line.pts[0]);
            ends.push(*line.pts.last().unwrap());
        }
    }
    for e in ends {
        for id in hits_at(world, e) {
            if matches!(world.get(id), Some(Item::Solid(s)) if !s.edge && same_net(&s.net, &path_net(&path))) && !pads.contains(&id) {
                pads.push(id);
            }
        }
    }
    for pad in pads {
        let Some(item) = world.get(pad) else { continue };
        for it in &mut path.items {
            if let PathItem::Line { line, clipped } = it {
                optimise_trace_in(clipped, item, line.layer, false);
            }
        }
    }
    // clip the in-via portions of the lines on either side of a via
    let n = path.items.len();
    for idx in 0..n {
        let PathItem::Via(vid) = path.items[idx] else { continue };
        let Some(item) = world.get(vid) else { continue };
        for j in [idx.wrapping_sub(1), idx + 1] {
            if j < n {
                if let PathItem::Line { line, clipped } = &mut path.items[j] {
                    optimise_trace_in(clipped, item, line.layer, true);
                }
            }
        }
    }
    Some(path)
}

fn path_net(path: &TuningPath) -> Net {
    path.items.iter().find_map(|i| if let PathItem::Line { line, .. } = i { Some(line.net.clone()) } else { None }).unwrap_or(None)
}

impl TuningPath {
    /// [`TuningPath::length_nm`] as a count of nanometres: the studio's (`crates/cli`) signature until the length type's wave 3.
    pub fn length(&self, world: &Node, env: &LengthEnv) -> i64 {
        self.length_nm(world, env).get()
    }

    /// The origin line.
    pub fn origin_line(&self) -> &Line {
        match &self.items[self.origin] {
            PathItem::Line { line, .. } => line,
            PathItem::Via(_) => unreachable!("the origin is a line"),
        }
    }

    /// `lineLength( path, startPad, endPad )` (`CalculateRoutedPathLength`): the lines of the path (pad-clipped), plus the height of the stackup a
    /// via of the path spans when the board uses height in its lengths, plus the via KiCad infers where the path leaves a pad on the other side
    /// of the board (`inferViaInPad`). Nanometres.
    pub fn length_nm(&self, world: &Node, env: &LengthEnv) -> Nm {
        let mut total = Nm::ZERO;
        for (idx, it) in self.items.iter().enumerate() {
            match it {
                PathItem::Line { clipped, .. } => total += clipped.length_nm(),
                PathItem::Via(_) if env.rules.use_height_for_length_calcs && idx > 0 && idx + 1 < self.items.len() => {
                    // `getLengthDelayCalculationItems`: a via between two lines on different layers
                    let layer_of = |i: &PathItem| if let PathItem::Line { line, .. } = i { Some(line.layer) } else { None };
                    if let (Some(a), Some(b)) = (layer_of(&self.items[idx - 1]), layer_of(&self.items[idx + 1])) {
                        if a != b {
                            let name = |i: i32| env.rules.layers.get(i as usize).cloned().unwrap_or_default();
                            total += stackup_distance(&env.stackup, &env.rules.layers, &name(a), &name(b));
                        }
                    }
                }
                _ => {}
            }
        }
        if env.rules.use_height_for_length_calcs {
            // `inferViaInPad`: a pad not on the layer of the line that leaves it
            for (pad, item) in [(self.start_pad, self.items.first()), (self.end_pad, self.items.last())] {
                let (Some(pad), Some(PathItem::Line { line, .. })) = (pad, item) else { continue };
                let Some(Item::Solid(s)) = world.get(pad) else { continue };
                if !s.layers.overlaps_layer(line.layer) {
                    let pad_layer = if s.layers.overlaps_layer(0) { 0 } else { env.rules.layers.len() as i32 - 1 };
                    let name = |i: i32| env.rules.layers.get(i as usize).cloned().unwrap_or_default();
                    total += stackup_distance(&env.stackup, &env.rules.layers, &name(line.layer), &name(pad_layer));
                }
            }
        }
        total
    }
}

/// [`stackup_distance`] as a count of nanometres: the studio's (`crates/cli`) signature until the length type's wave 3.
pub fn stackup_distance_nm(stackup: &Stackup, copper: &[String], first: &str, second: &str) -> i64 {
    stackup_distance(stackup, copper, first, second).get()
}

/// `BOARD_STACKUP::GetLayerDistance( aFirstLayer, aSecondLayer )`: the thickness between two copper layers, half of an inner copper layer at
/// either end counted. Nanometres.
pub fn stackup_distance(stackup: &Stackup, copper: &[String], first: &str, second: &str) -> Nm {
    if first == second {
        return Nm::ZERO;
    }
    let pos = |n: &str| copper.iter().position(|c| c == n);
    let (mut a, mut b) = (first.to_string(), second.to_string());
    if let (Some(pa), Some(pb)) = (pos(first), pos(second)) {
        if pb < pa {
            std::mem::swap(&mut a, &mut b);
        }
    }
    let is_copper = |l: &eda_model::StackupLayer| l.kind.as_deref() == Some("copper") || (l.kind.is_none() && l.name.ends_with(".Cu"));
    let is_dielectric = |l: &eda_model::StackupLayer| matches!(l.kind.as_deref(), Some("core") | Some("prepreg") | Some("dielectric"));
    let inner = |n: &str| n != copper.first().map(String::as_str).unwrap_or("F.Cu") && n != copper.last().map(String::as_str).unwrap_or("B.Cu");
    let mut total = Nm::ZERO;
    let mut start = false;
    let mut half = false;
    for l in &stackup.layers {
        let layer: Option<&str> = if is_copper(l) { Some(l.name.as_str()) } else { None };
        if layer.is_none() && !is_dielectric(l) {
            continue; // silk, paste and mask
        }
        if !start && layer == Some(a.as_str()) {
            start = true;
            if inner(&a) {
                half = true;
            }
        } else if !start {
            continue;
        }
        if start && layer == Some(b.as_str()) && inner(&b) {
            half = true;
        }
        let thickness = ki_round_nm(l.thickness_mm.unwrap_or(0.0) * PCB_IU_PER_MM);
        total += if half { thickness / 2 } else { thickness };
        half = false;
        if layer == Some(b.as_str()) {
            break;
        }
    }
    total
}

/// The segment of `world` the point `p` is on the track of: `PCB_TRACK`s nearest to `p` of `net` on `layer` (a segment id), within `within`.
pub fn nearest_segment(world: &Node, p: Point, layer: i32, net: Option<&Net>, within: Nm) -> Option<(ItemId, Point)> {
    let id = world.segment_at(p, layer, net, Some(within), &[])?;
    match world.get(id)? {
        Item::Segment(s) => {
            let nearest = Seg::new(s.a, s.b).nearest_point(p);
            Some((id, nearest))
        }
        // `getSnappedStartPoint` for an arc: its nearer end
        Item::Arc(a) => Some((id, if crate::seg_ops::length(p, a.start()) <= crate::seg_ops::length(p, a.end()) { a.start() } else { a.end() })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Segment, Solid, Via};
    use crate::layer::LayerRange;

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn seg(net: &str, layer: i32, a: Point, b: Point) -> Item {
        Item::Segment(Segment { net: net_of(net), layer, a, b, width: mm(0.25), source_track: None, locked: false })
    }

    fn pad(net: &str, at: Point) -> Item {
        pad_on(net, at, LayerRange::single(0))
    }

    fn pad_on(net: &str, at: Point, layers: LayerRange) -> Item {
        Item::Solid(Solid { net: net_of(net), layers, pos: at, shape: Shape::Circle { c: at, r: mm(0.4) }, source: "U1.1".into(), edge: false })
    }

    fn env() -> LengthEnv {
        let mut rules = BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        LengthEnv { stackup: eda_model::rules::default_stackup(2, mm(1.6)), rules }
    }

    #[test]
    fn a_track_from_pad_to_pad_is_as_long_as_its_segments() {
        let mut w = Node::new();
        w.add(pad("S", p(0, 0)));
        w.add(pad("S", p(10_000_000, 5_000_000)));
        let a = w.add(seg("S", 0, p(0, 0), p(10_000_000, 0)));
        w.add(seg("S", 0, p(10_000_000, 0), p(10_000_000, 5_000_000)));
        let path = assemble_tuning_path(&w, a).expect("a path");
        assert_eq!(path.items.len(), 1);
        assert!(path.start_pad.is_some() && path.end_pad.is_some());
        assert_eq!(path.length_nm(&w, &env()), mm(15.0), "15 mm in nanometres, the ends on the pad centres");
        assert_eq!(path.origin_line().pts.len(), 3);
    }

    #[test]
    fn the_path_goes_on_through_a_via_and_counts_its_height() {
        let mut w = Node::new();
        w.add(pad("S", p(0, 0)));
        let a = w.add(seg("S", 0, p(0, 0), p(10_000_000, 0)));
        w.add(Item::Via(Via { net: net_of("S"), layers: LayerRange::new(0, 1), pos: p(10_000_000, 0), diameter: mm(0.6), drill: mm(0.3), source_via: Some("via".into()), locked: false, is_virtual: false }));
        w.add(seg("S", 1, p(10_000_000, 0), p(10_000_000, 8_000_000)));
        w.add(pad_on("S", p(10_000_000, 8_000_000), LayerRange::new(0, 1)));
        let path = assemble_tuning_path(&w, a).expect("a path");
        let kinds: Vec<&str> = path.items.iter().map(|i| if matches!(i, PathItem::Via(_)) { "via" } else { "line" }).collect();
        assert_eq!(kinds, ["line", "via", "line"]);
        // 10 mm + 8 mm, minus nothing; plus the 1.6 mm board less its two mask coats and half... (the stackup between the two copper layers)
        let l = path.length_nm(&w, &env());
        let h = stackup_distance(&env().stackup, &env().rules.layers, "F.Cu", "B.Cu");
        assert!(h > mm(1.4) && h < mm(1.6), "{h}");
        assert_eq!(l, mm(18.0) + h);
        // the same without the height setting
        let mut e = env();
        e.rules.use_height_for_length_calcs = false;
        assert_eq!(path.length_nm(&w, &e), mm(18.0));
        // a pad on the other side of the board from the line that leaves it is a via KiCad infers (`inferViaInPad`)
        let mut w2 = Node::new();
        let b = w2.add(seg("S", 1, p(0, 0), p(10_000_000, 0)));
        w2.add(pad("S", p(0, 0)));
        let inferred = assemble_tuning_path(&w2, b).expect("a path");
        assert_eq!(inferred.length_nm(&w2, &env()), mm(10.0) + h);
    }

    #[test]
    fn a_junction_ends_the_walk_where_the_line_ends() {
        let mut w = Node::new();
        let a = w.add(seg("S", 0, p(0, 0), p(10_000_000, 0)));
        w.add(seg("S", 0, p(10_000_000, 0), p(10_000_000, 5_000_000)));
        w.add(seg("S", 0, p(10_000_000, 0), p(20_000_000, 0)));
        let path = assemble_tuning_path(&w, a).expect("a path");
        assert_eq!(path.items.len(), 1);
        assert_eq!(path.length_nm(&w, &env()), mm(10.0));
        assert!(path.start_pad.is_none() && path.end_pad.is_none());
    }

    #[test]
    fn the_distance_between_inner_layers_counts_half_of_each_copper_layer() {
        let s = eda_model::rules::default_stackup(4, mm(1.6));
        let copper: Vec<String> = eda_model::rules::copper_layer_names(4);
        let all = stackup_distance(&s, &copper, "F.Cu", "B.Cu");
        let outer_to_in1 = stackup_distance(&s, &copper, "F.Cu", "In1.Cu");
        let in1_to_in2 = stackup_distance(&s, &copper, "In1.Cu", "In2.Cu");
        // the dielectric between F.Cu and In1.Cu plus half of In1's copper
        assert!(outer_to_in1 > Nm::ZERO && in1_to_in2 > Nm::ZERO && all > outer_to_in1 + in1_to_in2, "{outer_to_in1} {in1_to_in2} {all}");
        assert_eq!(stackup_distance(&s, &copper, "B.Cu", "F.Cu"), all, "the order of the layers does not matter");
        assert_eq!(stackup_distance(&s, &copper, "F.Cu", "F.Cu"), Nm::ZERO);
    }
}
