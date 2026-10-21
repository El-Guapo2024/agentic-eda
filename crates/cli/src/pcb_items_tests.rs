//! The board items of GAPS.md item 24 (package P16) through a board directory: a text box is one undo step of the board and reaches the derived `.kicad_pcb`, and (the slow tier,
//! `EDA_SLOW_TESTS=1`) kicad-cli loads what this studio writes and plots it as it plots the same board after its own rewrite of the file.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_model::ir::Point;
use eda_model::pcb_items::TextBox;
use eda_ops::{pcb_items::PcbItemCmd, Cmd, Domain};
use std::path::Path;

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

fn add(dir: &Path, t: TextBox) {
    step(dir, Cmd::PcbItem(PcbItemCmd::AddTextBox { text_box: t }), false, "test").unwrap();
}

fn boxes(dir: &Path) -> Vec<TextBox> {
    load(dir).unwrap().1.drawings.map(|d| d.board_items.text_boxes).unwrap_or_default()
}

/// A text box as the Draw Text Box tool and its dialog make one on the silkscreen: three lines of text in a box 20 mm by 8 mm, a dashed border, margins of its own.
fn notes_box() -> TextBox {
    let mut t = TextBox::new("F.SilkS", Point { x: 2_000, y: 2_000 }, Point { x: 22_000, y: 10_000 }, 1_000, 120);
    t.text = "REV A\nBuilt by the studio, wrapped to the width of its box when the line is long".into();
    t.line_style = eda_model::pcb_items::LineStyle::Dash;
    t.margin_left_um = 900;
    t.margin_right_um = 1_100;
    t
}

/// A knockout box: the box is copper-free silkscreen ink with the text cut out of it, turned a quarter, mirrored on the back.
fn knockout_box() -> TextBox {
    let mut t = TextBox::new("B.SilkS", Point { x: 2_000, y: 14_000 }, Point { x: 18_000, y: 20_000 }, 1_200, 150);
    t.text = "TOP SECRET".into();
    t.knockout = true;
    t.border = false;
    t.halign = eda_model::ir::TextJustify::Center;
    t.valign = eda_model::ir::TextVAlign::Center;
    t.bold = true;
    t
}

#[test]
fn a_text_box_is_one_undo_step_of_the_board_and_survives_the_project_file() {
    let dir = scratch("p16_text_box");
    setup(&dir);
    assert!(boxes(&dir).is_empty());
    add(&dir, notes_box());
    let shown = boxes(&dir);
    assert_eq!(shown.len(), 1);
    assert!(shown[0].id.starts_with("tbx_"));
    // The project file is design.json: a fresh load has the box with everything the dialog set.
    let design: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap();
    assert_eq!(design["drawings"]["board_items"]["text_boxes"][0]["line_style"], "dash", "{design}");
    assert_eq!((shown[0].margin_left_um, shown[0].margin_right_um), (900, 1_100));

    // Move, edit and delete are verbs of the same history.
    let id = shown[0].id.clone();
    step(&dir, Cmd::MoveItems { ids: vec![id.clone()], dx: 1_000, dy: 500 }, false, "test").unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 3_000, y: 2_500 });
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 2_000, y: 2_000 }, "one undo takes the move back");
    let mut edited = boxes(&dir)[0].clone();
    edited.text = "REV B".into();
    step(&dir, Cmd::PcbItem(PcbItemCmd::EditTextBox { id: id.clone(), text_box: edited }), false, "test").unwrap();
    assert_eq!(boxes(&dir)[0].text, "REV B");
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteTextBox { id }), false, "test").unwrap();
    assert!(boxes(&dir).is_empty());
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(boxes(&dir).len(), 1, "undo brings the deleted box back");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(boxes(&dir).is_empty(), "and the whole history of the box goes back to nothing");
}

/// kicad-cli's `pcb export svg` of `layers` from the board file `board`, the same plot whoever wrote the file.
fn plot(cli: &Path, board: &Path, layers: &str, out: &Path) -> String {
    let done = eda_kicad_engine::cli_command(cli, None)
        .args(["pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet", "--page-size-mode", "2", "--layers", layers, "-o"])
        .arg(out)
        .arg(board)
        .output()
        .expect("kicad-cli runs");
    assert!(done.status.success() && out.exists(), "kicad-cli did not plot {}: {}{}", board.display(), String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
    std::fs::read_to_string(out).unwrap()
}

/// What the plot draws, without the lines that say when and by whom.
fn body(svg: &str) -> String {
    svg.lines().filter(|l| !l.contains("<title>") && !l.contains("<!--") && !l.contains("<desc>") && !l.contains("Created by") && !l.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// The text boxes this studio writes load in kicad-cli and plot as KiCad plots them: the plot of the derived board file and the plot of kicad-cli's own rewrite of it
/// (`pcb upgrade --force`, which reads the file with `PCB_IO_KICAD_SEXPR_PARSER` and writes it with `PCB_IO_KICAD_SEXPR`) are one and the same, so the parser took the box
/// as the writer meant it -- the text wrapped to the margins, the dashed border, the knockout with its text cut out. The rewrite still has both boxes, with the margins.
#[test]
fn kicad_cli_loads_the_text_boxes_this_studio_writes_and_plots_them_as_its_own_rewrite() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_text_box_cli");
    setup(&dir);
    add(&dir, notes_box());
    add(&dir, knockout_box());
    // The studio's own plot derives the board file (`.kicad/*.kicad_pcb`).
    let args: Vec<String> = ["--layers", "F.SilkS,B.SilkS"].iter().map(|s| s.to_string()).collect();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    let text = std::fs::read_to_string(&derived).unwrap();
    assert_eq!(text.matches("(gr_text_box").count(), 2, "{text}");

    let (ours, theirs) = (dir.join("ours.svg"), dir.join("theirs.svg"));
    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(upgraded.matches("(gr_text_box").count(), 2, "kicad-cli kept both boxes: {upgraded}");
    assert!(upgraded.contains("(margins 0.9 0.81 1.1 0.81)") || upgraded.contains("(margins 0.9 0.8100 1.1 0.8100)") || upgraded.contains("(margins 0.9 0.81"), "the margins: {upgraded}");

    // KiCad plots F.SilkS: the box's border (dashed: many short strokes) and the glyphs of the wrapped text; B.SilkS: the knockout
    let (a_front, b_front) = (plot(&cli, &derived, "F.SilkS", &ours), plot(&cli, &copy, "F.SilkS", &theirs));
    assert!(a_front.len() > 2_000, "a real plot: {} bytes", a_front.len());
    assert_eq!(body(&a_front), body(&b_front), "the same plot before and after kicad-cli's rewrite of the file");
    let (a_back, b_back) = (plot(&cli, &derived, "B.SilkS", &ours), plot(&cli, &copy, "B.SilkS", &theirs));
    assert_eq!(body(&a_back), body(&b_back), "the knockout plots the same too");
    assert_ne!(body(&a_front), body(&a_back), "the two layers plot different things");

    // Take the box away and the plot is empty of it: the strokes are the box's.
    let id = boxes(&dir)[0].id.clone();
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteTextBox { id }), false, "test").unwrap();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let without = plot(&cli, &derived, "F.SilkS", &dir.join("without.svg"));
    assert!(body(&without).len() + 500 < body(&a_front).len(), "the box is most of the plot: {} against {}", body(&without).len(), body(&a_front).len());
}
