//! The maze search: from the start items' rooms, expand door by door --
//! through rooms, into drills to change layers -- cheapest estimate first,
//! until a destination item's door is reached. Ported from FreeRouting's
//! `MazeSearchAlgo` and `MazeListElement`.
//!
//! The queue orders by cost estimate, then cost so far, then the door's
//! number and section, exactly as the Java's `TreeSet` does, as ties decide
//! which way the search goes.
//!
//! Ripping up and pushing aside obstacles in the way are not ported yet:
//! where the search would enter an obstacle's room, it stops with a message
//! saying so.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::door::{RoomId, RoomState};
use crate::geometry::{FloatLine, FloatPoint, Polyline, TileShape};
use crate::model::ItemKind;

use super::control::Control;
use super::distance::DestinationDistance;
use super::engine::{Adjustment, Engine, Expandable, TRACE_WIDTH_TOLERANCE};
use crate::routing::shove::DrillCheck;

/// One queued expansion. `MazeListElement`.
#[derive(Debug, Clone)]
pub struct ListElement {
    pub door: Expandable,
    pub section: usize,
    pub backtrack_door: Option<Expandable>,
    pub backtrack_section: usize,
    pub expansion_value: f64,
    pub sorting_value: f64,
    pub next_room: Option<RoomId>,
    pub shape_entry: FloatLine,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    pub already_checked: bool,
    pub ripup_cost: i32,
    /// The door's number when queued, which orders ties.
    door_id_no: i32,
}

impl PartialEq for ListElement {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ListElement {}

impl PartialOrd for ListElement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ListElement {
    /// `MazeListElement.compareTo`: by estimate, then cost so far, then the
    /// door's number, then its section; equal otherwise, and a set keeps
    /// only the first of equals.
    fn cmp(&self, other: &Self) -> Ordering {
        let by = |a: f64, b: f64| {
            if a < b {
                Ordering::Less
            } else if a > b {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        };
        by(self.sorting_value, other.sorting_value)
            .then(by(self.expansion_value, other.expansion_value))
            .then(self.door_id_no.cmp(&other.door_id_no))
            .then(self.section.cmp(&other.section))
    }
}

/// What a finished search found: the destination door reached, and its
/// section. `MazeSearchAlgo.Result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub door: Expandable,
    pub section: usize,
}

/// The search for one connection. `MazeSearchAlgo`.
pub struct MazeSearch<'e, 'b> {
    pub engine: &'e mut Engine<'b>,
    pub ctrl: &'e Control,
    list: BTreeSet<ListElement>,
    destination_distance: DestinationDistance,
    destination: Option<Found>,
}

impl<'e, 'b> MazeSearch<'e, 'b> {
    /// A search from `start_items` to `dest_items` (indices into the
    /// board's items, in FreeRouting's set order: by item number
    /// descending); `None` if there is no start or destination.
    /// `MazeSearchAlgo.get_instance`.
    pub fn new(engine: &'e mut Engine<'b>, ctrl: &'e Control, start_items: &[usize], dest_items: &[usize]) -> Option<Self> {
        let destination_distance = DestinationDistance::new(&ctrl.trace_costs, &ctrl.layer_active, ctrl.min_normal_via_cost, ctrl.min_cheap_via_cost);
        let mut s = MazeSearch { engine, ctrl, list: BTreeSet::new(), destination_distance, destination: None };
        s.init(start_items, dest_items).then_some(s)
    }

