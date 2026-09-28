//! The state of one connection's maze search: the free space as rooms and
//! doors over the autoroute tree, the targets in it, and the places a via
//! could go. Ported from FreeRouting's `AutorouteEngine` and the objects the
//! search expands: `ExpansionDoor` sections, `TargetItemExpansionDoor`,
//! `DrillPage`, `DrillPageArray` and `ExpansionDrill`.

use std::collections::HashMap;

use crate::board::{tree_shapes, TreeKind};
use crate::door::{DoorId, Entry, RoomGraph, RoomId, RoomState};
use crate::geometry::{FloatLine, FloatPoint, IntBox, IntOctagon, IntPoint, Line, Point, PolylineArea, PolylineShape, Side, TileShape, CRIT};
use crate::model::{Board, ItemKind};
use crate::room::TreeObject;
use crate::searchtree::ShapeTree;

/// Added to a trace's half width wherever door widths are judged.
/// `AutorouteEngine.TRACE_WIDTH_TOLERANCE`.
pub const TRACE_WIDTH_TOLERANCE: f64 = 2.0;

/// One of an item's shapes in a search tree.
#[derive(Debug, Clone)]
pub struct TreeItem {
    /// Index into [`Board::items`].
    pub item: usize,
    pub id: u32,
    pub shape_index: u32,
    pub layer: i32,
    /// `is_trace_obstacle` for the net being routed.
    pub trace_obstacle: bool,
    pub routable: bool,
    /// The tree shape where it is not an octagon.
    pub exact: Option<TileShape>,
    /// See [`TreeObject::trace_end_line`].
    pub end_line: Option<Line>,
}

impl TreeObject for TreeItem {
    fn id(&self) -> u64 {
        self.id as u64
    }
    fn shape_index(&self) -> u32 {
        self.shape_index
    }
    fn layer(&self) -> i32 {
        self.layer
    }
    fn is_obstacle_for(&self, _net: i32) -> bool {
        self.trace_obstacle
    }
    fn is_free_space_room(&self) -> bool {
        false
    }
    fn exact_shape(&self) -> Option<&TileShape> {
        self.exact.as_ref()
    }
    fn is_routable(&self) -> bool {
        self.routable
    }
    fn trace_end_line(&self) -> Option<Line> {
        self.end_line
    }
}

/// `MazeSearchElement.Adjustment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Adjustment {
    #[default]
    None,
    Right,
    Left,
}

/// Where the search stands in one section of something it expands.
/// `MazeSearchElement`.
#[derive(Debug, Clone, Default)]
pub struct SearchElement {
    pub is_occupied: bool,
    pub backtrack_door: Option<Expandable>,
    pub section_no_of_backtrack_door: usize,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    pub ripup_cost: i32,
}

/// Something the search expands: a door between rooms, a target, a drill
/// page, or a drill. FreeRouting's `ExpandableObject`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Expandable {
    Door(DoorId),
    Target(usize),
    Page(usize),
    Drill(usize),
}

/// A door into a room onto one of the net's own items.
/// `TargetItemExpansionDoor`.
#[derive(Debug, Clone)]
pub struct TargetDoor {
    /// Index into [`Board::items`].
    pub item: usize,
    pub tree_entry_no: u32,
    pub room: RoomId,
    /// The item's tree shape cut to the room.
    pub shape: TileShape,
    pub element: SearchElement,
}

/// A place for a via: a convex piece of a drill page clear of obstacles,
/// its location, and the room it opens into on each layer.
/// `ExpansionDrill`.
#[derive(Debug, Clone)]
pub struct Drill {
    pub shape: TileShape,
    pub location: IntPoint,
    pub first_layer: i32,
    pub last_layer: i32,
    pub rooms: Vec<RoomId>,
    pub elements: Vec<SearchElement>,
}

/// A rectangle of the board whose drills are worked out when the search
/// first reaches it. `DrillPage`.
#[derive(Debug, Clone)]
pub struct DrillPage {
    pub shape: IntBox,
    pub elements: Vec<SearchElement>,
    drills: Option<Vec<usize>>,
    net_no: i32,
}

