//! A faithful port of `Clipper2Lib/include/clipper2/clipper.rectclip.h` and
//! `Clipper2Lib/src/clipper.rectclip.cpp` -- `RectClip64`, the fast
//! rectangular-clip algorithm. `RectClipLines64` (the open-path variant) is
//! not ported: it is a separate, smaller class that nothing in KiCad's
//! `pcbnew`/`libs/kimath` calls (confirmed by grepping the KiCad source
//! tree for `RectClip`, which turns up no call sites at all -- `RectClip64`
//! itself is ported here only because the porting task explicitly names it
//! as part of a faithful Clipper2 port, for `SHAPE_POLY_SET` code that may
//! use it later).
//!
//! Same arena-of-indices treatment as `engine.rs`: `OutPt2`'s `next`/`prev`
//! raw pointers become indices into an append-only `op_container: Vec<OutPt2>`
//! arena (the original literally uses a `std::deque<OutPt2>` for the same
//! "stable addresses, never individually freed" reason), and the `edge: *mut
//! OutPt2List` back-pointer (used only to test "is this op already on some
//! edge list" and to remove it from one) becomes `edge: Option<usize>`, an
//! index into the `edges` array of eight `Vec<OutPtIdx>` lists.

use crate::core::*;

type OutPtIdx = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Location {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
    Inside = 4,
}

impl Location {
    fn from_i32(v: i32) -> Location {
        match v.rem_euclid(4) {
            0 => Location::Left,
            1 => Location::Top,
            2 => Location::Right,
            _ => Location::Bottom,
        }
    }
    fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone)]
struct OutPt2 {
    pt: Point64,
    owner_idx: usize,
    /// index into `RectClip64::edges` (0..8), `None` == not yet on an edge list
    edge: Option<usize>,
    next: OutPtIdx,
    prev: OutPtIdx,
}

#[inline]
fn is_horizontal(pt1: Point64, pt2: Point64) -> bool {
    pt1.y == pt2.y
}

fn path1_contains_path2(path1: &[Point64], path2: &[Point64]) -> bool {
    let mut io_count = 0i32;
    // precondition: no (significant) overlap
    for &pt in path2 {
        match point_in_polygon(pt, path1) {
            PointInPolygonResult::IsOutside => io_count += 1,
            PointInPolygonResult::IsInside => io_count -= 1,
            PointInPolygonResult::IsOn => continue,
        }
        if io_count.abs() > 1 {
            break;
        }
    }
    io_count <= 0
}

/// `GetLocation`, returning `(location, is_on_boundary)` -- the negation of
/// the original's `bool` return (`true` meant "cleanly determined, not on
/// the rect's boundary"; `false` meant "sits exactly on one of the rect's
/// four edges"). Spelling it as `is_on_boundary` avoids a double-negative
/// (`!GetLocation(...)`) at every call site.
fn get_location(rec: &Rect64, pt: Point64) -> (Location, bool) {
    if pt.x == rec.left && pt.y >= rec.top && pt.y <= rec.bottom {
        return (Location::Left, true);
    }
    if pt.x == rec.right && pt.y >= rec.top && pt.y <= rec.bottom {
        return (Location::Right, true);
    }
    if pt.y == rec.top && pt.x >= rec.left && pt.x <= rec.right {
        return (Location::Top, true);
    }
    if pt.y == rec.bottom && pt.x >= rec.left && pt.x <= rec.right {
        return (Location::Bottom, true);
    }
    let loc = if pt.x < rec.left {
        Location::Left
    } else if pt.x > rec.right {
        Location::Right
    } else if pt.y < rec.top {
        Location::Top
    } else if pt.y > rec.bottom {
        Location::Bottom
    } else {
        Location::Inside
    };
    (loc, false)
}