    /// `MazeSearchAlgo.init`.
    fn init(&mut self, start_items: &[usize], dest_items: &[usize]) -> bool {
        let board = self.engine.board;
        // reduce_trace_shapes_at_tie_pins: trims the traces of other nets on
        // pins of several nets, so do nothing without traces.
        let has_traces = board.items.iter().any(|i| matches!(i.kind, ItemKind::Trace { .. }));
        for &i in start_items.iter().chain(dest_items) {
            let item = &board.items[i];
            if has_traces && matches!(item.kind, ItemKind::Pin { .. }) && item.nets.len() > 1 {
                unimplemented!("reduce_trace_shapes_at_tie_pins: pins of several nets are not ported yet");
            }
        }
        let mut destination_ok = false;
        for &i in dest_items {
            self.engine.start_info.insert(i, false);
            let item = &board.items[i];
            let count = self.engine.tree_shape_count(i);
            for k in 0..count {
                if let Some(shape) = self.engine.tree_shape(i, k as u32) {
                    let layer = item.shape_layer(k, board.layer_count(), count);
                    self.destination_distance.join(&shape.bounding_box(), layer);
                }
            }
            destination_ok = true;
        }
        if !destination_ok {
            // Fanout, which needs no destination, is not ported.
            return false;
        }
        let mut start_rooms = Vec::new();
        for &i in start_items {
            self.engine.start_info.insert(i, true);
            let item = &board.items[i];
            if !item.is_connectable_kind() {
                continue;
            }
            let count = self.engine.tree_shape_count(i);
            for k in 0..count {
                let layer = item.shape_layer(k, board.layer_count(), count);
                let contained = self.engine.connection_shape(i, k as u32);
                start_rooms.push((layer, contained));
            }
        }
        let mut completed = Vec::new();
        for (layer, contained) in start_rooms {
            // ShapeSearchTree45Degree.complete_shape takes only shapes an
            // octagon can hold, by their bounding octagon; others give no
            // room.
            let contained = contained.filter(is_int_octagon).and_then(|c| c.bounding_octagon());
            let Some(contained) = contained else { continue };
            let room = self.engine.graph.add_incomplete_room(None, layer, contained);
            completed.extend(self.engine.complete_expansion_room(room));
        }
        let mut start_ok = false;
        for room in completed {
            for t in self.engine.target_doors(room) {
                if self.is_destination(t) {
                    continue;
                }
                let target = &self.engine.targets[t];
                let (item, entry) = (target.item, target.tree_entry_no);
                let room_shape = TileShape::Octagon(self.engine.graph.room(room).shape.expect("a completed room"));
                let connection = self.engine.connection_shape(item, entry).expect("a target connects").intersection(&room_shape);
                let center = connection.centre_of_gravity();
                let layer = self.engine.graph.room(room).layer;
                let sorting_value = self.destination_distance.calculate_point(&center, layer);
                self.add(Expandable::Target(t), 0, None, 0, 0.0, sorting_value, Some(room), FloatLine::point(center), false, Adjustment::None, false, 0);
                start_ok = true;
            }
        }
        start_ok
    }

    /// Queue an expansion; ignored if an equal one is queued.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        door: Expandable,
        section: usize,
        backtrack_door: Option<Expandable>,
        backtrack_section: usize,
        expansion_value: f64,
        sorting_value: f64,
        next_room: Option<RoomId>,
        shape_entry: FloatLine,
        room_ripped: bool,
        adjustment: Adjustment,
        already_checked: bool,
        ripup_cost: i32,
    ) {
        let door_id_no = self.engine.id_no(door);
        self.list.insert(ListElement {
            door,
            section,
            backtrack_door,
            backtrack_section,
            expansion_value,
            sorting_value,
            next_room,
            shape_entry,
            room_ripped,
            adjustment,
            already_checked,
            ripup_cost,
            door_id_no,
        });
    }

    /// Whether target door `t` is onto a destination item.
    /// `TargetItemExpansionDoor.is_destination_door`.
    fn is_destination(&self, t: usize) -> bool {
        !self.engine.start_info.get(&self.engine.targets[t].item).copied().unwrap_or(false)
    }

    /// The expansion `occupy_next_element` will take next: the first queued
    /// one whose section is not yet occupied.
    pub fn peek(&self) -> Option<&ListElement> {
        self.list.iter().find(|e| !self.engine.element(e.door, e.section).is_occupied)
    }

    /// Search to the end. `MazeSearchAlgo.find_connection`.
    pub fn find_connection(&mut self) -> Option<Found> {
        while self.occupy_next_element() {}
        self.destination
    }

