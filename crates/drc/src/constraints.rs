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
//! - A parsed `.kicad_dru` rule file's `clearance` constraints *are* ported
//!   (task item 4): [`clearance_with_custom_rules`] layers
//!   `BoardRules::custom_rules` on top of the netclass fast path, evaluated
//!   by `crate::pcbexpr`'s condition-evaluator subset. Every other
//!   constraint type a custom rule can set (`hole_clearance`, `track_width`,
//!   `annular_width`, `disallow`, `assertion`, ...), local per-item
//!   clearance overrides, net ties, diff pairs, creepage, tuning profiles,
//!   and keepout zones are **not** ported: our model has no fields for any
//!   of them. See the task report for the full gap list.

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
    // A netless item (net 0, or a non-connected copper graphic) is still
    // in the Default netclass: its implicit clearance rule applies.
    let ca = a.map(|n| rules.clearance_of(n)).unwrap_or(rules.clearance);
    let cb = b.map(|n| rules.clearance_of(n)).unwrap_or(rules.clearance);
    // `bds.m_MinClearance`: an absolute board-wide floor maxed in on top of
    // the netclass value -- `EvalRules`'s final `CLEARANCE_CONSTRAINT`
    // special case ("Board minimum clearance"), applied whenever the
    // winning rule (if any) was implicit. A board with the factory-default
    // `min_clearance` of 0 sees no change here; [`clearance_with_custom_rules`]
    // layers explicit `.kicad_dru` rules on top of *this* value by full
    // replacement, matching KiCad's early-return-before-the-floor behavior
    // for an explicit match (see that function's doc comment).
    ca.max(cb).max(rules.min_clearance_um)
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
///
/// Takes `compiled` (see [`CompiledClearanceRules`]) rather than reading
/// `rules.custom_rules` directly and tokenizing each condition on the fly:
/// a real board resolves clearance for thousands of pairs, and
/// re-tokenizing/re-parsing the same handful of condition strings from
/// scratch on every single one of them is a real cost, not a
/// micro-optimization -- measured directly on a real QA-corpus board
/// (`issue11814`, which pairs a `.kicad_dru` with a lot of copper) blowing
/// the parity harness's own per-board watchdog. Compile once per board
/// (`CompiledClearanceRules::new`), reuse for every pair.
pub fn clearance_with_custom_rules(rules: &BoardRules, a_net: Option<&str>, b_net: Option<&str>, layer: &str, a: &crate::pcbexpr::Facts, b: &crate::pcbexpr::Facts, compiled: &CompiledClearanceRules) -> Um {
    let mut value = clearance(rules, a_net, b_net);
    for entry in &compiled.0 {
        if let Some(pat) = &entry.layer {
            if !eda_model::glob_match(pat, layer) {
                continue;
            }
        }
        if crate::pcbexpr::matches_compiled(entry.condition.as_ref(), a, b) {
            value = entry.min;
        }
    }
    value
}

/// `rules.custom_rules`, filtered to the `clearance`-constraint entries
/// [`clearance_with_custom_rules`] can actually apply and with each one's
/// `condition` tokenized once -- see that function's doc comment for why
/// this exists as a separate compile-once step rather than reading
/// `BoardRules` directly per pair. Build one of these once per board
/// (cheap: real `.kicad_dru` files are a handful of rules), not per pair.
pub struct CompiledClearanceRules(Vec<CompiledClearanceRule>);

struct CompiledClearanceRule {
    min: Um,
    layer: Option<String>,
    condition: Option<crate::pcbexpr::CompiledCondition>,
}

impl CompiledClearanceRules {
    pub fn new(rules: &BoardRules) -> Self {
        CompiledClearanceRules(
            rules
                .custom_rules
                .iter()
                .filter(|r| r.constraint_type == "clearance")
                .filter_map(|r| {
                    let min = r.min?;
                    Some(CompiledClearanceRule { min, layer: r.layer.clone(), condition: r.condition.as_deref().and_then(crate::pcbexpr::compile) })
                })
                .collect(),
        )
    }
}

