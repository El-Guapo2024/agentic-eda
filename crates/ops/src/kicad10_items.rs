//! The verbs of KiCad 10's board items that the 8303b2ad source predates (ported from KiCad 25ec2335, the commit of the box's kicad-cli): grid items (`PCB_GRID_ITEM`), via
//! stitching (`PCB_VIA_STITCH`) and drill charts and maps (`PCB_DRILL_CHART`, `PCB_DRILL_MAP`). Line endings are a shape's style: [`crate::pcb_items::PcbItemCmd::SetShapeStyle`].
//!
//! They are verbs of [`crate::pcb_items::PcbItemCmd`] (`{"op": "pcb_item", "verb": "add_grid_item", ..}`); Move, Rotate, Flip, Lock and Delete-by-selection go through the
//! verbs that take any item, which call [`transform_grid_item`] for these kinds.

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
