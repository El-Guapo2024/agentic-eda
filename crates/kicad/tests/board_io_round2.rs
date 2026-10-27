//! What the board reader and writer lost on the way through in round 2 (`work/findings/round2/r2-board-io.md`): each test reads a small `.kicad_pcb` with
//! `import_kicad_pcb`, writes it back with `export_kicad_pcb` and checks the file says what KiCad's parser read (`pcb_io_kicad_sexpr_parser.cpp`) and its writer would
//! write (`pcb_io_kicad_sexpr.cpp`). The slow ones (`EDA_SLOW_TESTS=1`) let kicad-cli read our file back.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::Design;

fn export(d: &Design, m: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(d, m, &ExportMeta { date: "2026-01-01", title: "round2" }).expect("export_kicad_pcb")
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

const HEAD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (9 "F.Adhes" user "F.Adhesive") (11 "B.Adhes" user "B.Adhesive") (13 "F.Paste" user) (15 "B.Paste" user)
  (5 "F.SilkS" user "F.Silkscreen") (7 "B.SilkS" user "B.Silkscreen") (1 "F.Mask" user) (3 "B.Mask" user) (17 "Dwgs.User" user "User.Drawings") (19 "Cmts.User" user "User.Comments")
  (25 "Edge.Cuts" user) (27 "Margin" user) (31 "F.CrtYd" user "F.Courtyard") (29 "B.CrtYd" user "B.Courtyard") (35 "F.Fab" user) (33 "B.Fab" user))
 (setup (pad_to_mask_clearance 0))
 (net 0 "") (net 1 "A")
 (gr_rect (start 90 90) (end 130 130) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "11111111-1111-1111-1111-111111111111"))
"#;

fn board(items: &str) -> String {
    format!("{HEAD}{items}\n)")
}

