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
//! - A `.kicad_dru` rule file, the implicit rules of the board setup and the
//!   per-item local overrides: [`RuleEngine`] below is `DRC_ENGINE::EvalRules`
//!   itself, returning the winning constraint and every rule it considered with
//!   KiCad's report text (what the Clearance and Constraints Resolution reports
//!   show). It inspects the rules; DRC stays kicad-cli's.

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
    // `min_clearance` of 0 sees no change here.
    ca.max(cb).max(rules.min_clearance_um)
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


// =================================================================================================================================================
// The rule engine: `DRC_ENGINE::EvalRules`
// =================================================================================================================================================
//
// What follows is `pcbnew/drc/drc_engine.cpp` (loadImplicitRules, compileRules, EvalRules, EvalZoneConnection, ProcessAssertions) and the rule and constraint
// classes of `drc_rule.h` at KiCad 8303b2ad, to answer one question: which constraint wins for these items on this layer, and which rules were considered
// on the way. It is what the Clearance and Constraints Resolution reports show (`BOARD_INSPECTION_TOOL`), with KiCad's own report text, and nothing more:
// kicad-cli stays the only DRC engine, this inspects the rules it enforces.
//
// Lengths are nanometres here, KiCad's internal unit (the studio's IR is in micrometres: [`nm`] converts). The winner is "the last rule that applies":
// the implicit rules come first (board setup, net classes, keepouts), the `.kicad_dru` rules in file order after them, each match overwrites the
// fields it sets; an explicit rule's match ends the evaluation, otherwise the local overrides, the zones' own clearance and the board's minimum clearance
// have their say.

use crate::pcbexpr::{Condition, Env};
use crate::rule_item::{Class, RuleItem};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

/// The studio's micrometres as KiCad's nanometres.
pub fn nm(um: Um) -> i64 {
    um * 1000
}

/// `MAXIMUM_CLEARANCE`: 500 mm, "to prevent int-overflows".
pub const MAXIMUM_CLEARANCE: i64 = 500_000_000;

/// `std::numeric_limits<int>::max()`: what `MINOPTMAX::Max()` answers when no maximum was set.
pub const INT_MAX: i64 = 2_147_483_647;

// ------------------------------------------------------------------------------------------------------------------------------------- formatting

/// The units a report shows lengths in (`m_frame->GetUserUnits()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Mm,
    Mils,
    Inch,
}

impl Units {
    pub fn parse(s: &str) -> Units {
        match s {
            "mils" | "mil" => Units::Mils,
            "in" | "inch" | "inches" => Units::Inch,
            _ => Units::Mm,
        }
    }
}

/// C's `%.3e`: a mantissa with three decimals and an exponent of at least two digits.
fn sci3(v: f64) -> String {
    let s = format!("{v:.3e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// `removeTrailingZeros` (`common/eda_units.cpp`).
fn remove_trailing_zeros(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut len = b.len() as i64;
    let mut remove = 0usize;
    loop {
        len -= 1;
        if len > 0 && b[len as usize] == '0' {
            remove += 1;
        } else {
            break;
        }
    }
    if len >= 0 && (b[len as usize] == '.' || b[len as usize] == ',') {
        remove += 1;
    }
    b[..b.len() - remove].iter().collect()
}

/// `UNITS_PROVIDER::MessageTextFromValue( int )` of the DRC engine, which is in millimetres: `%.4f`, scientific notation when that would show only zeros.
pub fn message_text_from_value(value_nm: f64) -> String {
    let v = value_nm / 1_000_000.0;
    let mut text = format!("{v:.4}");
    if v != 0.0 && !text.chars().any(|c| ('1'..='9').contains(&c)) {
        text = sci3(v);
    }
    text + " mm"
}

/// `MessageTextFromUnscaledValue`: a count (`%.0f`).
pub fn message_text_from_unscaled(v: i64) -> String {
    format!("{v}")
}

/// `UNITS_PROVIDER::StringFromValue( value, true )` of the frame: the user's units, `%.10f` (`%.5f` mils, `%.8f` inches) without the trailing zeros.
pub fn string_from_value(value_nm: f64, units: Units, add_label: bool) -> String {
    let (v, decimals, label) = match units {
        Units::Mm => (value_nm / 1_000_000.0, 10, " mm"),
        Units::Mils => (value_nm / 25_400.0, 5, " mils"),
        Units::Inch => (value_nm / 25_400_000.0, 8, " in"),
    };
    let mut text = remove_trailing_zeros(&format!("{v:.decimals$}"));
    if v != 0.0 && (text == "0" || text == "-0") {
        text = remove_trailing_zeros(&format!("{v:.10}"));
    }
    if add_label {
        text.push_str(label);
    }
    text
}

/// `EscapeHTML`.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

// --------------------------------------------------------------------------------------------------------------------------------------- the types

/// `DRC_CONSTRAINT_T`, in KiCad's order (the numbers are the ones a rule's functions see as `aConstraint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u32)]
pub enum CType {
    Null = 0,
    Clearance,
    Creepage,
    HoleClearance,
    HoleToHole,
    EdgeClearance,
    HoleSize,
    CourtyardClearance,
    SilkClearance,
    TextHeight,
    TextThickness,
    TrackWidth,
    TrackSegmentLength,
    AnnularWidth,
    ZoneConnection,
    ThermalReliefGap,
    ThermalSpokeWidth,
    MinResolvedSpokes,
    SolderMaskExpansion,
    SolderPasteAbsMargin,
    SolderPasteRelMargin,
    Disallow,
    ViaDiameter,
    Length,
    Skew,
    DiffPairGap,
    MaxUncoupled,
    DiffPairIntraSkew,
    ViaCount,
    PhysicalClearance,
    PhysicalHoleClearance,
    Assertion,
    ConnectionWidth,
    TrackAngle,
    ViaDangling,
    BridgedMask,
    SolderMaskSliver,
}

impl CType {
    /// The keyword of a `.kicad_dru` `(constraint ..)`.
    pub fn token(self) -> &'static str {
        match self {
            CType::Null => "",
            CType::Clearance => "clearance",
            CType::Creepage => "creepage",
            CType::HoleClearance => "hole_clearance",
            CType::HoleToHole => "hole_to_hole",
            CType::EdgeClearance => "edge_clearance",
            CType::HoleSize => "hole_size",
            CType::CourtyardClearance => "courtyard_clearance",
            CType::SilkClearance => "silk_clearance",
            CType::TextHeight => "text_height",
            CType::TextThickness => "text_thickness",
            CType::TrackWidth => "track_width",
            CType::TrackSegmentLength => "track_segment_length",
            CType::AnnularWidth => "annular_width",
            CType::ZoneConnection => "zone_connection",
            CType::ThermalReliefGap => "thermal_relief_gap",
            CType::ThermalSpokeWidth => "thermal_spoke_width",
            CType::MinResolvedSpokes => "min_resolved_spokes",
            CType::SolderMaskExpansion => "solder_mask_expansion",
            CType::SolderPasteAbsMargin => "solder_paste_abs_margin",
            CType::SolderPasteRelMargin => "solder_paste_rel_margin",
            CType::Disallow => "disallow",
            CType::ViaDiameter => "via_diameter",
            CType::Length => "length",
            CType::Skew => "skew",
            CType::DiffPairGap => "diff_pair_gap",
            CType::MaxUncoupled => "diff_pair_uncoupled",
            CType::DiffPairIntraSkew => "skew",
            CType::ViaCount => "via_count",
            CType::PhysicalClearance => "physical_clearance",
            CType::PhysicalHoleClearance => "physical_hole_clearance",
            CType::Assertion => "assertion",
            CType::ConnectionWidth => "connection_width",
            CType::TrackAngle => "track_angle",
            CType::ViaDangling => "via_dangling",
            CType::BridgedMask => "bridged_mask",
            CType::SolderMaskSliver => "solder_mask_sliver",
        }
    }

    pub fn from_token(t: &str) -> Option<CType> {
        use CType::*;
        [
            Clearance, Creepage, HoleClearance, HoleToHole, EdgeClearance, HoleSize, CourtyardClearance, SilkClearance, TextHeight, TextThickness, TrackWidth, TrackSegmentLength, AnnularWidth,
            ZoneConnection, ThermalReliefGap, ThermalSpokeWidth, MinResolvedSpokes, SolderMaskExpansion, SolderPasteAbsMargin, SolderPasteRelMargin, Disallow, ViaDiameter, Length, Skew, DiffPairGap,
            MaxUncoupled, ViaCount, PhysicalClearance, PhysicalHoleClearance, Assertion, ConnectionWidth, TrackAngle, ViaDangling, BridgedMask, SolderMaskSliver,
        ]
        .into_iter()
        .find(|c| c.token() == t)
    }

    /// The constraints whose value is a clearance: `applyConstraint` caps them at [`MAXIMUM_CLEARANCE`].
    fn is_clearance(self) -> bool {
        matches!(self, CType::Clearance | CType::CourtyardClearance | CType::SilkClearance | CType::HoleClearance | CType::EdgeClearance | CType::PhysicalClearance | CType::PhysicalHoleClearance)
    }
}

/// `DRC_DISALLOW_T`.
pub mod disallow {
    pub const THROUGH_VIAS: u32 = 1 << 0;
    pub const MICRO_VIAS: u32 = 1 << 1;
    pub const BLIND_VIAS: u32 = 1 << 2;
    pub const BURIED_VIAS: u32 = 1 << 3;
    pub const TRACKS: u32 = 1 << 4;
    pub const PADS: u32 = 1 << 5;
    pub const ZONES: u32 = 1 << 6;
    pub const TEXTS: u32 = 1 << 7;
    pub const GRAPHICS: u32 = 1 << 8;
    pub const HOLES: u32 = 1 << 9;
    pub const FOOTPRINTS: u32 = 1 << 10;
}

/// `ZONE_CONNECTION` (`pcbnew/zones.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneConnection {
    Inherited,
    None,
    Thermal,
    Full,
    ThtThermal,
}

impl ZoneConnection {
    /// `PrintZoneConnection`.
    pub fn print(self) -> &'static str {
        match self {
            ZoneConnection::Inherited => "inherited",
            ZoneConnection::None => "none",
            ZoneConnection::Thermal => "thermal reliefs",
            ZoneConnection::Full => "solid",
            ZoneConnection::ThtThermal => "thermal reliefs for PTH",
        }
    }

    pub fn from_pad_connection(p: eda_model::ir::PadConnection) -> ZoneConnection {
        match p {
            eda_model::ir::PadConnection::None => ZoneConnection::None,
            eda_model::ir::PadConnection::Thermal => ZoneConnection::Thermal,
            eda_model::ir::PadConnection::Full => ZoneConnection::Full,
            eda_model::ir::PadConnection::ThtThermal => ZoneConnection::ThtThermal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Undefined,
    Ignore,
    Warning,
    Error,
    Exclusion,
}

/// `DRC_IMPLICIT_SOURCE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplicitSource {
    None,
    BoardSetup,
    BarcodeDefaults,
    Keepout,
    NetClass,
    TuningProfile,
}

/// `MINOPTMAX<int>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinOptMax {
    pub min: Option<i64>,
    pub opt: Option<i64>,
    pub max: Option<i64>,
}

impl MinOptMax {
    pub fn with_min(min: i64) -> MinOptMax {
        MinOptMax { min: Some(min), ..Default::default() }
    }

    /// `Min()`: 0 when none was set.
    pub fn min(&self) -> i64 {
        self.min.unwrap_or(0)
    }

    /// `Opt()`: the minimum when none was set.
    pub fn opt(&self) -> i64 {
        self.opt.unwrap_or_else(|| self.min())
    }

    /// `Max()`: the largest int when none was set.
    pub fn max(&self) -> i64 {
        self.max.unwrap_or(INT_MAX)
    }
}

