//! Intended as this workspace's single design-rule authority: a Rust port
//! of KiCad's PCB design-rule checker (`pcbnew/drc/`), following its actual
//! algorithms and error-type strings rather than reinventing simplified
//! equivalents, *plus* the placement-quality checks that have no KiCad
//! equivalent (proximity, decoupling, board use, net compactness, stub
//! crossings, edge-connector placement, refdes clearance), ported in as
//! providers of their own -- `eda_gates::pcb` still owns its own copies of
//! these today.
//!
//! **Integration status** (see the task report's gates-mapping table and
//! migration section for the full story): a compatibility shim making
//! `eda_gates::check_placement`/`check_routing` call through to this crate
//! was built and passed its own and `eda_gates`'s unit tests, but the
//! *workspace* test suite caught a real problem: `eda_gates::
//! check_placement_partial` (in `crates/gates/src/partial.rs`) re-runs
//! `check_placement` on a narrowed, partially-placed model at every step of
//! the constructive ("build") placer and its repair pass, using the
//! *count* of failures as a search signal. Swapping in this crate's
//! geometry -- identical to the old code for any *given, fixed* design,
//! verified directly -- still perturbed that search finely enough to change
//! which of two candidate layouts two of eleven `examples/*.yaml` boards
//! converged to, tripping a pre-existing zero-tolerance stub-crossing
//! check on the new one. That is a search-stability question about
//! `crates/ops`'s iterative placer, not a correctness defect in the ported
//! checks, but it is a real regression risk, so the shim was reverted
//! rather than shipped; `eda_gates::pcb` is unchanged and still the one
//! `check_placement`/`check_routing` callers get. This crate is fully
//! functional and oracle-verified on its own (`eda check --drc`,
//! `GET /api/drc`) -- what remains is making the switch-over safe, not
//! finishing the engine.
//!
//! Entry point: [`run`], which flattens a `Design`/`ConstraintModel` into a
//! [`board::DrcBoard`] (this crate's analogue of KiCad's `BOARD`) and runs
//! every test provider over it in the order KiCad itself runs them for the
//! checks this port covers, then the placement-quality providers last.
//!
//! See the task report for the full fidelity/gap list; the short version:
//! there is no `.kicad_dru` custom-rule support, and a handful of KiCad
//! features this workspace's own model has no fields for (net ties, diff
//! pairs, creepage, blind/buried vias, per-item solder-mask overrides) are
//! out of scope.
//!
//! [`fill::fill_all_zones`] runs the real `eda_zone_filler` port (zone
//! outlines are no longer used as a stand-in for their fill -- that was
//! this crate's biggest zone-related gap until the zone-filling port
//! landed); `eda_connectivity` and `eda_kicad`'s `.kicad_pcb` export both
//! call into it too, rather than each re-deriving fills on their own.

pub mod board;
pub mod constraints;
pub mod fill;
pub mod item;
pub mod kimath;
pub mod pcbexpr;
pub mod providers;
pub mod rtree;
pub mod stroke_font;

use eda_model::ir::Design;
use eda_model::ConstraintModel;
pub use item::{DrcRefItem, DrcViolation, ErrorType, FixHint, Severity};

/// Run every ported test provider and return every violation found, in the
/// same provider order the task brief lists them (copper clearance, track
/// width, via/annular width, hole size & hole-to-hole, edge clearance,
/// courtyard, silk & mask, text dimensions, dangling items, rule-area
/// keepouts -- task item 3), followed by
/// the placement-quality providers ported in from `eda_gates::pcb`.
pub fn run(design: &Design, model: &ConstraintModel) -> Vec<DrcViolation> {
    run_with(design, model, None, false)
}

/// A replacement for the built-in dangling check: `eda_connectivity`
/// supplies its port of `CONNECTIVITY_DATA::TestTrackEndpointDangling`
/// (the real connectivity graph, per-layer via rule) through
/// `eda_connectivity::run_drc` -- this crate can't depend on it directly
/// (`eda_connectivity` depends on this crate for zone fills).
pub type DanglingCheck<'a> = &'a dyn Fn(&Design, &ConstraintModel) -> Vec<DrcViolation>;

/// [`run`], with the dangling-track/via check optionally swapped for the
/// connectivity graph's own.
///
/// `test_footprints` is `RunTests`' `aTestFootprints` -- the DRC dialog's
/// "Test for parity between PCB and schematic" (unchecked by default) /
/// `kicad-cli pcb drc --schematic-parity`: only then does
/// `DRC_TEST_PROVIDER_SCHEMATIC_PARITY` run.
pub fn run_with(design: &Design, model: &ConstraintModel, dangling: Option<DanglingCheck>, test_footprints: bool) -> Vec<DrcViolation> {
    let b = board::build(design, model);
    let rules = &model.board;

    let mut out = Vec::new();
    out.extend(providers::copper_clearance::check(&b, rules));
    out.extend(providers::track_width::check(&b, rules));
    out.extend(providers::track_width::check_netclass_conformance(&b, rules));
    out.extend(providers::annular_via::check(&b, rules));
    out.extend(providers::hole::check(&b, rules));
    out.extend(providers::edge_clearance::check(&b, rules));
    out.extend(providers::courtyard::check(&b));
    out.extend(providers::silk_mask::check(&b, rules));
    out.extend(providers::text_dims::check(&b, rules));
    match dangling {
        Some(f) => out.extend(f(design, model)),
        None => out.extend(providers::dangling::check(&b)),
    }
    out.extend(providers::disallow::check(&b, rules));
    out.extend(providers::outline::check(design, model));
    if test_footprints {
        out.extend(providers::schematic_parity::check(design, model));
    }
    out.extend(providers::placement_quality::check(design, model));
    apply_rule_severities(&mut out, &rules.rule_severities);
    apply_error_limits(&mut out);
    out
}

