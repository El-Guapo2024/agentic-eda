//! Schematic details (package P10) in the `.kicad_sch` the studio writes: pins that Swap Pins moved (a cached copy of the symbol with `lib_name`, as KiCad
//! writes it), a sheet pin moved along the edge of its sheet and a justified text that was mirrored.
//!
//! The pure-Rust tests are our writer. The slow ones hand the file to kicad-cli (`EDA_SLOW_TESTS=1`, and they skip when there is none): it loads every file and runs
//! its ERC, and its netlist says which pin each wire reaches -- the swapped pins are on each other's nets there too.

use eda_kicad::{export_kicad_sch, export_kicad_sch_tree, ExportMeta};
use eda_model::ir::{Design, LabelKind, LabelShape, NetLabel, Point, Provenance, SchematicSection, SheetInstance, SheetPin, SymbolInstance, Wire};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-11", title: "schematic details" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1 }
}

/// A two-input gate: A and B on the left (one grid cell apart), Y on the right.
fn gate() -> LibSymbol {
    LibSymbol {
        lib_id: "test:GATE".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-2.54, 5.08), end: SPoint::new(2.54, -5.08), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "A", "input", -7.62, 2.54, 0.0), pin("2", "B", "input", -7.62, 0.0, 0.0), pin("3", "Y", "output", 7.62, 0.0, 180.0)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2", "3"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Signal }).to_vec();
    let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("GATE".into()), package: None, footprint: Some("Foo:Bar".into()), symbol: Some("test:GATE".into()), datasheet: None, pins, body_nm: None, edge: None };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
}

fn design_with(sch: SchematicSection) -> Design {
    Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

/// The gate with a wire from each pin (where the library puts it) out to a label: `NET_A`, `NET_B`, `NET_Y`; with `swapped`, pins 1 and 2 have traded places.
fn gate_sheet(m: &ConstraintModel, swapped: bool) -> SchematicSection {
    let sym = SymbolInstance { id: "U1".into(), at: Point { x: 76_200_000, y: 76_200_000 }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:GATE".into(), unit: 1, value: "GATE".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    let mut sch = SchematicSection { symbols: vec![sym.clone()], ..Default::default() };
    let part = m.part("U1").unwrap();
    for (number, tip) in eda_engine::placed::pin_points(&sym, part, m.real_symbol_of(&sym.lib_id, part).as_ref()) {
        let left = number != "3";
        let end = Point { x: tip.x + if left { -12_700_000 } else { 12_700_000 }, y: tip.y };
        let net = format!("NET_{}", ["A", "B", "Y"][number.parse::<usize>().unwrap() - 1]);
        sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts: vec![tip, end], bus: false });
        sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
    }
    if swapped {
        sch.extras.swap_pin_places("U1", "1", "2");
    }
    sch
}

/// The `(at x y angle)` of pin `number` in the cached symbol `name` of `text`.
fn cached_pin_at(text: &str, name: &str, number: &str) -> String {
    let start = text.find(&format!("\t\t(symbol \"{name}\"")).unwrap_or_else(|| panic!("no cached symbol {name}"));
    let entry = &text[start..];
    let end = entry[3..].find("\n\t\t(symbol ").map_or(entry.len(), |i| i + 3000).min(entry.find("\n\t)\n").unwrap_or(entry.len()));
    let entry = &entry[..end];
    let at = entry.find(&format!("(number \"{number}\"")).unwrap_or_else(|| panic!("no pin {number} in {name}"));
    let pin_start = entry[..at].rfind("(pin ").unwrap();
    let from = pin_start + entry[pin_start..].find("(at ").unwrap();
    entry[from..from + entry[from..].find(')').unwrap() + 1].to_string()
}

#[test]
fn swapped_pins_are_written_as_the_cached_copy_of_the_symbol_with_a_lib_name() {
    let m = model();
    let plain = export_kicad_sch(&design_with(gate_sheet(&m, false)), &m, &META).expect("the sheet exports");
    let swapped = export_kicad_sch(&design_with(gate_sheet(&m, true)), &m, &META).expect("the sheet exports");
    assert!(!plain.contains("lib_name"), "a symbol with its library's pins needs no copy of its own");
    // the instance names its copy and its library symbol, as KiCad writes them
    assert!(swapped.contains("(symbol (lib_name \"GATE_1\") (lib_id \"test:GATE\")"), "{}", swapped.lines().filter(|l| l.contains("lib_id")).collect::<Vec<_>>().join("\n"));
    // the copy has pin 1 where pin 2 is in the library symbol, and pin 2 where pin 1 is; pin 3 stays
    let (a1, a2, a3) = (cached_pin_at(&plain, "test:GATE", "1"), cached_pin_at(&plain, "test:GATE", "2"), cached_pin_at(&plain, "test:GATE", "3"));
    assert_ne!(a1, a2);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "1"), a2);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "2"), a1);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "3"), a3);
    // the library symbol of that name is still there, unchanged, for any other instance
    assert_eq!(cached_pin_at(&swapped, "test:GATE", "1"), a1);
    // the pins of the instance are still listed by number
    assert!(swapped.contains("(pin \"1\" (uuid") && swapped.contains("(pin \"2\" (uuid"));
}