/// `DRC_CONSTRAINT` as a rule holds it.
#[derive(Clone)]
pub struct Constraint {
    pub ctype: CType,
    pub value: MinOptMax,
    pub disallow: u32,
    pub zone_connection: ZoneConnection,
    /// `m_Test`: an assertion's expression.
    pub test: Option<Condition>,
    /// `SKEW_WITHIN_DIFF_PAIRS`, `SPACE_DOMAIN`, `TIME_DOMAIN`.
    pub options: u8,
}

impl Constraint {
    pub fn new(ctype: CType) -> Constraint {
        Constraint { ctype, value: MinOptMax::default(), disallow: 0, zone_connection: ZoneConnection::Inherited, test: None, options: 0 }
    }

    pub fn with_min(ctype: CType, min: i64) -> Constraint {
        let mut c = Constraint::new(ctype);
        c.value.min = Some(min);
        c
    }
}

/// `DRC_RULE`.
#[derive(Clone)]
pub struct Rule {
    pub name: String,
    pub implicit: ImplicitSource,
    /// `m_ImplicitItemId`: the keepout zone an implicit rule is about.
    pub implicit_item: Option<String>,
    pub layer_source: String,
    /// `m_LayerCondition`: `None` is `LSET::AllLayersMask()`.
    pub layers: Option<BTreeSet<String>>,
    pub condition: Option<Condition>,
    pub constraints: Vec<Constraint>,
    pub severity: Severity,
}

impl Rule {
    pub fn new(name: impl Into<String>) -> Rule {
        Rule { name: name.into(), implicit: ImplicitSource::None, implicit_item: None, layer_source: String::new(), layers: None, condition: None, constraints: Vec::new(), severity: Severity::Undefined }
    }

    fn implicit(name: impl Into<String>, source: ImplicitSource) -> Rule {
        let mut r = Rule::new(name);
        r.implicit = source;
        r
    }

    pub fn is_implicit(&self) -> bool {
        self.implicit != ImplicitSource::None
    }

    pub fn find_constraint(&self, t: CType) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.ctype == t)
    }
}

/// The constraint `EvalRules` settled on (`DRC_CONSTRAINT` as it is returned): its type is [`CType::Null`] when no rule gave one.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub ctype: CType,
    pub value: MinOptMax,
    pub disallow: u32,
    pub zone_connection: ZoneConnection,
    pub implicit_min: bool,
    /// The rule it came from, an index into [`RuleEngine::rules`]; none for a constraint made on the spot ("net tie", "board minimum", a local override).
    pub parent: Option<usize>,
    /// `SetName`, for the constraints made on the spot.
    pub name: String,
}

impl Resolved {
    fn new(ctype: CType) -> Resolved {
        Resolved { ctype, value: MinOptMax::default(), disallow: 0, zone_connection: ZoneConnection::Inherited, implicit_min: false, parent: None, name: String::new() }
    }

    /// `IsNull`.
    pub fn is_null(&self) -> bool {
        self.ctype == CType::Null
    }
}

/// What a rule check told, line by line, and the rules it went through.
#[derive(Default, Debug, Clone)]
pub struct Report {
    /// The report box's lines: HTML, as KiCad writes them.
    pub lines: Vec<String>,
    /// Every rule `EvalRules` considered for the constraint, in the order it did.
    pub considered: Vec<Considered>,
}

impl Report {
    pub fn line(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }
}

/// One rule `EvalRules` looked at: whether it applied and why not.
#[derive(Debug, Clone)]
pub struct Considered {
    /// `GetName()`: the implicit rule's own name or `rule 'name'`.
    pub rule: String,
    pub implicit: bool,
    pub constraint: CType,
    pub condition: String,
    pub applied: bool,
    /// The line that told why a rule did not apply, or that it did.
    pub note: String,
}

// ----------------------------------------------------------------------------------------------------------------------------------- the settings

/// The `BOARD_DESIGN_SETTINGS` the engine reads, in nanometres.
#[derive(Debug, Clone)]
pub struct Settings {
    pub track_min_width: i64,
    pub min_conn: i64,
    pub min_through_drill: i64,
    pub via_min_annular_width: i64,
    pub via_min_size: i64,
    pub hole_to_hole_min: i64,
    pub min_resolved_spokes: i64,
    pub silk_clearance: i64,
    pub min_silk_text_height: i64,
    pub min_silk_text_thickness: i64,
    pub hole_clearance: i64,
    pub copper_edge_clearance: i64,
    pub micro_vias_min_drill: i64,
    pub micro_vias_min_size: i64,
    pub min_clearance: i64,
    pub solder_mask_expansion: i64,
    pub solder_paste_margin: i64,
    pub solder_paste_margin_ratio: f64,
    /// `GetEnabledLayers()`.
    pub enabled_layers: BTreeSet<String>,
}

impl Settings {
    pub fn from_rules(r: &BoardRules) -> Settings {
        let mut enabled: BTreeSet<String> = r.layers.iter().cloned().collect();
        enabled.extend(r.layer_setup.enabled.iter().cloned());
        Settings {
            track_min_width: nm(r.track_width_min_um),
            min_conn: nm(r.min_connection_um),
            min_through_drill: nm(r.via_drill_min_um),
            via_min_annular_width: nm(r.annular_width_min_um),
            via_min_size: nm(r.via_diameter_min_um),
            hole_to_hole_min: nm(r.hole_to_hole_min_um),
            min_resolved_spokes: r.min_resolved_spokes as i64,
            silk_clearance: nm(r.silk_clearance_um),
            min_silk_text_height: nm(r.min_silk_text_height_um),
            min_silk_text_thickness: nm(r.min_silk_text_thickness_um),
            hole_clearance: nm(r.hole_clearance_um),
            copper_edge_clearance: nm(edge_clearance_min(r)),
            micro_vias_min_drill: nm(r.microvia_drill_min_um),
            micro_vias_min_size: nm(r.microvia_diameter_min_um),
            min_clearance: nm(r.min_clearance_um),
            solder_mask_expansion: nm(r.solder_mask.expansion_um),
            solder_paste_margin: nm(r.solder_mask.paste_margin_um),
            solder_paste_margin_ratio: r.solder_mask.paste_margin_ratio,
            enabled_layers: enabled,
        }
    }
}

/// A net class as `loadImplicitRules` sees it: for each setting its value and the human readable name of the class it comes from (`GetClearanceParent()` and its kin).
#[derive(Debug, Clone, Default)]
pub struct ClassView {
    /// `NETCLASS::GetName()`: what `hasExactNetclass` compares against.
    pub name: String,
    pub clearance: Option<(i64, String)>,
    pub track_width: Option<(i64, String)>,
    pub diff_pair_width: Option<(i64, String)>,
    pub diff_pair_gap: Option<(i64, String)>,
    pub via_diameter: Option<(i64, String)>,
    pub via_drill: Option<(i64, String)>,
    pub uvia_diameter: Option<(i64, String)>,
    pub uvia_drill: Option<(i64, String)>,
}

/// `NETCLASS::GetHumanReadableName` of a class made of `constituents`.
pub fn human_readable_class_name(constituents: &[String]) -> String {
    match constituents.len() {
        0 | 1 => constituents.first().cloned().unwrap_or_default(),
        2 => format!("{} and {}", constituents[0], constituents[1]),
        3 => format!("{}, {} and {}", constituents[0], constituents[1], constituents[2]),
        n => format!("{}, {} and {} more", constituents[0], constituents[1], n - 2),
    }
}

/// The net classes of the board for the implicit rules: the Default class, the classes `rules` lists (in the name order of `std::map`), then the composite classes the board's
/// `nets` are in (also by name), each setting with the class that gives it.
pub fn class_views(rules: &BoardRules, nets: &[String]) -> Vec<ClassView> {
    let def = rules.default_class.clone().unwrap_or_else(|| eda_model::NetClass {
        name: "Default".into(),
        nets: Vec::new(),
        track_width: None,
        clearance: None,
        via_diameter: None,
        via_drill: None,
        microvia_diameter: None,
        microvia_drill: None,
        diff_pair_width: None,
        diff_pair_gap: None,
        diff_pair_via_gap: None,
        priority: 0,
    });
    let default_view = ClassView {
        name: "Default".into(),
        clearance: Some((nm(rules.clearance), "Default".into())),
        track_width: Some((nm(rules.track_width), "Default".into())),
        via_diameter: Some((nm(rules.via_diameter), "Default".into())),
        via_drill: Some((nm(rules.via_drill), "Default".into())),
        uvia_diameter: Some((def.microvia_diameter.map(nm).unwrap_or(300_000), "Default".into())),
        uvia_drill: Some((def.microvia_drill.map(nm).unwrap_or(100_000), "Default".into())),
        diff_pair_width: Some((def.diff_pair_width.map(nm).unwrap_or(200_000), "Default".into())),
        diff_pair_gap: Some((def.diff_pair_gap.map(nm).unwrap_or(250_000), "Default".into())),
    };
    // A setting of the Default class, for a composite class that gets it from there.
    let setting = |cl: &ClassView, get: &dyn Fn(&ClassView) -> &Option<(i64, String)>| -> Option<(i64, String)> { get(cl).clone() };
    let own = |c: &eda_model::NetClass| ClassView {
        name: c.name.clone(),
        clearance: c.clearance.map(|v| (nm(v), c.name.clone())),
        track_width: c.track_width.map(|v| (nm(v), c.name.clone())),
        diff_pair_width: c.diff_pair_width.map(|v| (nm(v), c.name.clone())),
        diff_pair_gap: c.diff_pair_gap.map(|v| (nm(v), c.name.clone())),
        via_diameter: c.via_diameter.map(|v| (nm(v), c.name.clone())),
        via_drill: c.via_drill.map(|v| (nm(v), c.name.clone())),
        uvia_diameter: c.microvia_diameter.map(|v| (nm(v), c.name.clone())),
        uvia_drill: c.microvia_drill.map(|v| (nm(v), c.name.clone())),
    };
    let mut plain: Vec<&eda_model::NetClass> = rules.net_classes.iter().collect();
    plain.sort_by(|a, b| a.name.cmp(&b.name));
    let plain_views: Vec<ClassView> = plain.iter().map(|c| own(c)).collect();

    // The composite classes: the nets that are in more than one class (or in one class that leaves a setting to the Default class).
    let mut composites: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for net in nets {
        let eff = rules.effective_class(net);
        if eff.constituents.len() > 1 {
            composites.entry(eff.name.clone()).or_insert(eff.constituents.clone());
        }
    }
    let mut views = vec![default_view.clone()];
    views.extend(plain_views.iter().cloned());
    for (name, constituents) in composites {
        // `NETCLASS::ResolveConstituents`: each setting from the first constituent that sets it (the Default class last).
        let parts: Vec<&ClassView> = constituents.iter().filter_map(|n| if n == "Default" { Some(&default_view) } else { plain_views.iter().find(|v| &v.name == n) }).collect();
        let pick = |get: &dyn Fn(&ClassView) -> &Option<(i64, String)>| -> Option<(i64, String)> { parts.iter().find_map(|p| setting(p, get)) };
        views.push(ClassView {
            name,
            clearance: pick(&|c| &c.clearance),
            track_width: pick(&|c| &c.track_width),
            diff_pair_width: pick(&|c| &c.diff_pair_width),
            diff_pair_gap: pick(&|c| &c.diff_pair_gap),
            via_diameter: pick(&|c| &c.via_diameter),
            via_drill: pick(&|c| &c.via_drill),
            uvia_diameter: pick(&|c| &c.uvia_diameter),
            uvia_drill: pick(&|c| &c.uvia_drill),
        });
    }
    views
}

