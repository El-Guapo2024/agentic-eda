//! The DRC Rule Editor (`pcbnew.DRETool.drcRuleEditor`, `pcbnew/drc/rule_editor/` at KiCad 8303b2ad): the rules of a `.kicad_dru` file as the editor's
//! panels hold them, and back. The loader is `DRC_RULE_LOADER` (`drc_re_rule_loader.cpp`) over `DRC_PANEL_MATCHER` (`drc_re_panel_matcher.cpp`), the saver
//! `DRC_RULE_SAVER` (`drc_re_rule_saver.cpp`), the panels' data the `DRC_RE_*_CONSTRAINT_DATA` classes (`drc_re_*_constraint_data.h`): what each one writes
//! (`GetConstraintClauses`, `GenerateRule`, `buildRule`) and how it checks itself (`Validate`, with KiCad's messages word for word). What the dialog's
//! panels do with the layer selector (`PANEL_DRC_RULE_EDITOR::populateLayerSelector`, `buildLayerClause`, `getSelectedLayers`, `getSelectedLayerSource`,
//! `setSelectedLayers`) and with Save (`onSaveButtonClicked`, `ValidateInputs`, `TransferDataFromWindow`) is here too, so the verb that saves a rule
//! (`eda_ops`'s `edit_drc_rule`) and the dialog agree on every word. The dialog itself is `web/studio/src/components/RuleEditorDialog.tsx`.
//!
//! The file is the only state: the editor reads it ([`load`]), one rule changes, and the whole file is written again ([`save`]). A rule nobody edited is
//! written back as the text it was read from (`generateRuleText`'s "round-trip preservation"); an edited one is generated from its panel.
//!
//! Where this differs from KiCad, and why (each is a case where KiCad's editor loses or duplicates a rule the user never touched):
//!   - A rule whose panel has no node in the tree (`HOLE_SIZE`, `MINIMUM_VIA_DIAMETER`: `DRC_PANEL_MATCHER` claims `hole_size` and `via_diameter` for them,
//!     and `GetDefaultRuleTreeItems` lists neither) goes to the node that writes that constraint: a `hole_size` with no maximum to Minimum drill size (what
//!     that panel writes), any other `hole_size` to a Custom rule, a `via_diameter` to Via style (which writes one alone when no hole size is given). KiCad
//!     skips it (`LoadExistingRules`: "No parent found ... skipping"), and the next save leaves it out of the file -- a rule made with Minimum drill size, or
//!     a Via style with no hole size, is gone the next time the editor opens.
//!   - Two rules with the same name each keep their own text: the n-th rule of a name is cut from the n-th `(rule <name>` of the file, and the saver groups
//!     only the panels of one rule of the file ([`Entry::source`]). KiCad cuts both from the first (`ExtractRuleText`) and merges both into one group when
//!     their conditions match, so the second is lost or replaced by a copy of the first.
//!   - The claims of one priority are kept in the order `initClaims` lists them (a stable sort); KiCad's `std::sort` leaves their order to the library. It
//!     only orders the panels of a rule with two constraints of one tier in the tree.
//!   - A file that does not parse is not opened ([`load`] says why) where KiCad's loader shows an empty tree, and its first save writes an empty file.

use crate::constraints::{disallow, CType, Constraint, Rule, Severity};
use crate::dru::{self, ParseError, OPT_SKEW_WITHIN_DIFF_PAIRS};
use crate::pcbexpr::Env;
use eda_model::layers::{pcb_layer_id, standard_layer_name};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

// ------------------------------------------------------------------------------------------------------------------------------------------------- panels

/// `DRC_RULE_EDITOR_CONSTRAINT_NAME` (`drc_rule_editor_enums.h`), in KiCad's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelType {
    MinimumClearance,
    CreepageDistance,
    MinimumConnectionWidth,
    CopperToHoleClearance,
    MinimumThermalReliefSpokeCount,
    MinimumAnnularWidth,
    CopperToEdgeClearance,
    CourtyardClearance,
    PhysicalClearance,
    MinimumDrillSize,
    HoleSize,
    HoleToHoleDistance,
    MinimumViaDiameter,
    ViaStyle,
    MinimumTextHeightAndThickness,
    SilkToSilkClearance,
    SilkToSoldermaskClearance,
    MinimumSoldermaskSliver,
    SoldermaskExpansion,
    SolderpasteExpansion,
    MatchedLengthDiffPair,
    RoutingDiffPair,
    RoutingWidth,
    MaximumViaCount,
    AbsoluteLength,
    PermittedLayers,
    AllowedOrientation,
    ViasUnderSmd,
    CustomRule,
}

/// `DRC_LAYER_CATEGORY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerCategory {
    CopperOnly,
    SilkscreenOnly,
    SoldermaskOnly,
    SolderpasteOnly,
    TopBottomAny,
    GeneralAnyLayer,
    NoLayerSelector,
}

/// What kind of data a panel holds: which `DRC_RE_*_CONSTRAINT_DATA` class `buildRuleTreeNode`'s factory map (and, for the rest,
/// `CreateNumericConstraintData`) makes for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Numeric,
    ViaStyle,
    TextHeightThickness,
    RoutingWidth,
    RoutingDiffPair,
    AbsoluteLength,
    MatchedLengthDiffPair,
    PermittedLayers,
    AllowedOrientation,
    ViasUnderSmd,
    Custom,
}

impl PanelType {
    pub const ALL: [PanelType; 29] = [
        PanelType::MinimumClearance,
        PanelType::CreepageDistance,
        PanelType::MinimumConnectionWidth,
        PanelType::CopperToHoleClearance,
        PanelType::MinimumThermalReliefSpokeCount,
        PanelType::MinimumAnnularWidth,
        PanelType::CopperToEdgeClearance,
        PanelType::CourtyardClearance,
        PanelType::PhysicalClearance,
        PanelType::MinimumDrillSize,
        PanelType::HoleSize,
        PanelType::HoleToHoleDistance,
        PanelType::MinimumViaDiameter,
        PanelType::ViaStyle,
        PanelType::MinimumTextHeightAndThickness,
        PanelType::SilkToSilkClearance,
        PanelType::SilkToSoldermaskClearance,
        PanelType::MinimumSoldermaskSliver,
        PanelType::SoldermaskExpansion,
        PanelType::SolderpasteExpansion,
        PanelType::MatchedLengthDiffPair,
        PanelType::RoutingDiffPair,
        PanelType::RoutingWidth,
        PanelType::MaximumViaCount,
        PanelType::AbsoluteLength,
        PanelType::PermittedLayers,
        PanelType::AllowedOrientation,
        PanelType::ViasUnderSmd,
        PanelType::CustomRule,
    ];

    /// `sCodeMap` (`drc_rule_editor_utils.cpp`, `GetConstraintCode`): the keyword a panel's rules are written with; empty for a Custom rule.
    pub fn code(self) -> &'static str {
        use PanelType::*;
        match self {
            MinimumClearance => "clearance",
            CreepageDistance => "creepage",
            MinimumConnectionWidth => "connection_width",
            CopperToHoleClearance => "hole_clearance",
            MinimumThermalReliefSpokeCount => "min_resolved_spokes",
            MinimumAnnularWidth => "annular_width",
            CopperToEdgeClearance => "edge_clearance",
            CourtyardClearance => "courtyard_clearance",
            PhysicalClearance => "physical_clearance",
            MinimumDrillSize | HoleSize => "hole_size",
            HoleToHoleDistance => "hole_to_hole",
            MinimumViaDiameter => "via_diameter",
            ViaStyle => "via_style",
            MinimumTextHeightAndThickness => "text_height",
            SilkToSilkClearance | SilkToSoldermaskClearance => "silk_clearance",
            MinimumSoldermaskSliver => "solder_mask_sliver",
            SoldermaskExpansion => "solder_mask_expansion",
            SolderpasteExpansion => "solder_paste_abs_margin",
            MatchedLengthDiffPair | AbsoluteLength => "length",
            RoutingDiffPair => "diff_pair_gap",
            RoutingWidth => "track_width",
            MaximumViaCount => "via_count",
            PermittedLayers => "permitted_layers",
            AllowedOrientation => "allowed_orientation",
            ViasUnderSmd => "disallow_via",
            CustomRule => "",
        }
    }

    /// `DRC_RULE_EDITOR_UTILS::IsNumericInputType`.
    pub fn is_numeric_input(self) -> bool {
        use PanelType::*;
        matches!(
            self,
            CopperToEdgeClearance
                | CopperToHoleClearance
                | CourtyardClearance
                | PhysicalClearance
                | CreepageDistance
                | HoleSize
                | HoleToHoleDistance
                | MatchedLengthDiffPair
                | MaximumViaCount
                | MinimumAnnularWidth
                | MinimumClearance
                | MinimumConnectionWidth
                | MinimumSoldermaskSliver
                | MinimumThermalReliefSpokeCount
                | MinimumDrillSize
                | MinimumViaDiameter
                | SilkToSilkClearance
                | SilkToSoldermaskClearance
                | SoldermaskExpansion
                | SolderpasteExpansion
        )
    }

    /// `IsIntegerOnly`: `DRC_RE_MAXIMUM_VIA_COUNT_CONSTRAINT_DATA` and `DRC_RE_MINIMUM_THERMAL_SPOKE_COUNT_CONSTRAINT_DATA`.
    pub fn is_integer_only(self) -> bool {
        matches!(self, PanelType::MaximumViaCount | PanelType::MinimumThermalReliefSpokeCount)
    }

    /// The data class `buildRuleTreeNode`'s `s_constraintFactories` makes, else a numeric one (every other panel is a numeric input type).
    pub fn data_kind(self) -> DataKind {
        use PanelType::*;
        match self {
            ViaStyle => DataKind::ViaStyle,
            MinimumTextHeightAndThickness => DataKind::TextHeightThickness,
            RoutingDiffPair => DataKind::RoutingDiffPair,
            RoutingWidth => DataKind::RoutingWidth,
            PermittedLayers => DataKind::PermittedLayers,
            AllowedOrientation => DataKind::AllowedOrientation,
            CustomRule => DataKind::Custom,
            AbsoluteLength => DataKind::AbsoluteLength,
            MatchedLengthDiffPair => DataKind::MatchedLengthDiffPair,
            ViasUnderSmd => DataKind::ViasUnderSmd,
            _ => DataKind::Numeric,
        }
    }

    /// Whether `DIALOG_DRC_RULE_EDITOR::GetDefaultRuleTreeItems` gives the panel a node of its own (every one but `HOLE_SIZE` and `MINIMUM_VIA_DIAMETER`).
    pub fn has_tree_node(self) -> bool {
        !matches!(self, PanelType::HoleSize | PanelType::MinimumViaDiameter)
    }

    /// `DRC_RULE_EDITOR_UTILS::GetLayerCategoryForConstraint`.
    pub fn layer_category(self) -> LayerCategory {
        use PanelType::*;
        match self {
            MinimumClearance | CreepageDistance | CopperToHoleClearance | CopperToEdgeClearance | ViaStyle | MinimumViaDiameter | MinimumAnnularWidth
            | MinimumConnectionWidth | RoutingWidth | RoutingDiffPair | MinimumThermalReliefSpokeCount | MatchedLengthDiffPair | AbsoluteLength => {
                LayerCategory::CopperOnly
            }
            SilkToSilkClearance => LayerCategory::SilkscreenOnly,
            MinimumSoldermaskSliver | SoldermaskExpansion => LayerCategory::SoldermaskOnly,
            SolderpasteExpansion => LayerCategory::SolderpasteOnly,
            CourtyardClearance | SilkToSoldermaskClearance | ViasUnderSmd | AllowedOrientation => LayerCategory::TopBottomAny,
            PhysicalClearance | HoleSize | HoleToHoleDistance | MinimumDrillSize | MinimumTextHeightAndThickness => LayerCategory::GeneralAnyLayer,
            MaximumViaCount | PermittedLayers | CustomRule => LayerCategory::NoLayerSelector,
        }
    }

    /// `constraintNeedsTwoObjects` (`panel_drc_rule_editor.cpp`): the conditions offer Object A and Object B.
    pub fn needs_two_objects(self) -> bool {
        use PanelType::*;
        matches!(self, MinimumClearance | CreepageDistance | CopperToHoleClearance | PhysicalClearance | HoleToHoleDistance)
    }
}

