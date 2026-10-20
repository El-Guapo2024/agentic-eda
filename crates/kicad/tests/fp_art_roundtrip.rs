//! A footprint's graphics and texts through the importer and the writer ([`eda_model::fp_art`]): a board's `fp_line`, `fp_arc`, `fp_circle`,
//! `fp_rect`, `fp_poly`, `fp_curve` and `fp_text user` come back out of `export_kicad_pcb`, a library footprint placed by this studio carries its
//! silkscreen and fab drawings onto the board it makes, and a bottom-side footprint is written mirrored on the other side's layers. The first
//! group needs no external tool. The last ones (`EDA_SLOW_TESTS=1`) ask kicad-cli: the silkscreen and fab layers it plots are the same before
//! and after a round trip -- on one of KiCad's own QA boards and on a board this studio made from one of its examples.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{default_footprint_library_root, export_kicad_pcb, import_kicad_pcb, resolve_library_footprints, ExportMeta};
use eda_model::fp_art::FpArt;
use eda_model::ir::{Design, Shape};
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};

const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
"#;

fn board(items: &str) -> String {
    format!("{HEADER}{items})\n")
}

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "fp_art_rt" }).expect("export_kicad_pcb")
}

fn count_in(text: &str, token: &str) -> usize {
    text.matches(token).count()
}

/// The footprint definition the part `reference` of `model` uses.
fn art_of<'a>(model: &'a ConstraintModel, reference: &str) -> &'a FpArt {
    let part = model.part(reference).unwrap_or_else(|| panic!("no part {reference}"));
    let name = part.footprint.as_deref().unwrap_or_else(|| panic!("{reference} has no footprint"));
    &model.footprints.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no footprint {name}")).art
}

/// One footprint on each side, turned, each with a line, an arc, a circle, a rectangle, a polygon, a curve and a text.
fn two_footprints() -> String {
    let items = |layer_silk: &str, layer_fab: &str, mirrored: &str| {
        format!(
            r#"		(fp_line (start -1 -2) (end 1 -2) (stroke (width 0.12) (type solid)) (layer "{layer_silk}") (uuid "00000000-0000-0000-0000-000000000011"))
		(fp_arc (start 1 0) (mid 0 1) (end -1 0) (stroke (width 0.12) (type solid)) (layer "{layer_silk}") (uuid "00000000-0000-0000-0000-000000000012"))
		(fp_circle (center -1.5 -1.5) (end -1.2 -1.5) (stroke (width 0.1) (type solid)) (fill yes) (layer "{layer_silk}") (uuid "00000000-0000-0000-0000-000000000013"))
		(fp_rect (start -1.2 -0.8) (end 1.2 0.8) (stroke (width 0.1) (type solid)) (fill no) (layer "{layer_fab}") (uuid "00000000-0000-0000-0000-000000000014"))
		(fp_poly (pts (xy 0 0) (xy 0.4 0) (xy 0.4 0.4)) (stroke (width 0.05) (type solid)) (fill yes) (layer "{layer_fab}") (uuid "00000000-0000-0000-0000-000000000015"))
		(fp_curve (pts (xy 0 0) (xy 0.1 0.2) (xy 0.3 0.2) (xy 0.4 0)) (stroke (width 0.1) (type solid)) (layer "{layer_silk}") (uuid "00000000-0000-0000-0000-000000000016"))
		(fp_text user "${{REFERENCE}}" (at 0.3 0.1 30) (layer "{layer_fab}") (uuid "00000000-0000-0000-0000-000000000017") (effects (font (size 0.5 0.4) (thickness 0.075)) (justify left{mirrored})))
"#
        )
    };
    board(&format!(
        r#"	(footprint "T:Top" (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000a1") (at 10 10 90)
		(property "Reference" "U1" (at 0 -3 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-0000000000a2") (effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "T" (at 0 3 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-0000000000a3") (effects (font (size 1 1) (thickness 0.15))))
{top}		(pad "1" smd rect (at -1 0) (size 0.8 0.8) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "00000000-0000-0000-0000-0000000000a4")))
	(footprint "T:Bottom" (layer "B.Cu") (uuid "00000000-0000-0000-0000-0000000000b1") (at 30 10 -45)
		(property "Reference" "U2" (at 0 -3 0) (layer "B.SilkS") (uuid "00000000-0000-0000-0000-0000000000b2") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
		(property "Value" "T" (at 0 3 0) (layer "B.Fab") (uuid "00000000-0000-0000-0000-0000000000b3") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
{bottom}		(pad "1" smd rect (at 1 0) (size 0.8 0.8) (layers "B.Cu" "B.Mask" "B.Paste") (uuid "00000000-0000-0000-0000-0000000000b4")))
"#,
        top = items("F.SilkS", "F.Fab", ""),
        bottom = items("B.SilkS", "B.Fab", " mirror"),
    ))
}