/// A keepout rule area for the implicit keepout rules (`addKeepoutZoneRule`).
#[derive(Debug, Clone)]
pub struct KeepoutRule {
    pub id: String,
    pub name: String,
    /// `DescribeRef( parentFP->GetReference() )` of a footprint's own rule area.
    pub parent_ref: Option<String>,
    pub layers: BTreeSet<String>,
    pub flags: u32,
}

// -------------------------------------------------------------------------------------------------------------------------------------- the engine

/// `DRC_ENGINE`, as far as rule evaluation goes: the implicit rules, the rules of a `.kicad_dru` and a map from each constraint type to the rules that have one.
pub struct RuleEngine {
    pub rules: Vec<Rule>,
    /// Per constraint type: `(rule, constraint)` indices, in the order the rules were added.
    map: HashMap<CType, Vec<(usize, usize)>>,
    pub settings: Settings,
    pub env: Rc<Env>,
    /// Whether the rules file made a rule of its own (`m_hasExplicitClearanceRules`).
    pub has_explicit_clearance_rules: bool,
}

impl RuleEngine {
    /// `loadImplicitRules`: the board setup constraints, the net classes' rules, the tuning profiles' and the keepout areas'.
    pub fn new(settings: Settings, env: Rc<Env>, classes: &[ClassView], keepouts: &[KeepoutRule]) -> RuleEngine {
        let mut eng = RuleEngine { rules: Vec::new(), map: HashMap::new(), settings, env, has_explicit_clearance_rules: false };
        eng.load_implicit_rules(classes, keepouts);
        eng
    }

    fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    fn load_implicit_rules(&mut self, classes: &[ClassView], keepouts: &[KeepoutRule]) {
        let bds = self.settings.clone();
        let silk_layers: BTreeSet<String> = ["F.SilkS", "B.SilkS"].iter().map(|s| s.to_string()).collect();

        // 1) global defaults
        let mut rule = Rule::implicit("board setup constraints", ImplicitSource::BoardSetup);
        for (t, v) in [
            (CType::TrackWidth, bds.track_min_width),
            (CType::ConnectionWidth, bds.min_conn),
            (CType::HoleSize, bds.min_through_drill),
            (CType::AnnularWidth, bds.via_min_annular_width),
            (CType::ViaDiameter, bds.via_min_size),
            (CType::HoleToHole, bds.hole_to_hole_min),
        ] {
            rule.constraints.push(Constraint::with_min(t, v));
        }
        self.add_rule(rule);

        let mut rule = Rule::implicit("board setup constraints zone fill strategy", ImplicitSource::BoardSetup);
        rule.constraints.push(Constraint::with_min(CType::MinResolvedSpokes, bds.min_resolved_spokes));
        self.add_rule(rule);

        for (name, t, v) in [
            ("board setup constraints silk", CType::SilkClearance, bds.silk_clearance),
            ("board setup constraints silk text height", CType::TextHeight, bds.min_silk_text_height),
            ("board setup constraints silk text thickness", CType::TextThickness, bds.min_silk_text_thickness),
        ] {
            let mut rule = Rule::implicit(name, ImplicitSource::BoardSetup);
            rule.layers = Some(silk_layers.clone());
            rule.constraints.push(Constraint::with_min(t, v));
            self.add_rule(rule);
        }

        for (name, t, v) in [
            ("board setup constraints hole", CType::HoleClearance, bds.hole_clearance),
            ("board setup constraints edge", CType::EdgeClearance, bds.copper_edge_clearance),
        ] {
            let mut rule = Rule::implicit(name, ImplicitSource::BoardSetup);
            rule.constraints.push(Constraint::with_min(t, v));
            self.add_rule(rule);
        }

        // (KiCad sets `holeToHoleConstraint`'s minimum to 0 here and adds the courtyard constraint, which has no value at all.)
        let mut rule = Rule::implicit("board setup constraints courtyard", ImplicitSource::BoardSetup);
        rule.constraints.push(Constraint::new(CType::CourtyardClearance));
        self.add_rule(rule);

        // 2a) micro-via specific defaults
        let mut rule = Rule::implicit("board setup constraints micro-via", ImplicitSource::BoardSetup);
        rule.condition = Some(Condition::new("A.Via_Type == 'Micro'"));
        rule.constraints.push(Constraint::with_min(CType::HoleSize, bds.micro_vias_min_drill));
        rule.constraints.push(Constraint::with_min(CType::ViaDiameter, bds.micro_vias_min_size));
        self.add_rule(rule);

        // 2b) barcode-specific defaults
        let mut rule = Rule::implicit("barcode visual separation default", ImplicitSource::BarcodeDefaults);
        rule.constraints.push(Constraint::with_min(CType::PhysicalClearance, 1_000_000));
        rule.condition = Some(Condition::new("A.Type == 'Barcode'"));
        self.add_rule(rule);

        // 3) per-netclass rules
        let mut clearance_rules: Vec<Rule> = Vec::new();
        let mut item_rules: Vec<Rule> = Vec::new();
        for nc in classes {
            let nc_name = nc.name.replace('\'', "\\'");
            let netclass_rule = |parent: &str, suffix: &str, cond: String| {
                let mut r = Rule::implicit(format!("netclass '{parent}'{suffix}"), ImplicitSource::NetClass);
                r.condition = Some(Condition::new(&cond));
                r
            };
            if let Some((v, parent)) = &nc.clearance {
                let mut r = netclass_rule(parent, "", format!("A.hasExactNetclass('{nc_name}')"));
                r.constraints.push(Constraint::with_min(CType::Clearance, *v));
                clearance_rules.push(r);
            }
            if let Some((v, parent)) = &nc.track_width {
                let mut r = netclass_rule(parent, "", format!("A.hasExactNetclass('{nc_name}')"));
                let mut c = Constraint::with_min(CType::TrackWidth, bds.track_min_width);
                c.value.opt = Some(*v);
                r.constraints.push(c);
                clearance_rules.push(r);
            }
            if let Some((v, parent)) = &nc.diff_pair_width {
                let mut r = netclass_rule(parent, " (diff pair)", format!("A.hasExactNetclass('{nc_name}') && A.inDiffPair('*')"));
                let mut c = Constraint::with_min(CType::TrackWidth, bds.track_min_width);
                c.value.opt = Some(*v);
                r.constraints.push(c);
                item_rules.push(r);
            }
            if let Some((gap, parent)) = &nc.diff_pair_gap {
                let mut r = netclass_rule(parent, " (diff pair)", format!("A.hasExactNetclass('{nc_name}')"));
                let mut c = Constraint::with_min(CType::DiffPairGap, bds.min_clearance);
                c.value.opt = Some(*gap);
                r.constraints.push(c);
                item_rules.push(r);
                // A narrower diffpair gap overrides the netclass min clearance
                if nc.clearance.as_ref().is_some_and(|(cl, _)| gap < cl) {
                    let mut r = netclass_rule(parent, " (diff pair)", format!("A.hasExactNetclass('{nc_name}') && AB.isCoupledDiffPair()"));
                    r.constraints.push(Constraint::with_min(CType::Clearance, *gap));
                    item_rules.push(r);
                }
            }
            if let Some((v, parent)) = &nc.via_diameter {
                let mut r = netclass_rule(parent, "", format!("A.hasExactNetclass('{nc_name}') && A.Via_Type != 'Micro'"));
                let mut c = Constraint::with_min(CType::ViaDiameter, bds.via_min_size);
                c.value.opt = Some(*v);
                r.constraints.push(c);
                item_rules.push(r);
            }
            if let Some((v, parent)) = &nc.via_drill {
                let mut r = netclass_rule(parent, "", format!("A.hasExactNetclass('{nc_name}') && A.Via_Type != 'Micro'"));
                let mut c = Constraint::with_min(CType::HoleSize, bds.min_through_drill);
                c.value.opt = Some(*v);
                r.constraints.push(c);
                item_rules.push(r);
            }
            if let Some((v, parent)) = &nc.uvia_diameter {
                let mut r = netclass_rule(parent, " (uvia)", format!("A.hasExactNetclass('{nc_name}') && A.Via_Type == 'Micro'"));
                // (KiCad sets the minimum to the micro-via floor and then overwrites it with the class's own diameter.)
                r.constraints.push(Constraint::with_min(CType::ViaDiameter, *v));
                item_rules.push(r);
            }
            if let Some((v, parent)) = &nc.uvia_drill {
                let mut r = netclass_rule(parent, " (uvia)", format!("A.hasExactNetclass('{nc_name}') && A.Via_Type == 'Micro'"));
                let mut c = Constraint::with_min(CType::HoleSize, bds.micro_vias_min_drill);
                c.value.opt = Some(*v);
                r.constraints.push(c);
                item_rules.push(r);
            }
        }
        // The netclass rules are sorted by their first constraint's minimum so that the right one fires when `A` and `B` are in two classes.
        clearance_rules.sort_by_key(|r| r.constraints[0].value.min());
        for r in clearance_rules.into_iter().chain(item_rules) {
            self.add_rule(r);
        }

        // 5) keepout area rules
        for k in keepouts {
            let name = match (k.name.is_empty(), &k.parent_ref) {
                (true, Some(r)) => format!("keepout area of {r}"),
                (true, None) => "keepout area".to_string(),
                (false, Some(r)) => format!("keepout area '{}' of {r}", k.name),
                (false, None) => format!("keepout area '{}'", k.name),
            };
            let mut rule = Rule::implicit(name, ImplicitSource::Keepout);
            rule.implicit_item = Some(k.id.clone());
            rule.condition = Some(Condition::new(&format!("A.intersectsArea('{}')", k.id)));
            rule.layers = Some(k.layers.clone());
            let mut c = Constraint::new(CType::Disallow);
            c.disallow = k.flags;
            rule.constraints.push(c);
            self.add_rule(rule);
        }
    }

    /// `loadRules` and `compileRules`: the rules of a `.kicad_dru` are added after the implicit ones and every condition is compiled. The error is the first thing KiCad's
    /// `compileRules` throws on; the engine then keeps only the implicit rules, as `InitEngine` does.
    pub fn add_explicit(&mut self, rules: Vec<Rule>) -> Result<(), crate::dru::ParseError> {
        let before = self.rules.len();
        self.rules.extend(rules);
        let mut failure = None;
        for r in &mut self.rules[before..] {
            if let Some(cond) = &mut r.condition {
                if !cond.expression().is_empty() {
                    cond.compile(&self.env);
                    if !cond.compile_errors().is_empty() {
                        failure = Some(crate::dru::ParseError { message: "Parse error".to_string(), line: 0, offset: 0, rule: r.name.clone(), source: cond.expression().to_string() });
                        break;
                    }
                }
            }
        }
        if let Some(e) = failure {
            self.rules.truncate(before);
            self.compile_implicit();
            return Err(e);
        }
        self.compile_implicit();
        Ok(())
    }

    /// An engine for a board: the implicit rules and, when there is a rules file, its rules. A file that does not parse or compile leaves the implicit rules alone and
    /// comes back as the error (`BOARD_INSPECTION_TOOL::makeDRCEngine`'s `compileError`).
    pub fn with_rules_file(settings: Settings, env: Rc<Env>, classes: &[ClassView], keepouts: &[KeepoutRule], dru: Option<&str>) -> (RuleEngine, Option<crate::dru::ParseError>) {
        let mut eng = RuleEngine::new(settings, env, classes, keepouts);
        let mut error = None;
        if let Some(text) = dru.filter(|t| !t.trim().is_empty()) {
            match crate::dru::parse(text, &eng.env) {
                Ok(parsed) => {
                    if let Err(e) = eng.add_explicit(parsed.rules) {
                        error = Some(e);
                    }
                }
                Err(e) => {
                    eng.compile_implicit();
                    error = Some(e);
                }
            }
        } else {
            eng.compile_implicit();
        }
        (eng, error)
    }

