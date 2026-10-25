//! KiCad 10's drill charts, drill maps and the board's drill symbol profile (KiCad 25ec2335, the commit of the box's kicad-cli: the 8303b2ad source the rest of the port reads
//! predates them).
//!
//! - [`DrillSymbolProfile`] is `(setup (drill_symbol_profile ..))` (`DRILL_SYMBOL_PROFILE`, pcbnew/drill/drill_symbol_profile.h): which properties split the holes into groups,
//!   how a new group is marked, the size and width of the marks, and the mark each group has been given. A chart and a map read the same one, so a symbol means the same hole in
//!   both. `None` on the board is KiCad's default profile, which the file does not write.
//! - [`DrillChart`] is `(drill_chart ..)` (`PCB_DRILL_CHART`, a `PCB_GENERATED_TABLE`, a `PCB_TABLE`): the table itself is one of the board's tables
//!   (`DrawingsSection::tables`, [`crate::table::Table`]), drawn, moved and edited as any table is; this overlay, keyed by the table's id, holds what makes it a chart -- its
//!   columns, units and precision, the filter of the holes it reports, the totals row, what each row reports (`row_keys`) and the shape drawn in the symbol column of each.
//! - [`DrillMap`] is `(drill_map ..)` (`PCB_DRILL_MAP`): the switch that draws the drill marks at the holes on a documentation layer, slid by an offset.
//!
//! Additive: absent in an older `design.json` reads as "none".

use crate::ir::{Point, Nm};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ------------------------------------------------------------------------------------------------------------------------------------- the drill symbol profile

/// `DRILL_GROUP_KEY`, in its declaration order (the order a `std::set` of them is written in), with its file token (`DrillGroupKeyToken`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillGroupKey {
    Size,
    Slot,
    Plating,
    Span,
    Operation,
    HoleFunction,
    Protection,
    PostMachining,
}

impl DrillGroupKey {
    pub const ALL: [DrillGroupKey; 8] = [Self::Size, Self::Slot, Self::Plating, Self::Span, Self::Operation, Self::HoleFunction, Self::Protection, Self::PostMachining];

    pub fn token(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Slot => "slot",
            Self::Plating => "plating",
            Self::Span => "span",
            Self::Operation => "operation",
            Self::HoleFunction => "hole_function",
            Self::Protection => "protection",
            Self::PostMachining => "post_machining",
        }
    }

    pub fn from_token(t: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.token() == t)
    }
}

/// `DRILL_MARK_POLICY`: how a group with no mark gets one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillMarkPolicy {
    Shapes,
    Letters,
    #[default]
    ShapesThenLetters,
    SizeText,
}

impl DrillMarkPolicy {
    pub fn token(self) -> &'static str {
        match self {
            Self::Shapes => "shapes",
            Self::Letters => "letters",
            Self::ShapesThenLetters => "shapes_then_letters",
            Self::SizeText => "size_text",
        }
    }

    pub fn from_token(t: &str) -> Option<Self> {
        [Self::Shapes, Self::Letters, Self::ShapesThenLetters, Self::SizeText].into_iter().find(|p| p.token() == t)
    }
}

/// `DRILL_MARK_MODE`: what one group's mark is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillMarkMode {
    #[default]
    Shape,
    Letter,
    SizeText,
}

impl DrillMarkMode {
    pub fn token(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::Letter => "letter",
            Self::SizeText => "size_text",
        }
    }

    pub fn from_token(t: &str) -> Option<Self> {
        [Self::Shape, Self::Letter, Self::SizeText].into_iter().find(|m| m.token() == t)
    }
}

/// `DRILL_SYMBOL_ASSIGNMENT`: the mark of one group -- a curated shape (by its index, [`crate::drill_chart::CURATED_SHAPES`]), a letter, or the hole's size as text -- and the
/// description a chart's Description column prints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillSymbolAssignment {
    #[serde(default)]
    pub mode: DrillMarkMode,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub shape_index: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub letter: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

/// `DRILL_SYMBOL_PROFILE`. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillSymbolProfile {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `m_groupBy`, in [`DrillGroupKey`] order.
    pub group_by: Vec<DrillGroupKey>,
    pub mark_policy: DrillMarkPolicy,
    /// `m_symbolSize` and `m_symbolWidth`, nm.
    #[serde(alias = "size_um", deserialize_with = "crate::units::de")]
    pub size_nm: Nm,
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
    pub freeze_assignments: bool,
    /// `m_assignments`: group key (`GroupKeyString`) -> mark, in the key's byte order (a `std::map<std::string, ..>`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assignments: BTreeMap<String, DrillSymbolAssignment>,
}

