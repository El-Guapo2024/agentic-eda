//! The layers of a board: which of the optional ones are on, what the user calls each, and what type each has (Board Setup > Board Stackup > Board Editor Layers).
//!
//! The copper layers are the stackup's (`BoardRules::layers`, and the Physical Stackup page changes their number); everything else a board can have is here: the
//! technical layers KiCad lists after the copper ones, and up to 45 user layers (`User.1` .. `User.45`). A layer's name in the board file is fixed (`F.SilkS`, `User.3`,
//! `In2.Cu`); what the user can change is the name KiCad *shows*, which the file keeps as a fourth word of the layer's entry.
//!
//! Ported from:
//!   pcbnew/dialogs/panel_setup_layers.cpp   the page: which layers it offers, the mandatory ones, the three kinds of type, `testLayerNames`, `TransferDataFromWindow`
//!   pcbnew/board.cpp                        `GetLayerName`, `SetLayerName`, `LAYER::ShowType` / `ParseType`
//!   common/lset.cpp                         `LSET::Name` (the file's names), `TechAndUserUIOrder` (the order the file and the pickers list them in)
//!   common/layer_id.cpp                     `LayerName` (the names KiCad shows by default: `F.Silkscreen`, `User.Drawings` ...)
//!   pcbnew/pcb_io/kicad_sexpr/              `formatBoardLayers` (the file's `(layers ..)`), `parseLayers` (what reads it back)

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::CheckResult;

/// `LSET::TechAndUserUIOrder` without the user layers: the standard technical layers by the names the board file writes, in the order KiCad lists them.
pub const TECH_LAYERS: [&str; 18] = [
    "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab",
];

/// The layers whose check box the page keeps checked (`mandatoryLayerCbSetup`): a board always has its courtyards, its outline and its margin.
pub const MANDATORY_LAYERS: [&str; 4] = ["F.CrtYd", "B.CrtYd", "Edge.Cuts", "Margin"];

/// `User_1` .. `User_45`.
pub const MAX_USER_LAYERS: u32 = 45;

/// `PCB_LAYER_ID` (`include/layer_ids.h`): the number KiCad's `LSET` iterates a board's layers in, front and back interleaved (F.Cu 0, F.Mask 1, B.Cu 2, B.Mask 3, In1.Cu 4, F.SilkS 5 ..).
/// The page's name test (`testLayerNames`) walks the layers in this order, so it is the later of two clashing names that is refused. `None` for a name that is no layer.
pub fn pcb_layer_id(file_name: &str) -> Option<u32> {
    let fixed = match file_name {
        "F.Cu" => 0,
        "F.Mask" => 1,
        "B.Cu" => 2,
        "B.Mask" => 3,
        "F.SilkS" => 5,
        "B.SilkS" => 7,
        "F.Adhes" => 9,
        "B.Adhes" => 11,
        "F.Paste" => 13,
        "B.Paste" => 15,
        "Dwgs.User" => 17,
        "Cmts.User" => 19,
        "Eco1.User" => 21,
        "Eco2.User" => 23,
        "Edge.Cuts" => 25,
        "Margin" => 27,
        "B.CrtYd" => 29,
        "F.CrtYd" => 31,
        "B.Fab" => 33,
        "F.Fab" => 35,
        _ => 0,
    };
    if fixed != 0 || file_name == "F.Cu" {
        return Some(fixed);
    }
    if let Some(n) = user_layer_number(file_name) {
        return Some(37 + 2 * n);
    }
    let inner: u32 = file_name.strip_prefix("In")?.strip_suffix(".Cu")?.parse().ok()?;
    is_copper_layer_name(file_name).then_some(2 + 2 * inner)
}

/// `User.7` -> 7.
pub fn user_layer_number(file_name: &str) -> Option<u32> {
    let n: u32 = file_name.strip_prefix("User.")?.parse().ok()?;
    ((1..=MAX_USER_LAYERS).contains(&n) && file_name == user_layer_name(n)).then_some(n)
}

