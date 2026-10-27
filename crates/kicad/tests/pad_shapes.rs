//! Trapezoid, chamfered-rectangle and custom pads through the importer and the writer: a board that has them comes back out of
//! `export_kicad_pcb` with the same tokens (`rect_delta`, `chamfer_ratio` and `chamfer`, `options` and `primitives`, the offset inside
//! `drill`), a footprint on the bottom side is mirrored the way `PAD::Flip` mirrors it, and the copper the DRC measures is the pad's
//! own outline. The first group needs no external tool; the last test (`EDA_SLOW_TESTS=1`) asks kicad-cli to plot the copper of the
//! original and of our export and compares the two.

use eda_model::units::mm;
use eda_model::units::Nm;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::footprint::{CustomZoneMode, Pad, PadAnchor, PadShape};
use eda_model::ir::{ChamferCorners, Design, Shape};
use eda_model::ConstraintModel;

const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (9 "F.Adhes" user "F.Adhesive") (13 "F.Paste" user) (15 "B.Paste" user) (1 "F.Mask" user) (3 "B.Mask" user) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(gr_rect (start 0 0) (end 40 20) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
"#;

/// A footprint `Q` at (x, y) turned `rot` degrees on `layer` with the given pads.
fn footprint(reference: &str, layer: &str, x: f64, y: f64, rot: f64, pads: &str) -> String {
    format!(
        "\t(footprint \"Test:{reference}\" (layer \"{layer}\") (uuid \"10000000-0000-0000-0000-{:012}\") (at {x} {y} {rot})\n\t\t(property \"Reference\" \"{reference}\" (at 0 0 0) (layer \"F.SilkS\"))\n{pads}\t)\n",
        reference.bytes().map(|b| b as u64).sum::<u64>()
    )
}

fn board(footprints: &str) -> String {
    format!("{HEADER}{footprints})\n")
}

/// The top-side set: a trapezoid, a chamfered and rounded rectangle, a custom pad with every kind of primitive, and a pad with an offset.
fn top_pads() -> &'static str {
    r#"		(pad "1" smd trapezoid (at -3 0) (size 2 1) (rect_delta 0.4 0) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "20000000-0000-0000-0000-000000000001"))
		(pad "2" smd roundrect (at 0 0) (size 1.2 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.2) (chamfer_ratio 0.25) (chamfer top_left bottom_right) (uuid "20000000-0000-0000-0000-000000000002"))
		(pad "3" smd custom (at 3 0 30) (size 0.8 0.8) (layers "F.Cu" "F.Mask") (options (clearance convexhull) (anchor rect))
			(primitives
				(gr_poly (pts (xy 0 0) (xy 1.5 0) (xy 1.5 0.5) (xy 0 1)) (width 0) (fill yes))
				(gr_line (start 0 0) (end -1.2 -0.6) (width 0.2))
				(gr_arc (start 0 -1) (mid 0.7 -0.7) (end 1 0) (width 0.15))
				(gr_circle (center 0 1.5) (end 0.4 1.5) (width 0.1) (fill no))
				(gr_rect (start -1 -1.6) (end -0.6 -1.3) (width 0) (fill yes))
				(gr_bbox (start -0.4 -0.4) (end 0.4 0.4))
				(gr_vector (start 0 0) (end 2 0)))
			(uuid "20000000-0000-0000-0000-000000000003"))
		(pad "4" thru_hole oval (at 0 3) (size 2 1.2) (drill 0.7 (offset 0.3 0)) (layers "*.Cu" "*.Mask") (uuid "20000000-0000-0000-0000-000000000004"))
"#
}