/// `VIA_STYLE_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViaType {
    #[default]
    Any,
    Through,
    Micro,
    Blind,
    Buried,
}

impl ViaType {
    /// `DRC_RE_VIA_STYLE_CONSTRAINT_DATA::GetViaTypeCondition`.
    pub fn condition(self) -> &'static str {
        match self {
            ViaType::Through => "A.Via_Type == 'Through'",
            ViaType::Micro => "A.Via_Type == 'Micro'",
            ViaType::Blind => "A.Via_Type == 'Blind'",
            ViaType::Buried => "A.Via_Type == 'Buried'",
            ViaType::Any => "",
        }
    }
}

fn d_true() -> bool {
    true
}

/// The values of a panel: the members of its `DRC_RE_*_CONSTRAINT_DATA` class, all in one flat record (the panel type says which ones it reads). Lengths
/// are in millimetres, as the classes keep them. The defaults are the classes' own: zero, unchecked, except the four via types of Vias under SMD, which
/// start checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fields {
    /// `DRC_RE_NUMERIC_INPUT_CONSTRAINT_DATA::m_numericInputValue`.
    pub value: f64,
    pub min_via_diameter: f64,
    pub max_via_diameter: f64,
    pub min_via_hole: f64,
    pub max_via_hole: f64,
    pub via_type: ViaType,
    pub min_text_height: f64,
    pub min_text_thickness: f64,
    /// Routing width and Routing diff pair.
    pub opt_width: f64,
    pub width_tolerance: f64,
    pub opt_gap: f64,
    pub gap_tolerance: f64,
    pub max_uncoupled: f64,
    /// Absolute length and Matched length diff pair.
    pub opt_length: f64,
    pub tolerance: f64,
    pub max_skew: f64,
    pub within_diff_pairs: bool,
    /// Permitted layers.
    pub top_layer: bool,
    pub bottom_layer: bool,
    /// Allowed orientation.
    pub deg0: bool,
    pub deg90: bool,
    pub deg180: bool,
    pub deg270: bool,
    pub all_degrees: bool,
    /// Vias under SMD (`m_disallowThroughVias` ..; `true` by default).
    #[serde(default = "d_true")]
    pub through_vias: bool,
    #[serde(default = "d_true")]
    pub micro_vias: bool,
    #[serde(default = "d_true")]
    pub blind_vias: bool,
    #[serde(default = "d_true")]
    pub buried_vias: bool,
    /// A Custom rule's body: everything of the rule but its `(rule name` and the last `)` (`DRC_RE_CUSTOM_RULE_CONSTRAINT_DATA::m_ruleText`).
    pub rule_text: String,
}

impl Default for Fields {
    fn default() -> Fields {
        Fields {
            value: 0.0,
            min_via_diameter: 0.0,
            max_via_diameter: 0.0,
            min_via_hole: 0.0,
            max_via_hole: 0.0,
            via_type: ViaType::Any,
            min_text_height: 0.0,
            min_text_thickness: 0.0,
            opt_width: 0.0,
            width_tolerance: 0.0,
            opt_gap: 0.0,
            gap_tolerance: 0.0,
            max_uncoupled: 0.0,
            opt_length: 0.0,
            tolerance: 0.0,
            max_skew: 0.0,
            within_diff_pairs: false,
            top_layer: false,
            bottom_layer: false,
            deg0: false,
            deg90: false,
            deg180: false,
            deg270: false,
            all_degrees: false,
            through_vias: true,
            micro_vias: true,
            blind_vias: true,
            buried_vias: true,
            rule_text: String::new(),
        }
    }
}

/// `SEVERITY` as the editor carries it (`RPT_SEVERITY_UNDEFINED` when the rule says none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSeverity {
    #[default]
    Undefined,
    Ignore,
    Warning,
    Error,
    Exclusion,
}

impl From<Severity> for RuleSeverity {
    fn from(s: Severity) -> RuleSeverity {
        match s {
            Severity::Undefined => RuleSeverity::Undefined,
            Severity::Ignore => RuleSeverity::Ignore,
            Severity::Warning => RuleSeverity::Warning,
            Severity::Error => RuleSeverity::Error,
            Severity::Exclusion => RuleSeverity::Exclusion,
        }
    }
}

impl RuleSeverity {
    /// `DRC_RULE_SAVER::generateSeverityClause`.
    fn clause(self) -> &'static str {
        match self {
            RuleSeverity::Ignore => "(severity ignore)",
            RuleSeverity::Warning => "(severity warning)",
            RuleSeverity::Error => "(severity error)",
            RuleSeverity::Exclusion => "(severity exclusion)",
            RuleSeverity::Undefined => "",
        }
    }
}

/// One rule of the tree: a `RULE` node's data (`DRC_RE_BASE_CONSTRAINT_DATA` and its subclass) with what `SaveRulesToFile` makes of it
/// (`DRC_RE_LOADED_PANEL_ENTRY`). A rule of the file that two panels share (a track width and a clearance in one rule) is two entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub panel: PanelType,
    pub fields: Fields,
    pub rule_name: String,
    pub comment: String,
    /// `GetRuleCondition`: the condition the panel's rows show (for Via style, Vias under SMD and the two silk panels, without the part the panel writes itself).
    pub rule_condition: String,
    pub constraint_code: String,
    /// `GetLayers`, by the names the board file writes, in `PCB_LAYER_ID` order.
    pub layers: Vec<String>,
    /// `GetLayerSource`: the layer the rule names, as it names it (`outer`, `inner`, a layer's name); empty for none.
    pub layer_source: String,
    pub severity: RuleSeverity,
    /// The rule's text in the file, kept for the first panel of a rule only (`SetOriginalRuleText`).
    pub original_text: String,
    pub was_edited: bool,
    /// `GetGeneratedRule`: what the panel wrote when it was last saved.
    pub generated_rule: String,
    /// Which rule of the file (its index) the entry comes from; `None` for a rule the editor made. Not KiCad's: see the module's comment.
    pub source: Option<usize>,
}