/// The sheet of `gate_sheet` plus four texts: the legacy one (left, bottom), a centred one, a right-justified one turned to run up the sheet, and a mirrored one.
fn text_sheet(m: &ConstraintModel) -> SchematicSection {
    use eda_model::ir::{SchematicText, TextJustify as H, TextVAlign as V};
    use eda_model::sch_extras::SchTextJustify as J;
    let mut sch = gate_sheet(m, false);
    let text = |id: &str, content: &str, x: i64, angle: u32| SchematicText { id: id.into(), content: content.into(), at: Point { x, y: 40_000_000 }, angle, size_nm: 1_270_000 };
    sch.texts = vec![text("txt_legacy", "legacy", 10_000_000, 0), text("txt_centre", "centred", 40_000_000, 0), text("txt_up", "runs up", 70_000_000, 90_000), text("txt_mirror", "mirrored", 100_000_000, 0)];
    sch.extras.set_text_justify("txt_centre", J::FILE_DEFAULT);
    sch.extras.set_text_justify("txt_up", J { h: H::Right, v: V::Top });
    // what `X` on a left-justified horizontal text leaves: the text ends at its anchor (`SCH_TEXT::MirrorSpinStyle`)
    sch.extras.set_text_justify("txt_mirror", J::default().mirrored_spin(false, true));
    sch
}

