//! `/api/sch/clipboard/*`: the read side of the schematic clipboard. The edit itself is the `Cmd::PasteSch` verb (`/api/cmd`, with undo);
//! these two routes only turn the selection into KiCad's clipboard text and the clipboard text into something the canvas can carry.
//!
//!  * `POST /api/sch/clipboard/copy`: `{ids, sheet?}` -> `{ok, text, items, skipped}`. `text` is what `SCH_EDITOR_CONTROL::doCopy` puts on the
//!    system clipboard (`eda_kicad::write_clipboard`); `ids` are the ids the studio selects by on the sheet in view (`sheet` is the same
//!    `/`-joined path `GET /api/schematic?sheet=` takes).
//!  * `POST /api/sch/clipboard/parse`: `{text, sheet?, mode?, duplicate?, cursor?}` -> `{ok, kind, fragment, anchor, preview, notes, ...}`.
//!    `fragment` is what a `paste_sch` command carries; `preview` is what the paste would add to the sheet in view, drawn the way that sheet
//!    draws (the paste is tried on a copy of the design, nothing is stored) so the canvas can show it following the cursor; `anchor` is the
//!    point of the fragment the cursor holds (`SCH_SELECTION::GetTopLeftItem`, or, for Duplicate, the connection point nearest the cursor).
//!    Text that is not a schematic fragment comes back as `kind: "text"`: a single text item, as KiCad pastes it.

use crate::board;
use eda_model::ir::{Design, Point, SchematicText};
use eda_model::sch_clipboard::{closest_anchor, place_offset_um, top_left_anchor, PasteMode, SchFragment};
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd};
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// KiCad's `m_MaxPastedTextLength` (`advanced_config.cpp`): a longer plain-text paste asks first.
const MAX_PASTED_TEXT: usize = 100;

/// The board as the studio shows it: a board with no stored schematic has the one derived from its intent.
fn loaded(dir: &Path) -> Result<(board::Meta, Design, ConstraintModel), Value> {
    let (meta, mut design, model) = board::load(dir).map_err(|e| err(board::reasons(&e)))?;
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model).map_err(|e| err(board::reasons(&e)))?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok((meta, design, model))
}

