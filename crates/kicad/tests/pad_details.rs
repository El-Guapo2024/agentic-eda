//! Padstacks through the importer, the writer and kicad-cli: a pad with a shape of its own on the front, the inner and the back copper
//! (`(padstack (mode front_inner_back|custom) ...)`). The first group needs no external tool (KiCad's own QA board with padstacks goes in and
//! comes out with the same blocks); the rest (`EDA_SLOW_TESTS=1`) asks kicad-cli to plot each copper layer of the original and of our export and
//! compares: the copper of a layer is what the model has for that layer (the box of each pad's shape there), and what our writer wrote plots as
//! what the reader took.

use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_kicad_mod, export_kicad_pcb, import_kicad_pcb, parse_library_footprint, ExportMeta};
use eda_model::footprint::Pad;
use eda_model::ir::Design;
use eda_model::padstack::PadStackMode;
use eda_model::ConstraintModel;

fn qa_boards_dir() -> PathBuf {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("pcbnew")
}

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "pad_details" }).expect("export_kicad_pcb")
}

/// Every pad of a board with the footprint it is in: `(reference, side, pad)`.
fn pads_of(model: &ConstraintModel, design: &Design) -> Vec<(String, Pad)> {
    let mut out = Vec::new();
    for fp in &design.placement.as_ref().expect("a placement").footprints {
        let part = model.part(&fp.id).expect("a part");
        for pad in model.footprint_of(part).expect("a footprint").pads {
            out.push((fp.id.clone(), pad));
        }
    }
    out
}

// ----------------------------------------------------------------------------------------------------------------------
// KiCad's own board with padstacks: the reader takes them and the writer writes them back
// ----------------------------------------------------------------------------------------------------------------------

#[test]
fn the_padstacks_of_kicads_qa_board_are_read_and_written_back_block_by_block() {
    let Ok(text) = std::fs::read_to_string(qa_boards_dir().join("padstacks_complex.kicad_pcb")) else {
        eprintln!("skipped: no QA board padstacks_complex.kicad_pcb");
        return;
    };
    let (design, model, notes) = import_kicad_pcb(&text).expect("imports");
    assert_eq!(notes.non_rect_pad_shapes_approximated, 0);
    let pads = pads_of(&model, &design);
    assert_eq!(pads.len(), 4, "four pads");
    let modes: Vec<(String, PadStackMode)> = pads.iter().map(|(_, p)| (p.number.clone(), p.ext.stack.mode)).collect();
    // Pads 3 and 4: a shape on each of the four copper layers; pads 1 and 2: one for the front, one for the inner layers, one for the back.
    for (number, mode) in &modes {
        let expected = if number == "3" || number == "4" { PadStackMode::Custom } else { PadStackMode::FrontInnerBack };
        assert_eq!(*mode, expected, "pad {number}");
    }
    let layers = &model.board.layers;
    assert_eq!(layers.len(), 4, "{layers:?}");
    for (_, pad) in &pads {
        // Every pad of this board keeps its copper on all the layers (`remove_unused_layers no`).
        assert!(!pad.ext.stack.unconnected.removes(), "pad {}", pad.number);
        let groups = pad.shape_groups(eda_model::ir::Side::Top, layers);
        let covered: usize = groups.iter().map(|g| g.layers.len()).sum();
        assert_eq!(covered, 4, "pad {}: the shapes cover the four copper layers: {groups:?}", pad.number);
    }

    // The writer gives the blocks back: read again, every pad has the same stack.
    let out = export(&design, &model);
    assert_eq!(out.matches("(padstack").count(), 4, "four padstack blocks");
    assert_eq!(out.matches("(mode custom)").count(), 2);
    assert_eq!(out.matches("(mode front_inner_back)").count(), 2);
    let (design2, model2, _) = import_kicad_pcb(&out).expect("imports again");
    let mut before = pads_of(&model, &design);
    let mut after = pads_of(&model2, &design2);
    before.sort_by(|a, b| a.1.number.cmp(&b.1.number));
    after.sort_by(|a, b| a.1.number.cmp(&b.1.number));
    for ((_, a), (_, b)) in before.iter().zip(&after) {
        assert_eq!(a.ext.stack, b.ext.stack, "pad {}", a.number);
        assert_eq!((a.shape, a.size), (b.shape, b.size), "pad {}", a.number);
    }
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

/// kicad-cli's plot of one layer of a board: the SVG.
fn plot_board(cli: &Path, board: &Path, layer: &str, dir: &Path, name: &str) -> String {
    let svg = dir.join(format!("{name}.svg"));
    let out = Command::new(cli).args(["pcb", "export", "svg", "--layers", layer, "--mode-single", "--exclude-drawing-sheet", "-o"]).arg(&svg).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::read_to_string(&svg).expect("an svg")
}

/// The numbers in the text of a path's `d`: its x and y, in the order they come.
fn path_numbers(d: &str) -> Vec<f64> {
    d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse::<f64>().ok()).collect()
}

fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = element.find(&key)? + key.len();
    element[start..].split('"').next()
}

/// The boxes `[min x, min y, max x, max y]` of the copper that an SVG plot of kicad-cli draws: a circle, a closed path (a polygon) and a stroked
/// line (an oval pad is one: its segment and the stroke's width round it). The circles filled white or black are the drills, drawn on top of the copper
/// (black where the layer's copper is a polygon, white elsewhere): they are not copper.
fn plot_boxes(svg: &str) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    let mut stroke = 0.0f64;
    let mut hole_fill = false;
    for piece in svg.split('<') {
        if let Some(rest) = piece.strip_prefix("g ") {
            let style = attribute(rest, "style").unwrap_or("");
            let fill = style.split("fill:").nth(1).and_then(|t| t.split(';').next()).map(str::trim).unwrap_or("");
            hole_fill = fill.eq_ignore_ascii_case("#FFFFFF") || fill.eq_ignore_ascii_case("#000000");
            stroke = style.split("stroke-width:").nth(1).and_then(|t| t.split(';').next()).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
        } else if let Some(rest) = piece.strip_prefix("circle ") {
            if hole_fill {
                continue;
            }
            let num = |n: &str| attribute(rest, n).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let (cx, cy, r) = (num("cx"), num("cy"), num("r"));
            out.push([cx - r, cy - r, cx + r, cy + r]);
        } else if let Some(rest) = piece.strip_prefix("path ") {
            let Some(d) = attribute(rest, "d") else { continue };
            let nums = path_numbers(d);
            let pts: Vec<(f64, f64)> = nums.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
            if pts.is_empty() {
                continue;
            }
            let closed = d.trim_end().ends_with('Z');
            let grow = if closed { 0.0 } else { stroke / 2.0 };
            // A stroked path is only a pad when it has a stroke of its own width (the group's), not the 0 of the first group.
            if !closed && stroke == 0.0 {
                continue;
            }
            let fold = |f: fn(f64, f64) -> f64, i: usize, start: f64| pts.iter().map(|p| if i == 0 { p.0 } else { p.1 }).fold(start, f);
            out.push([fold(f64::min, 0, f64::INFINITY) - grow, fold(f64::min, 1, f64::INFINITY) - grow, fold(f64::max, 0, f64::NEG_INFINITY) + grow, fold(f64::max, 1, f64::NEG_INFINITY) + grow]);
        }
    }
    out
}

/// The plot of kicad-cli without what is not the board's (its title carries the file's name and the time).
fn plot_without_title(svg: &str) -> String {
    svg.lines().filter(|l| !l.trim_start().starts_with("<title>")).collect::<Vec<_>>().join("\n")
}

