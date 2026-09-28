//! Rooms and the doors between them, ported from FreeRouting's
//! `Sorted45DegreeRoomNeighbours` and the room bookkeeping in
//! `AutorouteEngine`.
//!
//! The maze search walks free space as a graph. Its nodes are rooms --
//! obstacle-free octagons grown by [`complete_shape`] -- and its edges are
//! doors, where two rooms meet. A room starts *incomplete*: a region the
//! search may expand into, holding a shape the finished room must contain
//! (typically the door the search arrived through). Completing it grows it,
//! collects every object touching its border, sorted counter-clockwise
//! around it, and opens a door to each neighbouring room. A stretch of
//! border between two consecutive neighbours that do not touch is free
//! space nobody has claimed yet: it gets a new incomplete room behind a
//! door, completed later if the search goes that way.
//!
//! Before any of that, a room with a side that touches nothing could have
//! been bigger, so that border line is dropped and the room regrown.
//!
//! Only free-space rooms are ported. Obstacle expansion rooms (for ripup
//! and push-and-shove) and target doors into the net's own copper need the
//! board model and come later.
//!
//! Java links rooms and doors by object reference in both directions. Here
//! both live in `Vec`s and refer to each other by index; removing one
//! leaves a `None`, so a stale id never names something else. Each room
//! keeps its door ids in insertion order: the Java walks doors in that
//! order and takes the first match, so the order changes which rooms come
//! out.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use crate::geometry::{IntBox, IntOctagon, Line, TileShape, CRIT};
use crate::room::{complete_shape, GrownRoom, IncompleteRoom, TreeObject};
use crate::searchtree::{LeafId, ShapeTree};

/// A handle to a room. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoomId(usize);

/// A handle to a door. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoorId(usize);

/// What the graph's tree stores: the caller's board objects, and the rooms
/// completed so far.
#[derive(Debug, Clone)]
pub enum Entry<T> {
    Item(T),
    Room { room: RoomId, id_no: u64, layer: i32 },
}

impl<T: TreeObject> TreeObject for Entry<T> {
    fn id(&self) -> u64 {
        match self {
            Entry::Item(t) => t.id(),
            Entry::Room { id_no, .. } => *id_no,
        }
    }

    fn shape_index(&self) -> u32 {
        match self {
            Entry::Item(t) => t.shape_index(),
            Entry::Room { .. } => 0,
        }
    }

    fn layer(&self) -> i32 {
        match self {
            Entry::Item(t) => t.layer(),
            Entry::Room { layer, .. } => *layer,
        }
    }

    /// A completed room is an obstacle to every net
    /// (`CompleteFreeSpaceExpansionRoom.is_trace_obstacle`), so rooms grown
    /// later stop at it.
    fn is_obstacle_for(&self, net: i32) -> bool {
        match self {
            Entry::Item(t) => t.is_obstacle_for(net),
            Entry::Room { .. } => true,
        }
    }

    fn is_free_space_room(&self) -> bool {
        match self {
            Entry::Item(t) => t.is_free_space_room(),
            Entry::Room { .. } => true,
        }
    }

    fn exact_shape(&self) -> Option<&TileShape> {
        match self {
            Entry::Item(t) => t.exact_shape(),
            Entry::Room { .. } => None,
        }
    }

    fn is_routable(&self) -> bool {
        match self {
            Entry::Item(t) => t.is_routable(),
            Entry::Room { .. } => false,
        }
    }

    fn trace_end_line(&self) -> Option<Line> {
        match self {
            Entry::Item(t) => t.trace_end_line(),
            Entry::Room { .. } => None,
        }
    }

    fn is_trace(&self) -> bool {
        match self {
            Entry::Item(t) => t.is_trace(),
            Entry::Room { .. } => false,
        }
    }

    fn shares_net_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Entry::Item(a), Entry::Item(b)) => a.shares_net_with(b),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    /// Not grown yet (`IncompleteFreeSpaceExpansionRoom`). The finished
    /// room must contain `contained`.
    Incomplete { contained: IntOctagon },
    /// Grown, with its doors calculated (`CompleteFreeSpaceExpansionRoom`).
    Complete {
        /// FreeRouting's room number, from a counter shared by every room
        /// the graph completes.
        id_no: u64,
        /// Where the room is stored in the tree. `None` only while its
        /// doors are being calculated, before it is stored.
        leaf: Option<LeafId>,
        /// The room touches copper of the net being routed, so it is only
        /// valid for that net; see [`Room::targets`].
        net_dependent: bool,
    },
    /// The room an item is to the search: entered to rip the item up or
    /// push it aside (`ObstacleExpansionRoom`). Its shape is the item's tree
    /// shape, in [`Room::tile`].
    Obstacle {
        /// The item's entry in the tree.
        item: LeafId,
        /// `(item id << 10) | shape index`, in Java `int` arithmetic.
        id_no: i64,
    },
}

#[derive(Debug, Clone)]
pub struct Room {
    /// `None` only for an incomplete room that starts from the whole board,
    /// and for an obstacle room, whose shape is `tile`.
    pub shape: Option<IntOctagon>,
    pub layer: i32,
    pub state: RoomState,
    doors: Vec<DoorId>,
    /// For a completed room: the entries of items no trace of the net must
    /// avoid that touch it -- the net's own copper among them -- in the
    /// order they were found. FreeRouting's
    /// `CompleteFreeSpaceExpansionRoom.calculate_target_doors` looks at
    /// each for a target door.
    pub targets: Vec<LeafId>,
    /// For an obstacle room: its item's tree shape.
    pub tile: Option<TileShape>,
}

impl Room {
    /// The room's doors, in the order they were made.
    pub fn doors(&self) -> &[DoorId] {
        &self.doors
    }

    /// A completed free-space room or an obstacle room: anything but an
    /// incomplete room (`CompleteExpansionRoom`).
    pub fn is_complete(&self) -> bool {
        !matches!(self.state, RoomState::Incomplete { .. })
    }

    pub fn is_obstacle_room(&self) -> bool {
        matches!(self.state, RoomState::Obstacle { .. })
    }

    /// The room's shape, whatever its kind: its octagon, or an obstacle
    /// room's item shape. `None` only for an incomplete room over the whole
    /// board.
    pub fn tile_shape(&self) -> Option<TileShape> {
        match (&self.tile, self.shape) {
            (Some(t), _) => Some(t.clone()),
            (None, Some(o)) => Some(TileShape::Octagon(o)),
            (None, None) => None,
        }
    }

    /// FreeRouting's room number: `get_id_no` of a completed room or an
    /// obstacle room.
    pub fn id_no(&self) -> Option<i64> {
        match self.state {
            RoomState::Complete { id_no, .. } => Some(id_no as i64),
            RoomState::Obstacle { id_no, .. } => Some(id_no),
            RoomState::Incomplete { .. } => None,
        }
    }
}

/// Where two rooms meet. `ExpansionDoor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Door {
    pub first: RoomId,
    pub second: RoomId,
    /// 1 when the rooms share a stretch of edge, 2 when they overlap.
    pub dimension: i32,
}

impl Door {
    /// The room on the other side from `room`; `None` if `room` is on
    /// neither side.
    pub fn other(&self, room: RoomId) -> Option<RoomId> {
        if room == self.first {
            Some(self.second)
        } else if room == self.second {
            Some(self.first)
        } else {
            None
        }
    }
}

/// The rooms and doors of one net's search, over a tree of board objects.
/// The state `AutorouteEngine` keeps for them.
#[derive(Debug, Clone)]
pub struct RoomGraph<T> {
    tree: ShapeTree<Entry<T>>,
    board: IntBox,
    net: i32,
    rooms: Vec<Option<Room>>,
    doors: Vec<Option<Door>>,
    /// `incomplete_expansion_rooms`, in the order they were made.
    incomplete: Vec<RoomId>,
    /// `complete_expansion_rooms`.
    complete: Vec<RoomId>,
    /// `expansion_room_instance_count`: the last room number handed out.
    room_count: u64,
    /// Each routable item entry's obstacle room, made when first needed:
    /// `ItemAutorouteInfo.get_expansion_room`.
    obstacle_rooms: HashMap<LeafId, RoomId>,
    /// Obstacle rooms whose doors have been calculated:
    /// `ObstacleExpansionRoom.all_doors_calculated`.
    obstacle_doors_calculated: std::collections::HashSet<RoomId>,
}