impl Default for DrillSymbolProfile {
    /// `DRILL_SYMBOL_PROFILE::DRILL_SYMBOL_PROFILE()`: grouped by size, slot, plating, span, operation and post-machining; shapes, then letters; 1.5 mm marks drawn 0.15 mm wide;
    /// assignments frozen.
    fn default() -> Self {
        DrillSymbolProfile {
            name: String::new(),
            group_by: vec![DrillGroupKey::Size, DrillGroupKey::Slot, DrillGroupKey::Plating, DrillGroupKey::Span, DrillGroupKey::Operation, DrillGroupKey::PostMachining],
            mark_policy: DrillMarkPolicy::ShapesThenLetters,
            size_nm: Nm::new(1_500_000),
            width_nm: Nm::new(150_000),
            freeze_assignments: true,
            assignments: BTreeMap::new(),
        }
    }
}

impl DrillSymbolProfile {
    pub fn is_grouped_by(&self, k: DrillGroupKey) -> bool {
        self.group_by.contains(&k)
    }

    /// `SetGroupedBy`: on or off, kept in key order.
    pub fn set_grouped_by(&mut self, k: DrillGroupKey, on: bool) {
        self.group_by.retain(|x| *x != k);
        if on {
            self.group_by.push(k);
            self.group_by.sort();
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the drill chart

/// `GENERATED_TABLE_UNITS`, with its token (`GeneratedTableUnitsToken`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableUnits {
    #[default]
    Mm,
    Inch,
    Mils,
}

impl TableUnits {
    pub fn token(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Inch => "inch",
            Self::Mils => "mils",
        }
    }

    pub fn from_token(t: &str) -> Option<Self> {
        [Self::Mm, Self::Inch, Self::Mils].into_iter().find(|u| u.token() == t)
    }
}

/// `GENERATED_TABLE_ALIGN`, with its token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TableAlign {
    pub fn token(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub fn from_token(t: &str) -> Option<Self> {
        [Self::Left, Self::Center, Self::Right].into_iter().find(|a| a.token() == t)
    }
}

/// `DRILL_CHART_COLUMN_ID`, in the order `DrillChartSchema()` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillChartColumnId {
    Symbol,
    DrillDiameter,
    SlotSize,
    Plating,
    OperationCount,
    SiteCount,
    LayerSpan,
    Operation,
    Protection,
    BackdrillStub,
    AspectRatio,
    Description,
}

impl DrillChartColumnId {
    pub const ALL: [DrillChartColumnId; 12] = [
        Self::Symbol,
        Self::DrillDiameter,
        Self::SlotSize,
        Self::Plating,
        Self::OperationCount,
        Self::SiteCount,
        Self::LayerSpan,
        Self::Operation,
        Self::Protection,
        Self::BackdrillStub,
        Self::AspectRatio,
        Self::Description,
    ];

    /// `DrillChartSchema()`'s entry: the file token, the heading, the alignment, whether a new chart shows it, the name the column chooser gives it.
    pub fn def(self) -> (&'static str, &'static str, TableAlign, bool, &'static str) {
        use TableAlign::*;
        match self {
            Self::Symbol => ("symbol", "SYM", Center, true, "Symbol"),
            Self::DrillDiameter => ("drill_diameter", "DRILL DIA", Right, true, "Drill diameter"),
            Self::SlotSize => ("slot_size", "SLOT W x L", Right, true, "Slot size"),
            Self::Plating => ("plating", "PLATED", Center, true, "Plating"),
            Self::OperationCount => ("operation_count", "OPS", Right, true, "Operation count"),
            Self::SiteCount => ("site_count", "SITE COUNT", Right, false, "Site count"),
            Self::LayerSpan => ("layer_span", "FROM / TO", Center, true, "Layer span"),
            Self::Operation => ("operation", "OPERATION", Left, false, "Operation"),
            Self::Protection => ("protection", "PROTECTION", Left, false, "Protection"),
            Self::BackdrillStub => ("backdrill_stub", "BACKDRILL STUB", Right, false, "Backdrill stub"),
            Self::AspectRatio => ("aspect_ratio", "ASPECT RATIO", Right, false, "Aspect ratio"),
            Self::Description => ("description", "DESCRIPTION", Left, false, "Description"),
        }
    }

    pub fn token(self) -> &'static str {
        self.def().0
    }

    pub fn from_token(t: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.token() == t)
    }
}

/// `GENERATED_TABLE_COLUMN`: a column of a chart, its heading, alignment and the width it is at least (0: as wide as its text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillChartColumn {
    pub id: DrillChartColumnId,
    pub heading: String,
    pub align: TableAlign,
    #[serde(default)]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
}

impl DrillChartColumn {
    /// `GENERATED_TABLE_SCHEMA::DefaultColumn`.
    pub fn default_of(id: DrillChartColumnId) -> Self {
        let (_, heading, align, _, _) = id.def();
        DrillChartColumn { id, heading: heading.to_string(), align, width_nm: Nm::ZERO }
    }

    /// `GENERATED_TABLE_SCHEMA::Defaults`: the columns a new chart has.
    pub fn defaults() -> Vec<Self> {
        DrillChartColumnId::ALL.into_iter().filter(|c| c.def().3).map(Self::default_of).collect()
    }
}

