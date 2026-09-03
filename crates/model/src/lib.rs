//! Constraint model — the SOURCE OF TRUTH for a design.
//! Coordinates never appear here; they are derived by the engine.
//! JSON internally, YAML at LLM-facing edges.

pub mod footprint;
pub mod ir;

pub use footprint::{Footprint, Pad, PadKind, PadShape};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintModel {
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub nets: Vec<Net>,
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    #[serde(default)]
    pub placement_rules: Vec<PlacementRule>,
    #[serde(default)]
    pub stackup: Option<Stackup>,
    #[serde(default)]
    pub impedance_targets: Vec<ImpedanceTarget>,
    /// Explicit footprint definitions; anything not listed here falls to
    /// the built-in package library (`footprint::builtin`).
    #[serde(default)]
    pub footprints: Vec<Footprint>,
    /// Board rules consumed by the placer, router and routing gates.
    #[serde(default)]
    pub board: BoardRules,
}

/// Physical design rules. Integer µm throughout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardRules {
    /// Routing grid pitch.
    #[serde(default = "d_grid")]
    pub grid: ir::Um,
    #[serde(default = "d_track")]
    pub track_width: ir::Um,
    #[serde(default = "d_clearance")]
    pub clearance: ir::Um,
    #[serde(default = "d_via_drill")]
    pub via_drill: ir::Um,
    #[serde(default = "d_via_dia")]
    pub via_diameter: ir::Um,
    /// Copper layer names, outer first: `["F.Cu", "B.Cu"]`.
    #[serde(default = "d_layers")]
    pub layers: Vec<String>,
    /// Board outline override, µm. When absent the placer sizes a rectangle.
    #[serde(default)]
    pub outline: Option<Vec<ir::Point>>,
}
fn d_grid() -> ir::Um { 254 }
fn d_track() -> ir::Um { 200 }
fn d_clearance() -> ir::Um { 200 }
fn d_via_drill() -> ir::Um { 300 }
fn d_via_dia() -> ir::Um { 600 }
fn d_layers() -> Vec<String> { vec!["F.Cu".into(), "B.Cu".into()] }
impl Default for BoardRules {
    fn default() -> Self {
        BoardRules { grid: d_grid(), track_width: d_track(), clearance: d_clearance(), via_drill: d_via_drill(), via_diameter: d_via_dia(), layers: d_layers(), outline: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// Reference designator, unique: "U1", "C3".
    pub reference: String,
    #[serde(default)]
    pub mpn: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    /// KiCad footprint id, e.g. "Capacitor_SMD:C_0402_1005Metric".
    #[serde(default)]
    pub footprint: Option<String>,
    #[serde(default)]
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub number: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: PinKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinKind {
    Power,
    Ground,
    #[default]
    Signal,
    Passive,
    Nc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub name: String,
    /// Pin refs as "U1.3".
    #[serde(default)]
    pub pins: Vec<String>,
}

/// The atomic unit for both schematic and PCB.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub anchor: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlacementRule {
    Proximity { a: String, b: String, max_mm: f64 },
    Keepout { zone: String, refs: Vec<String> },
    ThermalGroup { refs: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stackup {
    pub layers: Vec<StackupLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackupLayer {
    pub name: String,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub thickness_mm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpedanceTarget {
    /// Glob over net names, e.g. "USB_D*".
    pub net_pattern: String,
    pub ohms: f64,
    #[serde(default)]
    pub tolerance_pct: Option<f64>,
}

// ---------------------------------------------------------------- check shape
/// Every checker — schema, ERC, metrics, critic — returns this one shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub status: CheckStatus,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

impl CheckResult {
    pub fn fail(check: &str, location: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { check: check.into(), status: CheckStatus::Fail,
               location: Some(location.into()), hint: Some(hint.into()) }
    }
    pub fn pass(check: &str) -> Self {
        Self { check: check.into(), status: CheckStatus::Pass, location: None, hint: None }
    }
}

impl ConstraintModel {
    pub fn part(&self, reference: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.reference == reference)
    }
    /// Resolve the physical footprint for a part: explicit model
    /// definitions first (by `Part::footprint` name, then by `package`),
    /// then the built-in library. `None` means the part is not physically
    /// realisable and every physical stage must fail on it.
    pub fn footprint_of(&self, part: &Part) -> Option<Footprint> {
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(fp) = self.footprints.iter().find(|f| f.name == key) {
                return Some(fp.clone());
            }
            let norm = footprint::normalize_name(key);
            if let Some(fp) = self.footprints.iter().find(|f| footprint::normalize_name(&f.name) == norm) {
                return Some(fp.clone());
            }
        }
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(fp) = footprint::builtin(key) {
                return Some(fp);
            }
        }
        None
    }
    /// Simple glob match ('*' wildcard) over net names.
    pub fn nets_matching(&self, pattern: &str) -> Vec<&Net> {
        self.nets.iter().filter(|n| glob_match(pattern, &n.name)).collect()
    }
}

pub fn glob_match(pattern: &str, s: &str) -> bool {
    // '*' matches any run (incl. empty); everything else literal.
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], s) || (!s.is_empty() && rec(p, &s[1..])),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    rec(pattern.as_bytes(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob() {
        assert!(glob_match("SPI_*", "SPI_MOSI"));
        assert!(glob_match("VBUS", "VBUS"));
        assert!(!glob_match("SPI_*", "I2C_SCL"));
        assert!(glob_match("*", "anything"));
    }
    #[test]
    fn roundtrip() {
        let m = ConstraintModel::default();
        let j = serde_json::to_string(&m).unwrap();
        let _: ConstraintModel = serde_json::from_str(&j).unwrap();
    }
}
