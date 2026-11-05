//! `POST /api/sch/place_preview`: the item a schematic placing tool carries, drawn before the click that places it.
//!
//! In KiCad the dialog of a label, a text or a directive label (`SCH_DRAWING_TOOLS::TwoClickPlace` -> `createNewLabel` / `createNewText`), or the
//! chooser of a power symbol (`PlaceSymbol` with the power filter), comes first; the new item then rides the cursor as a preview
//! (`m_view->AddToPreview( item )`) until a click places it. The studio sends the very command that click would commit (`place_label`, `place_text`,
//! a directive label's `add_graphic`, `add_power_symbol`), at a point of its choosing: `{ "sheet": "<id>/<id>", "cmd": {...} }`. The command is tried on
//! a copy of the design -- with the library definition an installed power symbol brings (`crate::library_place`), as the commit has it -- and what it
//! added comes back in the shapes `GET /api/schematic` has them (`{ "ok": true, "preview": { "labels": [...], "power_symbols": [...], "lib_symbols":
//! {...}, ... } }`, the paste preview's shape, `crate::sch_clipboard_api::tried_preview`). The canvas lays that over the sheet shifted to the cursor,
//! so the item is drawn as the commit will draw it. Nothing is stored and no history is kept.

use crate::sch_clipboard_api::{loaded, tried_preview};
use eda_ops::Cmd;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn place_preview(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("").to_string();
    let cmd: Cmd = match req.get("cmd").cloned().map(serde_json::from_value) {
        Some(Ok(c)) => c,
        Some(Err(e)) => return json!({ "ok": false, "message": format!("not a command: {e}") }),
        None => return json!({ "ok": false, "message": "no command to preview" }),
    };
    let (meta, design, mut model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // the definition of an installed symbol goes in with the command, as `board::step_quiet` puts it in before the commit
    let cmd = crate::library_place::with_installed_definitions(dir, &cmd, &design, &model).unwrap_or(cmd);
    match tried_preview(&meta, design, &mut model, &sheet, cmd) {
        Ok(preview) => json!({ "ok": true, "preview": preview }),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    /// A resistor and a capacitor on two nets, drawn by the schematic generator: a board whose first schematic edit stores the derived sheet.
    fn board(tag: &str) -> std::path::PathBuf {
        use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
        let dir = std::env::temp_dir().join(format!("eda_cli_sch_place_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pin = |number: &str| Pin { number: number.into(), name: Some(number.into()), kind: PinKind::Passive };
        let part = |reference: &str, symbol: &str| Part { reference: reference.into(), mpn: None, lcsc: None, value: Some("1k".into()), package: None, footprint: Some("Foo:Bar".into()), pins: vec![pin("1"), pin("2")], body_nm: None, symbol: Some(symbol.into()), datasheet: None, edge: None };
        let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
        let model = ConstraintModel { parts: vec![part("R1", "Device:R"), part("C1", "Device:C")], nets: vec![net("A", &["R1.1", "C1.1"]), net("B", &["R1.2", "C1.2"])], ..Default::default() };
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "hash")).unwrap();
        design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        crate::board::save(&dir, &design).unwrap();
        let meta = crate::board::Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        dir
    }

    fn preview(dir: &Path, cmd: Value) -> Value {
        place_preview(dir, json!({ "sheet": "", "cmd": cmd }).to_string().as_bytes())
    }

    #[test]
    fn a_label_a_text_and_a_power_symbol_are_previewed_as_the_commit_would_add_them_and_nothing_is_stored() {
        let dir = board("kinds");
        let before = std::fs::read(dir.join("design.json")).unwrap();
        let at = json!({ "x": mm(-254.0).get(), "y": mm(-254.0).get() });

        let v = preview(&dir, json!({ "op": "sch_edit", "verb": "place_label", "net": "DATA0", "at": at, "kind": { "scope": "global", "shape": "output" }, "spin": "up" }));
        assert_eq!(v["ok"], true, "{v}");
        let labels = v["preview"]["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 1, "{v}");
        assert_eq!((labels[0]["net"].clone(), labels[0]["scope"].clone(), labels[0]["shape"].clone(), labels[0]["spin"].clone()), (json!("DATA0"), json!("global"), json!("output"), json!("up")));
        assert_eq!(labels[0]["at"], json!([mm(-254.0).get(), mm(-254.0).get()]));
        assert!(v["preview"]["symbols"].as_array().unwrap().is_empty() && v["preview"]["wires"].as_array().unwrap().is_empty(), "only the label is new: {v}");

        let v = preview(&dir, json!({ "op": "sch_edit", "verb": "place_text", "content": "Hello", "at": at, "angle": 90000, "size_nm": 1_270_000, "h": "left", "v": "center" }));
        let texts = v["preview"]["texts"].as_array().unwrap_or_else(|| panic!("{v}"));
        assert_eq!((texts.len(), texts[0]["content"].clone(), texts[0]["h"].clone()), (1, json!("Hello"), json!("left")));

        let v = preview(&dir, json!({ "op": "add_power_symbol", "lib_id": "power:GND", "at": at, "rot_millideg": 180000, "net": "GND", "pin": "" }));
        let power = v["preview"]["power_symbols"].as_array().unwrap_or_else(|| panic!("{v}"));
        assert_eq!((power.len(), power[0]["net"].clone(), power[0]["rot"].clone()), (1, json!("GND"), json!(180.0)));
        assert!(v["preview"]["lib_symbols"]["power:GND"].is_object(), "it comes with the drawing of its symbol: {v}");

        // a command the design refuses says why; one that is not a command too
        let v = preview(&dir, json!({ "op": "add_power_symbol", "lib_id": "power:GND", "at": at, "rot_millideg": 0, "net": "", "pin": "" }));
        assert_eq!(v["ok"], false, "{v}");
        assert_eq!(place_preview(&dir, b"{\"cmd\": {\"op\": \"nope\"}}")["ok"], false);

        assert_eq!(std::fs::read(dir.join("design.json")).unwrap(), before, "a preview stores nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
