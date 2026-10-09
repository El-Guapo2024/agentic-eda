//! Keeping `placement.outline` the summary of the board's Edge.Cuts shapes when they are the outline
//! ([`eda_model::ir::DrawingsSection::outline_is_shapes`]).
//!
//! The shapes are what the writer emits and what every consumer that wants the real outline reads; `placement.outline` is the ring that stands for
//! them in the places that only know a polygon -- the placement gate's bounding box, the canvas fit, an interchange file. Any command that adds, edits,
//! moves or deletes a shape on Edge.Cuts changes the outline, so [`Board::apply`](crate::Board::apply) refreshes the summary after it.
//! A board whose outline is its polygon has nothing to refresh.

use eda_model::ir::{Design, Shape};
use eda_model::outline::{edge_cuts_shapes, outline_is_shapes};

/// The Edge.Cuts shapes before a command, or `None` for a board with no summary to keep.
pub(crate) fn snapshot(design: &Design) -> Option<Vec<Shape>> {
    outline_is_shapes(design).then(|| edge_cuts_shapes(design))
}

/// Re-derive the summary when the command changed the Edge.Cuts shapes.
pub(crate) fn refresh_if_changed(design: &mut Design, before: Option<Vec<Shape>>) {
    if let Some(before) = before {
        if edge_cuts_shapes(design) != before {
            eda_drc::outline::refresh_outline_summary(design);
        }
    }
}
