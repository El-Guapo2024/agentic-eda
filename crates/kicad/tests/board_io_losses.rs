//! What the board reader and writer used to lose on the way through (`docs/parity/findings`, batch B): each test reads a small `.kicad_pcb` with
//! `import_kicad_pcb`, writes it back with `export_kicad_pcb` and checks that the item is in the design and in the file as KiCad's parser and writer have it
//! (`pcb_io_kicad_sexpr_parser.cpp`, `pcb_io_kicad_sexpr.cpp`).

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point};

fn export(d: &Design, m: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(d, m, &ExportMeta { date: "2026-01-01", title: "losses" }).expect("export_kicad_pcb")
}

/// The top-level `(tag ...)` blocks of `text` (and those inside footprints) whose head is `tag`, as text.
fn blocks<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let pat = format!("({tag}");
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find(&pat) {
        let start = i + off;
        let after = bytes.get(start + pat.len()).copied().unwrap_or(b' ');
        if !(after == b' ' || after == b'\n' || after == b'\t' || after == b')') {
            i = start + 1;
            continue;
        }
        let mut depth = 0i32;
        let mut end = start;
        for (k, c) in text[start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(&text[start..end]);
        i = end.max(start + 1);
    }
    out
}

fn p(x: i64, y: i64) -> Point {
    Point { x, y }
}

const MM: i64 = 1_000_000;

/// p22 F1: `parseZONE`'s `case T_polygon` hands every `(polygon ...)` to `ZONE::AddPolygon`, which makes the first the outline and the others its holes. The zone
/// stays one zone, the hole is not a zone of its own, and the writer gives back one `(zone` with both `(polygon`s.
#[test]
fn zone_with_a_hole_is_one_zone_with_a_hole() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (net 0 "")
        (gr_rect (start 0 0) (end 20 20) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "e1"))
        (zone (net 0) (net_name "") (layer "F.Cu") (uuid "ko") (hatch edge 0.5)
            (connect_pads (clearance 0))
            (min_thickness 0.25)
            (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
            (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
            (polygon (pts (xy 0 0) (xy 10 0) (xy 10 10) (xy 0 10)))
            (polygon (pts (xy 4 4) (xy 6 4) (xy 6 6) (xy 4 6))))
    )"#;
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let zones = &design.routing.as_ref().expect("routing").zones;
    assert_eq!(zones.len(), 1, "one zone, not one per (polygon): {zones:?}");
    let z = &zones[0];
    assert_eq!(z.outline, vec![p(0, 0), p(10 * MM, 0), p(10 * MM, 10 * MM), p(0, 10 * MM)]);
    assert_eq!(z.holes, vec![vec![p(4 * MM, 4 * MM), p(6 * MM, 4 * MM), p(6 * MM, 6 * MM), p(4 * MM, 6 * MM)]]);
    assert!(z.is_rule_area && z.keepout_tracks);

    let out = export(&design, &model);
    let zs = blocks(&out, "zone");
    assert_eq!(zs.len(), 1, "{out}");
    assert_eq!(blocks(zs[0], "polygon").len(), 2, "the outline and its hole: {}", zs[0]);
    assert!(zs[0].contains("(uuid \"ko\")"), "the zone keeps its uuid: {}", zs[0]);

    // Read back once more: still one zone with its hole.
    let (again, _, _) = import_kicad_pcb(&out).expect("re-reads");
    let back = &again.routing.unwrap().zones;
    assert_eq!((back.len(), back[0].holes.len()), (1, 1));
}

