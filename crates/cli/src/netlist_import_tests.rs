//! File > Import > Netlist... through a board directory (UI-LEFT A1, `eda_ops::netlist_update`): the import is one undo step on both editors, what it
//! names survives the next load (the board's nets keep the netlist's names, its footprints the netlist's paths and values), the schematic is left as it was,
//! and -- slow tier -- a board a QA schematic's netlist (`kicad-cli sch export netlist`) was imported onto is that schematic's board as kicad-cli's
//! `--schematic-parity` reads it: no missing or extra footprint, no net that differs.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_ops::netlist_update::{ImportNetlist, NetlistOptions};
use eda_ops::{Cmd, Domain};
use eda_model::units::mm;
use std::path::{Path, PathBuf};

fn net_of(dir: &Path, pin: &str) -> String {
    let (_, _, model) = load(dir).unwrap();
    model.nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

fn import(dir: &Path, text: &str, name: &str, options: NetlistOptions) -> Result<String, String> {
    let mut c = ImportNetlist::new(text, name, options);
    c.at = Some(eda_model::ir::Point { x: mm(10.0), y: mm(40.0) });
    step(dir, Cmd::ImportNetlist(c), false, "test").map_err(|e| super::reasons(&e))
}

/// U1 and U2 (`2PAD`, the board `setup` makes), their pads 1 on `/SIG` and pads 2 on `GND`, by reference.
const NETLIST: &str = r#"(export (version "E")
  (components
    (comp (ref "U1") (value "10k") (footprint "2PAD") (tstamps "4c671539-9120-429d-9b52-8f805a6b0fad"))
    (comp (ref "U2") (value "22k") (footprint "2PAD") (tstamps "a5325dfa-f0ed-4395-9ec3-24765fab7875")))
  (nets
    (net (code "1") (name "/SIG") (node (ref "U1") (pin "1")) (node (ref "U2") (pin "1")))
    (net (code "2") (name "GND") (node (ref "U1") (pin "2")) (node (ref "U2") (pin "2")))))"#;

