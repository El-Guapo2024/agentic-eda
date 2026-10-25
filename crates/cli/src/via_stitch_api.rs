//! KiCad 10's via stitching through a board directory: a `set_via_stitch` that leaves out where the vias go has them placed here, from the zone fills kicad-cli made last
//! (`.kicad/zone_fills.json`, which the design does not hold): `eda_drc::via_stitch::plan`, the port of `PCB_VIA_STITCH::Update`'s placement. The command that runs is the
//! one with the vias in it, so the step is one undo step whoever sent it, and an agent can send the vias itself.

use eda_model::units::Nm;
use std::path::Path;

use eda_model::ir::Design;
use eda_model::ConstraintModel;
use eda_ops::pcb_items::{PcbItemCmd, PlannedVia};
use eda_ops::Cmd;

/// The plan of one stitching on the board as it is.
pub(crate) fn plan(dir: &Path, design: &Design, model: &ConstraintModel, stitch: &eda_model::via_stitch::ViaStitch) -> eda_drc::via_stitch::StitchPlan {
    let fills = crate::fill_api::cached(dir).map(|k| k.fills_nm(design)).unwrap_or_default();
    // `GetCurrentViaSize` / `GetCurrentViaDrill`: the board's default via (the via size list's first entry is the net class's).
    let inputs = eda_drc::via_stitch::PlanInputs { design, model, fills: &fills, current_via_size_nm: model.board.via_diameter, current_via_drill_nm: model.board.via_drill };
    eda_drc::via_stitch::plan(&inputs, stitch)
}

/// `cmd` with the vias of every `set_via_stitch` that has none placed (inside a batch too); `None` when there is nothing to place.
///
/// A Move of a via stitching (`PCB_VIA_STITCH::Move` translates the outline and its vias and marks it dirty; the move tool's `genUpdateEdit` / `genFinishEdit` make the
/// vias again, the grid anchored where the moved vias now are) is followed, in the same step, by the regeneration of the stitching where the move left it: `snap` and
/// `spacing` make the board the move is tried on. A Rotate or a Flip (`EDIT_TOOL::Rotate`, `Flip`) turns the vias with the outline and only marks it dirty: no regeneration.
pub(crate) fn with_planned_vias(cmd: &Cmd, dir: &Path, design: &Design, model: &ConstraintModel, snap: Nm, spacing: Nm) -> Option<Cmd> {
    let stitch_ids = |ids: &[String]| -> Vec<String> { design.drawings.iter().flat_map(|d| d.board_items.via_stitches.iter()).filter(|v| ids.contains(&v.id)).map(|v| v.id.clone()).collect() };
    match cmd {
        Cmd::PcbItem(PcbItemCmd::SetViaStitch { id, stitch, vias: None }) => {
            let p = plan(dir, design, model, stitch);
            Some(Cmd::PcbItem(PcbItemCmd::SetViaStitch { id: id.clone(), stitch: p.stitch, vias: Some(p.vias.into_iter().map(|(cell, at)| PlannedVia { cell, at }).collect()) }))
        }
        Cmd::MoveItems { ids, .. } => {
            let moved = stitch_ids(ids);
            if moved.is_empty() {
                return None;
            }
            let mut b = eda_ops::Board::new(design.clone(), model, snap, spacing);
            b.apply(cmd).ok()?;
            let after = b.design();
            let mut cmds = vec![cmd.clone()];
            for v in after.drawings.iter().flat_map(|d| d.board_items.via_stitches.iter()).filter(|v| moved.contains(&v.id)) {
                let p = plan(dir, after, model, v);
                cmds.push(Cmd::PcbItem(PcbItemCmd::SetViaStitch { id: Some(v.id.clone()), stitch: p.stitch, vias: Some(p.vias.into_iter().map(|(cell, at)| PlannedVia { cell, at }).collect()) }));
            }
            Some(Cmd::Batch { cmds })
        }
        Cmd::Batch { cmds } => {
            let done: Vec<Option<Cmd>> = cmds.iter().map(|c| with_planned_vias(c, dir, design, model, snap, spacing)).collect();
            if done.iter().all(Option::is_none) {
                return None;
            }
            Some(Cmd::Batch { cmds: cmds.iter().zip(done).map(|(c, d)| d.unwrap_or_else(|| c.clone())).collect() })
        }
        _ => None,
    }
}