/// p22 F2: an SMD pad on `*.Cu` keeps every copper layer (`PAD::SetLayerSet`), on a footprint of either side, and its paste and mask on both sides; the writer
/// folds the board's copper back into `*.Cu` (`formatLayers`).
#[test]
fn smd_pad_on_all_copper_layers_keeps_them() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "In2.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (net 0 "") (net 1 "A")
        (footprint "L:J" (layer "F.Cu") (uuid "f1") (at 10 10)
            (property "Reference" "J1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "*.Cu" "*.Paste" "*.Mask") (net 1 "A") (uuid "a1"))
            (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "a2")))
        (footprint "L:J" (layer "B.Cu") (uuid "f2") (at 20 10)
            (property "Reference" "J2" (at 0 0 0) (layer "B.SilkS") (uuid "p2"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "*.Cu" "B.Mask") (uuid "b1"))
            (pad "2" smd rect (at -2 0) (size 1 1) (layers "B.Cu" "B.Paste" "B.Mask") (uuid "b2")))
        (segment (start 10 10) (end 10 14) (width 0.2) (layer "In1.Cu") (net 1) (uuid "s1"))
    )"#;
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let out = export(&design, &model);
    let pad_layers = |reference: &str, number: &str| -> String {
        let fp = blocks(&out, "footprint").into_iter().find(|f| f.contains(&format!("\"Reference\" \"{reference}\""))).expect("footprint");
        let pad = blocks(fp, "pad").into_iter().find(|p| p.starts_with(&format!("(pad \"{number}\""))).expect("pad");
        blocks(pad, "layers")[0].to_string()
    };
    assert_eq!(pad_layers("J1", "1"), r#"(layers "*.Cu" "*.Paste" "*.Mask")"#);
    assert_eq!(pad_layers("J1", "2"), r#"(layers "F.Cu" "F.Paste" "F.Mask")"#, "a pad on one side is as before");
    assert_eq!(pad_layers("J2", "1"), r#"(layers "*.Cu" "B.Mask")"#);
    assert_eq!(pad_layers("J2", "2"), r#"(layers "B.Cu" "B.Paste" "B.Mask")"#);
    // Connectivity has the pad on its front-most copper layer, F.Cu, whatever its footprint's side (`CN_LIST::Add( PAD* )`).
    let pads = |id: &str| model.footprint_of(model.part(id).unwrap()).unwrap().pads.clone();
    let fp_side = |id: &str| design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).unwrap().side;
    let on_back = |id: &str, n: &str| pads(id).iter().find(|p| p.number == n).unwrap().on_back(fp_side(id));
    assert_eq!((on_back("J1", "1"), on_back("J2", "1"), on_back("J2", "2")), (false, false, true));
}

/// p7-p8 F2: a pad with no copper (a stencil opening on `F.Paste` alone, a mask aperture) is kept and written back (`parsePAD` keeps every pad), with its
/// primitives, and turned with its footprint.
#[test]
fn a_paste_only_custom_pad_survives_import_and_export() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (net 0 "") (net 1 "GND")
        (footprint "L:H" (layer "F.Cu") (uuid "f1") (at 10 10)
            (property "Reference" "H1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
            (pad "1" smd rect (at 0 0) (size 2 2) (layers "F.Cu" "F.Mask") (net 1 "GND") (uuid "a1"))
            (pad "" smd custom (at 1 1) (size 0.6 0.6) (layers "F.Paste") (options (clearance outline) (anchor circle))
                (primitives (gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (width 0) (fill yes))) (uuid "paste1"))
            (pad "" smd rect (at -1 1) (size 0.5 0.5) (layers "B.Mask") (uuid "mask1")))
    )"#;
    let (mut design, model, _) = import_kicad_pcb(text).expect("parses");
    let out = export(&design, &model);
    let fp = blocks(&out, "footprint")[0];
    let pads = blocks(fp, "pad");
    assert_eq!(pads.len(), 3, "{fp}");
    let paste = pads.iter().find(|p| p.contains("paste1")).expect("the paste pad");
    assert!(paste.contains("(layers \"F.Paste\")") && paste.contains("(primitives (gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1))"), "{paste}");
    assert!(pads.iter().any(|p| p.contains("mask1") && p.contains("(layers \"B.Mask\")")));

    // Turned with the footprint: the angle in the file is the board's.
    design.placement.as_mut().unwrap().footprints[0].rot = 270_000; // 90 degrees in KiCad's sense
    let out = export(&design, &model);
    let paste = blocks(&out, "pad").into_iter().find(|p| p.contains("paste1")).unwrap().to_string();
    assert!(paste.contains("(at 1 1 90)"), "{paste}");
}

/// p12 O1: a `gr_curve` whose control point sits on an end point (KiCad's curve tool leaves it there until dragged) is read with its four points as they are
/// (`parsePCB_SHAPE`, `SetBezierC1`), so the slot it closes stays closed and the curve is written back.
#[test]
fn a_curve_with_a_control_point_on_its_end_is_kept() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (gr_rect (start -10 -10) (end 20 20) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "r"))
        (gr_curve (pts (xy 0 0) (xy 0 0) (xy 3 2) (xy 3 0)) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "c"))
        (gr_line (start 3 0) (end 0 0) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "l"))
    )"#;
    let (design, model, notes) = import_kicad_pcb(text).expect("parses");
    let shapes = &design.drawings.as_ref().unwrap().shapes;
    let curve = shapes.iter().find_map(|s| match s {
        eda_model::ir::Shape::Bezier { start, c1, c2, end, .. } => Some((*start, *c1, *c2, *end)),
        _ => None,
    });
    assert_eq!(curve, Some((p(0, 0), p(0, 0), p(3 * MM, 2 * MM), p(3 * MM, 0))), "{shapes:?}");
    assert!(notes.outline_errors.is_empty(), "the slot is closed: {:?}", notes.outline_errors);
    let out = export(&design, &model);
    assert!(out.contains("(gr_curve (pts (xy 0 0) (xy 0 0) (xy 3 2) (xy 3 0))"), "{out}");
}