    /// Compile every implicit rule's condition and build the map from a constraint type to its rules (`compileRules`).
    pub fn compile_implicit(&mut self) {
        for r in &mut self.rules {
            if let Some(cond) = &mut r.condition {
                if !cond.is_compiled() && !cond.expression().is_empty() {
                    cond.compile(&self.env);
                }
            }
        }
        self.build_map();
    }

    fn build_map(&mut self) {
        self.map.clear();
        self.has_explicit_clearance_rules = false;
        for (ri, rule) in self.rules.iter().enumerate() {
            for (ci, c) in rule.constraints.iter().enumerate() {
                self.map.entry(c.ctype).or_default().push((ri, ci));
                if !rule.is_implicit() && c.ctype == CType::Clearance {
                    self.has_explicit_clearance_rules = true;
                }
            }
        }
    }

    /// `HasRulesForConstraintType`.
    pub fn has_rules_for(&self, t: CType) -> bool {
        self.map.get(&t).is_some_and(|v| !v.is_empty())
    }

    /// `DRC_CONSTRAINT::GetName`: an implicit rule's own name, `rule 'name'` for the rules of the file, what `SetName` said for a constraint made on the spot.
    pub fn constraint_name(&self, c: &Resolved) -> String {
        match c.parent {
            Some(i) => {
                let r = &self.rules[i];
                if r.is_implicit() {
                    r.name.clone()
                } else {
                    format!("rule '{}'", r.name)
                }
            }
            None => c.name.clone(),
        }
    }

    /// `DRC_ENGINE::EvalRules`: the constraint of type `ctype` that holds for the items `a` and `b` on `layer` (none for `UNDEFINED_LAYER`). With a `report` it tells what it
    /// checked, the way the Clearance and Constraints Resolution dialogs do.
    pub fn eval_rules(&self, ctype: CType, a: Option<&Rc<RuleItem>>, b: Option<&Rc<RuleItem>>, layer: Option<&str>, report: Option<&mut Report>) -> Resolved {
        let mut ev = Eval::new(self, ctype, a, b, layer, report);
        ev.run()
    }

    /// `DRC_ENGINE::EvalZoneConnection`.
    pub fn eval_zone_connection(&self, a: &Rc<RuleItem>, b: &Rc<RuleItem>, layer: Option<&str>, mut report: Option<&mut Report>) -> Resolved {
        let mut constraint = self.eval_rules(CType::ZoneConnection, Some(a), Some(b), layer, report.as_deref_mut());
        if let Some(r) = report.as_deref_mut() {
            r.line("");
            r.line(format!("Resolved zone connection type: {}.", constraint.zone_connection.print()));
        }
        if constraint.zone_connection == ZoneConnection::ThtThermal {
            let pad = if a.class == Class::Pad { Some(a) } else if b.class == Class::Pad { Some(b) } else { None };
            if pad.is_some_and(|p| p.pad_attribute == "PTH") {
                constraint.zone_connection = ZoneConnection::Thermal;
            } else {
                if let Some(r) = report.as_deref_mut() {
                    r.line(format!("Pad is not a through hole pad; connection will be: {}.", ZoneConnection::Full.print()));
                }
                constraint.zone_connection = ZoneConnection::Full;
            }
        }
        constraint
    }

