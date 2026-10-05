//! eda-lint -- our own fast checks, the ones KiCad does not have.
//!
//! * [`placement`]: placement quality (proximity rules, decoupling
//!   distance, edge connectors, board use, net compactness, crossing
//!   stubs, refdes labels over neighbours).
//! * [`routing`]: net-class track-width conformance.
//! * [`schematic`]: schematic readability (grid, wire length and overlap,
//!   label placement, sheet density).
//! * [`fab`]: fab readiness (part numbers on everything placed).
//!
//! These are never named DRC or ERC and never hold a copy of a KiCad rule:
//! clearance, courtyards, edge clearance, pin conflicts, plots and exports
//! all belong to kicad-cli (docs/ARCHITECTURE.md, "Engines"). Everything
//! here is cheap enough to run on every step of the constructive placer
//! and on every studio refresh, so none of it spawns a process.

pub mod fab;
pub mod finding;
pub mod placement;
pub mod routing;
pub mod schematic;

use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};

pub use finding::{counts, Check, Finding, FixHint, Item, Severity};

/// Every positioned PCB lint finding: placement quality, then net-class
/// track width.
pub fn check_pcb(design: &Design, model: &ConstraintModel) -> Vec<Finding> {
    let mut out = placement::check(design, model);
    out.extend(routing::check(design, model));
    out
}

/// The schematic readability checks, one [`CheckResult`] per finding plus a
/// `Pass` for each check that found nothing.
pub fn check_schematic(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    schematic::check_style(design, model)
}
