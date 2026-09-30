//! This workspace's single design-rule authority: a Rust port of KiCad's
//! PCB design-rule checker (`pcbnew/drc/`), following its actual algorithms
//! and error-type strings rather than reinventing simplified equivalents,
//! *plus* the placement-quality checks that have no KiCad equivalent
//! (proximity, decoupling, board use, net compactness, stub crossings,
//! edge-connector placement, refdes clearance), folded in as providers of
//! their own. `eda_gates::pcb` used to own those; see the task report's
//! gates-mapping table for exactly what moved here, what stayed, and why --
//! the short version is "no duplicate tools": once a check has a provider
//! here, `eda_gates` calls this crate instead of re-implementing it.
//!
//! Entry point: [`run`], which flattens a `Design`/`ConstraintModel` into a
//! [`board::DrcBoard`] (this crate's analogue of KiCad's `BOARD`) and runs
//! every test provider over it in the order KiCad itself runs them for the
//! checks this port covers, then the placement-quality providers last.
//!
//! See the task report for the full fidelity/gap list; the short version:
//! zone *fill* is not modelled (checks against a zone use its outline, as
//! directed), there is no `.kicad_dru` custom-rule support, and a handful
//! of KiCad features this workspace's own model has no fields for (net
//! ties, diff pairs, creepage, blind/buried vias, per-item solder-mask
//! overrides) are out of scope.

pub mod board;
pub mod constraints;
pub mod item;
pub mod kimath;
pub mod providers;

use eda_model::ir::Design;
use eda_model::ConstraintModel;
pub use item::{DrcRefItem, DrcViolation, ErrorType, FixHint, Severity};

/// Run every ported test provider and return every violation found, in the
/// same provider order the task brief lists them (copper clearance, track
/// width, via/annular width, hole size & hole-to-hole, edge clearance,
/// courtyard, silk & mask, text dimensions, dangling items), followed by
/// the placement-quality providers ported in from `eda_gates::pcb`.
pub fn run(design: &Design, model: &ConstraintModel) -> Vec<DrcViolation> {
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
    out.extend(providers::dangling::check(&b));
    out.extend(providers::placement_quality::check(design, model));
    out
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
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None };
        ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![Net { name: "A".into(), pins: vec!["R1.1".into()] }, Net { name: "B".into(), pins: vec!["R2.1".into()] }], ..Default::default() }
    }

    #[test]
    fn two_different_net_pads_too_close_report_clearance() {
        let model = model_two_pads();
        let footprints = vec![FootprintInstance { id: "R1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() }, FootprintInstance { id: "R2".into(), at: Point { x: 6_650, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() }];
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
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
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![Track { id: "t1".into(), net: "A".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 1000, y: 1000 }, Point { x: 5000, y: 1000 }] }], vias: vec![], zones: vec![] }),
            drawings: None,
        };
        let violations = run(&design, &model);
        assert!(violations.iter().any(|v| v.error_type == "track_dangling"), "{violations:#?}");
    }
}
