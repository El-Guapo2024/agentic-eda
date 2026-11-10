//! Carried placements: Duplicate and Paste, and the drop that ends them, are one undo step; Escape leaves nothing behind.
//!
//! `EDIT_TOOL::Duplicate` and `PCB_CONTROL::Paste` (`pcbnew/tools/edit_tool.cpp`, `pcb_control.cpp`) add the new items to a `BOARD_COMMIT` and hand that commit
//! to the Move tool (`doMoveSelection( aEvent, &commit, true )`; `placeBoardItems` runs `PCB_ACTIONS::move` with the commit): nothing is on the undo list
//! until the drop pushes it (`commit.Push( _( "Duplicate" ) )`, `commit.Push( _( "Paste" ) )`), so the new items and where they were dropped are one step, and a
//! cancelled move calls `commit.Revert()`: the items are gone, the undo list and the redo list are as they were.
//!
//! Here the new items are on the board the moment the command lands -- the canvas draws them from the board and carries them through a transform
//! (`kicad-port/pcbCarry.ts`) -- so the step is *held open* instead:
//!
//! * **begin** (`POST /api/cmd` with `"carry": "begin"`): the command runs and is saved as usual, but the design before it is kept aside as the *pending* step
//!   (`.history/pending/`) instead of going on the undo stack, and the redo stack is left alone;
//! * **hold** (`"carry": "hold"`): Move Individually's drop of an item that is not the last (`doMoveSelection` picks the next item up into the same
//!   `aCommit`, pushed once): the command runs inside the pending step, which stays open -- or, with none open, begins it as **begin** does;
//! * **commit** (`"carry": "commit"`, or `POST /api/carry` `{"action": "commit"}` for a drop that changes nothing): the drop's command runs, then the pending
//!   design goes on the undo stack as the one step both belong to, and the redo stack is cleared;
//! * **cancel** (`POST /api/carry` `{"action": "cancel"}`): the pending design comes back and is saved -- no undo entry, no redo entry;
//! * **anything else** that touches the history (another command, Undo, Redo, an edit of `design.json` by hand) first *settles* the pending step as a step of
//!   its own: the duplicate stands, as it would had the Move tool been left some other way.
//!
//! Under the studio's Strict switch (UI-LEFT P7, the studio's own; KiCad's Move tool never refuses a drop) the command that begins a carry is not judged -- the
//! items are on the cursor, not where they will be put -- and the drop is judged against the design the step began from (`board::step_carrying`). A drop the
//! gates refuse ends the carry with nothing left behind: the pending design comes back, as **cancel** has it, and the refusal is the answer. A drop that changes
//! nothing (**commit** through `/api/carry`) is judged the same way, so a strict carry never lands unjudged. The strictness is the drop's own (`"strict"` in the
//! request), or, when the request does not say, the one the carry began with.
//!
//! A pending step survives a restart of the server the same way: it is on disk, and the next command settles it.

use super::{clear_dir, design_path, fail, history_root, keep_outside_history, lock, log_activity, outside_history_of, push_snapshot_tagged, redo_dir, save, undo_dir};
use eda_model::CheckResult;
use std::path::{Path, PathBuf};

/// Where a command stands in a carried placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Carry {
    /// The command makes the items that are then carried (Duplicate, Paste).
    Begin,
    /// The command is the drop: it ends the carry as one step with the command that began it.
    Commit,
    /// The command joins the step held open (or begins one): Move Individually's drops before the last.
    Hold,
}

impl Carry {
    pub(crate) fn parse(text: &str) -> Option<Carry> {
        match text {
            "begin" => Some(Carry::Begin),
            "commit" => Some(Carry::Commit),
            "hold" => Some(Carry::Hold),
            _ => None,
        }
    }
}

fn pending_dir(dir: &Path) -> PathBuf {
    history_root(dir).join("pending")
}

/// The step held open: the design before the command that began it, the undo scope tag, who made it, and whether it began under Strict.
struct Pending {
    design: eda_model::ir::Design,
    tag: String,
    by: String,
    strict: bool,
}

fn read_pending(dir: &Path) -> Option<Pending> {
    let root = pending_dir(dir);
    let design = std::fs::read_to_string(root.join("design.json")).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    let meta = std::fs::read_to_string(root.join("meta")).unwrap_or_default();
    let mut lines = meta.lines();
    let tag = lines.next().filter(|t| !t.is_empty()).unwrap_or("pcb").to_string();
    let by = lines.next().unwrap_or("ui").to_string();
    let strict = lines.next() == Some("strict");
    Some(Pending { design, tag, by, strict })
}

/// The design the step held open began from (before the command that began it), if a step is held open: what the drop of a carried placement is judged
/// against (`board::step_carrying`).
pub(crate) fn pending_design(dir: &Path) -> Option<eda_model::ir::Design> {
    read_pending(dir).map(|p| p.design)
}

fn drop_pending(dir: &Path) {
    let _ = std::fs::remove_dir_all(pending_dir(dir));
}

/// Holds `before` open as the pending step, under the history tag `tag` (`board::snapshot_tag`), begun under Strict or not.
fn hold(dir: &Path, before: &eda_model::ir::Design, tag: &str, by: &str, strict: bool) {
    let root = pending_dir(dir);
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::write(root.join("design.json"), serde_json::to_string(before).unwrap_or_default());
    let _ = std::fs::write(root.join("meta"), format!("{tag}\n{by}\n{}\n", if strict { "strict" } else { "off" }));
}