/// F2: `parsePCB_TRACK` / `parseARC`: `case T_layers: SetLayerSet( parseLayersForCuItemWithSoldermask() )`, and `PCB_TRACK::SetLayerSet` takes the copper layer of the set as
/// the track's layer and a mask layer as `SetHasSolderMask( true )`. `format( PCB_TRACK )` writes it back as `(layers "B.Cu" "B.Mask")` and the margin after it.
#[test]
fn import_reads_a_masked_track_on_its_copper_layer() {
    let text = board(
        r#" (segment (start 100 100) (end 120 100) (width 0.5) (layers "B.Cu" "B.Mask") (solder_mask_margin 0.2) (net 1) (uuid "22222222-2222-2222-2222-222222222222"))
 (arc (start 100 110) (mid 110 105) (end 120 110) (width 0.5) (layers "B.Cu" "B.Mask") (net 1) (uuid "33333333-3333-3333-3333-333333333333"))
 (segment (start 100 125) (end 120 125) (width 0.5) (layer "F.Cu") (net 1) (uuid "66666666-6666-6666-6666-666666666666"))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let tracks = &design.routing.as_ref().expect("routing").tracks;
    assert_eq!(tracks.iter().filter(|t| t.layer == "B.Cu").count(), 2, "{tracks:?}");
    let out = export(&design, &model);
    let segs = blocks(&out, "segment");
    assert!(segs.iter().any(|s| s.contains(r#"(layers "B.Cu" "B.Mask") (solder_mask_margin 0.2) (net 1)"#)), "{out}");
    assert!(segs.iter().any(|s| s.contains(r#"(layer "F.Cu")"#)), "an unmasked track stays `(layer ..)`: {out}");
    let arcs = blocks(&out, "arc");
    assert_eq!(arcs.len(), 1, "{out}");
    assert!(arcs[0].contains(r#"(layers "B.Cu" "B.Mask") (net 1)"#) && !arcs[0].contains("solder_mask_margin"), "{}", arcs[0]);
}

/// F2: a board shape on copper with a mask opening (`PCB_SHAPE::SetLayerSet`, `format( PCB_SHAPE )` line 1094) keeps its copper layer, its net and its mask.
#[test]
fn a_masked_copper_shape_stays_on_its_copper_layer() {
    let text = board(
        r#" (gr_rect (start 100 100) (end 105 105) (stroke (width 0.1) (type solid)) (fill yes) (layers "B.Cu" "B.Mask") (solder_mask_margin 0.3) (net 1) (uuid "44444444-4444-4444-4444-444444444444"))
 (gr_line (start 100 120) (end 110 120) (stroke (width 0.3) (type solid)) (layers "F.Cu" "F.Mask") (net 1) (uuid "55555555-5555-5555-5555-555555555555"))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let rects: Vec<&str> = blocks(&out, "gr_rect").into_iter().filter(|r| !r.contains("Edge.Cuts")).collect();
    assert_eq!(rects.len(), 1, "{out}");
    assert!(rects[0].contains(r#"(layers "B.Cu" "B.Mask") (solder_mask_margin 0.3) (net 1)"#), "{}", rects[0]);
    let lines = blocks(&out, "gr_line");
    assert!(lines.iter().any(|l| l.contains(r#"(layers "F.Cu" "F.Mask") (net 1)"#)), "{out}");
    assert!(!out.contains("Cmts.User\") (net"), "{out}");
}

const FP_HEAD: &str = r#" (footprint "R:X" (layer "F.Cu") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa") (at 110 110)
  (property "Reference" "R1" (at 0 -2 0) (layer "F.SilkS") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab1") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "X" (at 0 2 0) (layer "F.Fab") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab2") (effects (font (size 1 1) (thickness 0.15))))
"#;

/// F4: `format( FOOTPRINT )` writes ` allow_soldermask_bridges` last in `(attr ..)` when `AllowSolderMaskBridges()`; the solder-mask DRC skips the footprint's bridges.
#[test]
fn allow_soldermask_bridges_survives_export() {
    let text = board(&format!(
        r#"{FP_HEAD}  (attr smd allow_soldermask_bridges)
  (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1"))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac2")))"#
    ));
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    assert!(out.contains("(attr smd allow_soldermask_bridges)"), "{out}");
}

/// F3: `format( const PAD* )` writes the pad's own layer set (`formatLayers( aPad->GetLayerSet() )`), so a through-hole pad keeps its paste and a one-sided mask, and an edge
/// connector pad (`connect`) keeps a mask on its own side only.
#[test]
fn a_through_hole_pad_keeps_its_paste_and_one_sided_mask() {
    let text = board(&format!(
        r#"{FP_HEAD}  (attr through_hole)
  (pad "1" thru_hole circle (at -2 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "F.Mask" "*.Paste") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1"))
  (pad "2" connect rect (at 2 0) (size 1 2) (layers "F.Cu" "F.Mask") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac2"))
  (pad "3" thru_hole circle (at 0 2) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac3"))
  (pad "4" connect rect (at 4 0) (size 1 2) (layers "F.Cu" "*.Mask") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac4")))"#
    ));
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let pads = blocks(&out, "pad");
    let pad = |n: &str| *pads.iter().find(|p| p.starts_with(&format!("(pad \"{n}\""))).unwrap_or_else(|| panic!("pad {n}: {out}"));
    assert!(pad("1").contains(r#"(layers "*.Cu" "*.Paste" "F.Mask")"#), "{}", pad("1"));
    assert!(pad("2").contains(r#"(layers "F.Cu" "F.Mask")"#), "{}", pad("2"));
    assert!(pad("3").contains(r#"(layers "*.Cu" "*.Mask")"#), "{}", pad("3"));
    assert!(pad("4").contains(r#"(layers "F.Cu" "F.Mask" "B.Mask")"#), "the default stays: {}", pad("4"));
}

/// F3, a back-side footprint: the pad's set is held as for a top-side one and goes to the footprint's side.
#[test]
fn a_back_side_through_hole_pad_keeps_its_one_sided_mask_on_its_side() {
    let text = board(
        r#" (footprint "R:X" (layer "B.Cu") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa") (at 110 110)
  (property "Reference" "R1" (at 0 -2 0) (layer "B.SilkS") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab1") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
  (property "Value" "X" (at 0 2 0) (layer "B.Fab") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab2") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)))
  (attr through_hole)
  (pad "1" thru_hole circle (at -2 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "B.Mask" "B.Paste") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1")))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let pads = blocks(&out, "pad");
    assert!(pads[0].contains(r#"(layers "*.Cu" "B.Paste" "B.Mask")"#) || pads[0].contains(r#"(layers "*.Cu" "B.Mask" "B.Paste")"#), "{}", pads[0]);
}

/// F5: in a file before 20230620, `parseFOOTPRINT_unchecked` drops `ki_keywords` and `ki_locked`, makes `ki_description` the Description field's text, and hides every
/// field it keeps. Our 20241229 file says so in KiCad 9's own words.
#[test]
fn legacy_ki_properties_become_the_description_field() {
    let text = r#"(kicad_pcb (version 20221018) (generator pcbnew)
 (general (thickness 1.6)) (paper "A4")
 (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen") (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user) (49 "F.Fab" user) (48 "B.Fab" user))
 (setup (pad_to_mask_clearance 0))
 (net 0 "") (net 1 "A")
 (footprint "Conn:X" (layer "F.Cu") (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa) (at 110 110)
  (property "Datasheet" "~")
  (property "ki_description" "Generic connector")
  (property "ki_keywords" "connector")
  (fp_text reference "J1" (at 0 -2) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))) (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab1))
  (fp_text value "X" (at 0 2) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))) (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab2))
  (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1)))
)"#;
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let out = export(&design, &model);
    assert!(!out.contains("ki_description") && !out.contains("ki_keywords"), "{out}");
    assert!(out.contains(r#"(property "Description" "Generic connector" (at 0 0 0) (layer "F.Fab") (hide yes)"#), "{out}");
    assert!(out.contains(r#"(property "Datasheet" "~" (at 0 0 0) (layer "F.Fab") (hide yes)"#), "a legacy field is hidden: {out}");
}
