//! What the library editors' readers keep of a KiCad symbol or footprint that the editors do not edit, and the writers give back
//! (`eda_model::lib_keep`; P17 review). Each case is a divergence the round trip of KiCad's installed libraries found
//! (`tests/library_roundtrip.rs`, slow tier): before it was fixed, the editors' write of the item lost what the case checks.

use eda_model::units::mm;
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

/// UI-LEFT Y2: a bezier is a graphic of the symbol (`SCH_SHAPE` of `SHAPE_T::BEZIER`, `parseSymbolBezier`): its start, two control points and end, its stroke and fill, on its unit
/// and body style -- drawn, edited and moved with the rest, no longer kept aside as the file's text.
#[test]
fn a_bezier_is_a_graphic_with_its_four_points_written_back_by_formatbezier() {
    use eda_model::ir::{LibraryFill, LibrarySymbolGraphic};
    use eda_model::symbol::SPoint;
    let q = one_symbol(QUAD);
    assert!(q.kicad.items.is_empty(), "nothing is kept aside: {:?}", q.kicad.items);
    let b = q.graphics.iter().find(|g| matches!(g, LibrarySymbolGraphic::Bezier { .. })).expect("the bezier is a graphic");
    let LibrarySymbolGraphic::Bezier { unit, body_style, start, c1, c2, end, stroke_mm, fill, .. } = b else { unreachable!() };
    assert_eq!((*unit, *body_style), (1, 0), "Q_1_0");
    assert_eq!((*start, *c1, *c2, *end), (SPoint { x: 0.0, y: 0.0 }, SPoint { x: 1.0, y: 2.0 }, SPoint { x: 3.0, y: 2.0 }, SPoint { x: 4.0, y: 0.0 }));
    assert_eq!((*stroke_mm, *fill), (0.0, LibraryFill::None));
    // written as `formatBezier` writes it, read back the same
    let text = written(&q);
    assert!(text.contains("(bezier (pts (xy 0 0) (xy 1 2) (xy 3 2) (xy 4 0)) (stroke (width 0) (type default)) (fill (type none)))"), "{text}");
    let back = one_symbol(&text);
    assert_eq!(back.graphics.iter().filter(|g| matches!(g, LibrarySymbolGraphic::Bezier { .. })).count(), 1);
    // a placed symbol draws it as `BEZIER_POLY::GetPoly`'s polyline
    let engine = b.to_engine_graphics();
    let eda_model::symbol::SymbolGraphic::Polyline { pts, .. } = &engine[0] else { panic!("{engine:?}") };
    assert!(pts.len() > 4 && pts.first() == Some(start) && pts.last() == Some(end), "{pts:?}");
    // a bezier with fewer points than four is kept as the file wrote it
    let odd = one_symbol(r#"(kicad_symbol_lib (version 20251024) (symbol "B" (symbol "B_1_1" (bezier (pts (xy 0 0) (xy 1 1) (xy 2 0)) (stroke (width 0) (type default)) (fill (type none))))))"#);
    assert_eq!((odd.graphics.len(), odd.kicad.items.len()), (0, 1));
}

/// UI-LEFT Y4: a text's bold, italic and justification are the text's own (`DIALOG_TEXT_PROPERTIES` edits them, Mirror flips the justification); its pen stays in the kept
/// style. A design written before keeps them in the kept style: the writer and every edit take them from there (`absorb_kept_text_styles`).
#[test]
fn a_texts_bold_italic_and_justification_are_the_texts_own_and_an_older_kept_style_still_counts() {
    use eda_model::ir::LibrarySymbolGraphic;
    use eda_model::sym_fields::{FieldHAlign, FieldVAlign};
    let q = one_symbol(QUAD);
    let t = q.graphics.iter().find(|g| matches!(g, LibrarySymbolGraphic::Text { .. })).unwrap();
    let LibrarySymbolGraphic::Text { id, bold, italic, h_align, v_align, .. } = t else { unreachable!() };
    assert_eq!((*bold, *italic, *h_align, *v_align), (true, false, FieldHAlign::Left, FieldVAlign::Bottom));
    let kept = q.kicad.text_styles.get(id).expect("the pen is kept");
    assert_eq!((kept.thickness_mm, kept.bold, kept.justify.is_empty()), (Some(0.238), false, true), "{kept:?}");

    // the same symbol as a design of 2026-10-23 holds it: the looks in the kept style, the text centred and plain
    let mut old = q.clone();
    for g in &mut old.graphics {
        if let LibrarySymbolGraphic::Text { bold, h_align, v_align, .. } = g {
            *bold = false;
            *h_align = FieldHAlign::Center;
            *v_align = FieldVAlign::Center;
        }
    }
    let style = old.kicad.text_styles.get_mut(id).unwrap();
    style.bold = true;
    style.justify = vec!["left".into(), "bottom".into()];
    assert_eq!(written(&old), written(&q), "the writer takes the kept looks");
    old.absorb_kept_text_styles();
    assert_eq!(old, q, "absorbed: the text has them, the kept style its pen");
    let mut again = old.clone();
    again.absorb_kept_text_styles();
    assert_eq!(again, old, "idempotent");
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
    for kept in ["(solder_mask_margin 0.033)", "(clearance 0.04)", "(net_tie_pad_groups \"1, 2\")", "(point (at 3 3)", "(zone (layer \"F.Cu\") (uuid \"66666666-0000-4000-8000-000000000001\")", "(group \"\""] {
        assert!(text.contains(kept), "{kept} is kept: {text}");
    }
    // the margins, the clearance and the net ties are the footprint's settings since UI-LEFT F4 (Footprint Properties edits them); the jumper flag that is off is not
    // written (the file is written as version 20240108, which has no such token, and absent reads as off)
    assert_eq!(fp.settings.solder_mask_margin, Some(eda_model::units::Nm::new(33_000)));
    assert_eq!(fp.settings.clearance, Some(eda_model::units::Nm::new(40_000)));
    assert_eq!(fp.settings.net_tie_pad_groups, vec!["1, 2".to_string()]);
    assert!(!fp.settings.duplicate_pad_numbers_are_jumpers && !text.contains("duplicate_pad_numbers_are_jumpers"), "{text}");
    // the point, the zone and the group are the footprint's items (`FootprintItems`), the zone with no net
    assert_eq!((fp.items.points.len(), fp.items.zones.len(), fp.items.groups.len()), (1, 1, 1));
    assert!(fp.kicad.tokens.iter().all(|t| !t.starts_with("(zone") && !t.starts_with("(point") && !t.starts_with("(group")), "{:?}", fp.kicad.tokens);
    // the group names its members by the uuids the file gave them, which are kept
    assert!(text.contains("(members \"22222222-0000-4000-8000-000000000001\" \"44444444-0000-4000-8000-000000000002\")"), "{text}");
    let back = parse_library_footprint(&text).expect("parses back").footprint;
    assert_eq!(back.kicad.tokens, fp.kicad.tokens);
    assert_eq!(back.kicad.uuids, fp.kicad.uuids);
    assert_eq!(back.items, fp.items);
    assert_eq!(back.settings, fp.settings);
}

#[test]
fn a_footprints_edge_connector_pad_fabrication_property_and_pad_margins_are_written_back() {
    let fp = parse_library_footprint(FP).unwrap().footprint;
    let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
    // a `connect` pad was read as an SMD pad and written `smd`
    assert!(p1.kicad.connector);
    assert_eq!(p1.kicad.property.as_deref(), Some("pad_prop_testpoint"));
    assert_eq!(p1.kicad.solder_mask_margin, Some(mm(0.05)));
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

/// UI-LEFT F4 and F5: a footprint's private layers, jumper settings and paste margins are its settings, and a graphic's hatch and lock, a text's lock and its hidden
/// flag, its looks and keep-upright survive the editor's writer and reader -- and the footprint the board takes from the editor (`to_engine_footprint`) and the one the
/// editor makes of a board's (`from_engine_footprint`) keep the line styles, hatches, the texts' looks and the hidden text.
#[test]
fn a_footprints_settings_hatches_locks_and_hidden_texts_survive_the_writer_and_the_board() {
    const F: &str = r#"(footprint "Looks"
  (version 20260206) (generator "pcbnew") (layer "F.Cu")
  (solder_paste_margin -0.05)
  (solder_paste_margin_ratio -0.1)
  (attr smd)
  (private_layers "User.1" "Eco1.User")
  (duplicate_pad_numbers_are_jumpers yes)
  (jumper_pad_groups ("1" "3") ("2" "4"))
  (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.12) (type dash_dot)) (fill cross_hatch) (locked yes) (layer "F.SilkS") (uuid "22222222-0000-4000-8000-000000000002"))
  (fp_text user "hidden" (at 0 3 0) (layer "F.Fab") (hide yes) (uuid "33333333-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15))))
  (fp_text user "kept" (locked yes) (at 0 -3 45) (unlocked yes) (layer "F.SilkS") (uuid "33333333-0000-4000-8000-000000000002") (effects (font (size 1 1.2) (thickness 0.15) italic) (justify left top)))
  (pad "1" smd rect (at -1 0) (size 0.6 1) (layers "F.Cu" "F.Mask" "F.Paste") (locked yes) (uuid "44444444-0000-4000-8000-000000000001"))
  (pad "2" smd rect (at 1 0) (size 0.6 1) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "44444444-0000-4000-8000-000000000002"))
  (pad "3" smd rect (at -1 2) (size 0.6 1) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "44444444-0000-4000-8000-000000000003"))
  (pad "4" smd rect (at 1 2) (size 0.6 1) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "44444444-0000-4000-8000-000000000004"))
)"#;
    let fp = parse_library_footprint(F).expect("parses").footprint;
    let s = &fp.settings;
    assert_eq!(s.solder_paste_margin, Some(eda_model::units::Nm::new(-50_000)));
    assert_eq!(s.solder_paste_margin_ratio, Some(-0.1));
    assert_eq!(s.private_layers, vec!["User.1".to_string(), "Eco1.User".to_string()]);
    assert!(s.duplicate_pad_numbers_are_jumpers);
    assert_eq!(s.jumper_pad_groups, vec![vec!["1".to_string(), "3".to_string()], vec!["2".to_string(), "4".to_string()]]);
    assert!(fp.kicad.tokens.is_empty(), "every form is the footprint's own: {:?}", fp.kicad.tokens);
    let rect = fp.graphics[0].id().to_string();
    let hidden = fp.texts.iter().find(|t| t.content == "hidden").unwrap().id.clone();
    let kept = fp.texts.iter().find(|t| t.content == "kept").unwrap().id.clone();
    assert_eq!(fp.kicad.hatches.get(&rect), Some(&eda_model::fp_art::Hatch::CrossHatch));
    assert_eq!(fp.kicad.stroke_types.get(&rect).map(String::as_str), Some("dash_dot"));
    assert!(fp.kicad.locked.contains(&rect) && fp.kicad.locked.contains(&kept) && !fp.kicad.locked.contains(&hidden));
    assert!(fp.kicad.hidden_texts.contains(&hidden) && !fp.kicad.hidden_texts.contains(&kept));
    assert!(fp.kicad.unlocked_texts.contains(&kept));
    let look = fp.kicad.text_extra(&kept).expect("the text's looks");
    assert!(look.italic && look.valign == eda_model::ir::TextVAlign::Top);
    // a pad's lock is one of its own kept forms
    assert!(fp.pads.iter().find(|p| p.number == "1").unwrap().kicad.tokens.iter().any(|t| t == "(locked yes)"));

    let text = export_kicad_mod(&fp);
    for written in [
        "(solder_paste_margin -0.05)",
        "(solder_paste_margin_ratio -0.1)",
        "(private_layers \"User.1\" \"Eco1.User\")",
        "(duplicate_pad_numbers_are_jumpers yes)",
        "(jumper_pad_groups (\"1\" \"3\") (\"2\" \"4\"))",
        "(type dash_dot)) (fill cross_hatch) (locked yes) (layer \"F.SilkS\")",
        "(fp_text user \"kept\" (locked yes) (at",
        "(layer \"F.Fab\") (hide yes)",
    ] {
        assert!(text.contains(written), "{written}: {text}");
    }
    let back = parse_library_footprint(&text).expect("parses back").footprint;
    assert_eq!(back.settings, fp.settings);
    assert_eq!(back.kicad.hatches, fp.kicad.hatches);
    assert_eq!(back.kicad.locked, fp.kicad.locked);
    assert_eq!(back.kicad.hidden_texts, fp.kicad.hidden_texts);
    assert_eq!(back.kicad.text_extras, fp.kicad.text_extras);

    // to the board and back: the art keeps the dash-dot cross-hatched rectangle, the italic top-justified text that turns with the footprint and the hidden text
    let engine = fp.to_engine_footprint();
    let art_rect = engine.art.shapes.iter().find(|s| s.shape.layer() == "F.SilkS").unwrap();
    assert_eq!((art_rect.style, art_rect.hatch), (eda_model::fp_art::LineStyle::DashDot, Some(eda_model::fp_art::Hatch::CrossHatch)));
    let art_kept = engine.art.texts.iter().find(|t| t.text == "kept").unwrap();
    assert!(art_kept.layout.italic && art_kept.layout.valign == -1 && !art_kept.layout.keep_upright && art_kept.layout.size == (eda_model::units::Nm::new(1_200_000), eda_model::units::Nm::new(1_000_000)));
    assert!(!engine.art.texts.iter().find(|t| t.text == "hidden").unwrap().layout.visible);
    let again = eda_model::ir::LibraryFootprint::from_engine_footprint(&engine);
    let r = again.graphics.iter().find(|g| g.layer() == "F.SilkS").unwrap().id().to_string();
    assert_eq!(again.kicad.hatches.get(&r), Some(&eda_model::fp_art::Hatch::CrossHatch));
    assert_eq!(again.kicad.stroke_types.get(&r).map(String::as_str), Some("dash_dot"));
    let h = again.texts.iter().find(|t| t.content == "hidden").expect("a hidden text comes back").id.clone();
    assert!(again.kicad.hidden_texts.contains(&h));
    let k = again.texts.iter().find(|t| t.content == "kept").unwrap().id.clone();
    assert!(again.kicad.unlocked_texts.contains(&k));
    let e = again.kicad.text_extra(&k).unwrap();
    assert!(e.italic && e.valign == eda_model::ir::TextVAlign::Top && e.width_nm == Some(eda_model::units::Nm::new(1_200_000)));
}