/// Puts the pending step on the undo stack as a step of its own and clears the redo stack (a new edit erases old redos). Called with the lock held.
pub(super) fn settle_locked(dir: &Path) -> bool {
    let Some(p) = read_pending(dir) else { return false };
    let _ = push_snapshot_tagged(&undo_dir(dir), &p.design, &p.tag, &p.by);
    clear_dir(&redo_dir(dir));
    drop_pending(dir);
    true
}

/// What a command that succeeded leaves in the history: for a normal command the design before it on the undo stack (and the redo stack cleared); for the
/// start of a carry that design kept aside (with `strict`, the Strict switch it began under); for the drop the pending design in its place, so the carry and
/// the drop are one step.
pub(super) fn record(dir: &Path, carry: Option<Carry>, before: &eda_model::ir::Design, tag: &str, by: &str, strict: bool) {
    match carry {
        Some(Carry::Begin) => hold(dir, before, tag, by, strict),
        // a step already held open keeps the design from before its first command
        Some(Carry::Hold) if read_pending(dir).is_some() => {}
        Some(Carry::Hold) => hold(dir, before, tag, by, strict),
        Some(Carry::Commit) if settle_locked(dir) => {}
        _ => {
            let _ = push_snapshot_tagged(&undo_dir(dir), before, tag, by);
            clear_dir(&redo_dir(dir));
        }
    }
}

/// `POST /api/carry` `{"action": "commit"}`: the drop changed nothing (a click where the items already were), so the carry just ends: the pending step is
/// a step of its own. Under Strict (`strict`, or the carry's own when `None`) the board as the carry left it is judged first against the design the step
/// began from; one the gates refuse is taken back, as a cancel, and the refusal is the answer.
pub(crate) fn commit(dir: &Path, strict: Option<bool>) -> Result<String, Vec<CheckResult>> {
    let _lock = lock(dir);
    let Some(p) = read_pending(dir) else {
        return Err(fail("board_no_carry", "carry", "nothing is being carried"));
    };
    if strict.unwrap_or(p.strict) {
        if let Err(e) = super::judge_against(dir, p.design.clone()) {
            refuse_locked(dir, &p.design, &e);
            return Err(e);
        }
    }
    settle_locked(dir);
    Ok("the carried placement stands".into())
}

/// A drop the gates refused (`board::step_carrying`, a strict drop that makes the board worse; [`commit`]): the carry ends with nothing left behind -- the
/// design from before the command that began it comes back, as [`cancel`] brings it, with no undo entry and no redo entry. Called with the lock held.
pub(super) fn refuse_locked(dir: &Path, before: &eda_model::ir::Design, why: &[CheckResult]) {
    let _ = save(dir, &revert_to(dir, before.clone()));
    drop_pending(dir);
    log_activity(dir, "ui", "cancel carried placement", true, &format!("the drop was refused, so nothing of the carried placement is left: {}", super::reasons(why)));
}

/// `commit.Revert()`: `before` as it comes back, with what no commit holds kept from the design as it is (`board::keep_outside_history`) -- a Paste that
/// numbered and was then cancelled leaves its numbers in the reference-designator tracker, as KiCad 25ec2335's `SCH_EDITOR_CONTROL::Paste` (the tracker is the
/// project's, `annotatedSymbols[path].SetRefDesTracker( schematicSettings.m_refDesTracker )`, and the revert restores items only). PARITY-sch §82.
fn revert_to(dir: &Path, before: eda_model::ir::Design) -> eda_model::ir::Design {
    match std::fs::read_to_string(design_path(dir)).ok().and_then(|s| serde_json::from_str::<eda_model::ir::Design>(&s).ok()) {
        Some(now) => keep_outside_history(outside_history_of(&now), before),
        None => before,
    }
}

/// [`refuse_locked`] for the step held open, if one is.
pub(super) fn refuse_pending_locked(dir: &Path, why: &[CheckResult]) -> bool {
    match read_pending(dir) {
        Some(p) => {
            refuse_locked(dir, &p.design, why);
            true
        }
        None => false,
    }
}

/// `POST /api/carry` `{"action": "cancel"}` (Escape while the items are carried): `commit.Revert()`. The design from before the carried command comes back
/// and is saved; there is no undo entry for it and no redo entry either, and the redo stack is as the carried command found it.
pub(crate) fn cancel(dir: &Path, by: &str) -> Result<String, Vec<CheckResult>> {
    let _lock = lock(dir);
    let Some(p) = read_pending(dir) else {
        return Err(fail("board_no_carry", "carry", "nothing is being carried"));
    };
    save(dir, &revert_to(dir, p.design))?;
    drop_pending(dir);
    let msg = "cancel: the carried items are gone".to_string();
    log_activity(dir, by, "cancel carried placement", true, &msg);
    Ok(msg)
}

/// Whether a step is being held open.
#[cfg(test)]
pub(crate) fn is_carrying(dir: &Path) -> bool {
    pending_dir(dir).join("design.json").exists()
}

#[cfg(test)]
#[path = "carry_tests.rs"]
mod tests;