impl<T: TreeObject> RoomGraph<T> {
    pub fn new(board: IntBox, net: i32) -> Self {
        RoomGraph {
            tree: ShapeTree::new(),
            board,
            net,
            rooms: Vec::new(),
            doors: Vec::new(),
            incomplete: Vec::new(),
            complete: Vec::new(),
            room_count: 0,
            obstacle_rooms: HashMap::new(),
            obstacle_doors_calculated: std::collections::HashSet::new(),
        }
    }

    /// Store a board object. Rooms grown afterwards see it.
    pub fn insert_item(&mut self, shape: IntOctagon, item: T) -> LeafId {
        self.tree.insert(shape, Entry::Item(item))
    }

    pub fn tree(&self) -> &ShapeTree<Entry<T>> {
        &self.tree
    }

    pub fn board(&self) -> IntBox {
        self.board
    }

    pub fn net(&self) -> i32 {
        self.net
    }

    pub fn room(&self, id: RoomId) -> &Room {
        self.rooms[id.0].as_ref().unwrap_or_else(|| panic!("RoomGraph::room: {id:?} was removed"))
    }

    fn room_mut(&mut self, id: RoomId) -> &mut Room {
        self.rooms[id.0].as_mut().unwrap_or_else(|| panic!("RoomGraph::room_mut: {id:?} was removed"))
    }

    pub fn door(&self, id: DoorId) -> &Door {
        self.doors[id.0].as_ref().unwrap_or_else(|| panic!("RoomGraph::door: {id:?} was removed"))
    }

    /// Where a door's two free-space rooms meet: a stretch of edge, or
    /// their overlap. `ExpansionDoor.get_shape`.
    pub fn door_shape(&self, id: DoorId) -> IntOctagon {
        let d = self.door(id);
        self.shape_of(d.first).intersection(&self.shape_of(d.second))
    }

    /// Where a door's two rooms meet, obstacle rooms included: the first
    /// room's shape cut by the second's. `ExpansionDoor.get_shape`.
    pub fn door_tile_shape(&self, id: DoorId) -> TileShape {
        let d = self.door(id);
        let first = self.room(d.first).tile_shape().expect("a room with a door has a shape");
        let second = self.room(d.second).tile_shape().expect("a room with a door has a shape");
        first.intersection(&second)
    }

    /// The room behind a door from `room`.
    pub fn doors_of(&self, room: RoomId) -> &[DoorId] {
        &self.room(room).doors
    }

    /// The obstacle room for a routable item's entry, made the first time
    /// it is asked for. `ItemAutorouteInfo.get_expansion_room`.
    pub fn obstacle_room(&mut self, leaf: LeafId) -> RoomId {
        if let Some(&r) = self.obstacle_rooms.get(&leaf) {
            return r;
        }
        let p = self.tree.payload(leaf);
        let (id, index, layer) = (p.id(), p.shape_index(), p.layer());
        let tile = p.exact_shape().cloned().unwrap_or(TileShape::Octagon(self.tree.bounds(leaf)));
        let id_no = ((id as i32).wrapping_shl(10) | index as i32) as i64;
        let room = RoomId(self.rooms.len());
        self.rooms.push(Some(Room {
            shape: None,
            layer,
            state: RoomState::Obstacle { item: leaf, id_no },
            doors: Vec::new(),
            targets: Vec::new(),
            tile: Some(tile),
        }));
        self.obstacle_rooms.insert(leaf, room);
        room
    }

    /// A room's shape, for a room known to have one: every room with a door
    /// does.
    fn shape_of(&self, id: RoomId) -> IntOctagon {
        self.room(id).shape.unwrap_or_else(|| panic!("RoomGraph: {id:?} has no shape"))
    }

    /// Incomplete rooms, in the order they were made.
    pub fn incomplete_rooms(&self) -> &[RoomId] {
        &self.incomplete
    }

    pub fn complete_rooms(&self) -> &[RoomId] {
        &self.complete
    }

    /// `AutorouteEngine.add_incomplete_expansion_room`. `shape: None`
    /// starts from the whole board.
    pub fn add_incomplete_room(&mut self, shape: Option<IntOctagon>, layer: i32, contained: IntOctagon) -> RoomId {
        let id = RoomId(self.rooms.len());
        self.rooms.push(Some(Room {
            shape,
            layer,
            state: RoomState::Incomplete { contained },
            doors: Vec::new(),
            targets: Vec::new(),
            tile: None,
        }));
        self.incomplete.push(id);
        id
    }

    /// `AutorouteEngine.remove_incomplete_expansion_room`.
    pub fn remove_incomplete_room(&mut self, room: RoomId) {
        self.remove_all_doors(room);
        if let Some(i) = self.incomplete.iter().position(|&r| r == room) {
            self.incomplete.remove(i);
        }
        self.rooms[room.0] = None;
    }

    /// Grow an incomplete room into completed ones, with their doors, and
    /// remove it. `AutorouteEngine.complete_expansion_room`.
    pub fn complete_expansion_room(&mut self, room: RoomId) -> Vec<RoomId> {
        let r = self.room(room).clone();
        let RoomState::Incomplete { contained } = r.state else {
            panic!("RoomGraph::complete_expansion_room: {room:?} is already complete");
        };
        // Entered through an overlap with a completed room: that room is no
        // obstacle, nor is another room's overlap that lies inside the door.
        let mut from_door = None;
        for &d in &r.doors {
            let door = *self.door(d);
            let Some(other) = door.other(room) else { continue };
            if door.dimension == 2 {
                if let RoomState::Complete { leaf: Some(leaf), .. } = self.room(other).state {
                    from_door = Some((leaf, self.door_shape(d)));
                    break;
                }
            }
        }
        let (ignore, ignore_shape) = (from_door.map(|f| f.0), from_door.map(|f| f.1));
        let incomplete = IncompleteRoom { shape: r.shape, layer: r.layer, contained };
        let grown = complete_shape(&self.tree, self.board, &incomplete, self.net, ignore, ignore_shape);
        self.remove_incomplete_room(room);

        let mut result = Vec::new();
        let mut first = true;
        for g in grown {
            if g.shape.dimension() != 2 {
                continue;
            }
            if first {
                first = false;
                result.extend(self.add_complete_room(g));
            } else {
                // The rooms completed so far are obstacles now, and may hold
                // space this one was grown into: grow it again.
                let again = IncompleteRoom { shape: Some(g.shape), layer: g.layer, contained: g.contained };
                for g in complete_shape(&self.tree, self.board, &again, self.net, ignore, ignore_shape) {
                    result.extend(self.add_complete_room(g));
                }
            }
        }
        result
    }

    /// Complete every incomplete room behind a door of `room`, and
    /// calculate the doors of every obstacle room behind one.
    /// `AutorouteEngine.complete_neighbour_rooms`.
    pub fn complete_neighbour_rooms(&mut self, room: RoomId) {
        let mut i = 0;
        while i < self.room(room).doors.len() {
            let d = self.room(room).doors[i];
            i += 1;
            let Some(other) = self.door(d).other(room) else { continue };
            if !self.room(other).is_complete() {
                self.complete_expansion_room(other);
                // That changed this room's doors: start over, as the Java
                // restarts its iterator.
                i = 0;
            } else if self.room(other).is_obstacle_room() && !self.obstacle_doors_calculated.contains(&other) {
                self.calculate_obstacle_doors(other);
                self.obstacle_doors_calculated.insert(other);
            }
        }
    }

