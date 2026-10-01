//! eda-gates — placement and routing geometry gates.
//!
//! The former schematic-section gates (`check_schematic` and its 23
//! individual checks) moved to `eda_kicad::check_erc` -- see that crate's
//! `erc_style` module -- so the ERC port is the one engine a schematic is
//! judged by, per the project's "no duplicate tools" rule. This crate now
//! covers only placement and routing (`pcb.rs`) and the partial-build
//! variant of the placement gates (`partial.rs`).

pub mod partial;
pub mod pcb;
pub use partial::check_placement_partial;
pub use pcb::{check_placement, check_placement_locality, check_routing, check_routing_quality};