/// `rules.custom_rules`'s `track_width`-constraint entries, condition
/// tokenized once -- the `track_width` analogue of
/// [`CompiledClearanceRules`] (see that type's doc comment for why this is
/// a compile-once step). A real QA-corpus example of why this matters:
/// `multinetclasses_drc.kicad_dru` caps each net class at its own
/// `(constraint track_width (max ...))` -- KiCad has no native per-class
/// *maximum* width, so a custom rule is the only way a real project
/// expresses one, and it is common enough to be worth porting alongside
/// the `min` side of this same constraint type.
///
/// KiCad evaluates `TRACK_WIDTH_CONSTRAINT` as a one-item condition
/// (`EvalRules(TRACK_WIDTH_CONSTRAINT, item, nullptr, layer)` --
/// `drc_test_provider_track_width.cpp`'s `Run()`), so every real rule's
/// `condition` for this constraint type only ever references `A.*`; this
/// port evaluates it by passing the same [`crate::pcbexpr::Facts`] for
/// both sides of [`crate::pcbexpr::matches_compiled`], which is exact for
/// an `A`-only condition, not an approximation.
pub struct CompiledWidthRules(Vec<CompiledWidthRule>);

struct CompiledWidthRule {
    min: Option<Um>,
    max: Option<Um>,
    layer: Option<String>,
    condition: Option<crate::pcbexpr::CompiledCondition>,
}

impl CompiledWidthRules {
    pub fn new(rules: &BoardRules) -> Self {
        CompiledWidthRules(
            rules
                .custom_rules
                .iter()
                .filter(|r| r.constraint_type == "track_width")
                .filter(|r| r.min.is_some() || r.max.is_some())
                .map(|r| CompiledWidthRule { min: r.min, max: r.max, layer: r.layer.clone(), condition: r.condition.as_deref().and_then(crate::pcbexpr::compile) })
                .collect(),
        )
    }
}

/// `(min, max)` bounds for one track/arc's width: [`track_width_min`]'s
/// board floor as `min`, `None` for `max`, then any matching `.kicad_dru`
/// `track_width` rule applied on top of *both*, independently -- last
/// match wins, same winner-take-all precedence as
/// [`clearance_with_custom_rules`] (only the fields a given rule's
/// constraint actually sets are overwritten, matching
/// `DRC_ENGINE::EvalRules`'s `applyConstraint`, which touches `Min`/`Opt`/
/// `Max` independently of one another).
pub fn track_width_bounds(rules: &BoardRules, layer: &str, a: &crate::pcbexpr::Facts, compiled: &CompiledWidthRules) -> (Um, Option<Um>) {
    let mut min = track_width_min(rules);
    let mut max = None;
    for entry in &compiled.0 {
        if let Some(pat) = &entry.layer {
            if !eda_model::glob_match(pat, layer) {
                continue;
            }
        }
        if crate::pcbexpr::matches_compiled(entry.condition.as_ref(), a, a) {
            if let Some(m) = entry.min {
                min = m;
            }
            if entry.max.is_some() {
                max = entry.max;
            }
        }
    }
    (min, max)
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
    let base = rules.clearance.max(rules.hole_clearance_um).max(rules.min_clearance_um);
    rules.net_classes.iter().filter_map(|c| c.clearance).fold(base, Um::max)
}

/// `bds.m_TrackMinWidth`: the absolute track-width floor, independent of
/// net class (a netclass's own `track_width` only ever sets the *nominal*
/// value KiCad's router aims for -- see `loadImplicitRules`'s
/// `constraint.Value().SetMin(bds.m_TrackMinWidth); constraint.Value().
/// SetOpt(nc->GetTrackWidth())`, min always the global setting). See
/// [`BoardRules::track_width_min_um`]'s own doc comment for the GAPS.md
/// #10 bug this used to have (reading the net-class nominal width here
/// instead of the real board floor).
pub fn track_width_min(rules: &BoardRules) -> Um {
    rules.track_width_min_um
}