pub fn user_layer_name(n: u32) -> String {
    format!("User.{n}")
}

/// `F.Cu`, `B.Cu`, `In1.Cu` .. `In30.Cu`.
pub fn is_copper_layer_name(file_name: &str) -> bool {
    match file_name {
        "F.Cu" | "B.Cu" => true,
        other => other.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| (1..=30).contains(&n) && other == format!("In{n}.Cu")),
    }
}

/// A layer the page can enable: one of the standard technical layers, or `User.N`.
pub fn is_optional_layer(file_name: &str) -> bool {
    TECH_LAYERS.contains(&file_name) || user_layer_number(file_name).is_some()
}

/// `LayerName(layer)`: the name KiCad shows for a layer that was never renamed, given the name the board file writes (`F.SilkS` is `F.Silkscreen`, `Dwgs.User` is
/// `User.Drawings`, a copper layer or a user layer is its own).
pub fn standard_layer_name(file_name: &str) -> String {
    match file_name {
        "B.Adhes" => "B.Adhesive",
        "F.Adhes" => "F.Adhesive",
        "B.SilkS" => "B.Silkscreen",
        "F.SilkS" => "F.Silkscreen",
        "Dwgs.User" => "User.Drawings",
        "Cmts.User" => "User.Comments",
        "Eco1.User" => "User.Eco1",
        "Eco2.User" => "User.Eco2",
        "F.CrtYd" => "F.Courtyard",
        "B.CrtYd" => "B.Courtyard",
        other => other,
    }
    .to_string()
}

/// `LAYER_T`: a copper layer is a signal, a power plane, a mixed or a jumper layer (what Freerouting and other external routers read); a user layer is auxiliary
/// (it does not flip with the board) or off-board at the front or the back (it does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerType {
    Signal,
    Power,
    Mixed,
    Jumper,
    Auxiliary,
    Front,
    Back,
}

impl LayerType {
    /// `LAYER::ShowType`: the word the board file writes.
    pub fn word(self) -> &'static str {
        match self {
            LayerType::Signal => "signal",
            LayerType::Power => "power",
            LayerType::Mixed => "mixed",
            LayerType::Jumper => "jumper",
            LayerType::Auxiliary => "auxiliary",
            LayerType::Front => "front",
            LayerType::Back => "back",
        }
    }

    /// `LAYER::ParseType`.
    pub fn parse(word: &str) -> Option<LayerType> {
        [LayerType::Signal, LayerType::Power, LayerType::Mixed, LayerType::Jumper, LayerType::Auxiliary, LayerType::Front, LayerType::Back].into_iter().find(|t| t.word() == word)
    }

    pub fn is_copper_type(self) -> bool {
        matches!(self, LayerType::Signal | LayerType::Power | LayerType::Mixed | LayerType::Jumper)
    }
}

fn all_tech_layers() -> Vec<String> {
    TECH_LAYERS.iter().map(|l| l.to_string()).collect()
}

/// What the Board Editor Layers page holds for the non-copper layers and for the names and types of every layer.
///
/// `enabled` is the whole set of non-copper layers the board has (the 18 technical layers KiCad starts a board with, unless some were turned off, and the user
/// layers that were added); `names` are the names the user gave, `types` the types that are not the layer's default (a copper layer is a signal layer, a user layer
/// auxiliary). A page is replaced as a whole, like KiCad's OK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSetup {
    #[serde(default = "all_tech_layers")]
    pub enabled: Vec<String>,
    /// The name the user gave a layer, by the name the board file writes it with. A layer that is not here shows its standard name (`LayerName`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, LayerType>,
}

impl Default for LayerSetup {
    /// A board as KiCad starts one: every standard technical layer, none of the user layers, nothing renamed, every copper layer a signal layer.
    fn default() -> Self {
        LayerSetup { enabled: all_tech_layers(), names: BTreeMap::new(), types: BTreeMap::new() }
    }
}

