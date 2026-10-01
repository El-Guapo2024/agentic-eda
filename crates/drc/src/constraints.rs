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

/// [`clearance`], then applying any matching `.kicad_dru` custom rules on
/// top (task item 4) -- see `crate::pcbexpr`'s module doc comment for the
/// condition-evaluator subset and `providers::copper_clearance`'s
/// `facts_of_*`/`net_class_name` helpers for how `a`/`b` are built from a
/// real board item. A board with no custom rules (`rules.custom_rules` is
/// empty, the overwhelming majority) is identical to plain [`clearance`].
///
/// Matches `DRC_ENGINE::EvalRules`'s own "winner takes all" rule selection
/// for an explicit rule (`drc_engine.cpp`, read directly from the KiCad
/// source): once any custom `clearance` rule's condition matches, its
/// `min` **replaces** the netclass/board-default value entirely rather
/// than being maxed with it (an explicit rule's `processConstraint`
/// overwrites `constraint.m_Value` and the function returns immediately,
/// *before* the separate "max with local override / board minimum" step
/// that only ever runs when nothing explicit matched) -- so a custom rule
/// can legitimately *loosen* the default too, same as real KiCad. Multiple
/// matching rules are walked in file order and the last match wins, same
/// as KiCad's own rule list (`m_constraintMap`'s iteration order is
/// implicit-then-file-order, and each match is a plain overwrite, not an
/// accumulation).
///
/// Not ported: a rule's own `(layer ...)` restriction is honored (glob
/// against `layer`), but KiCad also offers per-item *local* clearance
/// overrides and footprint net-tie exclusions that take precedence over
/// even an explicit rule -- this model has no field for either, so they
/// are simply never in play, same as before custom rules existed.
pub fn clearance_with_custom_rules(rules: &BoardRules, a_net: Option<&str>, b_net: Option<&str>, layer: &str, a: &crate::pcbexpr::Facts, b: &crate::pcbexpr::Facts) -> Um {
    let mut value = clearance(rules, a_net, b_net);
    for rule in &rules.custom_rules {
        if rule.constraint_type != "clearance" {
            continue;
        }
        let Some(min) = rule.min else { continue };
        if let Some(pat) = &rule.layer {
            if !eda_model::glob_match(pat, layer) {
                continue;
            }
        }
        if crate::pcbexpr::matches(rule.condition.as_deref(), a, b) {
            value = min;
        }
    }
    value
}

/// The largest clearance value *any* pair on this board could possibly
/// resolve to -- `BOARD::GetMaxClearanceValue()` / `m_DRCMaxClearance` in
/// `drc_cache_generator.cpp`, used there to size how far a `DRC_RTREE`
/// query reaches so a bounded spatial query can never miss a pair that
/// `EvalRules` would later resolve to a bigger-than-default clearance.
/// Board default and hole clearance are always candidates; so is every
/// netclass's own clearance override (the only per-net source this port
/// resolves -- see [`clearance`]'s own doc comment on what's out of scope).
pub fn worst_case_clearance(rules: &BoardRules) -> Um {
    let base = rules.clearance.max(rules.hole_clearance_um);
    rules.net_classes.iter().filter_map(|c| c.clearance).fold(base, Um::max)
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
        BoardRules {
            net_classes: vec![NetClass { name: "power".into(), nets: vec!["VIN".into()], track_width: None, clearance: Some(500), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }],
            ..BoardRules::default()
        }
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

    fn facts(item_type: &'static str, net_class: &'static str, net_name: &'static str) -> crate::pcbexpr::Facts<'static> {
        crate::pcbexpr::Facts { item_type, net_class, net_name, reference: "" }
    }

    #[test]
    fn custom_rule_overrides_the_netclass_default() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: None, severity: None, condition: Some("A.NetClass == B.NetClass".into()) }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "Default", "GND"), facts("Pad", "Default", "GND"));
        // Same class -> the rule matches and *replaces* the board default,
        // even though 50um here is smaller than it (an explicit rule can
        // loosen a default too -- see this function's doc comment).
        assert_eq!(clearance_with_custom_rules(&r, Some("GND"), Some("GND"), "F.Cu", &a, &b), 50);
    }

    #[test]
    fn custom_rule_with_non_matching_condition_is_a_no_op() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: None, severity: None, condition: Some("A.NetClass == B.NetClass".into()) }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "power", "VIN"), facts("Pad", "signal", "SIG"));
        assert_eq!(clearance_with_custom_rules(&r, Some("VIN"), Some("SIG"), "F.Cu", &a, &b), clearance(&r, Some("VIN"), Some("SIG")), "different classes -> condition does not match -> plain default");
    }

    #[test]
    fn custom_rule_scoped_to_a_different_layer_is_a_no_op() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: Some("B.Cu".into()), severity: None, condition: None }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "Default", "A"), facts("Pad", "Default", "B"));
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "F.Cu", &a, &b), clearance(&r, Some("A"), Some("B")), "rule is scoped to B.Cu, this pair is on F.Cu");
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "B.Cu", &a, &b), 50, "same pair, matching layer");
    }

    #[test]
    fn last_matching_custom_rule_wins() {
        let r = BoardRules {
            custom_rules: vec![
                eda_model::CustomRule { name: "first".into(), constraint_type: "clearance".into(), min: Some(300), max: None, opt: None, layer: None, severity: None, condition: None },
                eda_model::CustomRule { name: "second".into(), constraint_type: "clearance".into(), min: Some(400), max: None, opt: None, layer: None, severity: None, condition: None },
            ],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "Default", "A"), facts("Pad", "Default", "B"));
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "F.Cu", &a, &b), 400, "both match unconditionally; the later rule in file order wins");
    }
}
