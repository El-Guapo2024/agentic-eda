//! Record what was decided, and what it was worth.
//!
//! Every build makes one decision per part -- eighty-five on L4 -- each
//! with an exact before-and-after failure count, and until now all of it
//! went to stderr and vanished. That is free supervision thrown away on
//! every run: no human labelling, no reward model, just the gates
//! answering precisely what a move was worth.
//!
//! A line here is one step: the board as the decision layer saw it, the
//! command chosen, and the change in failures. That triple is what a
//! large action model trains on, and it is also how a chooser is judged
//! -- Opus playing the decision layer produces episodes, and the model
//! learns from the episodes rather than from Opus.
//!
//! Written as JSONL so a run appends without rewriting, and a training
//! set is `cat`.
//!
//! Off unless `EDA_EPISODE` names a file. Recording is never allowed to
//! fail a build: a placer that dies because a log could not be written
//! has traded the board for the note about the board.

use crate::{Board, Cmd};
use eda_model::ConstraintModel;
use serde_json::json;
use std::io::Write;

/// Where episodes are being written, if anywhere.
pub fn sink() -> Option<String> {
    std::env::var("EDA_EPISODE").ok().filter(|s| !s.is_empty())
}

/// Record one decision and its outcome.
///
/// `before` is the failure count the decision was made against, `after`
/// the count once it was applied; `after - before` is the reward, and
/// negative is good. The view is taken *before* the command, because
/// that is the state the decision was actually made from -- logging the
/// result instead would teach a model to predict the past.
pub fn record(
    board_before: &Board,
    model: &ConstraintModel,
    cmd: &Cmd,
    before: usize,
    after: usize,
    step: usize,
) {
    let Some(path) = sink() else { return };
    // A view that cannot be built is not recorded as a partial one: a
    // training line describing a board that does not exist is worse
    // than a missing line.
    let Ok(state) = crate::view::view(board_before, model) else { return };
    let line = json!({
        "step": step,
        "state": state,
        "action": cmd,
        "failures_before": before,
        "failures_after": after,
        // Negative is an improvement, which is the sign convention a
        // reward wants.
        "reward": after as i64 - before as i64,
    });
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    let _ = writeln!(f, "{line}");
}