    /// The doors of an obstacle room: to the rooms and routable items it
    /// touches, overlap doors to the obstacle rooms of routable items of its
    /// net it overlaps, and new incomplete rooms along its free sides.
    /// `Sorted45DegreeRoomNeighbours.calculate`, for an
    /// `ObstacleExpansionRoom`. Like the Java it takes a room number it
    /// never uses.
    fn calculate_obstacle_doors(&mut self, room: RoomId) {
        self.room_count += 1;
        let (tile, layer) = {
            let r = self.room(room);
            (r.tile.clone().expect("an obstacle room has a shape"), r.layer)
        };
        let shape = tile.bounding_octagon().expect("an obstacle within the critical bound");
        let mut n = Neighbours { completed: room, shape, layer, sorted: BTreeSet::new(), edge_touched: [false; 8] };
        // ShapeSearchTree.overlapping_tree_entries with the obstacle's own
        // shape: exactly, unless both are octagons.
        let is_octagon = matches!(tile, TileShape::Octagon(_));
        let mut entries: Vec<LeafId> = self
            .tree
            .overlaps(&shape)
            .into_iter()
            .filter(|&l| {
                let p = self.tree.payload(l);
                if p.layer() != layer {
                    return false;
                }
                let entry_shape = p.exact_shape().cloned().unwrap_or(TileShape::Octagon(self.tree.bounds(l)));
                (is_octagon && matches!(entry_shape, TileShape::Octagon(_))) || entry_shape.intersects(&tile)
            })
            .collect();
        entries.sort_by_key(|&l| {
            let p = self.tree.payload(l);
            (p.id(), p.shape_index(), matches!(p, Entry::Item(_)))
        });
        for leaf in entries {
            let (id, neighbour_room, routable) = {
                let p = self.tree.payload(leaf);
                let r = match p {
                    Entry::Room { room, .. } => Some(*room),
                    Entry::Item(_) => None,
                };
                (p.id(), r, p.is_routable())
            };
            let intersection = shape.intersection(&self.tree.bounds(leaf));
            let dimension = intersection.dimension();
            if dimension > 1 {
                // Only obstacle rooms overlap: join those of the same net.
                if neighbour_room.is_none() && routable {
                    let other = self.obstacle_room(leaf);
                    self.create_overlap_door(room, other);
                }
                continue;
            }
            if dimension < 0 {
                continue;
            }
            if let Some(nb) = Neighbour::new(&shape, intersection, id, &mut n.edge_touched) {
                n.sorted.insert(nb);
            }
            if dimension > 0 {
                let other = match neighbour_room {
                    Some(r) => Some(r),
                    None if routable => Some(self.obstacle_room(leaf)),
                    None => None,
                };
                if let Some(other) = other {
                    if self.insert_door_ok(room, other, &intersection) {
                        let other_tile = self.room(other).tile_shape().expect("a neighbour with a shape");
                        let dimension = tile.intersection(&other_tile).dimension();
                        self.add_door(room, other, dimension);
                    }
                }
            }
        }
        // An obstacle room is never regrown (try_remove_edge_line).
        if n.sorted.is_empty() {
            self.edge_incomplete_rooms_of_obstacle(&n, 0, 7);
        } else {
            self.calculate_new_incomplete_rooms(&n);
        }
    }

    /// An overlap door between two obstacle rooms: of routable items of a
    /// common net, and for one trace only between consecutive segments.
    /// `ObstacleExpansionRoom.create_overlap_door`.
    fn create_overlap_door(&mut self, room: RoomId, other: RoomId) {
        if self.door_exists(room, other) {
            return;
        }
        let item_of = |r: RoomId| match self.room(r).state {
            RoomState::Obstacle { item, .. } => item,
            _ => unreachable!("an obstacle room"),
        };
        let (a, b) = (self.tree.payload(item_of(room)), self.tree.payload(item_of(other)));
        if !(a.is_routable() && b.is_routable()) || !a.shares_net_with(b) {
            return;
        }
        if a.id() == b.id() {
            if !a.is_trace() {
                return;
            }
            let (i, j) = (a.shape_index(), b.shape_index());
            if i != j + 1 && i + 1 != j {
                return;
            }
        }
        self.add_door(room, other, 2);
    }

    /// New incomplete rooms along the free sides `from` to `to` of an
    /// obstacle room, each running out to the board beyond that side.
    /// `calculate_edge_incomplete_rooms_of_obstacle_expansion_room`.
    fn edge_incomplete_rooms_of_obstacle(&mut self, n: &Neighbours, from: usize, to: usize) {
        let board = Oct::of(&self.board.to_octagon());
        let r = Oct::of(&n.shape);
        let mut curr_corner = n.shape.corner(from);
        let mut side = from;
        loop {
            let next = (side + 1) % 8;
            let next_corner = n.shape.corner(next);
            if curr_corner != next_corner {
                let mut o = board;
                match side {
                    0 => o.uy = r.ly,
                    1 => o.ulx = r.lrx,
                    2 => o.lx = r.rx,
                    3 => o.llx = r.urx,
                    4 => o.ly = r.uy,
                    5 => o.lrx = r.ulx,
                    6 => o.rx = r.lx,
                    _ => o.urx = r.llx,
                }
                self.insert_incomplete_room(n, o.octagon());
            }
            if side == to {
                break;
            }
            // The Java compares each side's first corner with the corner of
            // the side it started from, which it never moves on.
            let _ = &mut curr_corner;
            side = next;
        }
    }

    /// New incomplete rooms between two neighbours of an obstacle room: at
    /// the end of `prev`'s touch, at the start of `next`'s, and along the
    /// free sides between. `same` when they are one neighbour, the room's
    /// only one. `calculate_new_incomplete_rooms_for_obstacle_expansion_room`.
    fn obstacle_rooms_between(&mut self, n: &Neighbours, prev: &Neighbour, next: &Neighbour, same: bool) {
        let (from_side, to_side) = (prev.last_side, next.first_side);
        if from_side == to_side && !same {
            return;
        }
        let board = Oct::of(&self.board.to_octagon());
        let r = Oct::of(&n.shape);
        let (p, q) = (Oct::of(&prev.intersection), Oct::of(&next.intersection));
        let mut o = board;
        match from_side {
            0 => {
                o.uy = r.ly;
                o.ulx = p.lrx;
            }
            1 => {
                o.ulx = r.lrx;
                o.lx = p.rx;
            }
            2 => {
                o.lx = r.rx;
                o.llx = p.urx;
            }
            3 => {
                o.llx = r.urx;
                o.ly = p.uy;
            }
            4 => {
                o.ly = r.uy;
                o.lrx = p.ulx;
            }
            5 => {
                o.lrx = r.ulx;
                o.rx = p.lx;
            }
            6 => {
                o.rx = r.lx;
                o.urx = p.llx;
            }
            _ => {
                o.urx = r.llx;
                o.uy = p.ly;
            }
        }
        self.insert_incomplete_room(n, o.octagon());
        let mut o = board;
        match to_side {
            0 => {
                o.uy = r.ly;
                o.urx = q.llx;
            }
            1 => {
                o.ulx = r.lrx;
                o.uy = q.ly;
            }
            2 => {
                o.lx = r.rx;
                o.ulx = q.lrx;
            }
            3 => {
                o.llx = r.urx;
                o.lx = q.rx;
            }
            4 => {
                o.ly = r.uy;
                o.llx = q.urx;
            }
            5 => {
                o.lrx = r.ulx;
                o.ly = q.uy;
            }
            6 => {
                o.rx = r.lx;
                o.lrx = q.ulx;
            }
            _ => {
                o.urx = r.llx;
                o.rx = q.lx;
            }
        }
        self.insert_incomplete_room(n, o.octagon());
        let curr_from = (from_side + 1) % 8;
        if curr_from == to_side {
            return;
        }
        let curr_to = (to_side + 7) % 8;
        self.edge_incomplete_rooms_of_obstacle(n, curr_from, curr_to);
    }

    /// Calculate the doors of a grown room and store it; a room that came
    /// out flat is not stored. `AutorouteEngine.add_complete_room`.
    ///
    /// As in the Java, a flat room still takes a room number, and keeps the
    /// doors calculated for it -- its neighbours still list them -- though
    /// it is in neither the tree nor the list of completed rooms.
    fn add_complete_room(&mut self, mut grown: GrownRoom) -> Option<RoomId> {
        let room = self.calculate_doors(&mut grown);
        if self.shape_of(room).dimension() != 2 {
            return None;
        }
        let (shape, layer) = (self.shape_of(room), self.room(room).layer);
        let RoomState::Complete { id_no, .. } = self.room(room).state else {
            unreachable!("calculate_doors makes a complete room");
        };
        let stored = self.tree.insert(shape, Entry::Room { room, id_no, layer });
        if let RoomState::Complete { leaf, .. } = &mut self.room_mut(room).state {
            *leaf = Some(stored);
        }
        self.complete.push(room);
        Some(room)
    }

