//! Gates for a board that is only half built.
//!
//! `check_placement` is written for a finished design: its very first
//! substantive check fails every part that is not placed. Run it after
//! part 12 of 100 and you get 88 `placement_coverage` failures and no
//! signal. That is why placement has only ever been judged at the end,
//! and why a failure could never be attributed to the decision that
//! caused it.
//!
//! A constructive placer needs the opposite: a verdict on what exists so
//! far, after every step, so a bad choice is caught at part 12 instead of
//! after the full run.
//!
//! The implementation deliberately does **not** reimplement any gate. It
//! narrows the model to what is placed and hands that to the real
//! `check_placement`. Every second model of what a gate wants has drifted
//! from the gate sooner or later in this codebase -- that divergence is
//! the single most expensive bug class here -- so the partial check is a
//! filter in front of the same code, not a parallel copy of it.
//!
//! What "narrowing" means, and why each choice:
//!
//! * A part that is not yet placed is removed from the model, so
//!   `placement_coverage` stops demanding it.
//! * A net is kept only when *every* one of its parts is placed. A net
//!   half laid down has no meaningful span: judging `LED_CH1` on two of
//!   its four parts would pass a net that is about to fail.
//! * A proximity or separation rule is kept only when both its parts are
//!   placed, for the same reason -- a rule against an absent part is not
//!   satisfied, it is merely unmeasurable.
//! * Geometric gates (courtyard overlap, outline, refdes clearance) need
//!   no special handling: they are true of any subset the moment it
//!   exists, and they are the ones worth failing early.
//!
//! So the partial verdict is sound in one direction, which is the
//! direction that matters: anything it fails is genuinely wrong now, and
//! it cannot see problems that only a later part will create.

use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel, PlacementRule};
use std::collections::BTreeSet;

/// Judge the parts placed so far.
///
/// Returns the same check names as `check_placement`, scoped to the
/// placed subset, so a caller can count failures across steps without
/// learning a second vocabulary.
pub fn check_placement_partial(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let placed: BTreeSet<&str> = design
        .placement
        .as_ref()
        .map(|p| p.footprints.iter().map(|f| f.id.as_str()).collect())
        .unwrap_or_default();

    // Nothing placed yet is not a failure, it is the starting state.
    if placed.is_empty() {
        return Vec::new();
    }

    let sub = narrow(model, &placed);
    let mut checks = super::pcb::check_placement(design, &sub);
    checks.retain(|c| !WHOLE_BOARD_ONLY.contains(&c.check.as_str()));
    checks
}

/// Gates that are statements about the *finished* board and are false of
/// every prefix of it.
///
/// `placement_board_use` asks whether the parts fill and centre the board
/// they were given. Two parts into a hundred they cover 0% of it and sit
/// in one corner, so it fails at part 2 of every run with three findings
/// that say nothing except "the board is not finished". Left in, it
/// drowns the failures worth acting on -- which is exactly what the
/// part-by-part replay of a real L4 placement showed.
///
/// Everything else in `check_placement` is true of a subset the moment
/// that subset exists: courtyards either overlap or they do not, a
/// connector is on an edge or it is not. Those are the ones early
/// feedback is for.
const WHOLE_BOARD_ONLY: &[&str] = &["placement_board_use"];

