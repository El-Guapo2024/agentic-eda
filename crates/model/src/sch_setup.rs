//! The project settings of Schematic Setup's pages (`eeschema/dialogs/dialog_schematic_setup.cpp`) that have no home in the drawing: General > Formatting's
//! sizes and ratios (`PANEL_SETUP_FORMATTING`), General > Annotation (`PANEL_EESCHEMA_ANNOTATION_OPTIONS`), General > Field Name Templates
//! (`PANEL_TEMPLATE_FIELDNAMES`), and the schematic's share of a net class (`PANEL_SETUP_NETCLASSES` with `aIsEEschema`: the wire and bus widths, the
//! colour and the line style). KiCad keeps the first three in `SCHEMATIC_SETTINGS` (`eeschema/schematic_settings.cpp`, the `.kicad_pro`'s `schematic`
//! object) and the last in `NETCLASS` (`common/netclass.cpp`, `net_settings.classes[]`).
//!
//! The design keeps each as an overlay with serde defaults: absent is KiCad's default, so a design that never opened the page reads and writes as before.
//! `eda_kicad::project_schematic` reads them from an imported project and `eda_kicad::pcb::export_kicad_pro_for` writes them to the derived one.

use serde::{Deserialize, Serialize};

use crate::ir::Nm;

pub use crate::units::NM_PER_MIL;

/// `junction_size_mult_list` (`schematic_settings.cpp`): the junction dot's diameter is the Default net class's wire width times this, by
/// `m_JunctionSizeChoice` (None, Smallest, Small, Default, Large, Largest).
pub const JUNCTION_SIZE_MULT: [f64; 6] = [0.0, 1.7, 4.0, 6.0, 9.0, 12.0];
/// `hopover_size_mult_list`: the hop-over's scale by `m_HopOverSizeChoice` (None, Smallest, Small, Medium, Large, Largest).
pub const HOP_OVER_SIZE_MULT: [f64; 6] = [0.0, 1.7, 4.0, 6.0, 9.0, 12.0];

/// The voltage ranges of the Operating-point Overlay (`m_vRangeCtrlChoices` of `panel_setup_formatting_base.cpp`); "~V" is "Auto".
pub const OPO_V_RANGES: [&str; 12] = ["~V", "fV", "pV", "nV", "uV", "mV", "V", "KV", "MV", "GV", "TV", "PV"];
/// The current ranges (`m_iRangeCtrlChoices`); "~A" is "Auto".
pub const OPO_I_RANGES: [&str; 12] = ["~A", "fA", "pA", "nA", "uA", "mA", "A", "KA", "MA", "GA", "TA", "PA"];

/// `eeschema/default_values.h`: `DEFAULT_TEXT_SIZE` (50 mils), `DEFAULT_LINE_WIDTH_MILS` (6), the pin symbol size (`DEFAULT_TEXT_SIZE / 2`),
/// `DEFAULT_TEXT_OFFSET_RATIO` (0.15), `DEFAULT_LABEL_SIZE_RATIO` (0.375), and `FONT_METRICS::m_OverbarHeight` (1.23).
pub const DEFAULT_TEXT_SIZE_MILS: i64 = 50;
pub const DEFAULT_LINE_WIDTH_MILS: i64 = 6;
pub const DEFAULT_PIN_SYMBOL_SIZE_MILS: i64 = 25;
pub const DEFAULT_TEXT_OFFSET_RATIO: f64 = 0.15;
pub const DEFAULT_LABEL_SIZE_RATIO: f64 = 0.375;
pub const DEFAULT_OVERBAR_OFFSET_RATIO: f64 = 1.23;
/// `DEFAULT_WIRE_WIDTH_MILS` (6) and `DEFAULT_BUS_WIDTH_MILS` (12): the Default net class's widths (`NETCLASS( .., aInitWithDefaults )`).
pub const DEFAULT_WIRE_WIDTH_MILS: i64 = 6;
pub const DEFAULT_BUS_WIDTH_MILS: i64 = 12;

const fn mils(n: i64) -> Nm {
    Nm::new(n * NM_PER_MIL)
}

