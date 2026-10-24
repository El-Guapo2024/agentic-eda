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