/// `DRC_ENGINE::RunTests`' `m_errorLimits`: at most `EXTENDED_ERROR_LIMIT`
/// (499) `clearance`/`unconnected_items` and `ERROR_LIMIT` (199) of every
/// other type -- each provider stops reporting a type once its limit is
/// spent (`IsErrorLimitExceeded`), so the first ones generated are kept.
/// This port's own non-KiCad checks are left uncapped.
fn apply_error_limits(violations: &mut Vec<DrcViolation>) {
    let mut seen: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    violations.retain(|v| {
        if v.error_type.starts_with("placement_") || v.error_type == "routing_track_width" {
            return true;
        }
        let limit = if v.error_type == "clearance" || v.error_type == "unconnected_items" { 499 } else { 199 };
        let n = seen.entry(v.error_type).or_insert(0);
        *n += 1;
        *n <= limit
    });
}

/// Task item 2: apply an imported `.kicad_pro`'s `rule_severities` the same
/// way KiCad's own engine does -- a type resolved to `"ignore"` is never
/// reported at all (every `DRC_TEST_PROVIDER` gates on
/// `m_drcEngine->IsErrorLimitExceeded`/`GetSeverity() == RPT_SEVERITY_IGNORE`
/// *before* creating the `DRC_ITEM`, not after), and one explicitly set to
/// `"warning"`/`"error"` reports at that severity instead of this port's
/// own [`item::ErrorType::default_severity`]. Applied as a single
/// post-filter here rather than threading the table through every
/// provider: the net set of reported violations is identical either way,
/// since nothing in a provider's own logic depends on severity except
/// whether to report at all -- this port has no custom `.kicad_dru` rule
/// that could set a *per-constraint* severity different from its type's
/// global default (see `constraints.rs`'s doc comment on what a parsed
/// custom rule could still add). A type with no entry in `severities`
/// (the common case: no sidecar project, or one that never touched that
/// type's default) is unaffected -- in practice that always includes this
/// crate's own placement-quality/netclass checks, since a real
/// `.kicad_pro` has no settings key for a check KiCad doesn't have.
fn apply_rule_severities(violations: &mut Vec<DrcViolation>, severities: &std::collections::BTreeMap<String, String>) {
    if severities.is_empty() {
        return;
    }
    violations.retain_mut(|v| match severities.get(v.error_type).map(String::as_str) {
        Some("ignore") => false,
        Some("warning") => {
            v.severity = Severity::Warning;
            true
        }
        Some("error") => {
            v.severity = Severity::Error;
            true
        }
        _ => true,
    });
}

/// Violation counts by KiCad settings-key ("type"), for a quick summary --
/// e.g. the CLI's `--drc` flag prints this.
pub fn counts_by_type(violations: &[DrcViolation]) -> std::collections::BTreeMap<&'static str, usize> {
    let mut m = std::collections::BTreeMap::new();
    for v in violations {
        *m.entry(v.error_type).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, Point, PlacementSection, Provenance, RoutingSection, Side, Track};
    use eda_model::{Net, Part, Pin, PinKind};

    fn model_two_pads() -> ConstraintModel {
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, datasheet: None, symbol: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None };
        ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![Net { name: "A".into(), pins: vec!["R1.1".into()] }, Net { name: "B".into(), pins: vec!["R2.1".into()] }], ..Default::default() }
    }

    #[test]
    fn two_different_net_pads_too_close_report_clearance() {
        let model = model_two_pads();
        let footprints = vec![FootprintInstance { id: "R1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() }, FootprintInstance { id: "R2".into(), at: Point { x: 6_650, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() }];
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }], footprints, modules: vec![] }),
            routing: None,
            drawings: None,
        };
        let violations = run(&design, &model);
        assert!(violations.iter().any(|v| v.error_type == "clearance"), "{violations:#?}");
    }

    #[test]
    fn a_dangling_track_is_reported() {
        let mut model = model_two_pads();
        model.nets = vec![]; // no pads on any net -- track's own net is unconnected to anything
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection {
                tracks: vec![Track { id: "t1".into(), net: "A".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 1000, y: 1000 }, Point { x: 5000, y: 1000 }], arc_mid_offset: None }],
                vias: vec![],
                zones: vec![],
                track_width_presets: vec![],
                via_presets: vec![],
                teardrop_settings: Default::default(),
            }),
            drawings: None,
        };
        let violations = run(&design, &model);
        assert!(violations.iter().any(|v| v.error_type == "track_dangling"), "{violations:#?}");
    }

    #[test]
    fn rule_severity_ignore_suppresses_a_type_entirely() {
        let mut v = vec![DrcViolation::new(ErrorType::TrackDangling, "", vec![]), DrcViolation::new(ErrorType::ViaDangling, "", vec![])];
        let severities = std::collections::BTreeMap::from([("track_dangling".to_string(), "ignore".to_string())]);
        apply_rule_severities(&mut v, &severities);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].error_type, "via_dangling");
    }

    #[test]
    fn rule_severity_error_overrides_default_warning() {
        let mut v = vec![DrcViolation::new(ErrorType::TrackDangling, "", vec![])];
        assert_eq!(v[0].severity, Severity::Warning, "sanity: default severity for this type");
        let severities = std::collections::BTreeMap::from([("track_dangling".to_string(), "error".to_string())]);
        apply_rule_severities(&mut v, &severities);
        assert_eq!(v[0].severity, Severity::Error);
    }
}
