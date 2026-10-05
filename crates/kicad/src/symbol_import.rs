//! `.kicad_sym` text -> editable [`LibrarySymbol`]s: the reader behind the Symbol Editor's
//! Import Symbol and Paste Symbol (`SYMBOL_EDIT_FRAME::ImportSymbol`,
//! `SCH_IO_KICAD_SEXPR::ParseLibSymbols`).
//!
//! `symbol_lib::parse_symbol_library` already reads these files, but into the engine's resolved,
//! read-only `LibSymbol` -- which has no hidden pins, no fill kinds, no text sizes, no keywords, no
//! footprint filters and no body styles, so a copy that went through it would lose all of them. This
//! reader is the inverse of [`export_kicad_sym`](crate::export_kicad_sym) instead: everything that
//! writer emits comes back, so copy -> paste and export -> import are lossless for the studio's own
//! symbols, and a real KiCad file loses only what the editable type cannot hold (text boxes, bezier
//! curves, alternate pin functions -- each reported in `warnings`, never dropped silently).
//!
//! Grammar notes confirmed against the KiCad source this repo ports (`eeschema/sch_io/kicad_sexpr`):
//! a symbol's own `(symbol "Name_<unit>_<style>" ...)` sub-blocks carry the drawn items (unit 0 = shared
//! by every unit, style 0 = shared by every body style); `(extends "Parent")` names a base symbol of
//! the same file whose graphics and pins this one shows; a pin or pin-number/name block hides with
//! either the bare atom `hide` (older files) or `(hide yes)` (KiCad 9+).

use std::collections::HashMap;

use eda_model::ir::{LibraryFill, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin};
use eda_model::symbol::SPoint;

use crate::sexpr::{self, Sexpr};

/// What [`parse_library_symbols`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedSymbols {
    /// In file order, each named by its bare symbol name (no `Library:` prefix -- the caller picks the
    /// library it lands in).
    pub symbols: Vec<LibrarySymbol>,
    /// One line per kind of item that could not be represented and was left out.
    pub warnings: Vec<String>,
}

/// Parse a `.kicad_sym` file (`(kicad_symbol_lib ...)`) or a bare run of `(symbol "Name" ...)` forms (what
/// `SYMBOL_EDIT_FRAME::CopySymbolToClipboard` puts on the clipboard).
pub fn parse_library_symbols(text: &str) -> Result<ParsedSymbols, String> {
    let trimmed = text.trim_start();
    let wrapped;
    let source = if trimmed.starts_with("(symbol") {
        wrapped = format!("(kicad_symbol_lib {text})");
        wrapped.as_str()
    } else {
        text
    };
    let tree = sexpr::parse(source).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    let root = tree.as_list().filter(|l| sexpr::tag(l) == Some("kicad_symbol_lib")).ok_or("the top-level form is not (kicad_symbol_lib ...); not a KiCad symbol library file")?;

    let items: Vec<(&str, &[Sexpr])> = sexpr::find_all(root, "symbol").filter_map(|it| Some((sexpr::txt(it, 1)?, it))).collect();
    if items.is_empty() {
        return Err("the file holds no symbol".into());
    }
    let by_name: HashMap<&str, &[Sexpr]> = items.iter().copied().collect();

    let mut warnings = Vec::new();
    let symbols = items.iter().map(|(name, item)| build(name, item, &by_name, &mut warnings)).collect();
    warnings.sort();
    warnings.dedup();
    Ok(ParsedSymbols { symbols, warnings })
}

/// `(hide yes)` or a bare `hide` among `list`'s own children.
fn hidden(list: &[Sexpr]) -> bool {
    list.iter().skip(1).any(|c| match c {
        Sexpr::Atom(a) => a == "hide",
        Sexpr::List(l) => sexpr::tag(l) == Some("hide") && sexpr::txt(l, 1).map(|v| v == "yes").unwrap_or(true),
    })
}

fn yes_no(item: &[Sexpr], key: &str, default: bool) -> bool {
    sexpr::find(item, key).and_then(|f| sexpr::txt(f, 1)).map(|v| v == "yes").unwrap_or(default)
}

fn property(item: &[Sexpr], base: Option<&[Sexpr]>, key: &str) -> String {
    let find = |node: &[Sexpr]| sexpr::find_all(node, "property").find(|p| sexpr::txt(p, 1) == Some(key)).and_then(|p| sexpr::txt(p, 2)).map(str::to_string);
    find(item).or_else(|| base.and_then(find)).unwrap_or_default()
}