impl LayerSetup {
    /// Whether this is what a board has when nobody opened the page (nothing to write beyond what the writer always writes).
    pub fn is_default(&self) -> bool {
        self.canonical() == LayerSetup::default()
    }

    /// The same setup with its layers in `TechAndUserUIOrder`, repeated ones once and every default-valued entry dropped (a name equal to the standard name, a type
    /// equal to the layer's default): two pages that mean the same thing compare equal.
    pub fn canonical(&self) -> LayerSetup {
        let on: BTreeSet<&str> = self.enabled.iter().map(String::as_str).collect();
        let mut enabled: Vec<String> = TECH_LAYERS.iter().filter(|l| on.contains(**l)).map(|l| l.to_string()).collect();
        let mut users: Vec<u32> = on.iter().filter_map(|l| user_layer_number(l)).collect();
        users.sort_unstable();
        enabled.extend(users.into_iter().map(user_layer_name));
        enabled.extend(on.iter().filter(|l| !is_optional_layer(l)).map(|l| l.to_string()));
        let names = self.names.iter().filter(|(layer, name)| !name.is_empty() && **name != standard_layer_name(layer)).map(|(l, n)| (l.clone(), n.clone())).collect();
        let types = self.types.iter().filter(|(layer, t)| **t != default_type(layer)).map(|(l, t)| (l.clone(), *t)).collect();
        LayerSetup { enabled, names, types }
    }

    /// The enabled non-copper layers in `TechAndUserUIOrder`.
    pub fn enabled_in_order(&self) -> Vec<String> {
        self.canonical().enabled.into_iter().filter(|l| is_optional_layer(l)).collect()
    }

    /// Whether the board has `layer` (a copper layer of `copper`, or an enabled optional one).
    pub fn is_enabled(&self, layer: &str, copper: &[String]) -> bool {
        copper.iter().any(|c| c == layer) || self.enabled.iter().any(|l| l == layer)
    }

    /// `BOARD::GetLayerName`: the name the user gave the layer, else KiCad's standard one.
    pub fn display_name(&self, layer: &str) -> String {
        self.names.get(layer).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| standard_layer_name(layer))
    }

    /// `BOARD::GetLayerType`: what the user chose, else the layer's default (a copper layer is a signal layer, a user layer auxiliary, a technical layer has none).
    pub fn layer_type(&self, layer: &str) -> LayerType {
        self.types.get(layer).copied().unwrap_or_else(|| default_type(layer))
    }

    /// `PANEL_SETUP_LAYERS::testLayerNames` and the page's other limits, on the setup as it would be stored for a board whose copper layers are `copper`. The first
    /// problem as a refusal whose `location` is the layer's name in the board file.
    pub fn validate(&self, copper: &[String]) -> Result<(), CheckResult> {
        let bad = |layer: &str, why: String| Err(CheckResult::fail("ops_bad_board_layers", layer, why));
        let mut seen = BTreeSet::new();
        for layer in &self.enabled {
            if !is_optional_layer(layer) {
                return bad(layer, format!("{layer:?} is not a layer a board can have besides its copper ones (the technical layers and User.1 to User.{MAX_USER_LAYERS})"));
            }
            if !seen.insert(layer.as_str()) {
                return bad(layer, format!("{layer} is listed twice"));
            }
        }
        for mandatory in MANDATORY_LAYERS {
            if !seen.contains(mandatory) {
                return bad(mandatory, "This layer is required and cannot be disabled".into());
            }
        }
        let on = |layer: &str| copper.iter().any(|c| c == layer) || seen.contains(layer);
        for (layer, t) in &self.types {
            let ok = if is_copper_layer_name(layer) { on(layer) && t.is_copper_type() } else { seen.contains(layer.as_str()) && user_layer_number(layer).is_some() && !t.is_copper_type() };
            if !ok {
                return bad(layer, if is_copper_layer_name(layer) || user_layer_number(layer).is_some() { format!("{layer} cannot be {}: a copper layer is a signal, power, mixed or jumper layer and a user layer auxiliary or off-board front or back", t.word()) } else { format!("{layer} has no type: only copper and user layers do") });
            }
        }
        // Names: every layer the board has, each as the page would show it, in the order `testLayerNames` walks them (`LSET::AllLayersMask`, by `PCB_LAYER_ID`).
        let forbidden = "*?\\/:\"<>|%";
        let mut shown: Vec<(String, String)> = Vec::new();
        let mut have: Vec<String> = copper.iter().cloned().chain(self.enabled_in_order()).collect();
        have.sort_by_key(|l| pcb_layer_id(l).unwrap_or(u32::MAX));
        for layer in have {
            shown.push((self.display_name(&layer), layer));
        }
        for layer in self.names.keys() {
            if !on(layer) {
                return bad(layer, format!("{layer} is not a layer this board has, so it cannot be named"));
            }
        }
        let mut used: BTreeSet<&str> = BTreeSet::new();
        for (name, layer) in &shown {
            if name.is_empty() {
                return bad(layer, "Layer must have a name.".into());
            }
            if name.chars().any(|c| forbidden.contains(c) || c.is_control()) {
                return bad(layer, format!("{forbidden} are forbidden in layer names."));
            }
            if name == "signal" {
                return bad(layer, "Layer name \"signal\" is reserved.".into());
            }
            if !used.insert(name.as_str()) {
                return bad(layer, format!("Layer name '{name}' already in use."));
            }
        }
        for (layer, name) in &self.names {
            if name.is_empty() {
                return bad(layer, "Layer must have a name.".into());
            }
        }
        Ok(())
    }
}

