//! From a found path to traces: walk the doors back from the destination
//! to the start, room by room, placing corners so every segment keeps to
//! 45 degrees and stays a trace's width inside its room; a via at each
//! drill. Ported from FreeRouting's `LocateFoundConnectionAlgo` and
//! `LocateFoundConnectionAlgo45Degree`.

use crate::door::RoomId;
use crate::geometry::{FloatPoint, IntBox, IntPoint, TileShape};

use super::control::Control;
use super::engine::{Engine, Expandable, TRACE_WIDTH_TOLERANCE};
use super::maze::Found;

/// One door of the found path, with the room behind it.
/// `LocateFoundConnectionAlgo.BacktrackElement`.
#[derive(Debug, Clone, Copy)]
pub struct BacktrackElement {
    pub door: Expandable,
    pub section: usize,
    pub next_room: Option<RoomId>,
}

/// A trace to insert: its corners and layer. `ResultItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedTrace {
    pub corners: Vec<IntPoint>,
    pub layer: i32,
}

/// The found connection as traces, from the destination back to the
/// start, with the items and layers at its ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Indices into the board's items.
    pub start_item: usize,
    pub start_layer: i32,
    pub target_item: usize,
    pub target_layer: i32,
    pub traces: Vec<LocatedTrace>,
    /// The items the path runs through, to be ripped up, by number
    /// descending.
    pub ripped: Vec<usize>,
}

/// The doors of a found path, from the destination back to the start,
/// and the items it runs through where they are to be ripped up, by
/// number descending. `LocateFoundConnectionAlgo.backtrack`.
pub fn backtrack(engine: &Engine, found: &Found) -> (Vec<BacktrackElement>, Vec<usize>) {
    let mut result = Vec::new();
    let mut ripped = Vec::new();
    let mut door = found.door;
    let mut element = engine.element(door, found.section).clone();
    let mut next_room = match door {
        Expandable::Target(t) => Some(engine.targets[t].room),
        Expandable::Drill(d) => {
            let drill = &engine.drills[d];
            if element.room_ripped {
                ripped.extend(drill.rooms.iter().filter_map(|&r| engine.obstacle_item(r)).map(|(i, _)| i));
            }
            // The Java indexes by first_layer plus the section.
            Some(drill.rooms[(drill.first_layer as usize) + found.section])
        }
        _ => None,
    };
    let mut current = BacktrackElement { door, section: found.section, next_room };
    loop {
        result.push(current);
        let Some(back) = element.backtrack_door else { break };
        door = back;
        let mut section = element.section_no_of_backtrack_door;
        let count = engine.element_count(door);
        if section >= count {
            section = count - 1;
        }
        next_room = match door {
            Expandable::Drill(d) => Some(engine.drills[d].rooms[section]),
            _ => next_room.and_then(|r| engine.other_room(door, r)),
        };
        element = engine.element(door, section).clone();
        current = BacktrackElement { door, section, next_room };
        if element.room_ripped {
            if let Some((item, _)) = next_room.and_then(|r| engine.obstacle_item(r)) {
                ripped.push(item);
            }
        }
    }
    engine.rb.sort_items(&mut ripped);
    (result, ripped)
}


/// The working state of `LocateFoundConnectionAlgo`.
struct Locate<'a, 'e, 'b> {
    engine: &'a mut Engine<'b>,
    ctrl: &'e Control,
    backtrack: Vec<BacktrackElement>,
    current_from_point: FloatPoint,
    current_trace_layer: i32,
    current_from_door_index: usize,
    current_to_door_index: usize,
    current_target_door_index: usize,
    current_target_shape: TileShape,
}

