//! Keeping `placement.outline` the summary of the board's Edge.Cuts shapes when they are the outline
//! ([`eda_model::ir::DrawingsSection::outline_is_shapes`]).
//!
//! The shapes are what the writer emits and what every consumer that wants the real outline reads; `placement.outline` is the ring that stands for
//! them in the places that only know a polygon -- the placement gate's bounding box, the canvas fit, an interchange file. Any command that adds, edits,
//! moves or deletes a shape on Edge.Cuts changes the outline, and so does a command that moves, turns or flips a footprint with Edge.Cuts of its own or changes a
//! rectangle's corner radius, so [`Board::apply`](crate::Board::apply) refreshes the summary after it (`BuildBoardPolygonOutlines` with the footprint pass).
//! A board whose outline is its polygon has nothing to refresh.

use eda_model::ir::{Design, Shape};
use eda_model::outline::{edge_cuts_outline_shapes, outline_is_shapes};
use eda_model::ConstraintModel;

/// Everything the outline is built from: the board's Edge.Cuts (a rounded rectangle as its polygon) and the footprints' own, on the board.
fn edges(design: &Design, model: &ConstraintModel) -> Vec<Shape> {
    let mut all = edge_cuts_outline_shapes(design);
    all.extend(eda_drc::outline::footprint_edge_cuts_shapes(design, model));
    all
}

/// The Edge.Cuts before a command, or `None` for a board with no summary to keep.
pub(crate) fn snapshot(design: &Design, model: &ConstraintModel) -> Option<Vec<Shape>> {
    outline_is_shapes(design).then(|| edges(design, model))
}

/// Re-derive the summary when the command changed the Edge.Cuts.
pub(crate) fn refresh_if_changed(design: &mut Design, model: &ConstraintModel, before: Option<Vec<Shape>>) {
    if let Some(before) = before {
        if edges(design, model) != before {
            eda_drc::outline::refresh_outline_summary(design, Some(model));
        }
    }
}
