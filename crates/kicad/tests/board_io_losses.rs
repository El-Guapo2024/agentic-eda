//! What the board reader and writer used to lose on the way through (`docs/parity/findings`, batch B): each test reads a small `.kicad_pcb` with
//! `import_kicad_pcb`, writes it back with `export_kicad_pcb` and checks that the item is in the design and in the file as KiCad's parser and writer have it
//! (`pcb_io_kicad_sexpr_parser.cpp`, `pcb_io_kicad_sexpr.cpp`).

use eda_model::units::Nm;
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
    Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
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
            (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "a2"))
            (pad "3" smd rect (at 4 0) (size 1 1) (layers "F.Cu" "In1.Cu" "B.Cu" "F.Mask") (uuid "a3")))
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
    assert_eq!(pad_layers("J1", "3"), r#"(layers "F.Cu" "In1.Cu" "B.Cu" "F.Mask")"#, "not every copper layer: listed, not `*.Cu` (`formatLayers`)");
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
        let cx = q.x.clamp(Nm::from_raw_shim(23 * MM), Nm::from_raw_shim(37 * MM));
        let cy = q.y.clamp(Nm::from_raw_shim(23 * MM), Nm::from_raw_shim(27 * MM));
        (((q.x - cx).raw_shim() as f64).hypot((q.y - cy).raw_shim() as f64)) > 3.0 * MM as f64 + 20_000.0
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
    assert!(design.drawings.as_ref().map_or(true, |d| d.shapes.iter().all(|s| s.layer() != "Edge.Cuts" || matches!(s, eda_model::ir::Shape::Rect { .. }) && s.stroke_width() == Nm::from_raw_shim(50_000))), "no footprint edge became a board shape");
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

/// p13-p16 P14-1: KiCad 10's Extruded 3D Body (`(model (type extruded) ..)`, a typed model with no path) is kept as the file wrote it, beside the footprint's
/// ordinary model, so kicad-cli's STEP, GLB and VRML of our board have the body.
#[test]
fn an_extruded_body_survives_import_and_export() {
    let body = r#"(model (type extruded) (overall_height 1.2) (body_pcb_gap 0.1) (layer auto) (material plastic) (color unspecified) (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))"#;
    let text = format!(
        r#"(kicad_pcb (version 20260901) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (footprint "L:R" (layer "F.Cu") (uuid "f1") (at 10 10)
            (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
            (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (uuid "a1"))
            (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask") (uuid "a2"))
            {body}
            (model "${{KICAD10_3DMODEL_DIR}}/Resistor_SMD.3dshapes/R_0603_1608Metric.step" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0))))
    )"#
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let fp = blocks(&out, "footprint")[0];
    assert!(fp.contains(body), "the typed model token for token: {fp}");
    assert!(fp.contains("R_0603_1608Metric.step"), "and the ordinary model: {fp}");

    // A library footprint (`.kicad_mod`) keeps it too.
    let lib = format!(
        r#"(footprint "R" (version 20260901) (generator "pcbnew") (layer "F.Cu")
        (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask"))
        {body})"#
    );
    let parsed = eda_kicad::parse_library_footprint(&lib).expect("parses");
    let written = eda_kicad::export_kicad_mod(&parsed.footprint);
    assert!(written.contains(body), "{written}");
}

