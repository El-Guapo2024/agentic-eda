//! Move, Rotate and Flip for every kind of PCB item, as three undoable verbs.
//!
//! Ports `EDIT_TOOL::Move`, `EDIT_TOOL::Rotate` and `EDIT_TOOL::Flip`
//! (`pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp` at KiCad 8303b2ad) down to what each item class does
//! with the transform: `BOARD_ITEM::Move`, `::Rotate`, `::Flip` of `PCB_TRACK`/`PCB_ARC`/`PCB_VIA`, `ZONE`,
//! `PCB_SHAPE` (`EDA_SHAPE::rotate`/`::flip`), `PCB_TEXT`, `PCB_DIMENSION_BASE` and its ortho/aligned subclasses,
//! `FOOTPRINT` and `PCB_GROUP`. The tool part of the real command (which items, about which point, which items a
//! lock keeps out) is the caller's: the studio reads KiCad's rules for those in
//! `web/studio/src/kicad-port/pcbTransform.ts` and sends the result here as one command, so one undo step covers
//! the whole selection (one `BOARD_COMMIT::Push` in source).
//!
//! # Conventions
//!
//! * Angles are millidegrees, **positive turns clockwise on the screen** (Y grows downward), the sense of
//!   `rotate_point_about` and of a footprint's `rot`. KiCad's own angles run the other way (`RotatePoint( +90 )`
//!   is counter-clockwise on the screen); the studio negates where it ports a KiCad angle.
//! * An item keeps its id: ids are assigned once, never recomputed from the new geometry (see `Track::id`).
//! * A group moves, turns and flips as its members do; the members that are footprints included.
//! * A footprint goes where the edit says (`Board::set_pose_exact`: `FOOTPRINT::Move` is `m_pos + aMoveVector`), never rounded to the board's placement step:
//!   the editor snapped the pointer as its "Snap to grid" says (`GRID_HELPER::canUseGrid`), so a part moved with Snap to grid Never stays off the grid
//!   (UI-LEFT-3 C3-1). Only the CLI's and the agents' own placer verbs (`move --to`, `nudge`, `place --near/--edge/--region`) keep the step.
//! * **The routing stays where it is** (`EDIT_TOOL::Move` / `Rotate` / `Flip` and `DeleteItems` touch the items they name, never the copper
//!   routed to a footprint's pads, which is left unconnected for the ratsnest to show). A footprint moved away from its tracks is judged by
//!   the studio's Strict switch like any other edit that opens a connection (`routing_connectivity`); the older part verbs (`move_to`,
//!   `rotate`, `flip`, ... of the CLI and the agents) still clear a routing they make stale (`Cmd::clears_routing`).
//! * Locks are the caller's business, as they are in source: `FilterCollectorForLockedItems` keeps locked items out
//!   of the selection the tool then works on; the item's own `Move`/`Rotate`/`Flip` never look at them.

use super::{rotate_point_about, Board};
use eda_model::fp_edit::{parse_field_id, PadEdit};
use eda_model::ir::{tessellate_arc, Dimension, DimensionKind, FootprintInstance, LabelSide, Millideg, Point, Shape, Side, Text, Track, Nm, Via, Zone, TRACK_ARC_SEGMENTS};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `FLIP_DIRECTION` (`include/geometry/flip_direction.h`): which way an item is turned over. `LeftRight` mirrors
/// across a vertical axis (x changes), KiCad's default for `Change Side / Flip` (`m_FlipDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipDirection {
    LeftRight,
    TopBottom,
}

/// One rigid transform of the board plane, applied to the points, the vectors and the orientations of an item.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Xform {
    Move { dx: Nm, dy: Nm },
    /// Clockwise-positive millidegrees about `pivot`.
    Rotate { pivot: Point, angle: i64 },
    /// `Flip( aCentre, aFlipDirection )`: mirror across the axis through `pivot`, and turn the item over to the other
    /// side of the board.
    Flip { pivot: Point, dir: FlipDirection },
}

impl Xform {
    pub(crate) fn point(self, p: Point) -> Point {
        match self {
            Xform::Move { dx, dy } => Point { x: p.x + dx, y: p.y + dy },
            Xform::Rotate { pivot, angle } => rotate_point_about(p, pivot, angle),
            Xform::Flip { pivot, dir: FlipDirection::LeftRight } => Point { x: 2 * pivot.x - p.x, y: p.y },
            Xform::Flip { pivot, dir: FlipDirection::TopBottom } => Point { x: p.x, y: 2 * pivot.y - p.y },
        }
    }

    /// The same transform on an offset between two points of one item (the mid point of an arc relative to its
    /// start): a translation leaves it alone.
    fn vector(self, v: Point) -> Point {
        match self {
            Xform::Move { .. } => v,
            Xform::Rotate { angle, .. } => rotate_point_about(v, Point { x: Nm::ZERO, y: Nm::ZERO }, angle),
            Xform::Flip { dir: FlipDirection::LeftRight, .. } => Point { x: -v.x, y: v.y },
            Xform::Flip { dir: FlipDirection::TopBottom, .. } => Point { x: v.x, y: -v.y },
        }
    }

    pub(crate) fn is_flip(self) -> bool {
        matches!(self, Xform::Flip { .. })
    }
}

/// `::FlipLayer( aLayerId, aCopperLayersCount )` (`common/layer_id.cpp`): the layer on the other side of the board.
/// Front and back pairs swap; with four or more copper layers the inner ones mirror through the middle of the stack
/// (`In1` <-> `In2` on four layers, `In1` <-> `In4` and `In2` <-> `In3` on six); every other layer stays.
pub fn flip_layer(layer: &str, copper_layers: usize) -> String {
    const PAIRS: [(&str, &str); 7] = [("F.Cu", "B.Cu"), ("F.SilkS", "B.SilkS"), ("F.Adhes", "B.Adhes"), ("F.Mask", "B.Mask"), ("F.Paste", "B.Paste"), ("F.CrtYd", "B.CrtYd"), ("F.Fab", "B.Fab")];
    for (front, back) in PAIRS {
        if layer == front {
            return back.to_string();
        }
        if layer == back {
            return front.to_string();
        }
    }
    if copper_layers >= 4 {
        if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse::<i64>().ok()) {
            let inner_index = n - 1;
            let max_index = copper_layers as i64 - 3;
            let flipped = (max_index - inner_index).clamp(0, max_index);
            return format!("In{}.Cu", flipped + 1);
        }
    }
    layer.to_string()
}