/// `bds.m_ViasMinSize` -- see [`track_width_min`]'s doc comment for why
/// this is a dedicated field now rather than the net class's own nominal
/// [`BoardRules::via_diameter`].
pub fn via_diameter_min(rules: &BoardRules) -> Um {
    rules.via_diameter_min_um
}

/// `bds.m_MinThroughDrill` (round-hole pads and standard vias; this port
/// does not model micro-vias, so `DRCE_MICROVIA_DRILL_OUT_OF_RANGE` never
/// applies) -- see [`track_width_min`]'s doc comment for why this reads a
/// dedicated field rather than the net class's own nominal
/// [`BoardRules::via_drill`].
pub fn hole_size_min(rules: &BoardRules) -> Um {
    rules.via_drill_min_um
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
    rules.copper_edge_clearance_um.unwrap_or(eda_model::KICAD_EDGE_CLEARANCE_UM)
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
    fn no_net_on_either_side_gets_the_default_netclass() {
        // Net 0 is still in the Default netclass (KiCad's implicit rule).
        let r = BoardRules::default();
        assert_eq!(clearance(&r, None, None), r.clearance);
    }

    fn facts(item_type: &'static str, net_class: &'static str, net_name: &'static str) -> crate::pcbexpr::Facts<'static> {
        crate::pcbexpr::Facts { item_type, net_class, net_name, reference: "", inside_courtyards: &[] }
    }

    #[test]
    fn custom_rule_overrides_the_netclass_default() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: None, severity: None, condition: Some("A.NetClass == B.NetClass".into()) }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "Default", "GND"), facts("Pad", "Default", "GND"));
        let compiled = CompiledClearanceRules::new(&r);
        // Same class -> the rule matches and *replaces* the board default,
        // even though 50um here is smaller than it (an explicit rule can
        // loosen a default too -- see this function's doc comment).
        assert_eq!(clearance_with_custom_rules(&r, Some("GND"), Some("GND"), "F.Cu", &a, &b, &compiled), 50);
    }

    #[test]
    fn custom_rule_with_non_matching_condition_is_a_no_op() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: None, severity: None, condition: Some("A.NetClass == B.NetClass".into()) }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "power", "VIN"), facts("Pad", "signal", "SIG"));
        let compiled = CompiledClearanceRules::new(&r);
        assert_eq!(clearance_with_custom_rules(&r, Some("VIN"), Some("SIG"), "F.Cu", &a, &b, &compiled), clearance(&r, Some("VIN"), Some("SIG")), "different classes -> condition does not match -> plain default");
    }

    #[test]
    fn custom_rule_scoped_to_a_different_layer_is_a_no_op() {
        let r = BoardRules {
            custom_rules: vec![eda_model::CustomRule { name: "r".into(), constraint_type: "clearance".into(), min: Some(50), max: None, opt: None, layer: Some("B.Cu".into()), severity: None, condition: None }],
            ..BoardRules::default()
        };
        let (a, b) = (facts("Pad", "Default", "A"), facts("Pad", "Default", "B"));
        let compiled = CompiledClearanceRules::new(&r);
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "F.Cu", &a, &b, &compiled), clearance(&r, Some("A"), Some("B")), "rule is scoped to B.Cu, this pair is on F.Cu");
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "B.Cu", &a, &b, &compiled), 50, "same pair, matching layer");
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
        let compiled = CompiledClearanceRules::new(&r);
        assert_eq!(clearance_with_custom_rules(&r, Some("A"), Some("B"), "F.Cu", &a, &b, &compiled), 400, "both match unconditionally; the later rule in file order wins");
    }

    /// GAPS.md #10's actual regression test: a net class whose own nominal
    /// width is set well *above* the board's real minimum must not raise
    /// the enforced floor -- that nominal value is only ever the `Opt` a
    /// router aims for, never a DRC `Min` (see `track_width_min`'s doc
    /// comment). This is exactly the issue11814 shape: a `0.1016mm` board
    /// minimum against a `0.1524mm` "Default" net class nominal width.
    #[test]
    fn track_width_min_ignores_the_netclass_nominal_width() {
        let r = BoardRules { track_width: 1524, track_width_min_um: 1016, ..BoardRules::default() };
        assert_eq!(track_width_min(&r), 1016, "the board's real floor, not the net class's much larger nominal width");
    }

    #[test]
    fn via_and_hole_minimums_ignore_the_netclass_nominal_values() {
        let r = BoardRules { via_diameter: 9999, via_diameter_min_um: 450, via_drill: 9999, via_drill_min_um: 200, ..BoardRules::default() };
        assert_eq!(via_diameter_min(&r), 450);
        assert_eq!(hole_size_min(&r), 200);
    }

    #[test]
    fn board_minimum_clearance_floors_the_netclass_value() {
        let r = BoardRules { clearance: 100, min_clearance_um: 150, ..BoardRules::default() };
        assert_eq!(clearance(&r, Some("A"), Some("B")), 150, "board floor beats a smaller netclass/default value");
        let r2 = BoardRules { clearance: 300, min_clearance_um: 150, ..BoardRules::default() };
        assert_eq!(clearance(&r2, Some("A"), Some("B")), 300, "netclass value already clears the floor");
    }

    #[test]
    fn default_min_clearance_is_a_no_op() {
        // Factory default (`rules.min_clearance` = 0mm) must not change
        // existing behavior for the overwhelming majority of boards.
        let r = BoardRules::default();
        assert_eq!(clearance(&r, Some("A"), Some("B")), r.clearance);
    }

    #[test]
    fn track_width_bounds_defaults_to_the_board_floor_with_no_max() {
        let r = BoardRules { track_width_min_um: 150, ..BoardRules::default() };
        let compiled = CompiledWidthRules::new(&r);
        let a = facts("Track", "Default", "SIG");
        assert_eq!(track_width_bounds(&r, "F.Cu", &a, &compiled), (150, None));
    }

    /// Real QA-corpus shape (`multinetclasses_drc.kicad_dru`): a custom
    /// rule caps a specific net class's width with `(max ...)` and never
    /// touches `(min ...)` at all -- the board floor must survive
    /// untouched alongside the newly-applied cap.
    #[test]
    fn custom_track_width_rule_adds_a_max_without_disturbing_the_floor() {
        let r = BoardRules {
            track_width_min_um: 100,
            custom_rules: vec![eda_model::CustomRule { name: "cap".into(), constraint_type: "track_width".into(), min: None, max: Some(254), opt: None, layer: None, severity: None, condition: Some("A.hasNetclass('CLASS2')".into()) }],
            ..BoardRules::default()
        };
        let compiled = CompiledWidthRules::new(&r);
        let class2 = facts("Track", "CLASS2", "N1");
        assert_eq!(track_width_bounds(&r, "F.Cu", &class2, &compiled), (100, Some(254)), "min untouched, max newly applied");
        let other = facts("Track", "Default", "N2");
        assert_eq!(track_width_bounds(&r, "F.Cu", &other, &compiled), (100, None), "condition doesn't match -> no cap");
    }

    #[test]
    fn last_matching_custom_track_width_rule_wins_per_field() {
        let r = BoardRules {
            track_width_min_um: 100,
            custom_rules: vec![
                eda_model::CustomRule { name: "a".into(), constraint_type: "track_width".into(), min: Some(120), max: None, opt: None, layer: None, severity: None, condition: None },
                eda_model::CustomRule { name: "b".into(), constraint_type: "track_width".into(), min: None, max: Some(500), opt: None, layer: None, severity: None, condition: None },
            ],
            ..BoardRules::default()
        };
        let compiled = CompiledWidthRules::new(&r);
        let a = facts("Track", "Default", "N1");
        // Rule "a" raises min to 120; rule "b" (unconditional, later) only
        // sets max, so it must not reset min back to the board floor.
        assert_eq!(track_width_bounds(&r, "F.Cu", &a, &compiled), (120, Some(500)));
    }
}
