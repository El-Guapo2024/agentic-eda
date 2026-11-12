//! Tests for what Symbol Properties and the symbol's and pin's context menus change on a placed symbol (`SchCmd::SetPinAlternates`, `SetSymbolUnit`,
//! `SetFieldLooks`): a pin's alternate function (`SCH_EDIT_FRAME::SetAltPinFunction`, `SCH_PIN::SetAlt`, `SyncOtherUnits`), the unit a placed symbol
//! draws (`SCH_EDIT_FRAME::SelectUnit`) and a field's font face and colour (`FIELDS_GRID_TABLE::SetValue`'s `FDC_FONT` / `FDC_COLOR`).

use super::*;
use crate::sch_edit::SchCmd;
use eda_engine::fields_edit::field_id;
use eda_model::ir::{field_key, PinAlternate, Provenance, SchematicSection, SymbolInstance};
use eda_model::sch_extras::{FieldFontLook, SchColor};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::units::mm;
use eda_model::{Part, Pin, PinKind};
use std::collections::BTreeMap;

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn pin(number: &str, name: &str, kind: &str, y: f64, unit: u32) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: kind.into(), shape: "line".into(), at: SPoint::new(-7.62, y), angle_deg: 0.0, length_mm: 2.54, unit, hidden: false }
}

/// A dual: unit A has pins 1 (`PA0`, bidirectional, alternates `USART1_TX` output and `TIM2_CH1` input with an inverted shape) and 2 (`PA1`, no
/// alternates); unit B has pins 3 and 4.
fn dual() -> LibSymbol {
    let mut s = LibSymbol {
        connectivity: Default::default(),
        lib_id: "test:DUAL".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 0, start: SPoint::new(-5.08, 2.54), end: SPoint::new(5.08, -2.54), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "PA0", "bidirectional", 1.27, 1), pin("2", "PA1", "bidirectional", -1.27, 1), pin("3", "PB0", "passive", 1.27, 2), pin("4", "PB1", "passive", -1.27, 2)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 2,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    };
    s.connectivity.pin_alternates.insert(
        "1".into(),
        vec![PinAlternate { name: "TIM2_CH1".into(), electrical_type: "input".into(), shape: "inverted".into() }, PinAlternate { name: "USART1_TX".into(), electrical_type: "output".into(), shape: "line".into() }],
    );
    s.connectivity.pin_alternates.insert("3".into(), vec![PinAlternate { name: "ADC0".into(), electrical_type: "input".into(), shape: "line".into() }]);
    s
}

fn part(reference: &str, pins: &[&str]) -> Part {
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some("v".into()), package: None, footprint: None, symbol: None, datasheet: None, pins: pins.iter().map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, edge: None }
}

fn model() -> ConstraintModel {
    ConstraintModel { parts: vec![part("U1", &["1", "2", "3", "4"])], symbols: vec![dual()], ..Default::default() }
}

fn instance(unit: u32, x: f64) -> SymbolInstance {
    SymbolInstance { id: "U1".into(), at: Point { x: mm(x), y: mm(50.8) }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:DUAL".into(), unit, value: "v".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false, no_footprint: false }
}

fn board<'a>(m: &'a ConstraintModel, units: &[u32]) -> Board<'a> {
    let mut design = empty_design();
    design.schematic = Some(SchematicSection { symbols: units.iter().enumerate().map(|(i, u)| instance(*u, 50.8 + 50.8 * i as f64)).collect(), ..Default::default() });
    Board::new(design, m, mm(0.1), mm(0.3))
}

fn sch<'b>(b: &'b Board) -> &'b SchematicSection {
    b.design().schematic.as_ref().unwrap()
}

fn alts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(n, a)| (n.to_string(), a.to_string())).collect()
}

fn set_alts(unit: Option<u32>, pairs: &[(&str, &str)], other_units: bool) -> Cmd {
    Cmd::SchEdit(SchCmd::SetPinAlternates { id: "U1".into(), unit, alternates: alts(pairs), other_units })
}

fn refusal(b: &mut Board, cmd: Cmd) -> String {
    b.apply(&cmd).unwrap_err()[0].check.clone()
}