/// General > Formatting, the settings of the page other than the connection grid and the inter-sheet references (which `SchExtras` keeps on their own:
/// `connection_grid_mils`, `intersheet_refs`). Each field is a `SCHEMATIC_SETTINGS` member, named by its `.kicad_pro` key in the doc; the defaults are the
/// constructor's (with no user configuration, as kicad-cli runs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchFormatting {
    /// Text > "Default text size:" (`m_DefaultTextSize`, `drawing.default_text_size`, mils in the file): the size a new label, text or sheet pin starts at.
    #[serde(default = "d_text_size", deserialize_with = "crate::units::de")]
    pub default_text_size_nm: Nm,
    /// Text > "Overbar offset ratio:" (`m_FontMetrics.m_OverbarHeight`, `drawing.overbar_offset_ratio`), shown as a percentage.
    #[serde(default = "d_overbar")]
    pub overbar_offset_ratio: f64,
    /// Text > "Label offset ratio:" (`m_TextOffsetRatio`, `drawing.text_offset_ratio`): "Percentage of the text size to offset labels above (or below) a
    /// wire, bus, or pin".
    #[serde(default = "d_text_offset")]
    pub text_offset_ratio: f64,
    /// Text > "Global label margin ratio:" (`m_LabelSizeRatio`, `drawing.label_size_ratio`): "Percentage of the text size to use as space around a global label".
    #[serde(default = "d_label_size")]
    pub label_size_ratio: f64,
    /// Symbols > "Default line width:" (`m_DefaultLineWidth`, `drawing.default_line_thickness`): the pen of a symbol's body and of a graphic item whose own is 0.
    #[serde(default = "d_line_width", deserialize_with = "crate::units::de")]
    pub default_line_width_nm: Nm,
    /// Symbols > "Pin symbol size:" (`m_PinSymbolSize`, `drawing.pin_symbol_size`): the size of a pin's inverted bubble, clock wedge and other decorations.
    #[serde(default = "d_pin_symbol", deserialize_with = "crate::units::de")]
    pub pin_symbol_size_nm: Nm,
    /// Connections > "Junction dot size:" (`m_JunctionSizeChoice`, `drawing.junction_size_choice`): None, Smallest, Small, Default, Large, Largest.
    #[serde(default = "d_junction")]
    pub junction_size_choice: u32,
    /// Connections > "Hop-over size:" (`m_HopOverSizeChoice`, `drawing.hop_over_size_choice`): None, Smallest, Small, Medium, Large, Largest.
    #[serde(default)]
    pub hop_over_size_choice: u32,
    /// Dashed Lines > "Dash length:" (`m_DashedLineDashRatio`, `drawing.dashed_lines_dash_length_ratio`), a ratio of the line width ("Default from ISO 128-2").
    #[serde(default = "d_dash")]
    pub dash_ratio: f64,
    /// Dashed Lines > "Gap length:" (`m_DashedLineGapRatio`, `drawing.dashed_lines_gap_length_ratio`).
    #[serde(default = "d_gap")]
    pub gap_ratio: f64,
    /// Operating-point Overlay > "Significant digits (voltages):" (`m_OPO_VPrecision`, `drawing.operating_point_overlay_v_precision`), 1 to 10.
    #[serde(default = "d_precision")]
    pub opo_v_precision: u32,
    /// "Range (voltages):" (`m_OPO_VRange`, `drawing.operating_point_overlay_v_range`): one of [`OPO_V_RANGES`], "~V" for Auto.
    #[serde(default = "d_v_range")]
    pub opo_v_range: String,
    /// "Significant digits (currents):" (`m_OPO_IPrecision`, `drawing.operating_point_overlay_i_precision`).
    #[serde(default = "d_precision")]
    pub opo_i_precision: u32,
    /// "Range (currents):" (`m_OPO_IRange`, `drawing.operating_point_overlay_i_range`): one of [`OPO_I_RANGES`], "~A" for Auto.
    #[serde(default = "d_i_range")]
    pub opo_i_range: String,
}

/// The ratios are never NaN (the verb refuses one, and the reader keeps the default), so equality is an equivalence.
impl Eq for SchFormatting {}

fn d_text_size() -> Nm {
    mils(DEFAULT_TEXT_SIZE_MILS)
}
fn d_overbar() -> f64 {
    DEFAULT_OVERBAR_OFFSET_RATIO
}
fn d_text_offset() -> f64 {
    DEFAULT_TEXT_OFFSET_RATIO
}
fn d_label_size() -> f64 {
    DEFAULT_LABEL_SIZE_RATIO
}
fn d_line_width() -> Nm {
    mils(DEFAULT_LINE_WIDTH_MILS)
}
fn d_pin_symbol() -> Nm {
    mils(DEFAULT_PIN_SYMBOL_SIZE_MILS)
}
fn d_junction() -> u32 {
    3
}
fn d_dash() -> f64 {
    12.0
}
fn d_gap() -> f64 {
    3.0
}
fn d_precision() -> u32 {
    3
}
fn d_v_range() -> String {
    "~V".into()
}
fn d_i_range() -> String {
    "~A".into()
}

impl Default for SchFormatting {
    /// `SCHEMATIC_SETTINGS::SCHEMATIC_SETTINGS` with no user configuration.
    fn default() -> Self {
        SchFormatting {
            default_text_size_nm: d_text_size(),
            overbar_offset_ratio: d_overbar(),
            text_offset_ratio: d_text_offset(),
            label_size_ratio: d_label_size(),
            default_line_width_nm: d_line_width(),
            pin_symbol_size_nm: d_pin_symbol(),
            junction_size_choice: d_junction(),
            hop_over_size_choice: 0,
            dash_ratio: d_dash(),
            gap_ratio: d_gap(),
            opo_v_precision: d_precision(),
            opo_v_range: d_v_range(),
            opo_i_precision: d_precision(),
            opo_i_range: d_i_range(),
        }
    }
}