    /// `DRC_ENGINE::ProcessAssertions`: every assertion rule that applies to `a`, tested. `failed` is called with the rule's name for each that fails.
    pub fn process_assertions(&self, a: &Rc<RuleItem>, mut report: Option<&mut Report>, mut failed: impl FnMut(&str)) {
        let Some(entries) = self.map.get(&CType::Assertion) else { return };
        let layer = a.layer.as_deref();
        let wants = report.is_some();
        for &(ri, ci) in entries {
            let rule = &self.rules[ri];
            let c = &rule.constraints[ci];
            let name = format!("rule '{}'", rule.name);
            let mut lines: Vec<String> = vec![String::new(), format!("Checking {name}.")];
            let layers_hit = match &rule.layers {
                None => !a.layers.is_empty(),
                Some(set) => a.layers.iter().any(|l| set.contains(l)),
            };
            if !layers_hit {
                lines.push(format!("Rule layer '{}' not matched; rule ignored.", escape_html(&rule.layer_source)));
            }
            let mut applies = false;
            match &rule.condition {
                Some(cond) if !cond.expression().is_empty() => {
                    lines.push(format!("Checking rule condition '{}'.", escape_html(cond.expression())));
                    let mut msgs = Vec::new();
                    let hit = cond.evaluate_for(&self.env, Some(a), None, c.ctype as u32, layer, wants.then_some(&mut msgs));
                    lines.extend(msgs);
                    if hit {
                        lines.push("Rule applied.".to_string());
                        applies = true;
                    } else {
                        lines.push("Condition not satisfied; rule ignored.".to_string());
                    }
                }
                _ => {
                    lines.push("Unconditional rule applied.".to_string());
                    applies = true;
                }
            }
            if applies {
                if let Some(t) = &c.test {
                    lines.push(format!("Checking rule assertion '{}'.", escape_html(t.expression())));
                    let mut msgs = Vec::new();
                    let ok = t.evaluate_for(&self.env, Some(a), None, c.ctype as u32, layer, wants.then_some(&mut msgs));
                    lines.extend(msgs);
                    if ok {
                        lines.push("Assertion passed.".to_string());
                    } else {
                        lines.push(escape_html("--> Assertion failed. <--"));
                        failed(&name);
                    }
                }
            }
            if let Some(r) = report.as_deref_mut() {
                r.lines.extend(lines);
            }
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------------------------- EvalRules

struct Eval<'e, 'r> {
    eng: &'e RuleEngine,
    ctype: CType,
    a: Option<Rc<RuleItem>>,
    b: Option<Rc<RuleItem>>,
    layer: Option<String>,
    report: Option<&'r mut Report>,
    constraint: Resolved,
    a_non_copper: bool,
    b_non_copper: bool,
}

fn is_keepout(item: &RuleItem) -> bool {
    item.class == Class::Zone && item.keepout
}

/// `LSET::FrontMask()` / `BackMask()`: the layers of one side.
fn side_layers(front: bool) -> Vec<&'static str> {
    if front {
        vec!["F.Cu", "F.Adhes", "F.Paste", "F.SilkS", "F.Mask", "F.CrtYd", "F.Fab"]
    } else {
        vec!["B.Cu", "B.Adhes", "B.Paste", "B.SilkS", "B.Mask", "B.CrtYd", "B.Fab"]
    }
}

impl<'e, 'r> Eval<'e, 'r> {
    fn new(eng: &'e RuleEngine, ctype: CType, a: Option<&Rc<RuleItem>>, b: Option<&Rc<RuleItem>>, layer: Option<&str>, report: Option<&'r mut Report>) -> Self {
        let a_non_copper = a.is_some_and(|a| !a.on_copper() || is_keepout(a));
        let b_non_copper = b.is_some_and(|b| !b.on_copper() || is_keepout(b));
        Eval { eng, ctype, a: a.cloned(), b: b.cloned(), layer: layer.map(String::from), report, constraint: Resolved::new(ctype), a_non_copper, b_non_copper }
    }

    fn say(&mut self, s: impl FnOnce() -> String) {
        if let Some(r) = self.report.as_deref_mut() {
            r.lines.push(s());
        }
    }

    fn describe(item: &Option<Rc<RuleItem>>) -> String {
        item.as_ref().map(|i| i.description.clone()).unwrap_or_default()
    }

    fn parent_is_explicit(&self) -> bool {
        self.constraint.parent.is_some_and(|i| !self.eng.rules[i].is_implicit())
    }

    /// `applyConstraint`.
    fn apply(&mut self, ri: usize, ci: usize) {
        let rule = &self.eng.rules[ri];
        let c = &rule.constraints[ci];
        if c.value.min.is_some() {
            self.constraint.implicit_min = rule.is_implicit();
            self.constraint.value.min = c.value.min;
        }
        if c.value.opt.is_some() {
            self.constraint.value.opt = c.value.opt;
        }
        if c.value.max.is_some() {
            self.constraint.value.max = c.value.max;
        }
        if c.ctype.is_clearance() && self.constraint.value.min() > MAXIMUM_CLEARANCE {
            self.constraint.value.min = Some(MAXIMUM_CLEARANCE);
        }
        // While the expectation would be to OR the disallow flags, they are already masked down to the item's type: only a boolean is wanted here.
        self.constraint.disallow = c.disallow;
        self.constraint.zone_connection = c.zone_connection;
        self.constraint.parent = Some(ri);
    }

    fn run(&mut self) -> Resolved {
        let eng = self.eng;
        let ctype = self.ctype;
        let (a, b) = (self.a.clone(), self.b.clone());
        let ac = a.as_ref().filter(|x| x.is_connected()).cloned();
        let bc = b.as_ref().filter(|x| x.is_connected()).cloned();

        let mut pad: Option<Rc<RuleItem>> = None;
        let mut zone: Option<Rc<RuleItem>> = None;
        let mut parent_footprint: Option<Rc<RuleItem>> = None;
        if matches!(ctype, CType::ZoneConnection | CType::ThermalReliefGap | CType::ThermalSpokeWidth | CType::SolderMaskExpansion | CType::SolderPasteAbsMargin | CType::SolderPasteRelMargin) {
            for it in [&a, &b].into_iter().flatten() {
                if it.class == Class::Pad {
                    pad = Some(it.clone());
                } else if it.class == Class::Zone {
                    zone = Some(it.clone());
                }
            }
            if let Some(p) = &pad {
                parent_footprint = p.parent.clone();
            }
        }

        // Handle footprint net ties, which zero out the clearance for footprint objects.
        let footprints: [Option<Rc<RuleItem>>; 2] = [a.as_ref().and_then(|x| x.parent.clone()), b.as_ref().and_then(|x| x.parent.clone())];
        let same_footprint = match (&footprints[0], &footprints[1]) {
            (Some(x), Some(y)) => Rc::ptr_eq(x, y) || x.id == y.id,
            _ => false,
        };
        if ctype == CType::Clearance && ((ac.is_none() != bc.is_none()) || same_footprint) && !self.a_non_copper && !self.b_non_copper {
            let child_items = [&a, &b];
            // These are the items being compared against, so the order is reversed.
            let alt_items = [&bc, &ac];
            for ii in 0..2 {
                let (Some(fp), Some(alt)) = (&footprints[ii], alt_items[ii]) else { continue };
                let Some(child) = child_items[ii] else { continue };
                if child.net_tie.iter().any(|n| *n == alt.net) {
                    let desc = escape_html(&fp.description);
                    self.say(|| String::new());
                    self.say(|| format!("Net tie on {desc}; clearance: 0."));
                    self.constraint.parent = None;
                    self.constraint.name = "net tie".to_string();
                    self.constraint.value.min = Some(0);
                    return self.constraint.clone();
                }
            }
        }

        // Local overrides take precedence over everything *except* board min clearance.
        if ctype == CType::Clearance || ctype == CType::HoleClearance {
            let mut override_val = 0i64;
            let mut msg = String::new();
            let override_a = if ac.is_some() && !self.b_non_copper { ac.as_ref().and_then(|x| x.overrides.clearance.clone()) } else { None };
            let override_b = if bc.is_some() && !self.a_non_copper { bc.as_ref().and_then(|x| x.overrides.clearance.clone()) } else { None };
            if override_a.is_some() || override_b.is_some() {
                if let Some((v, src)) = &override_a {
                    let (d, t) = (escape_html(&Self::describe(&a)), message_text_from_value(*v as f64));
                    self.say(|| String::new());
                    self.say(|| format!("Local override on {d}; clearance: {t}."));
                    override_val = *v;
                    msg = src.clone();
                }
                if let Some((v, src)) = &override_b {
                    let (d, t) = (escape_html(&Self::describe(&b)), message_text_from_value(*v as f64));
                    self.say(|| String::new());
                    self.say(|| format!("Local override on {d}; clearance: {t}."));
                    if *v > override_val {
                        override_val = *v;
                        msg = src.clone();
                    }
                }
                if override_val != 0 {
                    if ctype == CType::Clearance {
                        if override_val < eng.settings.min_clearance {
                            override_val = eng.settings.min_clearance;
                            msg = "board minimum".to_string();
                            let t = message_text_from_value(override_val as f64);
                            self.say(|| String::new());
                            self.say(|| format!("Board minimum clearance: {t}."));
                        }
                    } else if override_val < eng.settings.hole_clearance {
                        override_val = eng.settings.hole_clearance;
                        msg = "board minimum hole".to_string();
                        let t = message_text_from_value(override_val as f64);
                        self.say(|| String::new());
                        self.say(|| format!("Board minimum hole clearance: {t}."));
                    }
                    self.constraint.parent = None;
                    self.constraint.name = msg;
                    self.constraint.value.min = Some(override_val);
                    return self.constraint.clone();
                }
            }
        } else if ctype == CType::ZoneConnection {
            if let Some(p) = &pad {
                if let Some((zc, src)) = &p.overrides.zone_connection {
                    if *zc != ZoneConnection::Inherited {
                        let (d, t) = (escape_html(&p.description), zc.print());
                        self.say(|| String::new());
                        self.say(|| format!("Local override on {d}; zone connection: {t}."));
                        self.constraint.parent = None;
                        self.constraint.name = src.clone();
                        self.constraint.zone_connection = *zc;
                        return self.constraint.clone();
                    }
                }
            }
        } else if ctype == CType::ThermalReliefGap {
            if let Some(p) = &pad {
                if let Some((gap, src)) = p.overrides.thermal_gap.clone().filter(|(g, _)| *g > 0) {
                    let (d, t) = (escape_html(&p.description), message_text_from_value(gap as f64));
                    self.say(|| String::new());
                    self.say(|| format!("Local override on {d}; thermal relief gap: {t}."));
                    self.constraint.parent = None;
                    self.constraint.name = src;
                    self.constraint.value.min = Some(gap);
                    return self.constraint.clone();
                }
            }
        } else if ctype == CType::ThermalSpokeWidth {
            if let Some(p) = &pad {
                if let Some((mut spoke, src)) = p.overrides.spoke_width.clone().filter(|(g, _)| *g > 0) {
                    let (d, t) = (escape_html(&p.description), message_text_from_value(spoke as f64));
                    self.say(|| String::new());
                    self.say(|| format!("Local override on {d}; thermal spoke width: {t}."));
                    if let Some(z) = &zone {
                        if z.zone_min_thickness > spoke {
                            spoke = z.zone_min_thickness;
                            let (d, t) = (escape_html(&z.description), message_text_from_value(spoke as f64));
                            self.say(|| String::new());
                            self.say(|| format!("{d} min thickness: {t}."));
                        }
                    }
                    self.constraint.parent = None;
                    self.constraint.name = src;
                    self.constraint.value.min = Some(spoke);
                    return self.constraint.clone();
                }
            }
        } else if ctype == CType::SolderMaskExpansion {
            let mut over = None;
            let mut over_item = a.clone();
            if let Some(p) = &pad {
                over = p.overrides.solder_mask_margin;
            } else if let Some(x) = &a {
                if matches!(x.class, Class::Shape | Class::Track | Class::Arc | Class::Via) {
                    over = x.overrides.solder_mask_margin;
                }
            }
            if over.is_none() {
                if let Some(p) = &pad {
                    if let Some(f) = &p.parent {
                        over = f.overrides.solder_mask_margin;
                        over_item = Some(f.clone());
                    }
                }
            }
            if let Some(v) = over {
                let (d, t) = (escape_html(&Self::describe(&over_item)), message_text_from_value(v as f64));
                self.say(|| String::new());
                self.say(|| format!("Local override on {d}; solder mask expansion: {t}."));
                self.constraint.value.opt = Some(v);
                return self.constraint.clone();
            }
        } else if ctype == CType::SolderPasteAbsMargin {
            let mut over = None;
            let mut over_item = a.clone();
            if let Some(p) = &pad {
                over = p.overrides.solder_paste_margin;
                if over.is_none() {
                    if let Some(f) = &p.parent {
                        over = f.overrides.solder_paste_margin;
                        over_item = Some(f.clone());
                    }
                }
            }
            if let Some(v) = over {
                let (d, t) = (escape_html(&Self::describe(&over_item)), message_text_from_value(v as f64));
                self.say(|| String::new());
                self.say(|| format!("Local override on {d}; solder paste absolute clearance: {t}."));
                self.constraint.value.opt = Some(v);
                return self.constraint.clone();
            }
        } else if ctype == CType::SolderPasteRelMargin {
            let mut over = None;
            let mut over_item = a.clone();
            if let Some(p) = &pad {
                over = p.overrides.solder_paste_ratio;
                if over.is_none() {
                    if let Some(f) = &p.parent {
                        over = f.overrides.solder_paste_ratio;
                        over_item = Some(f.clone());
                    }
                }
            }
            if let Some(v) = over {
                let (d, t) = (escape_html(&Self::describe(&over_item)), message_text_from_value(v * 100.0));
                self.say(|| String::new());
                self.say(|| format!("Local override on {d}; solder paste relative clearance: {t}."));
                self.constraint.value.opt = Some((v * 1000.0).round() as i64);
                return self.constraint.clone();
            }
        }

        if let Some(entries) = eng.map.get(&ctype) {
            for &(ri, ci) in entries {
                self.process_constraint(ri, ci);
            }
        }

        if self.constraint.parent.is_some() && self.parent_is_explicit() {
            return self.constraint.clone();
        }

        // Special case for properties which can be inherited from parent footprints. We've already checked for local overrides, and there were no rules targetting the item
        // itself, so we know we're inheriting and need to see if there are any rules targetting the parent footprint.
        if let (Some(p), Some(pf)) = (&pad, &parent_footprint) {
            if matches!(ctype, CType::ZoneConnection | CType::ThermalReliefGap | CType::ThermalSpokeWidth | CType::SolderMaskExpansion | CType::SolderPasteAbsMargin | CType::SolderPasteRelMargin) {
                let d = escape_html(&pf.description);
                self.say(|| String::new());
                self.say(|| format!("Inheriting from parent: {d}."));
                if self.a.as_ref().is_some_and(|x| Rc::ptr_eq(x, p)) {
                    self.a = Some(pf.clone());
                } else {
                    self.b = Some(pf.clone());
                }
                if let Some(entries) = eng.map.get(&ctype) {
                    for &(ri, ci) in entries {
                        self.process_constraint(ri, ci);
                    }
                    if self.constraint.parent.is_some() && self.parent_is_explicit() {
                        return self.constraint.clone();
                    }
                }
                // Found nothing again? Return the defaults.
                let s = &eng.settings;
                let default = match ctype {
                    CType::SolderMaskExpansion => Some(s.solder_mask_expansion),
                    CType::SolderPasteAbsMargin => Some(s.solder_paste_margin),
                    CType::SolderPasteRelMargin => Some((s.solder_paste_margin_ratio * 1000.0).round() as i64),
                    _ => None,
                };
                if let Some(v) = default {
                    self.constraint.parent = None;
                    self.constraint.name = "board setup".to_string();
                    self.constraint.value.opt = Some(v);
                    return self.constraint.clone();
                }
            }
        }

        // Unfortunately implicit rules don't work for local clearances (such as zones) because they have to be max'ed with netclass values (which are already implicit rules),
        // and our rule selection paradigm is "winner takes all".
        if ctype == CType::Clearance {
            let global = self.constraint.value.min();
            let mut clearance = global;
            let mut need_blank_line = true;
            for (ci, item, item_c) in [(0, &a, &ac), (1, &b, &bc)] {
                let _ = ci;
                let Some(ic) = item_c else { continue };
                // a rule area has no clearance of its own
                let Some((local, src)) = ic.local_clearance.clone() else { continue };
                if need_blank_line {
                    self.say(|| String::new());
                    need_blank_line = false;
                }
                let (d, t) = (escape_html(&Self::describe(item)), message_text_from_value(local as f64));
                self.say(|| format!("Local clearance on {d}: {t}."));
                if local > clearance {
                    clearance = local;
                    self.constraint.parent = None;
                    self.constraint.name = src;
                    self.constraint.value.min = Some(clearance);
                }
            }
            if !self.a_non_copper && !self.b_non_copper {
                if need_blank_line {
                    self.say(|| String::new());
                }
                let t = message_text_from_value(eng.settings.min_clearance as f64);
                self.say(|| format!("Board minimum clearance: {t}."));
                if clearance < eng.settings.min_clearance {
                    self.constraint.parent = None;
                    self.constraint.name = "board minimum".to_string();
                    self.constraint.value.min = Some(eng.settings.min_clearance);
                }
            }
            return self.constraint.clone();
        } else if ctype == CType::DiffPairGap {
            let t = message_text_from_value(eng.settings.min_clearance as f64);
            self.say(|| String::new());
            self.say(|| format!("Board minimum clearance: {t}."));
            if self.constraint.value.min() < eng.settings.min_clearance {
                self.constraint.parent = None;
                self.constraint.name = "board minimum".to_string();
                self.constraint.value.min = Some(eng.settings.min_clearance);
            }
            return self.constraint.clone();
        } else if ctype == CType::ZoneConnection {
            if let (Some(_), Some(pf)) = (&pad, &parent_footprint) {
                if let Some((local, _)) = pf.overrides.zone_connection.clone().filter(|(z, _)| *z != ZoneConnection::Inherited) {
                    let (d, t) = (escape_html(&pf.description), local.print());
                    self.say(|| String::new());
                    self.say(|| format!("{d} zone connection: {t}."));
                    self.constraint.parent = None;
                    self.constraint.name = "footprint".to_string();
                    self.constraint.zone_connection = local;
                    return self.constraint.clone();
                }
            }
            if let Some(z) = &zone {
                let local = z.zone_pad_connection.unwrap_or(ZoneConnection::Inherited);
                let (d, t) = (escape_html(&z.description), local.print());
                self.say(|| String::new());
                self.say(|| format!("{d} pad connection: {t}."));
                self.constraint.parent = None;
                self.constraint.name = "zone".to_string();
                self.constraint.zone_connection = local;
                return self.constraint.clone();
            }
        } else if ctype == CType::ThermalReliefGap {
            if let Some(z) = &zone {
                let (d, t) = (escape_html(&z.description), message_text_from_value(z.zone_thermal_gap as f64));
                self.say(|| String::new());
                self.say(|| format!("{d} thermal relief gap: {t}."));
                self.constraint.parent = None;
                self.constraint.name = "zone".to_string();
                self.constraint.value.min = Some(z.zone_thermal_gap);
                return self.constraint.clone();
            }
        } else if ctype == CType::ThermalSpokeWidth {
            if let Some(z) = &zone {
                let (d, t) = (escape_html(&z.description), message_text_from_value(z.zone_spoke_width as f64));
                self.say(|| String::new());
                self.say(|| format!("{d} thermal spoke width: {t}."));
                self.constraint.parent = None;
                self.constraint.name = "zone".to_string();
                self.constraint.value.min = Some(z.zone_spoke_width);
                return self.constraint.clone();
            }
        }

        if self.constraint.parent.is_none() {
            self.constraint.ctype = CType::Null;
            self.constraint.disallow = 0;
        }
        self.constraint.clone()
    }

    /// `processConstraint`: one rule's constraint of the type being asked for.
    fn process_constraint(&mut self, ri: usize, ci: usize) {
        let eng = self.eng;
        let rule = &eng.rules[ri];
        let c = &rule.constraints[ci];
        let implicit = rule.is_implicit();
        let cname = if implicit { rule.name.clone() } else { format!("rule '{}'", rule.name) };
        let esc = escape_html(&cname);
        let (applied, note) = self.consider(ri, ci, implicit, &esc);
        if let Some(r) = self.report.as_deref_mut() {
            r.considered.push(Considered {
                rule: cname,
                implicit,
                constraint: c.ctype,
                condition: rule.condition.as_ref().map(|x| x.expression().to_string()).unwrap_or_default(),
                applied,
                note,
            });
        }
    }

    /// The body of `processConstraint`: whether the rule applied, and the line that said why not (or that it did).
    fn consider(&mut self, ri: usize, ci: usize, implicit: bool, name: &str) -> (bool, String) {
        let eng = self.eng;
        let rule = &eng.rules[ri];
        let c = &rule.constraints[ci];
        let (a, b) = (self.a.clone(), self.b.clone());
        let mv = |v: i64| message_text_from_value(v as f64);
        let undefined = || "<i>undefined</i>".to_string();
        self.say(|| String::new());

        let min = mv(c.value.min());
        let opt = mv(c.value.opt());
        let max = mv(c.value.max());
        let line = match c.ctype {
            CType::Clearance | CType::CourtyardClearance | CType::SilkClearance | CType::HoleClearance | CType::EdgeClearance | CType::PhysicalClearance | CType::PhysicalHoleClearance => format!("Checking {name} clearance: {min}."),
            CType::Creepage => format!("Checking {name} creepage: {min}."),
            CType::MaxUncoupled => format!("Checking {name} max uncoupled length: {max}."),
            CType::Skew => format!("Checking {name} max skew: {max}."),
            CType::ThermalReliefGap => format!("Checking {name} gap: {min}."),
            CType::ThermalSpokeWidth => format!("Checking {name} thermal spoke width: {opt}."),
            CType::SolderMaskExpansion => format!("Checking {name} solder mask expansion: {opt}."),
            CType::SolderPasteAbsMargin => format!("Checking {name} solder paste absolute clearance: {opt}."),
            CType::SolderPasteRelMargin => format!("Checking {name} solder paste relative clearance: {opt}."),
            CType::MinResolvedSpokes => format!("Checking {name} min spoke count: {}.", message_text_from_unscaled(c.value.min())),
            CType::ZoneConnection => format!("Checking {name} zone connection: {}.", c.zone_connection.print()),
            CType::TrackWidth | CType::AnnularWidth | CType::ViaDiameter | CType::HoleSize | CType::TextHeight | CType::TextThickness | CType::DiffPairGap | CType::Length | CType::ConnectionWidth | CType::HoleToHole => {
                if implicit {
                    match c.ctype {
                        CType::TrackWidth if c.value.opt.is_some() => format!("Checking {name} track width: opt {opt}."),
                        CType::TrackWidth if c.value.min.is_some() => format!("Checking {name} track width: min {min}."),
                        CType::TrackWidth => String::new(),
                        CType::AnnularWidth => format!("Checking {name} annular width: min {opt}."),
                        CType::ViaDiameter if c.value.opt.is_some() => format!("Checking {name} via diameter: opt {opt}."),
                        CType::ViaDiameter if c.value.min.is_some() => format!("Checking {name} via diameter: min {min}."),
                        CType::ViaDiameter => String::new(),
                        CType::HoleSize if c.value.opt.is_some() => format!("Checking {name} hole size: opt {opt}."),
                        CType::HoleSize if c.value.min.is_some() => format!("Checking {name} hole size: min {min}."),
                        CType::HoleSize => String::new(),
                        CType::TextHeight | CType::TextThickness | CType::ConnectionWidth => format!("Checking {name}: min {min}."),
                        CType::DiffPairGap if c.value.opt.is_some() => format!("Checking {name} diff pair gap: opt {opt}."),
                        CType::DiffPairGap if c.value.min.is_some() => format!("Checking {name} clearance: min {min}."),
                        CType::DiffPairGap => String::new(),
                        CType::HoleToHole => format!("Checking {name} hole to hole: min {min}."),
                        _ => format!("Checking {name}."),
                    }
                } else {
                    let part = |has: bool, s: String| if has { s } else { undefined() };
                    format!(
                        "Checking {name}: min {}; opt {}; max {}.",
                        part(c.value.min.is_some(), min.clone()),
                        part(c.value.opt.is_some(), opt.clone()),
                        part(c.value.max.is_some(), max.clone())
                    )
                }
            }
            _ => format!("Checking {name}."),
        };
        // A line KiCad's switch leaves unsaid (an implicit width with neither an optimum nor a minimum) is not reported at all.
        if !line.is_empty() {
            self.say(|| line);
        }

        if c.ctype == CType::Clearance {
            if self.a_non_copper || self.b_non_copper {
                let msg = if implicit {
                    "Netclass clearances apply only between copper items.".to_string()
                } else if self.a_non_copper {
                    format!("{} contains no copper.  Rule ignored.", escape_html(&Self::describe(&a)))
                } else {
                    format!("{} contains no copper.  Rule ignored.", escape_html(&Self::describe(&b)))
                };
                self.say(|| msg.clone());
                return (false, msg);
            }
        } else if c.ctype == CType::Disallow {
            let Some(a) = &a else { return (false, String::new()) };
            let mask = match a.class {
                Class::Via => match a.via_type {
                    Some("Micro") => disallow::MICRO_VIAS,
                    Some("Blind") => disallow::BLIND_VIAS,
                    Some("Buried") => disallow::BURIED_VIAS,
                    _ => disallow::THROUGH_VIAS,
                },
                Class::Track | Class::Arc => disallow::TRACKS,
                Class::Pad => disallow::PADS,
                Class::Footprint => disallow::FOOTPRINTS,
                Class::Shape => disallow::GRAPHICS,
                Class::Field | Class::Text => disallow::TEXTS,
                Class::Zone => {
                    if a.teardrop {
                        disallow::TRACKS
                    } else {
                        disallow::ZONES
                    }
                }
                _ => 0,
            };
            if c.disallow & mask == 0 {
                let msg = if implicit { "Keepout constraint not met." } else { "Disallow constraint not met." };
                self.say(|| msg.to_string());
                return (false, msg.to_string());
            }
            let mut item_layers: BTreeSet<String> = a.layers.iter().cloned().collect();
            if a.class == Class::Footprint {
                if let Some(f) = &a.footprint {
                    if !f.courtyard_front.is_empty() {
                        item_layers.extend(side_layers(true).into_iter().map(String::from));
                    }
                    if !f.courtyard_back.is_empty() {
                        item_layers.extend(side_layers(false).into_iter().map(String::from));
                    }
                }
            }
            let hit = match &rule.layers {
                None => !item_layers.is_empty(),
                Some(set) => item_layers.iter().any(|l| set.contains(l)),
            };
            if !hit {
                let msg = if implicit { "Keepout layer(s) not matched.".to_string() } else { format!("Rule layer '{}' not matched; rule ignored.", escape_html(&rule.layer_source)) };
                self.say(|| msg.clone());
                return (false, msg);
            }
        }

        let layer_miss = match (&self.layer, &rule.layers) {
            (Some(l), Some(set)) if !set.contains(l) => true,
            _ => false,
        } || match &rule.layers {
            Some(set) => !set.iter().any(|l| eng.settings.enabled_layers.contains(l)),
            None => eng.settings.enabled_layers.is_empty(),
        };
        if layer_miss {
            let msg = if implicit { "Constraint layer not matched.".to_string() } else { format!("Rule layer '{}' not matched; rule ignored.", escape_html(&rule.layer_source)) };
            self.say(|| msg.clone());
            return (false, msg);
        }
        if c.ctype == CType::HoleToHole && !a.as_ref().is_some_and(|x| x.has_drilled_hole) && !b.as_ref().is_some_and(|x| x.has_drilled_hole) {
            let msg = format!("{} is not a drilled hole; rule ignored.", Self::describe(&a));
            self.say(|| msg.clone());
            return (false, msg);
        }
        match &rule.condition {
            None => {
                self.apply_unconditional(ri, ci, implicit)
            }
            Some(cond) if cond.expression().is_empty() => self.apply_unconditional(ri, ci, implicit),
            Some(cond) => {
                if !implicit {
                    let e = escape_html(cond.expression());
                    self.say(|| format!("Checking rule condition '{e}'."));
                }
                let mut msgs: Vec<String> = Vec::new();
                let wants = self.report.is_some();
                let hit = cond.evaluate_for(&eng.env, a.as_ref(), b.as_ref(), c.ctype as u32, self.layer.as_deref(), wants.then_some(&mut msgs));
                for m in msgs {
                    self.say(|| m);
                }
                if hit {
                    let msg = if implicit {
                        "Constraint applied."
                    } else if self.ctype == CType::Assertion {
                        "Rule applied."
                    } else {
                        "Rule applied; overrides previous constraints."
                    };
                    self.say(|| msg.to_string());
                    if self.ctype == CType::Assertion {
                        self.test_assertion(c);
                    }
                    self.apply(ri, ci);
                    (true, msg.to_string())
                } else {
                    let msg = if implicit { "Membership not satisfied; constraint ignored." } else { "Condition not satisfied; rule ignored." };
                    self.say(|| msg.to_string());
                    (false, msg.to_string())
                }
            }
        }
    }

    fn apply_unconditional(&mut self, ri: usize, ci: usize, implicit: bool) -> (bool, String) {
        let eng = self.eng;
        let c = &eng.rules[ri].constraints[ci];
        let msg = if implicit {
            "Unconditional constraint applied."
        } else if self.ctype == CType::Assertion {
            "Unconditional rule applied."
        } else {
            "Unconditional rule applied; overrides previous constraints."
        };
        self.say(|| msg.to_string());
        if !implicit && self.ctype == CType::Assertion {
            self.test_assertion(c);
        }
        self.apply(ri, ci);
        (true, msg.to_string())
    }

    fn test_assertion(&mut self, c: &Constraint) {
        let Some(t) = &c.test else { return };
        let e = escape_html(t.expression());
        self.say(|| format!("Checking assertion '{e}'."));
        let mut msgs: Vec<String> = Vec::new();
        let wants = self.report.is_some();
        let ok = t.evaluate_for(&self.eng.env, self.a.as_ref(), self.b.as_ref(), c.ctype as u32, self.layer.as_deref(), wants.then_some(&mut msgs));
        for m in msgs {
            self.say(|| m);
        }
        if ok {
            self.say(|| "Assertion passed.".to_string());
        } else {
            self.say(|| escape_html("--> Assertion failed. <--"));
        }
    }
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

    // ---------------------------------------------------------------------------------------------------------------------------------- the engine

    use crate::rule_item::{Class, FootprintFacts, Prop, RuleItem};

    fn mk_env(rules: &BoardRules, nets: &[&str]) -> Rc<Env> {
        let mut layers: Vec<String> = vec!["F.Cu".into(), "B.Cu".into()];
        layers.extend(["F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Edge.Cuts", "F.CrtYd", "B.CrtYd"].map(String::from));
        let refs: Vec<&str> = layers.iter().map(String::as_str).collect();
        let mut env = Env::simple(&refs);
        env.nets = nets.iter().map(|s| s.to_string()).collect();
        let r = rules.clone();
        env.classes = Box::new(move |net: &str| {
            let e = r.effective_class(net);
            (e.name, e.constituents)
        });
        Rc::new(env)
    }

    fn engine(rules: &BoardRules, nets: &[&str], dru: Option<&str>, keepouts: &[KeepoutRule]) -> (RuleEngine, Option<crate::dru::ParseError>) {
        let env = mk_env(rules, nets);
        let net_names: Vec<String> = nets.iter().map(|s| s.to_string()).collect();
        let classes = class_views(rules, &net_names);
        RuleEngine::with_rules_file(Settings::from_rules(rules), env, &classes, keepouts, dru)
    }

    fn pad(reference: &str, number: &str, net: &str, layer: &str) -> Rc<RuleItem> {
        let mut it = RuleItem::new(format!("{reference}.{number}"), Class::Pad);
        it.net = net.to_string();
        it.layers = vec![layer.to_string()];
        it.layer = Some(layer.to_string());
        it.description = format!("Pad {number} [{net}] of {reference} on {layer}");
        it.pad_attribute = "SMD";
        it.footprint = Some(Rc::new(FootprintFacts { reference: reference.to_string(), ..Default::default() }));
        Rc::new(it)
    }

    fn track(net: &str, layer: &str, width_nm: i64) -> Rc<RuleItem> {
        let mut it = RuleItem::new("t1", Class::Track);
        it.net = net.to_string();
        it.layers = vec![layer.to_string()];
        it.layer = Some(layer.to_string());
        it.set("Width", Prop::Int(width_nm));
        it.description = format!("Track [{net}] on {layer}, length 10.0000 mm");
        Rc::new(it)
    }

    fn rules_hv() -> BoardRules {
        BoardRules {
            clearance: 200,
            net_classes: vec![NetClass { name: "HV".into(), nets: vec!["VBUS".into()], track_width: None, clearance: Some(500), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }],
            ..BoardRules::default()
        }
    }

    #[test]
    fn the_winner_is_the_larger_net_class_clearance_and_every_rule_it_looked_at_is_listed() {
        let rules = rules_hv();
        let (eng, err) = engine(&rules, &["VBUS", "GND"], None, &[]);
        assert!(err.is_none());
        let (a, b) = (pad("J1", "1", "VBUS", "F.Cu"), pad("U1", "2", "GND", "F.Cu"));
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&a), Some(&b), Some("F.Cu"), Some(&mut report));
        assert_eq!(c.value.min, Some(500_000), "the HV class (500 um) beats the Default class (200 um) the other net is in");
        assert_eq!(eng.constraint_name(&c), "netclass 'HV'");
        // the same, as the netclass fast path of `crate::constraints::clearance` has it
        assert_eq!(nm(clearance(&rules, Some("VBUS"), Some("GND"))), c.value.min());
        let text = report.lines.join("\n");
        // (the names are escaped for the HTML report box: `EscapeHTML( c->constraint.GetName() )`)
        assert!(text.contains("Checking netclass &apos;Default&apos; clearance: 0.2000 mm."), "{text}");
        assert!(text.contains("Checking netclass &apos;HV&apos; clearance: 0.5000 mm."), "{text}");
        assert!(text.contains("Constraint applied."), "{text}");
        assert!(text.contains("Board minimum clearance: 0.0000 mm."), "{text}");
        // every rule it went through, the implicit ones of the net classes (and of the composite class VBUS is in, HV and Default) among them
        let names: Vec<_> = report.considered.iter().map(|c| (c.rule.as_str(), c.applied)).collect();
        assert_eq!(names[0], ("netclass 'Default'", true));
        assert!(names.contains(&("netclass 'HV'", false)) && names.contains(&("netclass 'HV'", true)), "{names:?}");
        assert!(report.considered.iter().all(|c| c.implicit && c.constraint == CType::Clearance));
    }

