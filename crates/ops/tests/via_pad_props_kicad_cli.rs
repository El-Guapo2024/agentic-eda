//! Track & Via Properties and Pad Properties on the board (UI-LEFT P3, `task-u5-pcbfp`) as kicad-cli reads them: a via given the IPC-4761 Type IV-b preset
//! (plugged and tented both sides), a backdrill from each side, a countersink on the front, a front/inner/back padstack and its own teardrop parameters, through
//! the verbs the dialog sends (`set_via_props`, `set_teardrop_params`); and a pad given a fabrication property, a pad-to-die length and delay, a pin name and its
//! own teardrop parameters (`edit_board_pad`). The board writer's file goes through kicad-cli (`pcb upgrade --force`: `PCB_IO_KICAD_SEXPR_PARSER`, then
//! `PCB_IO_KICAD_SEXPR`) and its rewrite is read back: the same values. Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without it.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::Design;
use eda_model::padstack::PadStackMode;
use eda_model::teardrop_params::TeardropParams;
use eda_model::units::{mm, Nm};
use eda_model::via_extra::{PostMachiningMode, PostMachiningProps, ViaDrillProps, ViaPadstack, ViaProps};
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd};
use std::path::PathBuf;
use std::process::Command;

const BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (5 "F.SilkS" user) (35 "F.Fab" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:R" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 10 10 0)
    (property "Reference" "R1" (at 0 -1.5 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 1.5 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at -0.8 0) (size 0.6 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000014"))
    (pad "2" smd rect (at 0.8 0) (size 0.6 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-000000000015"))
  )
  (via (at 30 20) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1) (uuid "00000000-0000-0000-0000-0000000000d1"))
)"#;

fn via_props() -> ViaProps {
    ViaProps {
        // Type IV-b: plugged and tented both sides, nothing else (`m_IPC4761Presets`)
        front_tenting: Some(true),
        back_tenting: Some(true),
        front_covering: Some(false),
        back_covering: Some(false),
        front_plugging: Some(true),
        back_plugging: Some(true),
        filling: Some(false),
        capping: Some(false),
        // the dialog's Backdrill "Both": the front one the secondary drill from F.Cu, the back one the tertiary from B.Cu
        backdrill: Some(ViaDrillProps { size: mm(0.5), start: "F.Cu".into(), end: "In1.Cu".into() }),
        tertiary_drill: Some(ViaDrillProps { size: mm(0.55), start: "B.Cu".into(), end: "In2.Cu".into() }),
        front_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Countersink, size: mm(1.0), depth: Nm::ZERO, angle_ddeg: 900 }),
        back_post_machining: None,
        padstack: Some(ViaPadstack { mode: PadStackMode::FrontInnerBack, sizes: [("Inner".to_string(), mm(0.6)), ("B.Cu".to_string(), mm(0.7))].into() }),
    }
}

fn via_teardrops() -> TeardropParams {
    TeardropParams { enabled: true, allow_two_segments: false, best_length_ratio: 0.7, max_length: mm(1.5), best_width_ratio: 0.9, max_width: mm(1.2), filter_ratio: 0.8, ..Default::default() }
}

fn pad_teardrops() -> TeardropParams {
    TeardropParams { enabled: true, allow_two_segments: false, prefer_zone_connections: false, best_length_ratio: 0.6, ..Default::default() }
}

/// What the dialogs set, as a read of a board has them: the via's props and teardrops, pad R1.1's property, die length and delay, pin name and teardrops.
#[derive(Debug, PartialEq)]
struct Facts {
    via: ViaProps,
    via_teardrops: Option<TeardropParams>,
    pad_tokens: Vec<String>,
    pad_function: String,
    pad_teardrops: Option<TeardropParams>,
}

fn facts(design: &Design) -> Facts {
    let dr = design.drawings.as_ref().unwrap();
    let v = dr.via_extras.first().cloned().unwrap_or_default();
    let pad = &dr.footprint_extras.iter().find(|e| e.id == "R1").expect("R1's facts").pads[0];
    let mut pad_tokens: Vec<String> = pad.tokens.iter().filter(|t| t.starts_with("(property") || t.starts_with("(die_")).cloned().collect();
    pad_tokens.sort();
    Facts { via: v.props(), via_teardrops: v.teardrops(), pad_tokens, pad_function: pad.pin_function.clone(), pad_teardrops: TeardropParams::from_tokens(&pad.tokens) }
}

fn edited() -> (Design, ConstraintModel) {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("import");
    let via = design.routing.as_ref().unwrap().vias[0].id.clone();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetViaProps { id: via.clone(), props: via_props() }).expect("set_via_props");
    b.apply(&Cmd::SetTeardropParams { ids: vec![via], params: Some(via_teardrops()) }).expect("set_teardrop_params");
    let pad = eda_model::fp_edit::PadEdit {
        property: Some("pad_prop_testpoint".into()),
        die_length: Some(mm(0.25)),
        die_delay: Some(12_500_000),
        pin_function: Some("SDA".into()),
        teardrops: Some(pad_teardrops()),
        ..eda_model::fp_edit::PadEdit::none("1", 1)
    };
    b.apply(&Cmd::EditBoardPad { part: "R1".into(), edit: pad }).expect("edit_board_pad");
    (b.into_design(), model)
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "via and pad props" }).expect("export")
}

fn expected() -> Facts {
    Facts {
        via: via_props(),
        via_teardrops: Some(via_teardrops()),
        pad_tokens: vec!["(die_delay 12.5)".into(), "(die_length 0.25)".into(), "(property pad_prop_testpoint)".into()],
        pad_function: "SDA".into(),
        pad_teardrops: Some(pad_teardrops()),
    }
}

#[test]
fn the_dialogs_via_and_pad_props_come_back_through_the_writer_and_the_reader() {
    let (design, model) = edited();
    let out = write(&design, &model);
    assert!(out.contains("(plugging (front yes) (back yes))") && out.contains("(padstack (mode front_inner_back) (layer \"Inner\" (size 0.6)) (layer \"B.Cu\" (size 0.7)))"), "{out}");
    let (back, _, _) = import_kicad_pcb(&out).expect("read back");
    assert_eq!(facts(&back), expected());
}

#[test]
fn kicad_cli_reads_the_dialogs_via_and_pad_props_as_the_same_values() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let (design, model) = edited();
    let dir = std::env::temp_dir().join(format!("via_pad_props_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("via_pad_props.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (back, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    assert_eq!(facts(&back), expected(), "kicad-cli's rewrite:\n{theirs}");
    let _ = std::fs::remove_dir_all(&dir);
}