impl Entry {
    /// A new rule of `panel` named `name` (`buildRuleTreeNode`: the factory's data, the panel's code).
    pub fn new(panel: PanelType, name: &str) -> Entry {
        Entry {
            panel,
            fields: Fields::default(),
            rule_name: name.to_string(),
            comment: String::new(),
            rule_condition: String::new(),
            constraint_code: panel.code().to_string(),
            layers: Vec::new(),
            layer_source: String::new(),
            severity: RuleSeverity::Undefined,
            original_text: String::new(),
            was_edited: false,
            generated_rule: String::new(),
            source: None,
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the board

/// What the editor knows of the board: its copper layers (`CuStack`), its enabled layers (`UIOrder`) and the names it gives them (`BOARD::GetLayerName`).
#[derive(Debug, Clone, Default)]
pub struct EditorBoard {
    pub copper: Vec<String>,
    pub enabled: Vec<String>,
    pub names: BTreeMap<String, String>,
}

impl EditorBoard {
    /// `BOARD::GetLayerName`: the name the user gave the layer, else KiCad's standard one.
    pub fn name(&self, layer: &str) -> String {
        self.names.get(layer).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| standard_layer_name(layer))
    }

    /// A two-layer board with every layer of a fresh board, for tests and for a board that names nothing.
    pub fn two_layer() -> EditorBoard {
        let mut enabled = vec!["F.Cu".to_string(), "B.Cu".to_string()];
        enabled.extend(eda_model::layers::TECH_LAYERS.iter().map(|l| l.to_string()));
        EditorBoard { copper: vec!["F.Cu".into(), "B.Cu".into()], enabled, names: BTreeMap::new() }
    }

    /// The parser's environment for the board: every layer by its file name and by the board's name of it (`ENUM_MAP<PCB_LAYER_ID>`).
    pub fn env(&self) -> Rc<Env> {
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut standard = HashMap::new();
        for l in crate::rule_board::all_layer_names() {
            entries.push((l.clone(), l.clone()));
            let shown = self.name(&l);
            entries.push((shown.clone(), l.clone()));
            standard.insert(l.clone(), shown);
        }
        Rc::new(Env::new(entries, standard, Box::new(|_| ("Default".to_string(), vec!["Default".to_string()]))))
    }
}

/// `IsFrontLayer`.
fn is_front_layer(l: &str) -> bool {
    matches!(l, "F.Cu" | "F.Adhes" | "F.Paste" | "F.SilkS" | "F.Mask" | "F.CrtYd" | "F.Fab")
}

/// `IsBackLayer`.
fn is_back_layer(l: &str) -> bool {
    matches!(l, "B.Cu" | "B.Adhes" | "B.Paste" | "B.SilkS" | "B.Mask" | "B.CrtYd" | "B.Fab")
}

fn by_layer_id(layers: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = layers.into_iter().collect::<BTreeSet<_>>().into_iter().collect();
    v.sort_by_key(|l| pcb_layer_id(l).unwrap_or(u32::MAX));
    v
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the layer selector

/// One choice of the layer selector: `token` is what the dialog sends back (empty for Any, `outer`, `inner`, `top`, `bottom` or a layer's file name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerOption {
    pub token: String,
    pub label: String,
}

/// `PANEL_DRC_RULE_EDITOR::populateLayerSelector`: "Any", then the category's layers.
pub fn layer_options(category: LayerCategory, board: &EditorBoard) -> Vec<LayerOption> {
    let opt = |t: &str, l: String| LayerOption { token: t.to_string(), label: l };
    let mut v = vec![opt("", "Any".into())];
    match category {
        LayerCategory::CopperOnly => {
            v.push(opt("outer", "outer".into()));
            v.push(opt("inner", "inner".into()));
            for l in &board.copper {
                v.push(opt(l, board.name(l)));
            }
        }
        LayerCategory::SilkscreenOnly => {
            v.push(opt("F.SilkS", board.name("F.SilkS")));
            v.push(opt("B.SilkS", board.name("B.SilkS")));
        }
        LayerCategory::SoldermaskOnly => {
            v.push(opt("F.Mask", board.name("F.Mask")));
            v.push(opt("B.Mask", board.name("B.Mask")));
        }
        LayerCategory::SolderpasteOnly => {
            v.push(opt("F.Paste", board.name("F.Paste")));
            v.push(opt("B.Paste", board.name("B.Paste")));
        }
        LayerCategory::TopBottomAny => {
            v.push(opt("top", "Top".into()));
            v.push(opt("bottom", "Bottom".into()));
        }
        LayerCategory::GeneralAnyLayer => {
            v.push(opt("outer", "outer".into()));
            v.push(opt("inner", "inner".into()));
            for l in &board.enabled {
                v.push(opt(l, board.name(l)));
            }
        }
        LayerCategory::NoLayerSelector => {}
    }
    v
}

/// `DRC_RULE_EDITOR_UTILS::TranslateTopBottomLayer`.
fn translate_top_bottom(panel: PanelType, top: bool) -> String {
    match panel {
        PanelType::CourtyardClearance => format!("(layer \"{}\")", if top { "F.CrtYd" } else { "B.CrtYd" }),
        PanelType::SilkToSoldermaskClearance => format!(
            "(condition \"A.Layer == '{}' && B.Layer == '{}'\")",
            if top { "F.SilkS" } else { "B.SilkS" },
            if top { "F.Mask" } else { "B.Mask" }
        ),
        PanelType::ViasUnderSmd | PanelType::AllowedOrientation => format!("(layer \"{}\")", if top { "F.Cu" } else { "B.Cu" }),
        _ => String::new(),
    }
}

/// `PANEL_DRC_RULE_EDITOR::buildLayerClause`.
pub fn layer_clause(panel: PanelType, token: &str, board: &EditorBoard) -> String {
    match token {
        "" => String::new(),
        "outer" => "(layer outer)".into(),
        "inner" => "(layer inner)".into(),
        "top" => translate_top_bottom(panel, true),
        "bottom" => translate_top_bottom(panel, false),
        layer => format!("(layer \"{}\")", board.name(layer)),
    }
}

/// `PANEL_DRC_RULE_EDITOR::getSelectedLayers`.
pub fn selected_layers(panel: PanelType, token: &str) -> Vec<String> {
    let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
    match token {
        "" if panel.layer_category() == LayerCategory::SilkscreenOnly => v(&["F.SilkS", "B.SilkS"]),
        "" => Vec::new(),
        "outer" => v(&["F.Cu", "B.Cu"]),
        "inner" => v(&["In1.Cu"]),
        "top" => v(&["F.Cu"]),
        "bottom" => v(&["B.Cu"]),
        layer => vec![layer.to_string()],
    }
}

/// `PANEL_DRC_RULE_EDITOR::getSelectedLayerSource`.
pub fn selected_layer_source(panel: PanelType, token: &str, board: &EditorBoard) -> String {
    match token {
        "" => String::new(),
        "outer" | "inner" => token.to_string(),
        "top" => match panel {
            PanelType::CourtyardClearance => "F.CrtYd".into(),
            PanelType::SilkToSoldermaskClearance => "F.SilkS".into(),
            PanelType::ViasUnderSmd | PanelType::AllowedOrientation => "F.Cu".into(),
            _ => board.name("F.Cu"),
        },
        "bottom" => match panel {
            PanelType::CourtyardClearance => "B.CrtYd".into(),
            PanelType::SilkToSoldermaskClearance => "B.SilkS".into(),
            PanelType::ViasUnderSmd | PanelType::AllowedOrientation => "B.Cu".into(),
            _ => board.name("B.Cu"),
        },
        layer => board.name(layer),
    }
}

/// `PANEL_DRC_RULE_EDITOR::setSelectedLayers`: the choice the selector shows for a rule's layers and layer source.
pub fn selected_token(panel: PanelType, layers: &[String], layer_source: &str, board: &EditorBoard) -> String {
    let options = layer_options(panel.layer_category(), board);
    let has = |t: &str| options.iter().skip(1).any(|o| o.token == t);
    for kw in ["outer", "inner"] {
        if layer_source == kw && has(kw) {
            return kw.to_string();
        }
    }
    if let Some(target) = layers.first() {
        if is_front_layer(target) || is_back_layer(target) {
            let pseudo = if is_front_layer(target) { "top" } else { "bottom" };
            if has(pseudo) {
                return pseudo.to_string();
            }
        }
        if has(target) {
            return target.clone();
        }
    }
    String::new()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- writing a rule

/// `RULE_GENERATION_CONTEXT`.
#[derive(Debug, Clone, Default)]
pub struct GenerationContext {
    pub rule_name: String,
    pub comment: String,
    pub condition: String,
    pub layer_clause: String,
    pub constraint_code: String,
}

/// `EscapeString( .., CTX_QUOTED_STR )`: a double quote becomes `{dblquote}`.
fn quote_string(s: &str) -> String {
    s.replace('"', "{dblquote}")
}

/// `DRC_RE_BASE_CONSTRAINT_DATA::trimTrailingZeros`.
fn trim_trailing_zeros(s: String) -> String {
    let mut r = s;
    if !r.contains('.') {
        return r;
    }
    while r.chars().count() > 1 && r.ends_with('0') {
        r.pop();
    }
    if r.ends_with('.') {
        r.pop();
    }
    r
}

/// `DRC_RE_BASE_CONSTRAINT_DATA::formatDouble`: `wxString::FromCDouble( value, 6 )` with the trailing zeros taken off.
pub fn format_double(v: f64) -> String {
    trim_trailing_zeros(format!("{v:.6}"))
}

fn mm(v: f64) -> String {
    format_double(v) + "mm"
}

/// `DRC_RE_BASE_CONSTRAINT_DATA::formatRuleName`: a bare symbol (letters, digits, `_`, `-`, `.`, not starting with a digit) as it is, anything else quoted.
pub fn format_rule_name(name: &str) -> String {
    if name.is_empty() {
        return "\"\"".into();
    }
    let needs_quoting = name.chars().next().is_some_and(|c| c.is_ascii_digit()) || name.chars().any(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
    if !needs_quoting {
        return name.to_string();
    }
    format!("\"{}\"", quote_string(name))
}

/// `DRC_RE_BASE_CONSTRAINT_DATA::buildRule`.
fn build_rule(ctx: &GenerationContext, clauses: &[String]) -> String {
    let mut rule = format!("(rule {}\n", format_rule_name(&ctx.rule_name));
    for line in ctx.comment.split('\n') {
        if !line.is_empty() {
            rule += &format!("\t# {line}\n");
        }
    }
    let mut cond = ctx.condition.clone();
    if !ctx.layer_clause.is_empty() {
        if ctx.layer_clause.starts_with("(condition ") {
            // `AfterFirst( '"' ).BeforeLast( '"' )`.
            let after = ctx.layer_clause.split_once('"').map_or("", |(_, a)| a);
            let expr = after.rsplit_once('"').map_or("", |(b, _)| b).to_string();
            cond = if cond.is_empty() { expr } else { format!("{expr} && {cond}") };
        } else {
            rule += &format!("\t{}\n", ctx.layer_clause);
        }
    }
    for c in clauses.iter().filter(|c| !c.is_empty()) {
        rule += &format!("\t{c}\n");
    }
    if !cond.is_empty() {
        rule += &format!("\t(condition \"{}\")\n", quote_string(&cond));
    }
    rule += ")";
    rule
}

/// `GetConstraintClauses` of the panel's data class.
pub fn constraint_clauses(panel: PanelType, f: &Fields, code: &str) -> Vec<String> {
    match panel.data_kind() {
        DataKind::Numeric => {
            // DRC_RE_NUMERIC_INPUT_CONSTRAINT_DATA
            let value = if code == "via_count" || code == "min_resolved_spokes" { format!("{}", f.value as i32) } else { format_double(f.value) };
            match code {
                "via_count" => vec![format!("(constraint {code} (max {value}))")],
                "min_resolved_spokes" => vec![format!("(constraint {code} {value})")],
                "track_angle" => vec![format!("(constraint {code} (min {value}deg))")],
                _ => vec![format!("(constraint {code} (min {value}mm))")],
            }
        }
        DataKind::ViaStyle => {
            let mut v = Vec::new();
            if f.min_via_diameter > 0.0 && f.max_via_diameter > 0.0 {
                v.push(format!("(constraint via_diameter (min {}) (max {}))", mm(f.min_via_diameter), mm(f.max_via_diameter)));
            }
            if f.min_via_hole > 0.0 && f.max_via_hole > 0.0 {
                v.push(format!("(constraint hole_size (min {}) (max {}))", mm(f.min_via_hole), mm(f.max_via_hole)));
            }
            v
        }
        DataKind::TextHeightThickness => vec![
            format!("(constraint text_height (min {}))", mm(f.min_text_height)),
            format!("(constraint text_thickness (min {}))", mm(f.min_text_thickness)),
        ],
        DataKind::RoutingWidth => {
            let code = if code.is_empty() { "track_width" } else { code };
            vec![format!(
                "(constraint {code} (min {}) (opt {}) (max {}))",
                mm(f.opt_width - f.width_tolerance),
                mm(f.opt_width),
                mm(f.opt_width + f.width_tolerance)
            )]
        }
        DataKind::RoutingDiffPair => {
            let mut v = Vec::new();
            if f.opt_width > 0.0 {
                v.push(format!(
                    "(constraint track_width (min {}) (opt {}) (max {}))",
                    mm(f.opt_width - f.width_tolerance),
                    mm(f.opt_width),
                    mm(f.opt_width + f.width_tolerance)
                ));
            }
            if f.opt_gap > 0.0 {
                v.push(format!("(constraint diff_pair_gap (min {}) (opt {}) (max {}))", mm(f.opt_gap - f.gap_tolerance), mm(f.opt_gap), mm(f.opt_gap + f.gap_tolerance)));
            }
            if f.max_uncoupled > 0.0 {
                v.push(format!("(constraint diff_pair_uncoupled (max {}))", mm(f.max_uncoupled)));
            }
            v
        }
        DataKind::AbsoluteLength => absolute_length_clauses(f, code),
        DataKind::MatchedLengthDiffPair => {
            let mut v = if f.opt_length > 0.0 { absolute_length_clauses(f, code) } else { Vec::new() };
            if f.max_skew > 0.0 {
                if f.within_diff_pairs {
                    v.push(format!("(constraint skew (max {}mm) (within_diff_pairs))", format_double(f.max_skew)));
                } else {
                    v.push(format!("(constraint skew (max {}mm))", format_double(f.max_skew)));
                }
            }
            v
        }
        DataKind::PermittedLayers => {
            let mut terms = Vec::new();
            if f.top_layer {
                terms.push("A.Layer == 'F.Cu'");
            }
            if f.bottom_layer {
                terms.push("A.Layer == 'B.Cu'");
            }
            if terms.is_empty() {
                return Vec::new();
            }
            vec![format!("(constraint assertion \"{}\")", terms.join(" || "))]
        }
        DataKind::AllowedOrientation => {
            let mut terms = Vec::new();
            for (on, t) in [(f.deg0, "A.Orientation == 0 deg"), (f.deg90, "A.Orientation == 90 deg"), (f.deg180, "A.Orientation == 180 deg"), (f.deg270, "A.Orientation == 270 deg")] {
                if on {
                    terms.push(t);
                }
            }
            if terms.is_empty() {
                return Vec::new();
            }
            vec![format!("(constraint assertion \"{}\")", terms.join(" || "))]
        }
        DataKind::ViasUnderSmd => {
            let (t, m, b, u) = (f.through_vias, f.micro_vias, f.blind_vias, f.buried_vias);
            if !t && !m && !b && !u {
                return Vec::new();
            }
            if t && m && b && u {
                return vec!["(constraint disallow via)".into()];
            }
            let mut items = String::new();
            for (on, k) in [(t, " through_via"), (m, " micro_via"), (b, " blind_via"), (u, " buried_via")] {
                if on {
                    items += k;
                }
            }
            vec![format!("(constraint disallow {})", items.trim_start())]
        }
        // DRC_RE_BASE_CONSTRAINT_DATA's: none.
        DataKind::Custom => Vec::new(),
    }
}

/// `DRC_RE_ABSOLUTE_LENGTH_TWO_CONSTRAINT_DATA::GetConstraintClauses`.
fn absolute_length_clauses(f: &Fields, code: &str) -> Vec<String> {
    let code = if code.is_empty() { "length" } else { code };
    vec![format!("(constraint {code} (min {}) (opt {}) (max {}))", mm(f.opt_length - f.tolerance), mm(f.opt_length), mm(f.opt_length + f.tolerance))]
}

/// A Custom rule's `(rule "name" ... body)`: the name always quoted, a `"` in it escaped with a backslash.
fn custom_rule(ctx: &GenerationContext, body: &str) -> String {
    let mut rule = format!("(rule \"{}\"\n", ctx.rule_name.replace('"', "\\\""));
    for line in ctx.comment.split('\n') {
        if !line.is_empty() {
            rule += &format!("\t# {line}\n");
        }
    }
    rule += body;
    rule += ")";
    rule
}

/// The data class's `GenerateRule`.
pub fn generate_rule(panel: PanelType, f: &Fields, code: &str, ctx: &GenerationContext) -> String {
    match panel.data_kind() {
        DataKind::ViaStyle => {
            let mut c = ctx.clone();
            let via = f.via_type.condition();
            if !via.is_empty() {
                c.condition = if c.condition.is_empty() { via.to_string() } else { format!("{via} && {}", c.condition) };
            }
            build_rule(&c, &constraint_clauses(panel, f, code))
        }
        DataKind::ViasUnderSmd => {
            let mut c = ctx.clone();
            let pad = "A.Pad_Type == 'SMD'";
            c.condition = if c.condition.is_empty() { pad.to_string() } else { format!("{pad} && {}", c.condition) };
            build_rule(&c, &constraint_clauses(panel, f, code))
        }
        // DRC_RE_CUSTOM_RULE_CONSTRAINT_DATA::GenerateRule
        DataKind::Custom => {
            if f.rule_text.is_empty() {
                String::new()
            } else {
                custom_rule(ctx, &f.rule_text)
            }
        }
        _ => build_rule(ctx, &constraint_clauses(panel, f, code)),
    }
}

/// `DRC_RE_CUSTOM_RULE_PANEL::GenerateRule`: the panel's own (its text right-trimmed by the `Trim()` of the emptiness test), which is what Save keeps.
fn generate_custom_panel_rule(ctx: &GenerationContext, body: &str) -> String {
    let body = body.trim_end();
    if body.is_empty() {
        return String::new();
    }
    custom_rule(ctx, body)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- checks

/// The data class's `Validate`: KiCad's messages, in KiCad's order.
pub fn validate(panel: PanelType, f: &Fields) -> Vec<String> {
    let mut e: Vec<String> = Vec::new();
    let mut add = |m: &str| e.push(m.to_string());
    match panel.data_kind() {
        DataKind::Numeric => {
            if f.value <= 0.0 {
                add("Numeric input value must be greater than 0");
            }
        }
        DataKind::ViaStyle => {
            let has_dia = f.min_via_diameter > 0.0 || f.max_via_diameter > 0.0;
            let has_hole = f.min_via_hole > 0.0 || f.max_via_hole > 0.0;
            if has_dia {
                if f.min_via_diameter <= 0.0 {
                    add("Minimum Via Diameter must be greater than 0");
                }
                if f.max_via_diameter <= 0.0 {
                    add("Maximum Via Diameter must be greater than 0");
                }
                if f.min_via_diameter > 0.0 && f.max_via_diameter > 0.0 && f.min_via_diameter > f.max_via_diameter {
                    add("Minimum Via Diameter cannot be greater than Maximum Via Diameter");
                }
            }
            if has_hole {
                if f.min_via_hole <= 0.0 {
                    add("Minimum Via Hole Size must be greater than 0");
                }
                if f.max_via_hole <= 0.0 {
                    add("Maximum Via Hole Size must be greater than 0");
                }
                if f.min_via_hole > 0.0 && f.max_via_hole > 0.0 && f.min_via_hole > f.max_via_hole {
                    add("Minimum Via Hole Size cannot be greater than Maximum Via Hole Size");
                }
            }
            if !has_dia && !has_hole {
                add("At least one constraint must be specified");
            }
        }
        DataKind::TextHeightThickness => {
            if f.min_text_height <= 0.0 {
                add("Minimum Text Height must be greater than 0");
            }
            if f.min_text_thickness <= 0.0 {
                add("Minimum Text Thickness must be greater than 0");
            }
        }
        DataKind::RoutingWidth => {
            if f.opt_width <= 0.0 {
                add("Optimum Width must be greater than 0");
            }
            if f.width_tolerance < 0.0 {
                add("Width Tolerance must be greater than or equal to 0");
            }
            if f.width_tolerance >= f.opt_width {
                add("Width Tolerance must be less than Optimum Width");
            }
        }
        DataKind::RoutingDiffPair => {
            let has_width = f.opt_width > 0.0;
            if has_width {
                if f.width_tolerance < 0.0 {
                    add("Width Tolerance must be greater than or equal to 0");
                }
                if f.width_tolerance >= f.opt_width {
                    add("Width Tolerance must be less than Optimum Width");
                }
            }
            let has_gap = f.opt_gap > 0.0;
            if has_gap {
                if f.gap_tolerance < 0.0 {
                    add("Gap Tolerance must be greater than or equal to 0");
                }
                if f.gap_tolerance >= f.opt_gap {
                    add("Gap Tolerance must be less than Optimum Gap");
                }
            }
            if f.max_uncoupled < 0.0 {
                add("Maximum Uncoupled Length must be greater than or equal to 0");
            }
            if !has_width && !has_gap && f.max_uncoupled <= 0.0 {
                add("At least one constraint must be specified");
            }
        }
        DataKind::AbsoluteLength => return validate_absolute_length(f),
        DataKind::MatchedLengthDiffPair => {
            // DRC_RE_MATCHED_LENGTH_DIFF_PAIR_CONSTRAINT_DATA::Validate: the base's result replaces what was found before it.
            let has_length = f.opt_length > 0.0 || f.tolerance > 0.0;
            let has_skew = f.max_skew != 0.0;
            if !has_length && !has_skew {
                add("At least one constraint must be defined");
            }
            if has_length {
                e = validate_absolute_length(f);
            }
            if f.max_skew < 0.0 {
                e.push("Maximum Skew must be greater than or equal to 0".into());
            }
        }
        DataKind::PermittedLayers => {
            if !f.top_layer && !f.bottom_layer {
                add("At least one layer must be selected");
            }
        }
        DataKind::AllowedOrientation => {
            if !f.deg0 && !f.deg90 && !f.deg180 && !f.deg270 {
                add("At least one orientation must be selected");
            }
        }
        DataKind::ViasUnderSmd => {
            if !f.through_vias && !f.micro_vias && !f.blind_vias && !f.buried_vias {
                add("At least one via type must be selected");
            }
        }
        DataKind::Custom => {
            if f.rule_text.is_empty() {
                add("Rule text cannot be empty");
            }
        }
    }
    e
}

/// `DRC_RE_ABSOLUTE_LENGTH_TWO_CONSTRAINT_DATA::Validate`.
fn validate_absolute_length(f: &Fields) -> Vec<String> {
    let mut e: Vec<String> = Vec::new();
    if f.opt_length <= 0.0 {
        e.push("Optimum Length must be greater than 0".into());
    }
    if f.tolerance < 0.0 {
        e.push("Tolerance must be greater than or equal to 0".into());
    }
    if e.is_empty() && f.opt_length - f.tolerance <= 0.0 {
        e.push("Tolerance is too large: resulting minimum length is not positive".into());
    }
    e
}

/// `DRC_RULE_EDITOR_UTILS::FormatErrorMessage`.
fn numbered(n: usize, message: &str) -> String {
    format!("{n}. {message}\n")
}

/// `VALIDATOR_NUMERIC_CTRL::Validate` as `DRC_RE_NUMERIC_INPUT_OVERLAY_PANEL::ValidateInputs` words it: the panel's one value field, as typed.
fn numeric_text_problem(text: &str, integer_only: bool) -> Option<&'static str> {
    let value = text.trim();
    if value.is_empty() {
        return Some("Value is required.");
    }
    let buf = value.replace(',', ".");
    if integer_only {
        match parse_long(&buf) {
            None => Some("Value must be a whole number."),
            Some(v) if v <= 0 => Some("Value must be greater than 0."),
            _ => None,
        }
    } else {
        match buf.parse::<f64>() {
            Ok(v) if v.is_finite() => (v <= 0.0).then_some("Value must be greater than 0."),
            _ => Some("Value must be a number."),
        }
    }
}

/// `wxString::ToLong`: the whole string a decimal integer.
fn parse_long(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the loader

/// `MINOPTMAX<int>::Min()`, `Max()`, `Opt()` and `toMM`.
fn min_of(c: &Constraint) -> i64 {
    c.value.min.unwrap_or(0)
}
fn max_of(c: &Constraint) -> i64 {
    c.value.max.unwrap_or(i32::MAX as i64)
}
fn opt_of(c: &Constraint) -> i64 {
    c.value.opt.unwrap_or_else(|| min_of(c))
}
fn to_mm(v: i64) -> f64 {
    v as f64 / 1_000_000.0
}

fn find(rule: &Rule, t: CType) -> Option<&Constraint> {
    rule.constraints.iter().find(|c| c.ctype == t)
}

/// `isSymmetricMinOptMax`.
fn is_symmetric(c: Option<&Constraint>) -> bool {
    let Some(c) = c else { return true };
    let (Some(min), Some(opt), Some(max)) = (c.value.min, c.value.opt, c.value.max) else { return true };
    opt - min == max - opt
}

/// `DRC_PANEL_CLAIM`, in `initClaims`'s order.
struct Claim {
    panel: PanelType,
    required: &'static [CType],
    optional: &'static [CType],
    priority: i32,
}

fn claims() -> Vec<Claim> {
    use CType::*;
    use PanelType as P;
    let c = |panel, required, optional, priority| Claim { panel, required, optional, priority };
    let mut v = vec![
        c(P::RoutingDiffPair, &[TrackWidth, DiffPairGap][..], &[MaxUncoupled, Skew][..], 100),
        c(P::ViaStyle, &[ViaDiameter, HoleSize], &[], 90),
        c(P::MinimumTextHeightAndThickness, &[TextHeight, TextThickness], &[], 80),
        c(P::RoutingWidth, &[TrackWidth], &[], 70),
        c(P::AbsoluteLength, &[Length], &[], 60),
        c(P::PermittedLayers, &[Assertion], &[], 50),
        c(P::AllowedOrientation, &[Assertion], &[], 40),
        c(P::MinimumClearance, &[Clearance], &[], 30),
        c(P::CopperToEdgeClearance, &[EdgeClearance], &[], 30),
        c(P::CopperToHoleClearance, &[HoleClearance], &[], 30),
        c(P::HoleToHoleDistance, &[HoleToHole], &[], 30),
        c(P::CourtyardClearance, &[CourtyardClearance], &[], 30),
        c(P::MinimumSoldermaskSliver, &[SolderMaskSliver], &[], 30),
        c(P::PhysicalClearance, &[PhysicalClearance], &[], 30),
        c(P::SilkToSilkClearance, &[SilkClearance], &[], 30),
        c(P::CreepageDistance, &[Creepage], &[], 30),
        c(P::HoleSize, &[HoleSize], &[], 20),
        c(P::MinimumViaDiameter, &[ViaDiameter], &[], 20),
        c(P::MinimumAnnularWidth, &[AnnularWidth], &[], 20),
        c(P::MinimumConnectionWidth, &[ConnectionWidth], &[], 20),
        c(P::MaximumViaCount, &[ViaCount], &[], 20),
        c(P::SoldermaskExpansion, &[SolderMaskExpansion], &[], 20),
        c(P::SolderpasteExpansion, &[SolderPasteAbsMargin], &[], 20),
        c(P::MinimumThermalReliefSpokeCount, &[MinResolvedSpokes], &[], 20),
        c(P::MatchedLengthDiffPair, &[Length, Skew], &[], 65),
        c(P::MatchedLengthDiffPair, &[Skew], &[], 55),
        c(P::RoutingDiffPair, &[DiffPairGap], &[MaxUncoupled], 15),
        c(P::RoutingDiffPair, &[MaxUncoupled], &[], 10),
        c(P::ViasUnderSmd, &[Disallow], &[], 5),
    ];
    // Highest priority first. (A stable sort: see the module's comment.)
    v.sort_by(|a, b| b.priority.cmp(&a.priority));
    v
}

/// `DRC_PANEL_MATCHER::MatchRule`: the panels a rule's constraints go to, each with the constraints it claims; what no panel claims goes to a Custom rule.
pub fn match_rule(rule: &Rule) -> Vec<(PanelType, BTreeSet<CType>)> {
    let mut matches = Vec::new();
    let mut remaining: BTreeSet<CType> = rule.constraints.iter().map(|c| c.ctype).filter(|t| *t != CType::Null).collect();
    if remaining.is_empty() {
        return matches;
    }
    let condition = rule.condition.as_ref().map(|c| c.expression().to_string()).unwrap_or_default();
    for claim in claims() {
        if remaining.is_empty() {
            break;
        }
        if !claim.required.iter().all(|t| remaining.contains(t)) {
            continue;
        }
        let mut claimed: BTreeSet<CType> = claim.required.iter().copied().collect();
        claimed.extend(claim.optional.iter().filter(|t| remaining.contains(t)));
        if claimed.is_empty() {
            continue;
        }
        // VIAS_UNDER_SMD requires Pad_Type == 'SMD' in the condition.
        if claim.panel == PanelType::ViasUnderSmd && !condition.contains("Pad_Type == 'SMD'") {
            continue;
        }
        for t in &claimed {
            remaining.remove(t);
        }
        matches.push((claim.panel, claimed));
    }
    if !remaining.is_empty() {
        matches.push((PanelType::CustomRule, remaining));
    }
    matches
}

/// `DRC_RULE_LOADER::cleanStrippedCondition`.
fn clean_stripped_condition(c: &str) -> String {
    let mut s = c.to_string();
    loop {
        let next = s.replace("()", "");
        if next == s {
            break;
        }
        s = next;
    }
    s = s.replace("&& &&", "&&").replace("|| ||", "||");
    let mut s = s.trim().to_string();
    if let Some(r) = s.strip_prefix("&&") {
        s = r.trim_start().to_string();
    }
    if let Some(r) = s.strip_suffix("&&") {
        s = r.trim_end().to_string();
    }
    if let Some(r) = s.strip_prefix("||") {
        s = r.trim_start().to_string();
    }
    if let Some(r) = s.strip_suffix("||") {
        s = r.trim_end().to_string();
    }
    s
}

/// `DRC_RULE_LOADER::ExtractRuleBody`: what follows the rule's name up to its last `)`.
pub fn extract_rule_body(original: &str) -> String {
    let Some(kw) = original.find("rule ") else { return original.to_string() };
    let Some(start) = original[kw + 5..].find('(').map(|p| p + kw + 5) else { return original.to_string() };
    let mut body = original[start..].trim_end();
    if let Some(b) = body.strip_suffix(')') {
        body = b;
    }
    body.trim_end().to_string()
}

/// `DRC_RULE_LOADER::ExtractRuleComment`: the `#` lines of a rule, without the `#` and the blanks after it.
pub fn extract_rule_comment(original: &str) -> String {
    let mut comment = String::new();
    for line in original.split('\n') {
        if let Some(rest) = line.trim_start().strip_prefix('#') {
            if !comment.is_empty() {
                comment.push('\n');
            }
            comment += rest.trim_start();
        }
    }
    comment
}

/// `DRC_RULE_LOADER::ExtractRuleText`: the rule's whole `(rule ...)` in the file, by its name -- here its `occurrence`-th one (from 0), where KiCad takes
/// the first (see the module's comment). Empty when it is not found or its parentheses do not close.
pub fn extract_rule_text(content: &str, name: &str, occurrence: usize) -> String {
    let quoted = format!("(rule \"{name}\"");
    let unquoted = format!("(rule {name}");
    let mut starts: Vec<usize> = content.match_indices(&quoted).map(|(i, _)| i).collect();
    if starts.is_empty() {
        let mut pos = 0;
        while let Some(i) = content[pos..].find(&unquoted).map(|i| i + pos) {
            let after = i + unquoted.len();
            match content[after..].chars().next() {
                None | Some(')' | ' ' | '\n' | '\r' | '\t') => starts.push(i),
                _ => {}
            }
            pos = after;
        }
    }
    let Some(&start) = starts.get(occurrence) else { return String::new() };
    let (mut depth, mut in_string, mut escaped) = (0i32, false, false);
    for (i, c) in content[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return content[start..start + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// `DRC_RULE_LOADER::createConstraintData`: the panel's values from the rule's constraints, or `None` for a Custom rule's (the fallback the loader takes
/// when the panel cannot show the rule).
fn create_data(panel: PanelType, rule: &Rule, claimed: &BTreeSet<CType>) -> Option<(Fields, String, Option<String>)> {
    let mut f = Fields::default();
    let condition = rule.condition.as_ref().map(|c| c.expression().to_string());
    // (fields, constraint code, the condition the panel keeps when it sets one itself)
    match panel {
        PanelType::ViaStyle => {
            if let Some(c) = find(rule, CType::ViaDiameter) {
                f.min_via_diameter = to_mm(min_of(c));
                f.max_via_diameter = to_mm(max_of(c));
            }
            if let Some(c) = find(rule, CType::HoleSize) {
                f.min_via_hole = to_mm(min_of(c));
                f.max_via_hole = to_mm(max_of(c));
            }
            let mut kept = None;
            if let Some(expr) = condition {
                f.via_type = if expr.contains("'Micro'") {
                    ViaType::Micro
                } else if expr.contains("'Through'") {
                    ViaType::Through
                } else if expr.contains("'Blind'") {
                    ViaType::Blind
                } else if expr.contains("'Buried'") {
                    ViaType::Buried
                } else {
                    ViaType::Any
                };
                let mut cleaned = expr.clone();
                for t in ["A.Via_Type == 'Micro'", "A.Via_Type == 'Through'", "A.Via_Type == 'Blind'", "A.Via_Type == 'Buried'"] {
                    cleaned = cleaned.replace(t, "");
                }
                kept = Some(clean_stripped_condition(&cleaned));
            }
            Some((f, "via_style".into(), kept))
        }
        PanelType::RoutingDiffPair => {
            let tw = find(rule, CType::TrackWidth);
            let gap = find(rule, CType::DiffPairGap);
            if !is_symmetric(tw) || !is_symmetric(gap) {
                return None;
            }
            if let Some(c) = tw {
                let (opt, min) = (to_mm(opt_of(c)), to_mm(min_of(c)));
                f.opt_width = opt;
                f.width_tolerance = opt - min;
            }
            if let Some(c) = gap {
                let (opt, min) = (to_mm(opt_of(c)), to_mm(min_of(c)));
                f.opt_gap = opt;
                f.gap_tolerance = opt - min;
            }
            if let Some(c) = find(rule, CType::MaxUncoupled) {
                f.max_uncoupled = to_mm(max_of(c));
            }
            Some((f, "diff_pair_gap".into(), None))
        }
        PanelType::MinimumTextHeightAndThickness => {
            if let Some(c) = find(rule, CType::TextHeight) {
                f.min_text_height = to_mm(min_of(c));
            }
            if let Some(c) = find(rule, CType::TextThickness) {
                f.min_text_thickness = to_mm(min_of(c));
            }
            Some((f, "text_height".into(), None))
        }
        PanelType::RoutingWidth => {
            let tw = find(rule, CType::TrackWidth);
            if !is_symmetric(tw) {
                return None;
            }
            if let Some(c) = tw {
                let (opt, min) = (to_mm(opt_of(c)), to_mm(min_of(c)));
                f.opt_width = opt;
                f.width_tolerance = opt - min;
            }
            Some((f, "track_width".into(), None))
        }
        PanelType::AbsoluteLength | PanelType::MatchedLengthDiffPair => {
            let len = find(rule, CType::Length);
            if !is_symmetric(len) {
                return None;
            }
            if let Some(c) = len {
                let (min, opt, max) = (to_mm(min_of(c)), to_mm(opt_of(c)), to_mm(max_of(c)));
                f.opt_length = opt;
                f.tolerance = (max - min) / 2.0;
            }
            if panel == PanelType::MatchedLengthDiffPair {
                if let Some(c) = find(rule, CType::Skew) {
                    f.max_skew = to_mm(max_of(c));
                    f.within_diff_pairs = c.options & OPT_SKEW_WITHIN_DIFF_PAIRS != 0;
                }
            }
            Some((f, "length".into(), None))
        }
        PanelType::PermittedLayers => {
            if let Some(expr) = find(rule, CType::Assertion).and_then(|c| c.test.as_ref()).map(|t| t.expression().to_string()) {
                f.top_layer = expr.contains("F.Cu");
                f.bottom_layer = expr.contains("B.Cu");
                // Layer references the panel can't represent: a Custom rule.
                let remaining = expr.replace("A.Layer == 'F.Cu'", "").replace("A.Layer == 'B.Cu'", "");
                if remaining.contains("A.Layer") {
                    return None;
                }
            }
            Some((f, String::new(), None))
        }
        PanelType::AllowedOrientation => {
            match find(rule, CType::Assertion).and_then(|c| c.test.as_ref()).map(|t| t.expression().to_string()) {
                Some(expr) => {
                    f.deg0 = expr.contains("== 0 deg");
                    f.deg90 = expr.contains("== 90 deg");
                    f.deg180 = expr.contains("== 180 deg");
                    f.deg270 = expr.contains("== 270 deg");
                    if f.deg0 && f.deg90 && f.deg180 && f.deg270 {
                        f.all_degrees = true;
                    } else if !(f.deg0 || f.deg90 || f.deg180 || f.deg270) {
                        // Non-standard angles cannot be represented by the orientation panel.
                        return None;
                    }
                }
                None => {
                    f.all_degrees = true;
                    f.deg0 = true;
                    f.deg90 = true;
                    f.deg180 = true;
                    f.deg270 = true;
                }
            }
            Some((f, "allowed_orientation".into(), None))
        }
        PanelType::ViasUnderSmd => {
            if let Some(c) = find(rule, CType::Disallow) {
                f.through_vias = c.disallow & disallow::THROUGH_VIAS != 0;
                f.micro_vias = c.disallow & disallow::MICRO_VIAS != 0;
                f.blind_vias = c.disallow & disallow::BLIND_VIAS != 0;
                f.buried_vias = c.disallow & disallow::BURIED_VIAS != 0;
            }
            Some((f, "disallow_via".into(), None))
        }
        PanelType::CustomRule => None,
        p if p.is_numeric_input() => {
            // The first matching constraint of the claimed set.
            for t in claimed {
                if let Some(c) = find(rule, *t) {
                    f.value = match t {
                        CType::ViaCount => max_of(c) as f64,
                        CType::MinResolvedSpokes => min_of(c) as f64,
                        _ => to_mm(min_of(c)),
                    };
                    break;
                }
            }
            Some((f, p.code().to_string(), None))
        }
        _ => None,
    }
}

fn layer_source_of(rule: &Rule) -> String {
    let s = &rule.layer_source;
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].to_string()
    } else {
        s.clone()
    }
}

/// `DRC_RULE_LOADER::LoadRule` followed by what `DIALOG_DRC_RULE_EDITOR::LoadExistingRules` does with each entry: the panels of one rule of the file.
pub fn load_rule(rule: &Rule, original: &str, source: usize) -> Vec<Entry> {
    let condition = rule.condition.as_ref().map(|c| c.expression().to_string()).unwrap_or_default();
    let layer_source = layer_source_of(rule);
    let comment = extract_rule_comment(original);
    // `entry.layerCondition.Seq()`, when the rule names a layer.
    let rule_layers: Vec<String> = if layer_source.is_empty() { Vec::new() } else { by_layer_id(rule.layers.iter().flatten().cloned()) };
    let mut entries: Vec<Entry> = Vec::new();
    let base = |panel: PanelType| {
        let mut e = Entry::new(panel, &rule.name);
        e.constraint_code = String::new();
        e.severity = rule.severity.into();
        e.source = Some(source);
        e
    };

    for (mut panel, claimed) in match_rule(rule) {
        if panel == PanelType::PermittedLayers && claimed.contains(&CType::Assertion) {
            if let Some(expr) = find(rule, CType::Assertion).and_then(|c| c.test.as_ref()).map(|t| t.expression()) {
                if expr.contains("Orientation") && !expr.contains("Layer") {
                    panel = PanelType::AllowedOrientation;
                }
            }
        }
        if panel == PanelType::SilkToSilkClearance && claimed.contains(&CType::SilkClearance) {
            if !condition.is_empty() && (condition.contains("L == 'F.Mask'") || condition.contains("L == 'B.Mask'")) {
                panel = PanelType::SilkToSoldermaskClearance;
            } else if !condition.is_empty() && !condition.contains("L == 'F.SilkS'") && !condition.contains("L == 'B.SilkS'") {
                panel = PanelType::CustomRule;
            }
        }
        // Not KiCad's: a panel with no node in the tree goes to the node that writes its constraint (see the module's comment).
        if panel == PanelType::HoleSize {
            panel = if find(rule, CType::HoleSize).is_some_and(|c| c.value.max.is_none()) { PanelType::MinimumDrillSize } else { PanelType::CustomRule };
        }
        if panel == PanelType::MinimumViaDiameter {
            panel = PanelType::ViaStyle;
        }
        let created = create_data(panel, rule, &claimed);
        let mut e = base(panel);
        let mut kept_condition = None;
        match created {
            Some((fields, code, kept)) => {
                e.fields = fields;
                e.constraint_code = code;
                kept_condition = kept;
            }
            None => {
                // The custom-rule fallback.
                e.panel = PanelType::CustomRule;
                panel = PanelType::CustomRule;
                e.fields.rule_text = extract_rule_body(original);
            }
        }
        let mut data_layers: Vec<String> = Vec::new();
        if panel == PanelType::SilkToSoldermaskClearance {
            let mut cleaned = condition.clone();
            if condition.contains("L == 'F.Mask' || L == 'B.Mask'") {
                data_layers = vec!["F.SilkS".into(), "B.SilkS".into()];
                cleaned = cleaned.replace("L == 'F.Mask' || L == 'B.Mask'", "");
            } else {
                data_layers = vec![if condition.contains("F.Mask") { "F.SilkS".into() } else { "B.SilkS".into() }];
                cleaned = cleaned.replace("L == 'F.Mask'", "").replace("L == 'B.Mask'", "");
            }
            kept_condition = Some(clean_stripped_condition(&cleaned));
        }
        if panel == PanelType::SilkToSilkClearance {
            let mut cleaned = condition.clone();
            if condition.contains("L == 'F.SilkS' || L == 'B.SilkS'") {
                data_layers = vec!["F.SilkS".into(), "B.SilkS".into()];
                cleaned = cleaned.replace("L == 'F.SilkS' || L == 'B.SilkS'", "");
            } else if condition.contains("L == 'F.SilkS'") || condition.contains("L == 'B.SilkS'") {
                data_layers = vec![if condition.contains("F.SilkS") { "F.SilkS".into() } else { "B.SilkS".into() }];
                cleaned = cleaned.replace("L == 'F.SilkS'", "").replace("L == 'B.SilkS'", "");
            }
            kept_condition = Some(clean_stripped_condition(&cleaned));
        }
        if panel == PanelType::ViasUnderSmd {
            let cleaned = condition.replace("A.Pad_Type == 'SMD'", "").replace("B.Pad_Type == 'SMD'", "");
            kept_condition = Some(clean_stripped_condition(&cleaned));
        }
        e.rule_condition = match panel {
            PanelType::ViaStyle | PanelType::SilkToSoldermaskClearance | PanelType::SilkToSilkClearance | PanelType::ViasUnderSmd => kept_condition.unwrap_or_default(),
            _ => condition.clone(),
        };
        // LoadExistingRules: the rule's own layer source replaces the panel's, and its layers the panel's when it names one.
        e.layer_source = layer_source.clone();
        e.layers = if layer_source.is_empty() { data_layers } else { rule_layers.clone() };
        if !comment.is_empty() {
            e.comment = comment.clone();
        }
        if entries.is_empty() {
            e.original_text = original.to_string();
        }
        entries.push(e);
    }

    if entries.is_empty() {
        let mut e = base(PanelType::CustomRule);
        e.rule_condition = condition;
        e.comment = comment;
        e.fields.rule_text = extract_rule_body(original);
        e.layer_source = layer_source;
        e.layers = rule_layers;
        e.original_text = original.to_string();
        entries.push(e);
    }
    entries
}

/// `DRC_RULE_LOADER::LoadFromString` (and the tree `LoadExistingRules` builds of it): every panel of every rule of the file, in the file's order. A file
/// that does not parse is the parser's error (see the module's comment).
pub fn load(text: &str, env: &Rc<Env>) -> Result<Vec<Entry>, ParseError> {
    let mut rules_text = text.to_string();
    if !rules_text.contains("(version") {
        rules_text.insert_str(0, "(version 1)\n");
    }
    let parsed = dru::parse_rules(&rules_text, env)?;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut entries = Vec::new();
    for (i, rule) in parsed.rules.iter().enumerate() {
        let n = seen.entry(rule.name.clone()).or_insert(0);
        let original = extract_rule_text(text, &rule.name, *n);
        *n += 1;
        entries.extend(load_rule(rule, &original, i));
    }
    Ok(entries)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the saver

/// `formatLayerClause` (`drc_re_rule_saver.cpp`).
fn format_layer_clause(source: &str) -> String {
    if source.is_empty() {
        return String::new();
    }
    if source == "outer" || source == "inner" {
        return format!("(layer {source})");
    }
    format!("(layer \"{}\")", quote_string(source))
}

/// `DRC_RULE_SAVER::generateLayerClause`.
fn generate_layer_clause(layers: &[String], board: &EditorBoard) -> String {
    if layers.is_empty() {
        return String::new();
    }
    let cu: BTreeSet<&str> = layers.iter().map(String::as_str).filter(|l| l.ends_with(".Cu")).collect();
    let outer: BTreeSet<&str> = ["F.Cu", "B.Cu"].into_iter().collect();
    if cu == outer {
        return "(layer outer)".into();
    }
    let inner: Vec<String> = (1..=30).map(|n| format!("In{n}.Cu")).collect();
    if cu.len() == 30 && inner.iter().all(|l| cu.contains(l.as_str())) {
        return "(layer inner)".into();
    }
    // The parser takes one layer name: the first one.
    match by_layer_id(layers.iter().cloned()).first() {
        Some(l) => format!("(layer \"{}\")", board.name(l)),
        None => String::new(),
    }
}

fn silk_condition(e: &Entry, front: &str, back: &str) -> String {
    let has = |l: &str| e.layers.iter().any(|x| x == l);
    let (f, b) = (has("F.SilkS"), has("B.SilkS"));
    if f && !b {
        format!("L == '{front}'")
    } else if b && !f {
        format!("L == '{back}'")
    } else {
        format!("L == '{front}' || L == '{back}'")
    }
}

/// `DRC_RULE_SAVER::generateRuleText`.
fn generate_rule_text(e: &Entry, board: &EditorBoard) -> String {
    // Round-trip preservation: the original text of a rule nobody edited.
    if !e.was_edited && !e.original_text.is_empty() {
        return e.original_text.clone();
    }
    let mut text = e.generated_rule.clone();
    let silk = matches!(e.panel, PanelType::SilkToSoldermaskClearance | PanelType::SilkToSilkClearance);
    if text.is_empty() || silk {
        let mut ctx = GenerationContext { rule_name: e.rule_name.clone(), comment: e.comment.clone(), condition: e.rule_condition.clone(), constraint_code: e.constraint_code.clone(), ..Default::default() };
        if silk {
            let cond = if e.panel == PanelType::SilkToSoldermaskClearance { silk_condition(e, "F.Mask", "B.Mask") } else { silk_condition(e, "F.SilkS", "B.SilkS") };
            ctx.condition = if ctx.condition.is_empty() { cond } else { format!("({cond}) && {}", ctx.condition) };
        } else if !e.layer_source.is_empty() {
            ctx.layer_clause = format_layer_clause(&e.layer_source);
        } else {
            ctx.layer_clause = generate_layer_clause(&e.layers, board);
        }
        text = generate_rule(e.panel, &e.fields, &e.constraint_code, &ctx);
    }
    if !matches!(e.severity, RuleSeverity::Undefined | RuleSeverity::Error) {
        let clause = e.severity.clause();
        if !clause.is_empty() && !text.contains("(severity") {
            if let Some(last) = text.rfind(')') {
                text = format!("{}\n\t{clause})", &text[..last]);
            }
        }
    }
    text
}

/// `DRC_RULE_SAVER::generateMergedRuleText`: the panels of one rule written as one.
fn generate_merged_rule_text(group: &[&Entry], board: &EditorBoard) -> String {
    let Some(first) = group.first() else { return String::new() };
    if group.iter().all(|e| !e.was_edited) && !first.original_text.is_empty() {
        return first.original_text.clone();
    }
    let mut layer_clause = String::new();
    for e in group {
        if !e.layers.is_empty() {
            layer_clause = if !e.layer_source.is_empty() { format_layer_clause(&e.layer_source) } else { generate_layer_clause(&e.layers, board) };
            break;
        }
    }
    let mut rule = format!("(rule {}\n", format_rule_name(&first.rule_name));
    if !layer_clause.is_empty() {
        rule += &format!("\t{layer_clause}\n");
    }
    for e in group {
        for c in constraint_clauses(e.panel, &e.fields, &e.constraint_code) {
            if !c.is_empty() {
                rule += &format!("\t{c}\n");
            }
        }
    }
    if !first.rule_condition.is_empty() {
        rule += &format!("\t(condition \"{}\")\n", quote_string(&first.rule_condition));
    }
    if let Some(e) = group.iter().find(|e| !matches!(e.severity, RuleSeverity::Undefined | RuleSeverity::Error)) {
        rule += &format!("\t{}\n", e.severity.clause());
    }
    rule += ")";
    rule
}

/// `DRC_RULE_SAVER::GenerateRulesText`: the file. The panels of one rule (one name, condition and layer, and -- not KiCad's -- one rule of the file) are
/// written together.
pub fn save(entries: &[Entry], board: &EditorBoard) -> String {
    let mut out = String::from("(version 1)\n");
    let mut order: Vec<(String, String, String, Option<usize>)> = Vec::new();
    let mut groups: HashMap<(String, String, String, Option<usize>), Vec<&Entry>> = HashMap::new();
    for e in entries {
        let key = (e.rule_name.clone(), e.rule_condition.clone(), e.layer_source.clone(), e.source);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(e);
    }
    for key in order {
        let group = &groups[&key];
        let text = if group.len() == 1 { generate_rule_text(group[0], board) } else { generate_merged_rule_text(group, board) };
        if !text.is_empty() {
            out += &text;
            out.push('\n');
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- editing

/// What the dialog's rule panel holds when Save is pressed: `PANEL_DRC_RULE_EDITOR`'s name, comment, conditions (as the condition rows build them),
/// layer choice and its constraint panel's values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleEdit {
    pub panel: PanelType,
    pub name: String,
    #[serde(default)]
    pub comment: String,
    /// `DRC_RE_CONDITION_GROUP_PANEL::BuildCondition`.
    #[serde(default)]
    pub condition: String,
    /// The layer selector's choice ([`LayerOption::token`]).
    #[serde(default)]
    pub layer: String,
    #[serde(default)]
    pub fields: Fields,
    /// A numeric panel's value as typed (`VALIDATOR_NUMERIC_CTRL` checks the text); when absent, `fields.value` is taken as typed.
    #[serde(default)]
    pub value_text: Option<String>,
    /// The names of the tree's other rules the file does not have yet (new rules not saved), for `validateRuleName`.
    #[serde(default)]
    pub taken_names: Vec<String>,
    /// For a new rule: the severity and constraint code it came with (a duplicate's are its source's, `CopyFrom`).
    #[serde(default)]
    pub severity: Option<RuleSeverity>,
    #[serde(default)]
    pub constraint_code: Option<String>,
}

/// What Save says of a rule panel: `onSaveButtonClicked`'s validation message (empty when it passes) and the rule it writes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SaveCheck {
    pub valid: bool,
    pub message: String,
    pub generated: String,
}

/// The entry Save makes of the panel: `TransferDataFromWindow`, then `UpdateRuleTypeTreeItemData` (edited, no longer new).
fn edited_entry(base: &Entry, edit: &RuleEdit, board: &EditorBoard) -> Entry {
    let mut e = base.clone();
    e.panel = edit.panel;
    let mut f = edit.fields.clone();
    if edit.panel.data_kind() == DataKind::Numeric {
        if let Some(text) = &edit.value_text {
            let t = text.trim().replace(',', ".");
            f.value = if edit.panel.is_integer_only() { parse_long(&t).unwrap_or(0) as f64 } else { t.parse::<f64>().unwrap_or(0.0) };
        }
    }
    if edit.panel.data_kind() == DataKind::AllowedOrientation {
        f.all_degrees = f.deg0 && f.deg90 && f.deg180 && f.deg270;
    }
    e.fields = f;
    e.rule_name = edit.name.clone();
    e.comment = edit.comment.clone();
    e.layers = selected_layers(edit.panel, &edit.layer);
    e.layer_source = selected_layer_source(edit.panel, &edit.layer, board);
    e.rule_condition = edit.condition.clone();
    let ctx = GenerationContext {
        rule_name: edit.name.clone(),
        comment: edit.comment.clone(),
        condition: edit.condition.clone(),
        layer_clause: layer_clause(edit.panel, &edit.layer, board),
        constraint_code: e.constraint_code.clone(),
    };
    e.generated_rule =
        if edit.panel == PanelType::CustomRule { generate_custom_panel_rule(&ctx, &e.fields.rule_text) } else { generate_rule(edit.panel, &e.fields, &e.constraint_code, &ctx) };
    e.was_edited = true;
    e
}

/// The entry an edit starts from: the file's, or a new rule's.
fn base_entry(entries: &[Entry], at: Option<usize>, edit: &RuleEdit) -> Result<Entry, String> {
    match at {
        Some(i) => entries.get(i).cloned().ok_or_else(|| format!("the rules file has no rule {i}; reopen the editor")),
        None => {
            let mut e = Entry::new(edit.panel, &edit.name);
            if let Some(s) = edit.severity {
                e.severity = s;
            }
            if let Some(c) = &edit.constraint_code {
                e.constraint_code = c.clone();
            }
            Ok(e)
        }
    }
}

/// `PANEL_DRC_RULE_EDITOR::onSaveButtonClicked`: the rule panel's checks (`ValidateInputs`: the name, the layer) and its constraint panel's (the
/// overlay panel's `ValidateInputs`, which runs the data's `Validate`), numbered as KiCad numbers them -- the constraint panel restarts the count.
pub fn check_edit(entries: &[Entry], at: Option<usize>, edit: &RuleEdit, board: &EditorBoard) -> Result<SaveCheck, String> {
    let base = base_entry(entries, at, edit)?;
    let mut count = 0usize;
    let mut message = String::new();
    let mut valid = true;
    // validateRuleName: no other rule of the tree has the name.
    let taken = entries.iter().enumerate().any(|(i, e)| Some(i) != at && e.rule_name == edit.name) || edit.taken_names.iter().any(|n| *n == edit.name);
    if taken {
        valid = false;
        count += 1;
        message += &numbered(count, "Rule name must be unique.");
    }
    if !layer_options(edit.panel.layer_category(), board).iter().any(|o| o.token == edit.layer) {
        valid = false;
        count += 1;
        message += &numbered(count, "Layer selection is required.");
    }
    let edited = edited_entry(&base, edit, board);
    let mut panel_ok = true;
    if edit.panel.data_kind() == DataKind::Numeric {
        let text = edit.value_text.clone().unwrap_or_else(|| format_double(edit.fields.value));
        if let Some(problem) = numeric_text_problem(&text, edit.panel.is_integer_only()) {
            count += 1;
            message += &numbered(count, problem);
            panel_ok = false;
        }
    }
    if panel_ok && edit.panel != PanelType::CustomRule {
        let errors = validate(edit.panel, &edited.fields);
        if !errors.is_empty() {
            for (i, err) in errors.iter().enumerate() {
                message += &numbered(i + 1, err);
            }
            panel_ok = false;
        }
    }
    Ok(SaveCheck { valid: valid && panel_ok, message, generated: edited.generated_rule })
}

/// Save on a rule panel (`saveRule`): the rule as the panel has it replaces the file's (`at`) or is added after the others (a new rule), and the file is
/// written again (`SaveRulesToFile`). The validation message when it does not pass.
pub fn apply_edit(entries: &mut Vec<Entry>, at: Option<usize>, edit: &RuleEdit, board: &EditorBoard) -> Result<(), String> {
    let check = check_edit(entries, at, edit, board)?;
    if !check.valid {
        return Err(check.message);
    }
    let base = base_entry(entries, at, edit)?;
    let e = edited_entry(&base, edit, board);
    match at {
        Some(i) => entries[i] = e,
        None => entries.push(e),
    }
    Ok(())
}

/// `DIALOG_DRC_RULE_EDITOR::RemoveRule`.
pub fn delete_entry(entries: &mut Vec<Entry>, at: usize) -> Result<(), String> {
    if at >= entries.len() {
        return Err(format!("the rules file has no rule {at}; reopen the editor"));
    }
    entries.remove(at);
    Ok(())
}

/// Move Up / Move Down: the rule changes places with the one before (after) it under the same tree node. KiCad's moves the tree item only and the file
/// keeps its order (`onMoveUpRuleOptionClick` leaves `m_ruleTreeNodeDatas` as it is); here the file's order follows, which is the order the DRC reads the
/// rules in. `None` when there is no such rule.
pub fn move_entry(entries: &mut [Entry], at: usize, up: bool) -> Option<usize> {
    let panel = entries.get(at)?.panel;
    let other = if up { (0..at).rev().find(|&i| entries[i].panel == panel)? } else { (at + 1..entries.len()).find(|&i| entries[i].panel == panel)? };
    entries.swap(at, other);
    Some(other)
}

/// `DIALOG_DRC_RULE_EDITOR::buildRuleTreeNode`'s name: `base 1`, `base 2`, .. the first no node of the tree has. `base` is the constraint node's name for
/// a new rule, the source rule's name without a trailing ` <number>` for a duplicate (`DuplicateRule`).
pub fn unique_rule_name(base: &str, taken: &[String]) -> String {
    let mut n = 1;
    loop {
        let name = format!("{base} {n}");
        if !taken.iter().any(|t| *t == name) {
            return name;
        }
        n += 1;
    }
}

/// `DuplicateRule`'s base name: a trailing ` <number>` taken off.
pub fn duplicate_base_name(name: &str) -> String {
    if let Some((head, tail)) = name.rsplit_once(' ') {
        if tail.parse::<i64>().is_ok() {
            return head.to_string();
        }
    }
    name.to_string()
}

/// `DRC_RE_CUSTOM_RULE_PANEL::onCheckSyntax`: the panel's text as a rule of its own through the parser: "Syntax OK", or the error.
pub fn check_custom_syntax(name: &str, body: &str, env: &Rc<Env>) -> String {
    if body.trim_end().is_empty() {
        return "No rule text to check.".into();
    }
    let text = format!("(version 1)\n(rule \"{}\"\n{body})", name.replace('"', "\\\""));
    match dru::parse_rules(&text, env) {
        Ok(_) => "Syntax OK".into(),
        Err(e) => format!("ERROR at line {}, column {}: {}", e.line, e.offset, e.message),
    }
}

#[cfg(test)]
#[path = "rule_editor_tests.rs"]
mod tests;
