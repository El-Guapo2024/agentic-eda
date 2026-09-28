//! Traces joined where they meet end to end, split where they cross, and
//! cleaned of cycles: what keeps a net's traces tidy after every insertion.
//! Ported from FreeRouting's `PolylineTrace.combine`, `split`, `normalize`
//! and `change`, `Trace.is_cycle`, `Item.get_connection_items`, and the
//! board's `remove_if_cycle`, `normalize_traces`, `combine_traces` and
//! `split_traces`.

use crate::geometry::{IntOctagon, Line, LineSegment, Point, Polyline};
use crate::model::{AreaKind, ItemKind};

use super::{nets_equal, point_shape, Pick, RoutingBoard};

/// Where [`RoutingBoard::connection_items`] stops. `Item.StopConnectionOption`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopConnection {
    None,
    FanoutVia,
    Via,
}

/// `PolylineTrace.MAX_NORMALIZATION_DEPTH`.
const MAX_NORMALIZATION_DEPTH: usize = 16;

impl RoutingBoard {
    fn is_conduction(&self, item: usize) -> bool {
        matches!(self.board.items[item].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. })
    }

    fn is_drill(&self, item: usize) -> bool {
        matches!(self.board.items[item].kind, ItemKind::Pin { .. } | ItemKind::Via { .. })
    }

    /// Join the trace with the trace it meets end to end, again and again:
    /// true if anything was joined. The trace keeps its number; the others
    /// go. `PolylineTrace.combine`.
    pub fn combine(&mut self, item: usize) -> bool {
        let mut changed = false;
        while self.is_on_board(item) {
            if self.combine_at_start(item) || self.combine_at_end(item) {
                changed = true;
            } else {
                break;
            }
        }
        changed
    }

    /// The trace ending at this one's start, if it is the only thing there
    /// and alike -- layer, nets, width, fixed state -- with whether it
    /// runs the other way.
    fn combine_partner(&self, item: usize, corner: &Point, at_start: bool) -> Option<(usize, bool)> {
        let mut contacts = self.trace_contacts_at(item, corner);
        contacts.retain(|&c| !self.is_conduction(c));
        if contacts.len() != 1 {
            return None;
        }
        let other = contacts[0];
        if !self.is_trace(other) {
            return None;
        }
        let (this, that) = (&self.board.items[item], &self.board.items[other]);
        let (other_poly, other_layer, other_hw) = self.trace(other);
        let (_, layer, hw) = self.trace(item);
        if !(other_layer == layer && nets_equal(&that.nets, &this.nets) && other_hw == hw && that.fixed == this.fixed) {
            return None;
        }
        // At the start the other trace should end there; at the end it
        // should start there. Otherwise it runs the other way.
        let (same_way, other_way) = if at_start { (other_poly.last_corner(), other_poly.first_corner()) } else { (other_poly.first_corner(), other_poly.last_corner()) };
        if corner.java_equals(&same_way) {
            Some((other, false))
        } else if corner.java_equals(&other_way) {
            Some((other, true))
        } else {
            None
        }
    }

    /// `PolylineTrace.combine_at_start(true)`.
    fn combine_at_start(&mut self, item: usize) -> bool {
        let start_corner = self.first_corner(item);
        let Some((other, reverse)) = self.combine_partner(item, &start_corner, true) else { return false };
        let this_lines = self.trace(item).0.lines.clone();
        let other_lines = oriented_lines(&self.trace(other).0.lines, reverse);
        let skip_line = other_lines[other_lines.len() - 2].is_equal_or_opposite(&this_lines[1]);
        let mut new_lines: Vec<Line> = other_lines[..other_lines.len() - 1].to_vec();
        if skip_line {
            new_lines.pop();
        }
        new_lines.extend_from_slice(&this_lines[1..]);
        // Joining in front of this trace, the other one starts it; it runs
        // the other way if it starts where this one does.
        let change_order = self.first_corner(other).java_equals(&start_corner);
        self.finish_combine(item, other, &new_lines, &start_corner, true, change_order);
        true
    }

    /// `PolylineTrace.combine_at_end(true)`.
    fn combine_at_end(&mut self, item: usize) -> bool {
        let end_corner = self.last_corner(item);
        let Some((other, reverse)) = self.combine_partner(item, &end_corner, false) else { return false };
        let this_lines = self.trace(item).0.lines.clone();
        let other_lines = oriented_lines(&self.trace(other).0.lines, reverse);
        let skip_line = this_lines[this_lines.len() - 2].is_equal_or_opposite(&other_lines[1]);
        let mut new_lines: Vec<Line> = this_lines[..this_lines.len() - 1].to_vec();
        if skip_line {
            new_lines.pop();
        }
        new_lines.extend_from_slice(&other_lines[1..]);
        let change_order = self.last_corner(other).java_equals(&end_corner);
        self.finish_combine(item, other, &new_lines, &end_corner, false, change_order);
        true
    }

    /// Give the trace the joined lines and remove the other. The autoroute
    /// trees keep the leaves of both but where they meet -- unless lines
    /// fell out of the joined polyline, parallel where they meet, when the
    /// trace is entered afresh.
    fn finish_combine(&mut self, item: usize, other: usize, new_lines: &[Line], corner: &Point, in_front: bool, change_order: bool) {
        let joined = Polyline::from_lines(new_lines);
        let short = joined.lines.len() < 3;
        if joined.lines.len() != new_lines.len() {
            self.set_polyline(item, joined);
        } else {
            self.merge_polyline_entries(other, item, joined, in_front, change_order);
        }
        if short {
            self.remove_item(item);
        }
        self.remove_item(other);
        let (_, layer, _) = self.trace(item);
        let (x, y) = corner.to_float();
        self.join_changed_area(crate::geometry::FloatPoint::new(x, y), layer);
    }

    /// Split the trace where traces of its nets cross or overlap it, and
    /// where it runs through the centre of a pin or via of its nets; within
    /// `clip` if given. The pieces, cycles among them removed; just the
    /// trace if nothing was split. `PolylineTrace.split(IntOctagon)`.
    pub fn split_trace(&mut self, item: usize, clip: Option<&IntOctagon>) -> Vec<usize> {
        let mut result = Vec::new();
        if !self.nets_normal(&self.board.items[item].nets) {
            result.push(item);
            return result;
        }
        let mut own_trace_split = false;
        let lines = self.trace(item).0.clone();
        let layer = self.trace(item).1;
        for i in 0..lines.lines.len().saturating_sub(2) {
            if let Some(clip) = clip {
                let segment = LineSegment::of(&lines, i + 1);
                if !clip.intersects(&segment.bounding_box().to_octagon()) {
                    continue;
                }
            }
            let Some(curr_shape) = self.tree.get_shape(item, i as u32).cloned() else { continue };
            let curr_line_segment = LineSegment::of(&lines, i + 1);
            let mut entries: Vec<(usize, u32)> = self.tree.overlapping_entries(&self.board, &curr_shape, layer, &[]).into_iter().map(|(a, b, _)| (a, b)).collect();
            let mut pos = 0;
            while pos < entries.len() {
                if !self.is_on_board(item) {
                    // Removed in a clean-up.
                    return result;
                }
                let (found_item, found_index) = entries[pos];
                pos += 1;
                if found_item == item {
                    let fi = found_index as usize;
                    if fi + 1 >= i && fi <= i + 1 {
                        continue;
                    }
                    // Segments of length 0 between: compare end corners.
                    if i < fi {
                        if lines.corner(i + 1).java_equals(&lines.corner(fi)) {
                            continue;
                        }
                    } else if lines.corner(fi + 1).java_equals(&lines.corner(i)) {
                        continue;
                    }
                }
                if !self.board.items[found_item].shares_net(&self.board.items[item]) {
                    continue;
                }
                if self.is_trace(found_item) {
                    let found_lines = self.trace(found_item).0.clone();
                    let found_line_segment = LineSegment::of(&found_lines, found_index as usize + 1);
                    let intersecting_lines = found_line_segment.intersection(&curr_line_segment);
                    let mut split_pieces: Vec<usize> = Vec::new();
                    let mut found_trace_split = false;
                    if found_item != item {
                        for line in &intersecting_lines {
                            if let Some(pieces) = self.split_at_line(found_item, found_index as usize + 1, line) {
                                for p in pieces.into_iter().flatten() {
                                    found_trace_split = true;
                                    split_pieces.push(p);
                                }
                                if found_trace_split {
                                    // The board has changed: read the
                                    // entries again, after those read, and
                                    // start over.
                                    entries.extend(self.tree.overlapping_entries(&self.board, &curr_shape, layer, &[]).into_iter().map(|(a, b, _)| (a, b)));
                                    pos = 0;
                                    break;
                                }
                            }
                        }
                        if !found_trace_split {
                            split_pieces.push(found_item);
                        }
                    }
                    // Now split this trace.
                    let intersecting_lines = curr_line_segment.intersection(&found_line_segment);
                    for line in &intersecting_lines {
                        if let Some(pieces) = self.split_at_line(item, i + 1, line) {
                            own_trace_split = true;
                            if let Some(p) = pieces[0] {
                                let more = self.split_trace(p, clip);
                                result.extend(more);
                            }
                            if let Some(p) = pieces[1] {
                                let more = self.split_trace(p, clip);
                                result.extend(more);
                            }
                            break;
                        }
                    }
                    if found_trace_split || own_trace_split {
                        // Remove cycles through a piece, this trace's last
                        // to keep them if possible.
                        for p in split_pieces {
                            self.remove_if_cycle(p);
                        }
                        for p in result.clone() {
                            self.remove_if_cycle(p);
                        }
                    }
                    if own_trace_split {
                        break;
                    }
                } else if self.is_drill(found_item) {
                    let center = self.board.items[found_item].center().expect("a drill item");
                    if curr_line_segment.contains(center) {
                        let split_line = Line::through(center, curr_line_segment.middle.direction().turn_45_degree(2));
                        self.split_at_line(item, i + 1, &split_line);
                    }
                } else if !self.board.items[item].is_user_fixed() && self.is_conduction(found_item) {
                    let ignore_areas = self.board.items[item].nets.first().and_then(|&n| self.board.rules.net_class(n)).is_some_and(|c| c.ignore_cycles_with_areas);
                    if !ignore_areas && self.start_contacts(item).contains(&found_item) && self.end_contacts(item).contains(&found_item) {
                        // A cycle through the pour: the trace can go.
                        self.remove_item(item);
                        return result;
                    }
                }
            }
            if own_trace_split {
                break;
            }
        }
        if !own_trace_split {
            result.push(item);
        }
        result
    }

    /// Whether splitting at `line` crosses line `line_no` inside a pad of a
    /// pin of the trace's nets away from its centre -- not allowed -- with
    /// no trace of those nets ending there. `split_inside_drill_pad_prohibited`.
    fn split_inside_drill_pad_prohibited(&self, item: usize, line_no: usize, line: &Line) -> bool {
        let (polyline, layer, _) = self.trace(item);
        let intersection = polyline.lines[line_no].intersection(line);
        let this = &self.board.items[item];
        let mut pad_found = false;
        for i in self.pick_items(&intersection, layer, Pick::All) {
            let other = &self.board.items[i];
            if !other.shares_net(this) {
                continue;
            }
            match &other.kind {
                ItemKind::Pin { center, .. } => {
                    if Point::Int(*center).java_equals(&intersection) {
                        return false;
                    }
                    pad_found = true;
                }
                ItemKind::Trace { polyline: p, .. } => {
                    // As the Java has it: `a && b || c`.
                    if (i != item && p.first_corner().java_equals(&intersection)) || p.last_corner().java_equals(&intersection) {
                        return false;
                    }
                }
                _ => {}
            }
        }
        pad_found
    }

    /// Split the trace in two at line `line_no` by `end_line`, which ends
    /// the first piece and starts the second: the two new traces, either
    /// `None` if too short to insert; `None` if nothing was split.
    /// `PolylineTrace.split(int, Line)`.
    pub fn split_at_line(&mut self, item: usize, line_no: usize, end_line: &Line) -> Option<[Option<usize>; 2]> {
        if !self.is_on_board(item) {
            return None;
        }
        let [first, second] = self.trace(item).0.split(line_no, end_line)?;
        if self.split_inside_drill_pad_prohibited(item, line_no, end_line) {
            return None;
        }
        self.remove_item(item);
        let (_, layer, half_width) = self.trace(item);
        let it = &self.board.items[item];
        let (nets, class, fixed) = (it.nets.clone(), it.clearance_class, it.fixed);
        let a = self.insert_trace_without_cleaning(first, layer, half_width, &nets, class, fixed);
        let b = self.insert_trace_without_cleaning(second, layer, half_width, &nets, class, fixed);
        Some([a, b])
    }

    /// Split the trace at `p`, where a segment holds it, square to the
    /// segment. `PolylineTrace.split(Point)`.
    pub fn split_at_point(&mut self, item: usize, p: &Point) -> Option<[Option<usize>; 2]> {
        let q = p.as_int()?;
        let lines = self.trace(item).0.clone();
        for i in 0..lines.lines.len().saturating_sub(2) {
            let segment = LineSegment::of(&lines, i + 1);
            if segment.contains(q) {
                let split_line = Line::through(q, segment.middle.direction().turn_45_degree(2));
                if let Some(result) = self.split_at_line(item, i + 1, &split_line) {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Split the trace and the traces it overlaps, and join what meets end
    /// to end: true if anything changed. `PolylineTrace.normalize`.
    pub fn normalize(&mut self, item: usize, clip: Option<&IntOctagon>) -> Result<bool, String> {
        self.normalize_at(item, clip, 0)
    }

    fn normalize_at(&mut self, item: usize, clip: Option<&IntOctagon>, depth: usize) -> Result<bool, String> {
        if depth > MAX_NORMALIZATION_DEPTH {
            return Err(format!("We reached the maximum normalization depth ({MAX_NORMALIZATION_DEPTH})."));
        }
        let pieces = self.split_trace(item, clip);
        let mut result = pieces.len() != 1;
        for piece in pieces {
            if !self.is_on_board(piece) {
                continue;
            }
            let combined = self.combine(piece);
            let polyline = self.trace(piece).0;
            if polyline.corner_count() == 2 && polyline.first_corner().java_equals(&polyline.last_corner()) {
                self.remove_item(piece);
                result = true;
            } else if combined {
                self.normalize_at(piece, clip, depth + 1)?;
                result = true;
            }
        }
        Ok(result)
    }

    /// Give the trace a new polyline, then normalize it within the changed
    /// area; the autoroute trees keep the leaves of the shapes at either
    /// end whose lines are unchanged. Nothing changes if the lines agree as
    /// far as the shorter polyline goes, from either end. `PolylineTrace.change`,
    /// which compares lines by identity; they compare by value here, which
    /// differs only for a line made anew just as it was.
    pub fn change(&mut self, item: usize, polyline: Polyline) {
        if !self.is_on_board(item) {
            self.put_polyline(item, polyline);
            return;
        }
        let (old, new) = (&self.trace(item).0.lines, &polyline.lines);
        let last_index = old.len().min(new.len());
        let Some(first_different) = (0..last_index).find(|&i| new[i] != old[i]) else { return };
        let Some(last_different) = (1..=last_index).find(|&i| new[new.len() - i] != old[old.len() - i]).map(|i| new.len() - i) else { return };
        let keep_start = first_different.saturating_sub(2);
        let keep_end = (new.len() as i64 - last_different as i64 - 3).max(0) as usize;
        self.change_polyline_entries(item, polyline, keep_start, keep_end);
        let (_, layer, _) = self.trace(item);
        let clip = self.changed_area_on(layer);
        let _ = self.normalize(item, clip.as_ref());
    }

    /// Whether the trace meets the same item at both ends.
    /// `Trace.is_overlap`.
    fn is_overlap(&self, item: usize) -> bool {
        let start = self.start_contacts(item);
        let end = self.end_contacts(item);
        start.iter().any(|c| end.contains(c))
    }

    /// Whether the trace can be reached from its start by another way
    /// round. `Trace.is_cycle`.
    pub fn is_cycle(&self, item: usize) -> bool {
        if self.is_overlap(item) {
            return true;
        }
        let start_contacts = self.start_contacts(item);
        let mut visited: Vec<usize> = start_contacts.clone();
        let ignore_areas = self.board.items[item].nets.first().and_then(|&n| self.board.rules.net_class(n)).is_some_and(|c| c.ignore_cycles_with_areas);
        start_contacts.iter().any(|&c| self.is_cycle_recu(c, &mut visited, item, item, ignore_areas))
    }

    /// `Item.is_cycle_recu`.
    fn is_cycle_recu(&self, at: usize, visited: &mut Vec<usize>, search_item: usize, come_from: usize, ignore_areas: bool) -> bool {
        if ignore_areas && self.is_conduction(at) {
            return false;
        }
        for c in self.normal_contacts(at) {
            if c == come_from {
                continue;
            }
            if c == search_item {
                return true;
            }
            if !visited.contains(&c) {
                visited.push(c);
                if self.is_cycle_recu(c, visited, search_item, at, ignore_areas) {
                    return true;
                }
            }
        }
        false
    }

    /// Remove the trace with its connection if it closes a cycle, and any
    /// tail that leaves where there was none. `BasicBoard.remove_if_cycle`.
    pub fn remove_if_cycle(&mut self, item: usize) -> bool {
        if !self.is_on_board(item) || !self.is_cycle(item) {
            return false;
        }
        let (polyline, layer, _) = self.trace(item);
        let end_corners = [polyline.first_corner(), polyline.last_corner()];
        let nets = self.board.items[item].nets.clone();
        let tail_before: Vec<bool> = end_corners.iter().map(|p| self.get_trace_tail(p, layer, &nets).is_some()).collect();
        let connection = self.connection_items(item, StopConnection::None);
        self.remove_items(&connection, false);
        for i in 0..2 {
            if !tail_before[i] {
                if let Some(tail) = self.get_trace_tail(&end_corners[i], layer, &nets) {
                    let items = self.connection_items(tail, StopConnection::None);
                    self.remove_items(&items, false);
                }
            }
        }
        true
    }

    /// The point two items connect at, if there is exactly one.
    /// `Item.normal_contact_point`, through its double dispatch.
    pub(crate) fn normal_contact_point(&self, a: usize, b: usize) -> Option<Point> {
        let (ia, ib) = (&self.board.items[a], &self.board.items[b]);
        match (&ia.kind, &ib.kind) {
            (ItemKind::Trace { layer: la, polyline: pa, .. }, ItemKind::Trace { layer: lb, polyline: pb, .. }) => {
                // b.normal_contact_point(Trace a).
                if la != lb {
                    return None;
                }
                let at_first = pb.first_corner().java_equals(&pa.first_corner()) || pb.first_corner().java_equals(&pa.last_corner());
                let at_last = pb.last_corner().java_equals(&pa.first_corner()) || pb.last_corner().java_equals(&pa.last_corner());
                if at_first == at_last {
                    None
                } else if at_first {
                    Some(pb.first_corner())
                } else {
                    Some(pb.last_corner())
                }
            }
            (ItemKind::Trace { polyline, .. }, ItemKind::Pin { center, .. } | ItemKind::Via { center, .. })
            | (ItemKind::Pin { center, .. } | ItemKind::Via { center, .. }, ItemKind::Trace { polyline, .. }) => {
                if !super::shares_layer(ia, ib) {
                    return None;
                }
                let c = Point::Int(*center);
                (c.java_equals(&polyline.first_corner()) || c.java_equals(&polyline.last_corner())).then_some(c)
            }
            (ItemKind::Pin { center: ca, .. } | ItemKind::Via { center: ca, .. }, ItemKind::Pin { center: cb, .. } | ItemKind::Via { center: cb, .. }) => {
                (super::shares_layer(ia, ib) && ca == cb).then_some(Point::Int(*ca))
            }
            _ => None,
        }
    }

    /// `Item.first_common_layer`, `-1` for none.
    pub(crate) fn first_common_layer(&self, a: usize, b: usize) -> i32 {
        let (ia, ib) = (&self.board.items[a], &self.board.items[b]);
        let first = ia.first_layer.max(ib.first_layer);
        if first > ia.last_layer.min(ib.last_layer) {
            -1
        } else {
            first
        }
    }

    /// The traces and vias from the item to the next fork or item that
    /// cannot be routed, both ways, by number descending.
    /// `Item.get_connection_items`.
    pub fn connection_items(&self, item: usize, stop: StopConnection) -> Vec<usize> {
        let contacts = self.normal_contacts(item);
        let mut result = Vec::new();
        if self.board.items[item].is_routable() {
            result.push(item);
        }
        for start in contacts {
            let Some(mut prev_point) = self.normal_contact_point(item, start) else { continue };
            let mut prev_layer = self.first_common_layer(item, start);
            if self.is_trace(item) && self.trace_contacts_at(item, &prev_point).len() != 1 {
                continue;
            }
            let mut curr = start;
            loop {
                if !self.board.items[curr].is_routable() {
                    break;
                }
                if matches!(self.board.items[curr].kind, ItemKind::Via { .. }) {
                    match stop {
                        StopConnection::Via => break,
                        StopConnection::FanoutVia => unimplemented!("stopping at fanout vias (Item.is_fanout_via) is not ported yet"),
                        StopConnection::None => {}
                    }
                }
                if !result.contains(&curr) {
                    result.push(curr);
                }
                let mut next: Option<(usize, Point, i32)> = None;
                let mut fork_found = false;
                for c in self.normal_contacts(curr) {
                    let layer = self.first_common_layer(curr, c);
                    if layer < 0 {
                        continue;
                    }
                    let Some(point) = self.normal_contact_point(curr, c) else {
                        fork_found = true;
                        break;
                    };
                    if prev_layer != layer || !prev_point.java_equals(&point) {
                        if next.is_some() {
                            fork_found = true;
                            break;
                        }
                        next = Some((c, point, layer));
                    }
                }
                match next {
                    Some((c, point, layer)) if !fork_found => {
                        curr = c;
                        prev_point = point;
                        prev_layer = layer;
                    }
                    _ => break,
                }
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// Join the net's traces that meet end to end, until none do; every
    /// net if `net` is negative. `BasicBoard.combine_traces`.
    pub fn combine_traces(&mut self, net: i32) -> bool {
        let mut result = false;
        'again: loop {
            for i in self.items_in_order() {
                if !self.is_on_board(i) || !self.is_trace(i) {
                    continue;
                }
                if (net < 0 || self.board.items[i].contains_net(net)) && self.combine(i) {
                    result = true;
                    continue 'again;
                }
            }
            return result;
        }
    }

    /// Normalize the net's traces, and remove those closing cycles, until
    /// nothing changes. `BasicBoard.normalize_traces`.
    pub fn normalize_traces(&mut self, net: i32) -> Result<bool, String> {
        let mut result = false;
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            // The Java iterates the board while changing it; items it has
            // passed or that come new -- numbered higher, so in front -- are
            // not seen again this round, and those removed are skipped.
            for i in self.items_in_order() {
                if !self.is_on_board(i) || !self.is_trace(i) || !self.board.items[i].contains_net(net) {
                    continue;
                }
                // Normalized, or else removed as a cycle.
                if self.normalize(i, None)? || (!self.board.items[i].is_user_fixed() && self.remove_if_cycle(i)) {
                    something_changed = true;
                    result = true;
                }
            }
        }
        Ok(result)
    }

    /// Split the net's traces through `p` on `layer`. `BasicBoard.split_traces`,
    /// which clips to the octagon of the box round `p`.
    pub fn split_traces(&mut self, p: &Point, layer: i32, net: i32) -> bool {
        let Some(location) = point_shape(p).bounding_octagon() else { return false };
        let mut split = false;
        for i in self.pick_items(p, layer, Pick::Traces) {
            if self.board.items[i].contains_net(net) && self.split_trace(i, Some(&location)).len() != 1 {
                split = true;
            }
        }
        split
    }
}

/// A trace's lines, turned round if `reverse`, each line flipped.
fn oriented_lines(lines: &[Line], reverse: bool) -> Vec<Line> {
    if reverse {
        lines.iter().rev().map(|l| l.opposite()).collect()
    } else {
        lines.to_vec()
    }
}