    /// Expand the next queued element; false once a destination is reached
    /// or the queue is empty. `MazeSearchAlgo.occupy_next_element`.
    pub fn occupy_next_element(&mut self) -> bool {
        if self.destination.is_some() {
            return false;
        }
        let element = loop {
            let Some(e) = self.list.pop_first() else { return false };
            if !self.engine.element(e.door, e.section).is_occupied {
                break e;
            }
        };
        {
            let s = self.engine.element_mut(element.door, element.section);
            s.backtrack_door = element.backtrack_door;
            s.section_no_of_backtrack_door = element.backtrack_section;
            s.room_ripped = element.room_ripped;
            s.ripup_cost = element.ripup_cost;
            s.adjustment = element.adjustment;
        }
        if let Expandable::Page(_) = element.door {
            self.expand_to_drills_of_page(&element);
            return true;
        }
        if let Expandable::Target(t) = element.door {
            if self.is_destination(t) {
                self.destination = Some(Found { door: element.door, section: element.section });
                return false;
            }
        }
        if self.ctrl.is_fanout && matches!(element.door, Expandable::Drill(_)) && matches!(element.backtrack_door, Some(Expandable::Drill(_))) {
            self.destination = Some(Found { door: element.door, section: element.section });
            return false;
        }
        if self.ctrl.vias_allowed && matches!(element.door, Expandable::Drill(_)) && !matches!(element.backtrack_door, Some(Expandable::Drill(_))) {
            self.expand_to_other_layers(&element);
        }
        if element.next_room.is_some() && !self.expand_to_room_doors(&element) {
            // Occupying is delayed or nothing was expanded: another section
            // may still take this one, if the next room is thin.
            return true;
        }
        self.engine.element_mut(element.door, element.section).is_occupied = true;
        true
    }

    /// Expand the doors, targets and drill pages of the room behind the
    /// element's door. True if the element's section is to be occupied.
    /// `MazeSearchAlgo.expand_to_room_doors`.
    fn expand_to_room_doors(&mut self, element: &ListElement) -> bool {
        let board = self.engine.board;
        let next_room = element.next_room.expect("checked");
        let layer = self.engine.graph.room(next_room).layer;
        let layer_active = self.ctrl.layer_active[layer as usize];
        if !layer_active && board.layers[layer as usize].is_signal {
            return true;
        }
        let mut half_width = self.ctrl.compensated_trace_half_width[layer as usize] as f64;
        let mut curr_door_is_small = false;
        if let Expandable::Door(door) = element.door {
            let mut half_width_add = half_width + TRACE_WIDTH_TOLERANCE;
            if self.ctrl.with_neckdown {
                let neck_down = self.check_neck_down_at_dest_pin(next_room);
                if neck_down > 0.0 {
                    half_width_add = half_width_add.min(neck_down);
                    half_width = half_width_add;
                }
            }
            curr_door_is_small = self.door_is_small(door, 2.0 * half_width_add);
        }
        self.engine.complete_neighbour_rooms(next_room);
        let shape_entry_middle = element.shape_entry.middle();
        if self.ctrl.with_neckdown {
            if let Expandable::Target(t) = element.door {
                let item = &board.items[self.engine.targets[t].item];
                if let ItemKind::Pin { neckdown, .. } = &item.kind {
                    let n = neckdown[(layer - item.first_layer) as usize] as f64;
                    if n > 0.0 {
                        half_width = half_width.min(n);
                    }
                }
            }
        }
        let room = self.engine.graph.room(next_room);
        let mut next_room_is_thick = true;
        if room.is_obstacle_room() {
            unimplemented!("entering an obstacle's room (ripup, push and shove) is not ported yet");
        } else {
            let shape = TileShape::Octagon(room.shape.expect("a completed room"));
            if shape.min_width() < 2.0 * half_width {
                next_room_is_thick = false;
            } else if !element.already_checked && self.engine.dimension_of(element.door) == 1 && !curr_door_is_small {
                let nearest = shape.nearest_border_points_approx(&shape_entry_middle, 2);
                if nearest.len() < 2 {
                    next_room_is_thick = false;
                } else {
                    next_room_is_thick = nearest[1].distance(&shape_entry_middle) > half_width + 1.0;
                }
            }
        }
        if !layer_active && matches!(element.door, Expandable::Drill(_)) {
            unimplemented!("drills onto inactive layers (split planes) are not ported yet");
        }
        let mut something_expanded = self.expand_to_target_doors(element, next_room_is_thick, curr_door_is_small, &shape_entry_middle);
        if !layer_active {
            return true;
        }
        let ripup_costs = 0;
        if !element.already_checked && curr_door_is_small {
            // Entering a thick room through a small door is only allowed
            // coming from a ripped item, which needs an obstacle room.
            let from_obstacle = match element.door {
                Expandable::Door(d) => self.engine.graph.door(d).other(next_room).is_some_and(|r| self.engine.graph.room(r).is_obstacle_room()),
                _ => false,
            };
            if next_room_is_thick && from_obstacle {
                unimplemented!("leaving a ripped item through a small door is not ported yet");
            }
            return something_expanded;
        }
        let doors: Vec<_> = self.engine.graph.doors_of(next_room).to_vec();
        for d in doors {
            if Expandable::Door(d) == element.door {
                continue;
            }
            if self.expand_to_door(d, element, ripup_costs, next_room_is_thick, Adjustment::None) {
                something_expanded = true;
            }
        }
        if self.ctrl.vias_allowed && !matches!(element.door, Expandable::Drill(_)) {
            let is_free = matches!(self.engine.graph.room(next_room).state, RoomState::Complete { .. });
            if (something_expanded || next_room_is_thick) && is_free {
                let shape = TileShape::Octagon(self.engine.graph.room(next_room).shape.expect("a completed room"));
                for page in self.engine.pages.overlapping_pages(&shape) {
                    self.expand_to_drill_page(page, element);
                    something_expanded = true;
                }
            }
        }
        something_expanded
    }

