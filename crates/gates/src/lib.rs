//! eda-gates — placement and routing gates.
//!
//! Two tiers, by cost:
//!
//! * **In-process gates** ([`check_placement`], [`check_routing`],
//!   [`check_placement_partial`], ...): our own exact-geometry judges plus
//!   `eda-lint`'s placement-quality checks. Microseconds to milliseconds,
//!   so the constructive placer runs them on every step and the studio on
//!   every refresh.
//! * **KiCad's gates** ([`kicad`]): courtyard overlap and copper-to-edge
//!   clearance are KiCad design rules, so they come from one kicad-cli DRC
//!   run, filtered by type. Seconds, so only the judges call them (`eda
//!   board check`, `--strict`, the pipeline's stage gates).
//!
//! Schematic readability checks live in `eda-lint`; electrical rules are
//! kicad-cli's ERC.

pub mod kicad;
pub mod partial;
pub mod pcb;
pub use partial::check_placement_partial;
pub use pcb::{check_placement, check_placement_locality, check_routing, check_routing_quality};
