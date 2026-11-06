//! A dimension's text placed by hand (UI-LEFT P13; GAPS.md item 28): `(style .. (text_position_mode 2) ..)` is `DIM_TEXT_POSITION::MANUAL`
//! (`parseDIMENSION`'s `T_text_position_mode`: `static_cast<DIM_TEXT_POSITION>( parseInt( .. ) )`), and the text's `(gr_text .. (at x y))` is where it stays --
//! `PCB_DIM_ALIGNED::updateText` and `PCB_DIM_ORTHOGONAL::updateText` move only an `OUTSIDE` or `INLINE` text. The reader read every mode but 1 as 0, so a moved
//! text came back in its default place; the writer wrote 0 or 1. The QA boards with one (`issue12831`, `api_kitchen_sink`, `issue5830`, `issue22475`, KiCad
//! 25ec2335's `issue22267`) come back with the mode and the text where the file had them.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Dimension, DimensionKind, DimensionTextBorder, DimensionTextPosition, Point};
use eda_model::units::mm;
use std::path::PathBuf;

const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (37 "F.SilkS" user) (35 "F.Fab" user) (41 "Cmts.User" user "User.Comments"))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(dimension (type aligned) (layer "Cmts.User") (uuid "11111111-1111-1111-1111-111111111111")
		(pts (xy 10 10) (xy 30 10)) (height -3)
		(format (prefix "") (suffix "") (units 2) (units_format 0) (precision 2) (suppress_zeroes yes))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 2) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "20" (at 14 4) (layer "Cmts.User") (uuid "11111111-1111-1111-1111-111111111112") (effects (font (size 1 1) (thickness 0.15))))
	)
	(dimension (type orthogonal) (layer "Cmts.User") (uuid "22222222-2222-2222-2222-222222222222")
		(pts (xy 10 20) (xy 30 25)) (height 4) (orientation 0)
		(format (prefix "") (suffix "") (units 2) (units_format 0) (precision 2) (suppress_zeroes yes))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 1) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5))
		(gr_text "20" (at 20 24) (layer "Cmts.User") (uuid "22222222-2222-2222-2222-222222222223") (effects (font (size 1 1) (thickness 0.15))))
	)
	(dimension (type leader) (layer "Cmts.User") (uuid "33333333-3333-3333-3333-333333333333")
		(pts (xy 40 10) (xy 45 5))
		(format (prefix "") (suffix "") (units 0) (units_format 0) (precision 4) (override_value "NOTE"))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (text_frame 2) (extension_offset 0.5))
		(gr_text "NOTE" (at 50 5) (layer "Cmts.User") (uuid "33333333-3333-3333-3333-333333333334") (effects (font (size 1 1) (thickness 0.15))))
	)
)
"#;

fn export(d: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "dimension_text" }).expect("export_kicad_pcb")
}

fn dims(d: &eda_model::ir::Design) -> Vec<Dimension> {
    d.drawings.as_ref().map(|dr| dr.dimensions.clone()).unwrap_or_default()
}

#[test]
fn a_text_placed_by_hand_is_read_kept_and_written_where_it_is() {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let ds = dims(&design);
    let aligned = ds.iter().find(|d| matches!(d.kind, DimensionKind::Aligned { .. })).unwrap();
    assert_eq!(aligned.text_position, DimensionTextPosition::Manual);
    assert_eq!(aligned.text_pos, Some(Point { x: mm(14.0), y: mm(4.0) }));
    assert_eq!(eda_connectivity::dimension::compute_dimension_geometry(aligned).text_at, Point { x: mm(14.0), y: mm(4.0) }, "the geometry keeps it");
    let ortho = ds.iter().find(|d| matches!(d.kind, DimensionKind::Orthogonal { .. })).unwrap();
    assert_eq!((ortho.text_position, ortho.text_pos), (DimensionTextPosition::Inline, None), "an inline text is the geometry's");
    let leader = ds.iter().find(|d| matches!(d.kind, DimensionKind::Leader)).unwrap();
    assert_eq!((leader.text_frame, leader.text_pos), (DimensionTextBorder::Circle, Some(Point { x: mm(50.0), y: mm(5.0) })));

    let text = export(&design, &model);
    assert_eq!(text.matches("(text_position_mode 2)").count(), 1, "{text}");
    assert_eq!(text.matches("(text_position_mode 1)").count(), 1, "{text}");
    assert!(text.contains("(gr_text \"20\" (at 14 4 0)"), "the manual text is written at its place: {text}");
    assert!(text.contains("(text_frame 2)"), "{text}");
    let (again, model2, _) = import_kicad_pcb(&text).expect("re-imports");
    let strip = |mut v: Vec<Dimension>| {
        for d in &mut v {
            d.id.clear();
        }
        v.sort_by(|a, b| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)));
        v
    };
    assert_eq!(strip(dims(&again)), strip(ds));
    assert_eq!(export(&again, &model2), text, "a second round is the same text");
}