/// The same four pads as `top_pads` for a footprint on the back, as `PAD::Flip` leaves them in the file: x of every position, offset, delta and
/// primitive negated, the chamfer's corners swapped left to right, the layers on the back (the pad's own angle is the file's absolute one).
fn bottom_pads() -> &'static str {
    r#"		(pad "1" smd trapezoid (at 3 0) (size 2 1) (rect_delta -0.4 0) (layers "B.Cu" "B.Paste" "B.Mask") (uuid "20000000-0000-0000-0000-000000000011"))
		(pad "2" smd roundrect (at 0 0) (size 1.2 0.8) (layers "B.Cu" "B.Paste" "B.Mask") (roundrect_rratio 0.2) (chamfer_ratio 0.25) (chamfer top_right bottom_left) (uuid "20000000-0000-0000-0000-000000000012"))
		(pad "3" smd custom (at -3 0 -30) (size 0.8 0.8) (layers "B.Cu" "B.Mask") (options (clearance convexhull) (anchor rect))
			(primitives
				(gr_poly (pts (xy 0 0) (xy -1.5 0) (xy -1.5 0.5) (xy 0 1)) (width 0) (fill yes))
				(gr_line (start 0 0) (end 1.2 -0.6) (width 0.2))
				(gr_arc (start 0 -1) (mid -0.7 -0.7) (end -1 0) (width 0.15))
				(gr_circle (center 0 1.5) (end -0.4 1.5) (width 0.1) (fill no))
				(gr_rect (start 1 -1.6) (end 0.6 -1.3) (width 0) (fill yes))
				(gr_bbox (start 0.4 -0.4) (end -0.4 0.4))
				(gr_vector (start 0 0) (end -2 0)))
			(uuid "20000000-0000-0000-0000-000000000013"))
		(pad "4" thru_hole oval (at 0 3) (size 2 1.2) (drill 0.7 (offset -0.3 0)) (layers "*.Cu" "*.Mask") (uuid "20000000-0000-0000-0000-000000000014"))
"#
}

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "pad_shapes" }).expect("export_kicad_pcb")
}

fn pads_of<'a>(model: &'a ConstraintModel, reference: &str) -> &'a [Pad] {
    let part = model.part(reference).unwrap_or_else(|| panic!("{reference} imported"));
    let name = part.footprint.as_deref().expect("a footprint");
    &model.footprints.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("footprint {name}")).pads
}

fn pad<'a>(pads: &'a [Pad], number: &str) -> &'a Pad {
    pads.iter().find(|p| p.number == number).unwrap_or_else(|| panic!("pad {number}"))
}

