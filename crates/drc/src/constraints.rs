//! Constraint resolution -- the part of `DRC_ENGINE::EvalRules` /
//! `DRC_ENGINE::loadImplicitRules` (`pcbnew/drc/drc_engine.cpp`) that
//! matters once a `.kicad_dru` file is out of scope: turning board design
//! settings and net classes into the numeric value each test provider
//! checks against.
//!
//! What's ported and what's not, at a glance:
//! - Global "board setup constraints" (`loadImplicitRules`, section 1):
//!   every `*_min` function below is one of these -- e.g.
//!   `track_width_min` is exactly `widthConstraint.Value().SetMin(
//!   bds.m_TrackMinWidth)`.
//! - Per-netclass clearance (`loadImplicitRules`, section 3, and the
//!   `EvalRules` fast-path around `m_netclassClearances`): [`clearance`]
//!   below is that fast path -- `max` of the two nets' resolved class
//!   clearance, board default when a net has no class or no override.
//! - Local per-item clearance overrides, net ties, diff pairs, creepage,
//!   tuning profiles, keepout zones, and anything driven by a parsed
//!   `.kicad_dru` rule file are **not** ported: our model has no fields for
//!   any of them. See the task report for the full gap list.

use eda_model::ir::Um;
use eda_model::BoardRules;

/// Clearance required between two items on nets `a`/`b` (`None` = no net --
/// a non-plated hole, an unrouted graphic). Ported from `EvalRules`'s
/// netclass fast path: the *larger* of the two nets' resolved class
/// clearance (falling back to the board default), 0 when neither side has a
/// net. Same-net items are never clearance-checked at all (skipped by every
/// provider before calling this, exactly as `testSingleLayerItemAgainstItem`
/// zeroes `testClearance` when `itemNet == otherNet`).
pub fn clearance(rules: &BoardRules, a: Option<&str>, b: Option<&str>) -> Um {
    let ca = a.map(|n| rules.clearance_of(n)).unwrap_or(0);
    let cb = b.map(|n| rules.clearance_of(n)).unwrap_or(0);
    ca.max(cb)
}

/// `bds.m_TrackMinWidth`: the absolute track-width floor, independent of
/// net class (a netclass's own `track_width` only ever sets the *nominal*
/// value KiCad's router aims for -- see `loadImplicitRules`'s
/// `constraint.Value().SetMin(bds.m_TrackMinWidth); constraint.Value().
/// SetOpt(nc->GetTrackWidth())`, min always the global setting).
pub fn track_width_min(rules: &BoardRules) -> Um {
    rules.track_width
}

/// `bds.m_ViasMinSize`.
pub fn via_diameter_min(rules: &BoardRules) -> Um {
    rules.via_diameter
}

/// `bds.m_MinThroughDrill` (round-hole pads and standard vias; this port
/// does not model micro-vias, so `DRCE_MICROVIA_DRILL_OUT_OF_RANGE` never
/// applies).
pub fn hole_size_min(rules: &BoardRules) -> Um {
    rules.via_drill
}

/// `bds.m_ViasMinAnnularWidth`.
pub fn annular_width_min(rules: &BoardRules) -> Um {
    rules.annular_width_min_um
}

/// `bds.m_HoleToHoleMin` -- mechanical, applies between any two round
/// drilled holes regardless of net (see `drc_test_provider_hole_to_hole.cpp`:
/// no net check anywhere in it).
pub fn hole_to_hole_min(rules: &BoardRules) -> Um {
    rules.hole_to_hole_min_um
}

/// `bds.m_HoleClearance` -- copper-to-(another item's)-hole, regardless of
/// net (a hole has to clear foreign copper even where nets would otherwise
/// allow touching, since something physically has to be drilled there).
pub fn hole_clearance_min(rules: &BoardRules) -> Um {
    rules.hole_clearance_um
}

/// `bds.m_CopperEdgeClearance`; already resolved by the existing
/// `RoutingTuning::copper_edge_clearance` (this board's tuning, floored at
/// KiCad's own 0.5 mm default).
pub fn edge_clearance_min(rules: &BoardRules) -> Um {
    rules.tuning.copper_edge_clearance()
}

/// `bds.m_SilkClearance` -- silk-to-silk, and (via the `silk_clearance.rs`
/// provider) silk-to-exposed-copper and silk-to-edge.
pub fn silk_clearance_min(rules: &BoardRules) -> Um {
    rules.silk_clearance_um
}

/// `bds.m_MinSilkTextHeight` (implicit rule scoped to `F.SilkS`/`B.SilkS`).
pub fn min_silk_text_height(rules: &BoardRules) -> Um {
    rules.min_silk_text_height_um
}

/// `bds.m_MinSilkTextThickness` (implicit rule scoped to `F.SilkS`/`B.SilkS`).
pub fn min_silk_text_thickness(rules: &BoardRules) -> Um {
    rules.min_silk_text_thickness_um
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::NetClass;

    fn rules_with_classes() -> BoardRules {
        BoardRules { net_classes: vec![NetClass { name: "power".into(), nets: vec!["VIN".into()], track_width: None, clearance: Some(500), priority: 0 }], ..BoardRules::default() }
    }

    #[test]
    fn clearance_is_the_larger_of_the_two_nets_classes() {
        let r = rules_with_classes();
        assert_eq!(clearance(&r, Some("VIN"), Some("GND")), 500, "VIN's 500um class beats GND's board default");
        assert_eq!(clearance(&r, Some("GND"), Some("SIG")), r.clearance, "neither net has a class override");
    }

    #[test]
    fn no_net_on_either_side_is_zero() {
        let r = BoardRules::default();
        assert_eq!(clearance(&r, None, None), 0);
    }
}
