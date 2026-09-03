//! The geometry IR — `design.json`.
//!
//! One nested document that fills in stage by stage (schematic → placement →
//! routing). Frozen sections never change; the running loop regenerates only
//! its own section across N candidates. All coordinates are integer
//! micrometers: floats in a hashed file are a canonicalization landmine.
//!
//! Canonical form contract (what makes byte-hash == content-hash):
//! - serialization uses sorted object keys (serde_json with BTreeMap-backed
//!   structs / preserve_order off) and no floats anywhere
//! - all IDs are derived from intent (reference designators, net names),
//!   never random — the same intent always yields the same IDs
//! - vectors are sorted by `id` before serialization

use serde::{Deserialize, Serialize};

/// Integer micrometers. 1 mm = 1_000 um.
pub type Um = i64;

/// Rotation in millidegrees (0..360_000) so 45° etc. stay exact integers.
pub type Millideg = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Point {
    pub x: Um,
    pub y: Um,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    /// Schema version of this document; migrations live in the kernel.
    pub schema: u32,
    pub provenance: Provenance,
    /// Filled by loop 1 (E1). Artifact 1 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic: Option<SchematicSection>,
    /// Filled by loop 2 (E2). Artifact 2 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementSection>,
    /// Filled by loop 3 (E3). Artifact 3 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<RoutingSection>,
}

/// Every design.json can be traced to exactly what produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub engine_version: String,
    /// blake3 of the canonical intent slice this document was derived from.
    pub intent_hash: String,
    pub seed: u64,
    /// Per-stage hashes of the consumed intent slice, so adding a routing
    /// constraint later does not unfreeze the schematic (staged intent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_hashes: Vec<StageHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageHash {
    pub stage: Stage,
    pub intent_slice_hash: String,
    /// Set when the user acked this stage's render; freezes the section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen_at: Option<String>, // RFC 3339
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Schematic,
    Placement,
    Routing,
}

// ---------- stage 1: schematic ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicSection {
    /// Sorted by `id` (reference designator).
    pub symbols: Vec<SymbolInstance>,
    /// Sorted by (net, then first point).
    pub wires: Vec<Wire>,
    /// Net label placements, sorted by (net, at).
    #[serde(default)]
    pub labels: Vec<NetLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolInstance {
    /// Reference designator from intent ("U1") — the stable ID.
    pub id: String,
    pub at: Point,
    pub rot: Millideg,
    #[serde(default)]
    pub mirrored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub net: String,
    /// Pins this wire lands on, as "REF.PIN" ("U1.3") — borrowed from
    /// Circuit JSON's first-class ports: gates check connectivity exactly
    /// instead of re-inferring it from geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
    /// Polyline in sheet coordinates.
    pub pts: Vec<Point>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetLabel {
    pub net: String,
    pub at: Point,
}

// ---------- stage 2: placement ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementSection {
    pub outline: Vec<Point>,
    /// Sorted by `id`.
    pub footprints: Vec<FootprintInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintInstance {
    /// Same stable ID as the schematic symbol ("U1").
    pub id: String,
    pub at: Point,
    pub rot: Millideg,
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

// ---------- stage 3: routing ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingSection {
    /// Sorted by (net, layer, first point).
    pub tracks: Vec<Track>,
    /// Sorted by (net, at).
    pub vias: Vec<Via>,
    #[serde(default)]
    pub zones: Vec<Zone>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub net: String,
    /// Pad endpoints as "REF.PIN", when the track terminates on pads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
    /// Stackup layer name ("F.Cu").
    pub layer: String,
    pub width: Um,
    pub pts: Vec<Point>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Via {
    pub net: String,
    pub at: Point,
    pub drill: Um,
    pub diameter: Um,
    pub from_layer: String,
    pub to_layer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub net: String,
    pub layer: String,
    pub outline: Vec<Point>,
}

impl Design {
    /// Canonical bytes: sorted vectors, then compact JSON. Hash these.
    pub fn canonical_bytes(&self) -> serde_json::Result<Vec<u8>> {
        let mut d = self.clone();
        if let Some(s) = &mut d.schematic {
            s.symbols.sort_by(|a, b| a.id.cmp(&b.id));
            s.wires.sort_by(|a, b| (&a.net, a.pts.first()).cmp(&(&b.net, b.pts.first())));
            s.labels.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
        }
        if let Some(p) = &mut d.placement {
            p.footprints.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if let Some(r) = &mut d.routing {
            r.tracks
                .sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
            r.vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
            r.zones.sort_by(|a, b| (&a.net, &a.layer).cmp(&(&b.net, &b.layer)));
        }
        serde_json::to_vec(&d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Design {
        Design {
            schema: 1,
            provenance: Provenance {
                engine_version: "0.1.0".into(),
                intent_hash: "b3a9".into(),
                seed: 7,
                stage_hashes: vec![],
            },
            schematic: Some(SchematicSection {
                symbols: vec![
                    SymbolInstance { id: "U1".into(), at: Point { x: 50_800, y: 63_500 }, rot: 0, mirrored: false },
                    SymbolInstance { id: "C1".into(), at: Point { x: 38_100, y: 63_500 }, rot: 90_000, mirrored: false },
                ],
                wires: vec![Wire { net: "VIN".into(), pins: vec!["U1.3".into(), "C1.1".into()], pts: vec![Point { x: 35_000, y: 60_000 }, Point { x: 48_000, y: 60_000 }] }],
                labels: vec![],
            }),
            placement: None,
            routing: None,
        }
    }

    #[test]
    fn roundtrip() {
        let d = sample();
        let json = serde_json::to_string(&d).unwrap();
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schematic.as_ref().unwrap().symbols.len(), 2);
    }

    #[test]
    fn canonical_is_order_independent() {
        let d1 = sample();
        let mut d2 = sample();
        d2.schematic.as_mut().unwrap().symbols.reverse();
        assert_eq!(d1.canonical_bytes().unwrap(), d2.canonical_bytes().unwrap());
    }

    #[test]
    fn unknown_fields_rejected() {
        let json = r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0},"bogus":1}"#;
        assert!(serde_json::from_str::<Design>(json).is_err());
    }

    #[test]
    fn empty_sections_omitted() {
        let mut d = sample();
        d.schematic = None;
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("schematic"));
    }
}