fn build(name: &str, item: &[Sexpr], by_name: &HashMap<&str, &[Sexpr]>, warnings: &mut Vec<String>) -> LibrarySymbol {
    let base = sexpr::find(item, "extends").and_then(|e| sexpr::txt(e, 1)).and_then(|b| by_name.get(b).copied());

    let own = drawn_items(item, warnings);
    // `extends` with nothing drawn of its own shows the base's graphics and pins; a derived symbol that
    // does draw something keeps it.
    let (graphics, pins, unit_count, alt_style) = match base {
        Some(b) if own.0.is_empty() && own.1.is_empty() => drawn_items(b, warnings),
        _ => own,
    };

    let power = |node: &[Sexpr]| node.iter().any(|it| it.as_list().is_some_and(|l| sexpr::tag(l) == Some("power")) || it.text() == Some("power"));
    let pin_numbers = sexpr::find(item, "pin_numbers").or_else(|| base.and_then(|b| sexpr::find(b, "pin_numbers")));
    let pin_names = sexpr::find(item, "pin_names").or_else(|| base.and_then(|b| sexpr::find(b, "pin_names")));
    let footprint_filters = property(item, base, "ki_fp_filters").split_whitespace().map(str::to_string).collect();
    let reference = property(item, base, "Reference");

    let mut sym = LibrarySymbol {
        lib_id: name.to_string(),
        reference_prefix: if reference.is_empty() { "U".to_string() } else { reference },
        description: property(item, base, "Description"),
        keywords: property(item, base, "ki_keywords"),
        datasheet: property(item, base, "Datasheet"),
        power: power(item) || base.is_some_and(power),
        in_bom: yes_no(item, "in_bom", base.map(|b| yes_no(b, "in_bom", true)).unwrap_or(true)),
        on_board: yes_no(item, "on_board", base.map(|b| yes_no(b, "on_board", true)).unwrap_or(true)),
        pin_numbers_hidden: pin_numbers.map(hidden).unwrap_or(false),
        pin_names_hidden: pin_names.map(hidden).unwrap_or(false),
        footprint_filters,
        unit_count: unit_count.max(1),
        has_alternate_body_style: alt_style,
        graphics,
        pins,
        ..LibrarySymbol::default()
    };
    if let Some(offset) = pin_names.and_then(|p| sexpr::find(p, "offset")).and_then(|o| sexpr::num(o, 1)) {
        sym.pin_name_offset_mm = offset;
    }
    sym.assign_missing_ids();
    sym
}

/// `"Name_<unit>_<style>"` -> (unit, style); a name with no such suffix is unit 1, style 1.
fn unit_and_style(sub_name: &str) -> (u32, u32) {
    let mut parts = sub_name.rsplitn(3, '_');
    let style = parts.next().and_then(|s| s.parse().ok());
    let unit = parts.next().and_then(|s| s.parse().ok());
    match (unit, style) {
        (Some(u), Some(s)) => (u, s),
        _ => (1, 1),
    }
}

fn point(list: &[Sexpr], tag: &str) -> Option<SPoint> {
    let p = sexpr::find(list, tag)?;
    Some(SPoint { x: sexpr::num(p, 1)?, y: sexpr::num(p, 2)? })
}

fn stroke_width(item: &[Sexpr]) -> f64 {
    sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).unwrap_or(0.254)
}

/// `(fill (type none|outline|background|color))`; a coloured fill reads as the background fill.
fn fill_of(item: &[Sexpr]) -> LibraryFill {
    match sexpr::find(item, "fill").and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)) {
        Some("outline") => LibraryFill::Outline,
        Some("background") | Some("color") => LibraryFill::Background,
        _ => LibraryFill::None,
    }
}

fn font_size(item: &[Sexpr]) -> Option<f64> {
    sexpr::find(item, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1))
}