impl SchFormatting {
    /// What the page's controls can hold, as a refusal for a command that sends something else: a junction or hop-over choice of the six
    /// (`wxChoice`), a precision of 1 to 10 (`wxSpinCtrl( .., 1, 10, 3 )`), a range of the list, a ratio that is a number. The page has no
    /// check of its own for these (`PANEL_SETUP_FORMATTING::TransferDataFromWindow` validates the connection grid alone), so neither has this
    /// a check KiCad lacks: a size out of range is stored as typed, and read back as KiCad reads it ([`Self::effective`]).
    pub fn validate(&self) -> Result<(), (&'static str, String)> {
        if self.junction_size_choice as usize >= JUNCTION_SIZE_MULT.len() {
            return Err(("junction_size_choice", format!("{} is not one of the six junction dot sizes", self.junction_size_choice)));
        }
        if self.hop_over_size_choice as usize >= HOP_OVER_SIZE_MULT.len() {
            return Err(("hop_over_size_choice", format!("{} is not one of the six hop-over sizes", self.hop_over_size_choice)));
        }
        for (what, p) in [("opo_v_precision", self.opo_v_precision), ("opo_i_precision", self.opo_i_precision)] {
            if !(1..=10).contains(&p) {
                return Err((what, format!("{p} significant digits: the field takes 1 to 10")));
            }
        }
        if !OPO_V_RANGES.contains(&self.opo_v_range.as_str()) {
            return Err(("opo_v_range", format!("{:?} is not a voltage range", self.opo_v_range)));
        }
        if !OPO_I_RANGES.contains(&self.opo_i_range.as_str()) {
            return Err(("opo_i_range", format!("{:?} is not a current range", self.opo_i_range)));
        }
        for (what, r) in [("overbar_offset_ratio", self.overbar_offset_ratio), ("text_offset_ratio", self.text_offset_ratio), ("label_size_ratio", self.label_size_ratio), ("dash_ratio", self.dash_ratio), ("gap_ratio", self.gap_ratio)] {
            if !r.is_finite() {
                return Err((what, "not a number".into()));
            }
        }
        Ok(())
    }

    /// The settings as KiCad holds them once the project is read again: each `PARAM_SCALED` / ranged `PARAM` gives its default for a value outside its range
    /// (`PARAM_SCALED::Load`: `if( val > m_max || val < m_min ) val = m_default`) -- the default line width and text size from 5 to 1000 mils, the pin symbol
    /// size from 0 to 1000 mils, the label offset and margin ratios from 0 to 2. So what the studio draws is what kicad-cli plots.
    pub fn effective(&self) -> SchFormatting {
        let d = SchFormatting::default();
        let in_mils = |v: Nm, lo: i64, hi: i64, default: Nm| if v < mils(lo) || v > mils(hi) { default } else { v };
        let ratio = |v: f64, lo: f64, hi: f64, default: f64| if !(lo..=hi).contains(&v) { default } else { v };
        SchFormatting {
            default_text_size_nm: in_mils(self.default_text_size_nm, 5, 1000, d.default_text_size_nm),
            default_line_width_nm: in_mils(self.default_line_width_nm, 5, 1000, d.default_line_width_nm),
            pin_symbol_size_nm: in_mils(self.pin_symbol_size_nm, 0, 1000, d.pin_symbol_size_nm),
            text_offset_ratio: ratio(self.text_offset_ratio, 0.0, 2.0, d.text_offset_ratio),
            label_size_ratio: ratio(self.label_size_ratio, 0.0, 2.0, d.label_size_ratio),
            ..self.clone()
        }
    }

    /// `SCHEMATIC_SETTINGS::GetJunctionSize`: the junction dot's diameter for a Default net class of wire width `default_wire_width`
    /// (`KiROUND( wire width * junction_size_mult_list[ choice ] )`, at least one internal unit -- 100 nm -- which `SCH_JUNCTION` takes for "draw none").
    pub fn junction_diameter(&self, default_wire_width: Nm) -> Nm {
        let mult = JUNCTION_SIZE_MULT.get(self.junction_size_choice as usize).copied().unwrap_or(6.0);
        // in KiCad's schematic internal units (100 nm), as `KiROUND` rounds them
        let iu = crate::units::kiround((default_wire_width.get() as f64 / 100.0) * mult);
        Nm::new(iu.max(1) * 100)
    }