/// The box of the copper of each pad on `layer`, as the model has them (a pad's shape on that layer, the outline the DRC measures).
fn model_boxes(design: &Design, model: &ConstraintModel, layer: &str) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for fp in &design.placement.as_ref().expect("a placement").footprints {
        let part = model.part(&fp.id).expect("a part");
        for pad in &model.footprint_of(part).expect("a footprint").pads {
            let centre = eda_model::footprint::to_board(fp, pad.at);
            for group in pad.shape_groups(fp.side, &model.board.layers) {
                if !group.layers.iter().any(|l| l == layer) {
                    continue;
                }
                let polys = eda_drc::pad_shape::pad_polygons(fp, &group.shape.lay_on(pad), centre, 5);
                if polys.is_empty() || polys.area() <= 0.0 {
                    continue;
                }
                let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                for i in 0..polys.outline_count() {
                    for p in polys.outline(i) {
                        let (x, y) = (p.x as f64 / 1000.0, p.y as f64 / 1000.0);
                        lo = [lo[0].min(x), lo[1].min(y)];
                        hi = [hi[0].max(x), hi[1].max(y)];
                    }
                }
                out.push([lo[0], lo[1], hi[0], hi[1]]);
            }
        }
    }
    out
}

/// Every box of `ours` has one of `theirs` within `tolerance` mm on all its numbers, and none is left over: the two lists are the same set of boxes.
fn assert_same_boxes(what: &str, ours: &[[f64; 4]], theirs: &[[f64; 4]], tolerance: f64) {
    let mut left: Vec<[f64; 4]> = theirs.to_vec();
    for b in ours {
        let Some(i) = left.iter().position(|t| t.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)) else {
            panic!("{what}: the model has copper {b:?} that kicad-cli does not plot (kicad-cli plots {theirs:?})");
        };
        left.remove(i);
    }
    assert!(left.is_empty(), "{what}: kicad-cli plots copper the model does not have: {left:?} (the model has {ours:?})");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda_pad_details_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    dir
}