fn default_type(layer: &str) -> LayerType {
    if is_copper_layer_name(layer) {
        LayerType::Signal
    } else {
        LayerType::Auxiliary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copper(n: usize) -> Vec<String> {
        crate::rules::copper_layer_names(n)
    }

    #[test]
    fn a_board_starts_with_every_standard_technical_layer_and_none_of_the_user_ones() {
        let s = LayerSetup::default();
        assert_eq!(s.enabled.len(), 18);
        assert!(s.is_default() && s.validate(&copper(2)).is_ok());
        assert_eq!(s.enabled_in_order(), TECH_LAYERS.iter().map(|l| l.to_string()).collect::<Vec<_>>());
        assert_eq!(serde_json::from_str::<LayerSetup>("{}").unwrap(), s, "a design from before the page loads as the default");
        assert_eq!((s.display_name("F.SilkS"), s.display_name("Dwgs.User"), s.display_name("F.Mask"), s.display_name("In1.Cu"), s.display_name("User.3")), ("F.Silkscreen".into(), "User.Drawings".into(), "F.Mask".into(), "In1.Cu".into(), "User.3".into()));
    }

    #[test]
    fn the_names_and_numbers_of_layers_are_the_files() {
        assert_eq!((user_layer_number("User.1"), user_layer_number("User.45"), user_layer_number("User.46"), user_layer_number("User.0"), user_layer_number("User.07"), user_layer_number("Users.1")), (Some(1), Some(45), None, None, None, None));
        assert!(["F.Cu", "B.Cu", "In1.Cu", "In30.Cu"].iter().all(|l| is_copper_layer_name(l)));
        assert!(!["In0.Cu", "In31.Cu", "F.Mask", "In01.Cu", "Cu"].iter().any(|l| is_copper_layer_name(l)));
        assert!(is_optional_layer("F.Fab") && is_optional_layer("User.12") && !is_optional_layer("F.Cu") && !is_optional_layer("Rescue"));
        assert_eq!((LayerType::parse("power"), LayerType::parse("auxiliary"), LayerType::parse("user"), LayerType::Jumper.word()), (Some(LayerType::Power), Some(LayerType::Auxiliary), None, "jumper"));
    }

    #[test]
    fn two_pages_that_mean_the_same_are_the_same_page() {
        let a = LayerSetup { enabled: vec!["User.2".into(), "User.1".into(), "B.Fab".into()].into_iter().chain(TECH_LAYERS.iter().filter(|l| **l != "B.Fab").map(|l| l.to_string())).collect(), names: [("F.SilkS".to_string(), "F.Silkscreen".to_string()), ("User.1".to_string(), "Notes".to_string())].into(), types: [("In1.Cu".to_string(), LayerType::Signal), ("User.2".to_string(), LayerType::Front)].into() };
        let c = a.canonical();
        assert_eq!(c.enabled.len(), 20);
        assert_eq!(&c.enabled[16..], ["F.Fab", "B.Fab", "User.1", "User.2"], "the technical layers in KiCad's order, then the user layers by number");
        assert_eq!(c.names, [("User.1".to_string(), "Notes".to_string())].into(), "a name that is the standard one is no name");
        assert_eq!(c.types, [("User.2".to_string(), LayerType::Front)].into(), "a default type is no type");
        assert!(!a.is_default());
        assert!(LayerSetup { enabled: TECH_LAYERS.iter().rev().map(|l| l.to_string()).collect(), ..Default::default() }.is_default(), "the order of the list is not part of the page");
    }

    #[test]
    fn the_page_refuses_what_the_panel_refuses_naming_the_layer() {
        let with = |f: &dyn Fn(&mut LayerSetup)| {
            let mut s = LayerSetup::default();
            f(&mut s);
            s
        };
        let err = |s: LayerSetup, cu: usize| s.validate(&copper(cu)).unwrap_err();
        let e = err(with(&|s| s.enabled.retain(|l| l != "Edge.Cuts")), 2);
        assert_eq!((e.check.as_str(), e.location.as_deref()), ("ops_bad_board_layers", Some("Edge.Cuts")), "a mandatory layer cannot be turned off");
        assert_eq!(err(with(&|s| s.enabled.retain(|l| l != "Margin")), 2).location.as_deref(), Some("Margin"));
        assert_eq!(err(with(&|s| s.enabled.push("User.46".into())), 2).location.as_deref(), Some("User.46"));
        assert_eq!(err(with(&|s| s.enabled.push("F.Cu".into())), 2).location.as_deref(), Some("F.Cu"), "copper is the stackup's, not an optional layer");
        assert_eq!(err(with(&|s| s.enabled.push("F.Fab".into())), 2).location.as_deref(), Some("F.Fab"), "listed twice");
        let e = err(with(&|s| drop(s.names.insert("F.SilkS".into(), "".into()))), 2);
        assert_eq!((e.location.as_deref(), e.hint.as_deref()), (Some("F.SilkS"), Some("Layer must have a name.")));
        let e = err(with(&|s| drop(s.names.insert("F.SilkS".into(), "a/b".into()))), 2);
        assert!(e.hint.as_deref().unwrap().ends_with("are forbidden in layer names."), "{e:?}");
        for bad in ["a\"b", "a:b", "a*b", "a%b", "a|b", "tab\\t"] {
            assert!(with(&|s| drop(s.names.insert("F.SilkS".into(), bad.into()))).validate(&copper(2)).is_err(), "{bad}");
        }
        assert_eq!(err(with(&|s| drop(s.names.insert("F.SilkS".into(), "signal".into()))), 2).hint.as_deref(), Some("Layer name \"signal\" is reserved."));
        // a name already in use, by a layer that was renamed or by one that shows its standard name
        let e = err(with(&|s| drop(s.names.insert("F.Fab".into(), "B.Fab".into()))), 2);
        assert_eq!((e.location.as_deref(), e.hint.as_deref()), (Some("F.Fab"), Some("Layer name 'B.Fab' already in use.")), "the later of the two layers in PCB_LAYER_ID order (B.Fab 33, F.Fab 35) is the one named");
        assert!(with(&|s| {
            s.names.insert("F.Fab".into(), "Assembly".into());
            s.names.insert("B.Fab".into(), "F.Fab".into());
        })
        .validate(&copper(2))
        .is_ok(), "a name another layer gave up is free");
        assert_eq!(err(with(&|s| drop(s.names.insert("User.9".into(), "Nope".into()))), 2).location.as_deref(), Some("User.9"), "a layer the board does not have cannot be named");
    }

    #[test]
    fn layers_are_numbered_as_kicads_pcb_layer_id() {
        let id = |l: &str| pcb_layer_id(l);
        assert_eq!((id("F.Cu"), id("F.Mask"), id("B.Cu"), id("B.Mask"), id("In1.Cu"), id("F.SilkS"), id("In2.Cu"), id("B.SilkS")), (Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(6), Some(7)));
        assert_eq!((id("Edge.Cuts"), id("Margin"), id("B.CrtYd"), id("F.CrtYd"), id("B.Fab"), id("F.Fab"), id("User.1"), id("User.45"), id("In30.Cu")), (Some(25), Some(27), Some(29), Some(31), Some(33), Some(35), Some(39), Some(127), Some(62)));
        assert_eq!((id("User.46"), id("In31.Cu"), id("Rescue"), id("")), (None, None, None, None));
        let mut ids: Vec<u32> = TECH_LAYERS.iter().map(|l| id(l).unwrap()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 18, "every technical layer has a number of its own");
    }

    #[test]
    fn types_are_the_pages_three_kinds_and_only_where_a_layer_has_one() {
        let with = |layer: &str, t: LayerType, users: &[&str]| LayerSetup { enabled: TECH_LAYERS.iter().map(|l| l.to_string()).chain(users.iter().map(|u| u.to_string())).collect(), types: [(layer.to_string(), t)].into(), ..Default::default() };
        assert!(with("In1.Cu", LayerType::Power, &[]).validate(&copper(4)).is_ok());
        assert!(with("F.Cu", LayerType::Jumper, &[]).validate(&copper(2)).is_ok());
        assert_eq!(with("In1.Cu", LayerType::Power, &[]).validate(&copper(2)).unwrap_err().location.as_deref(), Some("In1.Cu"), "a copper layer the board does not have");
        assert!(with("User.2", LayerType::Back, &["User.2"]).validate(&copper(2)).is_ok());
        assert!(with("User.2", LayerType::Front, &["User.2"]).validate(&copper(2)).is_ok());
        assert!(with("User.2", LayerType::Power, &["User.2"]).validate(&copper(2)).is_err(), "a user layer is not a power plane");
        assert!(with("In1.Cu", LayerType::Front, &[]).validate(&copper(4)).is_err(), "a copper layer is not off-board");
        assert!(with("F.SilkS", LayerType::Auxiliary, &[]).validate(&copper(2)).is_err(), "a technical layer has no type");
        assert!(with("User.2", LayerType::Back, &[]).validate(&copper(2)).is_err(), "a layer that is off has no type");
        let s = with("In1.Cu", LayerType::Mixed, &[]);
        assert_eq!((s.layer_type("In1.Cu"), s.layer_type("F.Cu"), s.layer_type("User.1")), (LayerType::Mixed, LayerType::Signal, LayerType::Auxiliary));
    }

    #[test]
    fn whether_a_board_has_a_layer_follows_the_stackup_and_the_page() {
        let s = LayerSetup { enabled: vec!["Edge.Cuts".into(), "User.4".into()], ..Default::default() };
        let cu = copper(4);
        assert!(s.is_enabled("In2.Cu", &cu) && !s.is_enabled("In2.Cu", &copper(2)));
        assert!(s.is_enabled("User.4", &cu) && !s.is_enabled("User.5", &cu) && !s.is_enabled("F.Fab", &cu));
    }
}