    /// KiCad 25ec2335 `SCHEMATIC_SETTINGS::GetHopOverScale`: `hopover_size_mult_list[ m_HopOverSizeChoice ]`, the hop-over arc's radius as a multiple
    /// of the default line width (`SCH_PAINTER::draw( const SCH_LINE* )` and `SCH_SCREEN::Plot`: `arcRadius = defaultLineWidth * hopOverScale`). A
    /// choice past the six reads as None (`PARAM<int>( "drawing.hop_over_size_choice", .., 0, 5 )` gives its default for a value out of range).
    pub fn hop_over_scale(&self) -> f64 {
        HOP_OVER_SIZE_MULT.get(self.hop_over_size_choice as usize).copied().unwrap_or(0.0)
    }
}

/// General > Annotation (`PANEL_EESCHEMA_ANNOTATION_OPTIONS`) but the symbol unit notation, which `SchExtras` keeps as `subpart_id_separator` /
/// `subpart_first_id`. The Annotate dialog starts from these and leaves its choice in them (`DIALOG_ANNOTATE::TransferDataToWindow` / `~DIALOG_ANNOTATE`), and
/// placing or pasting a symbol numbers it by them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationSettings {
    /// Order: "Sort symbols by X position" (0, `SORT_BY_X_POSITION`) or "by Y position" (1) (`m_AnnotateSortOrder`, `annotation.sort_order`, 0 to 1).
    #[serde(default)]
    pub sort_order: u32,
    /// Numbering: "Use first free number after:" (0, `INCREMENTAL_BY_REF`), "First free after sheet number X 100" (1), "... X 1000" (2)
    /// (`m_AnnotateMethod`, `annotation.method`, 0 to 2).
    #[serde(default)]
    pub method: u32,
    /// The number after which the first free one is taken (`m_AnnotateStartNum`, `annotate_start_num`).
    #[serde(default)]
    pub start_num: i64,
    /// "Allow reference reuse" (`REFDES_TRACKER::GetReuseRefDes`, `reuse_designators`, true by default): a reference a deleted symbol had may be given again.
    #[serde(default = "yes")]
    pub reuse_designators: bool,
}

fn yes() -> bool {
    true
}

impl Default for AnnotationSettings {
    fn default() -> Self {
        AnnotationSettings { sort_order: 0, method: 0, start_num: 0, reuse_designators: true }
    }
}

impl AnnotationSettings {
    /// The page's radios hold an order of the two and a method of the three (`PARAM<int>( "annotation.sort_order", .., 0, 0, 1 )`, `( "annotation.method", .., 0, 0, 2 )`).
    pub fn validate(&self) -> Result<(), (&'static str, String)> {
        if self.sort_order > 1 {
            return Err(("sort_order", format!("{} is not an order (0 by X, 1 by Y)", self.sort_order)));
        }
        if self.method > 2 {
            return Err(("method", format!("{} is not a numbering (0 first free, 1 sheet x 100, 2 sheet x 1000)", self.method)));
        }
        Ok(())
    }
}

/// Electrical Rules > Compare Symbol with Library (KiCad 25ec2335 `PANEL_SETUP_SYMBOL_PARITY`, `SYMBOL_PARITY_SETTINGS`, the project's `schematic.compare_symbols`):
/// which differences between a placed symbol and its library symbol count -- for ERC's `lib_symbol_mismatch` (`ERC_TESTER::TestLibSymbolIssues`) and for
/// Compare Symbol with Library (`SCH_INSPECTION_TOOL::DiffSymbol`), both through `SCHEMATIC_SETTINGS::SymbolCompareFlags`. Each field is one box of the page;
/// the defaults are the `PARAM<bool>`s' (UI-LEFT-3 B3-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolParitySettings {
    /// "Check for missing fields" (`m_MissingFields`, `compare_symbols.missing_fields`, on).
    #[serde(default = "yes")]
    pub missing_fields: bool,
    /// "Check for extra fields" (`m_ExtraFields`, `extra_fields`, off).
    #[serde(default)]
    pub extra_fields: bool,
    /// "Compare non-reference field text" (`m_FieldTexts`, `field_texts`, on).
    #[serde(default = "yes")]
    pub field_texts: bool,
    /// "Compare field visibilities" (`m_FieldVisibilities`, `field_visibilities`, off).
    #[serde(default)]
    pub field_visibilities: bool,
    /// "Compare field text sizes and styles" (`m_FieldStyles`, `field_styles`, off).
    #[serde(default)]
    pub field_styles: bool,
    /// "Compare field positions" (`m_FieldPositions`, `field_positions`, off).
    #[serde(default)]
    pub field_positions: bool,
    /// "Compare pin name/number visibilities" (`m_PinVisibilities`, `pin_name_number_visibilities`, off).
    #[serde(default)]
    pub pin_visibilities: bool,
    /// "Compare alternate pin functions" (`m_PinAltFunctions`, `pin_alt_function_definitions`, on).
    #[serde(default = "yes")]
    pub pin_alt_functions: bool,
    /// "Compare exclude from board flags" (`m_ExcludeFromBoardFlags`, `exclude_from_board_flags`, on).
    #[serde(default = "yes")]
    pub exclude_from_board: bool,
    /// "Compare do not populate flags" (`m_DNPFlags`, `dnp_flags`, off).
    #[serde(default)]
    pub dnp: bool,
    /// "Compare exlucde from bill of materials flags" [sic] (`m_ExcludeFromBOMFlags`, `exclude_from_bom_flags`, off).
    #[serde(default)]
    pub exclude_from_bom: bool,
    /// "Compare exclude from position file flags" (`m_ExcludeFromPosFileFlags`, `exclude_from_position_file_flags`, on).
    #[serde(default = "yes")]
    pub exclude_from_pos_files: bool,
}