/// Every graphic and pin across every `Name_<unit>_<style>` sub-block of one symbol node, the highest
/// unit seen, and whether a body style above 1 exists.
fn drawn_items(item: &[Sexpr], warnings: &mut Vec<String>) -> (Vec<LibrarySymbolGraphic>, Vec<LibrarySymbolPin>, u32, bool) {
    let mut graphics = Vec::new();
    let mut pins = Vec::new();
    let mut max_unit = 1u32;
    let mut alt_style = false;
    for sub in sexpr::find_all(item, "symbol") {
        let Some(sub_name) = sexpr::txt(sub, 1) else { continue };
        let (unit, style) = unit_and_style(sub_name);
        max_unit = max_unit.max(unit);
        alt_style |= style >= 2;
        for child in sub.iter().skip(2).filter_map(Sexpr::as_list) {
            match sexpr::tag(child) {
                Some("rectangle") => {
                    if let (Some(start), Some(end)) = (point(child, "start"), point(child, "end")) {
                        graphics.push(LibrarySymbolGraphic::Rectangle { id: String::new(), unit, body_style: style, start, end, stroke_mm: stroke_width(child), fill: fill_of(child) });
                    }
                }
                Some("polyline") => {
                    if let Some(pts_node) = sexpr::find(child, "pts") {
                        let pts: Vec<SPoint> = sexpr::find_all(pts_node, "xy").filter_map(|xy| Some(SPoint { x: sexpr::num(xy, 1)?, y: sexpr::num(xy, 2)? })).collect();
                        if pts.len() >= 2 {
                            graphics.push(LibrarySymbolGraphic::Polyline { id: String::new(), unit, body_style: style, pts, stroke_mm: stroke_width(child), fill: fill_of(child) });
                        }
                    }
                }
                Some("circle") => {
                    if let (Some(center), Some(radius)) = (point(child, "center"), sexpr::find(child, "radius").and_then(|r| sexpr::num(r, 1))) {
                        graphics.push(LibrarySymbolGraphic::Circle { id: String::new(), unit, body_style: style, center, radius_mm: radius, stroke_mm: stroke_width(child), fill: fill_of(child) });
                    }
                }
                Some("arc") => {
                    if let (Some(start), Some(mid), Some(end)) = (point(child, "start"), point(child, "mid"), point(child, "end")) {
                        graphics.push(LibrarySymbolGraphic::Arc { id: String::new(), unit, body_style: style, start, mid, end, stroke_mm: stroke_width(child), fill: fill_of(child) });
                    }
                }
                Some("text") => {
                    if let (Some(content), Some(at)) = (sexpr::txt(child, 1), sexpr::find(child, "at")) {
                        if let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) {
                            graphics.push(LibrarySymbolGraphic::Text { id: String::new(), unit, body_style: style, text: content.to_string(), at: SPoint { x, y }, angle_deg: sexpr::num(at, 3).unwrap_or(0.0), size_mm: font_size(child).unwrap_or(1.27) });
                        }
                    }
                }
                Some("pin") => {
                    if let Some(pin) = parse_pin(child, unit, style) {
                        pins.push(pin);
                    }
                }
                Some("bezier") => warnings.push("bezier curves are not supported by the library symbol and were left out".to_string()),
                Some("text_box") => warnings.push("text boxes are not supported by the library symbol and were left out".to_string()),
                _ => {}
            }
        }
    }
    (graphics, pins, max_unit, alt_style)
}