    /// Queue the room's target doors. True if one was queued.
    /// `MazeSearchAlgo.expand_to_target_doors`.
    fn expand_to_target_doors(&mut self, element: &ListElement, next_room_is_thick: bool, curr_door_is_small: bool, shape_entry_middle: &FloatPoint) -> bool {
        let next_room = element.next_room.expect("checked");
        if curr_door_is_small {
            let from_obstacle = match element.door {
                Expandable::Door(d) => self.engine.graph.door(d).other(next_room).is_some_and(|r| self.engine.graph.room(r).is_obstacle_room()),
                _ => false,
            };
            if !from_obstacle {
                return false;
            }
        }
        let mut result = false;
        for t in self.engine.target_doors(next_room) {
            if Expandable::Target(t) == element.door {
                continue;
            }
            let (item, entry) = (self.engine.targets[t].item, self.engine.targets[t].tree_entry_no);
            let target_shape = self.engine.connection_shape(item, entry).expect("a target connects");
            let connection_point = target_shape.nearest_point_approx(shape_entry_middle).expect("a nearest point");
            if !next_room_is_thick {
                let layer = self.engine.graph.room(next_room).layer;
                let from = shape_entry_middle.round();
                let to = connection_point.round();
                if from != to {
                    let polyline = Polyline::from_points(&[from, to]);
                    let c = self.ctrl;
                    let ok = self.engine.rb.check_forced_trace_polyline(
                        &polyline,
                        c.trace_half_width[layer as usize],
                        layer,
                        &[c.net_no],
                        c.trace_clearance_class,
                        c.max_shove_trace_recursion_depth,
                        c.max_shove_via_recursion_depth,
                        c.max_spring_over_recursion_depth,
                    );
                    if !ok {
                        continue;
                    }
                }
            }
            if self.expand_to_door_section(Expandable::Target(t), 0, Some(FloatLine::point(connection_point)), element, 0, Adjustment::None) {
                result = true;
            }
        }
        result
    }

