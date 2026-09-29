//! Board-space pad geometry — a thin re-export of the model's own
//! transform so the router, gates and placer can never disagree about
//! where a "REF.PIN" physically sits.

pub use eda_model::footprint::{placed_pads as pad_positions, PlacedPad as PadGeom};
