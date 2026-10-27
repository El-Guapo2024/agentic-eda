//! Round 5's board-file findings (`work/findings/round5/r5-board-io.md`): each test reads a small `.kicad_pcb` with `import_kicad_pcb`, writes it back with
//! `export_kicad_pcb` and checks the file says what KiCad's parser read (`pcb_io_kicad_sexpr_parser.cpp`). The slow one (`EDA_SLOW_TESTS=1`) has kicad-cli read the
//! original and ours.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};

fn round_trip(text: &str) -> String {
    let (design, model, _) = import_kicad_pcb(text).expect("import_kicad_pcb");
    export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "round5" }).expect("export_kicad_pcb")
}

/// The reviewer's board: a 20210228 file whose net, pads and silkscreen text use the old overbar notation.
fn old_overbar_board(version: &str) -> String {
    board_with_zone(version, "")
}

const ZONE: &str = r#"(zone (net 1) (net_name "~RESET~") (layer "F.Cu") (tstamp 11111111-1111-1111-1111-11111111111a) (hatch edge 0.508) (connect_pads (clearance 0.5)) (min_thickness 0.254) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 95 95) (xy 105 95) (xy 105 105) (xy 95 105))))"#;

fn board_with_zone(version: &str, zone: &str) -> String {
    format!(
        r#"(kicad_pcb (version {version}) (generator pcbnew)
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "~RESET~")
  (net_class "Default" "" (clearance 0.2) (trace_width 0.25) (via_dia 0.8) (via_drill 0.4) (uvia_dia 0.3) (uvia_drill 0.1) (add_net "~RESET~"))
  (footprint "L:R" (layer "F.Cu") (tedit 0) (tstamp 11111111-1111-1111-1111-111111111111) (at 100 100)
    (fp_text reference "R1" (at 0 -2) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))) (tstamp 11111111-1111-1111-1111-111111111112))
    (fp_text value "R" (at 0 2) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))) (tstamp 11111111-1111-1111-1111-111111111113))
    (fp_text user "~WR~" (at 0 4) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))) (tstamp 11111111-1111-1111-1111-111111111119))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (net 1 "~RESET~") (tstamp 11111111-1111-1111-1111-111111111114))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask") (net 1 "~RESET~") (tstamp 11111111-1111-1111-1111-111111111115)))
  (gr_text "~CS~" (at 100 110) (layer "F.SilkS") (tstamp 11111111-1111-1111-1111-111111111116) (effects (font (size 1 1) (thickness 0.15))))
  (gr_rect (start 90 90) (end 130 120) (layer "Edge.Cuts") (width 0.1) (fill none) (tstamp 11111111-1111-1111-1111-111111111117))
  {zone}
  (segment (start 99 100) (end 101 100) (width 0.25) (layer "F.Cu") (net 1) (tstamp 11111111-1111-1111-1111-111111111118))
)"#
    )
}

/// F1: before 20210606 KiCad's parser converts `~X~` to `~{X}` in the board's net names (`parseNETINFO_ITEM`), a net class's nets (`T_add_net`), a pad's net
/// (`parsePAD`) and every text (`parseEDA_TEXT`). The model has `~{RESET}` with both pins on it and the track, and the texts `~{CS}` and `~{WR}`.
#[test]
fn an_old_boards_overbars_are_kicads_new_notation() {
    let (design, model, _) = import_kicad_pcb(&old_overbar_board("20210228")).expect("import");
    let net = model.nets.iter().find(|n| n.name == "~{RESET}").unwrap_or_else(|| panic!("no ~{{RESET}} net: {:?}", model.nets.iter().map(|n| &n.name).collect::<Vec<_>>()));
    let mut pins = net.pins.clone();
    pins.sort();
    assert_eq!(pins, ["R1.1", "R1.2"]);
    let routing = design.routing.as_ref().expect("routing");
    assert!(routing.tracks.iter().all(|t| t.net == "~{RESET}"), "{:?}", routing.tracks.iter().map(|t| &t.net).collect::<Vec<_>>());
    let out = round_trip(&old_overbar_board("20210228"));
    for want in ["\"~{RESET}\"", "(gr_text \"~{CS}\"", "\"~{WR}\""] {
        assert!(out.contains(want), "{want} missing from:\n{out}");
    }
    assert!(!out.contains("~RESET~") && !out.contains("~CS~") && !out.contains("~WR~"), "{out}");

    // A file of 20210606 or later is read as it is.
    let out = round_trip(&old_overbar_board("20211014"));
    assert!(out.contains("\"~RESET~\"") && out.contains("(gr_text \"~CS~\""), "{out}");
}

