//! The Appearance panel's share of the derived KiCad project: what the person chose in the studio's Appearance panel (`appearance.json`, written by
//! `crates/cli/src/appearance_api.rs`, keys documented in `web/studio/src/kicad-port/appearanceFile.ts`) in the two files KiCad keeps it in, so the
//! board opens in KiCad with the same layers, objects, colours, presets and views on screen.
//!
//! - `<stem>.kicad_prl`, the project's *local* settings (`PROJECT_LOCAL_SETTINGS`): which layers and objects are shown, the opacities, the inactive-layer and
//!   net colour modes, the hidden nets and net classes, the active layer. [`local_settings`].
//! - `<stem>.kicad_pro`, the project file: `net_settings.net_colors`, the saved layer presets (`board.layer_presets`) and viewports (`board.viewports`).
//!   [`merge_into_project`].
//!
//! The settings file is the studio's own and never part of the design; these files are derived from it, scratch output like the rest of `.kicad/`.
//! kicad-cli does not read any of this for DRC or exports, so a missing or odd settings file cannot change a check: every function here ignores what it
//! cannot read and returns what it can.
//!
//! Not written: the net classes' `pcb_color` (`net_settings.classes[].pcb_color`). A class defined in the project file as well as in the board's own
//! `(net_class ...)` block is refused by KiCad when the board loads ("Duplicate NETCLASS name", `PCB_IO_KICAD_SEXPR_PARSER::parseNETCLASS`), and the board file
//! has no place for a colour, so a class colour can only travel with the whole class definition -- see [`class_colors`] for how the engine uses them.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// `PCB_LAYER_ID_COUNT`: the width of a layer set.
const LAYER_COUNT: usize = 128;

/// The number KiCad gives a layer (`PCB_LAYER_ID`): F.Cu 0, F.Mask 1, B.Cu 2, B.Mask 3, In1.Cu 4, F.SilkS 5 ... Edge.Cuts 25, F.Fab 35.
pub fn layer_id(name: &str) -> Option<usize> {
    let fixed = match name {
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
        _ => {
            if let Some(n) = name.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=30).contains(n)) {
                return Some(2 + 2 * n);
            }
            if let Some(n) = name.strip_prefix("User.").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=45).contains(n)) {
                return Some(37 + 2 * n);
            }
            return None;
        }
    };
    Some(fixed)
}

/// `BASE_SET::FmtHex` of a 128-bit layer set: nibbles from the lowest bit, an underscore after every 8 nibbles, written most significant first
/// (`ffffffff_ffffffff_ffffffff_ffffffff` is every layer).
pub fn fmt_hex(bits: &[bool; LAYER_COUNT]) -> String {
    let mut out = String::new();
    for nibble in 0..LAYER_COUNT / 4 {
        let mut v = 0u32;
        for b in 0..4 {
            if bits[nibble * 4 + b] {
                v |= 1 << b;
            }
        }
        if nibble > 0 && nibble % 8 == 0 {
            out.push('_');
        }
        out.push(char::from_digit(v, 16).unwrap_or('0'));
    }
    out.chars().rev().collect()
}

/// `board.visible_layers`: every layer on but the ones named as hidden.
pub fn visible_layers_hex(hidden: &[String]) -> String {
    let mut bits = [true; LAYER_COUNT];
    for name in hidden {
        if let Some(id) = layer_id(name) {
            bits[id] = false;
        }
    }
    fmt_hex(&bits)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn section<'a>(appearance: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    appearance.get(key).and_then(Value::as_object)
}