    /// `AutorouteEngine.remove_all_doors`: detach every door of `room` from
    /// the room on its other side, removing that room too if it is
    /// incomplete, since a door is its only way in.
    fn remove_all_doors(&mut self, room: RoomId) {
        let doors = std::mem::take(&mut self.room_mut(room).doors);
        for d in doors {
            let Some(door) = self.doors[d.0] else { continue };
            if let Some(other) = door.other(room) {
                self.remove_door_from(other, d);
                if !self.room(other).is_complete() {
                    self.remove_incomplete_room(other);
                }
            }
            self.doors[d.0] = None;
        }
    }

    fn add_door(&mut self, first: RoomId, second: RoomId, dimension: i32) -> DoorId {
        let id = DoorId(self.doors.len());
        self.doors.push(Some(Door { first, second, dimension }));
        self.room_mut(first).doors.push(id);
        self.room_mut(second).doors.push(id);
        id
    }

    fn remove_door_from(&mut self, room: RoomId, door: DoorId) {
        let doors = &mut self.room_mut(room).doors;
        if let Some(i) = doors.iter().position(|&d| d == door) {
            doors.remove(i);
        }
    }

    /// Whether a door may join `room` and `other` where they meet in
    /// `meet`: one per pair; and into the obstacle room of a trace's end
    /// segment only along that segment.
    /// `SortedRoomNeighbours.insert_door_ok`, for a free-space room and a
    /// neighbour.
    fn insert_door_ok(&self, room: RoomId, other: RoomId, meet: &IntOctagon) -> bool {
        if self.door_exists(room, other) {
            return false;
        }
        let end_line = |r: RoomId| match self.room(r).state {
            RoomState::Obstacle { item, .. } => Some(self.tree.payload(item).trace_end_line()),
            _ => None,
        };
        let (first, second) = (end_line(room), end_line(other));
        if first.is_none() && second.is_none() {
            return true;
        }
        // The door's first side of any length.
        let door = TileShape::Octagon(*meet);
        let mut door_line = None;
        let mut prev = door.corner(0);
        for i in 1..door.border_line_count() {
            let curr = door.corner(i);
            if curr != prev {
                door_line = Some(door.border_line(i - 1));
                break;
            }
            prev = curr;
        }
        let ok = |end: Option<Option<Line>>| match end {
            None => true,
            // SortedRoomNeighbours warns and refuses without a door line.
            Some(_) if door_line.is_none() => false,
            Some(Some(line)) => line.is_parallel(&door_line.expect("checked")),
            Some(None) => true,
        };
        ok(first) && ok(second)
    }

    fn door_exists(&self, room: RoomId, other: RoomId) -> bool {
        self.room(room).doors.iter().any(|&d| {
            let door = self.door(d);
            door.first == other || door.second == other
        })
    }

    /// `Sorted45DegreeRoomNeighbours.calculate`, for a free-space room:
    /// make the completed room, its doors to the rooms it touches, and new
    /// incomplete rooms behind the stretches of its border nothing touches.
    /// `from` is regrown in place whenever a side of it touches nothing.
    fn calculate_doors(&mut self, from: &mut GrownRoom) -> RoomId {
        loop {
            let n = self.calculate_neighbours(from);
            if self.try_remove_edge_line(from, &n) {
                // It grew: discard this attempt and start again, as the
                // Java does by recursion.
                self.remove_all_doors(n.completed);
                self.rooms[n.completed.0] = None;
                continue;
            }
            if !n.sorted.is_empty() {
                self.calculate_new_incomplete_rooms(&n);
            }
            return n.completed;
        }
    }

    /// `Sorted45DegreeRoomNeighbours.calculate_neighbours`: a new completed
    /// room for `from`, the objects touching it in border order, and a door
    /// to each touching room.
    fn calculate_neighbours(&mut self, from: &GrownRoom) -> Neighbours {
        self.room_count += 1;
        let completed = RoomId(self.rooms.len());
        self.rooms.push(Some(Room {
            shape: Some(from.shape),
            layer: from.layer,
            state: RoomState::Complete { id_no: self.room_count, leaf: None, net_dependent: false },
            doors: Vec::new(),
            targets: Vec::new(),
            tile: None,
        }));
        let shape = from.shape;
        let mut n = Neighbours { completed, shape, layer: from.layer, sorted: BTreeSet::new(), edge_touched: [false; 8] };

        // ShapeSearchTree.overlapping_tree_entries: what the tree finds on
        // the room's layer, a shape that is not an octagon only if it
        // really touches the room. The Java collects these in the tree's
        // own order -- rooms before items -- then stable-sorts by number
        // and shape index.
        let mut entries: Vec<LeafId> = self
            .tree
            .overlaps(&shape)
            .into_iter()
            .filter(|&l| {
                let p = self.tree.payload(l);
                p.layer() == from.layer && p.exact_shape().is_none_or(|s| s.intersects_octagon(&shape))
            })
            .collect();
        entries.sort_by_key(|&l| {
            let p = self.tree.payload(l);
            (p.id(), p.shape_index(), matches!(p, Entry::Item(_)))
        });
        for leaf in entries {
            let (obstacle, id, neighbour_room, routable) = {
                let p = self.tree.payload(leaf);
                let room = match p {
                    Entry::Room { room, .. } => Some(*room),
                    Entry::Item(_) => None,
                };
                (p.is_obstacle_for(self.net), p.id(), room, p.is_routable())
            };
            if !obstacle {
                // Something no trace of the net avoids -- its own copper, or
                // a via keepout: it does not bound the room, but its own
                // copper may be a target (calculate_target_doors).
                let r = self.room_mut(completed);
                if let RoomState::Complete { net_dependent, .. } = &mut r.state {
                    *net_dependent = true;
                }
                r.targets.push(leaf);
                continue;
            }
            let intersection = shape.intersection(&self.tree.bounds(leaf));
            let dimension = intersection.dimension();
            if dimension < 0 {
                // Two diagonals can meet at a corner with half-integer
                // coordinates, which no integer point lies on.
                continue;
            }
            if let Some(nb) = Neighbour::new(&shape, intersection, id, &mut n.edge_touched) {
                // A TreeSet: a neighbour comparing equal to one already
                // there is dropped.
                n.sorted.insert(nb);
            }
            if dimension > 0 {
                // A routable item is entered through its obstacle room, for
                // ripup and pushing aside.
                let other = match neighbour_room {
                    Some(r) => Some(r),
                    None if routable => Some(self.obstacle_room(leaf)),
                    None => None,
                };
                if let Some(other) = other {
                    if self.insert_door_ok(completed, other, &intersection) {
                        // new ExpansionDoor(room, neighbour): the dimension
                        // of the rooms' own shapes' meeting, which for an
                        // obstacle room is its item's exact shape.
                        let dimension = match &self.room(other).tile {
                            Some(tile) => TileShape::Octagon(shape).intersection(tile).dimension(),
                            None => dimension,
                        };
                        self.add_door(completed, other, dimension);
                    }
                }
            }
        }
        n
    }