    #[test]
    fn a_rules_file_rule_beats_the_implicit_ones_and_the_later_of_two_wins() {
        let rules = rules_hv();
        let dru = "(version 1)\n(rule \"fat\" (condition \"A.NetName == 'GND'\") (constraint clearance (min 0.3mm)))\n(rule \"fatter\" (condition \"A.NetName == 'GND'\") (constraint clearance (min 0.4mm)))\n";
        let (eng, err) = engine(&rules, &["VBUS", "GND", "SIG"], Some(dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let (a, b) = (pad("J1", "1", "GND", "F.Cu"), pad("U1", "2", "SIG", "F.Cu"));
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&a), Some(&b), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(400_000), "rule 'fatter'".to_string()));
        let text = report.lines.join("\n");
        assert!(text.contains("Checking rule condition 'A.NetName == &apos;GND&apos;'."), "{text}");
        assert!(text.contains("Rule applied; overrides previous constraints."), "{text}");
        // an explicit match ends the evaluation: no "Board minimum clearance" line
        assert!(!text.contains("Board minimum clearance"), "{text}");
        // the condition is commutative
        let c2 = eng.eval_rules(CType::Clearance, Some(&b), Some(&a), Some("F.Cu"), None);
        assert_eq!(c2.value.min, Some(400_000));
        // pads on other nets: the implicit rules alone
        let (x, y) = (pad("J1", "1", "SIG", "F.Cu"), pad("U1", "2", "VBUS", "F.Cu"));
        let c3 = eng.eval_rules(CType::Clearance, Some(&x), Some(&y), Some("F.Cu"), None);
        assert_eq!((c3.value.min, eng.constraint_name(&c3)), (Some(500_000), "netclass 'HV'".to_string()));
    }

    #[test]
    fn a_rule_scoped_to_a_layer_is_ignored_on_another_and_says_so() {
        let rules = BoardRules::default();
        let dru = "(version 1)(rule \"back only\" (layer \"B.Cu\") (constraint clearance (min 1mm)))";
        let (eng, err) = engine(&rules, &["A", "B"], Some(dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let (a, b) = (track("A", "F.Cu", 200_000), track("B", "F.Cu", 200_000));
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&a), Some(&b), Some("F.Cu"), Some(&mut report));
        assert_eq!(c.value.min, Some(200_000));
        assert!(report.lines.join("\n").contains("Rule layer 'B.Cu' not matched; rule ignored."), "{:?}", report.lines);
        let back_a = track("A", "B.Cu", 200_000);
        let back_b = track("B", "B.Cu", 200_000);
        assert_eq!(eng.eval_rules(CType::Clearance, Some(&back_a), Some(&back_b), Some("B.Cu"), None).value.min, Some(1_000_000));
    }

    #[test]
    fn a_pads_own_clearance_beats_the_rules_but_not_the_board_minimum() {
        let mut rules = rules_hv();
        let (eng, _) = engine(&rules, &["VBUS", "GND"], None, &[]);
        let mut p = (*pad("J1", "1", "GND", "F.Cu")).clone();
        p.overrides.clearance = Some((150_000, "pad".to_string()));
        let p = Rc::new(p);
        let other = pad("U1", "2", "VBUS", "F.Cu");
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&p), Some(&other), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(150_000), "pad".to_string()));
        assert!(report.lines.join("\n").contains("Local override on Pad 1 [GND] of J1 on F.Cu; clearance: 0.1500 mm."));
        // the board's minimum clearance still has the last word
        rules.min_clearance_um = 300;
        let (eng, _) = engine(&rules, &["VBUS", "GND"], None, &[]);
        let c = eng.eval_rules(CType::Clearance, Some(&p), Some(&other), Some("F.Cu"), None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(300_000), "board minimum".to_string()));
    }

    #[test]
    fn a_zones_own_clearance_beats_the_net_class_when_larger() {
        let rules = rules_hv();
        let (eng, _) = engine(&rules, &["VBUS", "GND"], None, &[]);
        let mut z = RuleItem::new("z1", Class::Zone);
        z.net = "GND".into();
        z.layers = vec!["F.Cu".into()];
        z.layer = Some("F.Cu".into());
        z.local_clearance = Some((700_000, "zone".into()));
        z.description = "Zone [GND] on F.Cu, priority 0".into();
        let z = Rc::new(z);
        let p = pad("J1", "1", "VBUS", "F.Cu");
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&z), Some(&p), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(700_000), "zone".to_string()));
        assert!(report.lines.join("\n").contains("Local clearance on Zone [GND] on F.Cu, priority 0: 0.7000 mm."), "{:?}", report.lines);
    }

    #[test]
    fn a_net_tie_zeroes_the_clearance_of_the_tied_nets() {
        let rules = rules_hv();
        let (eng, _) = engine(&rules, &["VBUS", "GND"], None, &[]);
        let fp = {
            let mut f = RuleItem::new("TIE1", Class::Footprint);
            f.description = "Footprint TIE1".into();
            Rc::new(f)
        };
        // two pads of the same net-tie footprint (an item of the footprint against a connected item of another is the case `EvalRules` handles; tracks entering a tie are
        // `IsNetTieExclusion`'s, in the providers)
        let mut a = (*pad("TIE1", "1", "VBUS", "F.Cu")).clone();
        a.parent = Some(fp.clone());
        a.net_tie = vec!["VBUS".into(), "GND".into()];
        let mut b = (*pad("TIE1", "2", "GND", "F.Cu")).clone();
        b.parent = Some(fp.clone());
        b.net_tie = vec!["VBUS".into(), "GND".into()];
        let (a, b) = (Rc::new(a), Rc::new(b));
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&a), Some(&b), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(0), "net tie".to_string()));
        assert!(report.lines.join("\n").contains("Net tie on Footprint TIE1; clearance: 0."), "{:?}", report.lines);
        // a pad of another footprint on the other net is not tied
        let other = pad("U1", "1", "GND", "F.Cu");
        assert_eq!(eng.eval_rules(CType::Clearance, Some(&a), Some(&other), Some("F.Cu"), None).value.min, Some(500_000));
    }

    #[test]
    fn track_width_has_the_board_floor_and_the_class_optimum() {
        let mut rules = rules_hv();
        rules.track_width_min_um = 150;
        rules.net_classes[0].track_width = Some(400);
        let (eng, _) = engine(&rules, &["VBUS", "GND"], None, &[]);
        let t = track("VBUS", "F.Cu", 400_000);
        let mut report = Report::default();
        let c = eng.eval_rules(CType::TrackWidth, Some(&t), None, Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, c.value.opt, c.value.max), (Some(150_000), Some(400_000), None));
        let text = report.lines.join("\n");
        assert!(text.contains("Checking board setup constraints track width: min 0.1500 mm."), "{text}");
        assert!(text.contains("Checking netclass &apos;HV&apos; track width: opt 0.4000 mm.") || text.contains("Checking netclass 'HV' track width: opt 0.4000 mm."), "{text}");
        // a net in the Default class only: the board floor and the Default width
        let plain = track("GND", "F.Cu", 200_000);
        let d = eng.eval_rules(CType::TrackWidth, Some(&plain), None, Some("F.Cu"), None);
        assert_eq!((d.value.min, d.value.opt), (Some(150_000), Some(200_000)));
    }

    #[test]
    fn a_keepout_area_disallows_what_it_names_where_it_is() {
        let rules = BoardRules::default();
        let ko = KeepoutRule { id: "ko1".into(), name: "no tracks".into(), parent_ref: None, layers: ["F.Cu".to_string()].into_iter().collect(), flags: disallow::TRACKS };
        let (mut eng, _) = engine(&rules, &["A"], None, &[ko]);
        // the engine's env must know the area
        let mut area_env = Env::simple(&["F.Cu", "B.Cu"]);
        area_env.areas = vec![Rc::new(crate::rule_item::AreaFacts { id: "ko1".into(), name: "no tracks".into(), layers: vec!["F.Cu".into()], outline: vec![eda_model::ir::Point { x: 0, y: 0 }, eda_model::ir::Point { x: 10_000, y: 0 }, eda_model::ir::Point { x: 10_000, y: 10_000 }, eda_model::ir::Point { x: 0, y: 10_000 }], parent_footprint: None })];
        eng.env = Rc::new(area_env);
        eng.compile_implicit();
        let mut t = (*track("A", "F.Cu", 200_000)).clone();
        t.shapes = vec![crate::kimath::Shape::Stadium { a: eda_model::ir::Point { x: 1_000, y: 1_000 }, b: eda_model::ir::Point { x: 5_000, y: 1_000 }, r: 100 }];
        let t = Rc::new(t);
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Disallow, Some(&t), None, Some("F.Cu"), Some(&mut report));
        assert_ne!(c.disallow & disallow::TRACKS, 0, "{:?}", report.lines);
        assert_eq!(eng.constraint_name(&c), "keepout area 'no tracks'");
        assert!(report.lines.join("\n").contains("Constraint applied."));
    }

    #[test]
    fn an_assertion_is_tested_on_the_item() {
        let rules = BoardRules::default();
        let dru = "(version 1)(rule \"min width\" (condition \"A.Type == 'Track'\") (constraint assertion \"A.Width >= 0.25mm\"))";
        let (eng, err) = engine(&rules, &["A"], Some(dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let thin = track("A", "F.Cu", 200_000);
        let mut report = Report::default();
        let mut failed = Vec::new();
        eng.process_assertions(&thin, Some(&mut report), |n| failed.push(n.to_string()));
        assert_eq!(failed, ["rule 'min width'"]);
        let text = report.lines.join("\n");
        assert!(text.contains("Checking rule assertion 'A.Width &gt;= 0.25mm'."), "{text}");
        assert!(text.contains("--&gt; Assertion failed. &lt;--"), "{text}");
        let wide = track("A", "F.Cu", 300_000);
        let mut failed2 = Vec::new();
        eng.process_assertions(&wide, None, |n| failed2.push(n.to_string()));
        assert!(failed2.is_empty());
    }

    #[test]
    fn a_bad_rules_file_leaves_only_the_implicit_rules_and_says_so() {
        let rules = rules_hv();
        let (eng, err) = engine(&rules, &["VBUS", "GND"], Some("(version 1)(rule x (constraint clearance (min 1mm)) oops)"), &[]);
        assert!(err.is_some());
        assert!(eng.rules.iter().all(|r| r.is_implicit()));
        let (a, b) = (pad("J1", "1", "VBUS", "F.Cu"), pad("U1", "2", "GND", "F.Cu"));
        assert_eq!(eng.eval_rules(CType::Clearance, Some(&a), Some(&b), Some("F.Cu"), None).value.min, Some(500_000));
        // a condition the compiler objects to is as bad
        let (eng, err) = engine(&rules, &["VBUS"], Some("(version 1)(rule x (condition \"A.Nope == 'y'\") (constraint clearance (min 1mm)))"), &[]);
        assert_eq!(err.unwrap().message, "Parse error");
        assert!(eng.rules.iter().all(|r| r.is_implicit()));
    }

    #[test]
    fn lengths_print_the_way_kicad_prints_them() {
        assert_eq!(message_text_from_value(200_000.0), "0.2000 mm");
        assert_eq!(message_text_from_value(10.0), "1.000e-05 mm", "a value that would print as 0.0000 is shown in scientific notation");
        assert_eq!(string_from_value(200_000.0, Units::Mm, true), "0.2 mm");
        assert_eq!(string_from_value(254_000.0, Units::Mils, true), "10 mils");
        assert_eq!(string_from_value(1_270_000.0, Units::Inch, false), "0.05");
        assert_eq!(string_from_value(0.0, Units::Mm, true), "0 mm");
        assert_eq!(string_from_value(-100_000.0, Units::Mm, false), "-0.1");
    }

    // ----------------------------------------------------------------------------------------------- KiCad's own rule sets (qa/data/pcbnew)

    fn qa_rules(name: &str) -> Option<String> {
        std::fs::read_to_string(format!("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data/pcbnew/{name}")).ok()
    }

    #[test]
    fn issue16566_an_oval_pad_has_a_five_millimetre_clearance_and_no_other_pad_does() {
        let Some(dru) = qa_rules("issue16566.kicad_dru") else { return };
        let rules = BoardRules::default();
        let (eng, err) = engine(&rules, &["A", "B"], Some(&dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let mut oval = (*pad("U1", "1", "A", "F.Cu")).clone();
        oval.set("Pad Shape", Prop::Enum("Oval".into()));
        let oval = Rc::new(oval);
        let mut rect = (*pad("U1", "2", "B", "F.Cu")).clone();
        rect.set("Pad Shape", Prop::Enum("Rectangle".into()));
        let rect = Rc::new(rect);
        let mut report = Report::default();
        let c = eng.eval_rules(CType::Clearance, Some(&oval), Some(&rect), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(5_000_000), "rule 'oval_clearance'".to_string()));
        // the rule list: the Default class's implicit rule, then the file's rule
        let list: Vec<_> = report.considered.iter().map(|c| (c.rule.clone(), c.applied)).collect();
        assert_eq!(list.last().unwrap(), &("rule 'oval_clearance'".to_string(), true), "{list:?}");
        // the order does not matter: the condition is commutative
        assert_eq!(eng.eval_rules(CType::Clearance, Some(&rect), Some(&oval), Some("F.Cu"), None).value.min, Some(5_000_000));
        // two rectangular pads: the default
        let rect2 = pad("U2", "1", "A", "F.Cu");
        let c = eng.eval_rules(CType::Clearance, Some(&rect), Some(&rect2), Some("F.Cu"), None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(200_000), "netclass 'Default'".to_string()));
    }

    #[test]
    fn issue6879_hole_rules_by_pad_type_and_the_deprecated_hole_constraint() {
        let Some(dru) = qa_rules("issue6879.kicad_dru") else { return };
        let rules = BoardRules::default();
        let (eng, err) = engine(&rules, &["A"], Some(&dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let mut pth = (*pad("J1", "1", "A", "F.Cu")).clone();
        pth.set("Pad Type", Prop::Enum("Through-hole".into()));
        pth.pad_attribute = "PTH";
        pth.drill = 800_000;
        pth.has_hole = true;
        pth.has_drilled_hole = true;
        let pth = Rc::new(pth);
        let t = track("A", "F.Cu", 200_000);
        let mut report = Report::default();
        let c = eng.eval_rules(CType::HoleClearance, Some(&pth), Some(&t), Some("F.Cu"), Some(&mut report));
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(1_000_000), "rule 'PTH to Track Clearance'".to_string()));
        // an SMD pad does not match either hole rule: the board's hole clearance (0.25 mm)
        let smd = pad("U1", "1", "A", "F.Cu");
        let c = eng.eval_rules(CType::HoleClearance, Some(&smd), Some(&t), Some("F.Cu"), None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(250_000), "board setup constraints hole".to_string()));
        // `(constraint hole (max 2.0mm))` is `hole_size`: a maximum on top of the board's minimum drill
        let c = eng.eval_rules(CType::HoleSize, Some(&pth), None, None, None);
        assert_eq!((c.value.min, c.value.max), (Some(300_000), Some(2_000_000)));
        assert_eq!(eng.constraint_name(&c), "rule 'Max Drill Hole Size Mechanical'");
    }

    #[test]
    fn multinetclasses_a_track_width_maximum_per_class_and_the_composite_class_name() {
        let Some(dru) = qa_rules("multinetclasses_drc.kicad_dru") else { return };
        let mk = |name: &str, nets: &[&str]| NetClass { name: name.into(), nets: nets.iter().map(|s| s.to_string()).collect(), track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 };
        let rules = BoardRules { net_classes: vec![mk("CLASS1", &["N1"]), mk("CLASS2", &["N2"]), mk("BOTH", &["N3"])], ..BoardRules::default() };
        let (eng, err) = engine(&rules, &["N1", "N2", "N3", "OTHER"], Some(&dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let width = |net: &str| {
            let t = track(net, "F.Cu", 200_000);
            let c = eng.eval_rules(CType::TrackWidth, Some(&t), None, Some("F.Cu"), None);
            (c.value.max, eng.constraint_name(&c))
        };
        // `A.NetClass == 'Default'` is true of a composite class that has the Default class among its constituents (`PCBEXPR_NETCLASS_VALUE::EqualTo`), and the
        // later rule wins: rule_3's 5 mil is the maximum of every net whose class takes anything from Default -- N1 ("CLASS1,Default"), N2 and OTHER alike.
        assert_eq!(width("N1"), (Some(127_000), "rule 'rule_3'".to_string()));
        assert_eq!(width("N2"), (Some(127_000), "rule 'rule_3'".to_string()));
        assert_eq!(width("OTHER"), (Some(127_000), "rule 'rule_3'".to_string()));
        // a class that sets every setting itself is not in the Default class: rule_1 is its rule
        let full = NetClass { name: "FULL".into(), nets: vec!["N4".into()], track_width: Some(300), clearance: Some(250), via_diameter: Some(600), via_drill: Some(300), microvia_diameter: Some(300), microvia_drill: Some(100), diff_pair_width: Some(200), diff_pair_gap: Some(250), diff_pair_via_gap: Some(250), priority: 0 };
        let two = BoardRules { net_classes: vec![full, mk("CLASS2", &["N2"])], ..BoardRules::default() };
        let (eng2, _) = engine(&two, &["N2", "N4"], Some(&dru), &[]);
        let t4 = track("N4", "F.Cu", 200_000);
        let c = eng2.eval_rules(CType::TrackWidth, Some(&t4), None, Some("F.Cu"), None);
        assert_eq!(c.value.max, None, "none of the three rules names the FULL class");
        // the rule list for N1 holds the implicit width rules and the three of the file
        let t = track("N1", "F.Cu", 200_000);
        let mut report = Report::default();
        eng.eval_rules(CType::TrackWidth, Some(&t), None, Some("F.Cu"), Some(&mut report));
        let file_rules: Vec<_> = report.considered.iter().filter(|c| !c.implicit).map(|c| (c.rule.as_str(), c.applied)).collect();
        assert_eq!(file_rules, [("rule 'rule_1'", false), ("rule 'rule_2'", true), ("rule 'rule_3'", true)]);
    }

    #[test]
    fn issue11814_the_pad_to_track_and_via_to_track_rules() {
        let Some(dru) = qa_rules("issue11814.kicad_dru") else { return };
        let rules = BoardRules::default();
        let (eng, err) = engine(&rules, &["A", "B"], Some(&dru), &[]);
        assert!(err.is_none(), "{err:?}");
        let p = pad("U1", "1", "A", "F.Cu");
        let t = track("B", "F.Cu", 200_000);
        let c = eng.eval_rules(CType::Clearance, Some(&p), Some(&t), Some("F.Cu"), None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(200_000), "rule 'PadToTrack'".to_string()));
        let mut via = RuleItem::new("v1", Class::Via);
        via.net = "A".into();
        via.layers = vec!["F.Cu".into(), "B.Cu".into()];
        via.layer = Some("F.Cu".into());
        via.via_type = Some("Through");
        via.has_hole = true;
        via.has_drilled_hole = true;
        via.description = "Via [A] on all copper layers".into();
        let via = Rc::new(via);
        let c = eng.eval_rules(CType::HoleClearance, Some(&via), Some(&t), Some("F.Cu"), None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(254_000), "rule 'ViaToTrack'".to_string()));
        // two vias of one net: the same-net hole to hole rule
        let mut via2 = (*via).clone();
        via2.id = "v2".into();
        let via2 = Rc::new(via2);
        let c = eng.eval_rules(CType::HoleToHole, Some(&via), Some(&via2), None, None);
        assert_eq!((c.value.min, eng.constraint_name(&c)), (Some(254_000), "rule 'ViaToVia_SameNet'".to_string()));
    }
}
