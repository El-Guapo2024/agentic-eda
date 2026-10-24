//! What the library editors' readers keep of a KiCad symbol or footprint that the editors do not edit, and the writers give back
//! (`eda_model::lib_keep`; P17 review). Each case is a divergence the round trip of KiCad's installed libraries found
//! (`tests/library_roundtrip.rs`, slow tier): before it was fixed, the editors' write of the item lost what the case checks.

use eda_kicad::{export_kicad_mod, export_kicad_sym_library, parse_library_footprint, parse_library_symbols};

fn one_symbol(text: &str) -> eda_model::ir::LibrarySymbol {
    let mut parsed = parse_library_symbols(text).expect("parses");
    assert_eq!(parsed.symbols.len(), 1);
    parsed.symbols.remove(0)
}

fn written(sym: &eda_model::ir::LibrarySymbol) -> String {
    export_kicad_sym_library(&[sym])
}

const QUAD: &str = r#"(kicad_symbol_lib (version 20251024) (generator "kicad_symbol_editor")
  (symbol "Q"
    (power local)
    (exclude_from_sim yes) (in_bom yes) (on_board yes) (in_pos_files no)
    (duplicate_pin_numbers_are_jumpers yes)
    (jumper_pin_groups ("2" "1") ("3" "4"))
    (property "Reference" "U" (at 0 0 0) (effects (font (size 1.27 1.27))))
    (property "Value" "Q" (at 0 0 0) (effects (font (size 1.27 1.27))))
    (property private "KLC" "a note" (at 0 0 0) (hide yes) (effects (font (size 1.27 1.27))))
    (property "ki_locked" "" (at 0 0 0) (effects (font (size 1.27 1.27))))
    (symbol "Q_1_0"
      (polyline (pts (xy 0 0) (xy 1 1)) (stroke (width 0) (type dash)) (fill (type none)))
      (bezier (pts (xy 0 0) (xy 1 2) (xy 3 2) (xy 4 0)) (stroke (width 0) (type default)) (fill (type none)))
      (text "!" (at 0 0 0) (effects (font (size 2 2) (thickness 0.238) bold) (justify left bottom)))
      (pin passive line (at 0 0 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
    )
    (symbol "Q_2_1"
      (unit_name "Heater")
      (rectangle (start -1 -1) (end 1 1) (stroke (width 0.254) (type solid)) (fill (type background)))
    )
  )
)"#;

#[test]
fn a_symbols_locked_units_local_power_exclusions_and_jumpers_are_read_and_written_back() {
    let q = one_symbol(QUAD);
    // `ki_locked` (`LIB_SYMBOL::LockUnits`) was read and dropped: a symbol whose units are not interchangeable came back with interchangeable units
    assert!(q.units_locked, "ki_locked");
    assert!(q.power && q.power_local, "(power local)");
    assert!(q.exclude_from_sim && q.exclude_from_pos_files);
    assert!(q.kicad.duplicate_pin_numbers_are_jumpers);
    assert_eq!(q.kicad.jumper_pin_groups, vec![vec!["1".to_string(), "2".to_string()], vec!["3".to_string(), "4".to_string()]]);
    let text = written(&q);
    assert!(text.contains("(power local)"), "a local power symbol written as `(power)` is a global one: {text}");
    assert!(text.contains("(property \"ki_locked\" \"\""), "{text}");
    assert!(text.contains("(exclude_from_sim yes)") && text.contains("(in_pos_files no)") && text.contains("(duplicate_pin_numbers_are_jumpers yes)"), "{text}");
    assert!(text.contains("(jumper_pin_groups (\"1\" \"2\") (\"3\" \"4\"))"), "{text}");
    let back = one_symbol(&text);
    assert_eq!((back.units_locked, back.power_local, back.exclude_from_sim, back.exclude_from_pos_files), (true, true, true, true));
    assert_eq!(back.kicad.jumper_pin_groups, q.kicad.jumper_pin_groups);
}

#[test]
fn a_symbols_private_fields_stroke_styles_text_styles_unit_names_and_beziers_are_kept() {
    let q = one_symbol(QUAD);
    let klc = q.fields.iter().find(|f| f.name == "KLC").expect("the private field");
    assert!(klc.private, "`(property private ..)`");
    let text = written(&q);
    assert!(text.contains("(property private \"KLC\""), "{text}");
    assert!(text.contains("(type dash)"), "the dashed line: {text}");
    assert!(text.contains("(type solid)"), "a `solid` stroke is not `default`: {text}");
    assert!(text.contains("(unit_name \"Heater\")"), "{text}");
    assert!(text.contains("(bezier (pts (xy 0 0) (xy 1 2) (xy 3 2) (xy 4 0))"), "the bezier, kept as written: {text}");
    assert!(text.contains("(thickness 0.238) bold) (justify left bottom)"), "the text's pen, bold and justification: {text}");
    let back = one_symbol(&text);
    assert_eq!(back.kicad, q.kicad, "the second round trip changes nothing");
}

#[test]
fn an_item_common_to_every_body_style_is_written_in_its_units_style_0_sub_symbol() {
    // `SaveSymbol` writes `GetUnitDrawItems` by (unit, body style): an item of body style 0 is in `Q_1_0`, as the file had it, not copied into `Q_1_1`
    let q = one_symbol(QUAD);
    assert!(q.pins.iter().all(|p| p.body_style == 0));
    let text = written(&q);
    assert!(text.contains("(symbol \"Q_1_0\""), "{text}");
    assert!(!text.contains("(symbol \"Q_1_1\""), "unit 1 has no item of body style 1: {text}");
    assert!(text.contains("(symbol \"Q_2_1\""), "{text}");
    let back = one_symbol(&text);
    assert_eq!(back.pins.len(), 1);
    assert_eq!(back.pins[0].body_style, 0);
}

const FP: &str = r#"(footprint "Edge"
  (version 20260206) (generator "pcbnew") (layer "F.Cu")
  (descr "an edge connector")
  (property "Reference" "REF**" (at 0 -2 0) (layer "F.SilkS") (uuid "11111111-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Edge" (at 0 2 0) (layer "F.Fab") (uuid "11111111-0000-4000-8000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
  (property "Datasheet" "https://example.com/edge.pdf" (at 1 1 0) (layer "F.Fab") (hide yes) (uuid "11111111-0000-4000-8000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
  (property "Description" "Edge fingers" (at 0 0 0) (layer "F.Fab") (hide yes) (uuid "11111111-0000-4000-8000-000000000004") (effects (font (size 1.27 1.27))))
  (solder_mask_margin 0.033)
  (clearance 0.04)
  (attr smd allow_soldermask_bridges)
  (net_tie_pad_groups "1, 2")
  (duplicate_pad_numbers_are_jumpers no)
  (fp_line (start -1 0) (end 1 0) (stroke (width 0.1) (type default)) (layer "F.Fab") (uuid "22222222-0000-4000-8000-000000000001"))
  (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.12) (type dash)) (fill no) (layer "F.SilkS") (uuid "22222222-0000-4000-8000-000000000002"))
  (fp_text user "${REFERENCE}" (at 0 0 90) (unlocked yes) (layer "F.Fab") (uuid "33333333-0000-4000-8000-000000000001")
    (effects (font (size 0.8 0.8) (thickness 0.075) bold) (justify bottom)))
  (pad "1" connect rect (at -0.5 0) (size 0.6 2) (layers "F.Cu" "F.Mask") (property pad_prop_testpoint) (solder_mask_margin 0.05) (die_length 1.2) (uuid "44444444-0000-4000-8000-000000000001"))
  (pad "2" smd roundrect (at 0.5 0) (size 0.6 2) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.038462) (uuid "44444444-0000-4000-8000-000000000002"))
  (point (at 3 3) (size 1) (layer "F.Fab") (uuid "55555555-0000-4000-8000-000000000001"))
  (zone (net 0) (net_name "") (layer "F.Cu") (uuid "66666666-0000-4000-8000-000000000001") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
  (group "" (uuid "77777777-0000-4000-8000-000000000001") (members "22222222-0000-4000-8000-000000000001" "44444444-0000-4000-8000-000000000002"))
)"#;

#[test]
fn a_footprints_attributes_margins_net_ties_zones_points_and_groups_are_written_back() {
    let parsed = parse_library_footprint(FP).expect("parses");
    assert!(parsed.warnings.is_empty(), "nothing is left out: {:?}", parsed.warnings);
    let fp = parsed.footprint;
    let text = export_kicad_mod(&fp);
    // `allow_soldermask_bridges` was read and never written
    assert!(text.contains("(attr smd allow_soldermask_bridges)"), "{text}");
    for kept in ["(solder_mask_margin 0.033)", "(clearance 0.04)", "(net_tie_pad_groups \"1, 2\")", "(duplicate_pad_numbers_are_jumpers no)", "(point (at 3 3)", "(zone (net 0)", "(group \"\""] {
        assert!(text.contains(kept), "{kept} is kept: {text}");
    }
    // the group names its members by the uuids the file gave them, which are kept
    assert!(text.contains("(uuid \"22222222-0000-4000-8000-000000000001\")") && text.contains("(uuid \"44444444-0000-4000-8000-000000000002\")"), "{text}");
    let back = parse_library_footprint(&text).expect("parses back").footprint;
    assert_eq!(back.kicad.tokens, fp.kicad.tokens);
    assert_eq!(back.kicad.uuids, fp.kicad.uuids);
}

#[test]
fn a_footprints_edge_connector_pad_fabrication_property_and_pad_margins_are_written_back() {
    let fp = parse_library_footprint(FP).unwrap().footprint;
    let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
    // a `connect` pad was read as an SMD pad and written `smd`
    assert!(p1.kicad.connector);
    assert_eq!(p1.kicad.property.as_deref(), Some("pad_prop_testpoint"));
    assert_eq!(p1.kicad.solder_mask_margin, Some(50_000));
    let text = export_kicad_mod(&fp);
    assert!(text.contains("(pad \"1\" connect rect"), "{text}");
    assert!(text.contains("(property pad_prop_testpoint) (solder_mask_margin 0.05)"), "{text}");
    assert!(text.contains("(die_length 1.2)"), "{text}");
    // `FormatDouble2Str` of the ratio: it was written to four decimals, 0.0385
    assert!(text.contains("(roundrect_rratio 0.038462)"), "{text}");
}

#[test]
fn a_footprints_line_styles_text_looks_and_datasheet_and_description_fields_are_written_back() {
    let fp = parse_library_footprint(FP).unwrap().footprint;
    let text = export_kicad_mod(&fp);
    // every line was written `solid`
    assert!(text.contains("(type default)") && text.contains("(type dash)"), "{text}");
    // the text's bold, its vertical justification and `(unlocked yes)` were lost
    assert!(text.contains("(unlocked yes)"), "{text}");
    assert!(text.contains("(thickness 0.075) bold) (justify bottom)"), "{text}");
    // the Datasheet field was dropped and the Description field taken for the library description
    assert_eq!(fp.description, "an edge connector");
    assert_eq!(fp.datasheet.as_ref().and_then(|l| l.text.as_deref()), Some("https://example.com/edge.pdf"));
    assert_eq!(fp.description_field.as_ref().and_then(|l| l.text.as_deref()), Some("Edge fingers"));
    assert!(text.contains("(property \"Datasheet\" \"https://example.com/edge.pdf\" (at 1 1 0)"), "{text}");
    assert!(text.contains("(property \"Description\" \"Edge fingers\""), "{text}");
    let back = parse_library_footprint(&text).unwrap().footprint;
    assert_eq!((back.datasheet, back.description_field), (fp.datasheet.clone(), fp.description_field.clone()));
    assert_eq!(back.kicad.text_extras, fp.kicad.text_extras);
    assert_eq!(back.kicad.stroke_types, fp.kicad.stroke_types);
}