/// `BOARD_ITEM::IsSideSpecific`: a layer that belongs to one side of the board, so text on it reads mirrored from the
/// back (`LSET::SideSpecificMask`: the front and back technical layers and every copper layer).
pub(crate) fn side_specific(layer: &str) -> bool {
    layer.starts_with("F.") || layer.starts_with("B.") || (layer.starts_with("In") && layer.ends_with(".Cu"))
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// The items a verb names, by kind, with groups already replaced by their members.
#[derive(Default)]
struct Targets {
    parts: BTreeSet<String>,
    tracks: BTreeSet<String>,
    vias: BTreeSet<String>,
    zones: BTreeSet<String>,
    shapes: BTreeSet<String>,
    texts: BTreeSet<String>,
    dimensions: BTreeSet<String>,
    /// The text boxes, images and points (`crate::pcb_items`).
    board_items: BTreeSet<String>,
    /// Tables (`PCB_TABLE`); a click selects a cell, but a move, a turn and a flip take its table (`FilterCollectorForTableCells`), which the caller sends.
    tables: BTreeSet<String>,
    /// Footprint fields on their own (`REF:Reference`): text a footprint carries, moved without it.
    fields: BTreeSet<String>,
    /// A footprint's own texts on their own (`REF~t<k>`, [`crate::fp_texts`]).
    fp_texts: BTreeSet<String>,
    /// Tuning patterns (`PCB_TUNING_PATTERN`, a `PCB_GENERATOR`): their tracks are in `tracks`, their origin, end and base lines move with them.
    patterns: BTreeSet<String>,
    /// Pads on their own (`REF.NUM`, `REF.NUM#k`): with "Allow free pads" the studio sends a pad without its footprint (UI-LEFT-3 R3-7).
    pads: BTreeSet<String>,
}

impl Targets {
    fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.tracks.is_empty() && self.vias.is_empty() && self.zones.is_empty() && self.shapes.is_empty() && self.texts.is_empty() && self.dimensions.is_empty() && self.board_items.is_empty() && self.tables.is_empty() && self.fields.is_empty() && self.fp_texts.is_empty() && self.patterns.is_empty() && self.pads.is_empty()
    }
}

