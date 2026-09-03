//! Wire-aware repair pass for schematic wires.
//!
//! `routing::route_edges` produces good wires for the common case but is
//! blind to other wires: its direct-path shortcut and channel approaches
//! can leave two *different-net* wires collinear on the same line. This
//! module finds those overlaps and re-routes the offending edge with a
//! grid maze router that treats symbol boxes, pin stubs and other nets'
//! wires as obstacles. Crossing another net's wire perpendicularly is
//! allowed (at a cost); running along it is not. Same-group (same net)
//! wires may share cells freely, which is what lets star spokes leave one
//! hub stub together.
//!
//! Everything here works in the solver's transposed frame, exactly like
//! `routing`, on the layout grid.

use crate::graph::{LayoutGraph, Point};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

const DIRS: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const STEP: i64 = 1;
const BEND: i64 = 3;
const CROSS: i64 = 6;
const NO_DIR: u8 = 4;
const MAX_EXPANSIONS: usize = 300_000;
/// Free margin around the node bounding box, in grid cells.
const MARGIN_CELLS: i64 = 8;

impl Cell {
    fn group(self) -> Option<usize> {
        match self {
            Cell::Wire { group, .. } | Cell::Vertex { group } => Some(group),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Free,
    /// Nothing may pass: box interior, foreign stub, foreign bend/vertex.
    Solid,
    /// Occupied by a wire of `group` running horizontally / vertically.
    Wire { group: usize, horizontal: bool },
    /// A wire end or bend of `group`: solid for every other group.
    Vertex { group: usize },
    /// Two different nets cross here: `h` runs horizontally, `v`
    /// vertically. Each may pass in its own direction; nobody else may.
    Cross { h: usize, v: usize },
}

struct Grid {
    grid: i64,
    min_x: i64,
    min_y: i64,
    cells_x: i64,
    cells_y: i64,
    cells: Vec<Cell>,
}

impl Grid {
    fn idx(&self, cx: i64, cy: i64) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.cells_x || cy >= self.cells_y {
            None
        } else {
            Some((cy * self.cells_x + cx) as usize)
        }
    }
    fn to_cell(&self, p: Point) -> (i64, i64) {
        ((p.x - self.min_x).div_euclid(self.grid), (p.y - self.min_y).div_euclid(self.grid))
    }
    fn to_point(&self, cx: i64, cy: i64) -> Point {
        Point { x: self.min_x + cx * self.grid, y: self.min_y + cy * self.grid }
    }
    fn get(&self, cx: i64, cy: i64) -> Cell {
        self.idx(cx, cy).map(|i| self.cells[i]).unwrap_or(Cell::Solid)
    }
    fn set(&mut self, cx: i64, cy: i64, c: Cell) {
        if let Some(i) = self.idx(cx, cy) {
            self.cells[i] = c;
        }
    }
    /// Mark a wire polyline: interior run cells as directional wire cells,
    /// vertices (ends and bends) as solid for other groups. Same-group
    /// cells are left as they are (already ours).
    fn mark_wire(&mut self, poly: &[Point], group: usize) {
        for w in poly.windows(2) {
            let (a, b) = (self.to_cell(w[0]), self.to_cell(w[1]));
            let horizontal = a.1 == b.1;
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    let new = match self.get(cx, cy) {
                        Cell::Free => Cell::Wire { group, horizontal },
                        Cell::Wire { group: g, .. } | Cell::Vertex { group: g } if g == group => Cell::Wire { group, horizontal },
                        // Perpendicular different-net wires: a crossing.
                        Cell::Wire { group: g, horizontal: h2 } if h2 != horizontal => {
                            if horizontal { Cell::Cross { h: group, v: g } } else { Cell::Cross { h: g, v: group } }
                        }
                        Cell::Cross { h, v } if h == group || v == group => Cell::Cross { h, v },
                        // Same-direction different nets (the overlap being
                        // repaired) or a third party: nobody passes.
                        _ => Cell::Solid,
                    };
                    self.set(cx, cy, new);
                }
            }
        }
        for (i, p) in poly.iter().enumerate() {
            let (cx, cy) = self.to_cell(*p);
            let is_bend = i > 0 && i + 1 < poly.len() && {
                let (a, c) = (poly[i - 1], poly[i + 1]);
                !((a.x == p.x && c.x == p.x) || (a.y == p.y && c.y == p.y))
            };
            if is_bend || i == 0 || i + 1 == poly.len() {
                let here = self.get(cx, cy);
                let new = if matches!(here, Cell::Free) || here.group() == Some(group) { Cell::Vertex { group } } else { Cell::Solid };
                self.set(cx, cy, new);
            }
        }
    }
}

