//! Pushing aside a trace of another net the search runs into, rather than
//! ripping it up: how far along it a push could go, and which doors of its
//! room the search may go on through. Ported from FreeRouting's
//! `MazeShoveTraceAlgo`.

use crate::door::{DoorId, RoomId};
use crate::geometry::{FloatLine, FloatPoint, Line, LineSegment, Point, Side};
use crate::model::ItemKind;
use crate::routing::RoutingBoard;

use super::control::Control;
use super::engine::{Engine, Expandable};
use super::maze::ListElement;

/// A section of a door of the pushed trace's room, to expand.
/// `MazeShoveTraceAlgo.DoorSection`.
#[derive(Debug, Clone, Copy)]
pub struct DoorSection {
    pub door: DoorId,
    pub section: usize,
    pub line: FloatLine,
}

/// Whether pushing the trace of obstacle room `room`, entered by the
/// element's door, to the left or right could make way, and the sections
/// of the room's doors a push that far reaches, in `to_doors`. False if it
/// failed where pushing from another section of the door might not.
/// `MazeShoveTraceAlgo.check_shove_trace_line`.
pub fn check_shove_trace_line(engine: &mut Engine, ctrl: &Control, element: &ListElement, room: RoomId, shove_to_the_left: bool, to_doors: &mut Vec<DoorSection>) -> bool {
    let Expandable::Door(from_door) = element.door else { return true };
    let Some((trace, trace_corner_no)) = engine.obstacle_item(room) else { return true };
    let rb = engine.rb;
    let ItemKind::Trace { half_width, polyline: trace_polyline, .. } = &rb.board.items[trace].kind else { return true };
    let trace_layer = engine.graph.room(room).layer;
    // Only traces as wide as ours and of our clearance class are pushed.
    if *half_width != ctrl.trace_half_width[trace_layer as usize] || rb.board.items[trace].clearance_class != ctrl.trace_clearance_class {
        return true;
    }
    let compensated_trace_half_width = ctrl.compensated_trace_half_width[trace_layer as usize] as f64;
    let from_door_shape = engine.graph.door_tile_shape(from_door);
    if from_door_shape.max_width() < 2.0 * compensated_trace_half_width {
        return true;
    }
    let trace_corner_no = trace_corner_no as usize;
    if trace_corner_no >= trace_polyline.lines.len() - 1 {
        return false;
    }
    let dimension = engine.graph.door(from_door).dimension;
    let shove_line_segment = if dimension == 2 {
        // From a link door on, towards the other link door.
        let Some(other_room) = engine.other_room(element.door, room) else { return false };
        let Some((other_item, _)) = engine.obstacle_item(other_room) else { return false };
        if !end_points_matching(rb, trace, other_item) {
            return false;
        }
        let door_center = from_door_shape.centre_of_gravity();
        let corner_1 = trace_polyline.corner_float(trace_corner_no);
        let corner_2 = trace_polyline.corner_float(trace_corner_no + 1);
        if corner_1.distance_square(&corner_2) < 1.0 {
            return false;
        }
        let towards_trace_start = door_center.distance_square(&corner_2) < door_center.distance_square(&corner_1);
        let segment = LineSegment::of(trace_polyline, trace_corner_no + 1);
        if towards_trace_start {
            segment.opposite()
        } else {
            segment
        }
    } else {
        let Some(from_room) = engine.other_room(element.door, room) else { return false };
        let from_point = engine.graph.room(from_room).tile_shape().expect("a complete room has a shape").centre_of_gravity();
        let shove_trace_line = trace_polyline.lines[trace_corner_no + 1];
        let door_line_segment = from_door_shape.diagonal_corner_segment().expect("a door is not empty");
        let side_of_trace_line = shove_trace_line.side_of_float((door_line_segment.a.x, door_line_segment.a.y), 0.0);
        let polar_line_segment = from_door_shape.polar_line_segment(&from_point).expect("a door is not empty");
        let door_line_swapped = polar_line_segment.b.distance_square(&door_line_segment.a) < polar_line_segment.a.distance_square(&door_line_segment.a);
        // Push only from the rightmost section to the right, or from the
        // leftmost to the left.
        let check_distance = compensated_trace_half_width + 5.0;
        let check_dist_square = check_distance * check_distance;
        let entry = &element.shape_entry;
        let section_ok = if shove_to_the_left != door_line_swapped {
            element.section == engine.element_count(element.door) - 1
                && (entry.a.distance_square(&door_line_segment.b) <= check_dist_square || entry.b.distance_square(&door_line_segment.b) <= check_dist_square)
        } else {
            element.section == 0 && (entry.a.distance_square(&door_line_segment.a) <= check_dist_square || entry.b.distance_square(&door_line_segment.a) <= check_dist_square)
        };
        if !section_ok {
            return false;
        }
        let shrinked = polar_line_segment.shrink_segment(compensated_trace_half_width);
        let perpendicular = shove_trace_line.direction().turn_45_degree(2);
        let lines = &trace_polyline.lines;
        let forwards = |start: FloatPoint| LineSegment::new(Line::through(start.round(), perpendicular), lines[trace_corner_no + 1], lines[trace_corner_no + 2]);
        let backwards = |start: FloatPoint| LineSegment::new(Line::through(start.round(), perpendicular), lines[trace_corner_no + 1].opposite(), lines[trace_corner_no].opposite());
        match (side_of_trace_line == Side::Left, shove_to_the_left) {
            (true, true) => forwards(shrinked.b),
            (true, false) => backwards(shrinked.a),
            (false, true) => backwards(shrinked.b),
            (false, false) => forwards(shrinked.a),
        }
    };
    let trace_half_width = ctrl.trace_half_width[trace_layer as usize];
    let nets = [ctrl.net_no];
    let mut shove_width = rb.check_trace_line_segment(&shove_line_segment, trace_layer, &nets, trace_half_width, ctrl.trace_clearance_class, true);
    let mut shove_line_segment = shove_line_segment;
    let mut segment_shortened = false;
    if shove_width < i32::MAX as f64 {
        shove_width -= 1.0;
        if shove_width <= 0.0 {
            return true;
        }
        shove_line_segment = shove_line_segment.change_length_approx(shove_width);
        segment_shortened = true;
    }
    let from_corner = shove_line_segment.start_point_approx();
    let to_corner = shove_line_segment.end_point_approx();
    let segment_is_point = from_corner.distance_square(&to_corner) < 0.1;
    if !segment_is_point {
        shove_width = rb.shove_check_segment(&shove_line_segment, shove_to_the_left, trace_layer, &nets, trace_half_width, ctrl.trace_clearance_class, ctrl.max_shove_trace_recursion_depth, ctrl.max_shove_via_recursion_depth);
        if shove_width <= 0.0 {
            return true;
        }
    }
    if segment_shortened {
        shove_width = shove_width.min(from_corner.distance(&to_corner));
    }
    let shove_line = shove_line_segment.middle;
    // A door must lie between the one entered by and the end of the push.
    let from_door_compare_distance = if dimension == 2 || segment_is_point { f64::MAX } else { to_corner.distance_square(&from_door_shape.corner_approx(0)) };
    for curr_door in engine.graph.doors_of(room).to_vec() {
        if curr_door == from_door {
            continue;
        }
        let d = *engine.graph.door(curr_door);
        if let (Some((first_item, _)), Some((second_item, _))) = (engine.obstacle_item(d.first), engine.obstacle_item(d.second)) {
            if first_item != second_item {
                // There may be topological problems at a trace fork.
                continue;
            }
        }
        let curr_door_shape = engine.graph.door_tile_shape(curr_door);
        if d.dimension == 2 && shove_width >= i32::MAX as f64 {
            if curr_door_shape.contains_float(&to_corner) {
                let sections = engine.section_segments(curr_door, compensated_trace_half_width);
                to_doors.push(DoorSection { door: curr_door, section: 0, line: sections[0] });
            }
        } else if !segment_is_point {
            // A one-dimensional door on the same side of the push.
            let Some(curr_door_segment) = curr_door_shape.diagonal_corner_segment() else { continue };
            let wanted = if shove_to_the_left { Side::Left } else { Side::Right };
            if shove_line.side_of_float((curr_door_segment.a.x, curr_door_segment.a.y), 0.0) != wanted || shove_line.side_of_float((curr_door_segment.b.x, curr_door_segment.b.y), 0.0) != wanted {
                continue;
            }
            let curr_door_line = curr_door_shape.polar_line_segment(&from_corner).expect("a door is not empty");
            let nearest = |l: &FloatLine| if l.a.distance_square(&from_corner) <= l.b.distance_square(&from_corner) { l.a } else { l.b };
            let curr_door_nearest_corner = nearest(&curr_door_line);
            if to_corner.distance_square(&curr_door_nearest_corner) >= from_door_compare_distance {
                // Not towards the end of the push.
                continue;
            }
            let curr_door_projection = curr_door_nearest_corner.projection_approx(&shove_line);
            if curr_door_projection.distance(&from_corner) + compensated_trace_half_width <= shove_width {
                let sections = engine.section_segments(curr_door, compensated_trace_half_width);
                for (i, section) in sections.iter().enumerate() {
                    let projection = nearest(section).projection_approx(&shove_line);
                    if projection.distance(&from_corner) <= shove_width {
                        to_doors.push(DoorSection { door: curr_door, section: i, line: *section });
                    }
                }
            }
        }
    }
    true
}

/// Whether the trace and the item pushed before it meet end to end, so the
/// push can go on through a link door. `end_points_matching`.
fn end_points_matching(rb: &RoutingBoard, trace: usize, from_item: usize) -> bool {
    if from_item == trace {
        return true;
    }
    let (t, f) = (&rb.board.items[trace], &rb.board.items[from_item]);
    if !t.shares_net(f) {
        return false;
    }
    let ItemKind::Trace { polyline, .. } = &t.kind else { return false };
    let (first, last) = (polyline.first_corner(), polyline.last_corner());
    match &f.kind {
        ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => {
            let c = Point::Int(*center);
            c.java_equals(&first) || c.java_equals(&last)
        }
        ItemKind::Trace { polyline: from, .. } => {
            let (from_first, from_last) = (from.first_corner(), from.last_corner());
            first.java_equals(&from_first) || first.java_equals(&from_last) || last.java_equals(&from_first) || last.java_equals(&from_last)
        }
        _ => false,
    }
}
