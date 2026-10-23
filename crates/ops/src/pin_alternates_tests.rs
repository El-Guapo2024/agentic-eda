//! The alternate functions of a library symbol's pin (`DIALOG_PIN_PROPERTIES`'s "Alternate Assignments", `SCH_PIN::GetAlternates`): they ride along with the pin's own verbs, are kept
//! as the file's map keeps them, and are refused where KiCad refuses them.

use super::*;
use eda_model::ir::{Design, PinAlternate};
use eda_model::symbol::SPoint;
use eda_model::ConstraintModel;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, 100, 300)
}

fn alt(name: &str, t: &str, shape: &str) -> PinAlternate {
    PinAlternate { name: name.into(), electrical_type: t.into(), shape: shape.into() }
}

fn pin(number: &str, alternates: Vec<PinAlternate>) -> LibrarySymbolPin {
    LibrarySymbolPin {
        id: String::new(),
        number: number.into(),
        name: "PA0".into(),
        electrical_type: "bidirectional".into(),
        shape: "line".into(),
        at: SPoint { x: -5.08, y: 0.0 },
        angle_deg: 0.0,
        length_mm: 2.54,
        unit: 1,
        body_style: 1,
        hidden: false,
        name_size_mm: None,
        number_size_mm: None,
        alternates,
    }
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let e = b.apply(&cmd).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

fn pins_of<'a>(b: &'a Board<'_>, lib_id: &str) -> &'a [LibrarySymbolPin] {
    &b.design().symbol_library.as_ref().unwrap().by_lib_id(lib_id).unwrap().pins
}

#[test]
fn a_pin_keeps_its_alternates_by_name_in_the_maps_order_and_an_edit_replaces_them() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Mcu".into() }).unwrap();
    // names once (the last definition stays), sorted by name
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("1", vec![alt("UART_TX", "output", "line"), alt("ADC1", "input", "line"), alt("UART_TX", "bidirectional", "inverted")]) }).unwrap();
    let got: Vec<_> = pins_of(&b, "Lib:Mcu")[0].alternates.iter().map(|a| (a.name.as_str(), a.electrical_type.as_str(), a.shape.as_str())).collect();
    assert_eq!(got, [("ADC1", "input", "line"), ("UART_TX", "bidirectional", "inverted")]);
    let id = pins_of(&b, "Lib:Mcu")[0].id.clone();
    // Pin Properties replaces the list as a whole
    b.apply(&Cmd::EditSymbolPin { lib_id: "Lib:Mcu".into(), id: id.clone(), pin: pin("1", vec![alt("SPI_CLK", "output", "clock")]) }).unwrap();
    assert_eq!(pins_of(&b, "Lib:Mcu")[0].alternates, vec![alt("SPI_CLK", "output", "clock")]);
    b.apply(&Cmd::EditSymbolPin { lib_id: "Lib:Mcu".into(), id, pin: pin("1", vec![]) }).unwrap();
    assert!(pins_of(&b, "Lib:Mcu")[0].alternates.is_empty(), "an empty grid removes them");
}

#[test]
fn an_alternate_the_dialog_would_refuse_is_refused_with_its_words() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Mcu".into() }).unwrap();
    let said = refusal(&mut b, Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("1", vec![alt("", "input", "line")]) });
    assert_eq!(said, "ops_bad_pin: Alternate pin definitions must have a name.");
    let said = refusal(&mut b, Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("1", vec![alt("A", "inout", "line")]) });
    assert!(said.contains("is not a pin electrical type"), "{said}");
    let said = refusal(&mut b, Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("1", vec![alt("A", "input", "round")]) });
    assert!(said.contains("is not a pin graphic style"), "{said}");
    assert!(pins_of(&b, "Lib:Mcu").is_empty(), "a refused pin is not added");
}

#[test]
fn a_symbol_with_several_body_styles_takes_no_alternates() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Mcu".into() }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("1", vec![]) }).unwrap();
    let mut props = Cmd::EditSymbolProperties {
        lib_id: "Lib:Mcu".into(),
        reference_prefix: "U".into(),
        description: String::new(),
        keywords: String::new(),
        datasheet: String::new(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count: 1,
        has_alternate_body_style: true,
        footprint_filters: vec![],
        units_locked: None,
        power_local: None,
        exclude_from_sim: None,
        exclude_from_pos_files: None,
    };
    b.apply(&props).unwrap();
    let said = refusal(&mut b, Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("2", vec![alt("A", "input", "line")]) });
    assert_eq!(said, "ops_bad_pin: Alternate pin assignments are not available for symbols with multiple body styles.");
    // a pin without alternates is fine in it, and so is one in a symbol with a single body style
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("2", vec![]) }).unwrap();
    if let Cmd::EditSymbolProperties { has_alternate_body_style, .. } = &mut props {
        *has_alternate_body_style = false;
    }
    b.apply(&props).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Mcu".into(), pin: pin("3", vec![alt("A", "input", "line")]) }).unwrap();
}