/// The board tiled into drill pages. `DrillPageArray`.
#[derive(Debug, Clone)]
pub struct DrillPageArray {
    bounds: IntBox,
    columns: usize,
    rows: usize,
    page_width: i64,
    page_height: i64,
    /// Row by row.
    pub pages: Vec<DrillPage>,
}

impl DrillPageArray {
    pub fn new(bounds: IntBox, max_page_width: i64, layer_count: usize) -> Self {
        let length = (bounds.ur.x - bounds.ll.x) as f64;
        let height = (bounds.ur.y - bounds.ll.y) as f64;
        let columns = (length / max_page_width as f64).ceil() as usize;
        let rows = (height / max_page_width as f64).ceil() as usize;
        let page_width = (length / columns as f64).ceil() as i64;
        let page_height = (height / rows as f64).ceil() as i64;
        let mut pages = Vec::with_capacity(rows * columns);
        for j in 0..rows {
            for i in 0..columns {
                let ll_x = bounds.ll.x + i as i64 * page_width;
                let ur_x = if i == columns - 1 { bounds.ur.x } else { ll_x + page_width };
                let ll_y = bounds.ll.y + j as i64 * page_height;
                let ur_y = if j == rows - 1 { bounds.ur.y } else { ll_y + page_height };
                pages.push(DrillPage {
                    shape: IntBox::new(ll_x, ll_y, ur_x, ur_y),
                    elements: vec![SearchElement::default(); layer_count],
                    drills: None,
                    net_no: -1,
                });
            }
        }
        DrillPageArray { bounds, columns, rows, page_width, page_height, pages }
    }

    /// The pages `shape` overlaps with area, row by row.
    /// `DrillPageArray.overlapping_pages`.
    pub fn overlapping_pages(&self, shape: &TileShape) -> Vec<usize> {
        let sb = shape.bounding_box().intersection(&self.bounds);
        let min_j = (((sb.ll.y - self.bounds.ll.y) as f64) / self.page_height as f64).floor() as i64;
        let max_j = ((sb.ur.y - self.bounds.ll.y) as f64) / self.page_height as f64;
        let min_i = (((sb.ll.x - self.bounds.ll.x) as f64) / self.page_width as f64).floor() as i64;
        let max_i = ((sb.ur.x - self.bounds.ll.x) as f64) / self.page_width as f64;
        let mut result = Vec::new();
        let mut j = min_j;
        while (j as f64) < max_j {
            let mut i = min_i;
            while (i as f64) < max_i {
                let index = j as usize * self.columns + i as usize;
                debug_assert!((j as usize) < self.rows);
                let page = &self.pages[index];
                if shape.intersection(&TileShape::Box(page.shape)).dimension() > 1 {
                    result.push(index);
                }
                i += 1;
            }
            j += 1;
        }
        result
    }
}

/// The plain (default) search tree: every item's exact shapes, clearance
/// uncompensated. What the via and push checks search.
pub struct DefaultTree {
    /// Each entry: item index, shape index, layer.
    tree: ShapeTree<(usize, u32, i32)>,
    shapes: HashMap<(usize, u32), TileShape>,
}

impl DefaultTree {
    pub fn new(board: &Board) -> Self {
        let mut tree = ShapeTree::new();
        let mut shapes = HashMap::new();
        for (i, item) in board.items.iter().enumerate() {
            let all = tree_shapes(board, item, TreeKind::Plain, 0);
            let count = all.len();
            for (k, shape) in all.into_iter().enumerate() {
                let Some(shape) = shape else { continue };
                let Some(bounds) = shape.bounding_octagon() else { continue };
                tree.insert(bounds, (i, k as u32, item.shape_layer(k, board.layer_count(), count)));
                shapes.insert((i, k as u32), shape);
            }
        }
        DefaultTree { tree, shapes }
    }

    pub fn shape(&self, item: usize, index: u32) -> &TileShape {
        &self.shapes[&(item, index)]
    }