    /// `Sorted45DegreeRoomNeighbours.try_remove_edge_line`: if a side of the
    /// room longer than a unit touches nothing, the room could be bigger.
    /// Drop every such border line and regrow; keep the result if it is one
    /// room and larger.
    fn try_remove_edge_line(&mut self, from: &mut GrownRoom, n: &Neighbours) -> bool {
        let room_area = from.shape.area();
        let loose_side = (0..8).any(|i| {
            if n.edge_touched[i] {
                return false;
            }
            let (a, b) = (n.shape.corner(i), n.shape.corner((i + 1) % 8));
            let (dx, dy) = ((a.x - b.x) as f64, (a.y - b.y) as f64);
            dx * dx + dy * dy > 1.0
        });
        if !loose_side {
            return false;
        }
        let enlarged = remove_not_touching_border_lines(&from.shape, &n.edge_touched);
        // Where the room overlaps another completed room, the largest
        // overlap is the door it was entered by: regrowing must not treat
        // it as an obstacle.
        let mut ignore = None;
        let mut max_area = 0.0;
        for &d in &self.room(n.completed).doors {
            let door = self.door(d);
            if door.dimension != 2 {
                continue;
            }
            let Some(other) = door.other(n.completed) else { continue };
            if let RoomState::Complete { leaf: Some(leaf), .. } = self.room(other).state {
                let door_shape = self.door_shape(d);
                let area = door_shape.area();
                if area > max_area {
                    max_area = area;
                    ignore = Some((leaf, door_shape));
                }
            }
        }
        let enlarged_room = IncompleteRoom { shape: Some(enlarged), layer: from.layer, contained: from.contained };
        let grown =
            complete_shape(&self.tree, self.board, &enlarged_room, self.net, ignore.map(|i| i.0), ignore.map(|i| i.1));
        // Only a larger room counts, or this could loop forever.
        if let [room] = grown[..] {
            if room.shape.area() > room_area {
                from.shape = room.shape;
                from.contained = room.contained;
                return true;
            }
        }
        false
    }

    /// `Sorted45DegreeRoomNeighbours.calculate_new_incomplete_rooms`, for a
    /// free-space room: going round the border, wherever two consecutive
    /// neighbours do not touch, the free space between them gets a new
    /// incomplete room. Its shape runs out to the board, bounded by the
    /// room's edge and the two neighbours' ends.
    fn calculate_new_incomplete_rooms(&mut self, n: &Neighbours) {
        let board = Oct::of(&self.board.to_octagon());
        let Some(mut prev) = n.sorted.last().copied() else { return };
        let obstacle = self.room(n.completed).is_obstacle_room();
        if obstacle && n.sorted.len() == 1 {
            self.obstacle_rooms_between(n, &prev, &prev, true);
            return;
        }
        for &next in &n.sorted {
            let insert = if obstacle && n.sorted.len() == 2 {
                // Two neighbours: is the side between them open?
                let meet = next.intersection.intersection(&prev.intersection);
                if meet.is_empty() {
                    true
                } else if meet.dimension() >= 1 {
                    false
                } else if prev.last_side == next.first_side {
                    false
                } else {
                    prev.last_side != (next.first_side + 1) % 8
                }
            } else {
                !next.intersection.intersects(&prev.intersection)
            };
            if insert && obstacle && next.first_side != prev.last_side {
                self.obstacle_rooms_between(n, &prev, &next, false);
            } else if insert {
                // The Java's names: p and q are the previous and next
                // neighbour's touch, o the new room, which starts as the
                // board and is cut down case by case.
                let (p, q) = (Oct::of(&prev.intersection), Oct::of(&next.intersection));
                let mut o = board;
                match next.first_side {
                    0 => {
                        if p.llx < q.llx {
                            o.urx = q.llx;
                            o.uy = p.ly;
                            if prev.last_side == 0 {
                                o.ulx = p.lrx;
                            }
                        } else if p.llx > q.llx {
                            o.rx = q.lx;
                            o.urx = p.llx;
                        } else {
                            o.urx = q.llx;
                        }
                    }
                    1 => {
                        if p.ly < q.ly {
                            o.uy = q.ly;
                            o.ulx = p.lrx;
                            if prev.last_side == 1 {
                                o.lx = p.rx;
                            }
                        } else if p.ly > q.ly {
                            o.uy = p.ly;
                            o.urx = q.llx;
                        } else {
                            o.uy = q.ly;
                        }
                    }
                    2 => {
                        if p.lrx > q.lrx {
                            o.ulx = q.lrx;
                            o.lx = p.rx;
                            if prev.last_side == 2 {
                                o.llx = p.urx;
                            }
                        } else if p.lrx < q.lrx {
                            o.uy = q.ly;
                            o.ulx = p.lrx;
                        } else {
                            o.ulx = q.lrx;
                        }
                    }
                    3 => {
                        if p.rx > q.rx {
                            o.lx = q.rx;
                            o.llx = p.urx;
                            if prev.last_side == 3 {
                                o.ly = p.uy;
                            }
                        } else if p.rx < q.rx {
                            o.lx = p.rx;
                            o.ulx = q.lrx;
                        } else {
                            o.lx = q.rx;
                        }
                    }
                    4 => {
                        if p.urx > q.urx {
                            o.llx = q.urx;
                            o.ly = p.uy;
                            if prev.last_side == 4 {
                                o.lrx = p.ulx;
                            }
                        } else if p.urx < q.urx {
                            o.lx = q.rx;
                            o.llx = p.urx;
                        } else {
                            o.llx = q.urx;
                        }
                    }
                    5 => {
                        if p.uy > q.uy {
                            o.ly = q.uy;
                            o.lrx = p.ulx;
                            if prev.last_side == 5 {
                                o.rx = p.lx;
                            }
                        } else if p.uy < q.uy {
                            o.ly = p.uy;
                            o.llx = q.urx;
                        } else {
                            o.ly = q.uy;
                        }
                    }
                    6 => {
                        if p.ulx < q.ulx {
                            o.lrx = q.ulx;
                            o.rx = p.lx;
                            if prev.last_side == 6 {
                                o.urx = p.llx;
                            }
                        } else if p.ulx > q.ulx {
                            o.ly = q.uy;
                            o.lrx = p.ulx;
                        } else {
                            o.lrx = q.ulx;
                        }
                    }
                    7 => {
                        if p.lx < q.lx {
                            o.rx = q.lx;
                            o.urx = p.llx;
                            if prev.last_side == 7 {
                                o.uy = p.ly;
                            }
                        } else if p.lx > q.lx {
                            o.rx = p.lx;
                            o.lrx = q.ulx;
                        } else {
                            o.rx = q.lx;
                        }
                    }
                    s => unreachable!("touching side {s} out of range"),
                }
                self.insert_incomplete_room(n, o.octagon());
            }
            prev = next;
        }
    }

    /// `Sorted45DegreeRoomNeighbours.insert_incomplete_room`: add the room
    /// if it is an area, and a door to it where it meets the completed
    /// room, if that is more than a point.
    fn insert_incomplete_room(&mut self, n: &Neighbours, shape: IntOctagon) {
        let shape = shape.normalize();
        if shape.dimension() != 2 {
            return;
        }
        let contained = n.shape.intersection(&shape);
        let door_dimension = contained.dimension();
        if door_dimension > 0 {
            let room = self.add_incomplete_room(Some(shape), n.layer, contained);
            self.add_door(n.completed, room, door_dimension);
        }
    }