impl Board<'_> {
    /// Files `id` under the collection it belongs to. `false` when nothing on the board has the id.
    fn file_item(&self, id: &str, out: &mut Targets) -> bool {
        if self.pose_of(id).is_some() {
            out.parts.insert(id.to_string());
            return true;
        }
        if let Some(rt) = &self.design.routing {
            if rt.tracks.iter().any(|t| t.id == id) {
                out.tracks.insert(id.to_string());
                return true;
            }
            if rt.vias.iter().any(|v| v.id == id) {
                out.vias.insert(id.to_string());
                return true;
            }
            if rt.zones.iter().any(|z| z.id == id) {
                out.zones.insert(id.to_string());
                return true;
            }
        }
        if self.resolve_field(id).is_some() {
            out.fields.insert(id.to_string());
            return true;
        }
        if self.resolve_fp_text(id).is_some() {
            out.fp_texts.insert(id.to_string());
            return true;
        }
        if self.pad_of(id).is_some() {
            out.pads.insert(id.to_string());
            return true;
        }
        let Some(dr) = &self.design.drawings else { return false };
        if dr.shapes.iter().any(|s| s.id() == id) {
            out.shapes.insert(id.to_string());
        } else if dr.texts.iter().any(|t| t.id == id) {
            out.texts.insert(id.to_string());
        } else if dr.dimensions.iter().any(|d| d.id == id) {
            out.dimensions.insert(id.to_string());
        } else if crate::pcb_items::has_board_item(dr, id) {
            out.board_items.insert(id.to_string());
            // A via stitching is a group (`PCB_GENERATOR` is a `PCB_GROUP`): `PCB_GROUP::Move`, `Rotate` and `Flip` take its vias along.
            if let Some(v) = dr.board_items.via_stitches.iter().find(|v| v.id == id) {
                let on_board = |m: &String| self.design.routing.as_ref().is_some_and(|rt| rt.vias.iter().any(|x| &x.id == m));
                out.vias.extend(v.member_ids.iter().filter(|m| on_board(m)).cloned());
            }
            // A microvia stack is one too (`PCB_VIA_STACK` is a `PCB_GENERATOR`): its vias and traces go with it.
            if let Some(s) = dr.board_items.via_stacks.iter().find(|s| s.id == id) {
                let rt = self.design.routing.as_ref();
                out.vias.extend(s.member_ids.iter().filter(|m| rt.is_some_and(|rt| rt.vias.iter().any(|x| &x.id == *m))).cloned());
                out.tracks.extend(s.member_ids.iter().filter(|m| rt.is_some_and(|rt| rt.tracks.iter().any(|x| &x.id == *m))).cloned());
            }
        } else if dr.tables.iter().any(|t| t.id == id) {
            out.tables.insert(id.to_string());
        } else if let Some(p) = dr.tuning_patterns.iter().find(|p| p.id == id) {
            // A tuning pattern is a group too (`PCB_TUNING_PATTERN` is a `PCB_GENERATOR` is a `PCB_GROUP`): `PCB_GROUP::Move`, `Rotate` and `Flip` take its tracks along,
            // and the pattern moves its origin, end and base lines (`PCB_TUNING_PATTERN::Move`, `Rotate`, `Flip` outside an edit)
            out.patterns.insert(id.to_string());
            let on_board = |m: &String| self.design.routing.as_ref().is_some_and(|rt| rt.tracks.iter().any(|t| &t.id == m));
            out.tracks.extend(p.member_ids.iter().filter(|m| on_board(m)).cloned());
        } else {
            return false;
        }
        true
    }

    /// Every item `ids` names. A group stands for the items below it, nested groups opened (`PCB_GROUP::Move`/`Rotate`/`Flip`
    /// hand the move to each member, and a member that is a group hands it on); an id that is not on the board refuses the whole
    /// command, so nothing is half applied.
    fn resolve_targets(&self, verb: &str, ids: &[String]) -> Result<Targets, Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_transform", verb, "no items given")]);
        }
        let mut out = Targets::default();
        for id in ids {
            let group = self.design.drawings.as_ref().filter(|d| d.is_group(id));
            if let Some(d) = group {
                // A member the board no longer has (a stale group) is skipped, as everywhere else groups are read.
                for m in d.group_leaves(id) {
                    self.file_item(&m, &mut out);
                }
            } else if !self.file_item(id, &mut out) {
                return Err(vec![CheckResult::fail("ops_unknown_item", id, "no placed part, track, via, zone, shape, text, text box, dimension or group with this id")]);
            }
        }
        if out.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_transform", verb, "none of the items has anything on the board to transform")]);
        }
        Ok(out)
    }

    /// `Cmd::MoveItems`.
    pub(crate) fn move_items(&mut self, ids: &[String], dx: Nm, dy: Nm) -> Result<(), Vec<CheckResult>> {
        self.transform_items("move_items", ids, Xform::Move { dx, dy })
    }

    /// `Cmd::RotateItems`.
    pub(crate) fn rotate_items(&mut self, ids: &[String], pivot: Point, angle_millideg: i64) -> Result<(), Vec<CheckResult>> {
        self.transform_items("rotate_items", ids, Xform::Rotate { pivot, angle: angle_millideg })
    }

    /// `Cmd::FlipItems`.
    pub(crate) fn flip_items(&mut self, ids: &[String], pivot: Point, direction: FlipDirection) -> Result<(), Vec<CheckResult>> {
        self.transform_items("flip_items", ids, Xform::Flip { pivot, dir: direction })
    }

    /// `Cmd::MirrorTracks`: `PCB_TRACK::Mirror` / `PCB_ARC::Mirror` -- the points mirrored across the axis through `pivot`, the layer kept.
    pub(crate) fn mirror_tracks(&mut self, ids: &[String], pivot: Point, direction: FlipDirection) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", ids.join(","), "the board has no routing")])?;
        if let Some(missing) = ids.iter().find(|id| !rt.tracks.iter().any(|t| &t.id == *id)) {
            return Err(vec![CheckResult::fail("ops_unknown_track", missing.as_str(), "no track with this id")]);
        }
        let x = Xform::Flip { pivot, dir: direction };
        for t in rt.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
            // `transform_track` with the flip, less its layer change.
            let layer = std::mem::take(&mut t.layer);
            transform_track(t, x, 2);
            t.layer = layer;
        }
        Ok(())
    }

    /// `Cmd::MirrorItems`: `EDIT_TOOL::Mirror` of the text boxes and tuning patterns among `ids`, about `pivot` (the other kinds have verbs of their own, which
    /// the studio's Mirror sends with this one in one batch). A text box mirrors as `PCB_TEXTBOX::Mirror`: its shape (`PCB_SHAPE::Mirror`, the layer kept) and
    /// its text angle (`ANGLE_180 - angle` left-right, `-angle` top-bottom), not its text or justification. A tuning pattern as `PCB_TUNING_PATTERN::Mirror`
    /// (`baseMirror`): its base lines, origin and end mirror and its initial side swaps (its tracks are its members, mirrored with `MirrorTracks`). A reference
    /// image is not mirrorable (`MirrorableItems` has no `PCB_REFERENCE_IMAGE_T`) and is refused with the other unknown ids.
    pub(crate) fn mirror_items(&mut self, ids: &[String], pivot: Point, direction: FlipDirection) -> Result<(), Vec<CheckResult>> {
        let x = Xform::Flip { pivot, dir: direction };
        // a pad on its own ("Allow free pads"): `mirrorPad` (UI-LEFT-3 R3-7)
        let pads: BTreeSet<String> = ids.iter().filter(|id| self.pad_of(id).is_some()).cloned().collect();
        self.transform_pads(&pads, x, true)?;
        let ids: Vec<String> = ids.iter().filter(|id| !pads.contains(*id)).cloned().collect();
        if ids.is_empty() {
            return Ok(());
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_item", ids.join(","), "the board has no drawings")])?;
        for id in &ids {
            if !dr.board_items.text_boxes.iter().any(|t| &t.id == id) && !dr.tuning_patterns.iter().any(|p| &p.id == id) {
                return Err(vec![CheckResult::fail("ops_unknown_item", id.as_str(), "no text box, tuning pattern or pad with this id")]);
            }
        }
        for t in dr.board_items.text_boxes.iter_mut().filter(|t| ids.contains(&t.id)) {
            t.start = x.point(t.start);
            t.end = x.point(t.end);
            for p in t.pts.iter_mut() {
                *p = x.point(*p);
            }
            t.angle = match direction {
                FlipDirection::LeftRight => norm360(180_000 - t.angle as i64),
                FlipDirection::TopBottom => norm360(-(t.angle as i64)),
            } as _;
        }
        for p in dr.tuning_patterns.iter_mut().filter(|p| ids.contains(&p.id)) {
            for q in p.base_line.iter_mut().chain(p.base_line_coupled.iter_mut()) {
                *q = x.point(*q);
            }
            match (p.base_line.first(), p.base_line.last()) {
                (Some(&first), Some(&last)) => {
                    p.origin = first;
                    p.end = last;
                }
                _ => {
                    p.origin = x.point(p.origin);
                    p.end = x.point(p.end);
                }
            }
            // `if( m_settings.m_initialSide == MEANDER_SIDE_RIGHT ) .. = LEFT; else .. = RIGHT;`
            p.initial_side = if p.initial_side == eda_model::tuning::PatternSide::Right { eda_model::tuning::PatternSide::Left } else { eda_model::tuning::PatternSide::Right };
        }
        Ok(())
    }

    /// `FOOTPRINT::Flip` hands the flip to each of its items (`PCB_SHAPE::Flip`, `PCB_TEXT::Flip`, `PCB_FIELD`'s): `SetLayer( GetBoard()->FlipLayer( GetLayer() ) )`,
    /// and `BOARD::FlipLayer` takes a user layer typed front or back to its pair (`m_layers[..].m_opposite`, `recalcOpposites`); a text that lands on such a
    /// layer toggles its mirror (`if( IsSideSpecific() ) SetMirrored( !IsMirrored() )`: `IsSideSpecific` is true on a layer of type front or back).
    ///
    /// The footprint's graphics and texts are kept per definition, the side's own layers in the top-side frame (`fp_art::layer_on_side` turns F. and B. over),
    /// a user layer as it is on the board; so the part turned over gets its own copy of the definition (`FootprintAssignment::definition`, as Edit Text &
    /// Graphics makes one) with those items on the paired layer, and its fields' layouts (`footprint_edits`, board layers) follow. Nothing happens on a board
    /// with no typed user layer pair, or to a footprint that draws on none.
    pub(crate) fn flip_footprint_user_layers(&mut self, reference: &str) {
        let setup = &self.model.board.layer_setup;
        let opp = setup.user_layer_opposites();
        if opp.is_empty() {
            return;
        }
        let typed = |l: &str| l.starts_with("User.") && matches!(setup.types.get(l), Some(eda_model::layers::LayerType::Front | eda_model::layers::LayerType::Back));
        let swap = |l: &mut String| match opp.get(l.as_str()) {
            Some(o) => {
                *l = o.clone();
                true
            }
            None => false,
        };
        // the fields' layouts are board layers (`flip_footprint_fields` has turned F. and B. over)
        if let Some(edit) = self.design.drawings.as_mut().and_then(|dr| dr.footprint_edits.iter_mut().find(|e| e.id == reference)) {
            let layouts = edit.reference.iter_mut().chain(edit.value.iter_mut()).chain(edit.fields.iter_mut().map(|f| &mut f.layout));
            for l in layouts {
                if swap(&mut l.layer) && typed(&l.layer) {
                    l.mirror = !l.mirror;
                }
            }
        }
        let Some(part) = self.model.part(reference) else { return };
        let Some(before) = self.unedited_footprint(part) else { return };
        let mut fp = before.clone();
        for s in fp.art.shapes.iter_mut() {
            let mut layer = s.shape.layer().to_string();
            if swap(&mut layer) {
                s.shape.set_layer(layer);
            }
            for l in s.layers.iter_mut() {
                swap(l);
            }
        }
        for t in fp.art.texts.iter_mut() {
            if swap(&mut t.layout.layer) && typed(&t.layout.layer) {
                t.layout.mirror = !t.layout.mirror;
            }
        }
        if fp != before {
            let name = self.design.footprint_assignment(reference).map(|a| a.footprint.clone()).unwrap_or_else(|| crate::fp_exchange::footprint_name_of(part));
            self.drawings_mut().set_footprint_assignment(eda_model::fp_assign::FootprintAssignment { reference: reference.to_string(), footprint: name, definition: Some(fp) });
        }
    }

    pub(crate) fn transform_items(&mut self, verb: &str, ids: &[String], x: Xform) -> Result<(), Vec<CheckResult>> {
        let targets = self.resolve_targets(verb, ids)?;
        // `PAD::Flip` of a free pad ("Allow free pads" keeps a selected pad as itself): the pad alone to the other side of the board (crate::free_pads)
        match x {
            Xform::Flip { pivot, dir } => self.flip_pads(&targets.pads, pivot, dir)?,
            _ => self.transform_pads(&targets.pads, x, false)?,
        }
        let copper = self.model.board.layers.len();
        let outer = (self.model.board.layers.first().cloned(), self.model.board.layers.last().cloned());

        // Footprints first: `set_pose` takes `&mut self`, and the collections below borrow the design.
        for r in &targets.parts {
            let fp = self.require_placed(r)?;
            let (at, rot, side, label) = part_pose(&fp, x);
            self.set_pose_exact(r, at, rot)?;
            let list = &mut self.design.placement.as_mut().expect("a Board always carries a placement section").footprints;
            let turned_over = list.iter().find(|f| &f.id == r).is_some_and(|f| f.side != side);
            if let Some(f) = list.iter_mut().find(|f| &f.id == r) {
                f.side = side;
                f.label = label;
            }
            // Its fields turn over with it (`FOOTPRINT::Flip`), and what it draws on a user layer typed front or back goes to that layer's pair.
            if turned_over {
                self.flip_footprint_fields(r, copper);
                self.flip_footprint_user_layers(r);
            }
        }
        // A field of a footprint that is itself being transformed goes with the footprint, not on its own.
        let own_fields: BTreeSet<String> = targets.fields.iter().filter(|f| !parse_field_id(f).is_some_and(|(r, _)| targets.parts.contains(r))).cloned().collect();
        self.transform_fields(&own_fields, x, copper)?;
        // A footprint's own text likewise (UI-LEFT P15).
        let own_texts: BTreeSet<String> = targets.fp_texts.iter().filter(|t| !crate::fp_texts::parse_fp_text_id(t).is_some_and(|(r, _)| targets.parts.contains(r))).cloned().collect();
        self.transform_fp_texts(&own_texts, x, copper)?;

        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut().filter(|t| targets.tracks.contains(&t.id)) {
                transform_track(t, x, copper);
            }
            for v in rt.vias.iter_mut().filter(|v| targets.vias.contains(&v.id)) {
                transform_via(v, x, copper, &outer);
            }
            for z in rt.zones.iter_mut().filter(|z| targets.zones.contains(&z.id)) {
                transform_zone(z, x, copper);
            }
        }
        // `BOARD_ITEM::GetMaxError()`: the board's arc approximation (Board Setup's "Maximum error"), not a constant
        let max_error = self.model.board.max_error_nm;
        if let Some(dr) = self.design.drawings.as_mut() {
            for s in dr.shapes.iter_mut().filter(|s| targets.shapes.contains(s.id())) {
                let radius = dr.board_items.corner_radius_of(s.id());
                if transform_rounded_shape(s, x, copper, radius, max_error) && radius > Nm::ZERO {
                    dr.board_items.set_corner_radius(s.id(), Nm::ZERO);
                }
                // a polygon's holes go with its outline (`SHAPE_POLY_SET::Move` / `Rotate` / `Mirror` of `m_poly`, UI-LEFT-2 PP10)
                dr.board_items.map_holes(s.id(), |p| x.point(p));
            }
            for t in dr.texts.iter_mut().filter(|t| targets.texts.contains(&t.id)) {
                transform_text(t, x, copper);
            }
            for d in dr.dimensions.iter_mut().filter(|d| targets.dimensions.contains(&d.id)) {
                transform_dimension(d, x, copper);
            }
            crate::pcb_items::transform_board_items(dr, &targets.board_items, x, copper);
            for t in dr.tables.iter_mut().filter(|t| targets.tables.contains(&t.id)) {
                super::pcb_tables::transform_table(t, x, copper);
            }
        }
        // The tuning patterns (`PCB_TUNING_PATTERN::Move` / `Rotate` / `Flip` outside an edit, its tracks moved above as its members):
        // - Move: `m_origin += aMoveVector; m_end += aMoveVector;` and `m_baseLine->Move( aMoveVector )`, the coupled base line too;
        // - Rotate: `PCB_GENERATOR::Rotate` (`RotatePoint( m_origin, .. )`), `RotatePoint( m_end, .. )`, the base lines rotated;
        // - Flip: `PCB_GENERATOR::Flip` mirrors the origin (`PCB_GENERATOR::baseMirror`) and sets the layer of the pattern and its tracks to the other side
        //   (`SetLayer( FlipLayer( GetLayer() ) )`, which `PCB_TUNING_PATTERN::SetLayer` hands to its tracks), then `PCB_GROUP::Flip` flips each track -- back to
        //   the layer it was on -- and `PCB_TUNING_PATTERN::baseMirror` mirrors the origin back, the base lines, puts the origin and the end on the base line's ends
        //   and swaps the side of the first meander. So a pattern turned over is mirrored where it is, on its own layer (`GetLayer()` is its first track's): what
        //   Mirror makes of it.
        if !targets.patterns.is_empty() {
            let members: BTreeSet<String> = self.design.drawings.as_ref().map(|dr| dr.tuning_patterns.iter().filter(|p| targets.patterns.contains(&p.id)).flat_map(|p| p.member_ids.iter().cloned()).collect()).unwrap_or_default();
            if x.is_flip() {
                // the tracks of a pattern go back to their layer (the flip above took them to the other side; `transform_track` has no generator to ask)
                if let Some(rt) = self.design.routing.as_mut() {
                    for t in rt.tracks.iter_mut().filter(|t| members.contains(&t.id)) {
                        t.layer = flip_layer(&t.layer, copper);
                    }
                }
            }
            if let Some(dr) = self.design.drawings.as_mut() {
                for p in dr.tuning_patterns.iter_mut().filter(|p| targets.patterns.contains(&p.id)) {
                    for q in p.base_line.iter_mut().chain(p.base_line_coupled.iter_mut()) {
                        *q = x.point(*q);
                    }
                    if let Xform::Flip { .. } = x {
                        match (p.base_line.first(), p.base_line.last()) {
                            (Some(&first), Some(&last)) => {
                                p.origin = first;
                                p.end = last;
                            }
                            _ => {
                                p.origin = x.point(p.origin);
                                p.end = x.point(p.end);
                            }
                        }
                        p.initial_side = if p.initial_side == eda_model::tuning::PatternSide::Right { eda_model::tuning::PatternSide::Left } else { eda_model::tuning::PatternSide::Right };
                    } else {
                        p.origin = x.point(p.origin);
                        p.end = x.point(p.end);
                    }
                }
            }
        }
        // `BOARD::FlipLayer` is `m_layers[..].m_opposite`: a user layer typed front or back flips to its pair (`recalcOpposites`), which `::FlipLayer`
        // above left where it was.
        if x.is_flip() {
            let setup = &self.model.board.layer_setup;
            let opp = setup.user_layer_opposites();
            // `BOARD_ITEM::IsSideSpecific` is also true on a layer whose type is front or back (`GetLayerType( m_layer )`), so a text or a text box turned
            // over onto one toggles its mirror as on F. and B. (`PCB_TEXT::Flip`, `PCB_TEXTBOX::Flip`: `if( IsSideSpecific() ) SetMirrored( !IsMirrored() )`,
            // after `SetLayer( FlipLayer( .. ) )`); `transform_text` saw only the layer names.
            let typed_side = |l: &str| l.starts_with("User.") && matches!(setup.types.get(l), Some(eda_model::layers::LayerType::Front | eda_model::layers::LayerType::Back));
            if let Some(dr) = self.design.drawings.as_mut() {
                let new_layer = |l: &str| opp.get(l).cloned().unwrap_or_else(|| l.to_string());
                for t in dr.texts.iter_mut().filter(|t| targets.texts.contains(&t.id) && typed_side(&new_layer(&t.layer))) {
                    t.mirror = !t.mirror;
                }
                for t in dr.board_items.text_boxes.iter_mut().filter(|t| targets.board_items.contains(&t.id) && typed_side(&new_layer(&t.layer))) {
                    t.mirror = !t.mirror;
                }
            }
            if !opp.is_empty() {
                let to = |l: &mut String| {
                    if let Some(o) = opp.get(l.as_str()) {
                        *l = o.clone();
                    }
                };
                if let Some(rt) = self.design.routing.as_mut() {
                    for z in rt.zones.iter_mut().filter(|z| targets.zones.contains(&z.id)) {
                        z.map_layers(|l| opp.get(l).cloned().unwrap_or_else(|| l.to_string()));
                    }
                }
                if let Some(dr) = self.design.drawings.as_mut() {
                    for s in dr.shapes.iter_mut().filter(|s| targets.shapes.contains(s.id())) {
                        let mut l = s.layer().to_string();
                        to(&mut l);
                        s.set_layer(l);
                    }
                    dr.texts.iter_mut().filter(|t| targets.texts.contains(&t.id)).for_each(|t| to(&mut t.layer));
                    dr.dimensions.iter_mut().filter(|d| targets.dimensions.contains(&d.id)).for_each(|d| to(&mut d.layer));
                    dr.tables.iter_mut().filter(|t| targets.tables.contains(&t.id)).for_each(|t| to(&mut t.layer));
                    dr.barcodes.iter_mut().filter(|b| targets.board_items.contains(&b.id)).for_each(|b| to(&mut b.layer));
                    dr.board_items.text_boxes.iter_mut().filter(|b| targets.board_items.contains(&b.id)).for_each(|b| to(&mut b.layer));
                    dr.board_items.images.iter_mut().filter(|b| targets.board_items.contains(&b.id)).for_each(|b| to(&mut b.layer));
                    dr.board_items.points.iter_mut().filter(|b| targets.board_items.contains(&b.id)).for_each(|b| to(&mut b.layer));
                }
            }
        }
        if !targets.tracks.is_empty() || !targets.vias.is_empty() {
            self.sort_routing();
        }
        Ok(())
    }
}

