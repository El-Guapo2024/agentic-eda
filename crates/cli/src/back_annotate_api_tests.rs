//! Update Schematic from PCB's nets through a board directory (UI-LEFT-2 PP17, `crate::back_annotate_api`): after Swap Pad Nets the dialog's verbs, sent as one batch,
//! give the schematic the board's nets -- the net list in force and the derived files agree again, and kicad-cli's `--schematic-parity` (slow tier) finds no pad that
//! disagrees -- and one undo of both editors takes them back.

use super::tests::{scratch, setup};
use super::{derived_schematic, load, save, step, undo};
use eda_ops::back_annotate::BackAnnotateOptions;
use eda_ops::{Cmd, Domain};
use std::path::{Path, PathBuf};

/// The board `setup` makes (U1 and U2 on two signal nets, `SIGA` on their pads 1 and `SIGB` on their pads 2), with the schematic the studio shows for it stored.
fn signal_board(name: &str) -> PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let text = std::fs::read_to_string(&intent).unwrap();
    std::fs::write(&intent, text.replace("2PAD", "Test:2PAD").replace("GND", "SIGA").replace("VCC", "SIGB")).unwrap();
    let (_, mut design, model) = load(&dir).unwrap();
    let derived = derived_schematic(&design, &model).unwrap();
    design.schematic = derived.schematic;
    design.sheet_contents = derived.sheet_contents;
    save(&dir, &design).unwrap();
    dir
}

/// The net of each pin, as the net list in force has it.
fn nets_of(dir: &Path, pins: &[&str]) -> Vec<String> {
    let (_, design, model) = load(dir).unwrap();
    let nets = design.nets.unwrap_or(model.nets);
    pins.iter().map(|pin| nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()).collect()
}

/// The net of each pin, as the drawing traces it.
fn drawn_of(dir: &Path, pins: &[&str]) -> Vec<String> {
    let (_, mut design, model) = load(dir).unwrap();
    let traced = super::trace_kicad_names(&mut design, &model);
    pins.iter().map(|pin| traced.iter().find(|t| t.pins.iter().any(|p| p == pin)).map(|t| t.name.clone()).unwrap_or_default()).collect()
}

fn swap(dir: &Path) {
    let cmd = Cmd::SwapPadNets { ids: vec!["U1.1".into(), "U1.2".into()], connected_pads: Some(false) };
    step(dir, cmd, false, "test").unwrap_or_else(|e| panic!("{}", super::reasons(&e)));
}

const PINS: [&str; 4] = ["U1.1", "U1.2", "U2.1", "U2.2"];

#[test]
fn after_a_swap_the_dialogs_verbs_give_the_schematic_the_boards_nets_in_one_undo_step() {
    let dir = signal_board("back_annotate_dir");
    let start_nets = nets_of(&dir, &PINS);
    let start_drawn = drawn_of(&dir, &PINS);
    assert_eq!(start_nets, start_drawn, "the board and the drawing agree to begin with");
    // Nothing to do before the swap.
    let plan = crate::back_annotate_api::plan(&dir, &BackAnnotateOptions::default()).unwrap();
    assert!(plan.lines.is_empty() && plan.cmds.is_empty(), "{:?}", plan.lines);

    swap(&dir);
    let swapped = nets_of(&dir, &PINS);
    assert_eq!([&swapped[0], &swapped[1]], [&start_nets[1], &start_nets[0]], "U1's pads trade nets: {swapped:?}");
    assert_eq!(drawn_of(&dir, &PINS), start_drawn, "the drawing is untouched");

    let plan = crate::back_annotate_api::plan(&dir, &BackAnnotateOptions::default()).unwrap();
    let texts: Vec<&str> = plan.lines.iter().map(|l| l.text.as_str()).collect();
    eprintln!("the dry run: {texts:?}");
    assert!(texts.contains(&"Would swap U1 pins 1 and 2 to match PCB, but unconstrained pin swaps are disabled in the schematic preferences."), "{texts:?}");
    assert!(!plan.cmds.is_empty());

    // The dialog's button: one batch.
    step(&dir, Cmd::Batch { cmds: plan.cmds.clone() }, false, "test").unwrap_or_else(|e| panic!("{}", super::reasons(&e)));
    let drawn = drawn_of(&dir, &PINS);
    assert_eq!([&drawn[0], &drawn[1]], [&swapped[0], &swapped[1]], "U1's pins are drawn on the nets its pads have: {drawn:?}");
    assert_eq!(nets_of(&dir, &["U1.1", "U1.2"]), [swapped[0].clone(), swapped[1].clone()], "and the net list in force says the same");

    // One undo of the schematic takes the batch back.
    undo(&dir, "test", Some(Domain::Schematic)).unwrap_or_else(|e| panic!("{}", super::reasons(&e)));
    assert_eq!(drawn_of(&dir, &PINS), start_drawn);
    assert_eq!(nets_of(&dir, &PINS), swapped, "the board's swap stays");
}