/// Passability of `cell` for a wire of `group` moving in direction `dir`
/// (0..4). Returns the extra cost, or None if blocked.
fn enter_cost(cell: Cell, group: usize, dir: u8) -> Option<i64> {
    match cell {
        Cell::Free => Some(0),
        Cell::Solid => None,
        Cell::Vertex { group: g } => {
            if g == group {
                Some(0)
            } else {
                None
            }
        }
        Cell::Cross { h, v } => {
            let moving_horizontal = dir == 1 || dir == 3;
            if (moving_horizontal && h == group) || (!moving_horizontal && v == group) {
                Some(0)
            } else {
                None
            }
        }
        Cell::Wire { group: g, horizontal } => {
            if g == group {
                return Some(0);
            }
            let moving_horizontal = dir == 1 || dir == 3;
            if moving_horizontal == horizontal {
                None // would run along a foreign wire
            } else {
                Some(CROSS)
            }
        }
    }
}

type State = (i64, i64, u8);

#[derive(Eq, PartialEq)]
struct Item {
    f: i64,
    g: i64,
    s: State,
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.cmp(&self.f).then_with(|| o.s.cmp(&self.s))
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Search window: cells outside it are treated as solid. Keeps each
/// reroute local (and its state tables small).
struct Window {
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
}

/// Result of one windowed search: the path, or why it failed.
enum Search {
    Found(Vec<(i64, i64)>),
    /// Exhausted, but some expansion was cut off by the window edge, so a
    /// larger window may succeed.
    WindowBound,
    /// Exhausted without ever touching the window edge: enlarging the
    /// window cannot help.
    BoxedIn,
}

fn astar(grid: &Grid, win: &Window, group: usize, start: (i64, i64), goal: (i64, i64)) -> Search {
    let n = (win.w * win.h) as usize * 5;
    let index = |s: State| (((s.1 - win.y0) * win.w + (s.0 - win.x0)) as usize) * 5 + s.2 as usize;
    let inside = |x: i64, y: i64| x >= win.x0 && y >= win.y0 && x < win.x0 + win.w && y < win.y0 + win.h;
    let mut best = vec![i64::MAX; n];
    let mut from = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let h = |x: i64, y: i64| (x - goal.0).abs() + (y - goal.1).abs();
    let s0: State = (start.0, start.1, NO_DIR);
    best[index(s0)] = 0;
    heap.push(Item { f: h(start.0, start.1), g: 0, s: s0 });
    let mut expansions = 0;
    let mut end: Option<State> = None;
    let mut touched_edge = false;
    while let Some(Item { g, s, .. }) = heap.pop() {
        if g > best[index(s)] {
            continue;
        }
        if (s.0, s.1) == goal {
            end = Some(s);
            break;
        }
        expansions += 1;
        if expansions > MAX_EXPANSIONS {
            if std::env::var_os("EDA_MAZE_DEBUG").is_some() {
                eprintln!("maze: expansion budget hit");
            }
            return Search::WindowBound;
        }
        let here = grid.get(s.0, s.1);
        // Inside a foreign crossing cell we must keep going straight.
        let must_go_straight = (matches!(here, Cell::Wire { group: g2, .. } if g2 != group) || matches!(here, Cell::Cross { .. })) && s.2 != NO_DIR;
        for (d, (dx, dy)) in DIRS.iter().enumerate() {
            let d = d as u8;
            if must_go_straight && d != s.2 {
                continue;
            }
            let (nx, ny) = (s.0 + dx, s.1 + dy);
            if !inside(nx, ny) {
                touched_edge = true;
                continue;
            }
            let Some(extra) = enter_cost(grid.get(nx, ny), group, d) else { continue };
            let bend = if s.2 == NO_DIR || s.2 == d { 0 } else { BEND };
            let ng = g + STEP + extra + bend;
            let ns: State = (nx, ny, d);
            if ng < best[index(ns)] {
                best[index(ns)] = ng;
                from[index(ns)] = index(s) as u32;
                heap.push(Item { f: ng + h(nx, ny), g: ng, s: ns });
            }
        }
    }
    let Some(end) = end else {
        if std::env::var_os("EDA_MAZE_DEBUG").is_some() {
            let sc = grid.get(start.0, start.1);
            let gc = grid.get(goal.0, goal.1);
            let show = |c: Cell| match c { Cell::Free => "free".to_string(), Cell::Solid => "solid".into(), Cell::Wire { group, horizontal } => format!("wire g{group} h{horizontal}"), Cell::Vertex { group } => format!("vertex g{group}"), Cell::Cross { h, v } => format!("cross h{h} v{v}") };
            let nb: Vec<String> = DIRS.iter().map(|(dx, dy)| show(grid.get(start.0 + dx, start.1 + dy))).collect();
            eprintln!("maze: exhausted after {expansions} expansions, win {}x{}; start={} goal={} start-neighbours={:?}", win.w, win.h, show(sc), show(gc), nb);
        }
        return if touched_edge { Search::WindowBound } else { Search::BoxedIn };
    };
    let mut path = vec![(end.0, end.1)];
    let mut cur = index(end);
    while from[cur] != u32::MAX {
        cur = from[cur] as usize;
        let c = cur / 5;
        let (cx, cy) = (win.x0 + (c as i64) % win.w, win.y0 + (c as i64) / win.w);
        path.push((cx, cy));
    }
    path.reverse();
    path.dedup();
    Search::Found(path)
}

/// Axis-aligned segments touch or cross (closed segments).
fn seg_touches_seg(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (ax0, ax1) = (a.x.min(b.x), a.x.max(b.x));
    let (ay0, ay1) = (a.y.min(b.y), a.y.max(b.y));
    let (cx0, cx1) = (c.x.min(d.x), c.x.max(d.x));
    let (cy0, cy1) = (c.y.min(d.y), c.y.max(d.y));
    ax0 <= cx1 && cx0 <= ax1 && ay0 <= cy1 && cy0 <= ay1
}

fn collinear_overlap(a: Point, b: Point, c: Point, d: Point) -> bool {
    if a.x == b.x && c.x == d.x && a.x == c.x {
        let (a0, a1) = (a.y.min(b.y), a.y.max(b.y));
        let (b0, b1) = (c.y.min(d.y), c.y.max(d.y));
        return a0.max(b0) < a1.min(b1);
    }
    if a.y == b.y && c.y == d.y && a.y == c.y {
        let (a0, a1) = (a.x.min(b.x), a.x.max(b.x));
        let (b0, b1) = (c.x.min(d.x), c.x.max(d.x));
        return a0.max(b0) < a1.min(b1);
    }
    false
}

/// Indices of edges that need repair, sorted ascending: wires that overlap
/// collinearly with a different group's wire (only the later edge of each
/// pair, so the earlier one keeps its wire), and wires that cut through
/// any node box.
pub fn overlapping_edges(g: &LayoutGraph, node_top_left: &[Point], polys: &[Vec<Point>]) -> Vec<usize> {
    let mut bad = std::collections::BTreeSet::new();
    for (i, poly) in polys.iter().enumerate() {
        let through_box = poly.windows(2).any(|s| {
            g.nodes.iter().enumerate().any(|(id, n)| crate::routing::seg_intersects_box(s[0], s[1], node_top_left[id], n.width, n.height))
        });
        // A wire running over another port's stub or tip (a foreign pin).
        let own = [(g.edges[i].from.node, g.edges[i].from.port), (g.edges[i].to.node, g.edges[i].to.port)];
        let through_stub = poly.windows(2).any(|s| {
            g.nodes.iter().enumerate().any(|(id, n)| {
                (0..n.ports.len()).any(|pi| {
                    if own.contains(&(id, pi)) {
                        return false;
                    }
                    let pp = n.port_point(node_top_left[id], pi);
                    let tip = n.stub_tip(node_top_left[id], pi);
                    seg_touches_seg(s[0], s[1], pp, tip)
                })
            })
        });
        if through_box || through_stub {
            bad.insert(i);
        }
    }
    for i in 0..polys.len() {
        for j in i + 1..polys.len() {
            if g.edges[i].group == g.edges[j].group || shares_pin(&g.edges[i], &g.edges[j]) {
                continue;
            }
            let hit = polys[i].windows(2).any(|s| polys[j].windows(2).any(|t| collinear_overlap(s[0], s[1], t[0], t[1])));
            if hit {
                bad.insert(j);
            }
        }
    }
    bad.into_iter().collect()
}

/// Two edges on one pin are one net by construction (a pin carries one
/// net); synthetic graphs may not say so via `group`.
fn shares_pin(a: &crate::graph::Edge, b: &crate::graph::Edge) -> bool {
    let ends = |e: &crate::graph::Edge| [(e.from.node, e.from.port), (e.to.node, e.to.port)];
    ends(a).iter().any(|x| ends(b).contains(x))
}

fn simplify(path: &[(i64, i64)], grid: &Grid) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::new();
    for &(cx, cy) in path {
        let p = grid.to_point(cx, cy);
        if pts.len() >= 2 {
            let a = pts[pts.len() - 2];
            let b = pts[pts.len() - 1];
            if (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y) {
                pts.pop();
            }
        }
        pts.push(p);
    }
    pts
}