    /// The entries whose bounding octagons meet `bounds`, in FreeRouting's
    /// order: by item number descending, then shape index; each with its
    /// layer. `MinAreaTree.overlaps`.
    pub fn candidates(&self, board: &Board, bounds: &IntOctagon) -> Vec<(usize, u32, i32)> {
        let mut found: Vec<(usize, u32, i32)> = self.tree.overlaps(bounds).into_iter().map(|l| *self.tree.payload(l)).collect();
        found.sort_by(|a, b| board.items[b.0].id.cmp(&board.items[a.0].id).then(a.1.cmp(&b.1)));
        found
    }

    /// The entries touching `shape` on `layer` (every layer if negative),
    /// skipping items no obstacle to one of `ignore_nets`, in FreeRouting's
    /// order: by item number descending, then shape index; each with its
    /// layer. `ShapeSearchTree.overlapping_tree_entries`.
    pub fn overlapping_entries(&self, board: &Board, shape: &TileShape, layer: i32, ignore_nets: &[i32]) -> Vec<(usize, u32, i32)> {
        let Some(bounds) = shape.bounding_octagon() else { return Vec::new() };
        let found = self.candidates(board, &bounds);
        let is_45_degree = matches!(shape, TileShape::Octagon(_));
        found
            .into_iter()
            .filter(|&(i, k, l)| {
                if layer >= 0 && l != layer {
                    return false;
                }
                let item = &board.items[i];
                if ignore_nets.iter().any(|&net| !item.is_obstacle(net)) {
                    return false;
                }
                let s = self.shape(i, k);
                (is_45_degree && matches!(s, TileShape::Octagon(_))) || s.intersects(shape)
            })
            .collect()
    }
}

/// The search state for one connection. `AutorouteEngine`, with the
/// objects it hands the maze search.
pub struct Engine<'b> {
    pub board: &'b Board,
    pub net: i32,
    pub trace_clearance_class: i32,
    pub graph: RoomGraph<TreeItem>,
    pub default_tree: DefaultTree,
    pub pages: DrillPageArray,
    pub targets: Vec<TargetDoor>,
    room_targets: HashMap<RoomId, Vec<usize>>,
    pub drills: Vec<Drill>,
    sections: HashMap<DoorId, Vec<SearchElement>>,
    /// `ItemAutorouteInfo.start_info`, by item index.
    pub start_info: HashMap<usize, bool>,
    /// Each item's autoroute tree shapes, by item index and shape index.
    tree_shapes: HashMap<(usize, u32), TileShape>,
    /// How many shapes each item has in the autoroute tree, counting those
    /// FreeRouting has none for: `Item.tree_shape_count`.
    tree_counts: Vec<usize>,
}

impl<'b> Engine<'b> {
    /// A fresh engine for routing `net` with traces of clearance class
    /// `trace_class`: the autoroute tree built from the board, items in
    /// board order, and the drill pages. `AutorouteEngine(board, class,
    /// false)` then `init_connection(net)`.
    pub fn new(board: &'b Board, net: i32, trace_class: i32) -> Self {
        let mut graph = RoomGraph::new(board.bounds, net);
        let mut shapes = HashMap::new();
        let mut tree_counts = Vec::with_capacity(board.items.len());
        for (i, item) in board.items.iter().enumerate() {
            let tree = tree_shapes(board, item, TreeKind::FortyFive, trace_class);
            let count = tree.len();
            tree_counts.push(count);
            for (k, shape) in tree.into_iter().enumerate() {
                let Some(shape) = shape else { continue };
                let Some(bounds) = shape.bounding_octagon() else { continue };
                let end_line = match &item.kind {
                    ItemKind::Trace { polyline, .. } if k == 0 || k + 1 == count => Some(polyline.lines[k + 1]),
                    _ => None,
                };
                let exact = if matches!(shape, TileShape::Octagon(_)) { None } else { Some(shape.clone()) };
                let entry = TreeItem {
                    item: i,
                    id: item.id,
                    shape_index: k as u32,
                    layer: item.shape_layer(k, board.layer_count(), count),
                    trace_obstacle: item.is_trace_obstacle(net),
                    routable: item.is_routable(),
                    exact,
                    end_line,
                };
                graph.insert_item(bounds, entry);
                shapes.insert((i, k as u32), shape);
            }
        }
        let max_page_width = ((5.0 * board.rules.default_via_diameter) as i64).max(10_000);
        Engine {
            board,
            net,
            trace_clearance_class: trace_class,
            graph,
            default_tree: DefaultTree::new(board),
            pages: DrillPageArray::new(board.bounds, max_page_width, board.layer_count()),
            targets: Vec::new(),
            room_targets: HashMap::new(),
            drills: Vec::new(),
            sections: HashMap::new(),
            start_info: HashMap::new(),
            tree_shapes: shapes,
            tree_counts,
        }
    }

