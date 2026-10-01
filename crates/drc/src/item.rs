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
    InvalidOutline,
    DuplicateFootprint,
    MissingFootprint,
    ExtraFootprint,

    // ---- placement-quality checks: no KiCad equivalent, ported from
    // eda_gates::pcb (see the task report's gates-mapping table). Settings
    // keys are kept identical to the old `CheckResult::check` strings on
    // purpose, so eda_gates's compatibility shim can translate a
    // DrcViolation back into the exact check name existing callers filter
    // on.
    PlacementProximity,
    PlacementDecoupling,
    PlacementStubCrossings,
    PlacementBoardUse,
    PlacementNetCompactness,
    PlacementEdgeConnector,
    PlacementRefdesClear,
    /// Net-class track-width conformance: did the router actually use the
    /// width its net's class assigns, not just clear KiCad's absolute
    /// floor (see `ErrorType::TrackWidth`). No KiCad equivalent (KiCad has
    /// no concept of "the intent asked for this class"); kept under gates'
    /// original `routing_track_width` name.
    NetClassTrackWidth,

    /// `DRCE_ALLOWED_ITEMS` (`drc_test_provider_disallow.cpp`, task item
    /// 3): a track/via/pad/footprint/copper-pour landing inside a rule
    /// area (keepout) that disallows it. Appended here rather than sorted
    /// in among the other KiCad-ported codes above, purely to keep this
    /// addition a one-line diff at the end of an actively-worked-on file.
    ItemsNotAllowed,
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
            ErrorType::InvalidOutline => "invalid_outline",
            ErrorType::DuplicateFootprint => "duplicate_footprints",
            ErrorType::MissingFootprint => "missing_footprint",
            ErrorType::ExtraFootprint => "extra_footprint",
            ErrorType::PlacementProximity => "placement_proximity",
            ErrorType::PlacementDecoupling => "placement_decoupling",
            ErrorType::PlacementStubCrossings => "placement_stub_crossings",
            ErrorType::PlacementBoardUse => "placement_board_use",
            ErrorType::PlacementNetCompactness => "placement_net_compactness",
            ErrorType::PlacementEdgeConnector => "placement_edge_connector",
            ErrorType::PlacementRefdesClear => "placement_refdes_clear",
            ErrorType::NetClassTrackWidth => "routing_track_width",
            ErrorType::ItemsNotAllowed => "items_not_allowed",
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
            ErrorType::InvalidOutline => "Invalid board outline",
            ErrorType::DuplicateFootprint => "Duplicate footprints",
            ErrorType::MissingFootprint => "Missing footprint",
            ErrorType::ExtraFootprint => "Extra footprint",
            ErrorType::PlacementProximity => "Proximity rule violation",
            ErrorType::PlacementDecoupling => "Decoupling capacitor too far from its IC",
            ErrorType::PlacementStubCrossings => "Crossing two-pin net stubs",
            ErrorType::PlacementBoardUse => "Poor board utilization",
            ErrorType::PlacementNetCompactness => "Net spans too much board for its members",
            ErrorType::PlacementEdgeConnector => "Edge connector not on the board edge",
            ErrorType::PlacementRefdesClear => "Reference label overlaps a neighbouring courtyard",
            ErrorType::NetClassTrackWidth => "Track width does not match its net class",
            ErrorType::ItemsNotAllowed => "Items not allowed",
        }
    }

    /// Default severity, ported from `BOARD_DESIGN_SETTINGS`'s constructor:
    /// every code defaults to `error`; these are the explicit overrides.
    pub fn default_severity(self) -> Severity {
        match self {
            ErrorType::HoleToHole
            | ErrorType::HolesCoLocated
            | ErrorType::SilkEdgeClearance
            | ErrorType::SilkOverlap
            | ErrorType::SilkOverCopper
            | ErrorType::TextHeight
            | ErrorType::TextThickness
            | ErrorType::TrackDangling
            | ErrorType::ViaDangling
            | ErrorType::DuplicateFootprint
            | ErrorType::MissingFootprint
            | ErrorType::ExtraFootprint => Severity::Warning,
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

/// Optional agent-facing repair metadata: KiCad's own DRC has nothing like
/// this (a human fixes a KiCad violation by hand), but this workspace's
/// repair loop and AI placer read it today through `CheckResult::detail`/
/// `hint` for the checks ported from `eda_gates::pcb`'s placement-quality
/// gates. Carried as a first-class, uniformly-shaped field here instead of
/// each provider inventing its own JSON, the way `eda_gates` did.
#[derive(Debug, Clone, Serialize)]
pub struct FixHint {
    /// Reference designator of the part a fix would move.
    pub mover: String,
    /// What to move it toward: another part's reference, "board center",
    /// "nearest edge", or similar -- whatever the specific check computed.
    pub toward: String,
    /// How much closer (positive) or farther (negative), µm, `mover` needs
    /// to get to `toward` to clear this violation.
    pub distance_to_close_um: Um,
    /// Human-readable next step, the same voice `eda_gates`'s old
    /// `"suggest"` detail field used.
    pub suggested_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrcViolation {
    #[serde(rename = "type")]
    pub error_type: &'static str,
    pub description: String,
    pub severity: Severity,
    pub items: Vec<DrcRefItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixHint>,
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
        DrcViolation { error_type: error_type.key(), description, severity: error_type.default_severity(), items, fix: None }
    }

    /// Attach a [`FixHint`] (placement-quality providers only -- nothing in
    /// the ported KiCad checks needs one, since there's no automated fix
    /// for "your annular ring is too thin").
    pub fn with_fix(mut self, fix: FixHint) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Format a µm value the way KiCad's `MessageTextFromValue` does for a
/// millimetre-unit board: 4 decimal places, trailing zeros kept, e.g.
/// `200 -> "0.2000 mm"`.
pub fn format_um(v: Um) -> String {
    format!("{:.4} mm", v as f64 / 1000.0)
}