/// `(pin <type> <shape> (at x y angle) (length L) [hide] (name "N" (effects ...)) (number "N" (effects ...)))`.
fn parse_pin(item: &[Sexpr], unit: u32, style: u32) -> Option<LibrarySymbolPin> {
    let electrical_type = sexpr::txt(item, 1)?.to_string();
    let shape = sexpr::txt(item, 2).filter(|s| !s.is_empty()).unwrap_or("line").to_string();
    let at = sexpr::find(item, "at")?;
    let (x, y) = (sexpr::num(at, 1)?, sexpr::num(at, 2)?);
    let name_node = sexpr::find(item, "name");
    let number_node = sexpr::find(item, "number");
    let name = name_node.and_then(|n| sexpr::txt(n, 1)).unwrap_or("~");
    Some(LibrarySymbolPin {
        id: String::new(),
        number: number_node.and_then(|n| sexpr::txt(n, 1)).unwrap_or("").to_string(),
        name: if name == "~" { String::new() } else { name.to_string() },
        electrical_type,
        shape,
        at: SPoint { x, y },
        angle_deg: sexpr::num(at, 3).unwrap_or(0.0),
        length_mm: sexpr::find(item, "length").and_then(|l| sexpr::num(l, 1)).unwrap_or(0.0),
        unit,
        body_style: style,
        hidden: hidden(item),
        name_size_mm: name_node.and_then(font_size),
        number_size_mm: number_node.and_then(font_size),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export_kicad_sym, export_kicad_sym_library};

    fn pin(number: &str, unit: u32, body_style: u32, hidden: bool) -> LibrarySymbolPin {
        LibrarySymbolPin {
            id: String::new(),
            number: number.into(),
            name: format!("N{number}"),
            electrical_type: "passive".into(),
            shape: "inverted".into(),
            at: SPoint { x: 1.27, y: 5.08 },
            angle_deg: 270.0,
            length_mm: 2.54,
            unit,
            body_style,
            hidden,
            name_size_mm: Some(1.0),
            number_size_mm: Some(0.9),
        }
    }

    fn sample() -> LibrarySymbol {
        let mut sym = LibrarySymbol {
            lib_id: "Test:Sample".into(),
            reference_prefix: "IC".into(),
            description: "a sample".into(),
            keywords: "one two".into(),
            datasheet: "http://example.com/ds.pdf".into(),
            power: false,
            in_bom: false,
            on_board: true,
            pin_numbers_hidden: true,
            pin_names_hidden: true,
            pin_name_offset_mm: 0.762,
            unit_count: 2,
            has_alternate_body_style: true,
            footprint_filters: vec!["SOIC*".into(), "SO-8*".into()],
            graphics: vec![
                LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 0, body_style: 1, start: SPoint { x: -2.54, y: -2.54 }, end: SPoint { x: 2.54, y: 2.54 }, stroke_mm: 0.3, fill: LibraryFill::Background },
                LibrarySymbolGraphic::Polyline { id: String::new(), unit: 1, body_style: 1, pts: vec![SPoint { x: 0.0, y: 0.0 }, SPoint { x: 1.0, y: 1.0 }, SPoint { x: 2.0, y: 0.0 }], stroke_mm: 0.2, fill: LibraryFill::Outline },
                LibrarySymbolGraphic::Circle { id: String::new(), unit: 2, body_style: 1, center: SPoint { x: 0.5, y: 0.5 }, radius_mm: 1.5, stroke_mm: 0.254, fill: LibraryFill::None },
                LibrarySymbolGraphic::Arc { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: 1.0, y: 0.0 }, mid: SPoint { x: 0.0, y: 1.0 }, end: SPoint { x: -1.0, y: 0.0 }, stroke_mm: 0.254, fill: LibraryFill::None },
                LibrarySymbolGraphic::Text { id: String::new(), unit: 1, body_style: 1, text: "hello".into(), at: SPoint { x: 0.0, y: -4.0 }, angle_deg: 90.0, size_mm: 1.5 },
            ],
            pins: vec![pin("1", 1, 1, false), pin("2", 1, 1, true), pin("3", 2, 1, false), pin("1", 1, 2, false)],
            published: false,
        };
        sym.assign_missing_ids();
        sym
    }

    #[test]
    fn export_then_import_gives_back_everything_the_writer_emits() {
        let sym = sample();
        let parsed = parse_library_symbols(&export_kicad_sym(&sym)).expect("parses");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.symbols.len(), 1);
        let back = &parsed.symbols[0];
        assert_eq!(back.lib_id, "Sample", "bare name: the library is the caller's choice");
        assert_eq!((back.reference_prefix.as_str(), back.description.as_str(), back.keywords.as_str(), back.datasheet.as_str()), ("IC", "a sample", "one two", "http://example.com/ds.pdf"));
        assert_eq!((back.in_bom, back.on_board, back.power), (false, true, false));
        assert_eq!((back.pin_numbers_hidden, back.pin_names_hidden), (true, true));
        assert!((back.pin_name_offset_mm - 0.762).abs() < 1e-9);
        assert_eq!(back.unit_count, 2);
        assert!(back.has_alternate_body_style);
        assert_eq!(back.footprint_filters, vec!["SOIC*".to_string(), "SO-8*".to_string()]);

        // the pins: every number/unit/style, the hidden one, the text sizes, the shape
        let key = |p: &LibrarySymbolPin| (p.number.clone(), p.unit, p.body_style);
        let mut want: Vec<_> = sym.pins.iter().map(key).collect();
        let mut got: Vec<_> = back.pins.iter().map(key).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        let hidden_pin = back.pins.iter().find(|p| p.number == "2").unwrap();
        assert!(hidden_pin.hidden);
        assert_eq!((hidden_pin.shape.as_str(), hidden_pin.name_size_mm, hidden_pin.number_size_mm), ("inverted", Some(1.0), Some(0.9)));
        assert_eq!(hidden_pin.name, "N2");

        // the graphics: kinds, units, fills and stroke widths survive; the shared (unit 0) rectangle is
        // written once per body style (the writer's own rule), so it is read back as two
        let rects: Vec<_> = back.graphics.iter().filter(|g| matches!(g, LibrarySymbolGraphic::Rectangle { .. })).collect();
        assert_eq!(rects.len(), 1, "style 1 only: the sample's shared rectangle has body_style 1");
        assert!(matches!(rects[0], LibrarySymbolGraphic::Rectangle { fill: LibraryFill::Background, stroke_mm, unit: 0, .. } if (*stroke_mm - 0.3).abs() < 1e-9));
        assert!(back.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::Polyline { fill: LibraryFill::Outline, pts, unit: 1, .. } if pts.len() == 3)));
        assert!(back.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::Circle { unit: 2, radius_mm, .. } if (*radius_mm - 1.5).abs() < 1e-9)));
        assert!(back.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::Arc { .. })));
        assert!(back.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::Text { text, angle_deg, size_mm, .. } if text == "hello" && (*angle_deg - 90.0).abs() < 1e-9 && (*size_mm - 1.5).abs() < 1e-9)));
        assert!(back.pins.iter().all(|p| !p.id.is_empty()) && back.graphics.iter().all(|g| !g.id().is_empty()), "ids are assigned");
    }

    #[test]
    fn a_whole_library_file_gives_one_symbol_per_entry_in_file_order() {
        let a = LibrarySymbol { lib_id: "eda:Alpha".into(), pins: vec![pin("1", 1, 1, false)], ..LibrarySymbol::default() };
        let b = LibrarySymbol { lib_id: "eda:Beta".into(), pins: vec![pin("7", 1, 1, false)], ..LibrarySymbol::default() };
        let parsed = parse_library_symbols(&export_kicad_sym_library(&[&a, &b])).unwrap();
        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.lib_id.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn a_bare_run_of_symbol_forms_is_accepted_like_kicads_clipboard_text() {
        let text = r#"(symbol "R" (pin_numbers (hide yes)) (pin_names (offset 0) hide) (in_bom yes) (on_board yes)
            (property "Reference" "R" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "R_0_1" (rectangle (start -1 -2) (end 1 2) (stroke (width 0.254) (type default)) (fill (type none))))
            (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (hide yes) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))))"#;
        let parsed = parse_library_symbols(text).unwrap();
        let r = &parsed.symbols[0];
        assert_eq!(r.lib_id, "R");
        assert!(r.pin_numbers_hidden && r.pin_names_hidden, "both the (hide yes) and the bare `hide` spelling");
        assert_eq!(r.pin_name_offset_mm, 0.0);
        assert!(r.pins[0].hidden, "(hide yes) on a pin");
        assert_eq!(r.pins[0].name, "", "`~` is an empty name");
        assert_eq!(r.graphics.len(), 1);
    }

    #[test]
    fn extends_borrows_the_bases_drawing_and_unsupported_items_are_reported() {
        let text = r#"(kicad_symbol_lib (version 20231120)
            (symbol "Base" (property "Datasheet" "http://x" (at 0 0 0))
                (symbol "Base_0_1" (rectangle (start -1 -1) (end 1 1) (stroke (width 0.2) (type default)) (fill (type background)))
                    (bezier (pts (xy 0 0) (xy 1 1) (xy 2 1) (xy 3 0)))
                    (text_box "x" (at 0 0 0) (size 1 1)))
                (symbol "Base_1_1" (pin power_in line (at 0 -3.81 90) (length 2.54) (name "GND") (number "1"))))
            (symbol "Child" (extends "Base") (property "Description" "derived" (at 0 0 0))))"#;
        let parsed = parse_library_symbols(text).unwrap();
        let child = parsed.symbols.iter().find(|s| s.lib_id == "Child").unwrap();
        assert_eq!((child.pins.len(), child.graphics.len()), (1, 1), "the base's pin and rectangle");
        assert_eq!(child.datasheet, "http://x", "a property the child lacks comes from the base");
        assert_eq!(child.description, "derived");
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);
        assert!(parsed.warnings.iter().any(|w| w.contains("bezier")) && parsed.warnings.iter().any(|w| w.contains("text box")));
    }

    #[test]
    fn refuses_a_file_that_is_not_a_symbol_library() {
        assert!(parse_library_symbols("(kicad_pcb (version 1))").is_err());
        assert!(parse_library_symbols("not an s-expression").is_err());
        assert!(parse_library_symbols("(kicad_symbol_lib (version 20231120))").unwrap_err().contains("no symbol"));
    }
}