/// p13-p16 F6: a KiCad project's own DRC exclusions (`board.design_settings.drc_exclusions`) are the design's, in each form KiCad wrote them (a record of KiCad 10,
/// in proto or camel-case names; a `[ marker, comment ]` pair of KiCad 8 and 9; the bare text of KiCad 7), their items found by the ids the import gave them and named by
/// the uuids the derived board gives them.
#[test]
fn importing_a_project_keeps_its_drc_exclusions() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (net 0 "") (net 1 "A")
        (footprint "L:R" (layer "F.Cu") (uuid "fp-r1") (at 10 10)
            (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS") (uuid "p1"))
            (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (net 1 "A") (uuid "pad-r1-1"))
            (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask") (uuid "pad-r1-2")))
        (segment (start 9 10) (end 9 14) (width 0.05) (layer "F.Cu") (net 1) (uuid "trk-1"))
        (via (at 9 14) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "via-1"))
    )"#;
    let pro = r#"{ "board": { "design_settings": { "drc_exclusions": [
        { "marker": { "error_type": "DRCET_TRACK_WIDTH", "position": { "x_nm": "9000000", "y_nm": "12000000" }, "items": [ { "value": "trk-1" } ] }, "comment": "thin on purpose" },
        { "marker": { "errorType": "DRCET_CLEARANCE", "position": { "xNm": "9000000", "yNm": "10000000" }, "items": [ { "value": "trk-1" }, { "value": "pad-r1-2" } ] } },
        [ "annular_width|9000000|14000000|via-1|00000000-0000-0000-0000-000000000000", "small ring" ],
        "via_dangling|9000000|14000000|via-1|00000000-0000-0000-0000-000000000000",
        { "marker": { "error_type": "DRCET_TRACK_WIDTH", "position": { "x_nm": "1", "y_nm": "1" }, "items": [ { "value": "not-on-the-board" } ] } }
    ] } } }"#;
    let (design, model, notes) = import_kicad_pcb(text).expect("parses");
    let (out, derived) = eda_kicad::export_kicad_pcb_mapped(&design, &model, &ExportMeta { date: "2026-01-01", title: "x" }).expect("export");
    let waived = eda_kicad::drc_exclusions::from_project(pro, &notes.item_ids, &derived);
    let checks: Vec<&str> = waived.iter().map(|e| e.check.as_str()).collect();
    assert_eq!(checks, vec!["annular_width", "clearance", "track_width", "via_dangling"], "the one naming an item the board lacks is left out");
    let width = waived.iter().find(|e| e.check == "track_width").unwrap();
    let track_id = &design.routing.as_ref().unwrap().tracks[0].id;
    assert_eq!((&width.ids, width.positions_nm.clone(), width.comment.as_str()), (&vec![track_id.clone()], vec![[Nm::from_raw_shim(9_000_000), Nm::from_raw_shim(12_000_000)]], "thin on purpose"));
    assert!(out.contains(&format!("(uuid \"{}\")", width.items[0])), "the item is named as the derived board names it");
    let clearance = waived.iter().find(|e| e.check == "clearance").unwrap();
    assert_eq!(clearance.ids, vec![track_id.clone(), "R1.2".to_string()]);
    assert_eq!(waived.iter().find(|e| e.check == "annular_width").unwrap().comment, "small ring");
}

/// The board with a card-edge tab its footprint closes: the board's Edge.Cuts are a U open at the top, the footprint's three lines close it round a tab.
pub const CARD_EDGE: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
    (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
    (net 0 "")
    (gr_line (start 15 0) (end 0 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e1"))
    (gr_line (start 0 0) (end 0 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e2"))
    (gr_line (start 0 30) (end 40 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e3"))
    (gr_line (start 40 30) (end 40 0) (stroke (width 0.06) (type default)) (layer "Edge.Cuts") (uuid "e4"))
    (gr_line (start 40 0) (end 25 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e5"))
    (footprint "L:Tab" (layer "F.Cu") (uuid "f1") (at 20 0)
        (property "Reference" "J1" (at 0 2 0) (layer "F.SilkS") (uuid "p1"))
        (fp_line (start -5 0) (end -5 -5) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t1"))
        (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t2"))
        (fp_line (start 5 -5) (end 5 0) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t3"))
        (pad "1" smd rect (at 0 -3) (size 1 2) (layers "F.Cu" "F.Mask") (uuid "a1")))
)"#;

/// The summary polygon (`placement.outline`, which the Freerouting boundary, the placer and the canvas fit read) is built with the footprint pass and the shapes'
/// styles: a card-edge tab a footprint closes is in it (not KiCad's inferred box round an open outline), and a rounded rectangle has its rounded corners.
#[test]
fn the_summary_outline_has_the_footprints_tab_and_the_rounded_corners() {
    let (design, _, notes) = import_kicad_pcb(CARD_EDGE).expect("parses");
    assert!(notes.outline_errors.is_empty() && !notes.outline_open, "{:?}", notes.outline_errors);
    let ring = &design.placement.as_ref().unwrap().outline;
    let area = |r: &[Point]| (0..r.len()).map(|i| r[i].x.raw_shim() as f64 * r[(i + 1) % r.len()].y.raw_shim() as f64 - r[(i + 1) % r.len()].x.raw_shim() as f64 * r[i].y.raw_shim() as f64).sum::<f64>().abs() / 2e12;
    assert!(ring.iter().any(|q| q.y == Nm::from_raw_shim(-5 * MM)), "the tab is in the summary: {ring:?}");
    assert!((area(ring) - (1200.0 + 50.0)).abs() < 0.01, "{}", area(ring));

    let rounded = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
        (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
        (gr_rect (start 0 0) (end 50 40) (radius 5) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "r"))
        (gr_circle (center 25 20) (end 27 20) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "c"))
    )"#;
    let (design, _, _) = import_kicad_pcb(rounded).expect("parses");
    let ring = &design.placement.as_ref().unwrap().outline;
    assert!(ring.len() > 8 && (area(ring) - (2000.0 - (100.0 - 25.0 * std::f64::consts::PI))).abs() < 0.05, "the rounded corners: {} points, {}", ring.len(), area(ring));
}