/// The `.kicad_prl` for `stem`: the `board` part of the project's local settings, from the studio's `local` section. `None` when the studio saved nothing
/// (no file, or an empty `local`), so a project that was never touched gets no file of its own.
pub fn local_settings(appearance: &Value, stem: &str) -> Option<Value> {
    let local = section(appearance, "local").filter(|l| !l.is_empty())?;
    let mut board = Map::new();
    if let Some(items) = local.get("visible_items").filter(|v| v.is_array()) {
        board.insert("visible_items".into(), items.clone());
    }
    if let Some(hidden) = local.get("hidden_layers").filter(|v| v.is_array()) {
        board.insert("visible_layers".into(), json!(visible_layers_hex(&strings(Some(hidden)))));
    }
    if let Some(id) = local.get("active_layer").and_then(Value::as_str).and_then(layer_id) {
        board.insert("active_layer".into(), json!(id));
    }
    if let Some(preset) = local.get("active_layer_preset").and_then(Value::as_str) {
        board.insert("active_layer_preset".into(), json!(preset));
    }
    for key in ["high_contrast_mode", "net_color_mode"] {
        if let Some(n) = local.get(key).and_then(Value::as_i64).filter(|n| (0..=2).contains(n)) {
            board.insert(key.into(), json!(n));
        }
    }
    if let Some(opacity) = local.get("opacity").and_then(Value::as_object) {
        let mut out = Map::new();
        for key in ["tracks", "vias", "pads", "zones", "images", "shapes"] {
            if let Some(v) = opacity.get(key).and_then(Value::as_f64).filter(|v| (0.0..=1.0).contains(v)) {
                out.insert(key.into(), json!(v));
            }
        }
        board.insert("opacity".into(), Value::Object(out));
    }
    if local.get("hidden_nets").is_some_and(Value::is_array) {
        board.insert("hidden_nets".into(), json!(strings(local.get("hidden_nets"))));
    }
    if local.get("hidden_netclasses").is_some_and(Value::is_array) {
        board.insert("hidden_netclasses".into(), json!(strings(local.get("hidden_netclasses"))));
    }
    Some(json!({ "board": board, "meta": { "filename": format!("{stem}.kicad_prl"), "version": 5 } }))
}

/// One saved layer preset as `PARAM_LAYER_PRESET::presetsToJson` writes it: layer names turned into numbers, `-2` (`UNSELECTED_LAYER`) when it names no active layer.
fn preset_json(p: &Value) -> Option<Value> {
    let name = p.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())?;
    let layers: Vec<usize> = {
        let mut ids: Vec<usize> = strings(p.get("layers")).iter().filter_map(|n| layer_id(n)).collect();
        ids.sort_unstable();
        ids
    };
    let active = p.get("activeLayer").and_then(Value::as_str).and_then(layer_id).map_or(-2, |id| id as i64);
    Some(json!({
        "name": name,
        "activeLayer": active,
        "flipBoard": p.get("flipBoard").and_then(Value::as_bool).unwrap_or(false),
        "layers": layers,
        "renderLayers": strings(p.get("renderLayers")),
    }))
}

/// One saved viewport as `PARAM_VIEWPORT::viewportsToJson` writes it: the rectangle in nanometres (the studio keeps micrometres).
fn viewport_json(v: &Value) -> Option<Value> {
    let name = v.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())?;
    let um = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0) * 1000.0;
    Some(json!({ "name": name, "x": um("x"), "y": um("y"), "w": um("w"), "h": um("h") }))
}

