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
    // r3 F7: on a back footprint turned by -90 the Description field `FOOTPRINT::FOOTPRINT` made stays on F.Fab and turns with the footprint (`issue12609`'s C1).
    let back = text.replace(r#"(footprint "Conn:X" (layer "F.Cu")"#, r#"(footprint "Conn:X" (layer "B.Cu")"#).replace("(at 110 110)\n", "(at 110 110 -90)\n").replace(r#"(layers "F.Cu" "F.Paste" "F.Mask")"#, r#"(layers "B.Cu" "B.Paste" "B.Mask")"#);
    assert!(back.contains("(at 110 110 -90)"), "{back}");
    let (design, model, _) = import_kicad_pcb(&back).expect("parses");
    let out = export(&design, &model);
    assert!(out.contains(r#"(property "Description" "Generic connector" (at 0 0 270) (layer "F.Fab") (hide yes)"#), "{out}");
}

/// F6: a mask-only mounting hole keeps its mask margin (`issue5990`'s `MH1`: the mask opening is 5 mm + 2 x 1.2 mm), its zone connection and a one-sided layer set, as
/// `format( const PAD* )` writes the pad's own.
#[test]
fn a_mask_only_hole_keeps_its_margin_and_layers() {
    let text = board(&format!(
        r#"{FP_HEAD}  (attr exclude_from_pos_files)
  (pad "" np_thru_hole circle (at 0 0) (size 5 5) (drill 2.7) (layers "*.Mask") (solder_mask_margin 1.2) (clearance 1.2) (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1"))
  (pad "" np_thru_hole circle (at 6 0) (size 3 3) (drill 3) (layers "F.Mask") (zone_connect 0) (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac2")))"#
    ));
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let pads = blocks(&out, "pad");
    assert_eq!(pads.len(), 2, "{out}");
    assert!(pads.iter().any(|p| p.contains(r#"(layers "*.Mask") (solder_mask_margin 1.2) (clearance 1.2)"#)), "{out}");
    assert!(pads.iter().any(|p| p.contains(r#"(layers "F.Mask") (zone_connect 0)"#)), "{out}");
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI") {
        return Some(p.into());
    }
    let out = std::process::Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then(|| std::path::PathBuf::from(p))
}

/// Our file of `text`, as kicad-cli writes it back (`pcb upgrade --force`).
fn kicad_resave(text: &str, tag: &str) -> Option<String> {
    let cli = kicad_cli()?;
    let dir = std::env::temp_dir().join(format!("eda_board_io_round2_{}", std::process::id())).join(tag);
    std::fs::create_dir_all(&dir).unwrap();
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let path = dir.join("ours.kicad_pcb");
    std::fs::write(&path, export(&design, &model)).unwrap();
    let up = std::process::Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}", String::from_utf8_lossy(&up.stderr));
    Some(std::fs::read_to_string(&path).unwrap())
}

const KICAD10_VIAS: &str = r#"(kicad_pcb (version 20260410) (generator "pcbnew") (generator_version "10.0")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (25 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0) (tenting (front yes) (back yes)) (covering (front yes) (back no)) (plugging (front no) (back no)) (capping no) (filling yes))
 (net 0 "") (net 1 "A")
 (gr_rect (start 90 90) (end 130 130) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "11111111-1111-1111-1111-111111111111"))
 (via (at 100 100) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "22222222-2222-2222-2222-222222222222"))
 (via (at 110 100) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "33333333-3333-3333-3333-333333333333"))
)"#;

/// F1: a board whose setup has KiCad 10's via protection defaults is written as 20250228, the first version whose vias that say nothing inherit them (`parsePCB_VIA`:
/// `if( m_requiredVersion < 20250228 )` turns them all off). A board without them stays KiCad 9's 20241229.
#[test]
fn a_board_with_via_protection_defaults_is_written_as_a_version_whose_vias_inherit_them() {
    let (design, model, _) = import_kicad_pcb(KICAD10_VIAS).expect("parses");
    let out = export(&design, &model);
    assert!(out.contains("(version 20250228)") && out.contains("(covering (front yes) (back no))"), "{out}");
    let (plain, plain_model, _) = import_kicad_pcb(&board("")).expect("parses");
    assert!(export(&plain, &plain_model).contains("(version 20241229)"));
}

/// F1, as kicad-cli reads it: its re-save of our file has no protection on a via of its own (it inherits the board's), as its re-save of the original has none.
#[test]
fn a_kicad10_via_keeps_inheriting_the_board_protection() {
    if slow_tests_off() {
        return;
    }
    let Some(back) = kicad_resave(KICAD10_VIAS, "vias") else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let vias = blocks(&back, "via");
    assert_eq!(vias.len(), 2, "{back}");
    for v in vias {
        assert!(!v.contains("(covering") && !v.contains("(filling") && !v.contains("(capping") && !v.contains("(plugging"), "{v}");
    }
    assert!(back.contains("(filling yes)"), "the board's default is kept: {back}");
}

/// F1, the reference image: a 96 dpi PNG of a 20261017 file at scale 1 is scale 1 in kicad-cli's re-save of our 20241229 file (KiCad scales an older file's image by
/// `GetPPI() / GetLegacyPPI()`, 96 / 94, `parsePCB_REFERENCE_IMAGE`), and an older file's image keeps its size too.
#[test]
fn reference_image_scale_survives_a_kicad10_round_trip() {
    if slow_tests_off() {
        return;
    }
    let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(40, 40, [10, 200, 30], 96));
    let image = |version: i64, scale: &str| {
        format!(
            r#"(kicad_pcb (version {version}) (generator "pcbnew") (generator_version "10.0")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (17 "Dwgs.User" user "User.Drawings"))
 (setup (pad_to_mask_clearance 0))
 (net 0 "")
 (image (at 100 100) (layer "Dwgs.User"){scale} (uuid "44444444-4444-4444-4444-444444444444") (data "{data}"))
)"#
        )
    };
    let Some(back) = kicad_resave(&image(20261017, ""), "image_new") else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let img = blocks(&back, "image");
    assert_eq!(img.len(), 1, "{back}");
    assert!(!img[0].contains("(scale"), "{}", &img[0][..img[0].len().min(200)]);
    // A 20241229 file's image at scale 2 is KiCad's 2 x 96 / 94; it comes back as that.
    let back = kicad_resave(&image(20241229, " (scale 2)"), "image_old").unwrap();
    let img = blocks(&back, "image");
    assert!(img[0].contains("(scale 2.04255)"), "{}", &img[0][..img[0].len().min(200)]);
}

/// F8: `format( const ZONE* )` writes `(fill yes` only for a zone that is filled (`IsFilled()`). A board written with no fill of kicad-cli's says its zones are unfilled.
#[test]
fn an_unfilled_zone_is_written_unfilled() {
    let text = board(
        r#" (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "77777777-7777-7777-7777-777777777777") (hatch edge 0.5)
  (connect_pads (clearance 0.5)) (min_thickness 0.25) (filled_areas_thickness no)
  (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
  (polygon (pts (xy 95 95) (xy 125 95) (xy 125 125) (xy 95 125))))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let zones = blocks(&out, "zone");
    assert_eq!(zones.len(), 1, "{out}");
    assert!(zones[0].contains("(fill (thermal_gap") && !zones[0].contains("(fill yes"), "{}", zones[0]);
}

/// F9: a footprint with no `(attr ..)` has no attributes (`parseFOOTPRINT_unchecked`), and is written with none, not with the kind its pads would give.
#[test]
fn no_attr_means_no_attr() {
    let text = board(&format!(
        r#"{FP_HEAD}  (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1")))"#
    ));
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    assert!(!out.contains("(attr"), "{out}");
}

/// F10: a legacy `(pcbplotparams (viasonmask true))` is the vias untented (`parseSETUP`, `case T_pcbplotparams`); `false` is them tented.
#[test]
fn viasonmask_true_untents_the_vias() {
    let legacy = |on: &str| {
        format!(
            r#"(kicad_pcb (version 20230620) (generator pcbnew)
 (general (thickness 1.6)) (paper "A4")
 (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0) (pcbplotparams (layerselection 0x00010fc_ffffffff) (viasonmask {on}) (outputdirectory "")))
 (net 0 "")
)"#
        )
    };
    let (design, model, _) = import_kicad_pcb(&legacy("true")).expect("parses");
    assert!(export(&design, &model).contains("(tenting (front no) (back no))"));
    let (design, model, _) = import_kicad_pcb(&legacy("false")).expect("parses");
    assert!(export(&design, &model).contains("(tenting (front yes) (back yes))"));
}

/// F13: a footprint that names no Reference or Value keeps KiCad's empty ones (`FOOTPRINT::FOOTPRINT`'s mandatory fields, at its origin, 1.27 mm), not a made-up `FP1`.
#[test]
fn a_footprint_with_no_reference_keeps_an_empty_one() {
    let text = board(
        r#" (footprint "TestFP1" (layer "F.Cu") (at 100 100) (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
  (pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu") (net 1 "A") (uuid "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let props = blocks(&out, "property");
    let reference = props.iter().find(|p| p.starts_with("(property \"Reference\"")).unwrap_or_else(|| panic!("{out}"));
    assert!(reference.starts_with(r#"(property "Reference" "" (at 0 0 0) (layer "F.SilkS")"#) && reference.contains("(size 1.27 1.27)"), "{reference}");
    let value = props.iter().find(|p| p.starts_with("(property \"Value\"")).unwrap();
    assert!(value.starts_with(r#"(property "Value" "" (at 0 0 0) (layer "F.Fab")"#), "{value}");
}

/// F8: a zone the file says is filled with nothing (`(fill yes ..)`, no `(filled_polygon ..)`) is filled all the same, and comes back so with the fills the import keeps.
#[test]
fn a_zone_filled_with_nothing_stays_filled() {
    let text = board(
        r#" (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "77777777-7777-7777-7777-777777777777") (hatch edge 0.5)
  (connect_pads (clearance 0.5)) (min_thickness 0.25) (filled_areas_thickness no)
  (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))
  (polygon (pts (xy 95 95) (xy 125 95) (xy 125 125) (xy 95 125))))"#,
    );
    let (mut design, model, notes) = import_kicad_pcb(&text).expect("parses");
    design.assign_missing_ids();
    let entries = eda_kicad::zone_fills::entries_of_imported_file(&design, notes.zone_fills.clone());
    assert_eq!(entries.len(), 1, "{:?}", notes.zone_fills);
    let fills = eda_kicad::zone_fills::KicadFills::new("import".into(), String::new(), 0, entries);
    let out = eda_kicad::export_kicad_pcb_with(&design, &model, &ExportMeta { date: "2026-01-01", title: "round2" }, Some(&fills)).expect("export");
    let zones = blocks(&out, "zone");
    assert!(zones[0].contains("(fill yes (thermal_gap") && !zones[0].contains("filled_polygon"), "{}", zones[0]);
}

/// F8: `ZONE::IsFilled` is a flag of its own: a zone whose file has its rings but says `(fill (thermal_gap ..))` without `yes` (`issue14559`'s 339) is written with the rings
/// and without `yes`, as `format( const ZONE* )` writes it.
#[test]
fn a_zone_with_rings_the_file_says_is_unfilled_stays_unfilled() {
    let text = board(
        r#" (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "77777777-7777-7777-7777-777777777777") (hatch edge 0.5)
  (connect_pads (clearance 0.5)) (min_thickness 0.25) (filled_areas_thickness no)
  (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
  (polygon (pts (xy 95 95) (xy 125 95) (xy 125 125) (xy 95 125)))
  (filled_polygon (layer "F.Cu") (pts (xy 96 96) (xy 124 96) (xy 124 124) (xy 96 124))))"#,
    );
    let (mut design, model, notes) = import_kicad_pcb(&text).expect("parses");
    design.assign_missing_ids();
    let fills = eda_kicad::zone_fills::KicadFills::new("import".into(), String::new(), 0, eda_kicad::zone_fills::entries_of_imported_file(&design, notes.zone_fills.clone()));
    let out = eda_kicad::export_kicad_pcb_with(&design, &model, &ExportMeta { date: "2026-01-01", title: "round2" }, Some(&fills)).expect("export");
    let zones = blocks(&out, "zone");
    assert!(zones[0].contains("(fill (thermal_gap") && zones[0].contains("(filled_polygon"), "{}", zones[0]);
}

// ------------------------------------------------------------------------------------------------- round 3 (`work/findings/round3/r3-board-io.md`)

const THIEVING: &str = r#"(kicad_pcb (version 20260513) (generator "pcbnew") (generator_version "10.99")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0))
 (gr_rect (start 90 90) (end 130 130) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "11111111-1111-1111-1111-111111111111"))
 (zone (net 0) (net_name "") (layer "F.Cu") (uuid "44444444-4444-4444-4444-444444444444") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
  (fill yes (mode thieving) (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1)
   (thieving (type hatch) (size 5) (gap 2) (width 0.3) (stagger yes) (orientation 33)))
  (polygon (pts (xy 95 95) (xy 125 95) (xy 125 125) (xy 95 125))))
)"#;

/// r3 F2: `parseZONE`'s `T_thieving` (`SetFillMode( ZONE_FILL_MODE::COPPER_THIEVING )`, the settings block, "thieving requires file version >= 20260513"): a copper
/// thieving zone (QA 25ec2335 `zone_thieving_track_bisection`) is written back thieving, with its settings, in a 20260513 file -- not as a solid pour.
#[test]
fn a_thieving_zone_is_kept() {
    let (design, model, _) = import_kicad_pcb(THIEVING).expect("parses");
    let out = export(&design, &model);
    assert!(out.contains("(version 20260513)"), "{out}");
    let zones = blocks(&out, "zone");
    assert_eq!(zones.len(), 1, "{out}");
    assert!(zones[0].contains("(mode thieving)") && zones[0].contains("(thieving (type hatch) (size 5) (gap 2) (width 0.3) (stagger yes) (orientation 33))"), "{}", zones[0]);
    assert!(zones[0].contains("(net 0)") || !zones[0].contains("(net_name \"0\")"), "the netless pour stays netless: {}", zones[0]);
}

/// r3 F2, as kicad-cli reads our file: its re-save still has the thieving zone and its settings.
#[test]
fn kicad_cli_reads_the_thieving_zone_back() {
    if slow_tests_off() {
        return;
    }
    let Some(back) = kicad_resave(THIEVING, "thieving") else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let flat = back.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("(mode thieving)") && flat.contains("(thieving (type hatch) (size 5) (gap 2) (width 0.3) (stagger yes) (orientation 33)"), "{back}");
}

/// r3 F3: three vias of one net at one point (25ec2335 `multichannel_nested`): each keeps a uuid of its own, so the group that names all three keeps three members
/// (`resolveGroups` finds one item per uuid).
#[test]
fn vias_at_one_point_get_distinct_uuids() {
    let text = board(
        r#" (via (at 100 100) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "a0000000-0000-0000-0000-000000000001"))
 (via (at 100 100) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "a0000000-0000-0000-0000-000000000002"))
 (via (at 100 100) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "a0000000-0000-0000-0000-000000000003"))
 (group "G" (uuid "b0000000-0000-0000-0000-000000000001") (members "a0000000-0000-0000-0000-000000000001" "a0000000-0000-0000-0000-000000000002" "a0000000-0000-0000-0000-000000000003"))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let mut uuids: Vec<String> = blocks(&out, "via").iter().map(|v| v.split("(uuid \"").nth(1).unwrap().split('"').next().unwrap().to_string()).collect();
    assert_eq!(uuids.len(), 3, "{out}");
    uuids.sort();
    uuids.dedup();
    assert_eq!(uuids.len(), 3, "three distinct uuids: {out}");
    let (again, _, _) = import_kicad_pcb(&out).expect("parses ours");
    let groups = &again.drawings.as_ref().expect("drawings").groups;
    assert_eq!(groups.iter().find(|g| g.name == "G").map(|g| g.member_ids.len()), Some(3), "{groups:?}");
}

/// r3 F4: the bare `locked` a file before 20221130 writes after a group's name (8303b2ad `reverse_via`: `(group "" locked (id ..) (members ..))`) is the group's lock,
/// as `parseGROUP` reads it; a group named "locked" is not locked.
#[test]
fn a_legacy_group_lock_is_read() {
    let text = board(
        r#" (gr_line (start 100 100) (end 110 100) (stroke (width 0.1) (type solid)) (layer "F.SilkS") (uuid "c0000000-0000-0000-0000-000000000001"))
 (gr_line (start 100 110) (end 110 110) (stroke (width 0.1) (type solid)) (layer "F.SilkS") (uuid "c0000000-0000-0000-0000-000000000002"))
 (group "" locked (id "b0000000-0000-0000-0000-000000000001") (members "c0000000-0000-0000-0000-000000000001"))
 (group "locked" (id "b0000000-0000-0000-0000-000000000002") (members "c0000000-0000-0000-0000-000000000002"))"#,
    );
    let (design, _, _) = import_kicad_pcb(&text).expect("parses");
    let d = design.drawings.as_ref().expect("drawings");
    assert_eq!(d.groups.len(), 2, "{:?}", d.groups);
    let id_of = |name: &str| d.groups.iter().find(|g| g.name == name).unwrap().id.clone();
    assert!(d.locked_ids.contains(&id_of("")), "{:?}", d.locked_ids);
    assert!(!d.locked_ids.contains(&id_of("locked")), "{:?}", d.locked_ids);
}

/// 25ec2335's QA board `name`, if the KiCad 10 source is here (`KICAD10_SRC_DIR`, else `~/src/kicad-src-25ec2335`).
fn kicad10_qa(name: &str) -> Option<String> {
    let root = std::env::var_os("KICAD10_SRC_DIR").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("src/kicad-src-25ec2335")))?;
    std::fs::read_to_string(root.join("qa/data/pcbnew").join(name)).ok()
}

/// r3 F5: a generator nothing here reads (KiCad 10's microvia stack, 25ec2335 `via_stacks`) names its members by the uuids our file writes them as, so kicad-cli's
/// `resolveGroups` keeps it: its re-save of our file has both stacks, with their 2 and 3 vias.
#[test]
fn an_unread_generator_keeps_its_members() {
    let Some(text) = kicad10_qa("via_stacks.kicad_pcb") else {
        eprintln!("skipped: no 25ec2335 QA data");
        return;
    };
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let item_uuids: Vec<String> = out.split("(uuid \"").skip(1).map(|r| r.split('"').next().unwrap().to_string()).collect();
    let gens = blocks(&out, "generated");
    let stacks: Vec<&&str> = gens.iter().filter(|g| g.contains("(type via_stack)")).collect();
    assert_eq!(stacks.len(), 2, "{out}");
    let members = |g: &str| -> Vec<String> { g.split("(members").nth(1).unwrap_or("").split('"').skip(1).step_by(2).map(str::to_string).collect() };
    let counts: Vec<usize> = stacks.iter().map(|g| members(g).len()).collect();
    assert_eq!(counts, vec![2, 3], "{stacks:?}");
    for g in &stacks {
        assert!(members(g).iter().all(|u| item_uuids.contains(u)), "every member is an item of the file: {g}");
    }
    if slow_tests_off() {
        return;
    }
    let Some(back) = kicad_resave(&text, "via_stacks") else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let flat = back.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(flat.matches("(type via_stack)").count(), 2, "{back}");
}

/// r3 F6: a footprint read from a board that has no `(model ..)` keeps none (`format( FOOTPRINT )` writes only `aFootprint->Models()`): no header model is guessed from a
/// name with "1x" and digits (`fill_bad`'s `SolderWirePad_1x01_SMD_2.9x7.8mm#2` got `PinHeader_1x02`).
#[test]
fn an_imported_footprint_without_models_stays_without() {
    let text = board(
        r#" (footprint "X:Foo_1x03" (layer "F.Cu") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa") (at 110 110)
  (property "Reference" "J1" (at 0 -2 0) (layer "F.SilkS") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab1") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "X" (at 0 2 0) (layer "F.Fab") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab2") (effects (font (size 1 1) (thickness 0.15))))
  (pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "A") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1"))
  (pad "2" thru_hole circle (at 2.54 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac2"))
  (pad "3" thru_hole circle (at 5.08 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (uuid "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac3")))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    assert!(!out.contains("(model "), "{out}");
}

/// r3 F10: a zone outline drawn with arcs (`issue22475`'s GND pour, `issue22826`'s round rule areas) is written back with its arcs, as `format( const ZONE* )` writes
/// the chain; once the outline is edited, as points.
#[test]
fn a_zone_outline_keeps_its_arcs() {
    let text = board(
        r#" (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "44444444-4444-4444-4444-444444444444") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
  (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
  (polygon (pts (xy 95 95) (xy 120 95) (arc (start 120 95) (mid 123.535534 96.464466) (end 125 100)) (xy 125 125) (xy 95 125))))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    let zone = blocks(&out, "zone")[0].to_string();
    assert!(zone.contains("(arc (start 120 95) (mid 123.535534 96.464466) (end 125 100))"), "{zone}");
    let mut moved = design.clone();
    for p in &mut moved.routing.as_mut().unwrap().zones[0].outline {
        p.x += eda_model::units::mm(1.0);
    }
    let out = export(&moved, &model);
    let zone = blocks(&out, "zone")[0].to_string();
    assert!(!zone.contains("(arc ") && zone.contains("(xy 96 95)"), "an edited outline is its points: {zone}");
}

/// r2 F15: a user field of a file before 20230620 (`(property "Case" "0805")`, no place of its own) is where KiCad's parser leaves it -- a new `PCB_FIELD`, not turned
/// with the footprint -- which `format( PCB_FIELD )` writes at `p - R(a) p` (QA `simple`'s C1 at (147.91 100.48 -90): `(at 47.43 248.39 0)`); on a back footprint on
/// B.Fab, mirrored (`StyleFromSettings( .., true )`).
#[test]
fn a_legacy_user_field_is_where_kicad_reads_it() {
    let text = r#"(kicad_pcb (version 20220914) (generator pcbnew)
 (general (thickness 1.6)) (paper "A4")
 (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen") (44 "Edge.Cuts" user) (49 "F.Fab" user) (48 "B.Fab" user))
 (setup (pad_to_mask_clearance 0))
 (net 0 "")
 (footprint "C:C" (layer "F.Cu") (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa) (at 147.91 100.48 -90)
  (property "Case" "0805")
  (fp_text reference "C1" (at 0 -2 90) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))) (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab1))
  (fp_text value "X" (at 0 2 90) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))) (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaab2))
  (pad "1" smd rect (at 0 0 90) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (tstamp aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1)))
 (footprint "C:C" (layer "B.Cu") (tstamp bbbbbbbb-aaaa-aaaa-aaaa-aaaaaaaaaaaa) (at 160.325 107.075 180)
  (property "Case" "0603")
  (fp_text reference "C2" (at 0 -2 180) (layer "B.SilkS") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)) (tstamp bbbbbbbb-aaaa-aaaa-aaaa-aaaaaaaaaab1))
  (fp_text value "X" (at 0 2 180) (layer "B.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify mirror)) (tstamp bbbbbbbb-aaaa-aaaa-aaaa-aaaaaaaaaab2))
  (pad "1" smd rect (at 0 0 180) (size 1 1) (layers "B.Cu" "B.Paste" "B.Mask") (tstamp bbbbbbbb-aaaa-aaaa-aaaa-aaaaaaaaaac1)))
)"#;
    let (design, model, _) = import_kicad_pcb(text).expect("parses");
    let out = export(&design, &model);
    let line = |value: &str| blocks(&out, "property").into_iter().find(|l| l.contains(&format!("(property \"Case\" \"{value}\""))).unwrap_or_else(|| panic!("Case {value}: {out}")).split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(line("0805").contains("(at 47.43 248.39 0) (layer \"F.Fab\")"), "{}", line("0805"));
    assert!(!line("0805").contains("mirror"), "{}", line("0805"));
    assert!(line("0603").contains("(at 320.65 214.15 0) (layer \"B.Fab\")") && line("0603").contains("mirror"), "{}", line("0603"));
}