    /// Checks the structural invariants: each door is listed once by each
    /// of its two rooms and by no other; it is an edge or an overlap, never
    /// a point, and its dimension is that of the rooms' overlap; two rooms
    /// share at most one door; each completed room is stored in the tree
    /// under its own shape; the room lists hold exactly the live rooms, by
    /// state. For tests.
    pub fn check_invariants(&self) -> Result<(), String> {
        for (i, door) in self.doors.iter().enumerate() {
            let Some(door) = door else { continue };
            let id = DoorId(i);
            if !(1..=2).contains(&door.dimension) {
                return Err(format!("{id:?} has dimension {}", door.dimension));
            }
            for r in [door.first, door.second] {
                let Some(room) = &self.rooms[r.0] else { return Err(format!("{id:?} names removed {r:?}")) };
                let listed = room.doors.iter().filter(|&&d| d == id).count();
                if listed != 1 {
                    return Err(format!("{id:?} is listed {listed} times by {r:?}"));
                }
            }
            let (Some(a), Some(b)) = (self.room(door.first).tile_shape(), self.room(door.second).tile_shape()) else {
                return Err(format!("{id:?} has a room without a shape"));
            };
            let overlap = a.intersection(&b).dimension();
            if overlap != door.dimension {
                return Err(format!("{id:?} has dimension {} but its rooms meet in dimension {overlap}", door.dimension));
            }
        }
        let (mut live, mut unstored, mut obstacle) = (0, 0, 0);
        for (i, room) in self.rooms.iter().enumerate() {
            let Some(room) = room else { continue };
            let id = RoomId(i);
            live += 1;
            if room.is_obstacle_room() {
                obstacle += 1;
            }
            let mut others = Vec::new();
            for &d in &room.doors {
                match self.doors[d.0] {
                    None => return Err(format!("{id:?} lists removed {d:?}")),
                    Some(door) => match door.other(id) {
                        None => return Err(format!("{id:?} lists {d:?}, which is not its")),
                        Some(o) => others.push(o),
                    },
                }
            }
            others.sort();
            if let Some(w) = others.windows(2).find(|w| w[0] == w[1]) {
                return Err(format!("{id:?} has two doors to {:?}", w[0]));
            }
            if let RoomState::Complete { id_no, leaf, .. } = room.state {
                let Some(leaf) = leaf else {
                    // A room that came out flat: numbered and with doors, but
                    // never stored.
                    if self.complete.contains(&id) || room.shape.is_some_and(|s| s.dimension() == 2) {
                        return Err(format!("complete {id:?} is not stored"));
                    }
                    unstored += 1;
                    continue;
                };
                match self.tree.payload(leaf) {
                    Entry::Room { room: r, id_no: n, .. } if *r == id && *n == id_no => {}
                    _ => return Err(format!("{leaf:?} does not hold {id:?}")),
                }
                if room.shape != Some(self.tree.bounds(leaf)) {
                    return Err(format!("{id:?} is stored under another shape"));
                }
            }
        }
        for (list, complete) in [(&self.incomplete, false), (&self.complete, true)] {
            for &r in list {
                match &self.rooms[r.0] {
                    Some(room) if room.is_complete() == complete && !room.is_obstacle_room() => {}
                    _ => return Err(format!("{r:?} is on the wrong list or removed")),
                }
            }
        }
        let listed = self.incomplete.len() + self.complete.len();
        if live != listed + unstored + obstacle {
            return Err(format!("{live} live rooms, but the lists hold {listed}, with {unstored} flat and {obstacle} obstacle rooms"));
        }
        Ok(())
    }
}

/// A room being completed and what touches it: the working state of
/// `Sorted45DegreeRoomNeighbours`.
struct Neighbours {
    completed: RoomId,
    /// The room's shape. (The Java takes its bounding octagon, which for an
    /// octagon is itself.)
    shape: IntOctagon,
    layer: i32,
    sorted: BTreeSet<Neighbour>,
    /// Per side: whether some neighbour touches that edge somewhere other
    /// than at an end corner.
    edge_touched: [bool; 8],
}

/// `remove_not_touching_border_lines`: push every border line whose edge
/// touches nothing out to the critical bound, and normalize.
fn remove_not_touching_border_lines(room: &IntOctagon, touched: &[bool; 8]) -> IntOctagon {
    let r = Oct::of(room);
    let keep = |side: usize, bound: i64, open: i64| if touched[side] { bound } else { open };
    Oct {
        lx: keep(6, r.lx, -CRIT),
        ly: keep(0, r.ly, -CRIT),
        rx: keep(2, r.rx, CRIT),
        uy: keep(4, r.uy, CRIT),
        ulx: keep(5, r.ulx, -CRIT),
        lrx: keep(1, r.lrx, CRIT),
        llx: keep(7, r.llx, -CRIT),
        urx: keep(3, r.urx, CRIT),
    }
    .octagon()
    .normalize()
}

/// An object touching the room being completed, and where on the room's
/// border it touches. `SortedRoomNeighbour`.
#[derive(Debug, Clone, Copy)]
struct Neighbour {
    /// The object's number, the last tie-breaker in the ordering.
    id: u64,
    /// The object's overlap with the room: a stretch of border, a corner,
    /// or an area where another room overlaps this one.
    intersection: IntOctagon,
    /// The sides the touch starts and ends on, going counter-clockwise.
    first_side: usize,
    last_side: usize,
}

impl Neighbour {
    /// Place a touch on the border of `room`, marking the sides whose edge
    /// it covers beyond an end corner. `None` if it is not on the border:
    /// the room may lie inside the object.
    fn new(room: &IntOctagon, intersection: IntOctagon, id: u64, edge_touched: &mut [bool; 8]) -> Option<Neighbour> {
        let (is, rs) = (Oct::of(&intersection), Oct::of(room));
        let first_side = if is.ly == rs.ly && is.llx > rs.llx {
            0
        } else if is.lrx == rs.lrx && is.ly > rs.ly {
            1
        } else if is.rx == rs.rx && is.lrx < rs.lrx {
            2
        } else if is.urx == rs.urx && is.rx < rs.rx {
            3
        } else if is.uy == rs.uy && is.urx < rs.urx {
            4
        } else if is.ulx == rs.ulx && is.uy < rs.uy {
            5
        } else if is.lx == rs.lx && is.ulx > rs.ulx {
            6
        } else if is.llx == rs.llx && is.lx > rs.lx {
            7
        } else {
            return None;
        };
        let last_side = if is.llx == rs.llx && is.ly > rs.ly {
            7
        } else if is.lx == rs.lx && is.llx > rs.llx {
            6
        } else if is.ulx == rs.ulx && is.lx > rs.lx {
            5
        } else if is.uy == rs.uy && is.ulx > rs.ulx {
            4
        } else if is.urx == rs.urx && is.uy < rs.uy {
            3
        } else if is.rx == rs.rx && is.urx < rs.urx {
            2
        } else if is.lrx == rs.lrx && is.rx < rs.rx {
            1
        } else if is.ly == rs.ly && is.lrx < rs.lrx {
            0
        } else {
            return None;
        };

        let mut next = first_side;
        loop {
            let side = next;
            next = (next + 1) % 8;
            if !edge_touched[side] {
                // At the first and last side the touch may be only the
                // corner where that side ends or begins.
                let only_corner = (side == first_side && intersection.corner(side) == room.corner(next))
                    || (side == last_side && intersection.corner(next) == room.corner(side));
                if !only_corner {
                    edge_touched[side] = true;
                }
            }
            if side == last_side {
                break;
            }
        }
        Some(Neighbour { id, intersection, first_side, last_side })
    }
}

