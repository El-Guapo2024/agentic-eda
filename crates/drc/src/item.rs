//! Ported from `pcbnew/drc/drc_item.h`/`.cpp`: the `DRCE_*` error codes and
//! their exact settings-key strings and titles (`DRC_ITEM::Create`'s table),
//! so `type`/`description` in our report match `kicad-cli pcb drc --format
//! json`'s output byte-for-byte. Default severities are ported from
//! `BOARD_DESIGN_SETTINGS`'s constructor (everything defaults to `error`;
//! the table below lists only the overrides, exactly as KiCad's does).

use eda_model::ir::Um;
use serde::Serialize;

/// One error kind this port produces. The `&'static str` fields are exactly
/// KiCad's `wxT("...")` settings keys / `_HKI("...")` titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    Clearance,
    HoleClearance,
    TracksCrossing,
    ShortingItems,
    ZonesIntersect,
    TrackWidth,
    AnnularWidth,
    DrillOutOfRange,
    ViaDiameter,
    HoleToHole,
    HolesCoLocated,
    CopperEdgeClearance,
    SilkEdgeClearance,
    CourtyardsOverlap,
    PthInsideCourtyard,
    NpthInsideCourtyard,
    SilkOverlap,
    SilkOverCopper,
    SolderMaskBridge,
    TextHeight,
    TextThickness,
    TrackDangling,
    ViaDangling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl ErrorType {
    /// KiCad's settings key -- the JSON `"type"` field.
    pub fn key(self) -> &'static str {
        match self {
            ErrorType::Clearance => "clearance",
            ErrorType::HoleClearance => "hole_clearance",
            ErrorType::TracksCrossing => "tracks_crossing",
            ErrorType::ShortingItems => "shorting_items",
            ErrorType::ZonesIntersect => "zones_intersect",
            ErrorType::TrackWidth => "track_width",
            ErrorType::AnnularWidth => "annular_width",
            ErrorType::DrillOutOfRange => "drill_out_of_range",
            ErrorType::ViaDiameter => "via_diameter",
            ErrorType::HoleToHole => "hole_to_hole",
            ErrorType::HolesCoLocated => "holes_co_located",
            ErrorType::CopperEdgeClearance => "copper_edge_clearance",
            ErrorType::SilkEdgeClearance => "silk_edge_clearance",
            ErrorType::CourtyardsOverlap => "courtyards_overlap",
            ErrorType::PthInsideCourtyard => "pth_inside_courtyard",
            ErrorType::NpthInsideCourtyard => "npth_inside_courtyard",
            ErrorType::SilkOverlap => "silk_overlap",
            ErrorType::SilkOverCopper => "silk_over_copper",
            ErrorType::SolderMaskBridge => "solder_mask_bridge",
            ErrorType::TextHeight => "text_height",
            ErrorType::TextThickness => "text_thickness",
            ErrorType::TrackDangling => "track_dangling",
            ErrorType::ViaDangling => "via_dangling",
        }
    }

    /// KiCad's `_HKI(...)` title -- the first clause of the JSON `"description"`.
    pub fn title(self) -> &'static str {
        match self {
            ErrorType::Clearance => "Clearance violation",
            ErrorType::HoleClearance => "Hole clearance violation",
            ErrorType::TracksCrossing => "Tracks crossing",
            ErrorType::ShortingItems => "Items shorting two nets",
            ErrorType::ZonesIntersect => "Copper zones intersect",
            ErrorType::TrackWidth => "Track width",
            ErrorType::AnnularWidth => "Annular width",
            ErrorType::DrillOutOfRange => "Hole size out of range",
            ErrorType::ViaDiameter => "Via diameter",
            ErrorType::HoleToHole => "Drilled hole too close to other hole",
            ErrorType::HolesCoLocated => "Drilled holes co-located",
            ErrorType::CopperEdgeClearance => "Board edge clearance violation",
            ErrorType::SilkEdgeClearance => "Silkscreen clipped by board edge",
            ErrorType::CourtyardsOverlap => "Courtyards overlap",
            ErrorType::PthInsideCourtyard => "PTH inside courtyard",
            ErrorType::NpthInsideCourtyard => "NPTH inside courtyard",
            ErrorType::SilkOverlap => "Silkscreen clearance",
            ErrorType::SilkOverCopper => "Silkscreen clipped by solder mask",
            ErrorType::SolderMaskBridge => "Solder mask aperture bridges items with different nets",
            ErrorType::TextHeight => "Text height out of range",
            ErrorType::TextThickness => "Text thickness out of range",
            ErrorType::TrackDangling => "Track has unconnected end",
            ErrorType::ViaDangling => "Via is not connected or connected on only one layer",
        }
    }

    /// Default severity, ported from `BOARD_DESIGN_SETTINGS`'s constructor:
    /// every code defaults to `error`; these are the explicit overrides.
    pub fn default_severity(self) -> Severity {
        match self {
            ErrorType::HoleToHole | ErrorType::HolesCoLocated | ErrorType::SilkEdgeClearance | ErrorType::SilkOverlap | ErrorType::SilkOverCopper | ErrorType::TextHeight | ErrorType::TextThickness | ErrorType::TrackDangling | ErrorType::ViaDangling => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

/// A reference to one item involved in a violation -- kicad-cli's
/// `items[]` entries. `pos` is each item's own canonical anchor, not a
/// computed collision point: empirically, `kicad-cli`'s JSON reports a
/// pad/via's centre and a track's *start* point here (`BOARD_ITEM::
/// GetPosition()`), regardless of where along the two shapes the actual
/// violation geometry sits -- verified against `kicad-cli pcb drc` output
/// directly rather than assumed from the C++ (see the task report).
#[derive(Debug, Clone, Serialize)]
pub struct DrcRefItem {
    pub description: String,
    /// Board-space µm, this workspace's convention throughout (kicad-cli's
    /// own JSON uses mm; see the report for why `/api/drc` uses µm instead).
    pub pos: (Um, Um),
    /// Stable id of the referenced item (track/via/zone id, or a
    /// synthesized `<ref>.<pad>` / `<ref>` for footprints/pads), so the UI
    /// can highlight it without re-matching on position.
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrcViolation {
    #[serde(rename = "type")]
    pub error_type: &'static str,
    pub description: String,
    pub severity: Severity,
    pub items: Vec<DrcRefItem>,
}

impl DrcViolation {
    /// `description` is KiCad's own convention: the title, then, if
    /// `detail` is non-empty, `" (detail)"` appended -- e.g. `"Clearance
    /// violation (clearance 0.2000 mm; actual 0.1500 mm)"`. `detail` here is
    /// already-formatted (this module does not reproduce KiCad's
    /// `EDA_UNITS`-aware `MessageTextFromValue`; see `format_um`).
    pub fn new(error_type: ErrorType, detail: impl Into<String>, items: Vec<DrcRefItem>) -> Self {
        let detail = detail.into();
        let description = if detail.is_empty() { error_type.title().to_string() } else { format!("{} ({detail})", error_type.title()) };
        DrcViolation { error_type: error_type.key(), description, severity: error_type.default_severity(), items }
    }
}

/// Format a µm value the way KiCad's `MessageTextFromValue` does for a
/// millimetre-unit board: 4 decimal places, trailing zeros kept, e.g.
/// `200 -> "0.2000 mm"`.
pub fn format_um(v: Um) -> String {
    format!("{:.4} mm", v as f64 / 1000.0)
}