/// KiCad's own board with padstacks (`padstacks_complex`: two pads with a shape for each of the four copper layers, two with one for the front, the
/// inner and the back ones): the box of the copper kicad-cli plots on each layer is the one the model has, and the board we export plots as the original.
#[test]
fn kicad_cli_plots_each_layer_of_the_qa_boards_padstacks_as_the_model_has_them_and_as_our_export_does() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let Ok(text) = std::fs::read_to_string(qa_boards_dir().join("padstacks_complex.kicad_pcb")) else {
        eprintln!("skipped: no QA board padstacks_complex.kicad_pcb");
        return;
    };
    let dir = scratch("qa");
    let original = dir.join("original.kicad_pcb");
    std::fs::write(&original, &text).expect("write");
    let (design, model, _) = import_kicad_pcb(&text).expect("imports");
    let exported = dir.join("exported.kicad_pcb");
    std::fs::write(&exported, export(&design, &model)).expect("write");

    for layer in ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"] {
        let tag = layer.replace('.', "_");
        let before = plot_board(&cli, &original, layer, &dir, &format!("original_{tag}"));
        let after = plot_board(&cli, &exported, layer, &dir, &format!("exported_{tag}"));
        let theirs = plot_boxes(&before);
        let ours = model_boxes(&design, &model, layer);
        eprintln!("{layer}: kicad-cli plots {} shapes, the model has {}", theirs.len(), ours.len());
        assert!(!theirs.is_empty(), "{layer}: kicad-cli draws copper");
        assert_same_boxes(layer, &ours, &theirs, 0.012);
        // Our export is on the micrometre grid and the QA board's numbers are finer (139.0396 mm), so the two plots differ in the fourth decimal place and no further.
        assert_same_boxes(&format!("{layer} after the round trip"), &plot_boxes(&after), &theirs, 0.002);
    }
    // The layer that differs from the others: the pads 1 and 2 have a different shape on each of the three kinds of layer.
    let sizes: Vec<Vec<[f64; 4]>> = ["F.Cu", "In1.Cu", "B.Cu"].iter().map(|l| model_boxes(&design, &model, l)).collect();
    assert_ne!(sizes[0], sizes[1]);
    assert_ne!(sizes[1], sizes[2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A footprint on the back with padstacks, written as KiCad writes one (its file keeps the shape of the board's front layer in the pad's own fields and
/// the board's back layer in the `B.Cu` block, whichever side the footprint is on): kicad-cli's plot of every copper layer is the model's, the board we
/// export plots as the original, and a through-hole pad that keeps its copper on every layer is flashed on all four.
#[test]
fn a_footprint_on_the_back_has_the_copper_of_its_padstack_on_the_layers_kicad_cli_plots_it() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let text = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(gr_rect (start 0 0) (end 40 20) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
	(footprint "Test:Q1" (layer "B.Cu") (uuid "10000000-0000-0000-0000-000000000001") (at 20 10 0)
		(property "Reference" "Q1" (at 0 0 0) (layer "B.SilkS"))
		(pad "1" thru_hole rect (at -5 0) (size 2 1) (drill 0.5) (layers "*.Cu" "*.Mask") (remove_unused_layers no)
			(padstack (mode front_inner_back)
				(layer "Inner" (shape circle) (size 1.4 1.4))
				(layer "B.Cu" (shape oval) (size 3 1.2)))
			(uuid "20000000-0000-0000-0000-000000000001"))
		(pad "2" thru_hole rect (at 5 0) (size 1.6 1.6) (drill 0.6) (layers "*.Cu" "*.Mask") (remove_unused_layers no)
			(padstack (mode custom)
				(layer "F.Cu" (shape rect) (size 1.6 1.6))
				(layer "In1.Cu" (shape rect) (size 1.8 1.1))
				(layer "In2.Cu" (shape circle) (size 2.2 2.2))
				(layer "B.Cu" (shape rect) (size 2.6 2.4)))
			(uuid "20000000-0000-0000-0000-000000000002"))
		(pad "3" thru_hole circle (at 0 4) (size 1.5 1.5) (drill 0.6) (layers "*.Cu" "*.Mask") (remove_unused_layers no)
			(uuid "20000000-0000-0000-0000-000000000003"))
	)
)
"#;
    let dir = scratch("back");
    let original = dir.join("original.kicad_pcb");
    std::fs::write(&original, text).expect("write");
    let (design, model, _) = import_kicad_pcb(text).expect("imports");
    let exported = dir.join("exported.kicad_pcb");
    std::fs::write(&exported, export(&design, &model)).expect("write");

    for layer in ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"] {
        let tag = layer.replace('.', "_");
        let before = plot_board(&cli, &original, layer, &dir, &format!("original_{tag}"));
        let after = plot_board(&cli, &exported, layer, &dir, &format!("exported_{tag}"));
        let theirs = plot_boxes(&before);
        let ours = model_boxes(&design, &model, layer);
        assert_eq!(theirs.len(), 3, "{layer}: kicad-cli plots the three pads: {theirs:?}");
        assert_same_boxes(layer, &ours, &theirs, 0.012);
        assert_eq!(plot_without_title(&before), plot_without_title(&after), "kicad-cli draws other copper on {layer} after the round trip");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `.kicad_mod` with a padstack through the footprint editor's reader and writer: kicad-cli plots the front and the back copper of the footprint as
/// before (`fp export svg` has no inner layers), and the back shape is not the front one.
#[test]
fn kicad_cli_plots_the_padstack_of_a_library_footprint_before_and_after_the_editors_reader_and_writer() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let original = r#"(footprint "Q" (version 20241229) (generator "pcbnew") (generator_version "9.0") (layer "F.Cu")
	(property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS"))
	(property "Value" "Q" (at 0 3 0) (layer "F.Fab"))
	(pad "1" thru_hole oval (at -3 0) (size 3.048 1.524) (drill 0.762) (layers "*.Cu" "*.Mask") (remove_unused_layers no)
		(padstack (mode front_inner_back)
			(layer "Inner" (shape oval) (size 1.27 1.27))
			(layer "B.Cu" (shape oval) (size 2.54 1.016)))
		(uuid "43d32944-eb49-4135-9a55-229ae4e9215e"))
	(pad "2" thru_hole rect (at 3 0) (size 1.6 1.6) (drill 0.6) (layers "*.Cu" "*.Mask") (remove_unused_layers no)
		(padstack (mode custom)
			(layer "F.Cu" (shape rect) (size 1.6 1.6))
			(layer "In1.Cu" (shape rect) (size 1.8 1.1))
			(layer "B.Cu" (shape circle) (size 2.4 2.4)))
		(uuid "43d32944-eb49-4135-9a55-229ae4e9215f"))
)
"#;
    let parsed = parse_library_footprint(original).expect("the editor's reader takes it");
    assert_eq!(parsed.footprint.pads.len(), 2);
    assert!(parsed.footprint.pads.iter().all(|p| p.stack.mode != PadStackMode::Normal), "both pads keep their padstack");
    let exported = export_kicad_mod(&parsed.footprint);
    assert_eq!(exported.matches("(padstack").count(), 2, "{exported}");

    let dir = scratch("mod");
    let mut plots: Vec<Vec<Vec<[f64; 4]>>> = Vec::new();
    for (name, text) in [("original", original), ("exported", exported.as_str())] {
        let lib = dir.join(format!("{name}.pretty"));
        std::fs::create_dir_all(&lib).expect("a library folder");
        std::fs::write(lib.join("Q.kicad_mod"), text).expect("write");
        let mut layers = Vec::new();
        for layer in ["F.Cu", "B.Cu"] {
            let out_dir = dir.join(format!("{name}_{}_svg", layer.replace('.', "_")));
            std::fs::create_dir_all(&out_dir).expect("an output folder");
            let out = Command::new(&cli).args(["fp", "export", "svg", "--layers", layer, "-o"]).arg(&out_dir).arg(&lib).output().expect("runs kicad-cli");
            assert!(out.status.success(), "kicad-cli on the {name} footprint: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let mut boxes = plot_boxes(&std::fs::read_to_string(out_dir.join("Q.svg")).expect("an svg"));
            // The page of a footprint plot starts at the footprint's own box: put every plot's lowest x and y at 0.
            let (dx, dy) = (boxes.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min), boxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min));
            for b in boxes.iter_mut() {
                *b = [b[0] - dx, b[1] - dy, b[2] - dx, b[3] - dy];
            }
            boxes.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
            layers.push(boxes);
        }
        plots.push(layers);
    }
    assert_eq!(plots[0][0].len(), 2, "two pads on the front: {:?}", plots[0][0]);
    assert_eq!(plots[0][1].len(), 2, "two pads on the back: {:?}", plots[0][1]);
    assert_ne!(plots[0][0], plots[0][1], "the back is not the front");
    for (layer, (a, b)) in ["F.Cu", "B.Cu"].iter().zip(plots[0].iter().zip(&plots[1])) {
        assert_same_boxes(layer, a, b, 0.004);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ----------------------------------------------------------------------------------------------------------------------
// Remove Unused Pads: kicad-cli draws the rings the connectivity says a pad keeps
// ----------------------------------------------------------------------------------------------------------------------

/// Four copper layers, three plated through-hole pads and what connects them: pad 1 (1.6 mm, `remove_unused_layers` with no outside layers kept, net N1) has a track on
/// `In1.Cu` starting at its centre; pad 2 (2 mm, outside layers kept, net N2) has a via of its net from the front to `In2.Cu` at its centre; pad 3 (1.2 mm) keeps every layer.
const UNUSED_LAYERS_BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(net 1 "N1")
	(net 2 "N2")
	(gr_rect (start 0 0) (end 40 20) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
	(footprint "Test:J1" (layer "F.Cu") (uuid "10000000-0000-0000-0000-000000000001") (at 20 10 0)
		(property "Reference" "J1" (at 0 -4 0) (layer "F.SilkS"))
		(pad "1" thru_hole circle (at -5 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask") (remove_unused_layers yes) (keep_end_layers no) (net 1 "N1") (uuid "20000000-0000-0000-0000-000000000001"))
		(pad "2" thru_hole circle (at 5 0) (size 2 2) (drill 0.9) (layers "*.Cu" "*.Mask") (remove_unused_layers yes) (keep_end_layers yes) (net 2 "N2") (uuid "20000000-0000-0000-0000-000000000002"))
		(pad "3" thru_hole circle (at 0 5) (size 1.2 1.2) (drill 0.6) (layers "*.Cu" "*.Mask") (net 1 "N1") (uuid "20000000-0000-0000-0000-000000000003"))
	)
	(segment (start 15 10) (end 15 14) (width 0.25) (layer "In1.Cu") (net 1) (uuid "30000000-0000-0000-0000-000000000001"))
	(via (at 25 10) (size 0.8) (drill 0.4) (layers "F.Cu" "In2.Cu") (net 2) (uuid "40000000-0000-0000-0000-000000000001"))
)
"#;

/// Which of the three pads (by diameter: 1.6, 2.0 and 1.2 mm) kicad-cli's plot of `layer` draws a ring for.
fn rings_plotted(svg: &str) -> Vec<&'static str> {
    let round = |v: f64| (v * 10.0).round() / 10.0;
    let mut out = Vec::new();
    for b in plot_boxes(svg) {
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        if (w - h).abs() > 0.02 {
            continue; // a track
        }
        match round(w) {
            x if x == 1.6 => out.push("J1.1"),
            x if x == 2.0 => out.push("J1.2"),
            x if x == 1.2 => out.push("J1.3"),
            _ => {} // the via
        }
    }
    out.sort();
    out
}

#[test]
fn the_layers_a_pad_keeps_its_ring_on_are_those_kicad_cli_draws_one_on() {
    // The model's side needs no tool.
    let (design, model, _) = import_kicad_pcb(UNUSED_LAYERS_BOARD).expect("imports");
    let flashed = eda_connectivity::flashed_layers(&design, &model);
    assert_eq!(flashed.pads["J1.1"], ["In1.Cu"], "pad 1 removes every layer nothing is connected on: the track reaches it on In1.Cu");
    assert_eq!(flashed.pads["J1.2"], ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"], "pad 2 keeps its outside layers, and the via of its net takes the layers from the front to In2.Cu");
    assert!(!flashed.pads.contains_key("J1.3"), "pad 3 keeps all its layers and has nothing to say");
    // The mode is what the file said, and what the writer says again.
    assert_eq!(model.footprints[0].pads.iter().find(|p| p.number == "1").unwrap().ext.stack.unconnected, eda_model::padstack::UnconnectedLayers::RemoveAll);
    let out = export(&design, &model);
    let pad1 = out.lines().find(|l| l.contains("(pad \"1\"")).expect("pad 1");
    assert!(pad1.contains("(remove_unused_layers yes)") && pad1.contains("(keep_end_layers no)"), "{pad1}");
    let pad2 = out.lines().find(|l| l.contains("(pad \"2\"")).expect("pad 2");
    assert!(pad2.contains("(remove_unused_layers yes)") && pad2.contains("(keep_end_layers yes)"), "{pad2}");
    assert!(!out.lines().find(|l| l.contains("(pad \"3\"")).unwrap().contains("keep_end_layers"), "pad 3 says nothing of it beyond remove_unused_layers no");
}

#[test]
fn kicad_cli_draws_the_rings_of_the_layers_the_connectivity_says_a_pad_keeps() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = scratch("flash");
    let original = dir.join("original.kicad_pcb");
    std::fs::write(&original, UNUSED_LAYERS_BOARD).expect("write");
    let (design, model, _) = import_kicad_pcb(UNUSED_LAYERS_BOARD).expect("imports");
    let exported = dir.join("exported.kicad_pcb");
    std::fs::write(&exported, export(&design, &model)).expect("write");
    let flashed = eda_connectivity::flashed_layers(&design, &model);
    for layer in ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"] {
        let tag = layer.replace('.', "_");
        // What the model says each pad keeps.
        let mut ours: Vec<&str> = ["J1.1", "J1.2", "J1.3"].into_iter().filter(|id| flashed.pads.get(*id).map_or(true, |layers| layers.iter().any(|l| l == layer))).collect();
        ours.sort();
        let theirs = rings_plotted(&plot_board(&cli, &original, layer, &dir, &format!("original_{tag}")));
        assert_eq!(ours, theirs, "{layer}: the rings the model keeps against the ones kicad-cli draws");
        // And the board we write is drawn the same.
        assert_eq!(rings_plotted(&plot_board(&cli, &exported, layer, &dir, &format!("exported_{tag}"))), theirs, "{layer}: our export");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
