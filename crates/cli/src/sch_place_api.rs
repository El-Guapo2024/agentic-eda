//! `POST /api/sch/place_preview`: the item a schematic placing tool carries, drawn before the click that places it.
//!
//! In KiCad the dialog of a label, a text or a directive label (`SCH_DRAWING_TOOLS::TwoClickPlace` -> `createNewLabel` / `createNewText`), or the
//! chooser of a power symbol (`PlaceSymbol` with the power filter), comes first; the new item then rides the cursor as a preview
//! (`m_view->AddToPreview( item )`) until a click places it. The studio sends the very command that click would commit (`place_label`, `place_text`,
//! a directive label's `add_graphic`, `add_power_symbol`, and Import Sheet's `import_sheet` of a file's contents), at a point of its choosing: `{ "sheet": "<id>/<id>", "cmd": {...} }`. The command is tried on
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
    // Import Sheet's contents in the hand (`placeSheetContents` starts a move of what it read; UI-LEFT S17): the file is read into the command, as the commit reads it
    let cmd = match crate::sheet_import::with_loaded_file(&cmd, dir) {
        Ok(read) => read.unwrap_or(cmd),
        Err(e) => return json!({ "ok": false, "message": e.first().and_then(|c| c.hint.clone()).unwrap_or_else(|| "the file could not be read".into()) }),
    };
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

    /// Import Graphics' interactive placement on the sheet (`SCH_DRAWING_TOOLS::ImportGraphics`: the imported items are a preview until the click): the command at the
    /// preview point is drawn as the commit would add it -- the lines and the text, with the text's glyph width -- and nothing is stored.
    #[test]
    fn import_graphics_is_previewed_as_the_click_would_add_it_and_nothing_is_stored() {
        let dir = board("import_gfx");
        let before = std::fs::read(dir.join("design.json")).unwrap();
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n0\nTEXT\n8\n0\n10\n0\n20\n5\n40\n2.0\n1\nHi\n0\nENDSEC\n0\nEOF\n";
        let v = preview(&dir, json!({ "op": "import_sch_graphics", "format": "dxf", "text": dxf, "dxf_units": "mm", "scale": 1.0, "offset_x_nm": mm(-254.0).get(), "offset_y_nm": mm(-254.0).get(), "line_width_nm": 0 }));
        assert_eq!(v["ok"], true, "{v}");
        let lines = v["preview"]["lines"].as_array().unwrap_or_else(|| panic!("{v}"));
        assert_eq!(lines.len(), 1, "{v}");
        let texts = v["preview"]["texts"].as_array().unwrap();
        assert_eq!((texts.len(), texts[0]["content"].clone(), texts[0]["width_nm"].clone()), (1, json!("Hi"), json!(mm(1.8).get())), "{v}");
        assert_eq!(std::fs::read(dir.join("design.json")).unwrap(), before, "a preview stores nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Place Symbol (A) with the symbol in the hand (UI-LEFT S1b, `PlaceSymbol`'s `addSymbol`): the click's command -- `add_symbol` with the reference it rides
    /// with, then the turns made while it rode as the placed unit's own `sch_move` (`R5#1`) -- is previewed as the commit draws it and stores nothing; placed, it
    /// is one undo step.
    #[test]
    fn a_carried_symbol_is_previewed_turned_and_placed_in_one_undo_step() {
        let dir = board("symbol_carry");
        let before = std::fs::read(dir.join("design.json")).unwrap();
        let place = |at: Value| {
            json!({ "op": "batch", "cmds": [
                { "op": "add_symbol", "id": "R5", "lib_id": "Device:R", "at": at, "rot_millideg": 0, "value": "4k7", "footprint": "", "unit": 1 },
                { "op": "sch_move", "verb": "rotate", "ids": ["R5#1"], "ccw": true, "vertices": {} },
                { "op": "sch_move", "verb": "mirror", "ids": ["R5#1"], "vertical": true, "vertices": {} },
            ] })
        };
        let far = json!({ "x": mm(-254.0).get(), "y": mm(-254.0).get() });
        let v = preview(&dir, place(far.clone()));
        assert_eq!(v["ok"], true, "{v}");
        let symbols = v["preview"]["symbols"].as_array().unwrap_or_else(|| panic!("{v}"));
        assert_eq!(symbols.len(), 1, "only the new symbol: {v}");
        assert_eq!((symbols[0]["id"].clone(), symbols[0]["value"].clone(), symbols[0]["at"].clone()), (json!("R5"), json!("4k7"), json!([mm(-254.0).get(), mm(-254.0).get()])), "it is turned about its anchor: {v}");
        assert_ne!((symbols[0]["rot"].clone(), symbols[0]["mirror"].clone()), (json!(0.0), Value::Null), "the turn and the mirror made while it rode: {v}");
        assert!(v["preview"]["lib_symbols"]["Device:R"].is_object(), "with its drawing: {v}");
        assert_eq!(std::fs::read(dir.join("design.json")).unwrap(), before, "a preview stores nothing");

        let at = json!({ "x": mm(1.27).get() * 80, "y": mm(1.27).get() * 80 });
        crate::board::step(&dir, serde_json::from_value(place(at)).unwrap(), false, "test").unwrap();
        let (_, design, _) = crate::board::load(&dir).unwrap();
        let r5 = design.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "R5").cloned().expect("placed");
        assert_eq!((r5.at.x, r5.at.y), (mm(1.27) * 80, mm(1.27) * 80));
        assert_eq!((symbols[0]["rot"].as_f64().unwrap_or(0.0) * 1000.0) as i64, r5.rot as i64, "placed as it was previewed");
        crate::board::undo(&dir, "test", Some(eda_ops::Domain::Schematic)).unwrap();
        let (_, design, _) = crate::board::load(&dir).unwrap();
        assert!(design.schematic.as_ref().is_none_or(|s| !s.symbols.iter().any(|x| x.id == "R5")), "one undo takes the symbol and its turns back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Place Power with KiCad's libraries installed: the preview and the click both bring `power:+3V3`'s definition (`library_place`), the placed power
    /// symbol draws and exports as the library draws it, one undo takes the symbol and its definition back. The slow tier asks kicad-cli's ERC: no library
    /// finding (`lib_symbol_issues`, `lib_symbol_mismatch`) comes with it.
    #[test]
    fn an_installed_power_symbol_is_previewed_and_placed_with_its_library_definition_and_undo_takes_both_back() {
        if eda_kicad::find_symbol_library_file(&eda_kicad::default_symbol_library_root(), "power").is_none() {
            eprintln!("no KiCad power library installed (KICAD_PATH); skipping");
            return;
        }
        let dir = board("power_place");
        let place = json!({ "op": "add_power_symbol", "lib_id": "power:+3V3", "at": { "x": mm(1.27).get() * 40, "y": mm(1.27).get() * 40 }, "rot_millideg": 0, "net": "+3V3", "pin": "" });
        let v = preview(&dir, place.clone());
        assert!(v["preview"]["lib_symbols"]["power:+3V3"]["graphics"].as_array().is_some_and(|g| !g.is_empty()), "the preview draws the library's symbol: {v}");
        let erc_counts = |dir: &Path| -> std::collections::BTreeMap<String, u64> {
            let erc = crate::kicad_engine::erc(dir).expect("kicad-cli runs ERC");
            erc["counts"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect()).unwrap_or_default()
        };
        let slow = std::env::var_os("EDA_SLOW_TESTS").is_some() && eda_kicad_engine::find_cli().is_some();
        let base = if slow { erc_counts(&dir) } else { Default::default() };

        crate::board::step(&dir, serde_json::from_value(place).unwrap(), false, "test").unwrap();
        let (_, design, model) = crate::board::load(&dir).unwrap();
        let kept = design.symbol_library.as_ref().and_then(|l| l.by_lib_id("power:+3V3")).expect("the definition is kept with the schematic");
        assert!(kept.power && kept.published);
        assert!(model.symbol_of("power:+3V3").is_some_and(|s| s.power && !s.graphics.is_empty()));
        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.power_symbols.iter().map(|p| (p.lib_id.as_str(), p.net.as_str())).collect::<Vec<_>>(), [("power:+3V3", "+3V3")]);
        if slow {
            let after = erc_counts(&dir);
            eprintln!("kicad-cli ERC before placing power:+3V3 {base:?}, after {after:?}");
            for kind in ["lib_symbol_issues", "lib_symbol_mismatch", "unannotated", "duplicate_reference"] {
                assert_eq!(after.get(kind).copied().unwrap_or(0), base.get(kind).copied().unwrap_or(0), "placing the power symbol added {kind}: before {base:?}, after {after:?}");
            }
        }

        crate::board::undo(&dir, "test", Some(eda_ops::Domain::Schematic)).unwrap();
        let (_, design, _) = crate::board::load(&dir).unwrap();
        assert!(design.schematic.as_ref().is_none_or(|s| s.power_symbols.is_empty()), "the power symbol is gone");
        assert!(design.symbol_library.as_ref().is_none_or(|l| l.by_lib_id("power:+3V3").is_none()), "and its definition with it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A mirror while the symbol rides (UI-LEFT-2 SS2): the click makes the symbol in the orientation the turns left it in (`kicad-port/schPlaceTool.ts`
    /// `placeSymbolCmd`: `add_symbol` and the mirror flag, not the placed unit's own `sch_move`), so its fields are where Autoplace Fields puts them
    /// (`PlaceSymbol`'s `AutoplaceFields( screen, AUTOPLACE_AUTO )` before the commit): Autoplace Fields after it has nothing to move.
    #[test]
    fn a_symbol_mirrored_in_the_hand_is_placed_with_its_fields_autoplaced() {
        let dir = board("mirror_autoplace");
        let at = json!({ "x": mm(-254.0).get(), "y": mm(-254.0).get() });
        let add = json!({ "op": "add_symbol", "id": "R5", "lib_id": "Device:R", "at": at, "rot_millideg": 90_000, "value": "4k7", "footprint": "", "unit": 1 });
        let ridden = preview(&dir, json!({ "op": "batch", "cmds": [add.clone(), { "op": "sch_move", "verb": "mirror", "ids": ["R5#1"], "vertical": true, "vertices": {} }] }));
        // 90 degrees mirrored top to bottom is the matrix of 90 + `mirror_y`
        let placed = json!({ "op": "batch", "cmds": [add, { "op": "mirror_symbol_vertical", "id": "R5", "unit": 1 }] });
        let v = preview(&dir, placed.clone());
        let orientation = |v: &Value| (v["preview"]["symbols"][0]["rot"].clone(), v["preview"]["symbols"][0]["mirror"].clone());
        assert_eq!(orientation(&v), orientation(&ridden), "the orientation the turns leave: {v}");
        assert!(v["preview"]["symbols"][0]["fields"].as_array().is_some_and(|f| f.iter().any(|f| f["name"] == "Value" && f["visible"] == true)), "{v}");
        let mut cmds = placed["cmds"].as_array().unwrap().clone();
        cmds.push(json!({ "op": "sch_edit", "verb": "autoplace_fields", "ids": ["R5"] }));
        let again = preview(&dir, json!({ "op": "batch", "cmds": cmds }));
        assert_eq!(again["ok"], false, "{again}");
        assert!(again["message"].as_str().is_some_and(|m| m.contains("already where Autoplace Fields puts them")), "placed, the fields are where Autoplace Fields puts them: {again}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