/// With "Allow unconstrained pin swaps" on, U1's pins trade places in the drawing instead (Swap Pins), and the labels stay.
#[test]
fn with_pin_swaps_allowed_the_symbols_pins_trade_places() {
    let dir = signal_board("back_annotate_pin_swap");
    let start_drawn = drawn_of(&dir, &PINS);
    swap(&dir);
    let swapped = nets_of(&dir, &PINS);
    let opts = BackAnnotateOptions { allow_pin_swaps: true, ..Default::default() };
    let plan = crate::back_annotate_api::plan(&dir, &opts).unwrap();
    let texts: Vec<&str> = plan.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["Swap U1 pins 1 and 2 to match PCB."]);
    step(&dir, Cmd::Batch { cmds: plan.cmds }, false, "test").unwrap_or_else(|e| panic!("{}", super::reasons(&e)));
    let (_, design, _) = load(&dir).unwrap();
    let swaps: usize = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).map(|s| s.extras.pin_swaps.get("U1").map_or(0, |m| m.len())).sum();
    assert_eq!(swaps, 2, "U1's two pins are swapped on its sheet");
    let drawn = drawn_of(&dir, &PINS);
    assert_eq!(drawn, swapped, "every pin is drawn on its pad's net (was {start_drawn:?})");
    assert_eq!(nets_of(&dir, &PINS), swapped);
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): kicad-cli's `--schematic-parity` reports U1's two pads after the swap and none once the schematic is updated from the
/// board (the derived `.kicad_sch` and `.kicad_pcb` agree again).
#[test]
fn kicad_cli_finds_the_board_and_the_schematic_agreeing_after_the_update() {
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
    for (name, opts) in [("back_annotate_kicad_cli_labels", BackAnnotateOptions::default()), ("back_annotate_kicad_cli_pins", BackAnnotateOptions { allow_pin_swaps: true, ..Default::default() })] {
        let dir = signal_board(name);
        let before = parity(&dir);
        assert!(before.iter().all(|(kind, _)| kind != "net_conflict"), "{name}: the board and the schematic agree to begin with: {before:?}");
        swap(&dir);
        let after_swap = parity(&dir);
        assert_eq!(after_swap.iter().filter(|(kind, _)| kind == "net_conflict").count(), 2, "{name}: one finding for each swapped pad: {after_swap:?}");
        let plan = crate::back_annotate_api::plan(&dir, &opts).unwrap();
        step(&dir, Cmd::Batch { cmds: plan.cmds }, false, "test").unwrap_or_else(|e| panic!("{}", super::reasons(&e)));
        let updated = parity(&dir);
        eprintln!("{name}: kicad-cli's schematic parity after Update Schematic from PCB: {updated:?}");
        assert!(updated.iter().all(|(kind, _)| kind != "net_conflict"), "{name}: no pad disagrees with the schematic any more: {updated:?}");
    }
}
