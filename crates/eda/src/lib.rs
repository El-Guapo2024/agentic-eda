//! Single-entry facade: the whole toolkit behind one library.
//!
//! ```ignore
//! let model  = eda::import_zen(path)?;
//! let issues = eda::lint(&model);
//! let design = eda::derive_schematic(&model, &opts)?;
//! let checks = eda::check_schematic(&design, &model);
//! let svg    = eda::render_schematic(&design, &model)?;
//! let placed = eda::place(&design, &model, &PlaceOptions::default())?;
//! let routed = eda::route(&placed, &model, &model.board, seed)?;
//! ```

pub use eda_engine::{derive_schematic, EngineOptions};
pub use eda_gates::check_schematic;
pub use eda_intent::{import_zen, import_zen_cli};
pub use eda_intent::lint::lint;
pub use eda_render::render_schematic;
pub use eda_kicad::{export_kicad_pcb, export_kicad_sch, ExportMeta};
pub use eda_interchange::{from_bookshelf_pl, to_bookshelf, to_circuit_json, Bookshelf};
pub use eda_router::{route, route_partial, RouteRules};
pub use eda_place::{hpwl, place, PlaceOptions};
pub use eda_gates::{check_placement, check_routing};
pub use eda_logger::{now_rfc3339, EscalationPolicy, Event, RunLog, Tier};

/// Core data types.
pub mod model {
    pub use eda_model::*;
}

/// Everything most callers need, in one import.
pub mod prelude {
    pub use eda_model::ir::{Design, Stage};
    pub use eda_model::{CheckResult, CheckStatus, ConstraintModel};
    pub use super::{check_schematic, derive_schematic, import_zen, import_zen_cli, lint, render_schematic, EngineOptions};
    pub use super::{export_kicad_pcb, export_kicad_sch, route, to_circuit_json, RouteRules};
    pub use super::{check_placement, check_routing, hpwl, place, PlaceOptions};
    pub use super::{EscalationPolicy, Event, RunLog, Tier};
}

#[cfg(test)]
mod tests {
    #[test]
    fn facade_links_everything() {
        // Compile-time proof every re-export resolves.
        let _ = super::EngineOptions::default();
    }
}
