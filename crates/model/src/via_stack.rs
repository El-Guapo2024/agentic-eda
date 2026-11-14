//! KiCad 10's microvia stack (file version 20260830, "Microvia stack generator (via_stack)"; `PCB_VIA_STACK`, `pcbnew/generators/pcb_via_stack.h` of KiCad 25ec2335 --
//! the 8303b2ad source the rest of the port reads predates it): a generator (`PCB_GENERATOR`, `(generated (type via_stack) ..)`) of several single-hop microvias, one per
//! pair of adjacent copper layers from its start layer to its end layer, plus a short connecting trace on each shared landing layer when the stack is staggered. "Each hop
//! stays an ordinary PCB_VIA, bound as a group member": the vias and traces are ordinary items of the board, listed as its members, rebuilt from the stack's settings
//! (`PCB_VIA_STACK::Regenerate`) whenever they change.
//!
//! The board keeps them in [`crate::pcb_items::BoardItems::via_stacks`]. What the file has (`PCB_VIA_STACK::GetProperties`, written in the order of a `std::map`):
//! `capped`, `end_layer`, `filled`, `hops` (a staggered stack's hop centres), `origin` (the generator's position), `pitch`, `preset`, `start_layer`, `style`
//! ("stacked" / "staggered"), `use_netclass`, `via_drill`, `via_size`; then the members. The generator's own `(layer ..)` is its start layer
//! (`SetStartLayer` also `SetLayer`s: "The stack registers in the view on its own layer, so it tracks the start layer"). Its net is not written: it is its members'
//! (`GetNetCode`: "Read from a member when present, else the stored value"); [`MicroviaStack::net`] is the stored value, the net a stack is built on before it has members.

use serde::{Deserialize, Serialize};

use crate::ir::{Nm, Point};
use crate::setup_pages::ViaStackPreset;

/// `VIA_STACK_STYLE`: the file's `(style "stacked"|"staggered")`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViaStackStyle {
    /// Coaxial hops, every via at the stack's position (`STACKED`).
    #[default]
    Stacked,
    /// Each hop offset from the one before by the pitch, joined by a trace on the layer they share (`STAGGERED`).
    Staggered,
}

impl ViaStackStyle {
    /// `styleToString`.
    pub fn word(self) -> &'static str {
        match self {
            ViaStackStyle::Stacked => "stacked",
            ViaStackStyle::Staggered => "staggered",
        }
    }
    /// `styleFromString`: anything but "staggered" is stacked.
    pub fn from_word(word: &str) -> ViaStackStyle {
        if word == "staggered" {
            ViaStackStyle::Staggered
        } else {
            ViaStackStyle::Stacked
        }
    }
}

fn is_zero_nm(v: &Nm) -> bool {
    *v == Nm::ZERO
}
fn yes() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}
fn d_start() -> String {
    "F.Cu".into()
}
fn d_end() -> String {
    "In1.Cu".into()
}
fn d_name() -> String {
    MicroviaStack::DISPLAY_NAME.into()
}

/// One microvia stack generator (`PCB_VIA_STACK`). See the module doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicroviaStack {
    /// `vsk_xxxxxxxxxxxx`, assigned by the board.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// `GetName()`: "Microvia Stack" (`DISPLAY_NAME`).
    #[serde(default = "d_name")]
    pub name: String,
    /// `m_startLayer` (also the generator's own layer) and `m_endLayer`: copper layers by the names the board file writes.
    #[serde(default = "d_start")]
    pub start_layer: String,
    #[serde(default = "d_end")]
    pub end_layer: String,
    #[serde(default)]
    pub style: ViaStackStyle,
    /// `m_pitch`: "center to center distance, staggered only".
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    pub pitch_nm: Nm,
    /// `m_filled`: "copper filled hops, forced on when stacked".
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub filled: bool,
    /// `m_capped`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub capped: bool,
    /// `m_viaSize` / `m_viaDrill`: "0 means take from netclass / board defaults".
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    pub via_size_nm: Nm,
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    pub via_drill_nm: Nm,
    /// `m_useNetclass`: "Use netclass values".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_netclass: bool,
    /// `m_presetName`: "the preset this stack was placed from, if any".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preset: String,
    /// `PCB_GENERATOR::m_origin`: the stack's position (`GetPosition`), where a stacked stack's hops are and a staggered one's first hop.
    pub origin: Point,
    /// `m_hops`: "Hop center positions for a staggered stack. Empty for a stacked (coaxial) stack." `None` when the file has no `(hops ..)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hops: Option<Vec<Point>>,
    /// `m_netCode`: "net used when building hops, before members exist". Not in the file; a stack with members is on its members' net ([`Self::net_of`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub net: String,
    /// The ids of the board's vias and tracks the stack is made of (`GetItems()`, the file's `(members ..)`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member_ids: Vec<String>,
    /// KiCad 10's `(custom_property "key" "value")` forms, kept as the file has them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_properties: Vec<(String, String)>,
}

impl MicroviaStack {
    /// `PCB_VIA_STACK::DISPLAY_NAME`.
    pub const DISPLAY_NAME: &'static str = "Microvia Stack";
    /// `PCB_VIA_STACK::GENERATOR_TYPE`.
    pub const GENERATOR_TYPE: &'static str = "via_stack";