#[test]
fn a_boards_footprint_graphics_are_part_of_its_footprint_and_come_back_out() {
    let (design, model, _) = import_kicad_pcb(&two_footprints()).expect("imports");
    let top = art_of(&model, "U1");
    assert_eq!((top.shapes.len(), top.texts.len()), (6, 1), "{top:#?}");
    // the silkscreen is on the footprint's side, the fab body on its fab layer
    assert!(top.shapes.iter().all(|s| s.layer() == "F.SilkS" || s.layer() == "F.Fab"));
    // the bottom-side footprint is the same definition seen from the top: its file is B.SilkS and mirrored, its definition is F.SilkS
    let bottom = art_of(&model, "U2");
    assert_eq!(bottom.shapes.len(), 6);
    assert!(bottom.shapes.iter().all(|s| s.layer() == "F.SilkS" || s.layer() == "F.Fab"), "{bottom:#?}");
    // The fixture draws the same coordinates on both footprints, which is a mirrored drawing on the bottom one: its definition (the part seen from the
    // top) is the top footprint's with x negated.
    let line = |art: &FpArt| match &art.shapes[0].shape {
        Shape::Segment { start, end, .. } => (start.x, start.y, end.x, end.y),
        other => panic!("{other:?}"),
    };
    assert_eq!(line(top), (-1_000, -2_000, 1_000, -2_000));
    assert_eq!(line(bottom), (1_000, -2_000, -1_000, -2_000));
    assert_eq!(top.texts[0].layout.at.x, -bottom.texts[0].layout.at.x);

    let text = export(&design, &model);
    for tag in ["(fp_line ", "(fp_arc ", "(fp_circle ", "(fp_poly ", "(fp_curve ", "(fp_text user "] {
        assert_eq!(count_in(&text, tag), 2, "{tag}\n{text}");
    }
    // the fab rectangle of each; a footprint that had no courtyard gets none made up for it (`FootprintExtra::no_courtyard`)
    assert_eq!(count_in(&text, "(fp_rect "), 2, "{text}");
    assert_eq!(count_in(&text, "(layer \"F.CrtYd\")") + count_in(&text, "(layer \"B.CrtYd\")"), 0, "{text}");
    // each side's footprint is written on its own layers
    assert_eq!(count_in(&text, "(layer \"F.SilkS\") (uuid"), 3 + 1, "three shapes of U1 and its Reference");
    assert_eq!(count_in(&text, "(layer \"B.SilkS\") (uuid"), 3 + 1);

    // and read again it is the same art
    let (again, model2, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(art_of(&model2, "U1"), top);
    assert_eq!(art_of(&model2, "U2"), bottom);
    // and written again, the art is the same text
    let art_lines = |t: &str| t.lines().filter(|l| l.trim_start().starts_with("(fp_") && !l.contains("CrtYd")).map(str::to_string).collect::<Vec<_>>();
    assert_eq!(art_lines(&export(&again, &model2)), art_lines(&text));
}

/// Two instances of one library footprint whose drawings differ (one has its pin-1 mark moved, say) are two definitions here, kept apart under `Lib:Name` and `Lib:Name#2`: in the
/// file both are still `Lib:Name`.
#[test]
fn instances_of_one_library_footprint_with_different_drawings_keep_its_name_in_the_file() {
    let fp = |reference: &str, x: &str, uuid: &str| {
        format!(
            r#"	(footprint "Lib:Mark" (layer "F.Cu") (uuid "{uuid}") (at {x} 10)
		(property "Reference" "{reference}" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
		(fp_line (start -1 -2) (end 1 -2) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-0000000000f{reference}"))
		(fp_circle (center {x_mark} -2) (end {x_end} -2) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-0000000000e{reference}"))
		(pad "1" smd rect (at -1 0) (size 0.8 0.8) (layers "F.Cu" "F.Paste" "F.Mask")))
"#,
            x_mark = if reference == "1" { "-1.5" } else { "1.5" },
            x_end = if reference == "1" { "-1.2" } else { "1.8" },
        )
    };
    let text = board(&format!("{}{}", fp("1", "10", "00000000-0000-0000-0000-0000000000a1"), fp("2", "20", "00000000-0000-0000-0000-0000000000a2")));
    let (design, model, _) = import_kicad_pcb(&text).expect("imports");
    let names: Vec<&str> = model.footprints.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["Lib:Mark", "Lib:Mark#2"], "two definitions");
    let out = export(&design, &model);
    assert_eq!(count_in(&out, "(footprint \"Lib:Mark\""), 2, "{out}");
    assert_eq!(count_in(&out, "#2"), 0, "{out}");
}

/// The through-hole pads of an older library are on the silkscreen too (`(layers "*.Cu" "*.Mask" "F.SilkS")`): kicad-cli's silkscreen checks measure them there, so the writer keeps them
/// there -- and on the other side's layer once the footprint is flipped.
#[test]
fn the_layers_of_a_pad_besides_copper_paste_and_mask_come_back_out_on_the_footprints_side() {
    let text = board(
        r#"	(footprint "T:Old" (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000c1") (at 10 10)
		(property "Reference" "J1" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
		(pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask" "F.SilkS"))
		(pad "2" smd rect (at 3 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask" "F.Fab"))
		(pad "3" smd rect (at 6 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
	(footprint "T:Back" (layer "B.Cu") (uuid "00000000-0000-0000-0000-0000000000d1") (at 30 10)
		(property "Reference" "J2" (at 0 -3 0) (layer "B.SilkS") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
		(pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask" "B.SilkS")))
"#,
    );
    let (mut design, model, _) = import_kicad_pcb(&text).expect("imports");
    let layers_of = |out: &str| out.lines().filter(|l| l.trim_start().starts_with("(pad ")).map(|l| l[l.find("(layers").expect("layers")..].split(')').next().unwrap().to_string() + ")").collect::<Vec<_>>();

    // as the file had them (the pads are written in number order, J1 then J2)
    let out = export(&design, &model);
    assert_eq!(
        layers_of(&out),
        vec![r#"(layers "*.Cu" "*.Mask" "F.SilkS")"#, r#"(layers "F.Cu" "F.Paste" "F.Mask" "F.Fab")"#, r#"(layers "F.Cu" "F.Paste" "F.Mask")"#, r#"(layers "*.Cu" "*.Mask" "B.SilkS")"#],
        "{out}"
    );
    // and read again it is the same text
    let (again, model2, _) = import_kicad_pcb(&out).expect("re-imports");
    assert_eq!(layers_of(&export(&again, &model2)), layers_of(&out));

    // a footprint that is flipped takes its pads' silkscreen and fab layers to the other side
    for fp in design.placement.as_mut().expect("placement").footprints.iter_mut().filter(|f| f.id == "J1") {
        fp.side = eda_model::ir::Side::Bottom;
    }
    let flipped = layers_of(&export(&design, &model));
    assert_eq!(flipped[0], r#"(layers "*.Cu" "*.Mask" "B.SilkS")"#, "{flipped:?}");
    assert_eq!(flipped[1], r#"(layers "B.Cu" "B.Paste" "B.Mask" "B.Fab")"#, "{flipped:?}");
    assert_eq!(flipped[2], r#"(layers "B.Cu" "B.Paste" "B.Mask")"#, "{flipped:?}");
}

#[test]
fn a_library_footprint_placed_by_the_studio_carries_its_silkscreen_to_the_board() {
    // `parse_footprint_file` is how a part that names `Lib:Name` gets its pads; it now brings the graphics and texts as well.
    let fp = eda_kicad::parse_footprint_file(
        r#"(footprint "R_0603" (layer "F.Cu")
            (fp_line (start -0.2 -0.5) (end 0.2 -0.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
            (fp_rect (start -0.8 -0.4) (end 0.8 0.4) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
            (fp_rect (start -1.4 -0.7) (end 1.4 0.7) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
            (fp_text user "${REFERENCE}" (at 0 0) (layer "F.Fab") (effects (font (size 0.4 0.4) (thickness 0.06))))
            (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste"))
            (pad "2" smd roundrect (at 0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste")))"#,
        "Resistor_SMD:R_0603",
    )
    .expect("parses");
    assert_eq!((fp.art.shapes.len(), fp.art.texts.len()), (2, 1), "the courtyard is not art: {:#?}", fp.art);
    assert_eq!(fp.art.bbox_on("F.Fab"), Some((-850, -450, 850, 450)), "the fab body, the stroke included");
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root").to_path_buf()
}

/// A board made from one of the examples the way the studio makes one: the intent, its footprints resolved against the installed KiCad libraries, the
/// schematic derived, the parts placed. `None` when KiCad's footprint libraries are not installed.
fn board_from_example(name: &str) -> Option<(Design, ConstraintModel)> {
    let root = default_footprint_library_root();
    if !root.join("Resistor_SMD.pretty").is_dir() {
        eprintln!("skipped: no KiCad footprint libraries at {}", root.display());
        return None;
    }
    let text = std::fs::read_to_string(repo_root().join("examples").join(format!("{name}.yaml"))).expect("the example");
    let mut model: ConstraintModel = serde_yaml::from_str(&text).expect("the example parses");
    let warnings = resolve_library_footprints(&mut model, &root);
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = derive_schematic(&model, &EngineOptions { seed: 0, intent_hash: format!("fp_art_{name}"), ..Default::default() }).expect("derive");
    let placed = place(&design, &model, &PlaceOptions { seed: 0, ..Default::default() }).expect("place");
    Some((placed, model))
}

#[test]
fn a_board_made_from_an_example_has_the_outlines_of_its_parts() {
    let Some((design, model)) = board_from_example("opamp_filter") else { return };
    // The 0603 resistors and capacitors and the SOIC-8 are the studio's built-in packages: each has the silkscreen and fab body of the KiCad footprint it stands for.
    for reference in ["R1", "C3", "U1"] {
        let part = model.part(reference).expect("part");
        let footprint = model.footprint_of(part).expect("footprint");
        assert!(footprint.art.shapes.iter().any(|s| s.on_layer("F.SilkS")), "{reference}: no silkscreen");
        assert!(footprint.art.shapes.iter().any(|s| s.on_layer("F.Fab")), "{reference}: no fab body");
        assert!(footprint.art.texts.iter().any(|t| t.text == "${REFERENCE}"), "{reference}: no reference on the fab layer");
        assert!(footprint.art.bbox_on("F.Fab").is_some());
    }
    // An SOIC-8 has a pin-1 mark.
    let u1 = model.footprint_of(model.part("U1").unwrap()).unwrap();
    assert!(u1.art.shapes.len() >= 8, "{}", u1.art.shapes.len());

    let text = export(&design, &model);
    let silk = text.matches("(layer \"F.SilkS\") (uuid").count();
    assert!(silk > 15, "the silkscreen of 3 resistors, 3 capacitors and an op-amp, plus their references: {silk}");
    assert!(count_in(&text, "(fp_text user \"${REFERENCE}\"") >= 7, "{text}");
    // and it reads back the same
    let (_, again, _) = import_kicad_pcb(&text).expect("re-imports");
    let r1 = again.footprint_of(again.part("R1").unwrap()).unwrap();
    assert_eq!(r1.art.shapes.len(), model.footprint_of(model.part("R1").unwrap()).unwrap().art.shapes.len());
}

#[test]
fn kicad_cli_plots_the_same_silkscreen_and_fab_after_the_round_trip_of_a_board_made_from_an_example() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let Some((design, model)) = board_from_example("opamp_filter") else { return };
    let dir = std::env::temp_dir().join(format!("eda_fp_art_ex_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    let first = export(&design, &model);
    let first_path = dir.join("first.kicad_pcb");
    std::fs::write(&first_path, &first).expect("write");
    let (back, model2, _) = import_kicad_pcb(&first).expect("re-imports");
    let second_path = dir.join("second.kicad_pcb");
    std::fs::write(&second_path, export(&back, &model2)).expect("write");

    let before = plot(&cli, &first_path, PLOTTED, &dir, "before");
    let after = plot(&cli, &second_path, PLOTTED, &dir, "after");
    for (layer, items) in &before {
        eprintln!("example: {layer}: {} primitives before, {} after", items.len(), after.get(layer).map_or(0, Vec::len));
    }
    // the parts are on the board with their outlines: the silkscreen and the fab layer hold their drawings, and the references
    assert!(before["F_Silkscreen"].len() > 100, "{}", before["F_Silkscreen"].len());
    assert!(before["F_Fab"].len() > 100, "{}", before["F_Fab"].len());
    assert_same_plots(&before, &after, "opamp_filter");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Shape` of an art item, for the assertions above that want only geometry.
#[allow(dead_code)]
fn shape_kinds(art: &FpArt) -> Vec<&'static str> {
    art.shapes
        .iter()
        .map(|s| match s.shape {
            Shape::Segment { .. } => "line",
            Shape::Arc { .. } => "arc",
            Shape::Rect { .. } => "rect",
            Shape::Circle { .. } => "circle",
            Shape::Polygon { .. } => "poly",
            Shape::Bezier { .. } => "curve",
        })
        .collect()
}

// ----------------------------------------------------------------------------------------------------------------------
// kicad-cli
// ----------------------------------------------------------------------------------------------------------------------

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// KiCad's own QA boards: `<KICAD_QA_DATA>/pcbnew`, `KICAD_QA_DATA` naming `qa/data` of the KiCad sources (commit 8303b2ad); the default is the copy kept beside them.
fn qa_boards_dir() -> PathBuf {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("pcbnew")
}

/// One drawing primitive of an SVG plot: its element, the style of the group round it (its colours; the stroke widths are in `widths`) and its numbers.
#[derive(Debug, Clone)]
struct Item {
    kind: String,
    style: String,
    widths: Vec<f64>,
    nums: Vec<f64>,
}

impl Item {
    fn new(kind: &str, styles: &[String], nums: Vec<f64>) -> Item {
        let joined = styles.join(" / ");
        // The stroke width is a number to compare with a tolerance, not a word: KiCad prints a width with four decimals, this workspace keeps micrometres.
        // The width a closed polygon is stroked with depends on what was plotted just before it (the fill is plotted "with the current width"), which
        // the order of the footprints in the file changes: it is not compared.
        let mut widths = Vec::new();
        let mut style = String::new();
        let mut rest = joined.as_str();
        while let Some(i) = rest.find("stroke-width:") {
            let (head, tail) = rest.split_at(i + "stroke-width:".len());
            style.push_str(head);
            let end = tail.find(';').unwrap_or(tail.len());
            if let Ok(w) = tail[..end].trim().parse::<f64>() {
                if kind != "poly" {
                    widths.push(w);
                }
                style.push('W');
            } else {
                style.push_str(&tail[..end]);
            }
            rest = &tail[end..];
        }
        style.push_str(rest);
        // A polygon is its box and nothing else: this workspace keeps micrometres where KiCad's files have nanometres, and KiCad drops a point that the
        // rounding put on top of the one before it, so a logo of a few hundred points is a few points shorter after the round trip.
        // An arc is its two ends and the way it goes: its radius, worked out from three points on the micrometre grid, moves with them when the arc is
        // nearly a full turn.
        let nums = if kind == "arc" && nums.len() == 9 { vec![nums[0], nums[1], nums[5], nums[6], nums[7], nums[8]] } else { nums };
        let nums = if kind == "poly" && nums.len() >= 4 {
            let (xs, ys): (Vec<f64>, Vec<f64>) = (nums.iter().step_by(2).copied().collect(), nums.iter().skip(1).step_by(2).copied().collect());
            let fold = |v: &[f64], f: fn(f64, f64) -> f64, init: f64| v.iter().copied().fold(init, f);
            vec![fold(&xs, f64::min, f64::MAX), fold(&ys, f64::min, f64::MAX), fold(&xs, f64::max, f64::MIN), fold(&ys, f64::max, f64::MIN)]
        } else {
            nums
        };
        Item { kind: kind.to_string(), style, widths, nums }
    }

    fn describe(&self) -> String {
        format!("{} [{}] w{:?} {}", self.kind, self.style, self.widths, self.nums.iter().map(|v| format!("{v:.4}")).collect::<Vec<_>>().join(" "))
    }
}

fn numbers(s: &str) -> Vec<f64> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty() && *t != "-" && *t != ".").filter_map(|t| t.parse().ok()).collect()
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// The style an element is drawn in: its own `style` when it has one (a filled polygon is plotted with its own, wherever the plotter happens to have it in a group of
/// strokes), else the groups round it.
fn styled(tag: &str, groups: &[String]) -> Vec<String> {
    match attr(tag, "style") {
        Some(own) => vec![own.split_whitespace().collect::<Vec<_>>().join(" ")],
        None => groups.to_vec(),
    }
}

/// The drawing primitives of an SVG plot: every `path`, `circle`, `rect`, `ellipse` and `text`, with the style of the `g` they are in.
fn svg_items(svg: &str) -> Vec<Item> {
    // The plot also marks every drill hole, in black or white: pads and vias, which are not this test's business.
    let is_hole_mark = |style: &str| style.contains("fill:#000000;") && style.contains("stroke:none") || style.contains("fill:#FFFFFF;") && style.contains("stroke:none");
    let mut items = Vec::new();
    let mut styles: Vec<String> = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        match name.as_str() {
            "g" => {
                if !tag.ends_with('/') {
                    let s = attr(tag, "style").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                    styles.push(format!("{}{}", s, attr(tag, "class").map(|c| format!(" .{c}")).unwrap_or_default()));
                }
            }
            "path" => {
                let d = attr(tag, "d").unwrap_or("");
                let kind = if d.contains('A') { "arc" } else if d.contains('C') { "curve" } else if d.contains('Z') { "poly" } else { "line" };
                items.push(Item::new(kind, &styled(tag, &styles), numbers(&d.replace(['M', 'L', 'A', 'C', 'Z'], " "))));
            }
            "circle" if styles.last().is_some_and(|s| is_hole_mark(s)) => {}
            "circle" => items.push(Item::new("circle", &styled(tag, &styles), ["cx", "cy", "r"].iter().filter_map(|a| attr(tag, a).and_then(|v| v.parse().ok())).collect())),
            "rect" => items.push(Item::new("rect", &styled(tag, &styles), ["x", "y", "width", "height"].iter().filter_map(|a| attr(tag, a).and_then(|v| v.parse().ok())).collect())),
            "text" => items.push(Item::new(&format!("text:{}", rest[end + 1..].split('<').next().unwrap_or("")), &styled(tag, &styles), ["x", "y", "font-size"].iter().filter_map(|a| attr(tag, a).and_then(|v| v.parse().ok())).collect())),
            _ if name == "/g" || tag.starts_with("/g") => {
                styles.pop();
            }
            _ => {}
        }
        rest = &rest[end + 1..];
    }
    items
}

/// kicad-cli's plot of `layers` of `board`, one SVG per layer, as `layer name -> items`.
fn plot(cli: &Path, board: &Path, layers: &str, dir: &Path, name: &str) -> BTreeMap<String, Vec<Item>> {
    let out_dir = dir.join(name);
    std::fs::create_dir_all(&out_dir).expect("a plot directory");
    let out = Command::new(cli).args(["pcb", "export", "svg", "--layers", layers, "--mode-multi", "--exclude-drawing-sheet", "-o"]).arg(&out_dir).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    let mut plots = BTreeMap::new();
    for e in std::fs::read_dir(&out_dir).expect("the plots").flatten() {
        let file = e.file_name().to_string_lossy().to_string();
        // `<board>-<layer>.svg`
        let layer = file.rsplit_once('-').map(|(_, l)| l.trim_end_matches(".svg").to_string()).unwrap_or(file.clone());
        plots.insert(layer, svg_items(&std::fs::read_to_string(e.path()).expect("an svg")));
    }
    plots
}

/// Two plots of the same layer match when every primitive of one has a partner in the other with the same element and style whose numbers agree to
/// `tol` millimetres (a line either way round). Returns the primitives left over on each side.
fn unmatched(a: &[Item], b: &[Item], tol: f64) -> (Vec<Item>, Vec<Item>) {
    let canon = |it: &Item| -> Vec<f64> {
        let mut v = if it.kind == "line" && it.nums.len() == 4 && (it.nums[0], it.nums[1]) > (it.nums[2], it.nums[3]) { vec![it.nums[2], it.nums[3], it.nums[0], it.nums[1]] } else { it.nums.clone() };
        v.extend(&it.widths);
        v
    };
    let mut groups: BTreeMap<(String, String, usize), (Vec<Item>, Vec<Item>)> = BTreeMap::new();
    for it in a {
        groups.entry((it.kind.clone(), it.style.clone(), it.nums.len() + it.widths.len())).or_default().0.push(it.clone());
    }
    for it in b {
        groups.entry((it.kind.clone(), it.style.clone(), it.nums.len() + it.widths.len())).or_default().1.push(it.clone());
    }
    let (mut only_a, mut only_b) = (Vec::new(), Vec::new());
    for (_, (xs, ys)) in groups {
        let mut ys: Vec<Option<Item>> = ys.into_iter().map(Some).collect();
        for x in xs {
            let cx = canon(&x);
            let hit = ys.iter().position(|y| y.as_ref().is_some_and(|y| canon(y).iter().zip(&cx).all(|(p, q)| (p - q).abs() <= tol)));
            match hit {
                Some(i) => ys[i] = None,
                None => only_a.push(x),
            }
        }
        only_b.extend(ys.into_iter().flatten());
    }
    (only_a, only_b)
}

fn assert_same_plots(before: &BTreeMap<String, Vec<Item>>, after: &BTreeMap<String, Vec<Item>>, what: &str) {
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{what}: the layers plotted");
    for (layer, a) in before {
        let (only_before, only_after) = unmatched(a, &after[layer], 0.003);
        assert!(
            only_before.is_empty() && only_after.is_empty(),
            "{what}: {layer}: {} of {} primitives only before, {} of {} only after\nbefore: {:#?}\nafter: {:#?}",
            only_before.len(),
            a.len(),
            only_after.len(),
            after[layer].len(),
            only_before.iter().take(8).map(Item::describe).collect::<Vec<_>>(),
            only_after.iter().take(8).map(Item::describe).collect::<Vec<_>>()
        );
    }
}

const PLOTTED: &str = "F.SilkS,B.SilkS,F.Fab,B.Fab";

#[test]
fn kicad_cli_plots_the_same_silkscreen_and_fab_after_the_round_trip_of_a_small_board() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_fp_art_rt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    let original = two_footprints();
    let original_path = dir.join("original.kicad_pcb");
    std::fs::write(&original_path, &original).expect("write");
    let (design, model, _) = import_kicad_pcb(&original).expect("imports");
    let exported_path = dir.join("exported.kicad_pcb");
    std::fs::write(&exported_path, export(&design, &model)).expect("write");

    let before = plot(&cli, &original_path, PLOTTED, &dir, "before");
    let after = plot(&cli, &exported_path, PLOTTED, &dir, "after");
    assert!(before.values().map(Vec::len).sum::<usize>() > 40, "the plots hold the drawings: {:?}", before.iter().map(|(k, v)| (k, v.len())).collect::<Vec<_>>());
    assert_same_plots(&before, &after, "small board");
    let _ = std::fs::remove_dir_all(&dir);
}

/// KiCad boards with a lot of footprint graphics and texts on both sides, and `complex_hierarchy`, whose 185 through-hole pads are on the silkscreen too
/// (`(layers "*.Cu" "*.Mask" "F.SilkS")`, the legacy libraries' pads: `PadMaskInfo::art_layers`). Not here, for what they have besides: a board-level text of
/// several lines or of a glyph that is not square (`issue7004`, `issue8407`, `issue11814`, `issue22826`, `connect`) and a board-level dotted rectangle
/// (`connect`) -- the board-level `Text` and `Shape` keep neither -- and the 1 um grid, which moves the radius of a nearly closed arc.
const QA_BOARDS: [&str; 6] = ["footprints_load_save", "component_classes", "issue24078/issue24078", "issue5102", "issue5990", "complex_hierarchy"];

#[test]
fn kicad_cli_plots_the_same_silkscreen_and_fab_after_the_round_trip_of_qa_boards() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let qa = qa_boards_dir();
    if !qa.exists() {
        eprintln!("skipped: no QA boards at {}", qa.display());
        return;
    }
    let dir = std::env::temp_dir().join(format!("eda_fp_art_qa_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    if std::env::var_os("EDA_FP_ART_KEEP").is_some() {
        eprintln!("kept: {}", dir.display());
    }
    let names: Vec<String> = match std::env::var("EDA_FP_ART_BOARD") {
        Ok(one) => vec![one],
        Err(_) => QA_BOARDS.iter().map(|s| s.to_string()).collect(),
    };
    for name in names {
        let stem = Path::new(&name).file_name().and_then(|s| s.to_str()).expect("a board name").to_string();
        let work = dir.join(&stem);
        std::fs::create_dir_all(&work).expect("a board directory");
        let text = std::fs::read_to_string(qa.join(format!("{name}.kicad_pcb"))).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let original_path = work.join(format!("{stem}.kicad_pcb"));
        std::fs::write(&original_path, &text).expect("write");
        let (design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {name}: {e:?}"));
        let exported_path = work.join(format!("{stem}_exported.kicad_pcb"));
        std::fs::write(&exported_path, export(&design, &model)).expect("write");

        let before = plot(&cli, &original_path, PLOTTED, &work, "before");
        let after = plot(&cli, &exported_path, PLOTTED, &work, "after");
        for (layer, items) in &before {
            eprintln!("{name}: {layer}: {} primitives before, {} after", items.len(), after.get(layer).map_or(0, Vec::len));
        }
        assert!(before.values().map(Vec::len).sum::<usize>() > 100, "{name}: the plots hold the drawings");
        assert_same_plots(&before, &after, &name);
    }
    if std::env::var_os("EDA_FP_ART_KEEP").is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}