const LOOKS: &str = r#"(kicad_symbol_lib (version 20251024) (generator "kicad_symbol_editor")
  (symbol "L"
    (property "Reference" "U" (at 0 0 0) (effects (font (face "DejaVu Sans") (size 1.27 1.27) (color 255 0 0 1))))
    (property "Value" "L" (at 0 0 0) (effects (font (size 1.27 1.27))))
    (symbol "L_1_1"
      (rectangle private (start -2 -2) (end 2 2) (radius 0.5) (stroke (width 0.254) (type dash) (color 0 0 255 1)) (fill (type hatch) (color 0 255 0 1)))
      (circle (center 0 0) (radius 1) (stroke (width 0) (type default)) (fill (type color) (color 10 20 30 1)))
      (polyline private (pts (xy 0 0) (xy 1 1)) (stroke (width 0) (type default)) (fill (type cross_hatch) (color 0 0 0 0)))
      (text "T" (at 0 0 0) (effects (font (size 1.27 1.27)) (href "https://kicad.org")))
    )
  )
)"#;

/// UI-LEFT-2 YY2: a shape's Private flag, border and fill colours, hatched fill and corner radius, a text's hyperlink and a field's face and colour are read
/// (`parseSymbolRectangle`, `parseFill`, `parseEDA_TEXT`) and written where `SaveSymbol` writes them (`formatRect`, `formatFill`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format`).
#[test]
fn a_shapes_private_flag_colours_hatch_and_radius_a_texts_link_and_a_fields_face_are_kept() {
    use eda_model::ir::{LibraryFill, LibrarySymbolGraphic as G};
    let l = one_symbol(LOOKS);
    let rect = l.graphics.iter().find(|g| matches!(g, G::Rectangle { .. })).unwrap();
    let looks = &l.kicad.graphic_looks[rect.id()];
    assert!(looks.private && looks.radius_mm == 0.5 && looks.hatch.as_deref() == Some("hatch"), "{looks:?}");
    assert_eq!(looks.stroke_color.map(|c| (c.r, c.g, c.b)), Some((0, 0, 255)));
    assert_eq!(looks.fill_color.map(|c| (c.r, c.g, c.b)), Some((0, 255, 0)));
    assert!(matches!(rect, G::Rectangle { fill: LibraryFill::Color, .. }));
    let circle = l.graphics.iter().find(|g| matches!(g, G::Circle { .. })).unwrap();
    assert!(matches!(circle, G::Circle { fill: LibraryFill::Color, .. }), "a colour fill is the shape's own colour now");
    assert_eq!(l.kicad.graphic_looks[circle.id()].fill_color.map(|c| c.b), Some(30));
    let text = l.graphics.iter().find(|g| matches!(g, G::Text { .. })).unwrap();
    assert_eq!(l.kicad.graphic_looks[text.id()].hyperlink, "https://kicad.org");
    assert_eq!(l.kicad.field_looks["Reference"].font.as_deref(), Some("DejaVu Sans"));
    // a private shape is not drawn by a placed symbol
    assert_eq!(l.to_engine_symbol().graphics.len(), 2, "the circle and the text");

    let out = written(&l);
    assert!(out.contains("(rectangle private (start -2 -2) (end 2 2) (radius 0.5) (stroke (width 0.254) (type dash) (color 0 0 255 1)) (fill (type hatch) (color 0 255 0 1)))"), "{out}");
    assert!(out.contains("(fill (type color) (color 10 20 30 1))"), "{out}");
    assert!(out.contains("(polyline private (pts") && out.contains("(fill (type cross_hatch) (color 0 0 0 0))"), "{out}");
    assert!(out.contains("(href \"https://kicad.org\")"), "{out}");
    assert!(out.contains("(property \"Reference\" \"U\"") && out.contains("(font (face \"DejaVu Sans\") (size 1.27 1.27) (color 255 0 0 1))"), "{out}");
    let back = one_symbol(&out);
    assert_eq!(back.kicad.graphic_looks, l.kicad.graphic_looks);
    assert_eq!(back.kicad.field_looks, l.kicad.field_looks);
}