#[test]
fn a_board_with_trapezoid_chamfered_and_custom_pads_keeps_every_one_of_them() {
    let (design, model, notes) = import_kicad_pcb(&board(&footprint("Q1", "F.Cu", 10.0, 10.0, 0.0, top_pads()))).expect("imports");
    assert_eq!(notes.non_rect_pad_shapes_approximated, 0);
    let pads = pads_of(&model, "Q1");
    assert_eq!(pads.len(), 4);

    let trap = pad(pads, "1");
    assert_eq!((trap.shape, trap.size, trap.ext.trapezoid_delta), (PadShape::Trapezoid, (mm(2.0), mm(1.0)), Some((mm(0.4), Nm::ZERO))));

    let chamfered = pad(pads, "2");
    assert_eq!((chamfered.shape, chamfered.roundrect_ratio, chamfered.ext.chamfer_ratio), (PadShape::ChamferedRect, Some(0.2), Some(0.25)));
    assert_eq!(chamfered.ext.chamfer_corners, ChamferCorners { top_left: true, bottom_right: true, ..Default::default() });

    let custom = pad(pads, "3");
    assert_eq!((custom.shape, custom.rot), (PadShape::Custom, 330_000 % 360_000), "the pad's own angle is the file's: 30 degrees counter-clockwise");
    let c = custom.ext.custom.as_ref().expect("a custom pad's shapes");
    assert_eq!((c.anchor, c.zone), (PadAnchor::Rect, CustomZoneMode::ConvexHull));
    let kinds: Vec<&str> = c.primitives.iter().map(|p| match p.shape {
        Shape::Polygon { .. } => "poly",
        Shape::Segment { .. } => "line",
        Shape::Arc { .. } => "arc",
        Shape::Circle { .. } => "circle",
        Shape::Rect { .. } => "rect",
        Shape::Bezier { .. } => "curve",
    }).collect();
    assert_eq!(kinds, ["poly", "line", "arc", "circle", "rect", "rect", "line"]);
    assert_eq!(c.primitives.iter().map(|p| p.proxy).collect::<Vec<_>>(), [false, false, false, false, false, true, true], "gr_bbox and gr_vector are proxy items");
    assert!(c.primitives[0].shape.is_filled() && !c.primitives[3].shape.is_filled());

    let offset = pad(pads, "4");
    assert_eq!((offset.shape, offset.ext.offset, offset.drill), (PadShape::Oval, Some((mm(0.3), Nm::ZERO)), Some(mm(0.7))));

    // It writes them the way KiCad does ...
    let text = export(&design, &model);
    assert!(text.contains("trapezoid (at -3 0 0) (size 2 1) (rect_delta 0.4 0)"), "{text}");
    assert!(text.contains("(roundrect_rratio 0.2) (chamfer_ratio 0.25) (chamfer top_left bottom_right)"), "{text}");
    assert!(text.contains("(options (clearance convexhull) (anchor rect)) (primitives (gr_poly (pts (xy 0 0) (xy 1.5 0) (xy 1.5 0.5) (xy 0 1)) (width 0) (fill yes))"), "{text}");
    // (the number box with its fill: `formatPrimitives` writes it for every rectangle, and a box that says none is filled by `parsePCB_SHAPE`)
    assert!(text.contains("(gr_bbox (start -0.4 -0.4) (end 0.4 0.4) (fill yes)) (gr_vector (start 0 0) (end 2 0)))"), "{text}");
    assert!(text.contains("(drill 0.7 (offset 0.3 0))"), "{text}");
    assert!(!text.contains("pad \"1\" smd rect "), "a trapezoid is not written as the rectangle round it");

    // ... and what it writes reads back to the same pads.
    let (_, again, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(pads_of(&again, "Q1"), pads, "the pads after a round trip");
}

#[test]
fn a_footprint_on_the_back_is_mirrored_as_pad_flip_mirrors_it() {
    // The same pads, once as a top-side footprint and once as the file holds them for the bottom side.
    let (top_design, top_model, _) = import_kicad_pcb(&board(&footprint("Q1", "F.Cu", 10.0, 10.0, 0.0, top_pads()))).expect("top imports");
    let (bottom_design, bottom_model, _) = import_kicad_pcb(&board(&footprint("Q1", "B.Cu", 10.0, 10.0, 0.0, bottom_pads()))).expect("bottom imports");
    let (top, bottom) = (pads_of(&top_model, "Q1"), pads_of(&bottom_model, "Q1"));
    for number in ["1", "2", "3", "4"] {
        let (t, b) = (pad(top, number), pad(bottom, number));
        // The engine keeps a footprint as seen from the top whatever side it is on: the bottom one is read in the library's frame, which for a file that
        // mirrors x (as this fixture's does, at orientation 0) is the top's turned half a turn (KiCad's flip mirrors y and turns the orientation half a
        // turn), its orientation 180; the two read the same so (save the side the SMD copper is on).
        let mut b = b.clone();
        b.opposite_side = t.opposite_side;
        let mut t = t.clone();
        t.at = (-t.at.0, -t.at.1);
        if !t.ext.is_default() {
            t.rot = (t.rot + 180_000) % 360_000;
        }
        assert_eq!(t, b, "pad {number}");
    }
    assert_eq!(bottom_design.placement.as_ref().unwrap().footprints[0].rot, 180_000);
    // Written out again, the bottom side holds the file's mirrored tokens.
    let text = export(&bottom_design, &bottom_model);
    assert!(text.contains("(rect_delta -0.4 0)") && text.contains("(chamfer top_right bottom_left)") && text.contains("(drill 0.7 (offset -0.3 0))"), "{text}");
    assert!(text.contains("(gr_poly (pts (xy 0 0) (xy -1.5 0) (xy -1.5 0.5) (xy 0 1))") && text.contains("(gr_vector (start 0 0) (end -2 0))"), "{text}");
    // And a top-side one is written as it came.
    assert!(export(&top_design, &top_model).contains("(rect_delta 0.4 0)"));
}

#[test]
fn a_custom_pad_on_the_back_covers_the_mirror_image_of_the_one_on_the_front() {
    // Q1 on the front at x = 10, Q2 on the back at x = 30: the board's mirror about x = 20 holds the same copper, mirrored.
    let text = board(&format!("{}{}", footprint("Q1", "F.Cu", 10.0, 10.0, 0.0, top_pads()), footprint("Q2", "B.Cu", 30.0, 10.0, 0.0, bottom_pads())));
    let (design, model, _) = import_kicad_pcb(&text).expect("imports");
    let board = eda_drc::board::build(&design, &model);
    for number in ["1", "2", "3"] {
        let front = board.pads.iter().find(|p| p.id == format!("Q1.{number}")).expect("front pad");
        let back = board.pads.iter().find(|p| p.id == format!("Q2.{number}")).expect("back pad");
        let (f, b) = (front.copper.bbox(Nm::ZERO), back.copper.bbox(Nm::ZERO));
        // Mirrored about x = 20: x -> 40 - x.
        let mirrored = (mm(40.0) - f.2, f.1, mm(40.0) - f.0, f.3);
        let tol = Nm::new(2000);
        assert!((mirrored.0 - b.0).abs() <= tol && (mirrored.1 - b.1).abs() <= tol && (mirrored.2 - b.2).abs() <= tol && (mirrored.3 - b.3).abs() <= tol, "pad {number}: {f:?} mirrored {mirrored:?} vs {b:?}");
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// KiCad's own QA boards, when they are there
// ----------------------------------------------------------------------------------------------------------------------

fn qa_boards_dir() -> PathBuf {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("pcbnew")
}

#[test]
fn the_custom_pads_of_kicads_qa_boards_keep_their_primitives_through_a_round_trip() {
    let qa = qa_boards_dir();
    for (name, custom_pads) in [("custom_pads.kicad_pcb", 2usize), ("padstacks.kicad_pcb", 3), ("stonehenge.kicad_pcb", 20)] {
        let Ok(text) = std::fs::read_to_string(qa.join(name)) else {
            eprintln!("skipped: no QA board {name}");
            continue;
        };
        let (design, model, notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(notes.non_rect_pad_shapes_approximated, 0);
        let all: Vec<&Pad> = model.footprints.iter().flat_map(|f| f.pads.iter()).collect();
        let customs: Vec<&&Pad> = all.iter().filter(|p| p.shape == PadShape::Custom).collect();
        assert!(!customs.is_empty() && customs.iter().all(|p| p.ext.custom.as_ref().is_some_and(|c| !c.primitives.is_empty())), "{name}: every custom pad has its primitives");
        // The pads of the board that are on copper (the paste-only ones are left out by the importer).
        assert!(customs.len() <= custom_pads, "{name}: {} custom pads", customs.len());
        let out = export(&design, &model);
        let (_, again, _) = import_kicad_pcb(&out).unwrap_or_else(|e| panic!("{name} again: {e:?}"));
        // The writer lists a footprint's pads by number: compare them as a set, footprint by footprint.
        let sorted = |m: &ConstraintModel| -> Vec<(String, Vec<Pad>)> {
            let mut v: Vec<(String, Vec<Pad>)> = m.parts.iter().filter_map(|p| Some((p.reference.clone(), m.footprint_of(p)?.pads))).collect();
            for (_, pads) in v.iter_mut() {
                pads.sort_by(|a, b| (&a.number, a.at, a.size).cmp(&(&b.number, b.at, b.size)));
                // The writer prints a ratio to four decimals.
                for p in pads.iter_mut() {
                    p.roundrect_ratio = p.roundrect_ratio.map(|r| (r * 1e4).round() / 1e4);
                    p.ext.chamfer_ratio = p.ext.chamfer_ratio.map(|r| (r * 1e4).round() / 1e4);
                }
            }
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        let (before, after) = (sorted(&model), sorted(&again));
        assert_eq!(before.len(), after.len(), "{name}");
        // Every pad is exactly the same after the round trip (the numbers are micrometres on both sides).
        for ((reference, a), (_, b)) in before.iter().zip(&after) {
            assert_eq!(a, b, "{name}: footprint {reference}");
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// The copper's area against KiCad's own: the polygon `kicad-cli pcb export svg` draws for the same pad
// ----------------------------------------------------------------------------------------------------------------------

/// The area in mm^2 of the polygon with holes that is the copper of each pad of `text`, in the order of the board's footprints and their
/// pads as `eda_drc::pad_shape::pad_polygons` builds it (the DRC's own outline of the pad, the error of a curve on the inside).
fn pad_areas_mm2(text: &str) -> Vec<(String, f64)> {
    let (design, model, _) = import_kicad_pcb(text).expect("imports");
    let mut out = Vec::new();
    for fp in &design.placement.as_ref().expect("a placement").footprints {
        let part = model.part(&fp.id).expect("a part");
        let footprint = model.footprint_of(part).expect("a footprint");
        for pad in &footprint.pads {
            let centre = eda_model::footprint::to_board(fp, pad.at);
            let area = eda_drc::pad_shape::pad_polygons(fp, pad, centre, Nm::new(5_000)).area() / 1e12;
            out.push((format!("{}.{}", fp.id, pad.number), area));
        }
    }
    out
}

/// KiCad's numbers, measured on 2026-10-10 with kicad-cli 10.99.0 (`pcb export svg` of the copper layer, the area of each pad's path with
/// the even-odd rule): the two pads of `custom_pads.kicad_pcb` (a polygon each, anchor inside), and the ring pad of `stonehenge.kicad_pcb`'s
/// mounting hole H2 (a stroked circle 2.2 mm wide round a 1.45 mm anchor, `(fill no)`).
#[test]
fn the_copper_of_a_custom_pad_has_the_area_kicad_plots_for_it() {
    let qa = qa_boards_dir();
    let Ok(custom_pads) = std::fs::read_to_string(qa.join("custom_pads.kicad_pcb")) else {
        eprintln!("skipped: no QA board custom_pads.kicad_pcb");
        return;
    };
    let areas = pad_areas_mm2(&custom_pads);
    let kicad = [1.875, 1.35];
    assert_eq!(areas.len(), 2, "{areas:?}");
    for ((name, ours), theirs) in areas.iter().zip(kicad) {
        eprintln!("{name}: {ours:.4} mm2, kicad {theirs}");
        assert!((ours - theirs).abs() / theirs < 0.001, "{name}: {ours} mm2 against kicad's {theirs}");
    }

    // The ring: pi (3.7^2 - 1.5^2) = 35.94 mm2 exactly, KiCad's polygon 35.8565 (a polygon of the circle is smaller than the circle).
    let ring = r#"(pad "1" smd custom (at 2.975 0 180) (size 1.45 1.45) (layers "B.Cu" "B.Mask") (options (clearance outline) (anchor circle))
		(primitives (gr_circle (center -2.975 0) (end -0.375 0) (width 2.2) (fill no))))
"#;
    let areas = pad_areas_mm2(&board(&footprint("H2", "B.Cu", 20.0, 10.0, 0.0, ring)));
    let ours = areas[0].1;
    eprintln!("ring: {ours:.4} mm2, kicad 35.8565");
    assert!((ours - 35.8565).abs() / 35.8565 < 0.003, "the ring: {ours} mm2 against kicad's 35.8565");
    assert!((ours - std::f64::consts::PI * (3.7f64.powi(2) - 1.5f64.powi(2))).abs() > 0.0, "a polygon, not the exact circle");
}

/// The 16 custom pads of `issue5093`'s footprint J305 (`R_pads`), big polygons with a circular anchor: their total copper is
/// 3804.5968 mm2 in kicad-cli's plot (the sum of the 16 paths), and each pad's area is what KiCad draws to 0.2%.
#[test]
fn the_sixteen_custom_pads_of_issue5093_cover_the_area_kicad_plots() {
    let qa = qa_boards_dir();
    let Ok(text) = std::fs::read_to_string(qa.join("issue5093.kicad_pcb")) else {
        eprintln!("skipped: no QA board issue5093.kicad_pcb");
        return;
    };
    let areas: Vec<(String, f64)> = pad_areas_mm2(&text).into_iter().filter(|(n, _)| n.starts_with("J305.")).collect();
    assert_eq!(areas.len(), 16, "{areas:?}");
    let total: f64 = areas.iter().map(|(_, a)| a).sum();
    eprintln!("J305: {total:.4} mm2, kicad 3804.5968");
    assert!((total - 3804.5968).abs() / 3804.5968 < 0.002, "{total} mm2 against kicad's 3804.5968");
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

/// kicad-cli's plot of the copper of `layer`, as the sorted list of the closed shapes it drew (a pad is a path or a polygon): the numbers of each
/// rounded to 0.004 mm, which is what the micrometre grid of this workspace costs a curve, and the first point put first so that the shape
/// compares as itself whatever point it was started from.
fn plotted_copper(cli: &Path, board: &Path, layer: &str, dir: &Path, name: &str) -> Vec<String> {
    let svg = dir.join(format!("{name}.svg"));
    let out = Command::new(cli).args(["pcb", "export", "svg", "--layers", layer, "--mode-single", "--exclude-drawing-sheet", "-o"]).arg(&svg).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    copper_shapes_of_svg(&std::fs::read_to_string(&svg).expect("an svg"), false)
}

/// The closed shapes of a kicad-cli SVG plot (see [`plotted_copper`]). A footprint's plot is as large as the footprint's own box (its text included), so
/// `from_origin` moves every shape by the lowest x and y of them all: what the plots then have in common is the copper's own place.
fn copper_shapes_of_svg(text: &str, from_origin: bool) -> Vec<String> {
    let mut shapes: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<path") {
        rest = &rest[i..];
        let end = rest.find("/>").expect("a closed element");
        let el = &rest[..end];
        let d = el.split("d=\"").nth(1).and_then(|t| t.split('"').next()).unwrap_or("");
        let nums: Vec<f64> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse::<f64>().ok()).collect();
        // Skip the edge cuts and any open stroke: only closed shapes (Z) are copper.
        if d.contains('Z') && !nums.is_empty() {
            shapes.push(nums.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect());
        }
        rest = &rest[end..];
    }
    let low = |f: fn(&(f64, f64)) -> f64| shapes.iter().flatten().map(f).fold(f64::INFINITY, f64::min);
    let (dx, dy) = if from_origin { (low(|p| p.0), low(|p| p.1)) } else { (0.0, 0.0) };
    let r = |v: f64| (v * 250.0).round() as i64;
    let mut items: Vec<String> = shapes
        .iter()
        .map(|pts| {
            let pts: Vec<(i64, i64)> = pts.iter().map(|p| (r(p.0 - dx), r(p.1 - dy))).collect();
            let first = pts.iter().enumerate().min_by_key(|(_, p)| **p).map(|(i, _)| i).unwrap_or(0);
            pts.iter().cycle().skip(first).take(pts.len()).map(|p| format!("{},{}", p.0, p.1)).collect::<Vec<_>>().join(" ")
        })
        .collect();
    items.sort();
    items
}

#[test]
fn kicad_cli_plots_the_same_pad_copper_before_and_after_the_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_pad_shapes_rt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");

    let original = board(&format!("{}{}", footprint("Q1", "F.Cu", 10.0, 10.0, 0.0, top_pads()), footprint("Q2", "B.Cu", 30.0, 10.0, 90.0, bottom_pads())));
    let original_path = dir.join("original.kicad_pcb");
    std::fs::write(&original_path, &original).expect("write");
    let (design, model, _) = import_kicad_pcb(&original).expect("imports");
    let exported_path = dir.join("exported.kicad_pcb");
    std::fs::write(&exported_path, export(&design, &model)).expect("write");

    for layer in ["F.Cu", "B.Cu"] {
        let before = plotted_copper(&cli, &original_path, layer, &dir, &format!("original_{}", layer.replace('.', "_")));
        let after = plotted_copper(&cli, &exported_path, layer, &dir, &format!("exported_{}", layer.replace('.', "_")));
        assert!(before.len() >= 3, "{layer}: kicad-cli drew {} copper shapes of the original", before.len());
        assert_eq!(before, after, "kicad-cli draws other copper on {layer} after the round trip");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `.kicad_mod` through the footprint editor's reader and writer (`parse_library_footprint`, `export_kicad_mod`): kicad-cli plots the copper of
/// the library footprint (`fp export svg`) the same before and after, for the trapezoid, the chamfered and rounded rectangle, the custom pad with
/// every kind of primitive and the pad with an offset.
#[test]
fn kicad_cli_plots_the_same_library_footprint_copper_before_and_after_the_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_pad_shapes_mod_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");

    let original = format!(
        "(footprint \"Q\" (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\") (layer \"F.Cu\")\n\t(property \"Reference\" \"REF**\" (at 0 -3 0) (layer \"F.SilkS\"))\n\t(property \"Value\" \"Q\" (at 0 3 0) (layer \"F.Fab\"))\n{}\t)\n",
        top_pads()
    );
    let parsed = eda_kicad::parse_library_footprint(&original).expect("the editor's reader takes it");
    assert!(parsed.footprint.pads.len() == 4, "{} pads", parsed.footprint.pads.len());
    let exported = eda_kicad::export_kicad_mod(&parsed.footprint);

    let mut plots = Vec::new();
    for (name, text) in [("original", &original), ("exported", &exported)] {
        let lib = dir.join(format!("{name}.pretty"));
        let out_dir = dir.join(format!("{name}_svg"));
        std::fs::create_dir_all(&lib).expect("a library folder");
        std::fs::create_dir_all(&out_dir).expect("an output folder");
        std::fs::write(lib.join("Q.kicad_mod"), text).expect("write");
        let out = Command::new(&cli).args(["fp", "export", "svg", "--layers", "F.Cu", "-o"]).arg(&out_dir).arg(&lib).output().expect("runs kicad-cli");
        assert!(out.status.success(), "kicad-cli on the {name} footprint: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        plots.push(copper_shapes_of_svg(&std::fs::read_to_string(out_dir.join("Q.svg")).expect("an svg"), true));
    }
    assert!(plots[0].len() >= 5, "kicad-cli drew {} copper shapes of the original", plots[0].len());
    assert_eq!(plots[0], plots[1], "kicad-cli draws other copper after the footprint went through the editor's reader and writer");
    let _ = std::fs::remove_dir_all(&dir);
}

// ----------------------------------------------------------------------------------------------------------------------
// kicad-cli's DRC measures the same clearances to these pads
// ----------------------------------------------------------------------------------------------------------------------

/// The smallest `actual` of the clearance violations kicad-cli's DRC reports for `board`, in mm (`None`: there is none).
fn kicad_clearance_actual_mm(cli: &Path, board: &Path, dir: &Path, name: &str) -> Option<f64> {
    let json = dir.join(format!("{name}.drc.json"));
    let out = Command::new(cli).args(["pcb", "drc", "--format", "json", "--severity-all", "-o"]).arg(&json).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli drc: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).expect("a report")).expect("json");
    v["violations"]
        .as_array()?
        .iter()
        .filter(|x| x["type"].as_str() == Some("clearance"))
        .filter_map(|x| x["description"].as_str()?.split("actual ").nth(1)?.split(' ').next()?.parse::<f64>().ok())
        .reduce(f64::min)
}

/// A one-footprint board with `pads` (net B) at (10, 10) and a net-A track `width` wide from `a` to `b` (mm) on F.Cu.
fn clearance_board(pads: &str, a: (f64, f64), b: (f64, f64)) -> String {
    let header = HEADER.replace("(net 0 \"\")", "(net 0 \"\")\n\t(net 1 \"B\")\n\t(net 2 \"A\")");
    let fp = footprint("Q1", "F.Cu", 10.0, 10.0, 0.0, pads);
    let track = format!("\t(segment (start {} {}) (end {} {}) (width 0.2) (layer \"F.Cu\") (net 2) (uuid \"30000000-0000-0000-0000-000000000001\"))\n", a.0, a.1, b.0, b.1);
    format!("{header}{fp}{track})\n")
}

#[test]
fn kicad_cli_drc_measures_the_clearance_to_these_pads_as_the_drc_here_does() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_pad_shapes_drc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");

    // The track: 200 um wide, `air` um from the pad's copper along the direction `u`, passing the point `at` on the pad's outline with the outward normal `n`.
    let beside = |at: (f64, f64), n: (f64, f64), u: (f64, f64), air: f64| {
        let (nl, ul) = (n.0.hypot(n.1), u.0.hypot(u.1));
        let off = (air + 100.0) / 1000.0;
        let c = (10.0 + at.0 + n.0 / nl * off, 10.0 + at.1 + n.1 / nl * off);
        ((c.0 - u.0 / ul * 2.5, c.1 - u.1 / ul * 2.5), (c.0 + u.0 / ul * 2.5, c.1 + u.1 / ul * 2.5))
    };
    let pad = |shape_text: &str| format!("\t\t(pad \"1\" smd {shape_text} (net 1 \"B\") (layers \"F.Cu\" \"F.Mask\") (uuid \"20000000-0000-0000-0000-000000000001\"))\n");
    let cases: Vec<(&str, String, ((f64, f64), (f64, f64)))> = vec![
        // The slanted right side of a trapezoid, from (1.4, 0.5) to (0.6, -0.5): the track runs parallel to it.
        ("trapezoid", pad("trapezoid (at 0 0) (size 2 1) (rect_delta 0 0.8)"), beside((1.0, 0.0), (1.0, -0.8), (-0.8, -1.0), 150.0)),
        // The cut corner of a chamfered rectangle: the face x + y = -0.6 (mm, pad frame).
        ("chamfered", pad("roundrect (at 0 0) (size 1 1) (roundrect_rratio 0) (chamfer_ratio 0.4) (chamfer top_left)"), beside((-0.3, -0.3), (-1.0, -1.0), (1.0, -1.0), 150.0)),
        // A custom pad: a round anchor and a 0.2 mm line 2 mm to its right; the track is a vertical one past the line's end.
        ("custom", pad("custom (at 0 0) (size 0.6 0.6) (options (clearance outline) (anchor circle)) (primitives (gr_line (start 0 0) (end 2 0) (width 0.2)))"), ((12.25, 7.5), (12.25, 12.5))),
    ];
    for (name, pad_text, (a, b)) in cases {
        let text = clearance_board(&pad_text, a, b);
        let path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(&path, &text).expect("write");
        let theirs = kicad_clearance_actual_mm(&cli, &path, &dir, name).unwrap_or_else(|| panic!("{name}: kicad-cli reports no clearance violation:\n{text}"));

        // Ours: the gap between the pad's copper and the track's stadium.
        let (design, model, _) = import_kicad_pcb(&text).expect("imports");
        let board = eda_drc::board::build(&design, &model);
        let copper = &board.pads[0].copper;
        let nm = |v: f64| Nm::kiround(v * 1e6);
        let track = eda_drc::kimath::Shape::Stadium { a: eda_model::ir::Point { x: nm(a.0), y: nm(a.1) }, b: eda_model::ir::Point { x: nm(b.0), y: nm(b.1) }, r: mm(0.1) };
        let ours = copper.gap_to(&track) / 1e6;
        eprintln!("{name}: kicad-cli {theirs:.4} mm, here {ours:.4} mm");
        assert!((ours - theirs).abs() < 0.002, "{name}: kicad-cli's DRC says {theirs} mm of air, the DRC here {ours} mm");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
