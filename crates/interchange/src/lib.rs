//! Rust port of the tscircuit Circuit JSON interchange format (writer),
//! against our own IR (`eda_model::ir::Design`) and `ConstraintModel` — no
//! Node dependency. Unlocks circuit-to-svg style viewers.
//!
//! There is deliberately no Specctra DSN/SES bridge here: routing is done
//! by `eda-router`, natively, and nothing else.

pub mod bookshelf;
pub mod circuit_json;

pub use bookshelf::{from_bookshelf_pl, from_bookshelf_pl_fixed, to_bookshelf, to_bookshelf_fixed, Bookshelf, EDGE_PIN_GAP_UM};
pub use circuit_json::to_circuit_json;
