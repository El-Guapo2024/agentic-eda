//! Growing free-space rooms, ported from FreeRouting's
//! `ShapeSearchTree45Degree.completeShape` and its helpers.
//!
//! The maze search does not walk a grid. It walks *rooms*: maximal regions
//! of a layer that no obstacle touches, joined by doors. This is how one is
//! grown. Start from the whole board (or a given region) and visit every
//! obstacle on the layer that could touch the current candidate. Each
//! candidate an obstacle overlaps is cut by *one* of the obstacle's eight
//! edge lines -- the one furthest from the shape the room must contain --
//! keeping the side that shape is on. What survives every obstacle is a
//! room.
//!
//! Faithful to the Java in two ways that affect the answer, not just speed:
//! obstacles are visited in the same tree order (the Java pops the second
//! child before the first), and the search bound shrinks after every
//! obstacle, which prunes later ones. A different order yields different,
//! equally valid rooms, and would stop this matching FreeRouting.

use crate::geometry::{IntBox, IntOctagon, TileShape};
use crate::searchtree::{LeafId, ShapeTree};

/// What the room search needs to know about a stored shape.
pub trait TreeObject {
    /// The object's number, FreeRouting's `get_id_no`. Orders the objects
    /// touching a room, so it changes which doors come out. Items and rooms
    /// are numbered separately, so it need not be unique: identity is the
    /// tree leaf.
    fn id(&self) -> u64;
    /// Which of the object's shapes this leaf holds, for an object stored as
    /// several. Orders objects with the same number.
    fn shape_index(&self) -> u32 {
        0
    }
    fn layer(&self) -> i32;
    /// Whether a trace of `net` must stay out of this shape. Copper of the
    /// net itself is not an obstacle to that net.
    fn is_obstacle_for(&self, net: i32) -> bool;
    /// A room that has already been completed and stored. Such rooms get
    /// the ignore-shape exemption below.
    fn is_free_space_room(&self) -> bool;
    /// The shape itself, where it is not the octagon it is stored under --
    /// a trace segment at an odd angle. Growing rooms uses that octagon, as
    /// FreeRouting does; finding a room's neighbours tests the shape.
    fn exact_shape(&self) -> Option<&TileShape> {
        None
    }
    /// An unfixed trace or via of some net, which the autorouter may rip up
    /// or push aside: rooms touching it get a door into an obstacle room for
    /// it. `Item.is_routable`.
    fn is_routable(&self) -> bool {
        false
    }
    /// For the first or last segment of a trace, that segment's line: doors
    /// into its obstacle room must run parallel to it.
    /// `SortedRoomNeighbours.insert_door_ok(ObstacleExpansionRoom, Line)`.
    fn trace_end_line(&self) -> Option<crate::geometry::Line> {
        None
    }
}

/// A room whose shape is known but whose doors are not yet computed.
/// FreeRouting's `IncompleteFreeSpaceExpansionRoom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncompleteRoom {
    /// `None` means "start from the whole board".
    pub shape: Option<IntOctagon>,
    pub layer: i32,
    /// What the finished room must still contain -- typically the door the
    /// search arrived through.
    pub contained: IntOctagon,
}

/// A grown room: its shape and what it had to contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrownRoom {
    pub shape: IntOctagon,
    pub layer: i32,
    pub contained: IntOctagon,
}