fn get_segment_intersection(p1: Point64, p2: Point64, p3: Point64, p4: Point64) -> Option<Point64> {
    let res1 = cross_product(p1, p3, p4);
    let res2 = cross_product(p2, p3, p4);
    if res1 == 0.0 {
        if res2 == 0.0 {
            return None; // collinear
        }
        if p1 == p3 || p1 == p4 {
            return Some(p1);
        }
        return if is_horizontal(p3, p4) {
            if (p1.x > p3.x) == (p1.x < p4.x) { Some(p1) } else { None }
        } else if (p1.y > p3.y) == (p1.y < p4.y) {
            Some(p1)
        } else {
            None
        };
    } else if res2 == 0.0 {
        if p2 == p3 || p2 == p4 {
            return Some(p2);
        }
        return if is_horizontal(p3, p4) {
            if (p2.x > p3.x) == (p2.x < p4.x) { Some(p2) } else { None }
        } else if (p2.y > p3.y) == (p2.y < p4.y) {
            Some(p2)
        } else {
            None
        };
    }
    if (res1 > 0.0) == (res2 > 0.0) {
        return None;
    }

    let res3 = cross_product(p3, p1, p2);
    let res4 = cross_product(p4, p1, p2);
    if res3 == 0.0 {
        if p3 == p1 || p3 == p2 {
            return Some(p3);
        }
        return if is_horizontal(p1, p2) {
            if (p3.x > p1.x) == (p3.x < p2.x) { Some(p3) } else { None }
        } else if (p3.y > p1.y) == (p3.y < p2.y) {
            Some(p3)
        } else {
            None
        };
    } else if res4 == 0.0 {
        if p4 == p1 || p4 == p2 {
            return Some(p4);
        }
        return if is_horizontal(p1, p2) {
            if (p4.x > p1.x) == (p4.x < p2.x) { Some(p4) } else { None }
        } else if (p4.y > p1.y) == (p4.y < p2.y) {
            Some(p4)
        } else {
            None
        };
    }
    if (res3 > 0.0) == (res4 > 0.0) {
        return None;
    }

    // segments must intersect to get here
    get_intersect_point(p1, p2, p3, p4)
}

/// `GetIntersection`: the intersection closest to `p`, walking the rect's 4
/// edges starting from `loc`. Returns `None` (location left unchanged, as
/// the original documents) when there's no intersection.
fn get_intersection(rect_path: &[Point64; 4], p: Point64, p2: Point64, loc: Location) -> Option<(Point64, Location)> {
    match loc {
        Location::Left => {
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[3]) {
                return Some((ip, loc));
            }
            if p.y < rect_path[0].y {
                if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[1]) {
                    return Some((ip, Location::Top));
                }
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[2], rect_path[3]) {
                return Some((ip, Location::Bottom));
            }
            None
        }
        Location::Top => {
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[1]) {
                return Some((ip, loc));
            }
            if p.x < rect_path[0].x {
                if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[3]) {
                    return Some((ip, Location::Left));
                }
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[1], rect_path[2]) {
                return Some((ip, Location::Right));
            }
            None
        }
        Location::Right => {
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[1], rect_path[2]) {
                return Some((ip, loc));
            }
            if p.y < rect_path[1].y {
                if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[1]) {
                    return Some((ip, Location::Top));
                }
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[2], rect_path[3]) {
                return Some((ip, Location::Bottom));
            }
            None
        }
        Location::Bottom => {
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[2], rect_path[3]) {
                return Some((ip, loc));
            }
            if p.x < rect_path[3].x {
                if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[3]) {
                    return Some((ip, Location::Left));
                }
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[1], rect_path[2]) {
                return Some((ip, Location::Right));
            }
            None
        }
        Location::Inside => {
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[3]) {
                return Some((ip, Location::Left));
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[0], rect_path[1]) {
                return Some((ip, Location::Top));
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[1], rect_path[2]) {
                return Some((ip, Location::Right));
            }
            if let Some(ip) = get_segment_intersection(p, p2, rect_path[2], rect_path[3]) {
                return Some((ip, Location::Bottom));
            }
            None
        }
    }
}

#[inline]
fn get_adjacent_location(loc: Location, is_clockwise: bool) -> Location {
    let delta = if is_clockwise { 1 } else { 3 };
    Location::from_i32(loc.as_i32() + delta)
}

#[inline]
fn heading_clockwise(prev: Location, curr: Location) -> bool {
    (prev.as_i32() + 1) % 4 == curr.as_i32()
}

#[inline]
fn are_opposites(prev: Location, curr: Location) -> bool {
    (prev.as_i32() - curr.as_i32()).abs() == 2
}