impl Board<'_> {
    /// The pad a pad id (`REF.NUM`, `REF.NUM#k`) names on a placed footprint: its reference, number and `nth`, and the pad as the footprint's edits make it.
    pub(crate) fn pad_of(&self, id: &str) -> Option<(String, String, u32, eda_model::Pad)> {
        let (reference, number, nth) = eda_model::fp_edit::parse_pad_id(id)?;
        self.pose_of(&reference)?;
        let lib = self.pads_footprint(self.model.part(&reference)?)?;
        let edits = self.design.footprint_edit(&reference).map(|e| e.pads.clone()).unwrap_or_default();
        let fp = eda_model::fp_edit::patched_footprint(&lib, &edits);
        let pad = fp.pads.get(eda_model::fp_edit::pad_index(&fp, &number, nth)?)?.clone();
        Some((reference, number, nth, pad))
    }

    /// Pads moved, turned or mirrored on their own, as "Allow free pads" lets the board editor do (`PCBNEW_SETTINGS::m_AllowFreePads`: "pads can be moved with
    /// respect to the rest of the footprint"; the edit tools' `FilterCollectorForFreePads` then keeps a pad as itself; UI-LEFT-3 R3-7): `PAD::Move`, `PAD::Rotate`
    /// (`RotatePoint( m_pos, aRotCentre, aAngle )`, `SetOrientation( GetOrientation() + aAngle )`), and with `mirror` `EDIT_TOOL`'s `mirrorPad` -- the position
    /// mirrored, the orientation negated (`SetOrientation( -GetOrientation() )`), the offset, a trapezoid's delta and a custom pad's basic shapes mirrored in the
    /// pad's own frame (`FlipPrimitives`). The pad keeps what it was moved to as a pad edit of its footprint (`PadEdit::at`, `rot`, `offset` ..), in the
    /// footprint's frame, so the footprint moved later takes it along. (A pad of a bottom-side footprint is held in the top view's frame: its offset and shapes
    /// mirror there.)
    pub(crate) fn transform_pads(&mut self, pads: &BTreeSet<String>, x: Xform, mirror: bool) -> Result<(), Vec<CheckResult>> {
        for id in pads {
            let Some((reference, number, nth, pad)) = self.pad_of(id) else {
                return Err(vec![CheckResult::fail("ops_unknown_item", id.as_str(), "no pad with this id")]);
            };
            let fp = self.require_placed(&reference)?;
            let bottom = fp.side == Side::Bottom;
            // the pad's orientation on the board, clockwise millidegrees (a bottom-side footprint's pads turn the other way, `footprint::placed_pads`)
            let sense: i64 = if bottom { -1 } else { 1 };
            let orientation = fp.rot as i64 + sense * pad.rot as i64;
            let at = x.point(eda_model::footprint::to_board(&fp, pad.at));
            let orientation = match x {
                Xform::Move { .. } => orientation,
                Xform::Rotate { angle, .. } => orientation + angle,
                Xform::Flip { .. } => -orientation,
            };
            let mut edit = self.design.footprint_edit(&reference).and_then(|e| e.pad(&number, nth)).cloned().unwrap_or_else(|| PadEdit::none(&number, nth));
            edit.at = Some(local_point(&fp, at));
            edit.rot = Some(norm360(sense * (orientation - fp.rot as i64)) as Millideg);
            if let (true, Xform::Flip { dir, .. }) = (mirror, x) {
                let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
                let m = |p: Point| match dir {
                    FlipDirection::LeftRight => Point { x: -p.x, y: p.y },
                    FlipDirection::TopBottom => Point { x: p.x, y: -p.y },
                };
                if let Some((ox, oy)) = pad.ext.offset {
                    edit.offset = Some(m(Point { x: ox, y: oy }));
                }
                if let Some((dx, dy)) = pad.ext.trapezoid_delta {
                    let d = m(Point { x: dx, y: dy });
                    edit.trapezoid_delta = Some((d.x, d.y));
                }
                if let Some(mut custom) = pad.ext.custom.clone() {
                    for prim in &mut custom.primitives {
                        crate::fp_items::mirror_shape(&mut prim.shape, origin, dir);
                    }
                    edit.custom = Some(custom);
                }
            }
            self.edit_slot(&reference).set_pad(edit);
            self.tidy_edit(&reference);
        }
        Ok(())
    }
}

