//! A dimension's label has the looks of its own text (UI-LEFT-2 PP11): `PCB_DIMENSION_BASE` is a `PCB_TEXT`, and `PCB_IO_KICAD_SEXPR::format( const
//! PCB_DIMENSION_BASE* )` writes its `(gr_text ..)` through `format( const PCB_TEXT* )` and `EDA_TEXT::Format`: `(effects (font (face ..) (size H W) (thickness ..)
//! bold italic) (justify left|right mirror))`, which `parseDIMENSION`'s `T_gr_text` reads back (`parsePCB_TEXT`). The reader kept only the height and the pen, and the
//! writer wrote a square, centred label, mirrored on a back layer. Now the width, face, bold, italic, horizontal justification and mirror are
//! `Dimension::text_effects`, read and written; a mirror that is the layer side's own is kept as none (a board read and written again says the same).
//! The slow test has kicad-cli read our file and write it again (`pcb upgrade --force`): its rewrite reads back to the same looks.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Dimension, DimensionKind, DimensionTextEffects, TextJustify};
use eda_model::units::{mm, Nm};
use std::path::PathBuf;
use std::process::Command;

const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (37 "F.SilkS" user) (36 "B.SilkS" user) (35 "F.Fab" user) (34 "B.Fab" user) (41 "Cmts.User" user "User.Comments"))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(gr_rect (start 0 0) (end 60 40) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
	(dimension (type aligned) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111111")
		(pts (xy 10 10) (xy 30 10)) (height -3)
		(format (prefix "") (suffix "") (units 2) (units_format 0) (precision 2) (suppress_zeroes yes))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "20" (at 20 5.85 0) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111112")
			(effects (font (face "DejaVu Sans") (size 1.5 2) (thickness 0.3) bold italic) (justify left mirror)))
	)
	(dimension (type orthogonal) (layer "B.Fab") (uuid "22222222-2222-2222-2222-222222222222")
		(pts (xy 10 20) (xy 30 25)) (height 4) (orientation 0)
		(format (prefix "") (suffix "") (units 2) (units_format 0) (precision 2) (suppress_zeroes yes))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "20" (at 20 22.85 0) (layer "B.Fab") (uuid "22222222-2222-2222-2222-222222222223") (effects (font (size 1 1) (thickness 0.15)) (justify right mirror)))
	)
	(dimension (type aligned) (layer "B.SilkS") (uuid "33333333-3333-3333-3333-333333333333")
		(pts (xy 10 30) (xy 30 30)) (height 3)
		(format (prefix "") (suffix "") (units 2) (units_format 0) (precision 2) (suppress_zeroes yes))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "20" (at 20 34.15 0) (layer "B.SilkS") (uuid "33333333-3333-3333-3333-333333333334") (effects (font (size 1 1) (thickness 0.15))))
	)
)
"#;

fn export(d: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "dimension_label" }).expect("export_kicad_pcb")
}

/// The dimensions' looks by layer: what this test is about.
fn looks(d: &eda_model::ir::Design) -> Vec<(String, Nm, Option<Nm>, DimensionTextEffects, bool)> {
    let mut v: Vec<_> = d.drawings.as_ref().map(|dr| dr.dimensions.clone()).unwrap_or_default().into_iter().map(|d: Dimension| (d.layer.clone(), d.text_size_nm, d.text_thickness_nm, d.text_effects.clone(), d.text_mirrored())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn expected() -> Vec<(String, Nm, Option<Nm>, DimensionTextEffects, bool)> {
    vec![
        // a back layer's own mirror: kept as the layer's (none), right justified
        ("B.Fab".into(), mm(1.0), Some(mm(0.15)), DimensionTextEffects { h_justify: TextJustify::Right, ..Default::default() }, true),
        // a back-layer label written without a mirror is not mirrored: its own
        ("B.SilkS".into(), mm(1.0), Some(mm(0.15)), DimensionTextEffects { mirrored: Some(false), ..Default::default() }, false),
        // a front label with everything: (size 1.5 2) is 1.5 high and 2 wide
        (
            "F.Fab".into(),
            mm(1.5),
            Some(mm(0.3)),
            DimensionTextEffects { width_nm: Some(mm(2.0)), face: Some("DejaVu Sans".into()), bold: true, italic: true, mirrored: Some(true), h_justify: TextJustify::Left, keep_upright: false },
            true,
        ),
    ]
}

#[test]
fn a_dimension_labels_looks_are_read_kept_and_written() {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    assert_eq!(looks(&design), expected());
    let text = export(&design, &model);
    assert!(text.contains("(effects (font (face \"DejaVu Sans\") (size 1.5 2) (thickness 0.3) bold italic) (justify left mirror))"), "{text}");
    assert!(text.contains("(effects (font (size 1 1) (thickness 0.15)) (justify right mirror))"), "{text}");
    assert!(text.contains("(layer \"B.SilkS\") (uuid") && !text.contains("(effects (font (size 1 1) (thickness 0.15)) (justify mirror))"), "B.SilkS's label stays unmirrored: {text}");
    let (again, model2, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(looks(&again), expected());
    assert_eq!(export(&again, &model2), text, "a second round is the same text");
}

/// The text box the crossbar is cut by is the label's: twice as wide glyphs cut a wider gap, and a left-justified label's gap starts at its anchor.
#[test]
fn the_crossbar_gap_is_the_labels_own_box() {
    let (design, _, _) = import_kicad_pcb(BOARD).expect("imports");
    let mut dim = design.drawings.as_ref().unwrap().dimensions.iter().find(|d| d.layer == "B.SilkS").unwrap().clone();
    dim.text_position = eda_model::ir::DimensionTextPosition::Inline;
    let gap = |d: &Dimension| {
        let g = eda_connectivity::dimension::compute_dimension_geometry(d);
        let mut xs: Vec<Nm> = g.lines.iter().filter(|(a, b)| a.y == b.y).flat_map(|(a, b)| [a.x, b.x]).collect();
        xs.sort();
        // the crossbar's two pieces: the gap is between the second and the third x
        (xs[1], xs[2])
    };
    let square = gap(&dim);
    dim.text_effects.width_nm = Some(mm(2.0));
    let wide = gap(&dim);
    assert!(wide.1 - wide.0 > square.1 - square.0 + mm(1.0), "a wider label cuts a wider gap: {square:?} {wide:?}");
    dim.text_effects.h_justify = TextJustify::Left;
    let left = gap(&dim);
    assert!(left.0 > square.0 && left.1 > wide.1, "a left-justified label sits right of its anchor: {left:?} {wide:?}");
    assert!(matches!(dim.kind, DimensionKind::Aligned { .. }));
}

#[test]
fn kicad_cli_keeps_a_dimension_labels_looks() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let (design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let dir = std::env::temp_dir().join(format!("dimension_label_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dimension_label.kicad_pcb");
    std::fs::write(&path, export(&design, &model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (back, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    assert_eq!(looks(&back), expected(), "kicad-cli's rewrite:\n{theirs}");
    let _ = std::fs::remove_dir_all(&dir);
}