impl Default for SymbolParitySettings {
    fn default() -> Self {
        SymbolParitySettings {
            missing_fields: true,
            extra_fields: false,
            field_texts: true,
            field_visibilities: false,
            field_styles: false,
            field_positions: false,
            pin_visibilities: false,
            pin_alt_functions: true,
            exclude_from_board: true,
            dnp: false,
            exclude_from_bom: false,
            exclude_from_pos_files: true,
        }
    }
}

impl SymbolParitySettings {
    /// Each setting with the project file's key under `compare_symbols` (`SCHEMATIC_SETTINGS`' `PARAM<bool>( "compare_symbols.*" )`), in the file's order.
    pub fn keyed(&self) -> [(&'static str, bool); 12] {
        [
            ("missing_fields", self.missing_fields),
            ("extra_fields", self.extra_fields),
            ("field_texts", self.field_texts),
            ("field_visibilities", self.field_visibilities),
            ("field_styles", self.field_styles),
            ("field_positions", self.field_positions),
            ("pin_name_number_visibilities", self.pin_visibilities),
            ("pin_alt_function_definitions", self.pin_alt_functions),
            ("exclude_from_board_flags", self.exclude_from_board),
            ("dnp_flags", self.dnp),
            ("exclude_from_bom_flags", self.exclude_from_bom),
            ("exclude_from_position_file_flags", self.exclude_from_pos_files),
        ]
    }

    /// The settings a project's `compare_symbols` object holds, each its default where the key is missing or not a boolean (`PARAM<bool>`'s reader).
    pub fn from_project(compare: &serde_json::Value) -> SymbolParitySettings {
        let d = SymbolParitySettings::default();
        let flag = |k: &str, default: bool| compare.get(k).and_then(serde_json::Value::as_bool).unwrap_or(default);
        SymbolParitySettings {
            missing_fields: flag("missing_fields", d.missing_fields),
            extra_fields: flag("extra_fields", d.extra_fields),
            field_texts: flag("field_texts", d.field_texts),
            field_visibilities: flag("field_visibilities", d.field_visibilities),
            field_styles: flag("field_styles", d.field_styles),
            field_positions: flag("field_positions", d.field_positions),
            pin_visibilities: flag("pin_name_number_visibilities", d.pin_visibilities),
            pin_alt_functions: flag("pin_alt_function_definitions", d.pin_alt_functions),
            exclude_from_board: flag("exclude_from_board_flags", d.exclude_from_board),
            dnp: flag("dnp_flags", d.dnp),
            exclude_from_bom: flag("exclude_from_bom_flags", d.exclude_from_bom),
            exclude_from_pos_files: flag("exclude_from_position_file_flags", d.exclude_from_pos_files),
        }
    }
}

/// "Symbol unit notation:" (`m_choiceSeparatorRefIdChoices`: A, .A, -A, _A, .1, -1, _1) as `PANEL_EESCHEMA_ANNOTATION_OPTIONS::TransferDataFromWindow` sets it:
/// the first unit's id (`m_SubpartFirstId`, 'A' or '1') and the separator (`m_SubpartIdSeparator`, 0 for none).
pub const UNIT_NOTATIONS: [(&str, u32, u32); 7] = [("A", 'A' as u32, 0), (".A", 'A' as u32, '.' as u32), ("-A", 'A' as u32, '-' as u32), ("_A", 'A' as u32, '_' as u32), (".1", '1' as u32, '.' as u32), ("-1", '1' as u32, '-' as u32), ("_1", '1' as u32, '_' as u32)];

/// `PANEL_EESCHEMA_ANNOTATION_OPTIONS::loadEEschemaSettings`: the notation a project's first id and separator show as -- with 'A' first, the separator's
/// (none, '.', '-' or '_'; anything else is "A"); with any other first id, the separator's of the numbered three ('.' for anything else).
pub fn unit_notation_choice(first_id: u32, separator: u32) -> usize {
    if first_id == 'A' as u32 {
        match char::from_u32(separator) {
            Some('.') => 1,
            Some('-') => 2,
            Some('_') => 3,
            _ => 0,
        }
    } else {
        match char::from_u32(separator) {
            Some('-') => 5,
            Some('_') => 6,
            _ => 4,
        }
    }
}

/// One row of General > Field Name Templates (`TEMPLATE_FIELDNAME`, `drawing.field_names[]` of the project): a field every symbol is offered
/// (Symbol Properties adds the ones a symbol lacks, `DIALOG_SYMBOL_PROPERTIES::TransferDataToWindow`), shown or not, and whether its text is a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFieldName {
    pub name: String,
    /// "Visible" (`m_Visible`): a field added from the template is shown.
    #[serde(default)]
    pub visible: bool,
    /// "URL" (`m_URL`): the field's text is a link (`FIELDS_GRID_TABLE::GetAttr` gives it the URL editor).
    #[serde(default)]
    pub url: bool,
}