/// The inverse of [`eda_model::footprint::to_board`]: a board point in the footprint's own frame (the top view's: a bottom-side footprint mirrors x).
pub(crate) fn local_point(fp: &FootprintInstance, p: Point) -> Point {
    let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (dx, dy) = ((p.x - fp.at.x).as_f64(), (p.y - fp.at.y).as_f64());
    let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
    let mirror = if fp.side == Side::Bottom { -1.0 } else { 1.0 };
    Point { x: Nm::kiround(lx * mirror), y: Nm::kiround(ly) }
}

// ----------------------------------------------------------------- footprints

/// `FOOTPRINT::Move`/`::Rotate`/`::Flip` for this model's pose: the position follows the transform, and the
/// orientation and side follow the item.
///
/// A flip is KiCad's: `FOOTPRINT::Flip` mirrors across a horizontal axis (Y changes) and turns the part over, and for
/// `LEFT_RIGHT` then rotates it half a turn about the pivot, which in the end mirrors X and leaves Y alone. This
/// model draws a bottom-side part as its top-side pads with x negated, then turned by `rot` (`footprint::to_board`),
/// so the mirror of a part turned by `rot` is the other side's part turned by `-rot` (left-right) or `180 - rot`
/// (top-bottom). The refdes label sits on the mirrored side of the courtyard.
pub(crate) fn part_pose(fp: &FootprintInstance, x: Xform) -> (Point, u32, Side, LabelSide) {
    let at = x.point(fp.at);
    match x {
        Xform::Move { .. } => (at, fp.rot, fp.side, fp.label),
        Xform::Rotate { angle, .. } => (at, norm360(fp.rot as i64 + angle) as u32, fp.side, fp.label),
        Xform::Flip { dir, .. } => {
            let rot = match dir {
                FlipDirection::LeftRight => norm360(-(fp.rot as i64)),
                FlipDirection::TopBottom => norm360(180_000 - fp.rot as i64),
            };
            let side = if fp.side == Side::Top { Side::Bottom } else { Side::Top };
            let label = match (dir, fp.label) {
                (FlipDirection::LeftRight, LabelSide::Left) => LabelSide::Right,
                (FlipDirection::LeftRight, LabelSide::Right) => LabelSide::Left,
                (FlipDirection::TopBottom, LabelSide::Above) => LabelSide::Below,
                (FlipDirection::TopBottom, LabelSide::Below) => LabelSide::Above,
                (_, l) => l,
            };
            (at, rot as u32, side, label)
        }
    }
}