/// The model as it applies to `placed` alone.
fn narrow(model: &ConstraintModel, placed: &BTreeSet<&str>) -> ConstraintModel {
    let keep_part = |r: &str| placed.contains(r);
    let of = |pin: &str| pin.split('.').next().unwrap_or(pin).to_string();

    ConstraintModel {
        parts: model.parts.iter().filter(|p| keep_part(&p.reference)).cloned().collect(),
        nets: model
            .nets
            .iter()
            .filter(|n| n.pins.iter().all(|p| keep_part(&of(p))))
            .cloned()
            .collect(),
        clusters: model.clusters.iter().filter(|c| keep_part(&c.anchor) && c.members.iter().all(|m| keep_part(m))).cloned().collect(),
        placement_rules: model
            .placement_rules
            .iter()
            .filter(|r| match r {
                PlacementRule::Proximity { a, b, .. } | PlacementRule::Separation { a, b, .. } => keep_part(a) && keep_part(b),
                PlacementRule::Keepout { refs, .. } | PlacementRule::ThermalGroup { refs } => refs.iter().all(|r| keep_part(r)),
            })
            .cloned()
            .collect(),
        ..model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, LabelSide, PlacementSection, Point, Provenance, Side};
    use eda_model::{Net, Part};

    /// A part with a real package: without one there is no courtyard,
    /// and the geometric gates have nothing to measure.
    fn part(r: &str) -> Part {
        Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("0805".into()),
            footprint: None,
            pins: vec![
                eda_model::Pin { number: "1".into(), name: None, kind: eda_model::PinKind::Passive },
                eda_model::Pin { number: "2".into(), name: None, kind: eda_model::PinKind::Passive },
            ],
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        }
    }

    fn fp(r: &str, x: i64, y: i64) -> FootprintInstance {
        FootprintInstance { id: r.into(), at: Point { x, y }, rot: 0, side: Side::Top, label: LabelSide::Above }
    }

    fn model_of(parts: &[&str], nets: &[(&str, &[&str])], rules: Vec<PlacementRule>) -> ConstraintModel {
        ConstraintModel {
            parts: parts.iter().map(|r| part(r)).collect(),
            nets: nets
                .iter()
                .map(|(n, pins)| Net { name: (*n).into(), pins: pins.iter().map(|p| format!("{p}.1")).collect() })
                .collect(),
            placement_rules: rules,
            ..Default::default()
        }
    }

    fn design_with(fps: Vec<FootprintInstance>) -> Design {
        Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            routing: None,
            placement: Some(PlacementSection {
                outline: vec![
                    Point { x: 0, y: 0 },
                    Point { x: 100_000, y: 0 },
                    Point { x: 100_000, y: 100_000 },
                    Point { x: 0, y: 100_000 },
                ],
                footprints: fps,
                modules: Vec::new(),
            }),
            drawings: None,
        }
    }

    #[test]
    fn an_unplaced_part_is_not_a_coverage_failure() {
        // The whole reason this module exists: the finished-board gate
        // reports 88 failures on a board that is merely 12 parts in.
        let m = model_of(&["U1", "U2", "U3"], &[], vec![]);
        let d = design_with(vec![fp("U1", 10_000, 10_000)]);

        let full = super::super::pcb::check_placement(&d, &m);
        assert!(
            full.iter().any(|c| c.check == "placement_coverage" && matches!(c.status, eda_model::CheckStatus::Fail)),
            "the finished-board gate is expected to fail here; that is the problem being solved"
        );

        let partial = check_placement_partial(&d, &m);
        assert!(
            !partial.iter().any(|c| c.check == "placement_coverage" && matches!(c.status, eda_model::CheckStatus::Fail)),
            "{partial:?}"
        );
    }

    #[test]
    fn a_geometric_failure_is_caught_immediately() {
        // A part hanging off the board is wrong the moment it happens, and
        // is exactly what early feedback is for. (Two parts stacked on one
        // spot is wrong too, but that is KiCad's `courtyards_overlap`:
        // kicad-cli answers it, in `crate::kicad`, not in this per-step
        // gate.)
        let m = model_of(&["U1", "U2", "U3"], &[], vec![]);
        let d = design_with(vec![fp("U1", 10_000, 10_000), fp("U2", 100, 100)]);
        let checks = check_placement_partial(&d, &m);
        assert!(
            checks.iter().any(|c| c.check == "placement_within_outline" && matches!(c.status, eda_model::CheckStatus::Fail)),
            "a part off the board must fail now, not at the end: {checks:?}"
        );
    }

    #[test]
    fn a_rule_against_an_unplaced_part_is_not_judged() {
        // Unmeasurable is not the same as violated. Failing here would
        // make every step fail until the last part landed.
        let m = model_of(
            &["U1", "U2"],
            &[],
            vec![PlacementRule::Proximity { a: "U1".into(), b: "U2".into(), max_mm: 1.0, reason: None }],
        );
        let d = design_with(vec![fp("U1", 10_000, 10_000)]);
        let checks = check_placement_partial(&d, &m);
        assert!(
            !checks.iter().any(|c| c.check == "placement_proximity" && matches!(c.status, eda_model::CheckStatus::Fail)),
            "{checks:?}"
        );
    }

    #[test]
    fn a_rule_is_judged_once_both_its_parts_are_down() {
        let m = model_of(
            &["U1", "U2"],
            &[],
            vec![PlacementRule::Proximity { a: "U1".into(), b: "U2".into(), max_mm: 1.0, reason: None }],
        );
        // 60mm apart against a 1mm rule.
        let d = design_with(vec![fp("U1", 10_000, 10_000), fp("U2", 70_000, 10_000)]);
        let checks = check_placement_partial(&d, &m);
        assert!(
            checks.iter().any(|c| c.check == "placement_proximity" && matches!(c.status, eda_model::CheckStatus::Fail)),
            "{checks:?}"
        );
    }

    #[test]
    fn a_half_placed_net_is_not_judged_for_span() {
        // Two of four parts always look compact. Judging the net now
        // would pass something about to fail.
        let m = model_of(&["U1", "U2", "U3", "U4"], &[("N1", &["U1", "U2", "U3", "U4"])], vec![]);
        let d = design_with(vec![fp("U1", 10_000, 10_000), fp("U2", 12_000, 10_000)]);
        let sub = narrow(&m, &["U1", "U2"].into_iter().collect());
        assert!(sub.nets.is_empty(), "a net with unplaced members must be withheld, not measured");
        let _ = check_placement_partial(&d, &m);
    }

    #[test]
    fn a_whole_board_gate_is_not_applied_to_a_prefix() {
        // Two parts in a corner of a big board cover ~0% of it and sit
        // well off-centre. That is not a placement defect, it is an
        // unfinished board, and reporting it every step buries the
        // failures that matter.
        let m = model_of(&["U1", "U2", "U3", "U4"], &[], vec![]);
        let d = design_with(vec![fp("U1", 5_000, 5_000), fp("U2", 8_000, 5_000)]);
        let checks = check_placement_partial(&d, &m);
        assert!(
            !checks.iter().any(|c| c.check == "placement_board_use"),
            "whole-board gates must be withheld until the board is whole: {checks:?}"
        );
    }

    #[test]
    fn the_withheld_list_names_gates_that_actually_exist() {
        // A typo here would silently withhold nothing.
        let emitted = include_str!("pcb.rs");
        for g in WHOLE_BOARD_ONLY {
            assert!(emitted.contains(&format!("\"{g}\"")), "{g} is not a gate pcb.rs emits");
        }
    }

    #[test]
    fn nothing_placed_yet_is_not_a_verdict() {
        let m = model_of(&["U1"], &[], vec![]);
        let d = design_with(vec![]);
        assert!(check_placement_partial(&d, &m).is_empty());
    }
}
