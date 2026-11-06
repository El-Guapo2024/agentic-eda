//! Edit Text & Graphics Properties and Edit Track & Via Properties (UI-LEFT P12, `crates/ops/src/pcb_global_edit.rs`) as kicad-cli reads them: a footprint's Reference
//! set to another font, bold, a width and a height (`FieldLayout::face`, written `(font (face ..))`), its own `${REFERENCE}` text likewise and its silkscreen line
//! thicker -- in the footprint's own copy, the other footprint on the same library footprint left as it was -- and a via given "Start and end layers only" and the
//! IPC-4761 Type VII preset. The board writer's file goes through kicad-cli (`pcb upgrade --force`: `PCB_IO_KICAD_SEXPR_PARSER`, then `PCB_IO_KICAD_SEXPR`) and its
//! rewrite is read back: the same values. Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without it.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::Design;
use eda_model::padstack::UnconnectedLayers;
use eda_model::units::{mm, Nm};
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd, FootprintItems, Ipc4761Preset, TextRows};
use std::path::PathBuf;
use std::process::Command;

const FP: &str = r#"(footprint "Lib:R" (layer "F.Cu") (uuid "UUID") (at X 10 0)
    (property "Reference" "REF" (at 0 -1.5 0) (layer "F.SilkS") (uuid "UUID-r") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 1.5 0) (layer "F.Fab") (uuid "UUID-v") (effects (font (size 1 1) (thickness 0.15))))
    (fp_line (start -1 -0.7) (end 1 -0.7) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "UUID-l"))
    (fp_text user "${REFERENCE}" (at 0 0 0) (layer "F.Fab") (uuid "UUID-t") (effects (font (size 0.5 0.5) (thickness 0.08))))
    (pad "1" smd rect (at -0.8 0) (size 0.6 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "UUID-p1"))
    (pad "2" smd rect (at 0.8 0) (size 0.6 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (net 2 "B") (uuid "UUID-p2"))
  )"#;

fn board_text() -> String {
    let fp = |r: &str, x: &str, u: &str| FP.replace("REF", r).replace("X", x).replace("UUID", u);
    format!(
        r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (5 "F.SilkS" user) (35 "F.Fab" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "A")
  (net 2 "B")
  (gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  {}
  {}
  (via (at 30 20) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1) (uuid "00000000-0000-0000-0000-0000000000d1"))
)"#,
        fp("R1", "10", "00000000-0000-0000-0000-000000000011"),
        fp("R2", "20", "00000000-0000-0000-0000-000000000021")
    )
}

/// What the edits set, as a read of a board has them: R1's Reference (face, bold, size), R1's and R2's own text and silkscreen line, the via's rings and tokens.
#[derive(Debug, PartialEq)]
struct Facts {
    reference: (Option<String>, bool, (Nm, Nm)),
    texts: Vec<(Option<String>, bool, (Nm, Nm))>,
    lines: Vec<Nm>,
    via: (UnconnectedLayers, Option<bool>, Option<bool>, Option<bool>, Option<bool>),
}

fn facts(design: &Design, model: &ConstraintModel) -> Facts {
    let dr = design.drawings.as_ref().unwrap();
    let r1 = design.footprint_edit("R1").and_then(|e| e.reference.clone()).expect("R1's reference layout");
    let art = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().art;
    let texts = ["R1", "R2"].iter().map(|r| art(r).texts[0].clone()).map(|t| (t.face, t.layout.bold, (t.layout.size.0, t.layout.size.1))).collect();
    let lines = ["R1", "R2"].iter().map(|r| art(r).shapes.iter().find(|s| s.layer() == "F.SilkS").unwrap().shape.stroke_width()).collect();
    let x = dr.via_extras.first().cloned().unwrap_or_default();
    let p = x.props();
    Facts { reference: (r1.face, r1.bold, (r1.size.0, r1.size.1)), texts, lines, via: (x.unconnected_layers(), p.front_tenting, p.capping, p.filling, p.front_plugging) }
}

/// The model the studio builds from the design: the footprint each part's own copy is (`crates/cli/src/board.rs::fold_footprint_edits`).
fn with_copies(design: &Design, mut model: ConstraintModel) -> ConstraintModel {
    for a in &design.drawings.as_ref().unwrap().footprint_assignments {
        if let Some(def) = &a.definition {
            model.instance_footprints.insert(a.reference.clone(), def.clone());
        }
    }
    model
}

fn edited() -> (Design, ConstraintModel) {
    let (design, model, _) = import_kicad_pcb(&board_text()).expect("import");
    let via = design.routing.as_ref().unwrap().vias[0].id.clone();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let rows = TextRows { font: Some("DejaVu Sans".into()), bold: Some(true), width: Some(mm(1.2)), height: Some(mm(1.4)), ..Default::default() };
    let footprint = FootprintItems { field_ids: vec!["R1:Reference".into()], text_ids: vec!["R1~t0".into()], shape_ids: vec!["R1~g0".into()] };
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: Some(mm(0.3)), text_size: None, text_thickness: None, footprint, rows }).expect("text and graphics");
    b.apply(&Cmd::EditTracksAndVias { ids: vec![via], track_width: None, via_size: None, layer: None, unconnected: Some(UnconnectedLayers::StartEndOnly), protection: Some(Ipc4761Preset::Vii) }).expect("tracks and vias");
    let design = b.into_design();
    let model = with_copies(&design, model);
    (design, model)
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "global edits" }).expect("export")
}

fn expected() -> Facts {
    Facts {
        reference: (Some("DejaVu Sans".into()), true, (Nm::new(1_200_000), Nm::new(1_400_000))),
        texts: vec![(Some("DejaVu Sans".into()), true, (Nm::new(1_200_000), Nm::new(1_400_000))), (None, false, (Nm::new(500_000), Nm::new(500_000)))],
        lines: vec![Nm::new(300_000), Nm::new(120_000)],
        via: (UnconnectedLayers::StartEndOnly, Some(false), Some(true), Some(true), Some(false)),
    }
}

#[test]
fn the_global_edits_come_back_through_the_writer_and_the_reader() {
    let (design, model) = edited();
    assert_eq!(facts(&design, &model), expected());
    let out = write(&design, &model);
    assert!(out.contains("(font (face \"DejaVu Sans\") (size 1.4 1.2)"), "{out}");
    let (back, back_model, _) = import_kicad_pcb(&out).expect("read back");
    assert_eq!(facts(&back, &back_model), expected());
}

#[test]
fn kicad_cli_reads_the_global_edits_as_the_same_values() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let (design, model) = edited();
    let dir = std::env::temp_dir().join(format!("global_edits_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("global_edits.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (back, back_model, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    assert_eq!(facts(&back, &back_model), expected(), "kicad-cli's rewrite:\n{theirs}");
    let _ = std::fs::remove_dir_all(&dir);
}
