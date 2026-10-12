//! Footprint assignment, exchange and reference rename through a board directory (GAPS.md package P2): what the loader lays over the intent, what one undo step
//! takes back, and the files kicad-cli reads after each.

use super::tests::{scratch, setup};
use super::{derived_schematic, load, redo, save, step, undo};
use eda_ops::{Cmd, Domain};
use std::collections::BTreeSet;
use std::path::Path;

/// The board `setup` makes, with the schematic the studio shows for it stored (module sheets, as the first schematic edit stores it).
fn board_with_schematic(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    setup(&dir);
    // the footprint belongs to a library (`Test:2PAD`), as every footprint of a board that went through KiCad does: a bare name is written as `eda:2PAD` and the
    // parity test would say the symbol's `2PAD` is not it
    let intent = dir.join("intent.yaml");
    let text = std::fs::read_to_string(&intent).unwrap();
    std::fs::write(&intent, text.replace("2PAD", "Test:2PAD")).unwrap();
    let (_, mut design, model) = load(&dir).unwrap();
    let derived = derived_schematic(&design, &model).unwrap();
    design.schematic = derived.schematic;
    design.sheet_contents = derived.sheet_contents;
    save(&dir, &design).unwrap();
    dir
}

/// `cmd` addressed to the sheet that has the symbol `reference` (the root screen when it is there).
fn on_its_sheet(dir: &Path, reference: &str, cmd: Cmd) -> Cmd {
    let (_, design, _) = load(dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    if root.symbols.iter().any(|s| s.id == reference) {
        return cmd;
    }
    let sheet = root.sheets.iter().find(|sh| design.sheet_contents.as_ref().is_some_and(|c| c.get(&sh.file).is_some_and(|screen| screen.symbols.iter().any(|s| s.id == reference)))).unwrap_or_else(|| panic!("no sheet has {reference}"));
    Cmd::OnSheet { sheet: sheet.id.clone(), cmd: Box::new(cmd) }
}

fn rename(dir: &Path, from: &str, to: &str) {
    let cmd = on_its_sheet(dir, from, Cmd::RenameSymbol { id: from.into(), new_id: to.into() });
    step(dir, cmd, false, "test").unwrap_or_else(|e| panic!("rename {from} -> {to}: {}", super::reasons(&e)));
}

/// The invariant a rename must keep: every symbol has its part and every part its symbol, every pose and every pin names a part, every pad a route ends on is a
/// pad of a part. Nothing is left under a reference no symbol has.
fn assert_consistent(dir: &Path) {
    let (_, design, model) = load(dir).unwrap();
    let parts: BTreeSet<String> = model.parts.iter().map(|p| p.reference.clone()).collect();
    // a board whose schematic was never edited has none stored: the studio shows the one derived from the model, a symbol for each part
    if design.schematic.is_some() {
        let symbols: BTreeSet<String> = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter().map(|x| x.id.clone())).collect();
        assert_eq!(parts, symbols, "every part has its symbol and every symbol its part");
    }
    let poses: BTreeSet<String> = design.placement.iter().flat_map(|p| p.footprints.iter().map(|f| f.id.clone())).collect();
    assert!(poses.is_subset(&parts), "a pose with no part: {:?}", poses.difference(&parts).collect::<Vec<_>>());
    let named = |pin: &String| pin.split('.').next().is_some_and(|r| parts.contains(r));
    for net in &model.nets {
        assert!(net.pins.iter().all(named), "net {} names a part that is not there: {:?}", net.name, net.pins);
    }
    for track in design.routing.iter().flat_map(|r| r.tracks.iter()) {
        assert!(track.pins.iter().all(named), "a route ends on a pad of no part: {:?}", track.pins);
    }
}

fn pose_x(dir: &Path, reference: &str) -> Option<i64> {
    load(dir).unwrap().1.placement.unwrap().footprints.iter().find(|f| f.id == reference).map(|f| f.at.x)
}