/// Re-routes every edge in `overlapping_edges` around the others. Edges
/// the maze cannot route keep their original wire (the gate will report
/// them). Returns the number of edges rerouted.
pub fn repair(g: &LayoutGraph, node_top_left: &[Point], polys: &mut [Vec<Point>], grid_um: i64) -> usize {
    let t0 = std::time::Instant::now();
    let bad = overlapping_edges(g, node_top_left, polys);
    let debug = std::env::var_os("EDA_MAZE_DEBUG").is_some();
    if debug {
        eprintln!("maze: {} edges, {} overlapping, detect {:?}", polys.len(), bad.len(), t0.elapsed());
    }
    if bad.is_empty() {
        return 0;
    }
    let grid_um = grid_um.max(1);
    // Extent: nodes and existing wires, plus margin.
    let mut min_x = i64::MAX;
    let mut min_y = i64::MAX;
    let mut max_x = i64::MIN;
    let mut max_y = i64::MIN;
    for (id, n) in g.nodes.iter().enumerate() {
        let tl = node_top_left[id];
        min_x = min_x.min(tl.x);
        min_y = min_y.min(tl.y);
        max_x = max_x.max(tl.x + n.width);
        max_y = max_y.max(tl.y + n.height);
    }
    for p in polys.iter().flatten() {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let min_x = min_x.div_euclid(grid_um) * grid_um - MARGIN_CELLS * grid_um;
    let min_y = min_y.div_euclid(grid_um) * grid_um - MARGIN_CELLS * grid_um;
    let cells_x = (max_x - min_x) / grid_um + MARGIN_CELLS + 1;
    let cells_y = (max_y - min_y) / grid_um + MARGIN_CELLS + 1;
    let mut grid = Grid { grid: grid_um, min_x, min_y, cells_x, cells_y, cells: vec![Cell::Free; (cells_x * cells_y) as usize] };

    // Boxes and stubs are solid.
    for (id, n) in g.nodes.iter().enumerate() {
        let tl = node_top_left[id];
        let (x0, y0) = grid.to_cell(tl);
        let (x1, y1) = grid.to_cell(Point { x: tl.x + n.width, y: tl.y + n.height });
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                grid.set(cx, cy, Cell::Solid);
            }
        }
        for pi in 0..n.ports.len() {
            let pp = n.port_point(tl, pi);
            let tip = n.stub_tip(tl, pi);
            let (a, b) = (grid.to_cell(pp), grid.to_cell(tip));
            for cy in a.1.min(b.1)..=a.1.max(b.1) {
                for cx in a.0.min(b.0)..=a.0.max(b.0) {
                    grid.set(cx, cy, Cell::Solid);
                }
            }
        }
    }
    let base_cells = grid.cells.clone();
    let mut rerouted = 0;
    for &e in &bad {
        let edge = &g.edges[e];
        let group = edge.group;
        // Rebuild occupancy from every wire except this one, so masked
        // overlaps between the others are never lost.
        grid.cells.clone_from(&base_cells);
        for (i, poly) in polys.iter().enumerate() {
            if i != e {
                grid.mark_wire(poly, g.edges[i].group);
            }
        }
        let a = g.nodes[edge.from.node].stub_tip(node_top_left[edge.from.node], edge.from.port);
        let b = g.nodes[edge.to.node].stub_tip(node_top_left[edge.to.node], edge.to.port);
        let (ca, cb) = (grid.to_cell(a), grid.to_cell(b));
        // Free the two endpoint tip cells and their stubs for this group.
        let free_stub = |grid: &mut Grid, node: usize, port: usize| {
            let n = &g.nodes[node];
            let pp = n.port_point(node_top_left[node], port);
            let tip = n.stub_tip(node_top_left[node], port);
            let (p, t) = (grid.to_cell(pp), grid.to_cell(tip));
            for cy in p.1.min(t.1)..=p.1.max(t.1) {
                for cx in p.0.min(t.0)..=p.0.max(t.0) {
                    grid.set(cx, cy, Cell::Wire { group, horizontal: false });
                }
            }
            grid.set(t.0, t.1, Cell::Wire { group, horizontal: false });
        };
        free_stub(&mut grid, edge.from.node, edge.from.port);
        free_stub(&mut grid, edge.to.node, edge.to.port);

        let mut found = None;
        for margin in [6i64, 14, 30, i64::MAX] {
            let (x0, y0, x1, y1) = if margin == i64::MAX {
                (0, 0, grid.cells_x - 1, grid.cells_y - 1)
            } else {
                (
                    (ca.0.min(cb.0) - margin).max(0),
                    (ca.1.min(cb.1) - margin).max(0),
                    (ca.0.max(cb.0) + margin).min(grid.cells_x - 1),
                    (ca.1.max(cb.1) + margin).min(grid.cells_y - 1),
                )
            };
            let win = Window { x0, y0, w: x1 - x0 + 1, h: y1 - y0 + 1 };
            match astar(&grid, &win, group, ca, cb) {
                Search::Found(path) => {
                    found = Some(path);
                    break;
                }
                Search::BoxedIn => break,
                Search::WindowBound => {}
            }
            if margin == i64::MAX {
                break;
            }
        }
        if std::env::var_os("EDA_MAZE_DEBUG").is_some() {
            eprintln!("maze: edge {e} group {group} {a:?}->{b:?} rerouted={}", found.is_some());
        }
        if let Some(path) = found {
            let mut poly = simplify(&path, &grid);
            // Pin the exact stub tips; if a tip is off the maze grid, add
            // an orthogonal jog rather than a diagonal.
            if let Some(&p0) = poly.first() {
                if p0 != a {
                    let mut v = vec![a];
                    if a.x != p0.x && a.y != p0.y {
                        v.push(Point { x: p0.x, y: a.y });
                    }
                    v.extend(poly.into_iter());
                    poly = v;
                }
            }
            if let Some(&pl) = poly.last() {
                if pl != b {
                    if b.x != pl.x && b.y != pl.y {
                        poly.push(Point { x: pl.x, y: b.y });
                    }
                    poly.push(b);
                }
            }
            polys[e] = poly;
            rerouted += 1;
        }
    }
    if debug {
        eprintln!("maze: rerouted {rerouted}/{} in {:?}", bad.len(), t0.elapsed());
    }
    rerouted
}