/// `MANDATORY_FIELDS` by `GetCanonicalFieldName`: a template of one of these names is no template (`TEMPLATES::AddTemplateFieldName` returns).
pub const MANDATORY_FIELD_NAMES: [&str; 5] = ["Reference", "Value", "Footprint", "Datasheet", "Description"];

/// `PANEL_TEMPLATE_FIELDNAMES::TransferDataFromWindow` over `TEMPLATES::AddTemplateFieldName`: the rows the page keeps -- every row with a name, in order,
/// but one named like a mandatory field, and a second row of a name already kept replacing the first in its place ("ensure uniqueness, overwrite any
/// template fieldname by the same name"). (A name with white space at an end is kept as typed when the user answers "Keep White Space"; the page asks
/// first, [`field_name_whitespace_warning`].)
pub fn kept_template_fields(rows: &[TemplateFieldName]) -> Vec<TemplateFieldName> {
    let mut out: Vec<TemplateFieldName> = Vec::new();
    for f in rows.iter().filter(|f| !f.name.is_empty() && !MANDATORY_FIELD_NAMES.contains(&f.name.as_str())) {
        match out.iter_mut().find(|k| k.name == f.name) {
            Some(k) => *k = f.clone(),
            None => out.push(f.clone()),
        }
    }
    out
}

/// The page's question for a name with white space at either end (`PANEL_TEMPLATE_FIELDNAMES::TransferDataFromWindow`), word for word.
pub fn field_name_whitespace_warning(name: &str) -> Option<String> {
    (!name.is_empty() && name.trim() != name).then(|| format!("The field name '{name}' contains trailing and/or leading white space."))
}

/// The schematic's share of a net class (`PANEL_SETUP_NETCLASSES`' Eeschema columns, `NETCLASS`'s optional settings): each `None` is "not set", which a class
/// takes from the next class of a net's effective class and in the end from the Default class (`NET_SETTINGS::makeEffectiveNetclass`). The Default class's
/// `None` is KiCad's default ([`DEFAULT_WIRE_WIDTH_MILS`], [`DEFAULT_BUS_WIDTH_MILS`], solid, the layer's colour).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetClassSchematic {
    /// "Wire Thickness" (`GRID_WIREWIDTH`, `wire_width` in mils in the file).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::units::de")]
    pub wire_width: Option<Nm>,
    /// "Bus Thickness" (`GRID_BUSWIDTH`, `bus_width`).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::units::de")]
    pub bus_width: Option<Nm>,
    /// "Color" (`GRID_SCHEMATIC_COLOR`, `schematic_color`): a CSS colour as `COLOR4D::ToCSSString` writes it (`rgb(r, g, b)`, `rgba(r, g, b, a)`); `None` is
    /// `COLOR4D::UNSPECIFIED` -- the wire layer's colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// "Line Style" (`GRID_LINESTYLE`, `line_style`): 0 solid, 1 dashed, 2 dotted, 3 dash-dot, 4 dash-dot-dot (`LINE_STYLE`); `None` is "<Not defined>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<i32>,
}

impl NetClassSchematic {
    /// Nothing set.
    pub fn is_empty(&self) -> bool {
        *self == NetClassSchematic::default()
    }

    /// Each setting this class leaves unset taken from `next` (`makeEffectiveNetclass`, a lower-priority constituent).
    pub fn or(&self, next: &NetClassSchematic) -> NetClassSchematic {
        NetClassSchematic {
            wire_width: self.wire_width.or(next.wire_width),
            bus_width: self.bus_width.or(next.bus_width),
            color: self.color.clone().or_else(|| next.color.clone()),
            line_style: self.line_style.or(next.line_style),
        }
    }

    /// The Default class's settings with KiCad's defaults for what it leaves unset (`NETCLASS::NETCLASS( Default, true )`: 6 and 12 mils, solid).
    pub fn default_class(own: Option<&NetClassSchematic>) -> NetClassSchematic {
        let d = NetClassSchematic { wire_width: Some(mils(DEFAULT_WIRE_WIDTH_MILS)), bus_width: Some(mils(DEFAULT_BUS_WIDTH_MILS)), color: None, line_style: Some(0) };
        own.map_or(d.clone(), |o| o.or(&d))
    }