/// `DRILL_CHART_FILTER`: which holes a chart reports on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillChartFilter {
    pub plated: bool,
    pub non_plated: bool,
    pub vias: bool,
    pub slots: bool,
    pub backdrills: bool,
    pub castellated: bool,
}

impl Default for DrillChartFilter {
    fn default() -> Self {
        DrillChartFilter { plated: true, non_plated: true, vias: true, slots: true, backdrills: true, castellated: true }
    }
}

impl DrillChartFilter {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What makes a board table a drill chart. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillChart {
    /// The id of the table (`DrawingsSection::tables`) the chart is.
    pub table_id: String,
    #[serde(default)]
    pub units: TableUnits,
    /// `m_precision`, 0 to 6 (the schema's default is 3).
    #[serde(default = "d_precision")]
    pub precision: i32,
    pub columns: Vec<DrillChartColumn>,
    /// `m_rowKeys`: the table row -> the key of the group it reports.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_keys: BTreeMap<u32, String>,
    #[serde(default, skip_serializing_if = "DrillChartFilter::is_default")]
    pub filter: DrillChartFilter,
    #[serde(default = "d_true")]
    pub show_totals: bool,
    /// `m_symbolColumn`: the table column the marks are drawn in, -1 for none.
    #[serde(default = "d_minus_one")]
    pub symbol_column: i32,
    /// `m_rowShapes`: the table row -> the curated shape drawn in its symbol cell.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_shapes: BTreeMap<u32, i32>,
    /// `m_builtGeneration`: the board's drill generation the chart was last built against (`eda_ops::drill_chart::drill_generation`, a digest of the pads, vias, footprints,
    /// copper layers and drill maps, which is what bumps `BOARD::m_drillModelGeneration`). Not in the KiCad file: a chart that was read has none, so it is stale and the first
    /// command rebuilds it, as KiCad's `m_builtGeneration( 0 )` against a board's generation of at least 1 makes the first `OnModify` do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_generation: Option<String>,
}

fn d_precision() -> i32 {
    3
}
fn d_true() -> bool {
    true
}
fn d_minus_one() -> i32 {
    -1
}

impl DrillChart {
    /// `PCB_DRILL_CHART( board )` with `DRILL_CHART_TEMPLATE::MakeDefault()` applied: the default columns, millimetres to three places, totals shown.
    pub fn new(table_id: &str) -> Self {
        DrillChart {
            table_id: table_id.to_string(),
            units: TableUnits::Mm,
            precision: 3,
            columns: DrillChartColumn::defaults(),
            row_keys: BTreeMap::new(),
            filter: DrillChartFilter::default(),
            show_totals: true,
            symbol_column: -1,
            row_shapes: BTreeMap::new(),
            built_generation: None,
        }
    }
}

// -------------------------------------------------------------------------------------------------------------------------------------------------- the drill map

/// `DRILL_SPAN`: the layers a drill runs between (as the file names them, start first), and whether it is a backdrill's or a non-plated file's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillSpan {
    pub start: String,
    pub end: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backdrill: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub npth: bool,
}

/// `PCB_DRILL_MAP`. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillMap {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub layer: String,
    /// `m_offset`: how far every mark is slid from its hole, nm.
    #[serde(default)]
    pub offset: Point,
    /// `m_symbolSize`, nm.
    #[serde(alias = "size_um", deserialize_with = "crate::units::de")]
    pub size_nm: Nm,
    /// `m_span` when the map shows one span only (`m_allSpans` false); `None` shows them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<DrillSpan>,
    #[serde(default = "d_true")]
    pub outline_slots: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guide_cross: bool,
    /// `(custom_property ..)` entries as the file wrote them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_properties: Vec<String>,
}

impl DrillMap {
    /// `PCB_DRILL_MAP( board )`: on `layer`, no offset, the profile's mark size, every span, slots outlined, no guide cross.
    pub fn new(layer: &str, size_nm: Nm) -> Self {
        DrillMap { id: String::new(), layer: layer.to_string(), offset: Point { x: Nm::ZERO, y: Nm::ZERO }, size_nm, span: None, outline_slots: true, guide_cross: false, custom_properties: vec![] }
    }

    pub fn id_seed(&self) -> String {
        format!("drill_map:{}:{}:{}", self.layer, self.offset.x, self.offset.y)
    }
}

/// `DocumentationLayers()`: the layers that are not copper, mask, paste, adhesive or silk, nor Edge.Cuts, Margin, a courtyard or Rescue -- what a drill chart or map may be on.
pub fn is_documentation_layer(layer: &str) -> bool {
    matches!(layer, "Dwgs.User" | "Cmts.User" | "Eco1.User" | "Eco2.User" | "F.Fab" | "B.Fab") || (layer.starts_with("User.") && layer[5..].parse::<u32>().is_ok())
}

/// `DRILL_MARKERS::CuratedShape`'s table: the patterns the curated mark `i` draws (`marker_patterns[ curated[i] ]`).
pub const CURATED_SHAPES: [u32; 12] = [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 16];