// --------------------------------------------------------------------- copper

/// A true arc is rebuilt from its three points rather than carried sample by sample: its polyline is the arc's
/// tessellation, and a rotated or mirrored copy of the old samples can be a micrometre off the new arc's own
/// (`Track::arc` would then refuse it, and the writer would turn the arc into 32 straight segments).
pub(crate) fn transform_track(t: &mut Track, x: Xform, copper: usize) {
    if let Some((start, mid, end)) = t.arc() {
        let (s, m, e) = (x.point(start), x.point(mid), x.point(end));
        t.pts = tessellate_arc(s, m, e, TRACK_ARC_SEGMENTS);
        t.arc_mid_offset = Some(Point { x: m.x - s.x, y: m.y - s.y });
    } else {
        for p in t.pts.iter_mut() {
            *p = x.point(*p);
        }
        if let Some(off) = t.arc_mid_offset {
            t.arc_mid_offset = Some(x.vector(off));
        }
    }
    if x.is_flip() {
        t.layer = flip_layer(&t.layer, copper);
    }
}

/// `PCB_VIA::Flip`: the position mirrors; a blind or buried via changes its layer pair, a through via has none to
/// change (`GetViaType() != VIATYPE::THROUGH`). A through via spans the outer layers.
pub(crate) fn transform_via(v: &mut Via, x: Xform, copper: usize, outer: &(Option<String>, Option<String>)) {
    v.at = x.point(v.at);
    if x.is_flip() {
        let is_through = match outer {
            (Some(first), Some(last)) => (v.from_layer == *first && v.to_layer == *last) || (v.from_layer == *last && v.to_layer == *first),
            _ => false,
        };
        if !is_through {
            v.from_layer = flip_layer(&v.from_layer, copper);
            v.to_layer = flip_layer(&v.to_layer, copper);
        }
    }
}

