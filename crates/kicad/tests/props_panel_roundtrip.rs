//! The overlays the Properties panel's new rows set (UI-LEFT P2, `task-u1-props`) come back through the board writer and reader as they were set, and kicad-cli reads
//! them as the same values: a via's tenting, covering, plugging, capping, filling, backdrill and post-machining (`ViaExtra::set_props`, the tokens of
//! `PCB_IO_KICAD_SEXPR::format( const PCB_TRACK* )`), a board text's width, vertical justification, bold, italic, knockout and automatic pen (`TextExtra`), a track's and a
//! shape's solder mask opening and margin (`TrackMask`, `ShapeStyle`) and a rectangle's corner radius. The slow test (`EDA_SLOW_TESTS=1`) has kicad-cli rewrite our
//! file (`pcb upgrade --force`: `PCB_IO_KICAD_SEXPR_PARSER` then `PCB_IO_KICAD_SEXPR`) and reads its rewrite back: the same values.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, TextVAlign};
use eda_model::units::Nm;
use eda_model::via_extra::{PostMachiningMode, PostMachiningProps, ViaDrillProps, ViaProps};

const BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (5 "F.SilkS" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (gr_rect (start 5 20) (end 9 22) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000a1"))
  (gr_text "T1" (at 20 25 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-0000000000b1") (effects (font (size 1 1) (thickness 0.15))))
  (segment (start 5 10) (end 15 10) (width 0.25) (layer "F.Cu") (net 1) (uuid "00000000-0000-0000-0000-0000000000c1"))
  (via (at 15 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1) (uuid "00000000-0000-0000-0000-0000000000d1"))
)"#;

fn props() -> ViaProps {
    ViaProps {
        front_tenting: Some(true),
        back_tenting: Some(false),
        front_covering: Some(false),
        front_plugging: Some(true),
        capping: Some(true),
        filling: Some(false),
        backdrill: Some(ViaDrillProps { size: Nm::new(450_000), start: "B.Cu".into(), end: "In2.Cu".into() }),
        front_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Countersink, size: Nm::new(900_000), depth: Nm::ZERO, angle_ddeg: 900 }),
        back_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Counterbore, size: Nm::new(1_000_000), depth: Nm::new(300_000), angle_ddeg: 0 }),
        ..Default::default()
    }
}

/// The board with every overlay set as the panel's verbs set them.
fn edited() -> (Design, eda_model::ConstraintModel) {
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("import");
    let via = design.routing.as_ref().unwrap().vias[0].id.clone();
    let track = design.routing.as_ref().unwrap().tracks[0].id.clone();
    let d = design.drawings.as_mut().expect("drawings");
    let text = d.texts[0].id.clone();
    let shape = d.shapes.iter().find(|s| s.layer() == "F.Cu").unwrap().id().to_string();
    let mut extra = eda_model::via_extra::ViaExtra { id: via, ..Default::default() };
    extra.set_props(&props());
    d.via_extras = vec![extra];
    d.text_extras = vec![eda_model::board_text::TextExtra { id: text, width_nm: Some(Nm::new(1_500_000)), valign: TextVAlign::Bottom, bold: true, italic: true, knockout: true, ..Default::default() }];
    d.track_masks = vec![eda_model::track_mask::TrackMask { id: track, margin: Some(Nm::new(50_000)) }];
    d.board_items.set_solder_mask(&shape, true, Some(Nm::new(25_000)));
    d.board_items.set_corner_radius(&shape, Nm::new(500_000));
    (design, model)
}

/// What the overlays say, for comparing two reads of a board (ids are each read's own).
fn facts(design: &Design) -> (Vec<ViaProps>, Vec<(Option<Nm>, TextVAlign, bool, bool, bool)>, Vec<Option<Nm>>, Vec<(bool, Option<Nm>, Nm)>) {
    let d = design.drawings.as_ref().unwrap();
    let vias = d.via_extras.iter().map(|e| e.props()).collect();
    let texts = d.text_extras.iter().map(|x| (x.width_nm, x.valign, x.bold, x.italic, x.knockout)).collect();
    let tracks = d.track_masks.iter().map(|m| m.margin).collect();
    let shapes = d.board_items.shape_styles.iter().map(|s| (s.solder_mask, s.solder_mask_margin, s.corner_radius)).collect();
    (vias, texts, tracks, shapes)
}

fn write(design: &Design, model: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-05", title: "props" }).expect("export")
}

#[test]
fn the_panels_overlays_come_back_through_the_writer_and_the_reader() {
    let (design, model) = edited();
    let out = write(&design, &model);
    assert!(out.contains("(backdrill (size 0.45) (layers \"B.Cu\" \"In2.Cu\"))") && out.contains("(tenting (front yes) (back no))"), "{out}");
    let (back, _, _) = import_kicad_pcb(&out).expect("read back");
    assert_eq!(facts(&back), facts(&design));
    assert_eq!(facts(&back).0, vec![props()]);
}

#[test]
fn kicad_cli_reads_the_panels_overlays_as_the_same_values() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(std::path::PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let (design, model) = edited();
    let dir = std::env::temp_dir().join(format!("props_panel_roundtrip_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("props.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let up = std::process::Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (back, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    assert_eq!(facts(&back), facts(&design), "kicad-cli's rewrite:\n{theirs}");
    let _ = std::fs::remove_dir_all(&dir);
}
