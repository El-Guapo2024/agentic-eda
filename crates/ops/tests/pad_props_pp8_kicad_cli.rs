//! The Properties panel's rows UI-LEFT-2 PP8 made writeable, as the board file has them and kicad-cli reads them: a pad made an edge connector pad (`edit_board_pad`'s
//! `connector`, written `connect`), a plated pad given a backdrill from the back and a countersink on the front (`drill_props`, KiCad 10's `(backdrill ..)` and
//! `(front_post_machining ..)` of a pad), a pad renumbered (`set_pad_number`, which keeps its net) and a copper rectangle put on a net (`set_item_net`). The board
//! writer's file is read back by our reader, and (slow tier: `EDA_SLOW_TESTS=1`, skipped loudly without it) goes through kicad-cli (`pcb upgrade --force`:
//! `PCB_IO_KICAD_SEXPR_PARSER`, then `PCB_IO_KICAD_SEXPR`), whose rewrite is read back: the same values.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::fp_edit::PadEdit;
use eda_model::ir::Design;
use eda_model::units::{mm, Nm};
use eda_model::via_extra::{PadDrillProps, PostMachiningMode, PostMachiningProps, ViaDrillProps};
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
  (net 2 "B")
  (gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (gr_rect (start 20 20) (end 24 22) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000e2"))
  (footprint "Lib:J" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 10 10 0)
    (property "Reference" "J1" (at 0 -2.5 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "CONN" (at 0 2.5 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at -2 0) (size 0.6 1.6) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000014"))
    (pad "2" smd rect (at 0 0) (size 0.6 1.6) (layers "F.Cu" "F.Paste" "F.Mask") (net 2 "B") (uuid "00000000-0000-0000-0000-000000000015"))
    (pad "3" thru_hole circle (at 2 0) (size 1.2 1.2) (drill 0.6) (layers "*.Cu" "*.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000016"))
  )
)"#;

fn drill_props() -> PadDrillProps {
    PadDrillProps {
        backdrill: Some(ViaDrillProps { size: mm(0.7), start: "B.Cu".into(), end: "In2.Cu".into() }),
        front_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Countersink, size: mm(1.0), depth: Nm::ZERO, angle_ddeg: 900 }),
        ..Default::default()
    }
}

/// The design as the studio reads it again and the model it builds from it: each part's own footprint copy and the net list in force (`crates/cli/src/board.rs`).
fn reload(design: &Design, intent: &ConstraintModel) -> ConstraintModel {
    let mut model = intent.clone();
    eda_model::fp_assign::apply_footprint_assignments(design, &mut model);
    for a in &design.drawings.as_ref().unwrap().footprint_assignments {
        if let Some(def) = &a.definition {
            model.instance_footprints.insert(a.reference.clone(), def.clone());
        }
    }
    if let Some(nets) = &design.nets {
        model.nets = nets.clone();
    }
    model
}

fn edited() -> (Design, ConstraintModel) {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("import");
    let shape = design.drawings.as_ref().unwrap().shapes.iter().find(|s| s.layer() == "F.Cu").expect("the copper rectangle").id().to_string();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::EditBoardPad { part: "J1".into(), edit: PadEdit { connector: Some(true), ..PadEdit::none("1", 1) } }).expect("edge connector");
    b.apply(&Cmd::EditBoardPad { part: "J1".into(), edit: PadEdit { drill_props: Some(drill_props()), ..PadEdit::none("3", 1) } }).expect("backdrill");
    b.apply(&Cmd::SetPadNumber { pad: "J1.2".into(), number: "5".into() }).expect("renumber");
    b.apply(&Cmd::SetItemNet { ids: vec![shape], net: "A".into() }).expect("shape net");
    let design = b.into_design();
    let model = reload(&design, &model);
    (design, model)
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-07", title: "pad props pp8" }).expect("export")
}

/// What the edits set, as a read of a board has them: the pads' numbers, whether each is an edge connector, their nets, pad 3's drills, the copper shape's net.
#[derive(Debug, PartialEq)]
struct Facts {
    pads: Vec<(String, bool, String)>,
    drill: PadDrillProps,
    shape_net: Option<String>,
}

fn facts(design: &Design, model: &ConstraintModel) -> Facts {
    let dr = design.drawings.as_ref().unwrap();
    let extra = dr.footprint_extras.iter().find(|e| e.id == "J1").expect("J1's facts");
    let fp = model.footprint_of(model.part("J1").expect("J1")).expect("J1's footprint");
    let nets = design.nets.clone().unwrap_or_else(|| model.nets.clone());
    let net_of = |n: &str| nets.iter().find(|net| net.pins.iter().any(|p| *p == format!("J1.{n}"))).map(|net| net.name.clone()).unwrap_or_default();
    // (in number order: the writer writes them so)
    let mut pads: Vec<(String, bool, String)> = fp.pads.iter().zip(&extra.pads).map(|(p, i)| (p.number.clone(), i.connector, net_of(&p.number))).collect();
    pads.sort();
    let three = fp.pads.iter().position(|p| p.number == "3").expect("pad 3");
    let shape = dr.shapes.iter().find(|s| s.layer() == "F.Cu").expect("the copper rectangle").id().to_string();
    Facts { pads, drill: PadDrillProps::from_tokens(&extra.pads[three].tokens), shape_net: dr.board_items.net_of(&shape).map(str::to_string) }
}

fn expected() -> Facts {
    Facts {
        pads: vec![("1".into(), true, "A".into()), ("3".into(), false, "A".into()), ("5".into(), false, "B".into())],
        drill: drill_props(),
        shape_net: Some("A".into()),
    }
}

#[test]
fn the_panels_pad_type_number_drills_and_a_shapes_net_come_back_through_the_writer_and_the_reader() {
    let (design, model) = edited();
    let out = write(&design, &model);
    assert!(out.contains("(pad \"1\" connect rect"), "{out}");
    assert!(out.contains("(backdrill (size 0.7) (layers \"B.Cu\" \"In2.Cu\"))") && out.contains("(front_post_machining countersink (size 1) (angle 90))"), "{out}");
    assert!(out.contains("(pad \"5\" smd rect") && !out.contains("(pad \"2\""), "{out}");
    let (back, back_model, _) = import_kicad_pcb(&out).expect("read back");
    assert_eq!(facts(&back, &back_model), expected());
}

#[test]
fn kicad_cli_reads_the_panels_pad_type_number_drills_and_a_shapes_net_as_the_same_values() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let (design, model) = edited();
    let dir = std::env::temp_dir().join(format!("pad_props_pp8_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pad_props_pp8.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (back, back_model, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    assert_eq!(facts(&back, &back_model), expected(), "kicad-cli's rewrite:\n{theirs}");
    let _ = std::fs::remove_dir_all(&dir);
}