#[test]
fn a_pin_takes_one_of_its_alternates_and_its_own_name_clears_it() {
    let m = model();
    let mut b = board(&m, &[1]);
    b.apply(&set_alts(None, &[("1", "USART1_TX")], false)).unwrap();
    assert_eq!(sch(&b).extras.pin_alternate("U1", "1"), Some("USART1_TX"));
    // the pin is then of the alternate's type (`SCH_PIN::GetType`: `m_libPin->GetAlt( m_alt ).m_Type`)
    let lib = dual();
    assert_eq!(lib.placed_pin_type(lib.pin_by_number("1").unwrap(), sch(&b).extras.pin_alternate("U1", "1")), "output");
    // `SetAlt( aAlt == GetBaseName() )`: no alternate, and the symbol keeps no entry for it
    b.apply(&set_alts(None, &[("1", "PA0")], false)).unwrap();
    assert!(sch(&b).extras.pin_alternates.is_empty(), "{:?}", sch(&b).extras.pin_alternates);
    b.apply(&set_alts(None, &[("1", "TIM2_CH1")], false)).unwrap();
    b.apply(&set_alts(None, &[("1", "")], false)).unwrap();
    assert!(sch(&b).extras.pin_alternates.is_empty(), "an empty name clears it too");
}

#[test]
fn an_alternate_the_library_pin_has_not_is_refused_and_changes_nothing() {
    let m = model();
    let mut b = board(&m, &[1]);
    b.apply(&set_alts(None, &[("1", "USART1_TX")], false)).unwrap();
    let before = serde_json::to_value(sch(&b)).unwrap();
    assert_eq!(refusal(&mut b, set_alts(None, &[("1", "SPI1_MOSI")], false)), "ops_unknown_alternate");
    assert_eq!(refusal(&mut b, set_alts(None, &[("2", "ANYTHING")], false)), "ops_unknown_alternate", "pin 2 has no alternates at all");
    assert_eq!(refusal(&mut b, set_alts(None, &[("9", "")], false)), "ops_unknown_pin");
    assert_eq!(refusal(&mut b, set_alts(None, &[("1", "USART1_TX")], false)), "ops_nothing_to_change");
    assert_eq!(serde_json::to_value(sch(&b)).unwrap(), before);
}

#[test]
fn symbol_properties_keeps_the_alternates_of_every_placed_unit_in_sync() {
    // `SyncOtherUnits`: "Keep fields other than the reference, include/exclude flags, and alternate pin assignments in sync in multi-unit parts"
    let m = model();
    let mut b = board(&m, &[1, 2]);
    b.apply(&set_alts(Some(1), &[("1", "USART1_TX"), ("3", "ADC0")], true)).unwrap();
    for key in [field_key("U1", 1), field_key("U1", 2)] {
        assert_eq!(sch(&b).extras.pin_alternates.get(&key), Some(&alts(&[("1", "USART1_TX"), ("3", "ADC0")])), "{key}");
    }
    // the canvas's Pin Function submenu sets the one pin of the one placed unit (`SetAltPinFunction`)
    b.apply(&set_alts(Some(2), &[("3", "PB0")], false)).unwrap();
    assert_eq!(sch(&b).extras.pin_alternates.get(&field_key("U1", 2)), Some(&alts(&[("1", "USART1_TX")])));
    assert_eq!(sch(&b).extras.pin_alternates.get(&field_key("U1", 1)), Some(&alts(&[("1", "USART1_TX"), ("3", "ADC0")])));
}