/// Turn a found path into traces. `None` where the path does not start
/// from a target door, as the Java warns. `LocateFoundConnectionAlgo45Degree`.
pub fn locate(engine: &mut Engine, ctrl: &Control, found: &Found) -> Option<Located> {
    let (backtrack, ripped) = backtrack(engine, found);
    let start_info = *backtrack.last()?;
    let Expandable::Target(start_door) = start_info.door else { return None };
    let (start_item, start_entry) = (engine.targets[start_door].item, engine.targets[start_door].tree_entry_no);
    let start_room = engine.targets[start_door].room;
    let start_layer = engine.graph.room(start_room).layer;
    let Expandable::Target(dest) = found.door else {
        unimplemented!("a fanout ending at a drill is not ported yet");
    };
    let target_item = engine.targets[dest].item;
    let target_layer = engine.graph.room(engine.targets[dest].room).layer;
    let start_point = starting_point(engine, dest);
    let mut l = Locate {
        engine,
        ctrl,
        backtrack,
        current_from_point: start_point,
        current_trace_layer: target_layer,
        current_from_door_index: 0,
        current_to_door_index: 0,
        current_target_door_index: 0,
        current_target_shape: TileShape::Box(IntBox::new(0, 0, 0, 0)),
    };
    let mut traces = Vec::new();
    let mut done = false;
    while !done {
        let mut layer_changed = false;
        l.current_target_door_index = l.current_from_door_index + 1;
        while l.current_target_door_index < l.backtrack.len() && !layer_changed {
            if let Expandable::Drill(_) = l.backtrack[l.current_target_door_index].door {
                layer_changed = true;
            } else {
                l.current_target_door_index += 1;
            }
        }
        if layer_changed {
            let Expandable::Drill(d) = l.backtrack[l.current_target_door_index].door else { unreachable!() };
            let p = l.engine.drills[d].location;
            l.current_target_shape = TileShape::Box(IntBox::new(p.x, p.y, p.x, p.y));
        } else {
            done = true;
            l.current_target_door_index = l.backtrack.len() - 1;
            let room_shape = TileShape::Octagon(l.engine.graph.room(start_room).shape.expect("a completed room"));
            let target = l.engine.connection_shape(start_item, start_entry).expect("a target connects").intersection(&room_shape);
            l.current_target_shape = target;
            if l.current_target_shape.dimension() >= 2 {
                // A pour: connect safely inside it.
                let half = l.ctrl.compensated_trace_half_width[start_layer as usize] as f64;
                let shrinked = l.current_target_shape.offset(-half);
                if !shrinked.is_empty() {
                    l.current_target_shape = shrinked;
                }
            }
        }
        l.current_to_door_index = l.current_from_door_index + 1;
        traces.push(l.calculate_next_trace(layer_changed));
    }
    Some(Located { start_item, start_layer, target_item, target_layer, traces, ripped })
}

/// Where the traces start: the centre of the destination's connection
/// shape in its room, rounded. `calculate_starting_point`.
fn starting_point(engine: &Engine, door: usize) -> FloatPoint {
    let t = &engine.targets[door];
    let room_shape = TileShape::Octagon(engine.graph.room(t.room).shape.expect("a completed room"));
    let shape = engine.connection_shape(t.item, t.tree_entry_no).expect("a target connects").intersection(&room_shape);
    FloatPoint::from_int(shape.centre_of_gravity().round())
}