    /// The panel's checks of these columns: a width is taken as typed (`GRID_CELL_MARK_AS_NULLABLE` reads a number, a 0 or a negative one included: UI-LEFT-2
    /// BB8, UI-LEFT-3 B3-4), a line style of the five, a colour as the file writes one.
    pub fn validate(&self) -> Result<(), String> {
        if self.line_style.is_some_and(|s| !(0..=4).contains(&s)) {
            return Err(format!("line style {} is not one of solid, dashed, dotted, dash-dot and dash-dot-dot", self.line_style.unwrap_or_default()));
        }
        if let Some(c) = &self.color {
            if parse_css_color(c).is_none() {
                return Err(format!("{c:?} is not a colour (rgb(r, g, b) or rgba(r, g, b, a))"));
            }
        }
        Ok(())
    }
}

/// A colour as `COLOR4D::ToCSSString` writes it -- `rgb(r, g, b)` or `rgba(r, g, b, a)` with `a` from 0 to 1 -- or `#rrggbb`: (r, g, b, a). `None` for
/// anything else.
pub fn parse_css_color(text: &str) -> Option<(u8, u8, u8, f64)> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() == 6 {
            let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some((b(0)?, b(2)?, b(4)?, 1.0));
        }
        return None;
    }
    let (body, with_alpha) = if let Some(b) = t.strip_prefix("rgba(") { (b, true) } else { (t.strip_prefix("rgb(")?, false) };
    let parts: Vec<&str> = body.strip_suffix(')')?.split(',').map(str::trim).collect();
    if parts.len() != if with_alpha { 4 } else { 3 } {
        return None;
    }
    let c = |s: &str| s.parse::<u8>().ok();
    let a = if with_alpha { parts[3].parse::<f64>().ok().filter(|a| (0.0..=1.0).contains(a))? } else { 1.0 };
    Some((c(parts[0])?, c(parts[1])?, c(parts[2])?, a))
}

/// A project file's colour as the model keeps it: `None` for `COLOR4D::UNSPECIFIED` (0, 0, 0, 0 -- what KiCad writes for a class with no colour, as
/// `rgba(0, 0, 0, 0.000)`) and for a text that is not a colour, else the colour as `COLOR4D::ToCSSString` writes it.
pub fn color_from_project(text: &str) -> Option<String> {
    let (r, g, b, a) = parse_css_color(text)?;
    if (r, g, b) == (0, 0, 0) && a == 0.0 {
        return None;
    }
    Some(css_color(r, g, b, a))
}

/// `COLOR4D::ToCSSString`: `rgb(r, g, b)` when opaque, else `rgba(r, g, b, a)` with `a` to three places.
pub fn css_color(r: u8, g: u8, b: u8, a: f64) -> String {
    let alpha = (a * 255.0).round() as u8;
    if alpha == 255 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", f64::from(alpha) / 255.0)
    }
}