/// r3 F9: a teardrop read from a file keeps its border style (`parseZONE` reads `(hatch full ..)` into it; only the teardrops KiCad makes get `INVISIBLE_BORDER`).
#[test]
fn a_files_teardrop_keeps_its_border_style() {
    let text = board(
        r#" (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "44444444-4444-4444-4444-444444444444") (name "$teardrop_padvia$") (hatch full 0.1) (priority 30000)
  (attr (teardrop (type padvia))) (connect_pads yes (clearance 0)) (min_thickness 0.0254) (filled_areas_thickness no)
  (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
  (polygon (pts (xy 100 100) (xy 101 100) (xy 101 101))))"#,
    );
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    assert!(design.routing.as_ref().unwrap().zones[0].teardrop, "read as a teardrop");
    let out = export(&design, &model);
    assert!(blocks(&out, "zone")[0].contains("(hatch full "), "{out}");
}

/// r3 F12: a footprint whose file names a `(sheetfile)` and no `(sheetname)` gets one `(sheetfile ..)`, the file's.
#[test]
fn a_footprint_has_one_sheetfile() {
    let text = board(&format!("{FP_HEAD}  (sheetfile \"A05-attenuator.kicad_sch\")\n  (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (net 1 \"A\") (uuid \"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaac1\")))"));
    let (design, model, _) = import_kicad_pcb(&text).expect("parses");
    let out = export(&design, &model);
    assert_eq!(out.matches("(sheetfile ").count(), 1, "{out}");
    assert!(out.contains("(sheetfile \"A05-attenuator.kicad_sch\")"), "{out}");
}