/// Grow `room` to maximal obstacle-free octagons on its layer.
///
/// `net`: the net being routed, whose own copper is not an obstacle.
/// `ignore`: a stored object to treat as absent -- the room the search came
/// from. By leaf, as Java compares object identity.
/// `ignore_shape`: overlaps with stored free-space rooms that fall inside
/// this shape are not obstacles -- the door just passed through.
///
/// Returns several rooms when the contained shape cannot fit in one.
///
/// An empty `contained` is allowed, as in the Java: the start shape comes
/// back if no obstacle cuts it, and nothing if one does. Completing a room
/// relies on this for the board sections that `divide_large_room` makes
/// around a start point that lies in only one of them.
pub fn complete_shape<T: TreeObject>(
    tree: &ShapeTree<T>,
    board: IntBox,
    room: &IncompleteRoom,
    net: i32,
    ignore: Option<LeafId>,
    ignore_shape: Option<IntOctagon>,
) -> Vec<GrownRoom> {
    let contained = room.contained;
    let Some(root) = tree.root_node() else { return Vec::new() };
    let board_oct = board.to_octagon();
    let start = match room.shape {
        Some(s) => s.intersection(&board_oct),
        None => board_oct,
    };
    let mut bound = start;
    let mut result = vec![GrownRoom { shape: start, layer: room.layer, contained }];

    // Java's ArrayStack pops the most recently pushed node, and pushes the
    // first child before the second: the second is visited first.
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if !tree.node_bounds(n).intersects(&bound) {
            continue;
        }
        let leaf = match tree.node_step(n) {
            Ok(leaf) => leaf,
            Err((first, second)) => {
                stack.push(first);
                stack.push(second);
                continue;
            }
        };
        let obj = tree.payload(leaf);
        if !(obj.is_obstacle_for(net) && obj.layer() == room.layer && Some(leaf) != ignore) {
            continue;
        }
        let obstacle = tree.bounds(leaf);
        let mut next = Vec::new();
        let mut next_bound = IntOctagon::EMPTY;
        for cur in &result {
            if !cur.shape.overlaps(&obstacle) {
                next.push(*cur);
                next_bound = next_bound.union(&cur.shape);
                continue;
            }
            if obj.is_free_space_room() {
                if let Some(ig) = ignore_shape {
                    let meet = cur.shape.intersection(&obstacle);
                    if ig.contains_corners(&meet) {
                        // The overlap lies within the door just crossed:
                        // not an obstacle. Keep the room unless the door
                        // swallows it entirely.
                        if !ig.contains_corners(&cur.shape) {
                            next.push(*cur);
                            next_bound = next_bound.union(&cur.shape);
                        }
                        continue;
                    }
                }
            }
            next.extend(restrain_shape(cur, &obstacle));
            // The Java re-unions the whole accumulated list here, not just
            // the new pieces; kept, since it is what shapes the next bound.
            for r in &next {
                next_bound = next_bound.union(&r.shape);
            }
        }
        result = next;
        bound = next_bound;
    }

    let mut result = divide_large_room(result, board);
    // A room no bigger than what it must contain would be grown again
    // forever; drop it. Tested by corners, as the Java does, which keeps a
    // room equal to its contained shape if a corner of it lies at
    // half-integer coordinates (see IntOctagon::contains_corners).
    result.retain(|r| !r.contained.contains_corners(&r.shape));
    result
}

/// Split a lone room that spans more than half the board both ways, so a
/// layer never has just one room -- the maze search mishandles vias
/// otherwise. `ShapeSearchTree.divideLargeRoom` plus the 45-degree
/// override, which re-bounds every piece as an octagon.
fn divide_large_room(rooms: Vec<GrownRoom>, board: IntBox) -> Vec<GrownRoom> {
    if rooms.len() != 1 {
        return rooms;
    }
    let room = rooms[0];
    let bb = room.shape.bounding_box();
    let (h, w) = (bb.ur.y - bb.ll.y, bb.ur.x - bb.ll.x);
    let (bh, bw) = (board.ur.y - board.ll.y, board.ur.x - board.ll.x);
    if 2 * h <= bh || 2 * w <= bw {
        return rooms;
    }
    let max_section = 0.5 * (bh.max(bw) as f64);
    bb.divide_into_sections(max_section)
        .into_iter()
        .map(|b| room.shape.intersection(&b.to_octagon()))
        .filter(|s| s.dimension() == 2)
        .map(|s| GrownRoom { shape: s, layer: room.layer, contained: s.intersection(&room.contained) })
        .collect()
}