    /// `Item.tree_shape_count` in the autoroute tree.
    pub fn tree_shape_count(&self, item: usize) -> usize {
        self.tree_counts[item]
    }

    /// An item's shape in the autoroute tree.
    pub fn tree_shape(&self, item: usize, index: u32) -> Option<&TileShape> {
        self.tree_shapes.get(&(item, index))
    }

    /// Where a trace of the net connects to item `item`'s entry `index`: a
    /// pin's or via's centre, a trace segment's centre line, a pour's tree
    /// shape. `Connectable.get_trace_connection_shape`.
    pub fn connection_shape(&self, item: usize, index: u32) -> Option<TileShape> {
        let it = &self.board.items[item];
        match &it.kind {
            ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => Some(TileShape::Box(IntBox::new(center.x, center.y, center.x, center.y))),
            ItemKind::Trace { polyline, .. } => {
                // LineSegment(lines, index + 1).to_simplex().simplify(): the
                // segment between its neighbours' lines, as a polygon.
                let n = polyline.lines.len();
                let k = index as usize + 1;
                if k == 0 || k + 1 >= n {
                    return None;
                }
                Some(segment_simplex(&polyline.lines[k - 1], &polyline.lines[k], &polyline.lines[k + 1]))
            }
            ItemKind::Area { kind: crate::model::AreaKind::Conduction { .. }, .. } => self.tree_shape(item, index).cloned(),
            _ => None,
        }
    }

    /// A completed room's target doors, made the first time they are asked
    /// for, in the order their items touched the room.
    /// `CompleteFreeSpaceExpansionRoom.calculate_target_doors`.
    pub fn target_doors(&mut self, room: RoomId) -> Vec<usize> {
        if let Some(t) = self.room_targets.get(&room) {
            return t.clone();
        }
        let r = self.graph.room(room);
        let mut made = Vec::new();
        if let (RoomState::Complete { .. }, Some(room_shape)) = (r.state, r.shape) {
            let leaves = r.targets.clone();
            for leaf in leaves {
                let Entry::Item(entry) = self.graph.tree().payload(leaf) else { continue };
                let (item, index) = (entry.item, entry.shape_index);
                let it = &self.board.items[item];
                if !it.is_connectable_kind() || !it.contains_net(self.net) {
                    continue;
                }
                let Some(connection) = self.connection_shape(item, index) else { continue };
                if !TileShape::Octagon(room_shape).intersects(&connection) {
                    continue;
                }
                let item_shape = self.tree_shape(item, index).expect("tree entry").clone();
                let shape = item_shape.intersection(&TileShape::Octagon(room_shape));
                self.targets.push(TargetDoor { item, tree_entry_no: index, room, shape, element: SearchElement::default() });
                made.push(self.targets.len() - 1);
            }
        }
        self.room_targets.insert(room, made.clone());
        made
    }

    /// Complete an incomplete room: `AutorouteEngine.complete_expansion_room`.
    pub fn complete_expansion_room(&mut self, room: RoomId) -> Vec<RoomId> {
        self.graph.complete_expansion_room(room)
    }

    /// `AutorouteEngine.complete_neighbour_rooms`.
    pub fn complete_neighbour_rooms(&mut self, room: RoomId) {
        if self.graph.room(room).is_obstacle_room() {
            // Obstacle rooms get their doors only when completed for ripup.
            return;
        }
        self.graph.complete_neighbour_rooms(room);
    }

    /// FreeRouting's number for a room: `get_id_no`.
    pub fn room_id_no(&self, room: RoomId) -> i64 {
        self.graph.room(room).id_no().expect("a complete room")
    }