/// `ZONE::Move`/`::Rotate`/`::Flip` (`Mirror` of the outline, then the layer flips). The fill is derived from the
/// outline every time it is wanted, so there is nothing else to carry.
/// A polygon's holes ([`eda_model::pcb_items::ShapeStyle::holes`]) moved, turned, flipped or mirrored with it (UI-LEFT-2 PP10).
pub(crate) fn transform_holes(style: &mut eda_model::pcb_items::ShapeStyle, x: Xform) {
    for p in style.holes.iter_mut().flatten() {
        *p = x.point(*p);
    }
}

pub(crate) fn transform_zone(z: &mut Zone, x: Xform, copper: usize) {
    // The holes go with the outline (`m_Poly->Move`/`Rotate`/`Mirror` move the whole polygon set).
    for p in z.outline.iter_mut().chain(z.holes.iter_mut().flatten()) {
        *p = x.point(*p);
    }
    if x.is_flip() {
        // `ZONE::Flip`: every layer of the zone's set goes to the other side -- and so do its fills, which a zone on both sides tells from a mirror only by
        // this count (`Zone::flip_parity`, UI-LEFT-2 PP10).
        z.map_layers(|l| flip_layer(l, copper));
        z.flip_parity = !z.flip_parity;
    }
}

// ------------------------------------------------------------------- graphics

/// `EDA_SHAPE::Normalize` for a rectangle: start at the top left, end at the bottom right.
fn norm_rect(start: &mut Point, end: &mut Point) {
    let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
    let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
    *start = Point { x: x0, y: y0 };
    *end = Point { x: x1, y: y1 };
}

/// `EDA_SHAPE::rotate`/`::flip` and `PCB_SHAPE::Flip`. A rectangle turned by anything but a quarter turn becomes a
/// polygon (`m_shape = SHAPE_T::POLY`); every other rectangle is normalised after a turn or a flip (`Normalize`),
/// start at the top left. A mirrored arc swaps its ends, as in source, so it still runs counter-clockwise from start
/// to end; the three points describe the same arc either way.
pub(crate) fn transform_shape(s: &mut Shape, x: Xform, copper: usize) {
    transform_rounded_shape(s, x, copper, Nm::ZERO, eda_model::outline::MAX_ERROR_NM);
}