/// Cut `room` by one edge line of `obstacle` so it no longer overlaps it,
/// keeping what it must contain. `ShapeSearchTree45Degree.restrainShape`.
fn restrain_shape(room: &GrownRoom, obstacle: &IntOctagon) -> Vec<GrownRoom> {
    let keep = room.contained;
    if keep.is_empty() {
        return Vec::new();
    }
    let shape = room.shape;

    // Prefer a line that has everything to be kept on its far side, and of
    // those the one furthest from it.
    let mut best_dist = -1.0;
    let mut best_line = None;
    for line in 0..8 {
        let d = signed_line_distance(obstacle, line, &keep);
        if d > best_dist && segment_touches_inside(obstacle, line, &shape) {
            best_dist = d;
            best_line = Some(line);
        }
    }
    // Only if that line leaves all of it clear: a diagonal can come out
    // half a unit short (-0.5), and then the room is split instead.
    if best_dist >= 0.0 {
        if let Some(line) = best_line {
            return vec![GrownRoom { shape: outside(obstacle, line, &shape), layer: room.layer, contained: keep }];
        }
    }

    // No line leaves all of it clear, so split: find a line through the
    // interior of what must be kept, take the piece beyond it, and recurse
    // on the rest.
    if keep.dimension() < 1 {
        // Already a completed room around a point here.
        return Vec::new();
    }
    let mut cut = None;
    for line in 0..8 {
        if segment_touches_inside(obstacle, line, &shape) && line_cuts_interior(obstacle, line, &keep) {
            cut = Some(line);
            break;
        }
    }
    let Some(line) = cut else {
        // Parts, or all, of it may already be taken by another room.
        return Vec::new();
    };
    let mut out = Vec::new();
    let beyond = outside(obstacle, line, &shape);
    if beyond.dimension() == 2 {
        let kept = keep.intersection(&beyond);
        if kept.dimension() > 0 {
            out.push(GrownRoom { shape: beyond, layer: room.layer, contained: kept });
        }
    }
    let rest = inside(obstacle, line, &shape);
    if rest.dimension() >= 2 {
        let rest_keep = keep.intersection(&rest);
        if rest_keep.dimension() >= 0 {
            out.extend(restrain_shape(&GrownRoom { shape: rest, layer: room.layer, contained: rest_keep }, obstacle));
        }
    }
    out
}

/// Whether border line `line` of `shape` cuts through the interior of
/// `keep`: corners of `keep` lie strictly on both sides of it.
/// The use `restrainShape` makes of `TileShape.sideOf(Line) == COLLINEAR`.
fn line_cuts_interior(shape: &IntOctagon, line: usize, keep: &IntOctagon) -> bool {
    let (mut neg, mut pos) = (false, false);
    for i in 0..8 {
        let c = keep.corner(i);
        match shape.side_of_border_line(c.x, c.y, line) {
            s if s < 0 => neg = true,
            s if s > 0 => pos = true,
            _ => {}
        }
    }
    neg && pos
}

/// `room` intersected with the half-plane *outside* border line `line`.
fn outside(obstacle: &IntOctagon, line: usize, room: &IntOctagon) -> IntOctagon {
    let mut r = *room;
    match line {
        0 => r.top_y = obstacle.bottom_y,
        2 => r.left_x = obstacle.right_x,
        4 => r.bottom_y = obstacle.top_y,
        6 => r.right_x = obstacle.left_x,
        1 => r.upper_left_diag_x = obstacle.lower_right_diag_x,
        3 => r.lower_left_diag_x = obstacle.upper_right_diag_x,
        5 => r.lower_right_diag_x = obstacle.upper_left_diag_x,
        7 => r.upper_right_diag_x = obstacle.lower_left_diag_x,
        _ => panic!("outside: line {line} out of range"),
    }
    rebuild(r).normalize()
}

/// `room` intersected with the half-plane *inside* border line `line`.
fn inside(obstacle: &IntOctagon, line: usize, room: &IntOctagon) -> IntOctagon {
    let mut r = *room;
    match line {
        0 => r.bottom_y = obstacle.bottom_y,
        2 => r.right_x = obstacle.right_x,
        4 => r.top_y = obstacle.top_y,
        6 => r.left_x = obstacle.left_x,
        1 => r.lower_right_diag_x = obstacle.lower_right_diag_x,
        3 => r.upper_right_diag_x = obstacle.upper_right_diag_x,
        5 => r.upper_left_diag_x = obstacle.upper_left_diag_x,
        7 => r.lower_left_diag_x = obstacle.lower_left_diag_x,
        _ => panic!("inside: line {line} out of range"),
    }
    rebuild(r).normalize()
}