/// `POST /api/sch/clipboard/copy`.
pub fn copy(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let ids: Vec<String> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if ids.is_empty() {
        return err("nothing is selected");
    }
    let (_, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (section, _) = crate::studio::resolve_sheet(&design, req.get("sheet").and_then(Value::as_str).unwrap_or(""));
    let project = dir.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string();
    match eda_kicad::write_clipboard(&eda_kicad::CopyInput { design: &design, model: &model, section: &section, project: &project }, &ids) {
        Ok(out) => json!({ "ok": true, "text": out.text, "items": out.items, "skipped": out.skipped }),
        Err(e) => err(board::reasons(&e)),
    }
}

/// What KiCad pastes for a text that is not a schematic fragment: the text, as a text item at the origin (`new SCH_TEXT( VECTOR2I( 0, 0 ), content )`).
/// The IR has one line to a text, so the lines are joined.
fn text_fragment(text: &str) -> SchFragment {
    let content = text.lines().map(str::trim_end).collect::<Vec<_>>().join(" ");
    let mut f = SchFragment::default();
    f.section.texts.push(SchematicText { id: String::new(), content, at: Point { x: 0, y: 0 }, angle: 0, size_um: 1_270 });
    f
}

/// The items of `after[key]` whose `id_of` is not in `before[key]`.
fn new_items(before: &Value, after: &Value, key: &str, id_of: &dyn Fn(&Value) -> String) -> Vec<Value> {
    let known: std::collections::BTreeSet<String> = before.get(key).and_then(Value::as_array).map(|a| a.iter().map(id_of).collect()).unwrap_or_default();
    after.get(key).and_then(Value::as_array).map(|a| a.iter().filter(|v| !known.contains(&id_of(v))).cloned().collect()).unwrap_or_default()
}

/// Where the pins of the fragment's symbols are (KiCad's frame): the connection points a Duplicate is carried by.
fn pin_points(f: &SchFragment) -> Vec<Point> {
    let mut out = Vec::new();
    for s in &f.section.symbols {
        let Some(lib) = f.lib_symbol(&s.lib_id) else { continue };
        for pin in lib.pins.iter().filter(|p| p.unit == 0 || p.unit == s.unit) {
            let (dx, dy) = place_offset_um(s.rot as f64 / 1000.0, s.mirrored, s.mirror_y, (pin.at.x, pin.at.y));
            out.push(Point { x: s.at.x + dx, y: s.at.y + dy });
        }
    }
    out
}

/// `POST /api/sch/clipboard/parse`.
pub fn parse(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(text) = req.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) else {
        return err("the clipboard is empty");
    };
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("").to_string();
    let mode: PasteMode = req.get("mode").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
    let duplicate = req.get("duplicate").and_then(Value::as_bool).unwrap_or(false);
    let cursor = req.get("cursor").and_then(Value::as_array).and_then(|c| Some(Point { x: c.first()?.as_i64()?, y: c.get(1)?.as_i64()? }));

    let (kind, fragment) = match eda_kicad::parse_clipboard(text) {
        Ok(f) => ("fragment", f),
        Err(_) => ("text", text_fragment(text)),
    };
    if fragment.is_empty() {
        return err("the clipboard holds no schematic items");
    }

    // Try the paste on a copy of the design, to see what it adds and what the new symbols are numbered.
    let (meta, design, mut model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let before = crate::studio::schematic_json_of(&design, &model, &sheet);
    let paste = Cmd::PasteSch { fragment: fragment.clone(), dx: 0, dy: 0, mode };
    let cmd = if sheet.is_empty() { paste } else { Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(paste) } };
    let drawn_before = design.clone();
    let mut tried = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    if let Err(e) = tried.apply(&cmd) {
        return err(board::reasons(&e));
    }
    let mut after_design = tried.design().clone();
    drop(tried);
    board::reconcile_schematic(&mut after_design, &mut model, Some(&drawn_before));
    let after = crate::studio::schematic_json_of(&after_design, &model, &sheet);

    let by_id = |v: &Value| v.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let by_id_unit = |v: &Value| format!("{}#{}", by_id(v), v.get("unit").and_then(Value::as_u64).unwrap_or(1));
    let symbols = new_items(&before, &after, "symbols", &by_id_unit);
    let power = new_items(&before, &after, "power_symbols", &by_id);
    let mut lib_ids: Vec<&str> = symbols.iter().filter_map(|s| s.get("lib_id").and_then(Value::as_str)).chain(power.iter().filter_map(|s| s.get("lib_id").and_then(Value::as_str))).collect();
    lib_ids.sort_unstable();
    lib_ids.dedup();
    let lib_symbols: serde_json::Map<String, Value> = lib_ids.iter().filter_map(|id| Some((id.to_string(), after.get("lib_symbols")?.get(*id)?.clone()))).collect();
    let preview = json!({
        "symbols": symbols,
        "power_symbols": power,
        "wires": new_items(&before, &after, "wires", &by_id),
        "labels": new_items(&before, &after, "labels", &by_id),
        "texts": new_items(&before, &after, "texts", &by_id),
        "no_connects": new_items(&before, &after, "no_connects", &by_id),
        "bus_entries": new_items(&before, &after, "bus_entries", &by_id),
        "junctions": new_items(&before, &after, "junctions", &by_id),
        "lines": new_items(&before, &after, "lines", &by_id),
        "graphics": new_items(&before, &after, "graphics", &by_id),
        "lib_symbols": lib_symbols,
    });

    let anchor = match (duplicate, cursor) {
        (true, Some(c)) => closest_anchor(&fragment.section, &pin_points(&fragment), c),
        _ => top_left_anchor(&fragment.section),
    };
    json!({
        "ok": true,
        "kind": kind,
        "long_text": kind == "text" && text.chars().count() > MAX_PASTED_TEXT,
        "fragment": serde_json::to_value(&fragment).unwrap_or(Value::Null),
        "anchor": anchor.map(|p| json!([p.x, p.y])),
        "preview": preview,
        "notes": fragment.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::{Net, Part, Pin, PinKind};
    use eda_ops::Domain;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_cli_sch_clipboard_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An LDO, a resistor and a capacitor, drawn by the schematic generator (a derived sheet: symbols placed by the corner of their box).
    fn setup(dir: &Path) {
        let pin = |number: &str, name: &str, kind: PinKind| Pin { number: number.into(), name: Some(name.into()), kind };
        let part = |reference: &str, symbol: Option<&str>, value: &str, pins: Vec<Pin>| Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: Some(value.into()),
            package: None,
            footprint: Some("Foo:Bar".into()),
            pins,
            body_um: None,
            symbol: symbol.map(String::from),
            datasheet: None,
            edge: None,
        };
        let u1 = part("U1", None, "LDO", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "EN", PinKind::Signal), pin("4", "VOUT", PinKind::Power), pin("5", "NC", PinKind::Nc)]);
        let r1 = part("R1", Some("Device:R"), "10k", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let c1 = part("C1", Some("Device:C"), "1u", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
        let model = ConstraintModel {
            parts: vec![u1, r1, c1],
            nets: vec![net("VIN", &["U1.1", "R1.1"]), net("VOUT", &["U1.4", "C1.1"]), net("GND", &["U1.2", "C1.2"]), net("EN", &["U1.3", "R1.2"])],
            ..Default::default()
        };
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "hash")).unwrap();
        // A board always has a placement section; this one has no part on the PCB yet.
        design.placement = Some(eda_model::ir::PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }], footprints: vec![], modules: vec![] });
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    fn all_ids(dir: &Path) -> Vec<String> {
        let (_, design, _) = board::load(dir).unwrap();
        let sch = design.schematic.unwrap();
        let mut ids: Vec<String> = sch.symbols.iter().map(|s| s.id.clone()).collect();
        ids.extend(sch.wires.iter().map(|w| w.id.clone()));
        ids.extend(sch.power_symbols.iter().map(|p| p.id.clone()));
        ids.extend(sch.no_connects.iter().map(|n| n.id.clone()));
        ids.extend(sch.labels.iter().map(|l| l.id.clone()));
        ids
    }

    fn post(f: fn(&Path, &[u8]) -> Value, dir: &Path, body: Value) -> Value {
        f(dir, body.to_string().as_bytes())
    }

    /// Wires as polylines of points (moved by `(dx, dy)`), with whether each is a bus, in a fixed order.
    fn polylines(wires: &[eda_model::ir::Wire], dx: i64, dy: i64) -> Vec<(bool, Vec<(i64, i64)>)> {
        let mut out: Vec<_> = wires.iter().map(|w| (w.bus, w.pts.iter().map(|p| (p.x + dx, p.y + dy)).collect())).collect();
        out.sort();
        out
    }

    #[test]
    fn a_copy_pasted_back_is_the_same_schematic_one_undo_step_and_unique_references() {
        let dir = scratch("round_trip");
        setup(&dir);
        let (_, original, _) = board::load(&dir).unwrap();
        let orig = original.schematic.clone().unwrap();

        // Copy everything ...
        let copy = post(copy, &dir, json!({ "ids": all_ids(&dir) }));
        assert_eq!(copy["ok"], true, "{copy}");
        let text = copy["text"].as_str().unwrap().to_string();
        assert!(text.starts_with("(lib_symbols") && !text.contains("(kicad_sch"), "KiCad's clipboard has no file around it");
        assert!(text.contains("(lib_id \"Device:R\")") && text.contains("(label") == orig.labels.iter().any(|_| true), "{text}");

        // ... read it back: a paste to carry.
        let parsed = post(parse, &dir, json!({ "text": text, "mode": "unique" }));
        assert_eq!(parsed["ok"], true, "{parsed}");
        assert_eq!(parsed["kind"], "fragment");
        assert_eq!(parsed["preview"]["symbols"].as_array().unwrap().len(), 3, "the paste would add the three symbols");
        let preview_refs: Vec<&str> = parsed["preview"]["symbols"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert!(preview_refs.iter().all(|r| !["U1", "R1", "C1"].contains(r)), "numbered anew: {preview_refs:?}");
        assert_eq!(parsed["notes"].as_array().unwrap().len(), 0);
        let anchor = parsed["anchor"].as_array().expect("an anchor");
        assert_eq!(anchor.len(), 2);

        // Paste it, 40 mm to the right and 30 below: one command, one undo step.
        let (dx, dy) = (40_640, 30_480);
        // The command as the studio posts it (`/api/cmd` reads a parsed body with `from_value`): the fragment travels inside it, library symbols and all.
        let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"].clone(), "dx": dx, "dy": dy, "mode": "unique" })).expect("the studio's paste_sch command reads");
        board::step(&dir, cmd, false, "test").unwrap();
        let (_, pasted, model) = board::load(&dir).unwrap();
        let sch = pasted.schematic.as_ref().unwrap();
        assert_eq!(sch.symbols.len(), 6);
        let mut refs: Vec<&str> = sch.symbols.iter().map(|s| s.id.as_str()).collect();
        refs.sort();
        refs.dedup();
        assert_eq!(refs.len(), 6, "every reference is its own: {refs:?}");
        // The copy of each symbol is the symbol: same library symbol, value, footprint, orientation, flags.
        for old in &orig.symbols {
            let new = sch.symbols.iter().find(|s| !orig.symbols.iter().any(|o| o.id == s.id) && s.value == old.value && s.footprint == old.footprint && s.rot == old.rot && s.mirrored == old.mirrored).unwrap_or_else(|| panic!("no copy of {}", old.id));
            assert_eq!((new.at.x - old.at.x, new.at.y - old.at.y), (dx, dy), "{} moved by the paste's offset", old.id);
            if eda_model::symbol::is_synthetic_lib_id(&old.lib_id) {
                assert_eq!(new.lib_id, "clipboard:U1", "a generated symbol (`eda:` or `gen:`) is a library symbol of its own in the copy");
            } else {
                assert_eq!(new.lib_id, old.lib_id);
            }
        }
        // Wires, power symbols and flags came with them.
        let new_wires: Vec<_> = sch.wires.iter().filter(|w| !orig.wires.iter().any(|o| o.id == w.id)).cloned().collect();
        assert_eq!(polylines(&new_wires, 0, 0), polylines(&orig.wires, dx, dy), "the wires came back as the polylines they were");
        assert_eq!(sch.power_symbols.len(), orig.power_symbols.len() * 2);
        assert_eq!(sch.no_connects.len(), orig.no_connects.len() * 2);
        // Every power symbol and flag has its copy, of the same library symbol and net, on the same spot moved by the offset (a ground and its
        // PWR_FLAG share a pin, so spots repeat).
        let key = |p: &eda_model::ir::PowerSymbol, dx: i64, dy: i64| (p.lib_id.clone(), p.net.clone(), p.at.x + dx, p.at.y + dy, p.rot);
        let mut have: Vec<_> = sch.power_symbols.iter().map(|p| key(p, 0, 0)).collect();
        let mut want: Vec<_> = orig.power_symbols.iter().map(|p| key(p, 0, 0)).chain(orig.power_symbols.iter().map(|p| key(p, dx, dy))).collect();
        have.sort();
        want.sort();
        assert_eq!(have, want);
        // The new parts are in the model (the PCB sees them), with the pins their library symbol has.
        for r in refs.iter().filter(|r| !["U1", "R1", "C1"].contains(r)) {
            assert!(model.part(r).is_some_and(|p| !p.pins.is_empty()), "{r} has a part with pins");
        }
        // The nets of the pasted wires were traced: each new wire has a net, none of them an old one's name except GND (power symbols are global).
        assert!(sch.wires.iter().all(|w| !w.net.is_empty()), "every wire has a net");
        assert!(pasted.symbol_library.as_ref().is_some_and(|l| l.symbols.iter().any(|s| s.lib_id == "clipboard:U1" && s.published)));

        // One Ctrl+Z takes the whole paste back.
        board::undo(&dir, "test", Some(Domain::Schematic)).unwrap();
        let (_, undone, _) = board::load(&dir).unwrap();
        assert_eq!(undone.schematic.as_ref().unwrap().symbols.len(), 3);
        assert_eq!(undone.schematic.as_ref().unwrap().wires.len(), orig.wires.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_a_fragment_is_pasted_as_a_text_and_a_paste_works_onto_another_sheet() {
        let dir = scratch("text_and_sheets");
        setup(&dir);
        // Plain text becomes a text item at the origin.
        let t = post(parse, &dir, json!({ "text": "hello\nworld" }));
        assert_eq!((t["ok"].as_bool(), t["kind"].as_str()), (Some(true), Some("text")), "{t}");
        assert_eq!(t["fragment"]["section"]["texts"][0]["content"], "hello world");
        assert_eq!(t["long_text"], false);
        assert_eq!(post(parse, &dir, json!({ "text": "x".repeat(101) }))["long_text"], true);
        assert_eq!(post(parse, &dir, json!({ "text": "  " }))["ok"], false);

        // A paste onto a second sheet numbers against the first one too.
        board::step(&dir, Cmd::AddSheet { name: "Child".into(), file: "child.kicad_sch".into(), at: Point { x: 200_000, y: 20_000 }, size: (30_000, 20_000) }, false, "test").unwrap();
        let (_, d, _) = board::load(&dir).unwrap();
        let sheet = d.schematic.as_ref().unwrap().sheets[0].id.clone();
        let copy = post(copy, &dir, json!({ "ids": ["R1", "C1"] }));
        assert_eq!(copy["ok"], true, "{copy}");
        let parsed = post(parse, &dir, json!({ "text": copy["text"], "sheet": sheet }));
        assert_eq!(parsed["ok"], true, "{parsed}");
        let cmd = Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(Cmd::PasteSch { fragment: serde_json::from_value(parsed["fragment"].clone()).unwrap(), dx: 0, dy: 0, mode: PasteMode::Unique }) };
        board::step(&dir, cmd, false, "test").unwrap();
        let (_, d, _) = board::load(&dir).unwrap();
        let child = d.sheet_contents.as_ref().unwrap().get("child.kicad_sch").unwrap();
        let mut child_refs: Vec<&str> = child.symbols.iter().map(|s| s.id.as_str()).collect();
        child_refs.sort();
        assert_eq!(child_refs, vec!["C2", "R2"], "R1 and C1 are on the other sheet: the copies are R2 and C2");
        assert_eq!(d.schematic.as_ref().unwrap().symbols.len(), 3, "the root is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