/// Puts the studio's `project` section into a derived `.kicad_pro`: the net colours (`net_settings.net_colors`), the layer presets and the viewports
/// (`board.layer_presets`, `board.viewports`). Nothing is added for an empty list, so an untouched project is byte-for-byte what it was.
pub fn merge_into_project(project: &mut Value, appearance: &Value) {
    let Some(p) = section(appearance, "project") else { return };
    let Some(root) = project.as_object_mut() else { return };
    let colors: Map<String, Value> = p.get("net_colors").and_then(Value::as_object).map(|m| m.iter().filter(|(_, v)| v.is_string()).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    if !colors.is_empty() {
        let ns = root.entry("net_settings").or_insert_with(|| json!({}));
        if !ns.is_object() {
            *ns = json!({});
        }
        // `net_settings` is a nested settings object with its own schema version; saying so keeps KiCad from migrating a file that has nothing to migrate.
        ns["meta"] = json!({ "version": 5 });
        ns["net_colors"] = Value::Object(colors);
    }
    let presets: Vec<Value> = p.get("layer_presets").and_then(Value::as_array).map(|a| a.iter().filter_map(preset_json).collect()).unwrap_or_default();
    let viewports: Vec<Value> = p.get("viewports").and_then(Value::as_array).map(|a| a.iter().filter_map(viewport_json).collect()).unwrap_or_default();
    if presets.is_empty() && viewports.is_empty() {
        return;
    }
    let board = root.entry("board").or_insert_with(|| json!({}));
    if !board.is_object() {
        *board = json!({});
    }
    if !presets.is_empty() {
        board["layer_presets"] = Value::Array(presets);
    }
    if !viewports.is_empty() {
        board["viewports"] = Value::Array(viewports);
    }
}

/// The net classes that have a colour of their own in the studio's `project` section: class name -> colour text (`rgb(r, g, b)` / `rgba(...)`).
pub fn class_colors(appearance: &Value) -> BTreeMap<String, String> {
    section(appearance, "project")
        .and_then(|p| p.get("netclass_colors"))
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|c| (k.clone(), c.to_string()))).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_ids_are_kicads() {
        assert_eq!(layer_id("F.Cu"), Some(0));
        assert_eq!(layer_id("B.Cu"), Some(2));
        assert_eq!(layer_id("In1.Cu"), Some(4));
        assert_eq!(layer_id("In30.Cu"), Some(62));
        assert_eq!(layer_id("F.SilkS"), Some(5));
        assert_eq!(layer_id("Edge.Cuts"), Some(25));
        assert_eq!(layer_id("F.Fab"), Some(35));
        assert_eq!(layer_id("User.1"), Some(39));
        assert_eq!(layer_id("User.45"), Some(127));
        assert_eq!(layer_id("In31.Cu"), None);
        assert_eq!(layer_id("Nonsense"), None);
    }

    #[test]
    fn every_layer_on_is_the_hex_a_kicad_prl_has() {
        // `bench.kicad_prl`, written by KiCad itself.
        assert_eq!(visible_layers_hex(&[]), "ffffffff_ffffffff_ffffffff_ffffffff");
    }

    #[test]
    fn a_hidden_layer_clears_its_bit() {
        // F.Cu is bit 0: the last nibble of the string loses its lowest bit.
        assert_eq!(visible_layers_hex(&["F.Cu".into()]), "ffffffff_ffffffff_ffffffff_fffffffe");
        // B.Cu (bit 2) and F.SilkS (bit 5).
        assert_eq!(visible_layers_hex(&["B.Cu".into(), "F.SilkS".into()]), "ffffffff_ffffffff_ffffffff_ffffffdb");
        // Edge.Cuts is bit 25: the second nibble from the left in the last group, 25 = 6 * 4 + 1.
        assert_eq!(visible_layers_hex(&["Edge.Cuts".into()]), "ffffffff_ffffffff_ffffffff_fdffffff");
        // A name KiCad does not know hides nothing.
        assert_eq!(visible_layers_hex(&["Nope".into()]), "ffffffff_ffffffff_ffffffff_ffffffff");
    }

    fn sample() -> Value {
        json!({
            "version": 1,
            "local": {
                "hidden_layers": ["B.Cu"],
                "visible_items": ["tracks", "vias"],
                "active_layer": "F.Cu",
                "active_layer_preset": "All Layers",
                "high_contrast_mode": 1,
                "net_color_mode": 2,
                "opacity": { "tracks": 1.0, "zones": 0.25, "vias": 7 },
                "hidden_nets": ["GND"],
                "hidden_netclasses": ["usb"],
            },
            "project": {
                "net_colors": { "GND": "rgb(1, 2, 3)", "VCC": 5 },
                "netclass_colors": { "usb": "rgb(9, 9, 9)" },
                "layer_presets": [
                    { "name": "Front", "activeLayer": "F.SilkS", "flipBoard": false, "layers": ["F.Cu", "F.SilkS", "Edge.Cuts", "Bogus"], "renderLayers": ["tracks", "grid"] },
                    { "name": "", "layers": [] },
                    { "name": "Bare" }
                ],
                "viewports": [ { "name": "Home", "x": 1.5, "y": 2, "w": 30, "h": 40 }, { "x": 1 } ]
            }
        })
    }

    #[test]
    fn the_local_settings_are_the_boards_part_of_a_kicad_prl() {
        let prl = local_settings(&sample(), "board").unwrap();
        let b = &prl["board"];
        assert_eq!(b["visible_items"], json!(["tracks", "vias"]));
        assert_eq!(b["visible_layers"], "ffffffff_ffffffff_ffffffff_fffffffb");
        assert_eq!(b["active_layer"], 0);
        assert_eq!(b["active_layer_preset"], "All Layers");
        assert_eq!(b["high_contrast_mode"], 1);
        assert_eq!(b["net_color_mode"], 2);
        assert_eq!(b["opacity"], json!({ "tracks": 1.0, "zones": 0.25 }), "an opacity out of range is left out");
        assert_eq!(b["hidden_nets"], json!(["GND"]));
        assert_eq!(b["hidden_netclasses"], json!(["usb"]));
        assert_eq!(prl["meta"], json!({ "filename": "board.kicad_prl", "version": 5 }));
    }

    #[test]
    fn an_untouched_project_has_no_local_settings_file() {
        assert!(local_settings(&json!({}), "b").is_none());
        assert!(local_settings(&json!({ "version": 1, "local": {}, "project": {} }), "b").is_none());
    }

    #[test]
    fn the_project_file_gets_net_colors_presets_and_viewports() {
        let mut pro = json!({ "board": { "design_settings": { "rules": {} } }, "erc": {} });
        merge_into_project(&mut pro, &sample());
        assert_eq!(pro["net_settings"]["net_colors"], json!({ "GND": "rgb(1, 2, 3)" }), "only text colours");
        assert_eq!(pro["net_settings"]["meta"]["version"], 5);
        assert!(pro["net_settings"].get("classes").is_none(), "no class definitions are written");
        let presets = pro["board"]["layer_presets"].as_array().unwrap();
        assert_eq!(presets.len(), 2, "the nameless preset is skipped");
        assert_eq!(presets[0], json!({ "name": "Front", "activeLayer": 5, "flipBoard": false, "layers": [0, 5, 25], "renderLayers": ["tracks", "grid"] }));
        assert_eq!(presets[1]["activeLayer"], -2, "no active layer is UNSELECTED_LAYER");
        assert_eq!(pro["board"]["viewports"], json!([{ "name": "Home", "x": 1500.0, "y": 2000.0, "w": 30000.0, "h": 40000.0 }]), "micrometres become nanometres");
        assert!(pro["board"]["design_settings"]["rules"].is_object(), "what was there stays");
    }

    #[test]
    fn nothing_to_say_leaves_the_project_alone() {
        let mut pro = json!({ "board": { "design_settings": {} } });
        let before = pro.clone();
        merge_into_project(&mut pro, &json!({}));
        merge_into_project(&mut pro, &json!({ "project": { "net_colors": {}, "layer_presets": [], "viewports": [] } }));
        assert_eq!(pro, before);
    }

    #[test]
    fn class_colors_are_read_by_name() {
        assert_eq!(class_colors(&sample()).get("usb").map(String::as_str), Some("rgb(9, 9, 9)"));
        assert!(class_colors(&json!({})).is_empty());
    }
}