/// A fresh, non-empty octagon from edited bounds (Java constructs a new
/// one; editing a copy of an EMPTY would otherwise keep its flag).
fn rebuild(r: IntOctagon) -> IntOctagon {
    IntOctagon::new(r.left_x, r.bottom_y, r.right_x, r.top_y, r.upper_left_diag_x, r.lower_right_diag_x, r.lower_left_diag_x, r.upper_right_diag_x)
}

/// How far `keep` lies beyond border line `line` of `obstacle`; negative
/// when it is not wholly beyond. Diagonals are weighted 0.5, not
/// 1/sqrt(2), to prefer orthogonal cuts slightly -- FreeRouting's choice.
fn signed_line_distance(obstacle: &IntOctagon, line: usize, keep: &IntOctagon) -> f64 {
    match line {
        0 => (obstacle.bottom_y - keep.top_y) as f64,
        2 => (keep.left_x - obstacle.right_x) as f64,
        4 => (keep.bottom_y - obstacle.top_y) as f64,
        6 => (obstacle.left_x - keep.right_x) as f64,
        1 => 0.5 * (keep.upper_left_diag_x - obstacle.lower_right_diag_x) as f64,
        3 => 0.5 * (keep.lower_left_diag_x - obstacle.upper_right_diag_x) as f64,
        5 => 0.5 * (obstacle.upper_left_diag_x - keep.lower_right_diag_x) as f64,
        7 => 0.5 * (obstacle.lower_left_diag_x - keep.upper_right_diag_x) as f64,
        _ => panic!("signed_line_distance: line {line} out of range"),
    }
}

