//! r7 H4, M5: what a text item of a `.kicad_sch` holds beyond its text, its place and its size, read and written back as `SCH_IO_KICAD_SEXPR::saveText` /
//! `saveTextBox` write it: a free text's and a text box's "exclude from simulation" (the SPICE netlister skips an excluded directive), a text's hyperlink, bold
//! and italic, the font colour of a text, a text box, a label and a directive label (`EDA_TEXT::Format`), and the hatch fills of a shape (`formatFill`).

use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-28", title: "text attrs" };

const FILE: &str = r#"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0")
	(uuid "11111111-1111-1111-1111-111111111111")
	(paper "A4")
	(lib_symbols)
	(wire (pts (xy 50 50) (xy 70 50)) (stroke (width 0) (type default)) (uuid "21111111-1111-1111-1111-111111111111"))
	(label "B0" (at 50 50 0) (effects (font (size 1.651 1.651) (color 255 0 254 1)) (justify left bottom)) (uuid "31111111-1111-1111-1111-111111111111"))
	(text "A link" (exclude_from_sim yes) (at 100 100 0) (effects (font (size 1.27 1.27) (bold yes) (italic yes) (color 10 20 30 1)) (justify left bottom) (href "https://kicad.org")) (uuid "41111111-1111-1111-1111-111111111111"))
	(text ".param EXCLUDED=1" (exclude_from_sim yes) (at 100 120 0) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "41111111-1111-1111-1111-111111111112"))
	(text ".tran 1u 10m" (exclude_from_sim no) (at 100 130 0) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "41111111-1111-1111-1111-111111111113"))
	(text_box ".param BOXED=1" (exclude_from_sim yes) (at 20 20 0) (size 30 10) (margins 0.9525 0.9525 0.9525 0.9525)
		(stroke (width 0) (type solid)) (fill (type none))
		(effects (font (size 1.27 1.27) (color 10 9 37 1)) (justify left top))
		(uuid "51111111-1111-1111-1111-111111111111"))
	(netclass_flag "" (length 2.54) (shape round) (at 60 50 0) (fields_autoplaced yes)
		(effects (font (size 1.27 1.27) (color 236 104 255 1)) (justify left bottom))
		(uuid "61111111-1111-1111-1111-111111111111")
		(property "Netclass" "HV" (at 60.6985 47.46 0) (effects (font (size 1.27 1.27) (italic yes)) (justify left))))
	(rectangle (start 150 50) (end 170 70) (stroke (width 0) (type default)) (fill (type hatch) (color 0 0 0 0)) (uuid "71111111-1111-1111-1111-111111111111"))
	(circle (center 200 60) (radius 5) (stroke (width 0) (type default)) (fill (type cross_hatch) (color 0 0 0 0)) (uuid "71111111-1111-1111-1111-111111111112"))
)
"#;

fn item<'a>(text: &'a str, head: &str) -> &'a str {
    let start = text.find(head).unwrap_or_else(|| panic!("no {head} in\n{text}"));
    let rest = &text[start..];
    let end = rest.find("\n\t)").map_or(rest.len(), |e| e + 3);
    &rest[..end]
}

#[test]
fn a_texts_exclude_from_sim_href_and_style_are_kept() {
    let (design, model, _) = import_kicad_sch(FILE).unwrap_or_else(|e| panic!("{e:?}"));
    let out = export_kicad_sch(&design, &model, &META).unwrap();
    let link = item(&out, "(text \"A link\"");
    for want in ["(exclude_from_sim yes)", "(bold yes)", "(italic yes)", "(color 10 20 30 1)", "(href \"https://kicad.org\")"] {
        assert!(link.contains(want), "{want} in\n{link}");
    }
    assert!(item(&out, "(text \".param EXCLUDED=1\"").contains("(exclude_from_sim yes)"));
    assert!(item(&out, "(text \".tran 1u 10m\"").contains("(exclude_from_sim no)"));
    let tbox = item(&out, "(text_box \".param BOXED=1\"");
    assert!(tbox.contains("(exclude_from_sim yes)") && tbox.contains("(color 10 9 37 1)"), "{tbox}");
}

#[test]
fn font_colours_and_hatch_fills_are_kept() {
    let (design, model, _) = import_kicad_sch(FILE).unwrap_or_else(|e| panic!("{e:?}"));
    let out = export_kicad_sch(&design, &model, &META).unwrap();
    let label = item(&out, "(label \"B0\"");
    assert!(label.contains("(color 255 0 254 1)"), "{label}");
    let flag = item(&out, "(netclass_flag");
    assert!(flag.contains("(color 236 104 255 1)"), "{flag}");
    assert!(item(&out, "(rectangle").contains("(fill (type hatch)"), "{out}");
    assert!(item(&out, "(circle").contains("(fill (type cross_hatch)"), "{out}");
    // and read back the same
    let (again, model2, _) = import_kicad_sch(&out).unwrap();
    assert_eq!(export_kicad_sch(&again, &model2, &META).unwrap(), out, "a second round trip writes the same file");
}