    /// `PCB_VIA_STACK( parent, F_Cu )`: F.Cu to In1.Cu, stacked, no pitch, copper-filled, not capped, no sizes (the board's), no net class, no net.
    pub fn new(origin: Point) -> MicroviaStack {
        MicroviaStack {
            id: String::new(),
            name: d_name(),
            start_layer: d_start(),
            end_layer: d_end(),
            style: ViaStackStyle::Stacked,
            pitch_nm: Nm::ZERO,
            filled: true,
            capped: false,
            via_size_nm: Nm::ZERO,
            via_drill_nm: Nm::ZERO,
            use_netclass: false,
            preset: String::new(),
            origin,
            hops: None,
            net: String::new(),
            member_ids: Vec::new(),
            custom_properties: Vec::new(),
        }
    }

    pub fn id_seed(&self) -> String {
        format!("via_stack:{}:{}:{}:{:?}", self.start_layer, self.end_layer, self.net, self.origin)
    }

    /// `ApplyPreset`: every setting of the preset, its name as the stack's preset.
    pub fn apply_preset(&mut self, p: &ViaStackPreset) {
        self.start_layer = p.start_layer.clone();
        self.end_layer = p.end_layer.clone();
        self.style = if p.staggered { ViaStackStyle::Staggered } else { ViaStackStyle::Stacked };
        self.via_size_nm = p.via_size_nm;
        self.via_drill_nm = p.via_drill_nm;
        self.use_netclass = p.use_netclass;
        self.filled = p.filled;
        self.capped = p.capped;
        self.pitch_nm = p.pitch_nm;
        self.preset = p.name.clone();
    }

    /// `ToPreset`.
    pub fn to_preset(&self) -> ViaStackPreset {
        ViaStackPreset {
            name: self.preset.clone(),
            start_layer: self.start_layer.clone(),
            end_layer: self.end_layer.clone(),
            staggered: self.style == ViaStackStyle::Staggered,
            via_size_nm: self.via_size_nm,
            via_drill_nm: self.via_drill_nm,
            use_netclass: self.use_netclass,
            filled: self.filled,
            capped: self.capped,
            pitch_nm: self.pitch_nm,
        }
    }

    /// `GetNetCode`: the net of the first member that has one (a via or a track of the board, `routing_net` naming each), else the stored one.
    pub fn net_of(&self, routing_net: impl Fn(&str) -> Option<String>) -> String {
        self.member_ids.iter().find_map(|m| routing_net(m)).unwrap_or_else(|| self.net.clone())
    }

    /// `Move`: the position, and "The hop polyline stores absolute positions, so it must follow every transform".
    pub fn translate(&mut self, dx: Nm, dy: Nm) {
        self.origin = Point { x: self.origin.x + dx, y: self.origin.y + dy };
        for p in self.hops.iter_mut().flatten() {
            *p = Point { x: p.x + dx, y: p.y + dy };
        }
    }
}

/// `IsExternalCopperLayer`.
pub fn is_external_copper(layer: &str) -> bool {
    layer == "F.Cu" || layer == "B.Cu"
}

/// The copper layers a stack spans, in physical stack order from `start` to `end` (`LAYER_RANGE( start, end, copperCount )`): `copper` is the board's copper layers front
/// to back. Empty when either end is not on the board.
pub fn layer_range(copper: &[String], start: &str, end: &str) -> Vec<String> {
    let (Some(a), Some(b)) = (copper.iter().position(|l| l == start), copper.iter().position(|l| l == end)) else { return Vec::new() };
    if a <= b {
        copper[a..=b].to_vec()
    } else {
        copper[b..=a].iter().rev().cloned().collect()
    }
}

/// `IsSpanValid`: "True when both span ends are copper layers within the board's copper layer count" and they differ.
pub fn is_span_valid(copper: &[String], start: &str, end: &str) -> bool {
    copper.iter().any(|l| l == start) && copper.iter().any(|l| l == end) && start != end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cu(n: usize) -> Vec<String> {
        let mut v = vec!["F.Cu".to_string()];
        v.extend((1..n - 1).map(|i| format!("In{i}.Cu")));
        v.push("B.Cu".into());
        v
    }

    #[test]
    fn a_span_walks_the_stack_in_physical_order_either_way() {
        let c = cu(6);
        assert_eq!(layer_range(&c, "F.Cu", "In2.Cu"), ["F.Cu", "In1.Cu", "In2.Cu"]);
        assert_eq!(layer_range(&c, "B.Cu", "In3.Cu"), ["B.Cu", "In4.Cu", "In3.Cu"]);
        assert!(layer_range(&c, "F.Cu", "In9.Cu").is_empty());
        assert!(is_span_valid(&c, "In1.Cu", "B.Cu"));
        assert!(!is_span_valid(&c, "In1.Cu", "In1.Cu"));
        assert!(!is_span_valid(&cu(2), "F.Cu", "In1.Cu"));
    }

    #[test]
    fn a_preset_round_trips_through_a_stack() {
        let p = ViaStackPreset { name: "HDI".into(), start_layer: "F.Cu".into(), end_layer: "In2.Cu".into(), staggered: true, via_size_nm: Nm::new(250_000), via_drill_nm: Nm::new(100_000), use_netclass: false, filled: false, capped: true, pitch_nm: Nm::new(450_000) };
        let mut s = MicroviaStack::new(Point { x: Nm::ZERO, y: Nm::ZERO });
        s.apply_preset(&p);
        assert_eq!(s.to_preset(), p);
        assert_eq!(s.style, ViaStackStyle::Staggered);
        assert_eq!((ViaStackStyle::from_word("staggered"), ViaStackStyle::from_word("whatever")), (ViaStackStyle::Staggered, ViaStackStyle::Stacked));
    }
}