fn is_clockwise(prev: Location, curr: Location, prev_pt: Point64, curr_pt: Point64, rect_mp: Point64) -> bool {
    if are_opposites(prev, curr) {
        cross_product(prev_pt, rect_mp, curr_pt) < 0.0
    } else {
        heading_clockwise(prev, curr)
    }
}

#[inline]
fn get_edges_for_pt(pt: Point64, rec: &Rect64) -> u32 {
    let mut result = 0u32;
    if pt.x == rec.left {
        result = 1;
    } else if pt.x == rec.right {
        result = 4;
    }
    if pt.y == rec.top {
        result += 2;
    } else if pt.y == rec.bottom {
        result += 8;
    }
    result
}

#[inline]
fn is_heading_clockwise(pt1: Point64, pt2: Point64, edge_idx: usize) -> bool {
    match edge_idx {
        0 => pt2.y < pt1.y,
        1 => pt2.x > pt1.x,
        2 => pt2.y > pt1.y,
        _ => pt2.x < pt1.x,
    }
}

#[inline]
fn has_horz_overlap(left1: Point64, right1: Point64, left2: Point64, right2: Point64) -> bool {
    left1.x < right2.x && right1.x > left2.x
}

#[inline]
fn has_vert_overlap(top1: Point64, bottom1: Point64, top2: Point64, bottom2: Point64) -> bool {
    top1.y < bottom2.y && bottom1.y > top2.y
}

pub struct RectClip64 {
    rect: Rect64,
    rect_as_path: [Point64; 4],
    rect_mp: Point64,
    path_bounds: Rect64,
    op_container: Vec<OutPt2>,
    results: Vec<Option<OutPtIdx>>,
    edges: [Vec<OutPtIdx>; 8],
    start_locs: Vec<Location>,
}

impl RectClip64 {
    pub fn new(rect: Rect64) -> Self {
        let p = rect.as_path();
        RectClip64 {
            rect,
            rect_as_path: [p[0], p[1], p[2], p[3]],
            rect_mp: rect.mid_point(),
            path_bounds: Rect64::default(),
            op_container: Vec::new(),
            results: Vec::new(),
            edges: Default::default(),
            start_locs: Vec::new(),
        }
    }

    fn add(&mut self, pt: Point64, start_new: bool) -> OutPtIdx {
        // only called from `execute_internal`; later splitting/rejoining
        // never creates additional `OutPt2`s, only changes `results`' count.
        let curr_idx = self.results.len();
        if curr_idx == 0 || start_new {
            let idx = self.op_container.len();
            self.op_container.push(OutPt2 { pt, owner_idx: 0, edge: None, next: idx, prev: idx });
            self.results.push(Some(idx));
            idx
        } else {
            let prev_op = self.results[curr_idx - 1].unwrap();
            if self.op_container[prev_op].pt == pt {
                return prev_op;
            }
            let idx = self.op_container.len();
            let next = self.op_container[prev_op].next;
            self.op_container.push(OutPt2 { pt, owner_idx: curr_idx - 1, edge: None, next, prev: prev_op });
            self.op_container[next].prev = idx;
            self.op_container[prev_op].next = idx;
            self.results[curr_idx - 1] = Some(idx);
            idx
        }
    }

    fn add_corner_between(&mut self, prev: Location, curr: Location) {
        if heading_clockwise(prev, curr) {
            self.add(self.rect_as_path[prev.as_i32() as usize], false);
        } else {
            self.add(self.rect_as_path[curr.as_i32() as usize], false);
        }
    }

    fn add_corner(&mut self, loc: &mut Location, is_clockwise: bool) {
        if is_clockwise {
            self.add(self.rect_as_path[loc.as_i32() as usize], false);
            *loc = get_adjacent_location(*loc, true);
        } else {
            *loc = get_adjacent_location(*loc, false);
            self.add(self.rect_as_path[loc.as_i32() as usize], false);
        }
    }