/// The import is one step: the nets and the values are the netlist's after it and after the next load, the schematic the studio showed is stored as it was
/// shown, and one undo takes all of it back.
#[test]
fn an_import_is_one_undo_step_and_survives_the_next_load() {
    let dir = scratch("import_netlist_dir");
    setup(&dir);
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U1.2")], ["GND", "VCC"]);
    let before = load(&dir).unwrap().1;
    let shown = super::derived_schematic(&before, &load(&dir).unwrap().2).unwrap();
    import(&dir, NETLIST, "t.net", NetlistOptions { by_timestamp: false, ..Default::default() }).unwrap();
    // the next load: the netlist's names, not the ones the schematic would give
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U2.1"), net_of(&dir, "U1.2")], ["/SIG", "/SIG", "GND"]);
    let (_, design, model) = load(&dir).unwrap();
    assert_eq!(model.part("U1").unwrap().value.as_deref(), Some("10k"));
    assert_eq!(model.symbol_links.get("U1").map(|l| l.path.as_str()), Some("/4c671539-9120-429d-9b52-8f805a6b0fad"));
    // the schematic is the one that was shown, untouched: its symbols keep their values
    assert_eq!(design.schematic.as_ref().map(|s| s.symbols.len()), shown.schematic.as_ref().map(|s| s.symbols.len()));
    assert_eq!(design.schematic.as_ref().map(|s| s.symbols.iter().map(|y| y.value.clone()).collect::<Vec<_>>()), shown.schematic.as_ref().map(|s| s.symbols.iter().map(|y| y.value.clone()).collect::<Vec<_>>()));
    // the derived board file says the same
    let pcb = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(pcb.contains("(net 1 \"/SIG\")") || pcb.contains("\"/SIG\""), "{pcb}");
    assert!(pcb.contains("(path \"/4c671539-9120-429d-9b52-8f805a6b0fad\")"), "the footprint is linked to the netlist's symbol");
    // a second import of the same netlist has nothing to change
    let (meta, design, model) = load(&dir).unwrap();
    let again = eda_ops::netlist_update::test_netlist(design, &model, meta.snap_nm, meta.spacing_nm, &ImportNetlist::new(NETLIST, "t.net", NetlistOptions { by_timestamp: false, ..Default::default() }));
    let actions: Vec<&str> = again.lines.iter().filter(|l| l.severity == eda_ops::netlist_update::Severity::Action && !l.text.is_empty()).map(|l| l.text.as_str()).collect();
    assert!(actions.is_empty(), "{actions:?}");
    // one undo, from the PCB, takes it back whole
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U1.2")], ["GND", "VCC"]);
    let (_, after, model) = load(&dir).unwrap();
    assert!(after.schematic.is_none() && after.nets.is_none());
    assert!(after.drawings.as_ref().is_none_or(|d| d.imported_netlist.is_none()));
    assert_eq!(model.part("U1").unwrap().value, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Under the Strict switch an import that leaves the board with more gate failures is refused with the reason, and nothing changes.
#[test]
fn a_strict_import_that_breaks_a_routed_net_is_refused() {
    let dir = scratch("import_netlist_strict");
    setup(&dir);
    // a track joins U1's and U2's pads 1 (GND): a complete net
    let (_, design, model) = load(&dir).unwrap();
    let pad = |r: &str| {
        let fp = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == r).unwrap().clone();
        eda_model::footprint::placed_pads(&model, model.part(r).unwrap(), &fp).unwrap().iter().find(|q| q.number == "1").unwrap().center
    };
    let track = Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.25), pts: vec![pad("U1"), pad("U2")] };
    step(&dir, track, false, "test").unwrap();
    // the netlist puts U2's pad 1 on another net: the track now shorts two nets
    let text = NETLIST.replace(r#"(node (ref "U1") (pin "1")) (node (ref "U2") (pin "1"))"#, r#"(node (ref "U1") (pin "1"))) (net (code "3") (name "/OTHER") (node (ref "U2") (pin "1"))"#);
    let mut c = ImportNetlist::new(&text, "t.net", NetlistOptions { by_timestamp: false, ..Default::default() });
    c.at = Some(eda_model::ir::Point { x: mm(10.0), y: mm(40.0) });
    let refused = step(&dir, Cmd::ImportNetlist(c), true, "test").map_err(|e| super::reasons(&e));
    let why = refused.expect_err("Strict refuses an import that adds gate failures");
    assert!(why.contains("refused because --strict"), "{why}");
    assert_eq!(net_of(&dir, "U2.1"), "GND", "nothing changed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The footprint library table of a board folder that names the libraries of `names` in KiCad's footprint checkout.
fn write_fp_table(dir: &Path, names: &[&str]) -> bool {
    let root = std::env::var_os("KICAD_PATH").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{}/src/kicad-libs", std::env::var("HOME").unwrap_or_default())));
    let fps = root.join("footprints");
    if !names.iter().all(|n| fps.join(format!("{n}.pretty")).is_dir()) {
        return false;
    }
    let rows: String = names.iter().map(|n| format!("  (lib (name \"{n}\")(type \"KiCad\")(uri \"{}\")(options \"\")(descr \"\"))\n", fps.join(format!("{n}.pretty")).display())).collect();
    std::fs::write(dir.join("fp-lib-table"), format!("(fp_lib_table\n  (version 7)\n{rows})\n")).unwrap();
    true
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): kicad-cli's netlist of a KiCad QA schematic (`test_global_promotion_2`, two sheets, global labels promoted
/// across them) is imported onto a board with "Delete footprints with no components in netlist" on; the board written beside that schematic is, to
/// kicad-cli's `--schematic-parity`, its board: no footprint missing or extra, and no pad whose net differs from the schematic's.
#[test]
fn a_qa_schematics_netlist_imported_is_that_schematics_board_to_kicad_cli() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let src = std::env::var("KICAD_SRC_DIR").unwrap_or_else(|_| format!("{}/src/kicad-src-8303b2ad", std::env::var("HOME").unwrap_or_default()));
    let qa = PathBuf::from(&src).join("qa/data/eeschema/netlists/test_global_promotion_2");
    if !qa.is_dir() {
        eprintln!("no KiCad QA data at {}; skipping", qa.display());
        return;
    }
    let dir = scratch("import_netlist_kicad_cli");
    setup(&dir);
    if !write_fp_table(&dir, &["Resistor_SMD", "Connector_PinHeader_2.54mm"]) {
        eprintln!("no KiCad footprint libraries (KICAD_PATH); skipping");
        return;
    }
    // the schematic and its sheets, in a folder of their own, with the board to be written beside it
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    for f in ["test_global_promotion_2.kicad_sch", "subsheet.kicad_sch", "test_global_promotion_2.kicad_pro"] {
        std::fs::copy(qa.join(f), project.join(f)).unwrap();
    }
    let net = project.join("test_global_promotion_2.net");
    let out = eda_kicad_engine::cli_command(&cli, None).args(["sch", "export", "netlist", "-o"]).arg(&net).arg(project.join("test_global_promotion_2.kicad_sch")).output().unwrap();
    assert!(out.status.success(), "kicad-cli sch export netlist: {}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&net).unwrap();

    let options = NetlistOptions { delete_extra_footprints: true, ..Default::default() };
    // the dry run says what the update does
    let (meta, design, model) = load(&dir).unwrap();
    let libs = crate::library_tables::libs(&dir, crate::library_index::Kind::Footprint);
    let filled = crate::netlist_api::filled(&ImportNetlist::new(&text, "test_global_promotion_2.net", options), &model, &*libs);
    let dry = eda_ops::netlist_update::test_netlist(design, &model, meta.snap_nm, meta.spacing_nm, &filled);
    let lines: Vec<&str> = dry.lines.iter().map(|l| l.text.as_str()).collect();
    eprintln!("Load and Test Netlist:\n{}", lines.join("\n"));
    assert_eq!(dry.errors, 0, "{lines:?}");
    assert!(lines.contains(&"Remove unused footprint U1.") && lines.contains(&"Remove unused footprint U2."), "{lines:?}");
    import(&dir, &text, "test_global_promotion_2.net", options).unwrap();

    // the board written beside the schematic, as the studio writes it for kicad-cli
    let (_, design, model) = load(&dir).unwrap();
    let parity_of = |design: &eda_model::ir::Design| -> Vec<(String, String)> {
        let pcb = eda_kicad::export_kicad_pcb(design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "test_global_promotion_2" }).unwrap();
        let pcb_path = project.join("test_global_promotion_2.kicad_pcb");
        std::fs::write(&pcb_path, pcb).unwrap();
        let report = project.join("drc.json");
        let _ = std::fs::remove_file(&report);
        let out = eda_kicad_engine::cli_command(&cli, None)
            .args(["pcb", "drc", "--schematic-parity", "--severity-all", "--format", "json", "-o"])
            .arg(&report)
            .arg(&pcb_path)
            .output()
            .unwrap();
        assert!(report.is_file(), "kicad-cli pcb drc: {} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
        json["schematic_parity"].as_array().cloned().unwrap_or_default().iter().map(|v| (v["type"].as_str().unwrap_or("").to_string(), v["description"].as_str().unwrap_or("").to_string())).collect()
    };
    // the control: without R4 the same check says R4 is missing (the parity test ran against the schematic)
    let mut without = design.clone();
    without.placement.as_mut().unwrap().footprints.retain(|f| f.id != "R4");
    let control = parity_of(&without);
    assert!(control.iter().any(|(t, d)| t == "missing_footprint" && d.contains("R4")), "{control:?}");
    let parity = parity_of(&design);
    eprintln!("kicad-cli's schematic parity: {parity:?}");
    let bad: Vec<&(String, String)> = parity.iter().filter(|(t, _)| matches!(t.as_str(), "missing_footprint" | "extra_footprint" | "net_conflict" | "duplicate_footprints")).collect();
    assert!(bad.is_empty(), "the board is the schematic's: {bad:?}");
    // and the six footprints are there
    assert_eq!(design.placement.as_ref().unwrap().footprints.len(), 6, "{:?}", design.placement.as_ref().unwrap().footprints.iter().map(|f| &f.id).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}