    /// Queue the unoccupied sections of door `door` out of the element's
    /// room. True if one was queued. `MazeSearchAlgo.expand_to_door`.
    fn expand_to_door(&mut self, door: crate::door::DoorId, element: &ListElement, add_costs: i32, next_room_is_thick: bool, adjustment: Adjustment) -> bool {
        let next_room = element.next_room.expect("checked");
        let layer = self.engine.graph.room(next_room).layer;
        let half_width = self.ctrl.compensated_trace_half_width[layer as usize] as f64;
        let sections = self.engine.section_segments(door, half_width);
        let mut something_expanded = false;
        for (i, section) in sections.iter().enumerate() {
            if self.engine.door_sections(door)[i].is_occupied {
                continue;
            }
            let shape_entry = if next_room_is_thick {
                let d = *self.engine.graph.door(door);
                let free = |r: RoomId| matches!(self.engine.graph.room(r).state, RoomState::Complete { .. });
                if d.dimension == 1 && sections.len() == 1 && free(d.first) && free(d.second) {
                    // Entering the door at an acute corner of the room?
                    let middle = section.middle();
                    let room_shape = TileShape::Octagon(self.engine.graph.room(next_room).shape.expect("a completed room"));
                    if room_shape.min_width() < 2.0 * half_width {
                        return false;
                    }
                    let nearest = room_shape.nearest_border_points_approx(&middle, 2);
                    if nearest.len() < 2 || nearest[1].distance(&middle) <= half_width + 1.0 {
                        return false;
                    }
                }
                Some(*section)
            } else {
                // Only doors across the room from where it was entered.
                if self.engine.graph.door(door).dimension == 1 && i == 0 && section.b.distance_square(&section.a) < 1.0 {
                    // A door of a via or of a thin room.
                    continue;
                }
                match segment_projection(&element.shape_entry, section) {
                    Some(p) => Some(p),
                    None => continue,
                }
            };
            if self.expand_to_door_section(Expandable::Door(door), i, shape_entry, element, add_costs, adjustment) {
                something_expanded = true;
            }
        }
        something_expanded
    }

    /// Whether a door is too narrow for a trace `trace_width` wide: by its
    /// bounding octagon's longest extent. `MazeSearchAlgo.door_is_small`.
    fn door_is_small(&self, door: crate::door::DoorId, trace_width: f64) -> bool {
        let d = *self.engine.graph.door(door);
        let free = |r: RoomId| matches!(self.engine.graph.room(r).state, RoomState::Complete { .. });
        if d.dimension == 1 || free(d.first) && free(d.second) {
            let shape = self.engine.graph.door_tile_shape(door);
            if shape.is_empty() {
                return true;
            }
            let oct = shape.bounding_octagon().expect("a door within bounds");
            return octagon_max_width(&oct) < trace_width;
        }
        false
    }

    /// Queue section `section` of `door`, entered at `shape_entry`. True if
    /// queued. `MazeSearchAlgo.expand_to_door_section`.
    fn expand_to_door_section(&mut self, door: Expandable, section: usize, shape_entry: Option<FloatLine>, from: &ListElement, add_costs: i32, adjustment: Adjustment) -> bool {
        let Some(shape_entry) = shape_entry else { return false };
        if self.engine.element(door, section).is_occupied {
            return false;
        }
        let from_room = from.next_room.expect("checked");
        let next_room = self.engine.other_room(door, from_room);
        let layer = self.engine.graph.room(from_room).layer;
        let middle = shape_entry.middle();
        let (h, v) = self.ctrl.trace_costs[layer as usize];
        let expansion_value = from.expansion_value + add_costs as f64 + middle.weighted_distance(&from.shape_entry.middle(), h, v);
        let sorting_value = expansion_value + self.destination_distance.calculate_point(&middle, layer);
        let room_ripped = add_costs > 0 && adjustment == Adjustment::None || from.already_checked && from.room_ripped;
        let ripup_cost = if add_costs > 0 && adjustment == Adjustment::None { add_costs } else { 0 };
        self.add(door, section, Some(from.door), from.section, expansion_value, sorting_value, next_room, shape_entry, room_ripped, adjustment, false, ripup_cost);
        true
    }