/// KiCad 10's line endings of a footprint's line, arc or curve (`(start_shape ..)`, `(end_shape ..)` after the stroke, KiCad 25ec2335 `parsePCB_SHAPE` and `format(
/// PCB_SHAPE )`): read into the footprint's kept looks by the graphic's id, written back where KiCad writes them, and a pad's backdrill, post-machining and teardrops
/// kept among its forms (UI-LEFT-2 FF3).
#[test]
fn a_footprint_lines_endings_and_a_pads_drills_and_teardrops_are_read_and_written_back() {
    const ENDS: &str = r#"(footprint "E" (version 20260818) (generator "pcbnew") (layer "F.Cu")
  (fp_line (start 0 0) (end 5 0) (stroke (width 0.12) (type solid)) (start_shape arrow (length 1) (width 0.8)) (end_shape circle) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111111"))
  (fp_rect (start 0 0) (end 1 1) (stroke (width 0.12) (type solid)) (fill no) (layer "F.SilkS") (uuid "22222222-2222-2222-2222-222222222222"))
  (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "*.Cu" "*.Mask") (front_post_machining counterbore (size 1.5) (depth 0.3)) (teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments yes) (prefer_zone_connections yes)) (uuid "33333333-3333-3333-3333-333333333333"))
)"#;
    let fp = parse_library_footprint(ENDS).expect("parses").footprint;
    let line = fp.graphics.iter().find(|g| matches!(g, eda_model::ir::Shape::Segment { .. })).unwrap();
    let e = fp.kicad.endings.get(line.id()).expect("the line's endings are kept by its id");
    assert_eq!(e.start.style, eda_model::line_ending::LineEndingStyle::Arrow);
    assert_eq!(e.start.length_nm, mm(1.0));
    assert_eq!(e.end.style, eda_model::line_ending::LineEndingStyle::Circle);
    assert_eq!(fp.kicad.endings.len(), 1, "a rectangle has none");
    let text = export_kicad_mod(&fp);
    assert!(text.contains("(stroke (width 0.12) (type solid)) (start_shape arrow (length 1) (width 0.8)) (end_shape circle) (layer \"F.SilkS\")"), "{text}");
    let pad = &fp.pads[0];
    assert!(!eda_model::via_extra::PadDrillProps::from_tokens(&pad.kicad.tokens).is_none());
    assert!(eda_model::teardrop_params::TeardropParams::from_tokens(&pad.kicad.tokens).is_some_and(|t| t.enabled));
    let back = parse_library_footprint(&text).expect("parses back").footprint;
    assert_eq!(back.kicad.endings.values().next(), Some(e));
    assert_eq!(eda_model::via_extra::PadDrillProps::from_tokens(&back.pads[0].kicad.tokens), eda_model::via_extra::PadDrillProps::from_tokens(&pad.kicad.tokens));
}
