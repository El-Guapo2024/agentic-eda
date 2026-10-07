//! A symbol of the project library keeps its graphics in mm, as fractions. The `eda` build switches `serde_json`'s `arbitrary_precision` on
//! (through `starlark`); the dev-dependency in this crate's `Cargo.toml` does the same here. Under it, serde's internally tagged enum reader
//! turned every fractional number it had buffered into a map ("invalid type: map, expected f64"), which made any `design.json` holding a
//! library symbol graphic unreadable -- opening `Device:R` for editing was enough. `LibrarySymbolGraphic` is read field by field now.

use eda_model::ir::{LibraryFill, LibrarySymbol, LibrarySymbolGraphic};
use eda_model::symbol::SPoint;

fn p(x: f64, y: f64) -> SPoint {
    SPoint { x, y }
}

fn all_kinds() -> Vec<LibrarySymbolGraphic> {
    vec![
        LibrarySymbolGraphic::Rectangle { id: "sym_a".into(), unit: 0, body_style: 1, start: p(-1.016, -2.54), end: p(1.016, 2.54), stroke_mm: 0.254, fill: LibraryFill::None },
        LibrarySymbolGraphic::Polyline { id: "sym_b".into(), unit: 2, body_style: 2, pts: vec![p(0.0, 0.5), p(1.27, 2.0), p(-3.81, 0.127)], stroke_mm: 0.1524, fill: LibraryFill::Background },
        LibrarySymbolGraphic::Circle { id: String::new(), unit: 1, body_style: 1, center: p(0.0, 0.0), radius_mm: 2.54, stroke_mm: 0.254, fill: LibraryFill::Outline },
        LibrarySymbolGraphic::Arc { id: "sym_d".into(), unit: 0, body_style: 1, start: p(1.0, 0.0), mid: p(0.7071, 0.7071), end: p(0.0, 1.0), stroke_mm: 0.254, fill: LibraryFill::None },
        LibrarySymbolGraphic::Text { id: "sym_e".into(), unit: 0, body_style: 1, text: "TEXT".into(), at: p(1.27, -1.27), angle_deg: 90.0, size_mm: 1.27 },
    ]
}

#[test]
fn every_kind_survives_a_write_and_a_read_of_the_text() {
    for g in all_kinds() {
        let text = serde_json::to_string(&g).unwrap();
        let back: LibrarySymbolGraphic = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(back, g, "{text}");
    }
}

#[test]
fn a_whole_symbol_with_graphics_reads_back_from_text_as_the_design_file_holds_it() {
    let sym = LibrarySymbol {
        lib_id: "Device:R".into(),
        reference_prefix: "R".into(),
        description: "Resistor".into(),
        keywords: String::new(),
        datasheet: String::new(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count: 1,
        has_alternate_body_style: false,
        footprint_filters: vec![],
        graphics: all_kinds(),
        pins: vec![],
        published: false,
    };
    // The design file is pretty-printed, so floats that are whole numbers carry a `.0`.
    let text = serde_json::to_string_pretty(&sym).unwrap();
    let back: LibrarySymbol = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(back, sym);
}

#[test]
fn the_defaults_are_the_ones_the_enum_had() {
    let g: LibrarySymbolGraphic = serde_json::from_str(r#"{"kind":"rectangle","start":{"x":-1.016,"y":-2.54},"end":{"x":1.016,"y":2.54},"stroke_mm":0.254}"#).unwrap();
    assert_eq!(g, LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 0, body_style: 1, start: p(-1.016, -2.54), end: p(1.016, 2.54), stroke_mm: 0.254, fill: LibraryFill::None });
    let t: LibrarySymbolGraphic = serde_json::from_str(r#"{"kind":"text","text":"x","at":{"x":0.5,"y":0.25},"size_mm":1.27}"#).unwrap();
    assert_eq!(t, LibrarySymbolGraphic::Text { id: String::new(), unit: 0, body_style: 1, text: "x".into(), at: p(0.5, 0.25), angle_deg: 0.0, size_mm: 1.27 });
}

#[test]
fn a_field_that_does_not_belong_to_the_kind_is_still_refused() {
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"kind":"rectangle","start":{"x":0,"y":0},"end":{"x":1,"y":1},"stroke_mm":0.254,"radius_mm":1.5}"#).unwrap_err().to_string();
    assert!(e.contains("radius_mm"), "{e}");
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"kind":"text","text":"x","at":{"x":0,"y":0},"size_mm":1.27,"fill":"none"}"#).unwrap_err().to_string();
    assert!(e.contains("fill"), "{e}");
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"kind":"rectangle","start":{"x":0,"y":0},"end":{"x":1,"y":1},"stroke_mm":0.254,"nonsense":1}"#).unwrap_err().to_string();
    assert!(e.contains("nonsense"), "{e}");
}

#[test]
fn an_unknown_kind_and_a_missing_required_field_are_errors() {
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"kind":"bezier","stroke_mm":0.254}"#).unwrap_err().to_string();
    assert!(e.contains("bezier"), "{e}");
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"kind":"circle","center":{"x":0,"y":0},"stroke_mm":0.254}"#).unwrap_err().to_string();
    assert!(e.contains("radius_mm"), "{e}");
    let e = serde_json::from_str::<LibrarySymbolGraphic>(r#"{"stroke_mm":0.254}"#).unwrap_err().to_string();
    assert!(e.contains("kind"), "{e}");
}