/// F2: an outline font's bold text keeps its thickness in a 20260826+ file (`MigrateLegacyBoldStrokeWidth`: "Outline faces never baked bold into the width"); the
/// KiCad font's is widened back. kicad-cli 10.99.0's upgrade of a bold `(thickness 0.2)` writes STROKE 0.125 and OUTLINE 0.2.
#[test]
fn an_outline_fonts_bold_text_keeps_its_thickness() {
    let board = r#"(kicad_pcb (version 20261017) (generator "pcbnew") (generator_version "10.99")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen") (25 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0))
 (gr_text "STROKE" (at 100 100 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111111") (effects (font (size 1 1) (thickness 0.125) (bold yes))))
 (gr_text "OUTLINE" (at 100 110 0) (layer "F.SilkS") (uuid "22222222-2222-2222-2222-222222222222") (effects (font (face "DejaVu Sans") (size 1 1) (thickness 0.2) (bold yes))))
)"#;
    let out = round_trip(board);
    let thickness_of = |text: &str| {
        let at = out.find(&format!("(gr_text \"{text}\"")).unwrap_or_else(|| panic!("{text} missing:\n{out}"));
        let rest = &out[at..];
        let t = rest.find("(thickness ").unwrap();
        rest[t + 11..].split(')').next().unwrap().trim().to_string()
    };
    assert_eq!(thickness_of("STROKE"), "0.2");
    assert_eq!(thickness_of("OUTLINE"), "0.2");
}

/// F6: `parseDIMENSION`'s `T_text_frame`: `textFrame = std::clamp( textFrame, 0, 3 )`, so 7 is a rounded rectangle (3) and -1 none (0, which is not written).
#[test]
fn a_leaders_text_frame_is_clamped_to_the_four_borders() {
    let board = |frame: &str| {
        format!(
            r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
 (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
 (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen") (25 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0))
 (dimension (type leader) (layer "F.SilkS") (uuid "7d932562-7662-47a4-b0f3-f405d4e7fd17") (pts (xy 109.5 89) (xy 113.25 85.75))
  (format (prefix "") (suffix "") (units 0) (units_format 0) (precision 4) (override_value "Leader"))
  (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (text_frame {frame}) (extension_offset 0.5))
  (gr_text "Leader" (at 119 85.75 0) (layer "F.SilkS") (uuid "7d932562-7662-47a4-b0f3-f405d4e7fd17") (effects (font (size 1 1) (thickness 0.1)))))
)"#
        )
    };
    let frame_of = |out: &str| out.find("(text_frame ").map(|i| out[i + 12..].split(')').next().unwrap().to_string());
    assert_eq!(frame_of(&round_trip(&board("7"))).as_deref(), Some("3"));
    assert_eq!(frame_of(&round_trip(&board("2"))).as_deref(), Some("2"));
    let none = round_trip(&board("-1"));
    assert!(matches!(frame_of(&none).as_deref(), None | Some("0")), "{none}");
}

/// `parseZONE`'s `legacyNetnameFromFile`: the zone's `(net_name ..)` is not converted, so it is not the name of its net `~{RESET}`, and the zone goes to a new net
/// `~RESET~` (kicad-cli 10.99.0's upgrade of this file has the zone on `(net "~RESET~")`). The same rule takes a hand-edited zone to the net its name says.
#[test]
fn an_old_zones_net_name_is_not_converted_and_names_its_net() {
    let (design, model, _) = import_kicad_pcb(&board_with_zone("20210228", ZONE)).expect("import");
    let zones: Vec<&str> = design.routing.as_ref().map(|r| r.zones.iter().map(|z| z.net.as_str()).collect()).unwrap_or_default();
    assert_eq!(zones, ["~RESET~"]);
    assert!(model.nets.iter().any(|n| n.name == "~RESET~" && n.pins.is_empty()));
    assert!(model.nets.iter().any(|n| n.name == "~{RESET}" && n.pins.len() == 2));
    // A newer file whose zone says another net's name than its code's.
    let hand = board_with_zone("20211014", &ZONE.replace("(net_name \"~RESET~\")", "(net_name \"GND\")"));
    let (design, _, _) = import_kicad_pcb(&hand).expect("import");
    assert_eq!(design.routing.as_ref().unwrap().zones[0].net, "GND");
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

/// `text` as kicad-cli writes it back (`pcb upgrade --force`).
fn kicad_upgrade(text: &str, dir: &std::path::Path, name: &str) -> String {
    let cli = kicad_cli().expect("kicad-cli");
    let path = dir.join(format!("{name}.kicad_pcb"));
    std::fs::write(&path, text).unwrap();
    let up = std::process::Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}", String::from_utf8_lossy(&up.stderr));
    std::fs::read_to_string(&path).unwrap()
}

/// F1 against kicad-cli: KiCad's own upgrade of the old board and its upgrade of ours have the same net, pads and texts in the new notation.
#[test]
fn kicad_cli_reads_our_old_overbars_as_it_reads_the_original() {
    if slow_tests_off() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("eda_board_io_round5_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let original = old_overbar_board("20210228");
    let theirs = kicad_upgrade(&original, &dir, "theirs");
    let ours = kicad_upgrade(&round_trip(&original), &dir, "ours");
    for file in [&theirs, &ours] {
        for want in ["(net \"~{RESET}\")", "\"~{CS}\"", "\"~{WR}\""] {
            assert!(file.contains(want), "{want} missing from:\n{file}");
        }
        assert_eq!(file.matches("(net \"~{RESET}\")").count(), theirs.matches("(net \"~{RESET}\")").count(), "{file}");
        assert!(!file.contains("~RESET~"), "{file}");
    }
    // The zone: on a new net `~RESET~` in both.
    let original = board_with_zone("20210228", ZONE);
    let theirs = kicad_upgrade(&original, &dir, "theirs_zone");
    let ours = kicad_upgrade(&round_trip(&original), &dir, "ours_zone");
    for file in [&theirs, &ours] {
        let zone = &file[file.find("(zone").expect("a zone")..];
        assert!(zone[..zone.find("(layer").unwrap()].contains("(net \"~RESET~\")"), "{file}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