/// UI-LEFT-2 SS17: `SyncOtherUnits` walks the hierarchy (`for( SCH_SHEET_PATH& sheet : Schematic()->Hierarchy() ) CollectOtherUnits( ref, .. )`): unit B of U1
/// placed on a sheet takes the alternates set in unit A's dialog on the root, in the one command; the Pin Function submenu still sets its one unit.
#[test]
fn symbol_properties_syncs_the_alternates_of_a_unit_on_another_sheet() {
    let m = model();
    let mut b = board(&m, &[1]);
    b.design.schematic.as_mut().unwrap().sheets.push(eda_model::ir::SheetInstance { id: "s1".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: Point { x: mm(190.5), y: mm(12.7) }, size: (mm(25.4), mm(12.7)), pins: Vec::new(), page: String::new() });
    let mut other = instance(1, 50.8);
    other.id = "U9".into();
    b.design.sheet_contents = Some(BTreeMap::from([("sub.kicad_sch".to_string(), SchematicSection { symbols: vec![instance(2, 50.8), other], ..Default::default() })]));
    let sub = |b: &Board| b.design().sheet_contents.as_ref().unwrap()["sub.kicad_sch"].extras.pin_alternates.clone();
    b.apply(&set_alts(Some(1), &[("1", "USART1_TX")], false)).unwrap();
    assert!(sub(&b).is_empty(), "the Pin Function submenu sets the one unit");
    b.apply(&set_alts(Some(1), &[("1", "TIM2_CH1"), ("3", "ADC0")], true)).unwrap();
    assert_eq!(sub(&b).get(&field_key("U1", 2)), Some(&alts(&[("1", "TIM2_CH1"), ("3", "ADC0")])), "unit B on the sheet");
    assert!(!sub(&b).contains_key(&field_key("U9", 1)), "another reference is not synced");
    assert_eq!(sch(&b).extras.pin_alternates.get(&field_key("U1", 1)), Some(&alts(&[("1", "TIM2_CH1"), ("3", "ADC0")])));
}

#[test]
fn a_locked_symbol_keeps_its_pin_functions() {
    let m = model();
    let mut b = board(&m, &[1]);
    b.design.schematic.as_mut().unwrap().extras.set_locked("U1", true);
    assert_eq!(refusal(&mut b, set_alts(None, &[("1", "USART1_TX")], false)), "ops_locked");
}

fn set_unit(unit: Option<u32>, new_unit: u32, swap: bool) -> Cmd {
    Cmd::SchEdit(SchCmd::SetSymbolUnit { id: "U1".into(), unit, new_unit, swap })
}

#[test]
fn a_placed_symbol_changes_unit_and_takes_what_the_sheet_keeps_for_it_along() {
    let m = model();
    let mut b = board(&m, &[1]);
    {
        let s = b.design.schematic.as_mut().unwrap();
        s.extras.pin_alternates.insert(field_key("U1", 1), alts(&[("1", "USART1_TX")]));
        s.extras.excluded_from_pos_files.insert(field_key("U1", 1));
        s.extras.set_field_look(&field_key("U1", 1), "Value", FieldFontLook { font: Some("KiCad Font".into()), color: None });
    }
    b.apply(&set_unit(None, 2, false)).unwrap();
    let s = sch(&b);
    assert_eq!(s.symbols.iter().map(|x| x.unit).collect::<Vec<_>>(), [2]);
    let k2 = field_key("U1", 2);
    assert_eq!(s.extras.pin_alternates.get(&k2), Some(&alts(&[("1", "USART1_TX")])));
    assert!(s.extras.excluded_from_pos_files.contains(&k2) && !s.extras.excluded_from_pos_files.contains("U1"));
    assert!(s.extras.field_look(&k2, "Value").is_some() && s.extras.field_look("U1", "Value").is_none());
    // `if( unitCount <= 1 || currentUnit == aUnit ) return;` and `if( aUnit > unitCount ) aUnit = unitCount;`
    assert_eq!(refusal(&mut b, set_unit(None, 2, false)), "ops_nothing_to_change");
    assert_eq!(refusal(&mut b, set_unit(None, 9, false)), "ops_nothing_to_change", "9 is clamped to 2, the unit it has");
    b.apply(&set_unit(None, 1, false)).unwrap();
    assert_eq!(sch(&b).symbols[0].unit, 1);
}

