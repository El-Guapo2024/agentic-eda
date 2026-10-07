//! What a positioned lint check reports (placement quality, net-class
//! width). Shaped like one entry of `kicad-cli pcb drc --format json` --
//! `type`, `description`, `severity`, `items[]` with a position and our own
//! item id -- so one list in the studio can show both, plus an optional
//! [`FixHint`] KiCad's report has no concept of.
//!
//! The check names are the ones the placement/routing gates have always
//! used (`placement_proximity`, ..., `routing_track_width`); callers filter
//! on them, so they do not change.

use eda_model::ir::Um;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One lint check that reports positioned findings. None of these has a
/// KiCad equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    PlacementProximity,
    PlacementDecoupling,
    PlacementStubCrossings,
    PlacementBoardUse,
    PlacementNetCompactness,
    PlacementEdgeConnector,
    PlacementRefdesClear,
    /// Net-class track-width conformance: did the router use the width the
    /// intent's net class assigns, not just clear a manufacturability
    /// floor. KiCad has no notion of "the intent assigned this net a
    /// class".
    NetClassTrackWidth,
}

impl Check {
    /// The check name -- the JSON `"type"` and the gates' check name.
    pub fn key(self) -> &'static str {
        match self {
            Check::PlacementProximity => "placement_proximity",
            Check::PlacementDecoupling => "placement_decoupling",
            Check::PlacementStubCrossings => "placement_stub_crossings",
            Check::PlacementBoardUse => "placement_board_use",
            Check::PlacementNetCompactness => "placement_net_compactness",
            Check::PlacementEdgeConnector => "placement_edge_connector",
            Check::PlacementRefdesClear => "placement_refdes_clear",
            Check::NetClassTrackWidth => "routing_track_width",
        }
    }

    /// The first clause of a finding's `description`.
    pub fn title(self) -> &'static str {
        match self {
            Check::PlacementProximity => "Proximity rule violation",
            Check::PlacementDecoupling => "Decoupling capacitor too far from its IC",
            Check::PlacementStubCrossings => "Crossing two-pin net stubs",
            Check::PlacementBoardUse => "Poor board utilization",
            Check::PlacementNetCompactness => "Net spans too much board for its members",
            Check::PlacementEdgeConnector => "Edge connector not on the board edge",
            Check::PlacementRefdesClear => "Reference label overlaps a neighbouring courtyard",
            Check::NetClassTrackWidth => "Track width does not match its net class",
        }
    }

    pub fn default_severity(self) -> Severity {
        Severity::Error
    }
}

/// One item a finding names.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub description: String,
    /// Board-space µm, this workspace's convention throughout.
    pub pos: (Um, Um),
    /// Our own id for the item (`REF`, `REF.PIN`, a net name, `track#seg`),
    /// so the UI can highlight it without re-matching on position.
    pub id: String,
}

/// Repair metadata for an agent: which part to move, toward what, how far.
#[derive(Debug, Clone, Serialize)]
pub struct FixHint {
    /// Reference designator of the part a fix would move.
    pub mover: String,
    /// What to move it toward: another part's reference, "nearest board
    /// edge", or similar.
    pub toward: String,
    /// How much closer, µm, `mover` needs to get to `toward` to clear this.
    pub distance_to_close_um: Um,
    /// Human-readable next step.
    pub suggested_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    #[serde(rename = "type")]
    pub check: &'static str,
    pub description: String,
    pub severity: Severity,
    pub items: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixHint>,
}

impl Finding {
    /// `description` is the check's title, then ` (detail)` when `detail`
    /// is not empty.
    pub fn new(check: Check, detail: impl Into<String>, items: Vec<Item>) -> Self {
        let detail = detail.into();
        let description = if detail.is_empty() { check.title().to_string() } else { format!("{} ({detail})", check.title()) };
        Finding { check: check.key(), description, severity: check.default_severity(), items, fix: None }
    }

    pub fn with_fix(mut self, fix: FixHint) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Finding counts by check name.
pub fn counts(findings: &[Finding]) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for f in findings {
        *m.entry(f.check).or_insert(0) += 1;
    }
    m
}