    fn get_next_location(&mut self, path: &[Point64], loc: &mut Location, i: &mut i32, high_i: i32) {
        match *loc {
            Location::Left => {
                while *i <= high_i && path[*i as usize].x <= self.rect.left {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                *loc = if path[*i as usize].x >= self.rect.right {
                    Location::Right
                } else if path[*i as usize].y <= self.rect.top {
                    Location::Top
                } else if path[*i as usize].y >= self.rect.bottom {
                    Location::Bottom
                } else {
                    Location::Inside
                };
            }
            Location::Top => {
                while *i <= high_i && path[*i as usize].y <= self.rect.top {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                *loc = if path[*i as usize].y >= self.rect.bottom {
                    Location::Bottom
                } else if path[*i as usize].x <= self.rect.left {
                    Location::Left
                } else if path[*i as usize].x >= self.rect.right {
                    Location::Right
                } else {
                    Location::Inside
                };
            }
            Location::Right => {
                while *i <= high_i && path[*i as usize].x >= self.rect.right {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                *loc = if path[*i as usize].x <= self.rect.left {
                    Location::Left
                } else if path[*i as usize].y <= self.rect.top {
                    Location::Top
                } else if path[*i as usize].y >= self.rect.bottom {
                    Location::Bottom
                } else {
                    Location::Inside
                };
            }
            Location::Bottom => {
                while *i <= high_i && path[*i as usize].y >= self.rect.bottom {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                *loc = if path[*i as usize].y <= self.rect.top {
                    Location::Top
                } else if path[*i as usize].x <= self.rect.left {
                    Location::Left
                } else if path[*i as usize].x >= self.rect.right {
                    Location::Right
                } else {
                    Location::Inside
                };
            }
            Location::Inside => {
                while *i <= high_i {
                    let p = path[*i as usize];
                    if p.x < self.rect.left {
                        *loc = Location::Left;
                    } else if p.x > self.rect.right {
                        *loc = Location::Right;
                    } else if p.y > self.rect.bottom {
                        *loc = Location::Bottom;
                    } else if p.y < self.rect.top {
                        *loc = Location::Top;
                    } else {
                        self.add(p, false);
                        *i += 1;
                        continue;
                    }
                    break; // inner loop
                }
            }
        }
    }

    fn execute_internal(&mut self, path: &[Point64]) {
        self.results.clear();
        self.op_container.clear();
        for e in self.edges.iter_mut() {
            e.clear();
        }
        self.start_locs.clear();

        let high_i = (path.len() - 1) as i32;
        let (mut prev, mut loc);
        let mut crossing_loc = Location::Inside;
        let mut first_cross = Location::Inside;
        // `i` starts at (and, bar the on-boundary scan-back below, which
        // itself resets it to 0 too, stays at) 0 -- *not* `high_i`; only
        // `loc`/`prev` are seeded from `path[high_i]`.
        let mut i = 0i32;
        let (loc0, is_on_boundary0) = get_location(&self.rect, path[high_i as usize]);
        if !is_on_boundary0 {
            loc = loc0;
        } else {
            i = high_i - 1;
            prev = Location::Inside;
            let mut found = false;
            while i >= 0 {
                let (p, on_b) = get_location(&self.rect, path[i as usize]);
                if !on_b {
                    prev = p;
                    found = true;
                    break;
                }
                i -= 1;
            }
            if !found {
                // all of path must be inside rect
                for &pt in path {
                    self.add(pt, false);
                }
                return;
            }
            loc = if prev == Location::Inside { Location::Inside } else { loc0 };
            i = 0;
        }
        let starting_loc = loc;

        ///////////////////////////////////////////////////
        while i <= high_i {
            prev = loc;
            let crossing_prev = crossing_loc;

            self.get_next_location(path, &mut loc, &mut i, high_i);

            if i > high_i {
                break;
            }
            let prev_pt = if i != 0 { path[(i - 1) as usize] } else { path[high_i as usize] };

            crossing_loc = loc;
            let isect = get_intersection(&self.rect_as_path, path[i as usize], prev_pt, crossing_loc);
            let (ip, new_crossing_loc) = match isect {
                Some((ip, l)) => (ip, l),
                None => {
                    // ie remaining outside
                    if crossing_prev == Location::Inside {
                        let is_clockw = is_clockwise(prev, loc, prev_pt, path[i as usize], self.rect_mp);
                        loop {
                            self.start_locs.push(prev);
                            prev = get_adjacent_location(prev, is_clockw);
                            if prev == loc {
                                break;
                            }
                        }
                        crossing_loc = crossing_prev; // still not crossed
                    } else if prev != Location::Inside && prev != loc {
                        // `AddCorner(Location& loc, bool isClockwise)` -- the
                        // mutating overload, walking `prev` itself around
                        // the rect corners until it reaches `loc`.
                        let is_clockw = is_clockwise(prev, loc, prev_pt, path[i as usize], self.rect_mp);
                        loop {
                            self.add_corner(&mut prev, is_clockw);
                            if prev == loc {
                                break;
                            }
                        }
                    }
                    i += 1;
                    continue;
                }
            };
            crossing_loc = new_crossing_loc;

            ////////////////////////////////////////////////////
            // we must be crossing the rect boundary to get here
            ////////////////////////////////////////////////////

            if loc == Location::Inside {
                // path must be entering rect
                if first_cross == Location::Inside {
                    first_cross = crossing_loc;
                    self.start_locs.push(prev);
                } else if prev != crossing_loc {
                    let is_clockw = is_clockwise(prev, crossing_loc, prev_pt, path[i as usize], self.rect_mp);
                    loop {
                        self.add_corner(&mut prev, is_clockw);
                        if prev == crossing_loc {
                            break;
                        }
                    }
                }
            } else if prev != Location::Inside {
                // passing right through rect: `ip` is the second intersect
                // point, `ip2` the first
                loc = prev;
                let (ip2, loc_after) = get_intersection(&self.rect_as_path, prev_pt, path[i as usize], loc).unwrap();
                loc = loc_after;
                if crossing_prev != Location::Inside && crossing_prev != loc {
                    self.add_corner_between(crossing_prev, loc);
                }

                if first_cross == Location::Inside {
                    first_cross = loc;
                    self.start_locs.push(prev);
                }

                loc = crossing_loc;
                self.add(ip2, false);
                if ip == ip2 {
                    // it's very likely that path[i] is on rect
                    let (loc_pi, _) = get_location(&self.rect, path[i as usize]);
                    loc = loc_pi;
                    self.add_corner_between(crossing_loc, loc);
                    crossing_loc = loc;
                    continue;
                }
            } else {
                // path must be exiting rect
                loc = crossing_loc;
                if first_cross == Location::Inside {
                    first_cross = crossing_loc;
                }
            }

            self.add(ip, false);
        } //while i <= high_i
        ///////////////////////////////////////////////////

        if first_cross == Location::Inside {
            // path never intersects
            if starting_loc != Location::Inside {
                // path is outside rect, but may still contain it
                if self.path_bounds.contains_rect(&self.rect) && path1_contains_path2(path, &self.rect_as_path) {
                    // the path does fully contain rect: add rect to the solution
                    for j in 0..4 {
                        self.add(self.rect_as_path[j], false);
                        // we may well need to do some splitting later, so
                        let r0 = self.results[0].unwrap();
                        self.add_to_edge(j * 2, r0);
                    }
                }
            }
        } else if loc != Location::Inside && (loc != first_cross || self.start_locs.len() > 2) {
            if !self.start_locs.is_empty() {
                prev = loc;
                let locs = std::mem::take(&mut self.start_locs);
                for loc2 in locs {
                    if prev == loc2 {
                        continue;
                    }
                    self.add_corner_between_hc(prev, loc2);
                    prev = loc2;
                }
                loc = prev;
            }
            if loc != first_cross {
                self.add_corner_between_hc(loc, first_cross);
            }
        }
    }

    /// `AddCorner(loc, HeadingClockwise(loc, loc2))`: the original overload
    /// resolution picks the `(Location&, bool)` form here (mutating `loc`
    /// isn't needed by any caller afterwards, but kept for fidelity).
    fn add_corner_between_hc(&mut self, loc: Location, loc2: Location) {
        let mut l = loc;
        self.add_corner(&mut l, heading_clockwise(loc, loc2));
    }

    fn add_to_edge(&mut self, edge_idx: usize, op: OutPtIdx) {
        if self.op_container[op].edge.is_some() {
            return;
        }
        self.op_container[op].edge = Some(edge_idx);
        self.edges[edge_idx].push(op);
    }

    fn uncouple_edge(&mut self, op: OutPtIdx) {
        let edge_idx = match self.op_container[op].edge {
            Some(e) => e,
            None => return,
        };
        for slot in self.edges[edge_idx].iter_mut() {
            if *slot == op {
                // NB: the original leaves a `nullptr` hole in the edge
                // vector; since our indices are `usize` (no null), we
                // instead use `usize::MAX` as that same "hole" sentinel --
                // `TidyEdges` below treats it exactly like the original
                // treats a null entry.
                *slot = usize::MAX;
                break;
            }
        }
        self.op_container[op].edge = None;
    }

    fn set_new_owner(&mut self, op: OutPtIdx, new_idx: usize) {
        self.op_container[op].owner_idx = new_idx;
        let mut op2 = self.op_container[op].next;
        while op2 != op {
            self.op_container[op2].owner_idx = new_idx;
            op2 = self.op_container[op2].next;
        }
    }

    fn unlink_op(&mut self, op: OutPtIdx) -> Option<OutPtIdx> {
        if self.op_container[op].next == op {
            return None;
        }
        let (p, n) = (self.op_container[op].prev, self.op_container[op].next);
        self.op_container[p].next = n;
        self.op_container[n].prev = p;
        Some(n)
    }

    fn unlink_op_back(&mut self, op: OutPtIdx) -> Option<OutPtIdx> {
        if self.op_container[op].next == op {
            return None;
        }
        let (p, n) = (self.op_container[op].prev, self.op_container[op].next);
        self.op_container[p].next = n;
        self.op_container[n].prev = p;
        Some(p)
    }

    fn check_edges(&mut self) {
        for i in 0..self.results.len() {
            let mut op = match self.results[i] {
                Some(o) => o,
                None => continue,
            };
            let mut op2 = op;
            loop {
                let (p, n) = (self.op_container[op2].prev, self.op_container[op2].next);
                if cross_product(self.op_container[p].pt, self.op_container[op2].pt, self.op_container[n].pt) == 0.0 {
                    if op2 == op {
                        match self.unlink_op_back(op2) {
                            None => {
                                op2 = usize::MAX;
                                break;
                            }
                            Some(np) => {
                                op2 = np;
                                op = self.op_container[op2].prev;
                            }
                        }
                    } else {
                        match self.unlink_op_back(op2) {
                            None => {
                                op2 = usize::MAX;
                                break;
                            }
                            Some(np) => op2 = np,
                        }
                    }
                } else {
                    op2 = self.op_container[op2].next;
                }
                if op2 == op {
                    break;
                }
            }

            if op2 == usize::MAX {
                self.results[i] = None;
                continue;
            }
            self.results[i] = Some(op); // safety first

            let mut edge_set1 = get_edges_for_pt(self.op_container[self.op_container[op].prev].pt, &self.rect);
            let mut op2 = op;
            loop {
                let edge_set2 = get_edges_for_pt(self.op_container[op2].pt, &self.rect);
                if edge_set2 != 0 && self.op_container[op2].edge.is_none() {
                    let combined_set = edge_set1 & edge_set2;
                    for j in 0..4u32 {
                        if combined_set & (1 << j) != 0 {
                            let prev_pt = self.op_container[self.op_container[op2].prev].pt;
                            let pt = self.op_container[op2].pt;
                            if is_heading_clockwise(prev_pt, pt, j as usize) {
                                self.add_to_edge((j * 2) as usize, op2);
                            } else {
                                self.add_to_edge((j * 2 + 1) as usize, op2);
                            }
                        }
                    }
                }
                edge_set1 = edge_set2;
                op2 = self.op_container[op2].next;
                if op2 == op {
                    break;
                }
            }
        }
    }

    fn tidy_edges(&mut self, idx: usize, cw_idx: usize, ccw_idx: usize) {
        if self.edges[ccw_idx].is_empty() {
            return;
        }
        let is_horz = idx == 1 || idx == 3;
        let cw_is_toward_larger = idx == 1 || idx == 2;
        let (mut i, mut j) = (0usize, 0usize);

        while i < self.edges[cw_idx].len() {
            let p1_raw = self.edges[cw_idx][i];
            if p1_raw == usize::MAX || self.op_container[p1_raw].next == self.op_container[p1_raw].prev {
                self.edges[cw_idx][i] = usize::MAX;
                i += 1;
                j = 0;
                continue;
            }

            let j_lim = self.edges[ccw_idx].len();
            while j < j_lim {
                let c = self.edges[ccw_idx][j];
                if c != usize::MAX && self.op_container[c].next != self.op_container[c].prev {
                    break;
                }
                j += 1;
            }
            if j == j_lim {
                i += 1;
                j = 0;
                continue;
            }

            let cw_i = self.edges[cw_idx][i];
            let ccw_j = self.edges[ccw_idx][j];
            let (mut p1, p1a, mut p2, p2a);
            if cw_is_toward_larger {
                // p1 >>>> p1a; p2 <<<< p2a;
                p1 = self.op_container[cw_i].prev;
                p1a = cw_i;
                p2 = ccw_j;
                p2a = self.op_container[ccw_j].prev;
            } else {
                // p1 <<<< p1a; p2 >>>> p2a;
                p1 = cw_i;
                p1a = self.op_container[cw_i].prev;
                p2 = self.op_container[ccw_j].prev;
                p2a = ccw_j;
            }

            let overlap = if is_horz {
                has_horz_overlap(self.op_container[p1].pt, self.op_container[p1a].pt, self.op_container[p2].pt, self.op_container[p2a].pt)
            } else {
                has_vert_overlap(self.op_container[p1].pt, self.op_container[p1a].pt, self.op_container[p2].pt, self.op_container[p2a].pt)
            };
            if !overlap {
                j += 1;
                continue;
            }

            // to get here we're either splitting or rejoining
            let is_rejoining = self.op_container[cw_i].owner_idx != self.op_container[ccw_j].owner_idx;

            if is_rejoining {
                self.results[self.op_container[p2].owner_idx] = None;
                let new_owner = self.op_container[p1].owner_idx;
                self.set_new_owner(p2, new_owner);
            }

            // do the split or re-join
            if cw_is_toward_larger {
                // p1 >> | >> p1a;  p2 << | << p2a;
                self.op_container[p1].next = p2;
                self.op_container[p2].prev = p1;
                self.op_container[p1a].prev = p2a;
                self.op_container[p2a].next = p1a;
            } else {
                // p1 << | << p1a;  p2 >> | >> p2a;
                self.op_container[p1].prev = p2;
                self.op_container[p2].next = p1;
                self.op_container[p1a].next = p2a;
                self.op_container[p2a].prev = p1a;
            }

            if !is_rejoining {
                let new_idx = self.results.len();
                self.results.push(Some(p1a));
                self.set_new_owner(p1a, new_idx);
            }

            let (op_, op2_);
            if cw_is_toward_larger {
                op_ = p2;
                op2_ = p1a;
            } else {
                op_ = p1;
                op2_ = p2a;
            }
            self.results[self.op_container[op_].owner_idx] = Some(op_);
            self.results[self.op_container[op2_].owner_idx] = Some(op2_);

            // lots of work to get ready for the next loop
            let (op_is_larger, op2_is_larger);
            if is_horz {
                op_is_larger = self.op_container[op_].pt.x > self.op_container[self.op_container[op_].prev].pt.x;
                op2_is_larger = self.op_container[op2_].pt.x > self.op_container[self.op_container[op2_].prev].pt.x;
            } else {
                op_is_larger = self.op_container[op_].pt.y > self.op_container[self.op_container[op_].prev].pt.y;
                op2_is_larger = self.op_container[op2_].pt.y > self.op_container[self.op_container[op2_].prev].pt.y;
            }

            p1 = op_; // reuse locals as scratch, matching the original's reuse of op/op2
            p2 = op2_;
            let op_next_eq_prev = self.op_container[p1].next == self.op_container[p1].prev;
            let op_pt_eq_prev_pt = self.op_container[p1].pt == self.op_container[self.op_container[p1].prev].pt;
            let op2_next_eq_prev = self.op_container[p2].next == self.op_container[p2].prev;
            let op2_pt_eq_prev_pt = self.op_container[p2].pt == self.op_container[self.op_container[p2].prev].pt;

            if op_next_eq_prev || op_pt_eq_prev_pt {
                if op2_is_larger == cw_is_toward_larger {
                    self.edges[cw_idx][i] = p2;
                    self.edges[ccw_idx][j] = usize::MAX;
                    j += 1;
                } else {
                    self.edges[ccw_idx][j] = p2;
                    self.edges[cw_idx][i] = usize::MAX;
                    i += 1;
                }
            } else if op2_next_eq_prev || op2_pt_eq_prev_pt {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw_idx][i] = p1;
                    self.edges[ccw_idx][j] = usize::MAX;
                    j += 1;
                } else {
                    self.edges[ccw_idx][j] = p1;
                    self.edges[cw_idx][i] = usize::MAX;
                    i += 1;
                }
            } else if op_is_larger == op2_is_larger {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw_idx][i] = p1;
                    self.uncouple_edge(p2);
                    self.add_to_edge(cw_idx, p2);
                    self.edges[ccw_idx][j] = usize::MAX;
                    j += 1;
                } else {
                    self.edges[cw_idx][i] = usize::MAX;
                    i += 1;
                    self.edges[ccw_idx][j] = p2;
                    self.uncouple_edge(p1);
                    self.add_to_edge(ccw_idx, p1);
                    j = 0;
                }
            } else {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw_idx][i] = p1;
                } else {
                    self.edges[ccw_idx][j] = p1;
                }
                if op2_is_larger == cw_is_toward_larger {
                    self.edges[cw_idx][i] = p2;
                } else {
                    self.edges[ccw_idx][j] = p2;
                }
            }
        }
    }

    /// `GetPath`: also strips any residual collinear points, same as the
    /// main engine's `BuildPath64`.
    fn get_path(&mut self, op_in: Option<OutPtIdx>) -> (Path64, Option<OutPtIdx>) {
        let op = match op_in {
            Some(o) if self.op_container[o].next != self.op_container[o].prev => o,
            _ => return (Vec::new(), op_in),
        };

        let mut op = op;
        let mut op2 = self.op_container[op].next;
        while op2 != op {
            let (p, n) = (self.op_container[op2].prev, self.op_container[op2].next);
            if cross_product(self.op_container[p].pt, self.op_container[op2].pt, self.op_container[n].pt) == 0.0 {
                op = self.op_container[op2].prev;
                match self.unlink_op(op2) {
                    Some(next) => op2 = next,
                    None => {
                        return (Vec::new(), None);
                    }
                }
            } else {
                op2 = self.op_container[op2].next;
            }
        }

        let mut result = vec![self.op_container[op].pt];
        let mut cur = self.op_container[op].next;
        while cur != op {
            result.push(self.op_container[cur].pt);
            cur = self.op_container[cur].next;
        }
        (result, Some(op))
    }

    pub fn execute(&mut self, paths: &[Path64]) -> Paths64 {
        let mut result = Vec::new();
        if self.rect.is_empty() {
            return result;
        }

        for path in paths {
            if path.len() < 3 {
                continue;
            }
            self.path_bounds = get_bounds(path);
            if !self.rect.intersects(&self.path_bounds) {
                continue; // the path must be completely outside rect
            } else if self.rect.contains_rect(&self.path_bounds) {
                // the path must be completely inside rect
                result.push(path.clone());
                continue;
            }

            self.execute_internal(path);
            self.check_edges();
            for i in 0..4 {
                self.tidy_edges(i, i * 2, i * 2 + 1);
            }

            let results = std::mem::take(&mut self.results);
            for op in results {
                let (tmp, _) = self.get_path(op);
                if !tmp.is_empty() {
                    result.push(tmp);
                }
            }

            // clean up after every loop
            self.op_container.clear();
            self.results.clear();
            for e in self.edges.iter_mut() {
                e.clear();
            }
            self.start_locs.clear();
        }
        result
    }
}

/// `RectClip` free function.
pub fn rect_clip(rect: Rect64, paths: &[Path64]) -> Paths64 {
    if rect.is_empty() || paths.is_empty() {
        return Vec::new();
    }
    let mut rc = RectClip64::new(rect);
    rc.execute(paths)
}