    /// A door's sections, as last allocated.
    pub fn door_sections(&self, door: DoorId) -> &[SearchElement] {
        self.sections.get(&door).map_or(&[], |v| v.as_slice())
    }

    /// The door's width divided into sections for traces of half width
    /// `offset`, each a segment; allocating the door's search elements anew
    /// if their number changed. None for a door too narrow.
    /// `ExpansionDoor.get_section_segments`.
    pub fn section_segments(&mut self, door: DoorId, offset: f64) -> Vec<FloatLine> {
        let offset = offset + TRACE_WIDTH_TOLERANCE;
        let shape = self.graph.door_tile_shape(door);
        if shape.is_empty() {
            return Vec::new();
        }
        let d = *self.graph.door(door);
        let free = |r: RoomId| matches!(self.graph.room(r).state, RoomState::Complete { .. });
        let (door_line, shrinked) = if d.dimension == 1 {
            let line = shape.diagonal_corner_segment().expect("not empty");
            (line, line.shrink_segment(offset))
        } else if d.dimension == 2 && free(d.first) && free(d.second) {
            let Some(line) = self.door_line_segment(door, &shape) else { return Vec::new() };
            if line.b.distance_square(&line.a) < 4.0 * offset * offset {
                return Vec::new();
            }
            (line, line.shrink_segment(offset))
        } else {
            let g = shape.centre_of_gravity();
            (FloatLine::point(g), FloatLine::point(g))
        };
        let max_section_width = 10.0 * offset;
        let count = (door_line.b.distance(&door_line.a) / max_section_width) as usize + 1;
        let sections = self.sections.entry(door).or_default();
        if sections.len() != count {
            *sections = vec![SearchElement::default(); count];
        }
        shrinked.divide_segment_into_sections(count)
    }

    /// The segment between the first two distinct corners of an overlap
    /// door that lie inside neither room. `calc_door_line_segment`.
    fn door_line_segment(&self, door: DoorId, shape: &TileShape) -> Option<FloatLine> {
        let d = self.graph.door(door);
        let first = self.graph.room(d.first).tile_shape()?;
        let second = self.graph.room(d.second).tile_shape()?;
        let mut first_corner: Option<Point> = None;
        for i in 0..shape.border_line_count() {
            let c = shape.corner(i);
            if !first.contains_inside(&c) && !second.contains_inside(&c) {
                match first_corner {
                    None => first_corner = Some(c),
                    Some(f) if f != c => {
                        let (fx, fy) = f.to_float();
                        let (cx, cy) = c.to_float();
                        return Some(FloatLine::new(FloatPoint::new(fx, fy), FloatPoint::new(cx, cy)));
                    }
                    Some(_) => {}
                }
            }
        }
        None
    }

    /// The search element of `e`'s section `no`.
    pub fn element(&self, e: Expandable, no: usize) -> &SearchElement {
        match e {
            Expandable::Door(d) => &self.sections[&d][no],
            Expandable::Target(t) => &self.targets[t].element,
            Expandable::Page(p) => &self.pages.pages[p].elements[no],
            Expandable::Drill(d) => &self.drills[d].elements[no],
        }
    }

    pub fn element_mut(&mut self, e: Expandable, no: usize) -> &mut SearchElement {
        match e {
            Expandable::Door(d) => &mut self.sections.get_mut(&d).expect("sections allocated")[no],
            Expandable::Target(t) => &mut self.targets[t].element,
            Expandable::Page(p) => &mut self.pages.pages[p].elements[no],
            Expandable::Drill(d) => &mut self.drills[d].elements[no],
        }
    }

    /// The shape of an expandable object. `ExpandableObject.get_shape`.
    pub fn shape_of(&self, e: Expandable) -> TileShape {
        match e {
            Expandable::Door(d) => self.graph.door_tile_shape(d),
            Expandable::Target(t) => self.targets[t].shape.clone(),
            Expandable::Page(p) => TileShape::Box(self.pages.pages[p].shape),
            Expandable::Drill(d) => self.drills[d].shape.clone(),
        }
    }

    /// `ExpandableObject.get_dimension`.
    pub fn dimension_of(&self, e: Expandable) -> i32 {
        match e {
            Expandable::Door(d) => self.graph.door(d).dimension,
            _ => 2,
        }
    }