/// p12 O3 / p13-p16 F16-1: a board rectangle's `(radius r)` (`SetCornerRadius`) is read, written back after its end (`format( const PCB_SHAPE* )`), and the board
/// outline is the rounded rectangle (`processClosedShape`: `ROUNDRECT( .. ).TransformToPolygon`): 50 x 40 mm less four 5 mm corners is 1978.54 mm^2, KiCad's
/// `export stats` area for it.
#[test]
fn a_board_rectangles_corner_radius_is_read_and_written_back() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
        (gr_rect (start 0 0) (end 50 40) (radius 5) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "r"))
        (gr_rect (start 20 20) (end 40 30) (radius 3) (stroke (width 0.2) (type solid)) (fill hatch) (layer "F.SilkS") (uuid "h"))
    )"#;
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let out = export(&design, &model);
    assert!(out.contains("(gr_rect (start 0 0) (end 50 40) (radius 5)"), "{out}");
    assert!(out.contains("(gr_rect (start 20 20) (end 40 30) (radius 3)"), "{out}");
    let outline = eda_drc::outline::board_outline(&design, true);
    assert!(outline.valid && !outline.inferred);
    let area = outline.polys.area() / 1e12;
    let kicad = 2000.0 - (100.0 - 25.0 * std::f64::consts::PI);
    assert!((area - kicad).abs() < 0.05, "{area} mm^2, KiCad {kicad}");
    // The hatch stays inside the rounded outline: no hatch line reaches the sharp corner's box.
    let hatched = design.drawings.as_ref().unwrap().shapes.iter().find(|s| s.layer() == "F.SilkS").unwrap().clone();
    let lines = eda_drc::hatch::hatching(&design, Some(&model), &hatched, eda_model::pcb_items::HatchKind::Hatch).lines;
    assert!(!lines.is_empty());
    // Outside the rounded rectangle: beyond a corner's arc (centres 3 mm in from each corner), by more than the flattening.
    let outside = |q: Point| {
        let cx = q.x.clamp(23 * MM, 37 * MM);
        let cy = q.y.clamp(23 * MM, 27 * MM);
        (((q.x - cx) as f64).hypot((q.y - cy) as f64)) > 3.0 * MM as f64 + 20_000.0
    };
    assert!(lines.iter().all(|(a, b)| !outside(*a) && !outside(*b)), "a hatch line in a rounded-off corner: {lines:?}");
}

/// p12 O2: a footprint's Edge.Cuts stay in the footprint (`FOOTPRINT::GraphicalItems`) and are written there as `fp_*`; the board outline takes them in with
/// `BuildBoardPolygonOutlines`' footprint pass: a footprint with copper outside its contour makes that contour a hole, subtracted at the end and reported by
/// no chaining error, so a notch across the board's edge cuts the board (KiCad: 2000 - 6 x 2 = 1988 mm^2), and a contour off the board is no second board.
#[test]
fn a_footprints_edge_cuts_are_written_inside_the_footprint_and_cut_the_outline() {
    let board = |fp_at: &str| {
        format!(
            r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (net 0 "")
        (gr_rect (start 0 0) (end 50 40) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts") (uuid "e"))
        (footprint "L:Notch" (layer "F.Cu") (uuid "f1") (at {fp_at})
            (property "Reference" "J1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
            (fp_rect (start -3 -2) (end 3 2) (stroke (width 0.05) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "n1"))
            (pad "1" smd rect (at 0 5) (size 1 1) (layers "F.Cu" "F.Mask") (uuid "a1")))
    )"#
        )
    };
    // A: the notch across the top edge, its pad on the board.
    let (design, model, _) = import_kicad_pcb(&board("25 0")).expect("parses");
    assert!(design.drawings.as_ref().map_or(true, |d| d.shapes.iter().all(|s| s.layer() != "Edge.Cuts" || matches!(s, eda_model::ir::Shape::Rect { .. }) && s.stroke_width() == 50_000)), "no footprint edge became a board shape");
    let out = export(&design, &model);
    let fp = blocks(&out, "footprint")[0];
    assert!(fp.contains("(fp_rect (start -3 -2) (end 3 2)") && fp.contains("(layer \"Edge.Cuts\")"), "{fp}");
    let outline = eda_drc::outline::board_outline_with(&design, Some(&model), true);
    assert!(outline.valid, "{:?}", outline.errors);
    assert!((outline.polys.area() / 1e12 - 1988.0).abs() < 0.01, "{}", outline.polys.area() / 1e12);
    // Moving the footprint moves the notch: it is in the footprint's frame.
    let mut moved = design.clone();
    moved.placement.as_mut().unwrap().footprints[0].at = p(10 * MM, 0);
    let after = eda_drc::outline::board_outline_with(&moved, Some(&model), true);
    assert!((after.polys.area() / 1e12 - 1988.0).abs() < 0.01);
    assert_ne!(after.polys.outline(0), outline.polys.outline(0));

    // B: the contour off the board, its pad outside it: subtracted from nothing, and no second board piece.
    let (design, model, _) = import_kicad_pcb(&board("60 20")).expect("parses");
    let outline = eda_drc::outline::board_outline_with(&design, Some(&model), true);
    assert_eq!(outline.polys.outline_count(), 1);
    assert!((outline.polys.area() / 1e12 - 2000.0).abs() < 0.01, "{}", outline.polys.area() / 1e12);
}
