//! A board text's effective shape as KiCad's DRC sees it, in the stroke font or an outline font: moved to [`eda_model::text_shape`] (UI-LEFT-3 C3-3) so the
//! DRC, connectivity and model crates lay texts out the same way; re-exported here for the callers that name it `stroke_text`.

pub use eda_model::text_shape::*;