    /// The room on the other side of `e` from `room`, if `e` is a door and
    /// that room is complete. `ExpandableObject.other_room`.
    pub fn other_room(&self, e: Expandable, room: RoomId) -> Option<RoomId> {
        match e {
            Expandable::Door(d) => {
                let other = self.graph.door(d).other(room)?;
                self.graph.room(other).is_complete().then_some(other)
            }
            _ => None,
        }
    }

    /// FreeRouting's number for an expandable object, in Java `int`
    /// arithmetic: it breaks ties in the search's queue. `get_id_no`.
    pub fn id_no(&self, e: Expandable) -> i32 {
        match e {
            Expandable::Door(d) => {
                let door = self.graph.door(d);
                let (a, b) = (self.room_id_no(door.first) as i32, self.room_id_no(door.second) as i32);
                a.min(b).wrapping_mul(31).wrapping_add(a.max(b))
            }
            Expandable::Target(t) => {
                let target = &self.targets[t];
                (self.board.items[target.item].id as i32).wrapping_mul(31).wrapping_add(self.room_id_no(target.room) as i32)
            }
            Expandable::Page(p) => box_id_no(&self.pages.pages[p].shape).wrapping_mul(31).wrapping_add(self.pages.pages[p].net_no),
            Expandable::Drill(d) => {
                let drill = &self.drills[d];
                31i32.wrapping_mul(31i32.wrapping_mul(point_id_no(drill.location)).wrapping_add(drill.first_layer)).wrapping_add(drill.last_layer)
            }
        }
    }

    /// The drills of page `page` for the net, worked out on first asking:
    /// the page less every obstacle to vias in the default tree, cut into
    /// convex pieces, each a drill at its centre -- or at an SMD pin's centre
    /// where vias may attach to SMD pins -- with the room it opens into on
    /// every layer. `DrillPage.get_drills`.
    pub fn page_drills(&mut self, page: usize, attach_smd: bool) -> Vec<usize> {
        if let Some(d) = &self.pages.pages[page].drills {
            if self.pages.pages[page].net_no == self.net {
                return d.clone();
            }
        }
        self.pages.pages[page].net_no = self.net;
        let page_shape = self.pages.pages[page].shape;
        let board = self.board;
        let overlaps = self.default_tree.overlapping_entries(board, &TileShape::Box(page_shape), -1, &[]);
        let mut cutouts = Vec::new();
        let mut prev_obstacle = TileShape::Box(IntBox::new(CRIT, CRIT, -CRIT, -CRIT));
        for (i, k, _) in overlaps {
            let item = &board.items[i];
            if item.is_drillable(self.net) {
                continue;
            }
            if attach_smd && item.drill_allowed() {
                continue;
            }
            let obstacle = self.default_tree.shape(i, k).clone();
            if !prev_obstacle.contains_shape(&obstacle) {
                let cutout = obstacle.intersection(&TileShape::Box(page_shape));
                if cutout.dimension() == 2 {
                    cutouts.push(PolylineShape::Tile(cutout));
                }
            }
            prev_obstacle = obstacle;
        }
        let area = PolylineArea { border: PolylineShape::Tile(TileShape::Box(page_shape)), holes: cutouts };
        let pieces = area.split_to_convex().unwrap_or_default();
        let (first_layer, last_layer) = (0, board.layer_count() as i32 - 1);
        let mut drills = Vec::new();
        for shape in pieces {
            let mut location = None;
            if attach_smd {
                location = self.pin_center_in_drill(&shape, first_layer).or_else(|| self.pin_center_in_drill(&shape, last_layer));
            }
            let location = location.unwrap_or_else(|| shape.centre_of_gravity().round());
            if let Some(rooms) = self.drill_rooms(location, first_layer, last_layer) {
                let n = (last_layer - first_layer + 1) as usize;
                self.drills.push(Drill { shape, location, first_layer, last_layer, rooms, elements: vec![SearchElement::default(); n] });
                drills.push(self.drills.len() - 1);
            }
        }
        self.pages.pages[page].drills = Some(drills.clone());
        drills
    }