#[test]
fn a_text_moved_by_hand_on_the_board_is_written_manual() {
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let dr = design.drawings.as_mut().unwrap();
    let ortho = dr.dimensions.iter_mut().find(|d| matches!(d.kind, DimensionKind::Orthogonal { .. })).unwrap();
    // What the point editor's text handle does (`SetTextPositionMode( DIM_TEXT_POSITION::MANUAL ); SetTextPos( .. )`).
    ortho.text_position = DimensionTextPosition::Manual;
    ortho.text_pos = Some(Point { x: mm(33.0), y: mm(27.5) });
    let text = export(&design, &model);
    assert_eq!(text.matches("(text_position_mode 2)").count(), 2, "{text}");
    assert!(text.contains("(gr_text \"20\" (at 33 27.5 0)"), "{text}");
}

fn qa_board(rel: &str, kicad10: bool) -> Option<String> {
    let root = if kicad10 {
        std::env::var_os("KICAD10_SRC_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("src/kicad-src-25ec2335"))).map(|r| r.join("qa/data"))
    } else {
        std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")))
    };
    let text = root.and_then(|d| std::fs::read_to_string(d.join("pcbnew").join(rel)).ok());
    if text.is_none() {
        eprintln!("skipped: {rel} needs the KiCad QA boards (KICAD_QA_DATA, KICAD_SRC_DIR or KICAD10_SRC_DIR)");
    }
    text
}

/// The `(at x y ..)` of every dimension text whose dimension says `(text_position_mode 2)`, from a board file: the dimension blocks are found by their
/// `(dimension` head, which is all this needs.
fn manual_texts(text: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for block in text.split("(dimension").skip(1) {
        let block = &block[..block.find("\n\t(").or_else(|| block.find("\n  (")).unwrap_or(block.len())];
        if !block.contains("(text_position_mode 2)") {
            continue;
        }
        let Some(at) = block.find("(gr_text").and_then(|g| block[g..].find("(at ").map(|a| g + a + 4)) else { continue };
        let nums: Vec<f64> = block[at..].split(')').next().unwrap().split_whitespace().take(2).filter_map(|n| n.parse().ok()).collect();
        if let [x, y] = nums[..] {
            out.push((x, y));
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

#[test]
fn the_qa_boards_keep_each_text_placed_by_hand() {
    let mut seen = 0;
    for (rel, k10) in [("issue12831.kicad_pcb", false), ("api_kitchen_sink.kicad_pcb", false), ("issue5830.kicad_pcb", false), ("issue22475/issue22475.kicad_pcb", false), ("issue22267.kicad_pcb", true)] {
        let Some(text) = qa_board(rel, k10) else { continue };
        seen += 1;
        let theirs = manual_texts(&text);
        assert!(!theirs.is_empty(), "{rel} has a text placed by hand");
        let (design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let manual = dims(&design).into_iter().filter(|d| d.text_position == DimensionTextPosition::Manual).count();
        assert_eq!(manual, theirs.len(), "{rel}: each is read as manual");
        let ours = manual_texts(&export(&design, &model));
        assert_eq!(ours.len(), theirs.len(), "{rel}");
        for ((x0, y0), (x1, y1)) in theirs.iter().zip(&ours) {
            assert!((x0 - x1).abs() < 1e-6 && (y0 - y1).abs() < 1e-6, "{rel}: the text at ({x0}, {y0}) came back at ({x1}, {y1})");
        }
    }
    if seen == 0 {
        eprintln!("skipped: no QA board found");
    }
}