    /// Queue a drill of the element's room. `MazeSearchAlgo.expand_to_drill`.
    fn expand_to_drill(&mut self, drill: usize, from: &ListElement, add_costs: i32) {
        let from_room = from.next_room.expect("checked");
        let layer = self.engine.graph.room(from_room).layer;
        let trace_half_width = self.ctrl.compensated_trace_half_width[layer as usize];
        let room_shape = TileShape::Octagon(self.engine.graph.room(from_room).shape.expect("a completed room"));
        if room_shape.min_width() < 2.0 * trace_half_width as f64 {
            // A thin room: only drills meeting the door it was entered by.
            let Some(back) = from.backtrack_door else { return };
            if !self.engine.drills[drill].shape.intersects(&self.engine.shape_of(back)) {
                return;
            }
        }
        let via_radius = self.ctrl.via_radius[layer as usize];
        let shrinked = self.engine.drills[drill].shape.shrink(via_radius);
        let mut compare_corner = from.shape_entry.middle();
        if let (Expandable::Page(_), Some(Expandable::Target(t))) = (from.door, from.backtrack_door) {
            let item = self.engine.targets[t].item;
            if let Some(corner) = self.nearest_trace_exit_corner(item, &FloatPoint::from_int(self.engine.drills[drill].location), trace_half_width, layer) {
                compare_corner = corner;
            }
        }
        let nearest = shrinked.nearest_point_approx(&compare_corner).expect("a nearest point");
        let section = (layer - self.engine.drills[drill].first_layer) as usize;
        let (h, v) = self.ctrl.trace_costs[layer as usize];
        let mut expansion_value = from.expansion_value + add_costs as f64 + nearest.weighted_distance(&compare_corner, h, v);
        let (backtrack_door, backtrack_section) = if let Expandable::Page(_) = from.door {
            (from.backtrack_door, from.backtrack_section)
        } else {
            // Straight through an existing via, without its drill page.
            expansion_value += self.ctrl.min_normal_via_cost;
            (Some(from.door), from.section)
        };
        let sorting_value = expansion_value + self.destination_distance.calculate_point(&nearest, layer);
        self.add(Expandable::Drill(drill), section, backtrack_door, backtrack_section, expansion_value, sorting_value, None, FloatLine::point(nearest), from.room_ripped, Adjustment::None, false, 0);
    }

    /// Queue a drill page of the element's room, in front of its drills.
    /// `MazeSearchAlgo.expand_to_drill_page`.
    fn expand_to_drill_page(&mut self, page: usize, from: &ListElement) {
        let from_room = from.next_room.expect("checked");
        let layer = self.engine.graph.room(from_room).layer;
        let middle = from.shape_entry.middle();
        let nearest = self.engine.pages.pages[page].shape.nearest_point(&middle);
        let expansion_value = from.expansion_value + self.ctrl.min_normal_via_cost;
        let (h, v) = self.ctrl.trace_costs[layer as usize];
        let sorting_value = expansion_value + nearest.weighted_distance(&middle, h, v) + self.destination_distance.calculate_point(&nearest, layer);
        self.add(Expandable::Page(page), layer as usize, Some(from.door), from.section, expansion_value, sorting_value, from.next_room, from.shape_entry, from.room_ripped, Adjustment::None, false, 0);
    }

    /// Queue the drills of the element's page that open into its room.
    /// `MazeSearchAlgo.expand_to_drills_of_page`.
    fn expand_to_drills_of_page(&mut self, from: &ListElement) {
        let Expandable::Page(page) = from.door else { unreachable!() };
        let from_room_layer = from.section as i32;
        let drills = self.engine.page_drills(page, self.ctrl.attach_smd_allowed);
        for d in drills {
            let section = from_room_layer - self.engine.drills[d].first_layer;
            if section < 0 || section as usize >= self.engine.drills[d].rooms.len() {
                continue;
            }
            let section = section as usize;
            if Some(self.engine.drills[d].rooms[section]) == from.next_room && !self.engine.drills[d].elements[section].is_occupied {
                self.expand_to_drill(d, from, 0);
            }
        }
    }