/// Whether border segment `line` of `obstacle` passes through the interior
/// of `room`. `ShapeSearchTree45Degree.obstacleSegmentTouchesInside`: the
/// segment's two end corners must each be strictly inside the relevant
/// room border lines.
fn segment_touches_inside(obstacle: &IntOctagon, line: usize, room: &IntOctagon) -> bool {
    let c = obstacle.corner(line);
    let mut bl = line;
    for _ in 0..5 {
        if room.side_of_border_line(c.x, c.y, bl) >= 0 {
            return false;
        }
        bl = (bl + 1) % 8;
    }
    let next = (line + 1) % 8;
    let c = obstacle.corner(next);
    let mut bl = (line + 5) % 8;
    for _ in 0..3 {
        if room.side_of_border_line(c.x, c.y, bl) >= 0 {
            return false;
        }
        bl = (bl + 1) % 8;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct Pad {
        id: u64,
        layer: i32,
        net: i32,
    }

    impl TreeObject for Pad {
        fn id(&self) -> u64 {
            self.id
        }
        fn layer(&self) -> i32 {
            self.layer
        }
        fn is_obstacle_for(&self, net: i32) -> bool {
            self.net != net
        }
        fn is_free_space_room(&self) -> bool {
            false
        }
    }

    const BOARD: IntBox = IntBox::new(0, 0, 10_000, 8_000);

    fn tree(pads: &[(IntBox, i32, i32)]) -> ShapeTree<Pad> {
        let mut t = ShapeTree::new();
        for (i, (b, layer, net)) in pads.iter().enumerate() {
            t.insert(b.to_octagon(), Pad { id: i as u64, layer: *layer, net: *net });
        }
        t
    }

    fn point(x: i64, y: i64) -> IntOctagon {
        IntBox::new(x, y, x, y).to_octagon()
    }

    /// What every grown room must be, whatever the obstacles.
    fn check(rooms: &[GrownRoom], t: &ShapeTree<Pad>, net: i32, layer: i32) {
        for r in rooms {
            assert_eq!(r.layer, layer);
            assert!(r.shape.is_contained_in(&BOARD.to_octagon()), "room {:?} leaves the board", r.shape);
            assert!(r.contained.is_contained_in(&r.shape) || r.contained.is_empty(), "room {:?} lost what it must contain {:?}", r.shape, r.contained);
            for leaf in t.overlaps(&r.shape) {
                let p = t.payload(leaf);
                if p.layer == layer && p.is_obstacle_for(net) {
                    assert!(!r.shape.overlaps(&t.bounds(leaf)), "room {:?} overlaps obstacle {:?}", r.shape, t.bounds(leaf));
                }
            }
        }
    }

    #[test]
    fn an_obstacle_on_another_layer_is_no_obstacle() {
        // A different layer: the room is the whole board, split in pieces
        // so the layer has more than one room.
        let t = tree(&[(IntBox::new(4_000, 3_000, 6_000, 5_000), 1, 7)]);
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(1_000, 1_000) }, 1, None, None);
        check(&rooms, &t, 1, 0);
        let area: f64 = rooms.iter().map(|r| r.shape.area()).sum();
        assert!((area - 80_000_000.0).abs() < 1.0, "sections should tile the board, got {area}");
    }

    #[test]
    fn a_room_stops_at_an_obstacle() {
        let t = tree(&[(IntBox::new(4_000, 3_000, 6_000, 5_000), 0, 7)]);
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(1_000, 4_000) }, 1, None, None);
        assert!(!rooms.is_empty(), "no room grown");
        check(&rooms, &t, 1, 0);
        // The room holding the start point reaches right up to the obstacle
        // and no further: nothing is wasted between them.
        let r = rooms.iter().find(|r| r.contained.intersects(&point(1_000, 4_000))).expect("room with start");
        assert_eq!(r.shape.right_x, 4_000, "room {:?} should stop exactly at the obstacle's left edge", r.shape);
    }

    #[test]
    fn copper_of_the_same_net_is_not_an_obstacle() {
        let t = tree(&[(IntBox::new(4_000, 3_000, 6_000, 5_000), 0, 1)]);
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(1_000, 4_000) }, 1, None, None);
        let area: f64 = rooms.iter().map(|r| r.shape.area()).sum();
        assert!((area - 80_000_000.0).abs() < 1.0, "same-net pad should not cut the room, got {area}");
    }

    #[test]
    fn an_ignored_object_is_not_an_obstacle() {
        let mut t = ShapeTree::new();
        let pad = t.insert(IntBox::new(4_000, 3_000, 6_000, 5_000).to_octagon(), Pad { id: 0, layer: 0, net: 7 });
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(1_000, 4_000) }, 1, Some(pad), None);
        let area: f64 = rooms.iter().map(|r| r.shape.area()).sum();
        assert!((area - 80_000_000.0).abs() < 1.0, "ignored obstacle cut the room, got {area}");
    }

    /// Ignoring is by leaf, not by number: another object with the same
    /// number is still an obstacle. Items and rooms are numbered
    /// separately, so numbers collide in practice.
    #[test]
    fn ignoring_one_object_does_not_ignore_another_with_its_number() {
        let mut t = ShapeTree::new();
        let ignored = t.insert(IntBox::new(8_000, 6_000, 9_000, 7_000).to_octagon(), Pad { id: 5, layer: 0, net: 7 });
        t.insert(IntBox::new(4_000, 3_000, 6_000, 5_000).to_octagon(), Pad { id: 5, layer: 0, net: 7 });
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(1_000, 4_000) }, 1, Some(ignored), None);
        let r = rooms.iter().find(|r| r.shape.contains_point(1_000.0, 4_000.0)).expect("room with the start point");
        assert_eq!(r.shape.right_x, 4_000, "the unignored pad with the same number should still stop the room: {:?}", r.shape);
    }

    /// With nothing it must contain, a room survives whole where no
    /// obstacle cuts it, and vanishes where one does. The Java has no early
    /// return for this; completing a room needs the first half, for board
    /// sections that miss the start point.
    #[test]
    fn a_room_with_nothing_to_contain_survives_only_uncut() {
        let t = tree(&[(IntBox::new(4_000, 3_000, 6_000, 5_000), 0, 7)]);
        let clear = IntBox::new(0, 0, 3_000, 3_000).to_octagon();
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: Some(clear), layer: 0, contained: IntOctagon::EMPTY }, 1, None, None);
        assert_eq!(rooms.len(), 1, "uncut room should survive: {rooms:?}");
        assert_eq!(rooms[0].shape, clear);
        let cut = IntBox::new(3_000, 2_000, 5_000, 4_000).to_octagon();
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: Some(cut), layer: 0, contained: IntOctagon::EMPTY }, 1, None, None);
        assert!(rooms.is_empty(), "a cut room with nothing to keep should vanish: {rooms:?}");
    }

    /// The cut must use an obstacle edge that actually crosses the room,
    /// or free space is thrown away for nothing.
    ///
    /// The room is the strip x in [0, 2000]. The obstacle sits mostly to
    /// its right; only a corner, clipped by a diagonal (x + y >= 8500),
    /// reaches into the strip. From the start point (500, 1000) the
    /// obstacle's *bottom* edge (y = 6000) is furthest away, so a cut
    /// that ignored which edges cross the room would take it and discard
    /// everything above y = 6000. That bottom edge runs x = 2500..3000,
    /// entirely outside the strip. The right cut is the diagonal, which
    /// does cross the strip, and keeps e.g. (1000, 7000).
    ///
    /// Legality checks alone cannot see this: cutting along any edge line
    /// of a convex obstacle excludes it completely. With the crossing
    /// check removed, every other test here still passed.
    #[test]
    fn the_cut_uses_an_edge_that_crosses_the_room() {
        let obstacle = IntOctagon::new(1_500, 6_000, 3_000, 7_000, -CRIT_TEST, CRIT_TEST, 8_500, CRIT_TEST).normalize();
        let mut t = ShapeTree::new();
        t.insert(obstacle, Pad { id: 0, layer: 0, net: 7 });
        let strip = IntBox::new(0, 0, 2_000, 8_000).to_octagon();
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: Some(strip), layer: 0, contained: point(500, 1_000) }, 1, None, None);
        check(&rooms, &t, 1, 0);
        let r = rooms.iter().find(|r| r.shape.contains_point(500.0, 1_000.0)).expect("room with the start point");
        assert!(r.shape.contains_point(1_000.0, 7_000.0), "free space above the obstacle's far edge was discarded: room {:?}", r.shape);
    }

    const CRIT_TEST: i64 = 1 << 30;

    /// A cut must leave everything to be kept on or beyond the line. A
    /// diagonal can come out half a unit short: here the start point lies
    /// inside the obstacle by half a unit across its lower-right diagonal,
    /// and FreeRouting grows no room rather than one that has lost the
    /// point. Cutting along that diagonal anyway was a porting slip.
    #[test]
    fn a_line_half_a_unit_short_does_not_cut() {
        // A 100-square with the lower-right corner cut at x - y <= 86;
        // (90, 5) has x - y = 85.
        let obstacle = IntOctagon::new(0, 0, 100, 100, -100, 86, 0, 200).normalize();
        let mut t = ShapeTree::new();
        t.insert(obstacle, Pad { id: 0, layer: 0, net: 7 });
        let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer: 0, contained: point(90, 5) }, 1, None, None);
        assert!(rooms.is_empty(), "a start point inside an obstacle should get no room: {rooms:?}");
    }

    /// A field of pads, rooms grown from many start points: every result is
    /// on the board, free of obstacles, and still holds its start point.
    #[test]
    fn rooms_among_many_obstacles_are_always_legal() {
        let mut seed: u64 = 0xDEAD_BEEF;
        let mut next = move |lo: i64, hi: i64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % ((hi - lo + 1) as u64)) as i64
        };
        let mut pads = Vec::new();
        for _ in 0..40 {
            let (x, y) = (next(200, 9_400), next(200, 7_400));
            pads.push((IntBox::new(x, y, x + next(100, 500), y + next(100, 500)), next(0, 1) as i32, next(0, 5) as i32));
        }
        let t = tree(&pads);
        let mut grown = 0;
        for _ in 0..200 {
            let (x, y) = (next(0, 10_000), next(0, 8_000));
            let start = point(x, y);
            let layer = next(0, 1) as i32;
            let net = next(0, 5) as i32;
            // A start point buried in an obstacle has no free room.
            let buried = pads.iter().any(|(b, l, n)| *l == layer && *n != net && b.to_octagon().overlaps(&start.offset(1.0)));
            if buried {
                continue;
            }
            let rooms = complete_shape(&t, BOARD, &IncompleteRoom { shape: None, layer, contained: start }, net, None, None);
            check(&rooms, &t, net, layer);
            grown += rooms.len();
        }
        assert!(grown > 100, "only {grown} rooms grown");
    }
}
