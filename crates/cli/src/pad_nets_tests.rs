//! Swap Pad Nets through a board directory (GAPS.md item 26, package P25): the nets of the swapped pads reach the state and the derived `.kicad_pcb`, one undo step takes
//! the swap back, and kicad-cli's `--schematic-parity` finds the board and the schematic disagreeing about them -- the report KiCad gives for its own swap, as the board
//! file after it is the same file.

use super::tests::{scratch, setup};
use super::{derived_schematic, load, save, step, undo};
use eda_ops::{Cmd, Domain};
use std::path::Path;

/// The board `setup` makes (U1 and U2, GND on their pads 1, VCC on their pads 2), with the schematic the studio shows for it stored.
fn board_with_schematic(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    setup(&dir);
    // The footprint belongs to a library (`Test:2PAD`), as every footprint of a board that went through KiCad does: a bare name is written as `eda:2PAD` and the parity test
    // would say the symbol's `2PAD` is not it.
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

fn net_of(dir: &Path, pin: &str) -> String {
    let (_, design, model) = load(dir).unwrap();
    let nets = design.nets.unwrap_or(model.nets);
    nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

fn swap(dir: &Path, ids: &[&str]) -> Result<String, String> {
    let cmd = Cmd::SwapPadNets { ids: ids.iter().map(|s| s.to_string()).collect(), connected_pads: Some(true) };
    step(dir, cmd, false, "test").map_err(|e| super::reasons(&e))
}

#[test]
fn a_swap_goes_through_the_board_directory_as_one_undo_step() {
    let dir = board_with_schematic("swap_pad_nets_dir");
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U1.2")], ["GND", "VCC"]);
    swap(&dir, &["U1.1", "U1.2"]).unwrap();
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U1.2"), net_of(&dir, "U2.1"), net_of(&dir, "U2.2")], ["VCC", "GND", "GND", "VCC"], "U1's pads exchange, U2's stay");
    // The state names the nets the pads have now.
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    let state = crate::studio::state(&dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap();
    let u1 = state["parts"].as_array().unwrap().iter().find(|p| p["ref"] == "U1").unwrap();
    let nets: Vec<(&str, &str)> = u1["pads"].as_array().unwrap().iter().map(|p| (p["num"].as_str().unwrap(), p["net"].as_str().unwrap())).collect();
    assert_eq!(nets, [("1", "VCC"), ("2", "GND")]);
    // The derived board file says the same.
    let pcb = eda_kicad::export_kicad_pcb(&load(&dir).unwrap().1, &load(&dir).unwrap().2, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    let u1_block = &pcb[pcb.find("(footprint \"Test:2PAD\"").unwrap()..];
    let pad1 = u1_block.lines().find(|l| l.contains("(pad \"1\"")).unwrap();
    assert!(pad1.contains("\"VCC\""), "{pad1}");
    // A Ctrl+Z on the PCB takes the swap back, in one step.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U1.2")], ["GND", "VCC"]);
    // Pads on one net have nothing to swap; a footprint is no pad.
    assert!(swap(&dir, &["U1.1", "U2.1"]).unwrap_err().contains("same net"));
    assert!(swap(&dir, &["U1", "U2"]).is_err());
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): the board file after a swap is the file KiCad's own swap leaves (the pads' `(net ..)` changed), and kicad-cli's DRC with
/// `--schematic-parity` reports that the board's nets differ from the schematic's -- for the swapped pads and for no other -- where it reported nothing before.
#[test]
fn kicad_cli_reports_the_swapped_pads_as_differing_from_the_schematic() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let parity = |dir: &Path| -> Vec<(String, String)> {
        let report = crate::kicad_engine::drc_with(dir, false, true).expect("kicad-cli ran");
        assert_eq!(report["schematic_parity_run"], true, "the parity test ran: {}", report["schematic_parity_error"]);
        report["schematic_parity"].as_array().unwrap().iter().map(|v| (v["type"].as_str().unwrap_or("").to_string(), v["description"].as_str().unwrap_or("").to_string())).collect()
    };
    let dir = board_with_schematic("swap_pad_nets_kicad_cli");
    let before = parity(&dir);
    assert!(before.iter().all(|(_kind, text)| !text.to_lowercase().contains("net")), "the board and the schematic agree about the nets to begin with: {before:?}");
    swap(&dir, &["U1.1", "U1.2"]).unwrap();
    let after = parity(&dir);
    eprintln!("kicad-cli's schematic parity after the swap: {after:?}");
    let new: Vec<&(String, String)> = after.iter().filter(|f| !before.contains(f)).collect();
    // One `net_conflict` for each swapped pad, each naming the net the pad has and the net the schematic gives it (the report puts them at the pads; U2's two are untouched).
    assert_eq!(new.len(), 2, "one finding for each swapped pad: {after:?}");
    assert!(new.iter().all(|(kind, _)| kind == "net_conflict"), "{new:?}");
    let mut texts: Vec<&str> = new.iter().map(|(_, t)| t.as_str()).collect();
    texts.sort();
    assert_eq!(texts, ["Pad net (GND) doesn't match net given by schematic (VCC)", "Pad net (VCC) doesn't match net given by schematic (GND)"], "the board's pad 1 is on VCC where the schematic has GND, and its pad 2 the other way round");
    // The same board after the swap is undone says what it said before.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(parity(&dir), before, "undone: the board and the schematic agree again");
}