/// [`transform_shape`] of a shape whose rectangle has corners of `corner_radius` (`BoardItems::corner_radius_of`): a turn by anything but a
/// quarter makes it the polygon of its *rounded* rectangle, `ROUNDRECT( SHAPE_RECT( .. ), m_cornerRadius ).TransformToPolygon( .., getMaxError() )`
/// turned (`EDA_SHAPE::rotate`), at `max_error` (`getMaxError()`, the board's). True when the rectangle became a polygon, so the caller drops the radius (a
/// polygon has none). KiCad's polygon keeps its four corners as true arcs (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC, .. )`, written as `(arc ..)` in the
/// `gr_poly`); this model's polygon has no arcs, so they are their points.
pub(crate) fn transform_rounded_shape(s: &mut Shape, x: Xform, copper: usize, corner_radius: Nm, max_error: Nm) -> bool {
    let flip = x.is_flip();
    let moving = matches!(x, Xform::Move { .. });
    let mut as_polygon: Option<Shape> = None;
    match s {
        Shape::Segment { start, end, .. } | Shape::Circle { center: start, end, .. } => {
            *start = x.point(*start);
            *end = x.point(*end);
        }
        Shape::Arc { start, mid, end, .. } => {
            *start = x.point(*start);
            *mid = x.point(*mid);
            *end = x.point(*end);
            if flip {
                std::mem::swap(start, end);
            }
        }
        Shape::Rect { id, layer, stroke_width, filled, start, end } => {
            if let Xform::Rotate { angle, .. } = x {
                if angle.rem_euclid(90_000) != 0 {
                    let corners = eda_model::outline::rounded_rect_ring(*start, *end, corner_radius, max_error.as_f64());
                    as_polygon = Some(Shape::Polygon { id: std::mem::take(id), layer: layer.clone(), stroke_width: *stroke_width, filled: *filled, pts: corners.iter().map(|c| x.point(*c)).collect() });
                }
            }
            if as_polygon.is_none() {
                *start = x.point(*start);
                *end = x.point(*end);
                if !moving {
                    norm_rect(start, end);
                }
            }
        }
        Shape::Polygon { pts, .. } => {
            for p in pts.iter_mut() {
                *p = x.point(*p);
            }
        }
        Shape::Bezier { start, c1, c2, end, .. } => {
            *start = x.point(*start);
            *c1 = x.point(*c1);
            *c2 = x.point(*c2);
            *end = x.point(*end);
        }
    }
    let became_polygon = as_polygon.is_some();
    if let Some(poly) = as_polygon {
        *s = poly;
    }
    if flip {
        let flipped = flip_layer(s.layer(), copper);
        s.set_layer(flipped);
    }
    became_polygon
}

/// `PCB_TEXT::Move`/`::Rotate`/`::Flip`. A text's angle is KiCad's (counter-clockwise on the screen), so a clockwise
/// turn takes from it. Flipped left-right the angle negates; flipped top-bottom it becomes `180 - angle`. A text on a
/// side-specific layer reads mirrored from the back, and turning it over toggles that.
pub(crate) fn transform_text(t: &mut Text, x: Xform, copper: usize) {
    t.at = x.point(t.at);
    match x {
        Xform::Move { .. } => {}
        Xform::Rotate { angle, .. } => t.angle = norm360(t.angle as i64 - angle) as Millideg,
        Xform::Flip { dir, .. } => {
            t.angle = match dir {
                FlipDirection::LeftRight => norm360(-(t.angle as i64)),
                FlipDirection::TopBottom => norm360(180_000 - t.angle as i64),
            } as Millideg;
            t.layer = flip_layer(&t.layer, copper);
            if side_specific(&t.layer) {
                t.mirror = !t.mirror;
            }
        }
    }
}

// ----------------------------------------------------------------- dimensions

/// `PCB_DIMENSION_BASE::Move`/`::Rotate`/`::Flip` and the kind-specific overrides: `PCB_DIM_ORTHOGONAL::Rotate`
/// turns the crossbar to the nearest quarter turn, `PCB_DIM_ALIGNED::Mirror` and `PCB_DIM_ORTHOGONAL::Mirror` move
/// the side the crossbar sits on with the mirror. The text and the lines are worked out again from the two feature
/// points, so only those, the kind's own numbers, the text angle and a radial's or a leader's kept text position are carried.
pub(crate) fn transform_dimension(d: &mut Dimension, x: Xform, copper: usize) {
    match x {
        Xform::Move { dx, dy } => eda_connectivity::dimension::translate_dimension(d, dx, dy),
        Xform::Rotate { pivot, angle } => {
            ortho_for_rotation(d, angle);
            eda_connectivity::dimension::rotate_dimension(d, pivot, angle);
        }
        Xform::Flip { dir, .. } => {
            // `PCB_DIMENSION_BASE::Mirror`: "MIRROR( newPos, axis_pos, aFlipDirection ); SetTextPos( newPos );" first. Only a radial's and a leader's text
            // position is kept (`text_pos`); the others' is worked out again from the points.
            if let Some(t) = &mut d.text_pos {
                *t = x.point(*t);
            }
            d.start = x.point(d.start);
            d.end = x.point(d.end);
            d.text_angle = norm360(-(d.text_angle as i64)) as Millideg;
            match &mut d.kind {
                DimensionKind::Aligned { height } => *height = -*height,
                // "Only reverse the height if the height is aligned with the flip".
                DimensionKind::Orthogonal { height, horizontal } => {
                    if (*horizontal && dir == FlipDirection::TopBottom) || (!*horizontal && dir == FlipDirection::LeftRight) {
                        *height = -*height;
                    }
                }
                DimensionKind::Radial { .. } | DimensionKind::Leader | DimensionKind::Center => {}
            }
            d.flip_text_mirror();
            d.layer = flip_layer(&d.layer, copper);
        }
    }
}

/// `PCB_DIM_ORTHOGONAL::Rotate`'s adjustment before the points turn: the crossbar is axis-locked, so a turn of about
/// 90 degrees makes a horizontal one vertical and the other way round, with the height changing sign where the side it
/// is on swaps; about 180 degrees only the height changes sign. The angle is KiCad's there (counter-clockwise), ours
/// is clockwise.
fn ortho_for_rotation(d: &mut Dimension, angle_cw: i64) {
    let DimensionKind::Orthogonal { height, horizontal } = &mut d.kind else { return };
    // `angle.Normalize180()`: -179.999 to 180 degrees, in KiCad's sense.
    let mut a = norm360(-angle_cw);
    if a > 180_000 {
        a -= 360_000;
    }
    if a > 45_000 && a <= 135_000 {
        if *horizontal {
            *horizontal = false;
        } else {
            *horizontal = true;
            *height = -*height;
        }
    } else if a < -45_000 && a >= -135_000 {
        if *horizontal {
            *horizontal = false;
            *height = -*height;
        } else {
            *horizontal = true;
        }
    } else if a > 135_000 || a < -135_000 {
        *height = -*height;
    }
}
