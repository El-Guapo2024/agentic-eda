//! The schematic clipboard against real KiCad.
//!
//! * What `write_clipboard` puts on the clipboard must load in real KiCad: the test wraps it in a file header (what
//!   `SCH_EDITOR_CONTROL::Paste` does with the instance paths of the symbols) and has kicad-cli read it and derive the nets. It needs
//!   kicad-cli, so it runs with `EDA_SLOW_TESTS=1` (and skips when there is none).
//! * What real KiCad's writer produces must read: the forms of the QA schematics, taken out of their file the way `Format( SCH_SELECTION* )`
//!   writes them (no `(kicad_sch ...)`, no `paper`, `title_block`, `version`) read to the same items the file does. Needs `KICAD_QA_DATA`
//!   (default: the copy kept beside the KiCad sources); skips when there is none.

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{import_kicad_sch, parse_clipboard, write_clipboard, CopyInput};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn model() -> ConstraintModel {
    let pin = |number: &str, name: &str, kind: PinKind| Pin { number: number.into(), name: Some(name.into()), kind };
    let part = |reference: &str, symbol: Option<&str>, value: &str, pins: Vec<Pin>| Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some(value.into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        pins,
        body_nm: None,
        symbol: symbol.map(String::from),
        datasheet: None,
        edge: None,
    };
    let u1 = part("U1", None, "LDO", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "EN", PinKind::Signal), pin("4", "VOUT", PinKind::Power), pin("5", "NC", PinKind::Nc)]);
    let r1 = part("R1", Some("Device:R"), "10k", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
    let c1 = part("C1", Some("Device:C"), "1u", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
    ConstraintModel { parts: vec![u1, r1, c1], nets: vec![net("VIN", &["U1.1", "R1.1"]), net("VOUT", &["U1.4", "C1.1"]), net("GND", &["U1.2", "C1.2"]), net("EN", &["U1.3", "R1.2"])], ..Default::default() }
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

/// The sets of pins each net of `netlist` (kicad-cli's `kicadxml` export) joins, as sorted `REF.PIN` lists -- net names are KiCad's to choose.
fn pin_groups(netlist: &str) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in netlist.lines().map(str::trim) {
        if line.starts_with("<net ") {
            current = Some(Vec::new());
        } else if line.starts_with("</net>") {
            if let Some(mut g) = current.take() {
                g.sort();
                groups.push(g);
            }
        } else if line.starts_with("<node ") {
            let attr = |name: &str| line.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
            if let Some(g) = current.as_mut() {
                g.push(format!("{}.{}", attr("ref"), attr("pin")));
            }
        }
    }
    groups.sort();
    groups
}

#[test]
fn what_a_copy_writes_loads_in_kicad_and_keeps_the_connections() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let model = model();
    let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_ref().unwrap();
    let mut ids: Vec<String> = sch.symbols.iter().map(|s| s.id.clone()).collect();
    for list in [sch.wires.iter().map(|w| w.id.clone()).collect::<Vec<_>>(), sch.labels.iter().map(|l| l.id.clone()).collect(), sch.power_symbols.iter().map(|p| p.id.clone()).collect(), sch.no_connects.iter().map(|n| n.id.clone()).collect()] {
        ids.extend(list);
    }
    let copy = write_clipboard(&CopyInput { design: &design, model: &model, section: sch, project: "demo" }, &ids).unwrap();

    // `Paste` hands every pasted symbol the instance path of the sheet it lands on; a file needs it written out.
    let root = "11111111-1111-4111-8111-111111111111";
    let text = copy.text.replace("(path \"\"", &format!("(path \"/{root}\""));
    let file = format!("(kicad_sch (version 20260326) (generator \"eeschema\") (generator_version \"10.0\") (uuid \"{root}\") (paper \"A4\")\n{text}\n)\n");
    let dir = std::env::temp_dir().join(format!("eda_sch_clipboard_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fragment.kicad_sch");
    std::fs::write(&path, file).unwrap();
    let out = dir.join("fragment.xml");
    let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(&path).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli could not read the fragment: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let groups = pin_groups(&std::fs::read_to_string(&out).unwrap());
    let want: Vec<Vec<String>> = vec![vec!["C1.1", "U1.4"], vec!["C1.2", "U1.2"], vec!["R1.1", "U1.1"], vec!["R1.2", "U1.3"], vec!["U1.5"]].into_iter().map(|g| g.into_iter().map(String::from).collect()).collect();
    assert_eq!(groups, want, "KiCad reads the same connections the schematic has");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `(uuid "...")` the file names itself by: the first one in the header line.
fn root_uuid_of(file: &str) -> String {
    let at = file.find("(uuid \"").expect("the file has a uuid") + "(uuid \"".len();
    file[at..].split('"').next().unwrap().to_string()
}

fn components(netlist: &str) -> std::collections::BTreeSet<String> {
    netlist.lines().map(str::trim).filter(|l| l.starts_with("<comp ")).filter_map(|l| l.split("ref=\"").nth(1)?.split('"').next().map(String::from)).collect()
}

#[test]
fn what_a_copy_of_sheets_writes_loads_in_kicad_beside_the_screens_it_shows() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
    let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let design = eda_engine::derive_schematic_modules(&model, &EngineOptions::new(1, "hash")).unwrap();
    let root = design.schematic.as_ref().unwrap();
    assert!(root.sheets.len() >= 2, "mcu30 is drawn as module sheets");
    let ids: Vec<String> = root.sheets.iter().map(|s| s.id.clone()).collect();
    let copy = write_clipboard(&CopyInput { design: &design, model: &model, section: root, project: "demo" }, &ids).unwrap();
    assert_eq!(copy.text.matches("(sheet").count(), root.sheets.len(), "every sheet symbol is in the text");
    assert_eq!(copy.sheet_files.len(), root.sheets.len(), "and the files they show are named");
    assert!(copy.skipped.is_empty(), "{:?}", copy.skipped);

    // The screens a paste needs travel beside the text (KiCad's supplementary clipboard); here they are the files kicad-cli finds next to the fragment.
    let meta = eda_kicad::ExportMeta { date: "2026-10-10", title: "demo" };
    let tree = eda_kicad::export_kicad_sch_tree(&design, &model, &meta, "whole.kicad_sch").unwrap();
    let uuid = root_uuid_of(&tree[0].1);
    let dir = std::env::temp_dir().join(format!("eda_sch_clipboard_sheets_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in &tree {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let text = copy.text.replace("(path \"\"", &format!("(path \"/{uuid}\""));
    let fragment = format!("(kicad_sch (version 20260326) (generator \"eeschema\") (generator_version \"10.0\") (uuid \"{uuid}\") (paper \"A3\")\n{text}\n)\n");
    std::fs::write(dir.join("fragment.kicad_sch"), fragment).unwrap();

    let netlist = |file: &str| -> String {
        let out = dir.join(format!("{file}.xml"));
        let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(dir.join(file)).output().expect("run kicad-cli");
        assert!(run.status.success() && out.exists(), "kicad-cli could not read {file}: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        std::fs::read_to_string(&out).unwrap()
    };
    let from_fragment = netlist("fragment.kicad_sch");
    let from_whole = netlist("whole.kicad_sch");

    // KiCad finds every symbol of the screens the pasted sheets show...
    let inside: std::collections::BTreeSet<String> = design.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter().map(|x| x.id.clone())).collect();
    assert_eq!(components(&from_fragment), inside, "the fragment's sheets show their screens");
    assert!(components(&from_whole).is_superset(&inside));
    // ...and the connections inside a sheet are the ones the whole schematic has (the root's wires between the sheets are not in the copy)
    let whole = pin_groups(&from_whole);
    for group in pin_groups(&from_fragment).into_iter().filter(|g| g.len() > 1) {
        assert!(whole.iter().any(|w| group.iter().all(|p| w.contains(p))), "a net of the copied sheets is not one of the schematic's: {group:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fragments of real KiCad files

fn qa_data() -> Option<PathBuf> {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("eeschema").is_dir().then_some(root)
}

/// The text of a top-level form of `text` starting at `start` (a `(`), quote- and escape-aware.
fn form_at(text: &str, start: usize) -> &str {
    let bytes = text.as_bytes();
    let (mut depth, mut i, mut in_str) = (0i32, start, false);
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'(' if !in_str => depth += 1,
            b')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return &text[start..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    &text[start..]
}

/// The forms of a `.kicad_sch` that `Format( SCH_SELECTION* )` would write for the whole sheet: all but the file's own header and trailer.
fn fragment_of_file(text: &str) -> String {
    let body_start = text.find('(').map(|i| i + 1).unwrap();
    let mut out = String::new();
    let mut i = body_start;
    while let Some(off) = text[i..].find('(') {
        let start = i + off;
        let form = form_at(text, start);
        let tag: String = form[1..].chars().take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')').collect();
        if !matches!(tag.as_str(), "version" | "generator" | "generator_version" | "uuid" | "paper" | "title_block" | "sheet_instances" | "embedded_fonts" | "bus_alias") {
            out.push_str(form);
            out.push('\n');
        }
        i = start + form.len();
    }
    out
}

#[test]
fn forms_taken_from_real_kicad_files_read_as_the_file_does() {
    let Some(data) = qa_data() else {
        eprintln!("skipped: no KiCad QA data");
        return;
    };
    let mut checked = 0;
    for name in ["erc_label_test.kicad_sch", "NoConnectOnPin.kicad_sch", "NoConnectOnLine.kicad_sch", "erc_wire_endpoints.kicad_sch", "erc_pin_not_connected_basic.kicad_sch", "api_kitchen_sink.kicad_sch", "ground_pin_test_ok.kicad_sch", "ERC_dynamic_power_symbol_test.kicad_sch"] {
        let path: &Path = &data.join("eeschema").join(name);
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let (file_design, _, _) = import_kicad_sch(&text).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let file = file_design.schematic.unwrap();
        let fragment = fragment_of_file(&text);
        let f = parse_clipboard(&fragment).unwrap_or_else(|e| panic!("{name}: {e}"));
        let s = &f.section;
        assert_eq!(
            (s.symbols.len(), s.power_symbols.len(), s.labels.len(), s.texts.len(), s.no_connects.len(), s.junctions.len(), s.bus_entries.len(), s.lines.len(), s.extras.graphics.len()),
            (file.symbols.len(), file.power_symbols.len(), file.labels.len(), file.texts.len(), file.no_connects.len(), file.junctions.len(), file.bus_entries.len(), file.lines.len(), file.extras.graphics.len()),
            "{name}: the same items as the file"
        );
        // The wires: segments that meet at a plain bend come back as one polyline, so compare them as the segments they are.
        let segments = |wires: &[eda_model::ir::Wire]| {
            let mut out: Vec<(bool, (i64, i64), (i64, i64))> = wires
                .iter()
                .flat_map(|w| w.pts.windows(2).map(|p| (w.bus, (p[0].x, p[0].y), (p[1].x, p[1].y))).collect::<Vec<_>>())
                .map(|(bus, a, b)| if a <= b { (bus, a, b) } else { (bus, b, a) })
                .collect();
            out.sort();
            out
        };
        assert_eq!(segments(&s.wires), segments(&file.wires), "{name}: the same wires");
        let mut mine: Vec<_> = s.symbols.iter().map(|x| (x.id.clone(), x.lib_id.clone(), x.at, x.rot, x.unit)).collect();
        let mut theirs: Vec<_> = file.symbols.iter().map(|x| (x.id.clone(), x.lib_id.clone(), x.at, x.rot, x.unit)).collect();
        mine.sort();
        theirs.sort();
        assert_eq!(mine, theirs, "{name}: the same symbols, in KiCad's own frame");
        for sym in &s.symbols {
            assert!(f.lib_symbol(&sym.lib_id).is_some(), "{name}: {} has its library symbol", sym.lib_id);
        }
        checked += 1;
    }
    assert!(checked >= 3, "the QA schematics were found ({checked})");
}