#[test]
fn a_rename_through_the_board_directory_leaves_no_part_pose_or_route_under_the_old_reference_and_one_undo_takes_it_back_from_either_tab() {
    let dir = board_with_schematic("rename_carries_the_part");
    // a route that ends on U1's pad 1 and U2's pad 1 (GND), so there is copper to strand
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![eda_model::ir::Point { x: 4_000, y: 5_000 }, eda_model::ir::Point { x: 14_000, y: 5_000 }] }, false, "test").unwrap();
    assert_consistent(&dir);
    rename(&dir, "U1", "U5");
    assert_consistent(&dir);
    let (_, design, model) = load(&dir).unwrap();
    assert!(model.part("U1").is_none(), "no part left under U1");
    let u5 = model.part("U5").expect("the part is U5 now");
    assert_eq!(u5.footprint.as_deref(), Some("Test:2PAD"), "with the footprint it had");
    assert!(model.footprint_of(u5).is_some(), "which the model resolves");
    assert_eq!(pose_x(&dir, "U5"), Some(5_000), "where it stood");
    assert_eq!(pose_x(&dir, "U1"), None);
    assert!(model.nets.iter().any(|n| n.name == "GND" && n.pins.contains(&"U5.1".to_string())), "its pads are on their nets: {:?}", model.nets);
    assert_eq!(design.drawings.as_ref().unwrap().renamed_parts.len(), 1);

    // one Ctrl+Z, from the PCB tab, takes the schematic's half and the board's half back; Ctrl+Shift+Z puts both again
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_consistent(&dir);
    let (_, design, model) = load(&dir).unwrap();
    assert!(model.part("U1").is_some() && model.part("U5").is_none());
    assert!(design.drawings.as_ref().is_none_or(|d| d.renamed_parts.is_empty()));
    assert!(design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter()).all(|s| s.id != "U5"));
    redo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_consistent(&dir);
    assert!(load(&dir).unwrap().2.part("U5").is_some());
    // and from the schematic tab
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_consistent(&dir);
    assert!(load(&dir).unwrap().2.part("U1").is_some());
}

#[test]
fn annotate_with_reset_through_the_board_directory_keeps_every_part_with_its_symbol() {
    let dir = board_with_schematic("annotate_carries_the_parts");
    step(&dir, Cmd::AddTrack { net: "VCC".into(), layer: "F.Cu".into(), width: 200, pts: vec![eda_model::ir::Point { x: 6_000, y: 5_000 }, eda_model::ir::Point { x: 16_000, y: 5_000 }] }, false, "test").unwrap();
    rename(&dir, "U1", "U9");
    assert_consistent(&dir);
    let before: BTreeSet<Option<i64>> = ["U9", "U2"].iter().map(|r| pose_x(&dir, r)).collect();
    let options = eda_ops::AnnotateOptions { scope: Some(eda_ops::AnnotateScope::All), ..Default::default() };
    step(&dir, Cmd::Annotate { reset_existing: true, order: eda_ops::AnnotateOrder::XThenY, ids: None, options }, false, "test").unwrap();
    assert_consistent(&dir);
    let (_, design, model) = load(&dir).unwrap();
    // the footprints are on the board still, two of them, at the places they were: under whatever numbers Annotate gave the symbols
    let after: BTreeSet<Option<i64>> = model.parts.iter().map(|p| pose_x(&dir, &p.reference)).collect();
    assert_eq!(after, before, "no pose lost");
    assert_eq!(design.placement.as_ref().unwrap().footprints.len(), 2);
    // and a PCB-tab Ctrl+Z takes the whole renumbering back
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_consistent(&dir);
    assert!(load(&dir).unwrap().2.part("U9").is_some());
}

/// The types and descriptions of what kicad-cli's `--schematic-parity` reports on the board in `dir`.
fn parity(dir: &Path) -> Vec<(String, String)> {
    let report = crate::kicad_engine::drc_with(dir, false, true).expect("kicad-cli ran");
    assert_eq!(report["schematic_parity_run"], true, "the parity test ran: {}", report["schematic_parity_error"]);
    report["schematic_parity"].as_array().unwrap().iter().map(|v| (v["type"].as_str().unwrap_or("").to_string(), v["description"].as_str().unwrap_or("").to_string())).collect()
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): after a rename the derived `.kicad_pcb` and `.kicad_sch` load, and kicad-cli's DRC with `--schematic-parity` finds the
/// footprint of the renamed part under the reference the symbol has: no missing, extra or duplicate footprint, no footprint, value or net that differs.
#[test]
fn kicad_cli_finds_the_renamed_footprint_under_the_symbols_new_reference() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = board_with_schematic("rename_kicad_cli");
    let baseline = parity(&dir);
    rename(&dir, "U1", "U5");
    let after = parity(&dir);
    assert_eq!(after, baseline, "the rename adds nothing to what the parity test said before it");
    for (kind, text) in &after {
        assert!(!matches!(kind.as_str(), "missing_footprint" | "extra_footprint" | "duplicate_footprints"), "{kind}: {text}");
        assert!(!text.contains("U1") && !text.to_lowercase().contains("footprint given by symbol"), "{kind}: {text}");
    }
    // the file kicad-cli read names U5 and not U1
    let pcb = std::fs::read_to_string(dir.join(".kicad").join("board.kicad_pcb")).expect("the derived board is beside the project");
    assert!(pcb.contains("\"Reference\" \"U5\"") && !pcb.contains("\"Reference\" \"U1\""), "the derived .kicad_pcb names the renamed footprint");
    // (the symbols are on the module sheets: every sheet file beside the board)
    let sheets: String = std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_sch")).map(|p| std::fs::read_to_string(p).unwrap()).collect();
    assert!(sheets.contains("\"Reference\" \"U5\"") && !sheets.contains("\"Reference\" \"U1\""), "the derived .kicad_sch files name the renamed symbol");
}