impl Locate<'_, '_, '_> {
    /// The next trace: from the current point through the rooms to the
    /// next drill or to the start. `calculate_next_trace`.
    fn calculate_next_trace(&mut self, layer_changed: bool) -> LocatedTrace {
        let mut corners = vec![self.current_from_point];
        if let Some(adjusted) = self.adjust_start_corner() {
            corners.push(fortyfive_degree_corner(&self.current_from_point, &adjusted, true));
            corners.push(adjusted);
            self.current_from_point = adjusted;
        }
        loop {
            let next = self.calculate_next_trace_corners();
            if next.is_empty() {
                break;
            }
            // Every corner is a new point, never the one before.
            for c in next {
                corners.push(c);
                self.current_from_point = c;
            }
        }
        let mut next_layer = self.current_trace_layer;
        if layer_changed {
            self.current_from_door_index = self.current_target_door_index + 1;
            if let Some(room) = self.backtrack[self.current_from_door_index].next_room {
                next_layer = self.engine.graph.room(room).layer;
            }
        }
        let mut rounded: Vec<IntPoint> = Vec::new();
        for c in corners {
            let p = c.round();
            if rounded.last() != Some(&p) {
                rounded.push(p);
            }
        }
        let trace = LocatedTrace { corners: rounded, layer: self.current_trace_layer };
        self.current_trace_layer = next_layer;
        trace
    }

    /// Inside the room behind the current door, a trace's width from its
    /// border: the nearest such point, rounded, or `None` where the current
    /// point already is. `adjust_start_corner`.
    fn adjust_start_corner(&self) -> Option<FloatPoint> {
        let info = self.backtrack[self.current_from_door_index];
        let room = info.next_room?;
        let half = self.ctrl.compensated_trace_half_width[self.current_trace_layer as usize] as f64;
        let shape = self.engine.graph.room(room).tile_shape().expect("a room").offset(-half);
        if shape.is_empty() || shape.contains_float(&self.current_from_point) {
            return None;
        }
        Some(FloatPoint::from_int(shape.nearest_point_approx(&self.current_from_point).expect("not empty").round()))
    }

    /// The corners through the next room: into it at 45 degrees, then to
    /// the next door or the target. Empty once the target is reached.
    /// `LocateFoundConnectionAlgo45Degree.calculate_next_trace_corners`.
    fn calculate_next_trace_corners(&mut self) -> Vec<FloatPoint> {
        let mut result = Vec::new();
        if self.current_to_door_index > self.current_target_door_index {
            return result;
        }
        let from_info = self.backtrack[self.current_to_door_index - 1];
        let Some(from_room) = from_info.next_room else { return result };
        let room = self.engine.graph.room(from_room);
        let room_shape = room.tile_shape().expect("a room");
        let trace_halfwidth = self.ctrl.compensated_trace_half_width[self.current_trace_layer as usize];
        let trace_halfwidth_add = trace_halfwidth as f64 + TRACE_WIDTH_TOLERANCE;
        let shrink_offset = if room.is_obstacle_room() { trace_halfwidth as f64 } else { trace_halfwidth_add };
        let mut shrinked = room_shape.offset(-shrink_offset);
        if !shrinked.is_empty() {
            let nearest = shrinked.nearest_point_approx(&self.current_from_point).expect("not empty");
            let horizontal_first = self.horizontal_first(from_info.door, &self.current_from_point, &nearest, true);
            let nearest = round_to_integer(&nearest);
            result.push(fortyfive_degree_corner(&self.current_from_point, &nearest, horizontal_first));
            result.push(nearest);
            self.current_from_point = nearest;
        } else {
            shrinked = room_shape;
        }
        if self.current_to_door_index == self.current_target_door_index {
            let nearest = round_to_integer(&self.current_target_shape.nearest_point_approx(&self.current_from_point).expect("a target"));
            let mut add_corner = fortyfive_degree_corner(&self.current_from_point, &nearest, true);
            if !shrinked.contains_float(&add_corner) {
                add_corner = fortyfive_degree_corner(&self.current_from_point, &nearest, false);
            }
            result.push(add_corner);
            result.push(nearest);
            self.current_to_door_index += 1;
            return result;
        }
        let to_info = self.backtrack[self.current_to_door_index];
        let Expandable::Door(to_door) = to_info.door else { return result };
        let mut nearest = if self.engine.graph.door(to_door).dimension == 2 {
            let shape = self.engine.graph.door_tile_shape(to_door).shrink(shrink_offset);
            round_to_integer(&shape.nearest_point_approx(&self.current_from_point).expect("not empty"))
        } else {
            let sections = self.engine.section_segments(to_door, trace_halfwidth as f64);
            let Some(section) = sections.get(to_info.section).copied() else { return result };
            let mut p = section.nearest_segment_point(&self.current_from_point);
            let mut ok = true;
            if let Some(next_room) = to_info.next_room {
                let simplex = match self.engine.graph.room(next_room).tile_shape().expect("a room") {
                    TileShape::Octagon(o) => o.to_simplex(),
                    TileShape::Box(b) => b.to_simplex(),
                    TileShape::Simplex(s) => s,
                };
                let points = TileShape::Simplex(simplex).nearest_border_points_approx(&p, 2);
                if points.len() >= 2 {
                    ok = points[1].distance(&p) >= trace_halfwidth_add;
                }
            }
            if !ok {
                // The room may have a sharp corner at the door.
                p = section.middle();
            }
            p
        };
        nearest = round_to_integer(&nearest);
        let horizontal_first = self.horizontal_first(to_info.door, &self.current_from_point, &nearest, false);
        result.push(fortyfive_degree_corner(&self.current_from_point, &nearest, horizontal_first));
        result.push(nearest);
        self.current_to_door_index += 1;
        result
    }

    /// Whether the 45-degree bend from `from` to `to` should go horizontal
    /// first, judged by the door it leaves (`from_door`) or enters.
    /// `calc_horizontal_first_from_door` and `calc_horizontal_first_to_door`,
    /// which differ only in which answer each case gives.
    fn horizontal_first(&self, door: Expandable, from: &FloatPoint, to: &FloatPoint, from_door: bool) -> bool {
        let shape = self.engine.shape_of(door);
        let b = shape.bounding_box();
        let (width, height) = ((b.ur.x - b.ll.x) as f64, (b.ur.y - b.ll.y) as f64);
        if self.engine.dimension_of(door) != 1 {
            return if from_door { height >= width } else { height <= width };
        }
        let segment = shape.diagonal_corner_segment().expect("a door is not empty");
        let (left, right) = if segment.a.x < segment.b.x || segment.a.x == segment.b.x && segment.a.y <= segment.b.y {
            (segment.a, segment.b)
        } else {
            (segment.b, segment.a)
        };
        let door_dx = right.x - left.x;
        let door_dy = right.y - left.y;
        let door_half_max_width = 0.5 * door_dx.max(door_dy.abs());
        if width <= door_half_max_width {
            // About vertical.
            return from_door;
        }
        if height <= door_half_max_width {
            // About horizontal.
            return !from_door;
        }
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let same_sign = signum(dx) == signum(dy);
        let steeper = dx.abs() < dy.abs();
        let flatter = dx.abs() > dy.abs();
        // About the right diagonal, or the left.
        let result_from = if left.y < right.y { if same_sign { flatter } else { steeper } } else if same_sign { steeper } else { flatter };
        let result_to = if left.y < right.y { if same_sign { steeper } else { flatter } } else if same_sign { flatter } else { steeper };
        if from_door {
            result_from
        } else {
            result_to
        }
    }
}