    /// Queue the drill's other layers a via could reach from the element's
    /// layer. `MazeSearchAlgo.expand_to_other_layers`.
    fn expand_to_other_layers(&mut self, element: &ListElement) {
        let Expandable::Drill(d) = element.door else { unreachable!() };
        let c = self.ctrl;
        let (first, last) = (self.engine.drills[d].first_layer, self.engine.drills[d].last_layer);
        let from_layer = first + element.section as i32;
        let mut smd_attached_on_component_side = false;
        let mut smd_attached_on_solder_side = false;
        if self.engine.graph.room(self.engine.drills[d].rooms[element.section]).is_obstacle_room() {
            unimplemented!("ripping up an existing via is not ported yet");
        }
        let room_ripped = false;
        let via_lower_limit = first.max(c.via_lower_bound);
        let via_upper_limit = last.min(c.via_upper_bound);
        let location = self.engine.drills[d].location;
        let check = |engine: &Engine, layer: i32| {
            let room = engine.drills[d].rooms[(layer - first) as usize];
            let shape = engine.graph.room(room).tile_shape().expect("a room");
            engine.rb.check_via_layer(c.via_radius[layer as usize], c.via_clearance_class, c.attach_smd_allowed, &shape, location, layer, &[c.net_no], c.max_shove_trace_recursion_depth, 0)
        };
        let via_lower_bound;
        let mut layer = from_layer;
        loop {
            let result = check(self.engine, layer);
            if result == DrillCheck::NotDrillable {
                via_lower_bound = layer + 1;
                break;
            } else if result == DrillCheck::DrillableWithAttachSmd {
                if layer == 0 {
                    smd_attached_on_component_side = true;
                } else if layer == c.layer_count as i32 - 1 {
                    smd_attached_on_solder_side = true;
                }
            }
            if layer <= via_lower_limit {
                via_lower_bound = via_lower_limit;
                break;
            }
            layer -= 1;
        }
        if via_lower_bound > first {
            return;
        }
        let via_upper_bound;
        layer = from_layer + 1;
        loop {
            if layer > via_upper_limit {
                via_upper_bound = via_upper_limit;
                break;
            }
            let result = check(self.engine, layer);
            if result == DrillCheck::NotDrillable {
                via_upper_bound = layer - 1;
                break;
            } else if result == DrillCheck::DrillableWithAttachSmd && layer == c.layer_count as i32 - 1 {
                smd_attached_on_solder_side = true;
            }
            layer += 1;
        }
        if via_upper_bound < last {
            return;
        }
        for to_layer in via_lower_bound..=via_upper_bound {
            if to_layer == from_layer {
                continue;
            }
            let (curr_first, curr_last) = if to_layer < from_layer { (to_layer, from_layer) } else { (from_layer, to_layer) };
            let mask_found = c.via_masks.iter().any(|m| {
                curr_first >= m.from_layer
                    && curr_last <= m.to_layer
                    && m.from_layer >= via_lower_bound
                    && m.to_layer <= via_upper_bound
                    && (!(m.from_layer == 0 && smd_attached_on_component_side || m.to_layer == c.layer_count as i32 - 1 && smd_attached_on_solder_side)
                        || m.attach_smd_allowed)
            });
            if !mask_found {
                continue;
            }
            let index = (to_layer - first) as usize;
            if self.engine.drills[d].elements[index].is_occupied {
                continue;
            }
            let expansion_value = element.expansion_value + c.add_via_costs[from_layer as usize][to_layer as usize] as f64;
            let middle = element.shape_entry.middle();
            let sorting_value = expansion_value + self.destination_distance.calculate_point(&middle, to_layer);
            let room = self.engine.drills[d].rooms[index];
            self.add(Expandable::Drill(d), index, Some(Expandable::Drill(d)), element.section, expansion_value, sorting_value, Some(room), element.shape_entry, room_ripped, Adjustment::None, false, 0);
        }
    }

