//! The verbs of KiCad 10's board items that the 8303b2ad source predates (ported from KiCad 25ec2335, the commit of the box's kicad-cli): grid items (`PCB_GRID_ITEM`), via
//! stitching (`PCB_VIA_STITCH`) and drill charts and maps (`PCB_DRILL_CHART`, `PCB_DRILL_MAP`). Line endings are a shape's style: [`crate::pcb_items::PcbItemCmd::SetShapeStyle`].
//!
//! They are verbs of [`crate::pcb_items::PcbItemCmd`] (`{"op": "pcb_item", "verb": "add_grid_item", ..}`); Move, Rotate, Flip, Lock and Delete-by-selection go through the
//! verbs that take any item, which call [`transform_grid_item`] for these kinds.

use eda_model::units::Nm;
use super::pcb_transform::{FlipDirection, Xform};
use super::Board;
use eda_model::grid_item::{normalize_deg, GridItem};
use eda_model::CheckResult;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `PCB_GRID_ITEM::Move` / `::Rotate` / `::Flip`: the centre goes where the transform puts it; a turn adds to the orientation (`Xform::Rotate`'s angle is clockwise on the screen,
/// KiCad's counter-clockwise, so it is taken off); a left-right flip makes the orientation 180 degrees less it, a top-bottom one its negative. A grid has no layer to flip.
pub(crate) fn transform_grid_item(g: &mut GridItem, x: Xform) {
    g.at = x.point(g.at);
    match x {
        Xform::Move { .. } => {}
        Xform::Rotate { angle, .. } => g.angle = normalize_deg(g.angle - angle as f64 / 1000.0),
        Xform::Flip { dir: FlipDirection::LeftRight, .. } => g.angle = normalize_deg(180.0 - g.angle),
        Xform::Flip { dir: FlipDirection::TopBottom, .. } => g.angle = normalize_deg(-g.angle),
    }
}

impl<'a> Board<'a> {
    /// What a grid item must be: a finite orientation and sweep (the setters keep everything else in range).
    fn check_grid_item(&self, what: &str, g: &GridItem) -> Result<(), Vec<CheckResult>> {
        if !(g.angle.is_finite() && g.phi_extent_deg.is_finite() && g.phi_spacing_deg.is_finite()) {
            return Err(fail("ops_bad_grid_item", what, "the angles of a grid item are numbers"));
        }
        Ok(())
    }

    pub(crate) fn add_grid_item(&mut self, g: GridItem) -> Result<(), Vec<CheckResult>> {
        self.check_grid_item("grid_item", &g)?;
        let g = GridItem { id: String::new(), ..g.normalized() };
        let dr = self.drawings_mut();
        dr.board_items.grid_items.push(g);
        dr.assign_missing_ids();
        Ok(())
    }

    pub(crate) fn edit_grid_item(&mut self, id: &str, g: GridItem) -> Result<(), Vec<CheckResult>> {
        self.check_grid_item(id, &g)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_grid_item", id, "the board has no drawings yet"))?;
        let slot = dr.board_items.grid_items.iter_mut().find(|x| x.id == id).ok_or_else(|| fail("ops_unknown_grid_item", id, "no grid item with this id"))?;
        *slot = GridItem { id: id.to_string(), ..g.normalized() };
        Ok(())
    }

    pub(crate) fn delete_grid_item(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_grid_item", id, "the board has no drawings yet"))?;
        let before = dr.board_items.grid_items.len();
        dr.board_items.grid_items.retain(|g| g.id != id);
        if dr.board_items.grid_items.len() == before {
            return Err(fail("ops_unknown_grid_item", id, "no grid item with this id"));
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------------------------------- via stitching

use super::pcb_items::PlannedVia;
use eda_model::ir::{Point, Via};
use eda_model::via_stitch::ViaStitch;
use std::collections::BTreeMap;

impl<'a> Board<'a> {
    /// `ShowPropertiesDialog`'s `EditStart`, `Update`, `EditFinish` (and the generator a Draw Via Stitch Area makes): the stitching takes `stitch`'s settings, and its vias go
    /// where `vias` says, churning as few as `Update` does -- a member via on a cell that is still wanted stays (moved there if it was nudged off it, made again if its size,
    /// drill, layers or net no longer match the template), the others go, and a cell no member is on gets a new via, a copy of the template on the stitch net, `free`.
    pub(crate) fn set_via_stitch(&mut self, id: Option<&str>, stitch: ViaStitch, vias: Option<&[PlannedVia]>) -> Result<(), Vec<CheckResult>> {
        let what = id.unwrap_or("via_stitch");
        let vias = vias.ok_or_else(|| fail("ops_via_stitch_unplanned", what, "the vias of the stitching were not placed (the board directory places them from the zone fills)"))?;
        if !stitch.net.is_empty() {
            self.known_net(&stitch.net)?;
        }
        if stitch.outline.len() < 3 {
            return Err(fail("ops_bad_via_stitch", what, "a via stitching needs an outline of three corners or more"));
        }
        if stitch.via.size_nm <= Nm::ZERO || stitch.via.drill_nm <= Nm::ZERO || stitch.via.drill_nm >= stitch.via.size_nm {
            return Err(fail("ops_bad_via_stitch", what, "Via hole size must be smaller than via diameter"));
        }
        self.known_layer(&stitch.via.from_layer)?;
        self.known_layer(&stitch.via.to_layer)?;
        let members: Vec<String> = match id {
            Some(id) => {
                let dr = self.design.drawings.as_ref().ok_or_else(|| fail("ops_unknown_via_stitch", id, "the board has no drawings yet"))?;
                dr.board_items.via_stitches.iter().find(|v| v.id == id).ok_or_else(|| fail("ops_unknown_via_stitch", id, "no via stitching with this id"))?.member_ids.clone()
            }
            None => Vec::new(),
        };
        let pitch = stitch.pitch_nm;
        let desired: BTreeMap<(i64, i64), Point> = vias.iter().map(|p| (p.cell, p.at)).collect();
        // "Determine existing vias we can keep"
        let existing: Vec<Via> = self.design.routing.iter().flat_map(|rt| rt.vias.iter().filter(|v| members.contains(&v.id)).cloned()).collect();
        let mut kept: BTreeMap<(i64, i64), Via> = BTreeMap::new();
        let mut removed: Vec<String> = Vec::new();
        for v in existing {
            let cell = eda_drc::via_stitch::cell_for_position(&stitch, pitch.raw_shim(), (v.at.x.raw_shim(), v.at.y.raw_shim()));
            if desired.contains_key(&cell) && !kept.contains_key(&cell) {
                kept.insert(cell, v);
            } else {
                removed.push(v.id.clone());
            }
        }
        // `matchesRef`: the template's size, drill, layers and net (the position aside). A new stitch net is no mismatch: `PCB_VIA_STITCH::SetNetCode` gave every member the
        // net before `Update` (the dialog's `SetNetCode( newNetCode )`), so a kept via keeps its id and takes the net.
        let t = &stitch.via;
        let matches = |v: &Via| v.diameter == t.size_nm && v.drill == t.drill_nm && v.from_layer == t.from_layer && v.to_layer == t.to_layer;
        let mut keep_ids: Vec<String> = Vec::new();
        let mut moves: Vec<(String, Point)> = Vec::new();
        let mut adds: Vec<Point> = Vec::new();
        for (cell, at) in &desired {
            match kept.get(cell) {
                Some(v) if matches(v) => {
                    keep_ids.push(v.id.clone());
                    if v.at != *at {
                        // "Just nudge the via"
                        moves.push((v.id.clone(), *at));
                    }
                }
                Some(v) => {
                    removed.push(v.id.clone());
                    adds.push(*at);
                }
                None => adds.push(*at),
            }
        }
        // The vias.
        let rt = self.routing_mut();
        rt.vias.retain(|v| !removed.contains(&v.id));
        for (vid, at) in &moves {
            if let Some(v) = rt.vias.iter_mut().find(|v| &v.id == vid) {
                v.at = *at;
            }
        }
        for v in rt.vias.iter_mut().filter(|v| keep_ids.contains(&v.id)) {
            v.net = stitch.net.clone();
        }
        let before: std::collections::BTreeSet<String> = rt.vias.iter().map(|v| v.id.clone()).collect();
        for at in &adds {
            rt.vias.push(Via { id: String::new(), net: stitch.net.clone(), at: *at, drill: t.drill_nm, diameter: t.size_nm, from_layer: t.from_layer.clone(), to_layer: t.to_layer.clone() });
        }
        rt.assign_missing_ids();
        let new_ids: Vec<String> = rt.vias.iter().filter(|v| !before.contains(&v.id)).map(|v| v.id.clone()).collect();
        self.sort_routing();
        // "Vias must be marked free to avoid connectivity stealing them away"
        let dr = self.drawings_mut();
        dr.via_extras.retain(|x| !removed.contains(&x.id));
        for vid in &new_ids {
            dr.via_extras.push(eda_model::via_extra::ViaExtra { id: vid.clone(), kind: Default::default(), tokens: vec!["(free yes)".into()] });
        }
        let mut member_ids: Vec<String> = keep_ids.into_iter().chain(new_ids).collect();
        member_ids.sort();
        match id {
            Some(id) => {
                let slot = dr.board_items.via_stitches.iter_mut().find(|v| v.id == id).expect("found above");
                *slot = ViaStitch { id: id.to_string(), member_ids, ..stitch };
            }
            None => {
                dr.board_items.via_stitches.push(ViaStitch { id: String::new(), member_ids, ..stitch });
                dr.assign_missing_ids();
            }
        }
        Ok(())
    }

    /// `PCB_VIA_STITCH::Remove`: every via the stitching made, then the stitching.
    pub(crate) fn delete_via_stitch(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_via_stitch", id, "the board has no drawings yet"))?;
        let pos = dr.board_items.via_stitches.iter().position(|v| v.id == id).ok_or_else(|| fail("ops_unknown_via_stitch", id, "no via stitching with this id"))?;
        let gone = dr.board_items.via_stitches.remove(pos);
        dr.via_extras.retain(|x| !gone.member_ids.contains(&x.id));
        if let Some(rt) = self.design.routing.as_mut() {
            rt.vias.retain(|v| !gone.member_ids.contains(&v.id));
        }
        Ok(())
    }
}

/// `PCB_GENERATOR_POLY::Move`, `Rotate` and `Flip`: the outline and the origin (`m_origin`) moved, turned or mirrored. The vias go with them as a group's members do
/// (`crate::pcb_transform`); a Move is followed by the regeneration in the same step (the board directory's `with_planned_vias`), a turn and a flip only mark the stitching
/// dirty, so its vias stay where the turn put them until the next fill or edit.
pub(crate) fn transform_via_stitch(v: &mut ViaStitch, x: Xform) {
    for p in v.outline.iter_mut() {
        *p = x.point(*p);
    }
    v.origin = x.point(v.origin);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- drill maps

use eda_model::drill_chart::{is_documentation_layer, DrillMap};

impl<'a> Board<'a> {
    /// `PCB_CONTROL::PlaceDrillMap`: "Two maps on one layer would overdraw each other's marks", so a layer with a map already, or one that is no documentation layer, is refused.
    pub(crate) fn place_drill_map(&mut self, layer: &str) -> Result<(), Vec<CheckResult>> {
        if !is_documentation_layer(layer) {
            return Err(fail("ops_bad_drill_map", layer, "Every documentation layer already has a drill map."));
        }
        let size = crate::drill_chart::profile_of(&self.design).size_nm;
        let dr = self.drawings_mut();
        if dr.board_items.drill_maps.iter().any(|m| m.layer == layer) {
            return Err(fail("ops_bad_drill_map", layer, "Every documentation layer already has a drill map."));
        }
        dr.board_items.drill_maps.push(DrillMap::new(layer, size));
        dr.assign_missing_ids();
        Ok(())
    }

    pub(crate) fn edit_drill_map(&mut self, id: &str, m: DrillMap) -> Result<(), Vec<CheckResult>> {
        if !is_documentation_layer(&m.layer) {
            return Err(fail("ops_bad_drill_map", id, "a drill map is on a documentation layer"));
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_drill_map", id, "the board has no drawings yet"))?;
        let slot = dr.board_items.drill_maps.iter_mut().find(|x| x.id == id).ok_or_else(|| fail("ops_unknown_drill_map", id, "no drill map with this id"))?;
        // `SetSymbolSize`: at least one.
        *slot = DrillMap { id: id.to_string(), size_nm: m.size_nm.max(Nm::new(1)), ..m };
        Ok(())
    }

    pub(crate) fn delete_drill_map(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_drill_map", id, "the board has no drawings yet"))?;
        let before = dr.board_items.drill_maps.len();
        dr.board_items.drill_maps.retain(|m| m.id != id);
        if dr.board_items.drill_maps.len() == before {
            return Err(fail("ops_unknown_drill_map", id, "no drill map with this id"));
        }
        Ok(())
    }
}

/// `PCB_DRILL_MAP::Move` (the offset moves), `::Rotate` ("there is no geometry here to turn") and `::Flip` (the layer flips: "The side the map documents is still the side it
/// belongs on").
pub(crate) fn transform_drill_map(m: &mut DrillMap, x: Xform) {
    match x {
        Xform::Move { dx, dy } => m.offset = eda_model::ir::Point { x: m.offset.x + dx, y: m.offset.y + dy },
        // The flip's layer is `GetFlippedLayer`'s (`transform_board_items` gives it).
        Xform::Rotate { .. } | Xform::Flip { .. } => {}
    }
}