/// `Signum.of`.
fn signum(v: f64) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

/// `round_to_integer`: the point rounded, as a float point.
fn round_to_integer(p: &FloatPoint) -> FloatPoint {
    FloatPoint::from_int(p.round())
}

/// A corner between `from` and `to` so both segments run at multiples of
/// 45 degrees: the straight part first if `horizontal_first`.
/// `LocateFoundConnectionAlgo.fortyfive_degree_corner`.
pub fn fortyfive_degree_corner(from: &FloatPoint, to: &FloatPoint, horizontal_first: bool) -> FloatPoint {
    let abs_dx = (to.x - from.x).abs();
    let abs_dy = (to.y - from.y).abs();
    let (x, y);
    if abs_dx <= abs_dy {
        if horizontal_first {
            x = to.x;
            y = if to.y >= from.y { from.y + abs_dx } else { from.y - abs_dx };
        } else {
            x = from.x;
            y = if to.y > from.y { to.y - abs_dx } else { to.y + abs_dx };
        }
    } else if horizontal_first {
        y = from.y;
        x = if to.x > from.x { to.x - abs_dy } else { to.x + abs_dy };
    } else {
        y = to.y;
        x = if to.x > from.x { from.x + abs_dy } else { from.x - abs_dy };
    }
    FloatPoint::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bend makes both legs straight or diagonal.
    #[test]
    fn a_fortyfive_degree_corner_splits_a_move_into_legal_legs() {
        let (from, to) = (FloatPoint::new(0.0, 0.0), FloatPoint::new(10.0, 4.0));
        let c = fortyfive_degree_corner(&from, &to, true);
        assert_eq!(c, FloatPoint::new(6.0, 0.0));
        let c = fortyfive_degree_corner(&from, &to, false);
        assert_eq!(c, FloatPoint::new(4.0, 4.0));
    }
}