    /// The centre of an SMD pin a via may attach to that lies inside the
    /// drill shape on `layer`, the last such pin found.
    /// `DrillPage.calc_pin_center_in_drill`.
    fn pin_center_in_drill(&self, shape: &TileShape, layer: i32) -> Option<IntPoint> {
        // board.overlapping_items: the default tree's items on the layer.
        let mut result = None;
        let mut seen = Vec::new();
        for (i, _, _) in self.default_tree.overlapping_entries(self.board, shape, layer, &[]) {
            if seen.contains(&i) {
                continue;
            }
            seen.push(i);
            let item = &self.board.items[i];
            if let ItemKind::Pin { center, .. } = item.kind {
                if item.drill_allowed() && shape.contains_inside(&Point::Int(center)) {
                    result = Some(center);
                }
            }
        }
        result
    }

    /// The room at `location` on each layer from `first` to `last`: a
    /// completed room found there, else one grown from the point. `None`
    /// where growing gives other than one room.
    /// `ExpansionDrill.calculate_expansion_rooms`.
    fn drill_rooms(&mut self, location: IntPoint, first: i32, last: i32) -> Option<Vec<RoomId>> {
        let point = TileShape::Box(IntBox::new(location.x, location.y, location.x, location.y));
        let mut overlaps = self.overlapping_rooms(&point);
        let mut rooms = Vec::new();
        for layer in first..=last {
            let found = overlaps.iter().position(|&r| self.graph.room(r).layer == layer).map(|p| overlaps.remove(p));
            let room = match found {
                Some(r) => r,
                None => {
                    let contained = IntOctagon::new(location.x, location.y, location.x, location.y, location.x - location.y, location.x - location.y, location.x + location.y, location.x + location.y);
                    let incomplete = self.graph.add_incomplete_room(None, layer, contained);
                    let made = self.complete_expansion_room(incomplete);
                    if made.len() != 1 {
                        return None;
                    }
                    made[0]
                }
            };
            rooms.push(room);
        }
        Some(rooms)
    }

    /// The completed rooms of the autoroute tree touching `shape`, in the
    /// order FreeRouting's `overlapping_objects` gives them: rooms by number
    /// descending.
    pub fn overlapping_rooms(&self, shape: &TileShape) -> Vec<RoomId> {
        let Some(bounds) = shape.bounding_octagon() else { return Vec::new() };
        let tree = self.graph.tree();
        let mut rooms: Vec<(u64, RoomId)> = tree
            .overlaps(&bounds)
            .into_iter()
            .filter_map(|l| match tree.payload(l) {
                Entry::Room { room, id_no, .. } => {
                    let s = TileShape::Octagon(tree.bounds(l));
                    s.intersects(shape).then_some((*id_no, *room))
                }
                Entry::Item(_) => None,
            })
            .collect();
        rooms.sort_by(|a, b| b.0.cmp(&a.0));
        rooms.into_iter().map(|(_, r)| r).collect()
    }
}

/// `IntPoint.get_id_no`, in Java `int` arithmetic.
fn point_id_no(p: IntPoint) -> i32 {
    31i32.wrapping_mul(p.x as i32).wrapping_add(p.y as i32)
}

/// `IntBox.get_id_no`.
fn box_id_no(b: &IntBox) -> i32 {
    31i32.wrapping_mul(point_id_no(b.ll)).wrapping_add(point_id_no(b.ur))
}

/// A trace segment's centre line as a polygon: the line both ways round,
/// cut off by the lines before and after it, each turned to face the
/// segment. `LineSegment.to_simplex().simplify()`.
fn segment_simplex(start: &Line, middle: &Line, end: &Line) -> TileShape {
    let start_point = middle.intersection(start);
    let end_point = middle.intersection(end);
    // Point.side_of(Line), which is Line.side_of(Point) the other way.
    let first = if start.side_of(&end_point).negate() == Side::Right { start.opposite() } else { *start };
    let last = if end.side_of(&start_point).negate() == Side::Right { end.opposite() } else { *end };
    TileShape::from_lines(&[first, *middle, middle.opposite(), last])
}