/// `SortedRoomNeighbour.compareTo`: counter-clockwise round the room by
/// where the touch starts, then by where it ends, then by number.
impl Ord for Neighbour {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.first_side != other.first_side {
            return self.first_side.cmp(&other.first_side);
        }
        let (a, b) = (&self.intersection, &other.intersection);
        let start = match self.first_side {
            0 => a.corner(0).x - b.corner(0).x,
            1 => a.corner(1).x - b.corner(1).x,
            2 => a.corner(2).y - b.corner(2).y,
            3 => a.corner(3).y - b.corner(3).y,
            4 => b.corner(4).x - a.corner(4).x,
            5 => b.corner(5).x - a.corner(5).x,
            6 => b.corner(6).y - a.corner(6).y,
            7 => b.corner(7).y - a.corner(7).y,
            s => unreachable!("touching side {s} out of range"),
        };
        if start != 0 {
            return start.cmp(&0);
        }
        let span = (self.last_side + 8 - self.first_side) % 8;
        let other_span = (other.last_side + 8 - other.first_side) % 8;
        if span != other_span {
            return span.cmp(&other_span);
        }
        let end = match self.last_side {
            0 => a.corner(1).x - b.corner(1).x,
            1 => a.corner(2).x - b.corner(2).x,
            2 => a.corner(3).y - b.corner(3).y,
            3 => a.corner(4).y - b.corner(4).y,
            4 => b.corner(5).x - a.corner(5).x,
            5 => b.corner(6).x - a.corner(6).x,
            6 => b.corner(7).y - a.corner(7).y,
            7 => b.corner(0).y - a.corner(0).y,
            s => unreachable!("touching side {s} out of range"),
        };
        if end != 0 {
            return end.cmp(&0);
        }
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for Neighbour {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Neighbour {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Neighbour {}

/// An octagon's bounds under the Java's short names, so the case analyses
/// above read line for line like the source.
#[derive(Debug, Clone, Copy)]
struct Oct {
    lx: i64,
    ly: i64,
    rx: i64,
    uy: i64,
    ulx: i64,
    lrx: i64,
    llx: i64,
    urx: i64,
}

impl Oct {
    fn of(o: &IntOctagon) -> Oct {
        Oct {
            lx: o.left_x,
            ly: o.bottom_y,
            rx: o.right_x,
            uy: o.top_y,
            ulx: o.upper_left_diag_x,
            lrx: o.lower_right_diag_x,
            llx: o.lower_left_diag_x,
            urx: o.upper_right_diag_x,
        }
    }

    fn octagon(self) -> IntOctagon {
        IntOctagon::new(self.lx, self.ly, self.rx, self.uy, self.ulx, self.lrx, self.llx, self.urx)
    }
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
    /// The board outline's net: an obstacle to every routed net.
    const OUTLINE: i32 = -1;

    fn rng(mut seed: u64) -> impl FnMut(i64, i64) -> i64 {
        move |lo, hi| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % ((hi - lo + 1) as u64)) as i64
        }
    }

    fn point(x: i64, y: i64) -> IntOctagon {
        IntBox::new(x, y, x, y).to_octagon()
    }

    /// A graph whose board is framed by an outline on both layers. Every
    /// real board has one in FreeRouting's tree, so rooms along the edge
    /// have something to touch there.
    fn framed(net: i32) -> RoomGraph<Pad> {
        let mut g = RoomGraph::new(BOARD, net);
        let frame = [
            IntBox::new(-1_000, -1_000, 0, 9_000),
            IntBox::new(10_000, -1_000, 11_000, 9_000),
            IntBox::new(0, -1_000, 10_000, 0),
            IntBox::new(0, 8_000, 10_000, 9_000),
        ];
        let mut id = 1_000_000;
        for layer in 0..2 {
            for b in frame {
                g.insert_item(b.to_octagon(), Pad { id, layer, net: OUTLINE });
                id += 1;
            }
        }
        g
    }

    /// A field of pads on two layers and six nets, some of them octagons
    /// so rooms get real diagonal sides.
    fn pad_field(g: &mut RoomGraph<Pad>, seed: u64, count: u64) {
        let mut next = rng(seed);
        for id in 0..count {
            let (x, y) = (next(200, 9_400), next(200, 7_400));
            let (w, h) = (next(100, 500), next(100, 500));
            let o = IntBox::new(x, y, x + w, y + h).to_octagon();
            let cut = next(0, w.min(h) / 2);
            let o = IntOctagon::new(o.left_x, o.bottom_y, o.right_x, o.top_y, o.upper_left_diag_x + cut, o.lower_right_diag_x - cut, o.lower_left_diag_x + cut, o.upper_right_diag_x - cut)
                .normalize();
            g.insert_item(o, Pad { id, layer: next(0, 1) as i32, net: next(0, 5) as i32 });
        }
    }

    /// Complete rooms until none is incomplete: all the free space the
    /// search could reach from `start`.
    ///
    /// Checks each step: only a room entered through an overlap may
    /// overlap the room behind that door. Entered by an edge, or from the
    /// start, a new room is bounded by every completed room.
    fn fill(g: &mut RoomGraph<Pad>, layer: i32, start: IntOctagon) {
        g.add_incomplete_room(None, layer, start);
        let mut steps = 0;
        while let Some(&r) = g.incomplete_rooms().first() {
            let through_overlap = g.room(r).doors().iter().any(|&d| g.door(d).dimension == 2);
            let new = g.complete_expansion_room(r);
            if !through_overlap {
                for &n in &new {
                    let s = g.room(n).shape.unwrap();
                    for &m in g.complete_rooms().iter().filter(|&&m| m != n) {
                        let other = g.room(m).shape.unwrap();
                        assert!(!s.overlaps(&other), "room {s:?}, entered by an edge, overlaps room {other:?}");
                    }
                }
            }
            steps += 1;
            assert!(steps < 20_000, "filling did not finish");
        }
    }

    fn items_on(g: &RoomGraph<Pad>, layer: i32, pick: impl Fn(&Pad) -> bool) -> Vec<IntOctagon> {
        let all = g.tree().overlaps(&IntBox::new(-2_000, -2_000, 12_000, 10_000).to_octagon());
        all.into_iter()
            .filter(|&l| matches!(g.tree().payload(l), Entry::Item(p) if p.layer == layer && pick(p)))
            .map(|l| g.tree().bounds(l))
            .collect()
    }

    fn strictly_inside(o: &IntOctagon, x: i64, y: i64) -> bool {
        (0..8).all(|i| o.side_of_border_line(x, y, i) < 0)
    }

    fn door_between(g: &RoomGraph<Pad>, a: RoomId, b: RoomId) -> bool {
        g.room(a).doors().iter().any(|&d| g.door(d).other(a) == Some(b))
    }

    /// What a filled layer must be, whatever the obstacles.
    fn check_filled(g: &RoomGraph<Pad>, layer: i32) {
        g.check_invariants().unwrap();
        assert!(g.incomplete_rooms().is_empty());
        let net = g.net();
        let obstacles = items_on(g, layer, |p| p.is_obstacle_for(net));
        let own = items_on(g, layer, |p| !p.is_obstacle_for(net));
        let ids = g.complete_rooms();
        let shapes: Vec<IntOctagon> = ids.iter().map(|&r| g.room(r).shape.unwrap()).collect();
        for (i, r) in shapes.iter().enumerate() {
            assert!(r.is_contained_in(&BOARD.to_octagon()), "room {r:?} leaves the board");
            for o in &obstacles {
                assert!(!r.overlaps(o), "room {r:?} overlaps obstacle {o:?}");
            }
            // Rooms that meet in more than a point have a door, whichever
            // was completed first.
            for (j, s) in shapes.iter().enumerate().skip(i + 1) {
                if r.intersection(s).dimension() >= 1 {
                    assert!(door_between(g, ids[i], ids[j]), "rooms {r:?} and {s:?} meet with no door");
                }
            }
            // Touching the net's own copper makes a room net-dependent.
            let RoomState::Complete { net_dependent, .. } = g.room(ids[i]).state else { unreachable!() };
            assert_eq!(net_dependent, own.iter().any(|o| o.intersects(r)), "room {r:?} net-dependence");
        }
        // Every free point lies in some room.
        for x in (0..=10_000).step_by(97) {
            for y in (0..=8_000).step_by(89) {
                if obstacles.iter().any(|o| strictly_inside(o, x, y)) {
                    continue;
                }
                assert!(shapes.iter().any(|r| r.contains_point(x as f64, y as f64)), "free point ({x}, {y}) is in no room");
            }
        }
    }

    #[test]
    fn an_empty_board_is_tiled_by_rooms() {
        let mut g = framed(1);
        fill(&mut g, 0, point(1_000, 1_000));
        check_filled(&g, 0);
        let area: f64 = g.complete_rooms().iter().map(|&r| g.room(r).shape.unwrap().area()).sum();
        assert!((area - 80_000_000.0).abs() < 1.0, "rooms should tile the board exactly, got {area}");
        assert!(g.complete_rooms().len() > 1, "a layer must not be a single room");
    }

    #[test]
    fn free_space_among_pads_is_covered_by_legal_rooms() {
        for (seed, layer, start) in [(0xC0FFEE, 0, point(50, 50)), (0xBEEF, 1, point(9_950, 7_950)), (7, 0, point(5_000, 50))] {
            let mut g = framed(1);
            pad_field(&mut g, seed, 40);
            fill(&mut g, layer, start);
            check_filled(&g, layer);
        }
    }

    /// A room with a side touching nothing grows until each side touches
    /// something -- here the pad and the outline -- rather than stopping
    /// where it was first cut.
    #[test]
    fn a_side_touching_nothing_is_pushed_out() {
        let mut g = framed(1);
        g.insert_item(IntBox::new(4_000, 3_000, 6_000, 5_000).to_octagon(), Pad { id: 0, layer: 0, net: 7 });
        // A start region cut off short of the pad and the board edge.
        let start = g.add_incomplete_room(Some(IntBox::new(500, 3_500, 2_000, 4_500).to_octagon()), 0, point(1_000, 4_000));
        let rooms = g.complete_expansion_room(start);
        let r = g.room(rooms[0]).shape.unwrap();
        assert_eq!(r.right_x, 4_000, "room {r:?} should reach the pad");
        assert_eq!(r.left_x, 0, "room {r:?} should reach the board edge");
    }

    #[test]
    fn completing_neighbours_leaves_no_incomplete_room_behind_a_door() {
        let mut g = framed(1);
        pad_field(&mut g, 42, 30);
        let start = g.add_incomplete_room(None, 0, point(50, 50));
        let first = g.complete_expansion_room(start)[0];
        assert!(g.room(first).doors().iter().any(|&d| !g.room(g.door(d).other(first).unwrap()).is_complete()), "expected an incomplete neighbour to complete");
        g.complete_neighbour_rooms(first);
        g.check_invariants().unwrap();
        for &d in g.room(first).doors() {
            assert!(g.room(g.door(d).other(first).unwrap()).is_complete());
        }
    }

    #[test]
    fn removing_an_incomplete_room_removes_its_door() {
        let mut g = framed(1);
        g.insert_item(IntBox::new(4_000, 3_000, 6_000, 5_000).to_octagon(), Pad { id: 0, layer: 0, net: 7 });
        let start = g.add_incomplete_room(None, 0, point(1_000, 4_000));
        g.complete_expansion_room(start);
        let incomplete = g.incomplete_rooms().to_vec();
        assert!(!incomplete.is_empty(), "expected incomplete rooms around the pad");
        for r in incomplete {
            g.remove_incomplete_room(r);
            g.check_invariants().unwrap();
        }
        for &r in g.complete_rooms() {
            for &d in g.room(r).doors() {
                assert!(g.room(g.door(d).other(r).unwrap()).is_complete());
            }
        }
    }

    /// The segment between two points on a line at 0, 45 or 90 degrees.
    fn seg(x0: i64, y0: i64, x1: i64, y1: i64) -> IntOctagon {
        let (d0, d1, s0, s1) = (x0 - y0, x1 - y1, x0 + y0, x1 + y1);
        IntOctagon::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), d0.min(d1), d0.max(d1), s0.min(s1), s0.max(s1))
    }

    /// Two touches on each side of an octagon sort counter-clockwise from
    /// the start of the bottom side: rightwards along it, up the
    /// lower-right diagonal, up the right side, and so on round.
    #[test]
    fn neighbours_sort_counter_clockwise() {
        // Corners (30,0) (70,0) (100,30) (100,70) (70,100) (30,100) (0,70) (0,30).
        let room = IntOctagon::new(0, 0, 100, 100, -70, 70, 30, 170).normalize();
        let ccw = [
            seg(40, 0, 45, 0),
            seg(55, 0, 60, 0),
            seg(75, 5, 80, 10),
            seg(85, 15, 90, 20),
            seg(100, 40, 100, 45),
            seg(100, 55, 100, 60),
            seg(95, 75, 90, 80),
            seg(85, 85, 80, 90),
            seg(60, 100, 55, 100),
            seg(45, 100, 40, 100),
            seg(25, 95, 20, 90),
            seg(15, 85, 10, 80),
            seg(0, 60, 0, 55),
            seg(0, 45, 0, 40),
            seg(5, 25, 10, 20),
            seg(15, 15, 20, 10),
        ];
        let mut edges = [false; 8];
        // Numbered against the order and inserted scrambled, so only the
        // geometry can sort them.
        let mut set = BTreeSet::new();
        for k in [7, 12, 0, 15, 3, 9, 14, 1, 5, 10, 2, 13, 8, 4, 11, 6] {
            let n = Neighbour::new(&room, ccw[k], 100 - k as u64, &mut edges).unwrap();
            assert_eq!((n.first_side, n.last_side), (k / 2, k / 2), "touch {k} lies inside side {}", k / 2);
            set.insert(n);
        }
        let order: Vec<u64> = set.iter().map(|n| 100 - n.id).collect();
        assert_eq!(order, (0..16).collect::<Vec<u64>>());
        assert_eq!(edges, [true; 8]);
    }

    /// Touches starting at one point sort by where they end: sooner first,
    /// and one that turns a corner after one that does not.
    #[test]
    fn touches_from_one_point_sort_by_where_they_end() {
        let room = IntBox::new(0, 0, 100, 100).to_octagon();
        let mut edges = [false; 8];
        let mut set = BTreeSet::new();
        // An overlapping room's corner: along the bottom and up the right side.
        set.insert(Neighbour::new(&room, IntBox::new(20, 0, 100, 20).to_octagon(), 1, &mut edges).unwrap());
        set.insert(Neighbour::new(&room, seg(20, 0, 60, 0), 2, &mut edges).unwrap());
        set.insert(Neighbour::new(&room, seg(20, 0, 40, 0), 3, &mut edges).unwrap());
        let order: Vec<u64> = set.iter().map(|n| n.id).collect();
        assert_eq!(order, [3, 2, 1]);
    }

    /// Removing a completed room's doors removes the incomplete rooms
    /// behind them, which have no other way in. Nothing ported reaches this
    /// yet; removing completed rooms, for ripup, will.
    #[test]
    fn removing_a_rooms_doors_removes_the_incomplete_rooms_behind_them() {
        let mut g = framed(1);
        g.insert_item(IntBox::new(4_000, 3_000, 6_000, 5_000).to_octagon(), Pad { id: 0, layer: 0, net: 7 });
        let start = g.add_incomplete_room(None, 0, point(1_000, 4_000));
        let room = g.complete_expansion_room(start)[0];
        let behind: Vec<RoomId> =
            g.room(room).doors().iter().map(|&d| g.door(d).other(room).unwrap()).filter(|&o| !g.room(o).is_complete()).collect();
        assert!(!behind.is_empty(), "expected incomplete rooms around the pad");
        g.remove_all_doors(room);
        assert!(g.room(room).doors().is_empty());
        for r in behind {
            assert!(!g.incomplete_rooms().contains(&r), "{r:?} was left without a way in");
        }
        g.check_invariants().unwrap();
    }

    /// Touching only at a corner does not count as touching either side
    /// that meets there.
    #[test]
    fn a_corner_touch_touches_no_side() {
        let room = IntBox::new(0, 0, 100, 100).to_octagon();
        let mut edges = [false; 8];
        let n = Neighbour::new(&room, point(100, 100), 1, &mut edges).unwrap();
        assert_eq!((n.first_side, n.last_side), (2, 4));
        // Only the zero-length diagonal side between them is marked.
        assert_eq!(edges, [false, false, false, true, false, false, false, false]);
    }

    /// A touch covering a whole side, corners included, starts on the side
    /// before and ends on the side after, but marks neither: only its own
    /// side and the zero-length diagonals at its ends.
    #[test]
    fn a_touch_along_a_whole_side_marks_no_side_beyond_its_corners() {
        let room = IntBox::new(0, 0, 100, 100).to_octagon();
        let mut edges = [false; 8];
        let n = Neighbour::new(&room, IntBox::new(0, 0, 100, 0).to_octagon(), 1, &mut edges).unwrap();
        assert_eq!((n.first_side, n.last_side), (6, 2));
        assert_eq!(edges, [true, true, false, false, false, false, false, true]);
    }

    /// Two touches equal in every way but the number are both kept; equal
    /// in number too, the second is dropped, as by the Java's TreeSet.
    #[test]
    fn only_a_touch_equal_in_every_way_is_dropped() {
        let room = IntBox::new(0, 0, 100, 100).to_octagon();
        let mut edges = [false; 8];
        let s = IntBox::new(20, 0, 40, 0).to_octagon();
        let mut set = BTreeSet::new();
        assert!(set.insert(Neighbour::new(&room, s, 1, &mut edges).unwrap()));
        assert!(set.insert(Neighbour::new(&room, s, 2, &mut edges).unwrap()));
        assert!(!set.insert(Neighbour::new(&room, s, 1, &mut edges).unwrap()));
        assert_eq!(set.len(), 2);
    }
}
