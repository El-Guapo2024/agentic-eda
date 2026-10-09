//! The verbs the Properties panel needs where no existing verb fits (`pcbnew/widgets/pcb_properties_panel.cpp` at KiCad 8303b2ad).
//!
//! The panel is a property grid (`PROPERTIES_PANEL`): one edit of one row sets one property on every selected item, and
//! `PCB_PROPERTIES_PANEL::valueChanged` pushes the whole thing as ONE `BOARD_COMMIT` ("Edit Properties"). The studio builds that
//! as a single `Cmd::Batch`, out of the verbs that already existed (`move_items`, `rotate_items`, `flip_items`, `set_locked`,
//! `set_track_width`, `edit_tracks_and_vias`, `edit_via`, `edit_zone`, `edit_shape`, `edit_text`, `edit_dimension`, `edit_group`).
//! Four properties had none, and each gets the smallest verb that does it:
//!
//! * `PCB_TRACK::SetStartX/Y`, `SetEndX/Y` ("Start X", "End Y" ...): [`Cmd::EditTrack`](crate::Cmd::EditTrack);
//! * `BOARD_CONNECTED_ITEM::SetNetCode` ("Net"): [`Cmd::SetItemNet`](crate::Cmd::SetItemNet), for tracks, vias and zones;
//! * `ZONE::SetZoneName` ("Name"): [`Cmd::SetZoneName`](crate::Cmd::SetZoneName);
//! * `EDA_SHAPE::SetStartX/SetEndX/SetCenterX/SetRadius/SetRectangleWidth` ("Start X", "Radius", "Width" ...):
//!   [`Cmd::ReplaceShape`](crate::Cmd::ReplaceShape) -- the shape with its new geometry, in place, keeping its id and its lock (the
//!   studio plans the geometry the way those setters do, `kicad-port/pcbProperties.ts`).

use super::Board;
use eda_model::ir::{tessellate_arc, Point, Shape, Track, TRACK_ARC_SEGMENTS};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

impl Board<'_> {
    /// `Cmd::EditTrack`: move the first and/or the last point of a track. A field left out stays where it is. A KiCad arc
    /// (`PCB_ARC`: start, mid, end) keeps its mid point and is drawn through the new end points, as `PCB_ARC::SetStart`/`SetEnd`
    /// do; any other track is a polyline whose first or last vertex moves.
    pub(crate) fn edit_track(&mut self, id: &str, start: Option<Point>, end: Option<Point>) -> Result<(), Vec<CheckResult>> {
        if start.is_none() && end.is_none() {
            return Err(fail("ops_bad_track", id, "nothing to change: give a new start, a new end, or both"));
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| fail("ops_unknown_track", id, "the board has no routing yet"))?;
        let t = rt.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| fail("ops_unknown_track", id, "no track with this id"))?;
        let moved = moved_ends(t, start, end);
        if moved.pts.len() == 2 && moved.pts[0] == moved.pts[1] {
            return Err(fail("ops_bad_track", id, "a track needs two different end points"));
        }
        t.pts = moved.pts;
        t.arc_mid_offset = moved.arc_mid_offset;
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::SetItemNet`: `BOARD_CONNECTED_ITEM::SetNetCode` on tracks, vias and zones. The net must exist in the model; only a zone may be given
    /// no net at all ("" -- a rule area has none), as `add_zone` allows.
    pub(crate) fn set_item_net(&mut self, ids: &[String], net: &str) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_bad_net", "set_item_net", "no items given"));
        }
        if !net.is_empty() {
            self.known_net(net)?;
        }
        let (tracks, vias, zones): (BTreeSet<&str>, BTreeSet<&str>, BTreeSet<&str>) = match self.design.routing.as_ref() {
            Some(rt) => (
                rt.tracks.iter().map(|t| t.id.as_str()).collect(),
                rt.vias.iter().map(|v| v.id.as_str()).collect(),
                rt.zones.iter().map(|z| z.id.as_str()).collect(),
            ),
            None => Default::default(),
        };
        for id in ids {
            if !tracks.contains(id.as_str()) && !vias.contains(id.as_str()) && !zones.contains(id.as_str()) {
                return Err(fail("ops_unknown_item", id, "no track, via or zone with this id"));
            }
            if net.is_empty() && !zones.contains(id.as_str()) {
                return Err(fail("ops_bad_net", id, "a track or a via needs a net"));
            }
        }
        let wanted: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        let rt = self.design.routing.as_mut().expect("the ids were found in the routing");
        for t in rt.tracks.iter_mut().filter(|t| wanted.contains(t.id.as_str())) {
            t.net = net.to_string();
        }
        for v in rt.vias.iter_mut().filter(|v| wanted.contains(v.id.as_str())) {
            v.net = net.to_string();
        }
        for z in rt.zones.iter_mut().filter(|z| wanted.contains(z.id.as_str())) {
            z.net = net.to_string();
        }
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::SetZoneName`: `ZONE::SetZoneName` (the label the Zone Manager shows). Empty is KiCad's own "unnamed".
    pub(crate) fn set_zone_name(&mut self, id: &str, name: &str) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", id, "the board has no routing yet"))?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| fail("ops_unknown_zone", id, "no zone with this id"))?;
        z.name = name.trim().to_string();
        Ok(())
    }

    /// `Cmd::ReplaceShape`: the drawn shape `id` becomes `shape`, in place -- its id, its place in the drawing order and its lock stay.
    /// Refused for what `add_shape` refuses (no layer, a polygon under three points) and for a width below zero or a circle without a radius.
    pub(crate) fn replace_shape(&mut self, id: &str, mut shape: Shape) -> Result<(), Vec<CheckResult>> {
        if shape.layer().is_empty() {
            return Err(fail("ops_bad_shape", id, "a shape needs a layer"));
        }
        if shape.stroke_width() < 0 {
            return Err(fail("ops_bad_shape", id, "line width cannot be negative"));
        }
        match &shape {
            Shape::Polygon { pts, .. } if pts.len() < 3 => return Err(fail("ops_bad_shape", id, "a polygon needs at least three points")),
            Shape::Circle { center, end, .. } if center == end => return Err(fail("ops_bad_shape", id, "a circle needs a radius")),
            _ => {}
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_shape", id, "the board has no drawings yet"))?;
        let slot = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| fail("ops_unknown_shape", id, "no shape with this id"))?;
        shape.set_id(id.to_string());
        *slot = shape;
        Ok(())
    }
}

/// A track's points and arc mid offset once its first and/or last point is where `start` / `end` say.
struct MovedEnds {
    pts: Vec<Point>,
    arc_mid_offset: Option<Point>,
}

fn moved_ends(t: &Track, start: Option<Point>, end: Option<Point>) -> MovedEnds {
    if let Some((s, mid, e)) = t.arc() {
        let (s, e) = (start.unwrap_or(s), end.unwrap_or(e));
        return MovedEnds { pts: tessellate_arc(s, mid, e, TRACK_ARC_SEGMENTS), arc_mid_offset: Some(Point { x: mid.x - s.x, y: mid.y - s.y }) };
    }
    let mut pts = t.pts.clone();
    if let (Some(s), Some(first)) = (start, pts.first_mut()) {
        *first = s;
    }
    if let (Some(e), Some(last)) = (end, pts.last_mut()) {
        *last = e;
    }
    // A plain polyline that carried a stale arc mid offset (the points were reshaped since) keeps it: `Track::arc` ignores it unless the points
    // still are the arc's tessellation, exactly as before this edit.
    MovedEnds { pts, arc_mid_offset: t.arc_mid_offset }
}