    /// The neck-down half width of the first pin the room has a target door
    /// onto, 0 for none. `MazeSearchAlgo.check_neck_down_at_dest_pin`.
    fn check_neck_down_at_dest_pin(&mut self, room: RoomId) -> f64 {
        if self.engine.graph.room(room).is_obstacle_room() {
            return 0.0;
        }
        let layer = self.engine.graph.room(room).layer;
        for t in self.engine.target_doors(room) {
            let item = &self.engine.board.items[self.engine.targets[t].item];
            if let ItemKind::Pin { neckdown, .. } = &item.kind {
                return neckdown[(layer - item.first_layer) as usize] as f64;
            }
        }
        0.0
    }

    /// The nearest point to `from` where a trace may leave pin `item`
    /// under its exit restrictions; `None` without restrictions.
    /// `Pin.nearest_trace_exit_corner`.
    fn nearest_trace_exit_corner(&self, item: usize, from: &FloatPoint, trace_half_width: i64, layer: i32) -> Option<FloatPoint> {
        let board = self.engine.board;
        let it = &board.items[item];
        let ItemKind::Pin { center, pads, exits, .. } = &it.kind else { return None };
        let index = (layer - it.first_layer) as usize;
        let exits = &exits[index];
        if exits.is_empty() {
            return None;
        }
        let pin_shape = match pads[index].as_ref()? {
            crate::board::PadShape::Box(b) => TileShape::Box(*b),
            crate::board::PadShape::Octagon(o) => TileShape::Octagon(*o),
            crate::board::PadShape::Polygon(s) => TileShape::Simplex(s.clone()),
            crate::board::PadShape::Circle(_) => return None,
        };
        let edge_to_turn_dist = board.rules.pin_edge_to_turn_dist;
        if edge_to_turn_dist < 0.0 {
            return None;
        }
        let offset_pin_shape = pin_shape.offset(edge_to_turn_dist + trace_half_width as f64);
        let mut min_distance = f64::MAX;
        let mut nearest = None;
        for exit in exits {
            let dir = &exit.direction;
            let no = offset_pin_shape
                .intersecting_border_line_no(center, dir)
                .unwrap_or_else(|| panic!("Pin.nearest_trace_exit_corner: no border line for pin {} (the Java fails here)", it.id));
            let ray = crate::geometry::Line::through(*center, *dir);
            let (x, y) = ray.intersection_approx(&offset_pin_shape.border_line(no));
            let corner = FloatPoint::new(x, y);
            let d = corner.distance_square(from);
            if d < min_distance {
                min_distance = d;
                nearest = Some(corner);
            }
        }
        nearest
    }
}

/// Whether an octagon can hold the shape exactly. `is_IntOctagon`.
fn is_int_octagon(s: &TileShape) -> bool {
    match s {
        TileShape::Box(_) | TileShape::Octagon(_) => true,
        TileShape::Simplex(x) => x.is_int_octagon(),
    }
}

/// `IntOctagon.max_width`.
fn octagon_max_width(o: &crate::geometry::IntOctagon) -> f64 {
    let w1 = ((o.right_x - o.left_x).max(o.top_y - o.bottom_y)) as f64;
    let w2 = ((o.upper_right_diag_x - o.lower_left_diag_x).max(o.lower_right_diag_x - o.upper_left_diag_x)) as f64;
    w1.max(w2 / std::f64::consts::SQRT_2)
}

/// `from` projected onto `to`, both ways: square onto it, and moved square
/// to itself; the ends of the two nearest `to`'s ends where both exist.
/// `MazeSearchAlgo.segment_projection`.
fn segment_projection(from: &FloatLine, to: &FloatLine) -> Option<FloatLine> {
    let check = from.adjust_direction(to);
    let first = to.segment_projection(&check);
    let second = to.segment_projection_2(&check);
    match (first, second) {
        (Some(f), Some(s)) => {
            let a = if f.a == to.a || s.a == to.a {
                to.a
            } else if f.a.distance_square(&to.a) <= s.a.distance_square(&to.a) {
                f.a
            } else {
                s.a
            };
            let b = if f.b == to.b || s.b == to.b {
                to.b
            } else if f.b.distance_square(&to.b) <= s.b.distance_square(&to.b) {
                f.b
            } else {
                s.b
            };
            Some(FloatLine::new(a, b))
        }
        (Some(f), None) => Some(f),
        (None, s) => s,
    }
}