/// `COLOR4D::UNSPECIFIED` as the project file writes it.
pub const UNSPECIFIED_COLOR: &str = "rgba(0, 0, 0, 0.000)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_schematic_settings_constructor() {
        let d = SchFormatting::default();
        assert_eq!(d.default_text_size_nm, Nm::new(1_270_000), "50 mils");
        assert_eq!(d.default_line_width_nm, Nm::new(152_400), "6 mils");
        assert_eq!(d.pin_symbol_size_nm, Nm::new(635_000), "25 mils");
        assert_eq!((d.junction_size_choice, d.hop_over_size_choice), (3, 0));
        assert_eq!((d.dash_ratio, d.gap_ratio, d.text_offset_ratio, d.label_size_ratio, d.overbar_offset_ratio), (12.0, 3.0, 0.15, 0.375, 1.23));
        assert_eq!((d.opo_v_precision, d.opo_v_range.as_str(), d.opo_i_precision, d.opo_i_range.as_str()), (3, "~V", 3, "~A"));
        let read: SchFormatting = serde_json::from_str("{}").unwrap();
        assert_eq!(read, d, "an empty page is KiCad's defaults");
    }

    /// `GetJunctionSize` on the Default class's 6 mil wire: 36 mils at "Default" (`DEFAULT_JUNCTION_DIAM`), one unit -- no dot -- at "None".
    #[test]
    fn the_junction_dot_is_the_wire_width_times_the_choice() {
        let wire = Nm::new(152_400);
        let mut f = SchFormatting::default();
        assert_eq!(f.junction_diameter(wire), Nm::new(914_400));
        f.junction_size_choice = 0;
        assert_eq!(f.junction_diameter(wire), Nm::new(100));
        f.junction_size_choice = 5;
        assert_eq!(f.junction_diameter(wire), Nm::new(1_828_800), "Largest: 12 times");
        f.junction_size_choice = 1;
        assert_eq!(f.junction_diameter(wire), Nm::new(259_100), "Smallest: 1.7 times, rounded to the 100 nm unit");
    }

    /// `GetHopOverScale`: None draws no hop (KiCad's Plot dialog then plots none, `m_plotHopOver = GetHopOverScale() > 0.0`), Largest is 12 line widths.
    #[test]
    fn the_hop_over_scale_is_the_choice_s_multiplier() {
        let scale = |c: u32| SchFormatting { hop_over_size_choice: c, ..SchFormatting::default() }.hop_over_scale();
        assert_eq!([0, 1, 2, 3, 4, 5].map(scale), [0.0, 1.7, 4.0, 6.0, 9.0, 12.0]);
        assert_eq!(scale(6), 0.0, "past the six: the default, None");
    }

    #[test]
    fn a_size_out_of_its_param_range_reads_back_as_the_default() {
        let f = SchFormatting { default_text_size_nm: Nm::new(2 * NM_PER_MIL), default_line_width_nm: Nm::new(10 * NM_PER_MIL), label_size_ratio: 3.0, ..SchFormatting::default() };
        let e = f.effective();
        assert_eq!(e.default_text_size_nm, Nm::new(1_270_000), "2 mils is under 5");
        assert_eq!(e.default_line_width_nm, Nm::new(254_000), "10 mils is kept");
        assert_eq!(e.label_size_ratio, 0.375);
        assert!(SchFormatting { junction_size_choice: 6, ..SchFormatting::default() }.validate().is_err());
        assert!(SchFormatting { opo_v_range: "kV".into(), ..SchFormatting::default() }.validate().is_err(), "KiCad's list says KV");
        assert!(SchFormatting { dash_ratio: f64::NAN, ..SchFormatting::default() }.validate().is_err());
        assert!(SchFormatting::default().validate().is_ok());
    }

    #[test]
    fn the_unit_notation_choice_is_the_panels() {
        for (i, (_, first, sep)) in UNIT_NOTATIONS.iter().enumerate() {
            assert_eq!(unit_notation_choice(*first, *sep), i);
        }
        assert_eq!(unit_notation_choice('A' as u32, '/' as u32), 0, "an odd separator with letters shows as A");
        assert_eq!(unit_notation_choice('a' as u32, 0), 4, "any first id but 'A' is a numbered notation, '.' by default");
    }

    #[test]
    fn a_colour_reads_as_kicad_writes_it() {
        assert_eq!(color_from_project(UNSPECIFIED_COLOR), None);
        assert_eq!(color_from_project("rgb(255, 0, 0)").as_deref(), Some("rgb(255, 0, 0)"));
        assert_eq!(color_from_project("rgba(0, 128, 0, 0.500)").as_deref(), Some("rgba(0, 128, 0, 0.502)"), "alpha in 255ths, as wxColour keeps it");
        assert_eq!(color_from_project("#00ff00").as_deref(), Some("rgb(0, 255, 0)"));
        assert_eq!(color_from_project("green"), None);
    }

    #[test]
    fn the_default_class_has_kicads_schematic_defaults() {
        let d = NetClassSchematic::default_class(None);
        assert_eq!((d.wire_width, d.bus_width, d.line_style, d.color), (Some(Nm::new(152_400)), Some(Nm::new(304_800)), Some(0), None));
        let own = NetClassSchematic { wire_width: Some(Nm::new(254_000)), ..Default::default() };
        assert_eq!(NetClassSchematic::default_class(Some(&own)).wire_width, Some(Nm::new(254_000)));
        assert!(NetClassSchematic { line_style: Some(5), ..Default::default() }.validate().is_err());
        assert!(NetClassSchematic { wire_width: Some(Nm::ZERO), ..Default::default() }.validate().is_ok(), "KiCad takes a 0 as typed");
        assert!(NetClassSchematic { wire_width: Some(Nm::new(-1)), ..Default::default() }.validate().is_ok(), "and a negative one (UI-LEFT-3 B3-4)");
    }

    #[test]
    fn template_rows_without_a_name_are_dropped_and_white_space_is_asked_about() {
        let rows = vec![TemplateFieldName { name: "MPN".into(), visible: false, url: false }, TemplateFieldName { name: String::new(), visible: true, url: false }, TemplateFieldName { name: " Link".into(), visible: false, url: true }];
        assert_eq!(kept_template_fields(&rows).len(), 2);
        let again = vec![TemplateFieldName { name: "MPN".into(), visible: false, url: false }, TemplateFieldName { name: "Value".into(), visible: true, url: false }, TemplateFieldName { name: "MPN".into(), visible: true, url: false }];
        assert_eq!(kept_template_fields(&again), vec![TemplateFieldName { name: "MPN".into(), visible: true, url: false }], "a mandatory name is dropped, a second MPN replaces the first");
        assert_eq!(field_name_whitespace_warning(" Link").as_deref(), Some("The field name ' Link' contains trailing and/or leading white space."));
        assert_eq!(field_name_whitespace_warning("MPN"), None);
    }
}