#[test]
fn a_unit_already_placed_on_the_sheet_is_swapped_or_refused() {
    let m = model();
    let mut b = board(&m, &[1, 2]);
    b.design.schematic.as_mut().unwrap().extras.pin_alternates.insert(field_key("U1", 2), alts(&[("3", "ADC0")]));
    let at = |b: &Board, unit: u32| sch(b).symbols.iter().find(|s| s.unit == unit).unwrap().at;
    let (a, bb) = (at(&b, 1), at(&b, 2));
    // "Duplicate" is not possible: one placement of a unit per sheet
    assert_eq!(refusal(&mut b, set_unit(Some(1), 2, false)), "ops_unit_placed");
    // "Swap 'B' and 'A'": the other symbol takes the unit this one had
    b.apply(&set_unit(Some(1), 2, true)).unwrap();
    assert_eq!((at(&b, 2), at(&b, 1)), (a, bb), "the symbol at A's place draws B now, and the other one A");
    // what the sheet kept for the B placement went with the B: it is under unit 1's key now, which is the other symbol
    assert_eq!(sch(&b).extras.pin_alternates.get(&field_key("U1", 1)), Some(&alts(&[("3", "ADC0")])));
    assert!(sch(&b).extras.pin_alternates.get(&field_key("U1", 2)).is_none());
}

#[test]
fn a_symbol_of_one_unit_has_no_unit_to_change_to() {
    let mut resistor = eda_model::symbol::builtin("Device:R").expect("the built-in resistor");
    resistor.lib_id = "Device:R".into();
    let m = ConstraintModel { parts: vec![part("R1", &["1", "2"])], symbols: vec![resistor], ..Default::default() };
    let mut design = empty_design();
    design.schematic = Some(SchematicSection { symbols: vec![SymbolInstance { id: "R1".into(), lib_id: "Device:R".into(), ..instance(1, 10.0) }], ..Default::default() });
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    let r = b.apply(&Cmd::SchEdit(SchCmd::SetSymbolUnit { id: "R1".into(), unit: None, new_unit: 2, swap: false })).unwrap_err();
    assert_eq!((r[0].check.as_str(), r[0].hint.as_deref().unwrap_or("")), ("ops_nothing_to_change", "This symbol has only one unit."));
}

fn looks(ids: &[String], font: Option<&str>, color: Option<SchColor>) -> Cmd {
    Cmd::SchEdit(SchCmd::SetFieldLooks { ids: ids.to_vec(), font: font.map(String::from), color })
}

#[test]
fn a_fields_font_and_colour_are_set_and_the_default_font_keeps_no_entry() {
    let m = model();
    let mut b = board(&m, &[1]);
    let value = field_id("U1", "Value");
    let red = SchColor { r: 255, g: 0, b: 0, a: 255 };
    b.apply(&looks(&[value.clone()], Some("DejaVu Sans"), Some(red))).unwrap();
    assert_eq!(sch(&b).extras.field_look("U1", "Value"), Some(&FieldFontLook { font: Some("DejaVu Sans".into()), color: Some(red) }));
    // the colour alone: the font stays
    b.apply(&looks(&[value.clone()], None, Some(SchColor { r: 0, g: 0, b: 0, a: 0 }))).unwrap();
    assert_eq!(sch(&b).extras.field_look("U1", "Value"), Some(&FieldFontLook { font: Some("DejaVu Sans".into()), color: None }));
    // "KiCad Font" names the stroke font; "Default Font" is none of the field's own (`SetFont( nullptr )`)
    b.apply(&looks(&[value.clone()], Some("KiCad Font"), None)).unwrap();
    assert_eq!(sch(&b).extras.field_look("U1", "Value").and_then(|l| l.font.as_deref()), Some("KiCad Font"));
    b.apply(&looks(&[value.clone()], Some("Default Font"), None)).unwrap();
    assert!(sch(&b).extras.field_looks.is_empty(), "{:?}", sch(&b).extras.field_looks);
    assert_eq!(refusal(&mut b, looks(&[value], Some(""), None)), "ops_field_unchanged");
    assert_eq!(refusal(&mut b, looks(&[field_id("U1", "Nope")], Some("KiCad Font"), None)), "ops_unknown_item");
}

#[test]
fn a_renamed_symbol_keeps_its_fields_looks() {
    let m = model();
    let mut b = board(&m, &[1]);
    b.apply(&looks(&[field_id("U1", "Reference")], Some("KiCad Font"), None)).unwrap();
    b.apply(&Cmd::RenameSymbol { id: "U1".into(), new_id: "U7".into() }).unwrap();
    assert!(sch(&b).extras.field_look("U7", "Reference").is_some() && sch(&b).extras.field_look("U1", "Reference").is_none(), "{:?}", sch(&b).extras.field_looks);
}
