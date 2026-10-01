//! Single-entry facade: the whole toolkit behind one library.
//!
//! ```ignore
//! let model  = eda::import_zen(path)?;
//! let issues = eda::lint(&model);
//! let design = eda::derive_schematic(&model, &opts)?;
//! let checks = eda::check_erc(&design, &model);
//! let svg    = eda::render_schematic(&design, &model)?;
//! let placed = eda::place(&design, &model, &PlaceOptions::default())?;
//! let routed = eda::route(&placed, &model, &model.board, seed)?;
//! ```

pub use eda_engine::{derive_schematic, EngineOptions};
pub use eda_intent::{import_zen, import_zen_cli};
pub use eda_intent::lint::lint;
pub use eda_render::render_schematic;
pub use eda_kicad::{export_kicad_pcb, export_kicad_pro, export_kicad_sch, import_kicad_pcb, import_kicad_sch, merge_custom_rules, merge_project_design_rules, merge_project_net_classes, merge_project_rule_severities, ExportMeta, ImportNotes};
pub use eda_kicad::{default_footprint_library_root, resolve_library_footprints};
pub use eda_kicad::{check_erc, check_erc_excluding, default_symbol_library_root, resolve_library_symbols, Exclusions};
pub use eda_interchange::{from_bookshelf_pl, to_bookshelf, to_circuit_json, Bookshelf};
pub use eda_grid::{check_pours, preflight, RouteRules};
pub use eda_place::{hpwl, place, Anneal, PlaceOptions, Placer};
pub use eda_cypress::{place_with_cypress, Cypress, CypressOptions};
pub use eda_model::board::{fit_outline, trim_empty_edges};
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
    pub use super::{derive_schematic, import_zen, import_zen_cli, lint, render_schematic, EngineOptions};
    pub use super::{check_erc, export_kicad_pcb, export_kicad_sch, route, to_circuit_json, RouteRules};
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


/// Passes the router runs before it gives up on what is left.
const FREEROUTE_MAX_PASSES: i32 = 20;

/// Route a placed design: the gated contract. The routed design, or every
/// failure -- a net left apart, a pour cut off, a precondition unmet.
/// The router is deterministic; `seed` is taken for the callers' sake.
pub fn route(
    design: &eda_model::ir::Design,
    model: &eda_model::ConstraintModel,
    rules: &RouteRules,
    _seed: u64,
) -> Result<eda_model::ir::Design, Vec<eda_model::CheckResult>> {
    match route_partial(design, model, rules) {
        (Some(d), fails) if fails.is_empty() => Ok(d),
        (_, fails) => Err(fails),
    }
}

/// Route with the FreeRouting port: its batch passes over the board the
/// design makes, the routes written back. A net the passes leave apart
/// fails, named; a pour that no longer reaches every pad of its net fails
/// too. On failure, whatever was routed comes back with the failures, for
/// review; `None` where a precondition failed before routing started.
pub fn route_partial(
    design: &eda_model::ir::Design,
    model: &eda_model::ConstraintModel,
    rules: &RouteRules,
) -> (Option<eda_model::ir::Design>, Vec<eda_model::CheckResult>) {
    use eda_model::CheckResult;
    match eda_freeroute::design::route_design(design, model, rules, FREEROUTE_MAX_PASSES, i32::try_from(rules.tuning.optimizer_passes).unwrap_or(i32::MAX)) {
        Err(e) => (None, vec![CheckResult::fail("route_precondition", "design", e)]),
        Ok(routed) => {
            let passes = routed.passes.len();
            let fails = routed
                .unrouted
                .iter()
                .map(|net| {
                    CheckResult::fail("route_net_unrouted", net, format!("the FreeRouting port left the net's pins apart after {passes} passes"))
                        .with_detail(serde_json::json!({ "net": net, "passes": passes }))
                })
                .collect();
            let mut out = design.clone();
            out.routing = Some(routed.routing);
            // The pour's own honesty: FreeRouting lets other nets cross a
            // plane, and a plane cut into islands leaves pads floating.
            let mut fails: Vec<CheckResult> = fails;
            fails.extend(eda_grid::check_pours(&out, model, rules));
            (Some(out), fails)
        }
    }
}

/// Route, then check that the router and the geometry gate agree about
/// what they just produced.
///
/// The router reasons in its own shapes; the gates measure the copper
/// written back. They are two independent models of the same physics, and
/// they can drift: a grid router this pipeline once had reported zero
/// contested cells on an L4 board the clearance gate found 251 real
/// overlaps on. Both models were internally consistent and both were wrong
/// about each other, so neither could notice.
///
/// That matters more here than in an ordinary tool. These gates are the
/// reward signal: a router that reports success on a board with 251
/// violations is an environment that lies, and agents optimise against
/// whatever it says. A disagreement is therefore a hard failure of the
/// *engine*, named as such, and never a routing failure to be retried
/// under another seed -- the board may well be fine; the tooling is not.
pub fn route_checked(
    design: &eda_model::ir::Design,
    model: &eda_model::ConstraintModel,
    rules: &RouteRules,
    seed: u64,
) -> (Option<eda_model::ir::Design>, Vec<eda_model::CheckResult>) {
    use eda_model::{CheckResult, CheckStatus};
    let _ = seed;
    let (out, mut fails) = route_partial(design, model, rules);
    let router_claims_clean = fails.is_empty();
    let Some(routed) = out else { return (None, fails) };
    if !router_claims_clean {
        return (Some(routed), fails);
    }
    // Only the checks that describe copper the router itself placed. A
    // quality or style gate failing is a verdict on the board; a clearance
    // or short failing here is a verdict on the router's own model.
    const COPPER: &[&str] = &["routing_clearance", "routing_short", "routing_track_width", "routing_between_smd_pads"];
    let geometric: Vec<CheckResult> = eda_gates::check_routing(&routed, model)
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail && COPPER.contains(&c.check.as_str()))
        .collect();
    if !geometric.is_empty() {
        let mut by_check: std::collections::BTreeMap<&str, usize> = Default::default();
        for c in &geometric {
            *by_check.entry(c.check.as_str()).or_default() += 1;
        }
        let summary = by_check.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ");
        let first = geometric.first().map(|c| format!("{} @ {}: {}", c.check, c.location.clone().unwrap_or_default(), c.hint.clone().unwrap_or_default())).unwrap_or_default();
        fails.push(
            CheckResult::fail(
                "engine_router_gate_disagreement",
                "routing",
                format!("the router reported a clean board and the geometry gate found {} copper violation(s) on it ({summary}); first: {first}", geometric.len()),
            )
            .with_detail(serde_json::json!({
                "violations": geometric.len(),
                "by_check": by_check.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<String, usize>>(),
                "suggest": "engine bug, not a board problem: the router's grid model and the geometry gate disagree. Do not retry under another seed -- find the model that is wrong.",
            })),
        );
        fails.extend(geometric);
    }
    (Some(routed), fails)
}