/// The `(text ...)` form of `content` in `file`, on one line.
fn text_form(file: &str, content: &str) -> String {
    let start = file.find(&format!("(text \"{content}\"")).unwrap_or_else(|| panic!("no text {content:?}"));
    let end = start + file[start..].find("(uuid").unwrap();
    file[start..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_justified_text_is_written_as_kicad_writes_it_and_read_back_the_same() {
    let m = model();
    let sch = text_sheet(&m);
    let file = export_kicad_sch(&design_with(sch.clone()), &m, &META).expect("the sheet exports");
    // left and bottom is said, not left to KiCad's own default (centre and centre); a centred text says nothing; angle 90 for the one that runs up
    assert!(text_form(&file, "legacy").contains("(at 10 40 0) (effects (font (size 1.27 1.27)) (justify left bottom))"), "{}", text_form(&file, "legacy"));
    assert!(text_form(&file, "centred").ends_with("(effects (font (size 1.27 1.27)))"), "{}", text_form(&file, "centred"));
    assert!(text_form(&file, "runs up").contains("(at 70 40 90) (effects (font (size 1.27 1.27)) (justify right top))"), "{}", text_form(&file, "runs up"));
    assert!(text_form(&file, "mirrored").contains("(justify right bottom)"), "{}", text_form(&file, "mirrored"));

    // and the importer gives back the same angle and justification (`parseSchText`, `parseEDA_TEXT`)
    let (design, _, _) = eda_kicad::import_kicad_sch(&file).expect("the file reads back");
    let back = design.schematic.expect("a schematic");
    let by_content = |c: &str| back.texts.iter().find(|t| t.content == c).unwrap_or_else(|| panic!("no text {c:?} after the import"));
    for (content, id) in [("legacy", "txt_legacy"), ("centred", "txt_centre"), ("runs up", "txt_up"), ("mirrored", "txt_mirror")] {
        let (before, after) = (sch.texts.iter().find(|t| t.id == id).unwrap(), by_content(content));
        assert_eq!((after.angle, back.extras.text_justify_of(&after.id)), (before.angle, sch.extras.text_justify_of(id)), "{content}");
        assert_eq!(after.at, before.at, "{content}");
    }
    // a text read from a file with no `(justify ...)` is KiCad's centre and centre, which this model keeps as an entry
    let bare = file.replace(" (justify left bottom)", "");
    let (design, _, _) = eda_kicad::import_kicad_sch(&bare).unwrap();
    let back = design.schematic.unwrap();
    let id = back.texts.iter().find(|t| t.content == "legacy").unwrap().id.clone();
    assert_eq!(back.extras.text_justify_of(&id), eda_model::sch_extras::SchTextJustify::FILE_DEFAULT);
}

#[test]
fn a_text_angle_in_a_file_is_made_upright_as_kicad_does() {
    let m = model();
    let file = export_kicad_sch(&design_with(text_sheet(&m)), &m, &META).unwrap();
    // 180 and 270 are not angles KiCad keeps (`KeepUpright`): they read as 0 and 90
    let turned = file.replace("(at 10 40 0)", "(at 10 40 180)").replace("(at 70 40 90)", "(at 70 40 270)");
    let (design, _, _) = eda_kicad::import_kicad_sch(&turned).unwrap();
    let angles: Vec<(String, u32)> = design.schematic.unwrap().texts.iter().map(|t| (t.content.clone(), t.angle)).collect();
    assert!(angles.contains(&("legacy".to_string(), 0)) && angles.contains(&("runs up".to_string(), 90_000)), "{angles:?}");
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")).filter(|p| p.exists())
}

/// The pins of `U1` on each net, as kicad-cli's netlist says: `{"NET_A": ["2"], ...}`.
fn kicad_nets(cli: &Path, file: &Path, out: &Path) -> BTreeMap<String, Vec<String>> {
    let run = Command::new(cli).args(["sch", "export", "netlist", "--format", "kicadsexpr", "-o"]).arg(out).arg(file).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli sch export netlist: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(out).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
    let quoted = |s: &str, key: &str| -> Option<String> {
        let at = s.find(key)? + key.len();
        let rest = &s[at..];
        Some(rest[..rest.find('"')?].to_string())
    };
    let mut nets = BTreeMap::new();
    for chunk in text.split("(net (code").skip(1) {
        let name = quoted(chunk, "(name \"").unwrap_or_default();
        let pins: Vec<String> = chunk.split("(node (ref \"").skip(1).filter(|n| n.starts_with("U1\"")).filter_map(|n| quoted(n, "(pin \"")).collect();
        nets.insert(name.trim_start_matches('/').to_string(), pins);
    }
    nets
}

/// `kicad-cli sch erc` on `file`: the report, which exists only when the file loaded.
fn erc(cli: &Path, file: &Path) -> serde_json::Value {
    let report = file.with_extension("erc.json");
    let out = Command::new(cli).args(["sch", "erc", "--format", "json", "--output"]).arg(&report).arg(file).output().expect("run kicad-cli");
    assert!(report.exists(), "kicad-cli did not produce an ERC report for {}: {}{}", file.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).expect("the ERC report is JSON")
}

#[test]
fn kicad_loads_the_swapped_pins_and_reaches_the_pins_where_the_wires_are() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let dir = std::env::temp_dir().join(format!("eda_sch_details_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nets_of = |name: &str, swapped: bool| {
        let text = export_kicad_sch(&design_with(gate_sheet(&m, swapped)), &m, &META).expect("the sheet exports");
        let path = dir.join(format!("{name}.kicad_sch"));
        std::fs::write(&path, text).unwrap();
        // the ERC runs on it: the file loads
        let report = erc(&cli, &path);
        assert!(report.get("sheets").is_some(), "{name}: {report}");
        kicad_nets(&cli, &path, &dir.join(format!("{name}.net")))
    };
    let pins = |nets: &BTreeMap<String, Vec<String>>, net: &str| nets.get(net).cloned().unwrap_or_default();
    // as the library has them, the wire of NET_A meets pin 1; with pins 1 and 2 swapped it meets pin 2, and NET_B meets pin 1
    let plain = nets_of("plain", false);
    assert_eq!((pins(&plain, "NET_A"), pins(&plain, "NET_B"), pins(&plain, "NET_Y")), (vec!["1".to_string()], vec!["2".to_string()], vec!["3".to_string()]), "{plain:?}");
    let swapped = nets_of("swapped", true);
    assert_eq!((pins(&swapped, "NET_A"), pins(&swapped, "NET_B"), pins(&swapped, "NET_Y")), (vec!["2".to_string()], vec!["1".to_string()], vec!["3".to_string()]), "{swapped:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One sheet that has all three of this package's file details at once: U1 with pins 1 and 2 swapped, a sheet whose pin was moved to its top edge (it was on the left), and a text that
/// was justified right and bottom by a mirror (`SCH_TEXT::MirrorSpinStyle` on a left-justified one), one that runs up the sheet and a centred one.
fn details_design(m: &ConstraintModel) -> Design {
    let mut sch = text_sheet(m);
    sch.extras.swap_pin_places("U1", "1", "2");
    sch.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: Point { x: 120_000_000, y: 100_000_000 },
        size: (30_000_000, 20_000_000),
        // moved along the border from the left edge, (120, 110) mm, to the top edge
        pins: vec![SheetPin { id: String::new(), name: "IN".into(), shape: LabelShape::Input, at: Point { x: 135_000_000, y: 100_000_000 } }],
        page: "2".into(),
    });
    let mut design = design_with(sch);
    design.sheet_contents = Some(BTreeMap::from([("sub.kicad_sch".to_string(), SchematicSection::default())]));
    design
}

#[test]
fn our_file_with_the_three_details_has_what_we_say_in_it_and_kicad_reads_it_the_same() {
    let m = model();
    let design = details_design(&m);
    let files = export_kicad_sch_tree(&design, &m, &META, "root.kicad_sch").expect("the sheets export");
    let root = &files.iter().find(|(n, _)| n == "root.kicad_sch").expect("a root file").1;
    // what we write: the swapped pins as the cached copy, the pin on the top edge as KiCad names it (side 90), the texts with their justification
    assert!(root.contains("(symbol (lib_name \"GATE_1\") (lib_id \"test:GATE\")"));
    assert!(root.contains("(pin \"IN\" input\n\t\t\t(at 135 100 90)"), "{}", root.lines().filter(|l| l.contains("(pin \"IN\"") || l.contains("(at 135 100")).collect::<Vec<_>>().join("\n"));
    assert!(text_form(root, "mirrored").contains("(justify right bottom)"));

    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped the kicad-cli half: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped the kicad-cli half: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_sch_details_all_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in &files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let file = dir.join("root.kicad_sch");

    // 1. the ERC runs on it: the file and the sheet it names load
    let report = erc(&cli, &file);
    assert!(report.get("sheets").is_some(), "{report}");
    // ... and reports the sheet pin where we put it: the sheet's file has no hierarchical label for it ("hier_label_mismatch"), which KiCad says at the pin, in
    // millimetres. (A kicad-cli 10.99 before build 5334 gave its ERC positions in hundredths of a millimetre, (1.35, 1.0) for this pin.)
    let mismatch = report["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["violations"].as_array().cloned().unwrap_or_default())
        .find(|v| v["type"] == "hier_label_mismatch")
        .unwrap_or_else(|| panic!("KiCad did not report the sheet pin: {report}"));
    let pos = &mismatch["items"][0]["pos"];
    assert_eq!((pos["x"].as_f64(), pos["y"].as_f64()), (Some(135.0), Some(100.0)), "{mismatch}");

    // 2. KiCad reads the file and writes it again (`kicad-cli sch upgrade --force`, its own writer): what it kept is what it understood
    let run = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&file).output().expect("run kicad-cli");
    assert!(run.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let back = std::fs::read_to_string(&file).unwrap();
    // the justification of each text is the one we wrote (KiCad writes `(justify ...)` for what is not centre and centre, in its own order)
    assert!(text_form(&back, "legacy").contains("(justify left bottom)"), "{}", text_form(&back, "legacy"));
    assert!(!text_form(&back, "centred").contains("justify"), "{}", text_form(&back, "centred"));
    assert!(text_form(&back, "runs up").contains("(at 70 40 90)") && text_form(&back, "runs up").contains("(justify right top)"), "{}", text_form(&back, "runs up"));
    assert!(text_form(&back, "mirrored").contains("(justify right bottom)"), "{}", text_form(&back, "mirrored"));
    // the sheet pin is where we put it, on the side KiCad gives a pin on the top edge
    assert!(back.contains("(pin \"IN\" input") && back.contains("(at 135 100 90)"), "{}", back.lines().filter(|l| l.contains("IN")).collect::<Vec<_>>().join("\n"));
    // the pins that traded places are still written as a copy of the symbol of their own, with pin 1 where pin 2 is in the library
    assert!(back.contains("(lib_name \"GATE_1\")"), "KiCad kept the symbol's own copy");
    assert_eq!(cached_pin_at(&back, "GATE_1", "1"), cached_pin_at(root, "test:GATE", "2"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A power symbol of a project's own library (`complex_hierarchy_schlib:+12V`, as KiCad's QA `complex_hierarchy` has them) is written with its units named after
/// the symbol's own name, the part of its `lib_id` after the library: `SCH_IO_KICAD_SEXPR_PARSER::parseLibSymbol` refuses a unit whose name does not start with
/// it, and kicad-cli then loads no sheet at all.
#[test]
fn a_power_symbol_of_a_project_library_names_its_units_after_its_own_name() {
    let mut m = model();
    let mut rail = eda_model::symbol::builtin("power:+12V").expect("the generic rail");
    rail.lib_id = "complex_hierarchy_schlib:+12V".into();
    m.symbols.push(rail);
    let mut sch = gate_sheet(&m, false);
    sch.power_symbols.push(eda_model::ir::PowerSymbol { id: "#PWR01".into(), lib_id: "complex_hierarchy_schlib:+12V".into(), at: Point { x: 50_800_000, y: 50_800_000 }, rot: 0, net: "+12V".into(), pin: String::new() });
    let text = export_kicad_sch(&design_with(sch), &m, &META).expect("the sheet exports");
    assert!(text.contains("(symbol \"complex_hierarchy_schlib:+12V\""), "{text}");
    assert!(text.contains("(symbol \"+12V_0_1\""), "the unit is named after the symbol: {text}");
    assert!(!text.contains("complex_hierarchy_schlib:+12V_0_1"));
}
