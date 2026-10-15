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
//! curves -- each reported in `warnings`, never dropped silently); a pin's alternate functions come back as the pin's `alternates`.
//!
//! Grammar notes confirmed against the KiCad source this repo ports (`eeschema/sch_io/kicad_sexpr`):
//! a symbol's own `(symbol "Name_<unit>_<style>" ...)` sub-blocks carry the drawn items (unit 0 = shared
//! by every unit, style 0 = shared by every body style); `(extends "Parent")` names a base symbol of
//! the same file whose graphics and pins this one shows -- the symbol is read as a *derived* one
//! ([`LibrarySymbol::extends`]: its own properties, none of the drawing; `SCH_IO_KICAD_SEXPR_PARSER::parseSymbol`
//! keeps the link too) and `eda_model::SymbolLibrarySection::flatten` gives the symbol with everything it inherits; a pin
//! or pin-number/name block hides with either the bare atom `hide` (older files) or `(hide yes)` (KiCad 9+);
//! every `(property ...)` is a field with its place and look (`parseProperty`), `ki_keywords`, `ki_fp_filters`
//! and `ki_description` being the symbol's own members and `ki_locked` a flag that is not kept.

use eda_model::ir::{LibraryFill, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin};
use eda_model::sym_fields::{is_mandatory_field, FieldHAlign, FieldVAlign, LibrarySymbolField, MANDATORY_FIELDS};
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

    let mut parsed = symbols_of(root);
    if parsed.symbols.is_empty() {
        return Err("the file holds no symbol".into());
    }
    parsed.symbols = parents_first(parsed.symbols);
    Ok(parsed)
}

/// The symbols in file order, except that a symbol comes after the symbol it extends when that one is in the list too (a file does not have to put a base before its derived
/// symbols, and the library takes them one by one: a derived symbol needs its parent already there).
fn parents_first(symbols: Vec<LibrarySymbol>) -> Vec<LibrarySymbol> {
    let names: Vec<String> = symbols.iter().map(|s| s.lib_id.clone()).collect();
    let mut placed: Vec<bool> = vec![false; symbols.len()];
    let mut order: Vec<usize> = Vec::with_capacity(symbols.len());
    fn visit(i: usize, symbols: &[LibrarySymbol], names: &[String], placed: &mut [bool], order: &mut Vec<usize>, depth: usize) {
        if placed[i] {
            return;
        }
        placed[i] = true; // before the parent: a cycle in the file ends here
        if let Some(parent) = symbols[i].extends.as_deref() {
            if let Some(p) = names.iter().position(|n| n == parent) {
                if depth < 64 {
                    visit(p, symbols, names, placed, order, depth + 1);
                }
            }
        }
        order.push(i);
    }
    for i in 0..symbols.len() {
        visit(i, &symbols, &names, &mut placed, &mut order, 0);
    }
    let mut slots: Vec<Option<LibrarySymbol>> = symbols.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// Every `(symbol "Name" ...)` child of `list` as an editable [`LibrarySymbol`], named as the node names it -- a bare name in a library
/// file, the full `Library:Name` in a schematic's `(lib_symbols ...)` cache, whose units are still `Name_<unit>_<style>` (the part
/// after the colon). The reader behind both the Symbol Editor's import and the schematic clipboard.
pub(crate) fn symbols_of(list: &[Sexpr]) -> ParsedSymbols {
    let items: Vec<(&str, &[Sexpr])> = sexpr::find_all(list, "symbol").filter_map(|it| Some((sexpr::txt(it, 1)?, it))).collect();

    let mut warnings = Vec::new();
    let symbols = items.iter().map(|(name, item)| build(name, item, &mut warnings)).collect();
    warnings.sort();
    warnings.dedup();
    ParsedSymbols { symbols, warnings }
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

/// What a symbol's `(property ...)` forms hold: its fields, with the three `ki_*` properties that are the symbol's own members (`parseProperty`).
#[derive(Default)]
pub(crate) struct Properties {
    /// Every field in file order: the mandatory ones (by their canonical names; the match is not case sensitive) and the user fields. A user field whose name is already taken
    /// is renamed `name_1`, `name_2` ..., and after nine tries dropped, as `parseProperty` does.
    pub fields: Vec<LibrarySymbolField>,
    pub keywords: String,
    pub footprint_filters: Vec<String>,
    /// `ki_description`, where a file of KiCad 7 or before kept the description.
    pub legacy_description: Option<String>,
}

/// `parseEDA_TEXT`'s `(effects (font (size h w) bold italic) (justify ...) (hide yes))` into a field: the size is the height, `bold` and `italic` are bare tokens or `(bold yes)`,
/// a `(hide ...)` among the effects hides the field.
fn read_effects(field: &mut LibrarySymbolField, effects: &[Sexpr]) {
    let flag = |node: &[Sexpr], key: &str| {
        node.iter().skip(1).any(|c| match c {
            Sexpr::Atom(a) => a == key,
            Sexpr::List(l) => sexpr::tag(l) == Some(key) && sexpr::txt(l, 1).map(|v| v == "yes").unwrap_or(true),
        })
    };
    if let Some(font) = sexpr::find(effects, "font") {
        if let Some(size) = sexpr::find(font, "size").and_then(|s| sexpr::num(s, 1)) {
            field.size_mm = size;
        }
        field.bold = flag(font, "bold");
        field.italic = flag(font, "italic");
    }
    if let Some(justify) = sexpr::find(effects, "justify") {
        for c in justify.iter().skip(1).filter_map(Sexpr::text) {
            match c {
                "left" => field.h_align = FieldHAlign::Left,
                "right" => field.h_align = FieldHAlign::Right,
                "top" => field.v_align = FieldVAlign::Top,
                "bottom" => field.v_align = FieldVAlign::Bottom,
                _ => {}
            }
        }
    }
    if flag(effects, "hide") {
        field.hidden = true;
    }
}

/// `SCH_IO_KICAD_SEXPR_PARSER::parseProperty` for every `(property ...)` of a symbol's own node.
pub(crate) fn read_properties(item: &[Sexpr]) -> Properties {
    let mut out = Properties::default();
    for prop in sexpr::find_all(item, "property") {
        // `(property private "Name" ...)`: the private flag is not kept
        let at = if prop.get(1).and_then(Sexpr::text) == Some("private") { 2 } else { 1 };
        let (Some(name), Some(value)) = (sexpr::txt(prop, at), sexpr::txt(prop, at + 1)) else { continue };
        if name.is_empty() {
            continue;
        }
        match name {
            "ki_keywords" => out.keywords = value.to_string(),
            "ki_description" => out.legacy_description = Some(value.to_string()),
            "ki_fp_filters" => out.footprint_filters = value.split_whitespace().map(str::to_string).collect(),
            "ki_locked" => {}
            _ => {
                let mut f = LibrarySymbolField::named(name, value);
                if let Some(canonical) = MANDATORY_FIELDS.iter().find(|m| m.eq_ignore_ascii_case(name)) {
                    f.name = (*canonical).to_string();
                    f.hidden = false;
                }
                if let Some(a) = sexpr::find(prop, "at") {
                    f.at = SPoint { x: sexpr::num(a, 1).unwrap_or(0.0), y: sexpr::num(a, 2).unwrap_or(0.0) };
                    f.angle_deg = sexpr::num(a, 3).unwrap_or(0.0);
                }
                if let Some(h) = sexpr::find(prop, "hide") {
                    f.hidden = sexpr::txt(h, 1).map(|v| v == "yes").unwrap_or(true);
                }
                if let Some(n) = sexpr::find(prop, "show_name") {
                    f.show_name = sexpr::txt(n, 1).map(|v| v == "yes").unwrap_or(true);
                }
                if let Some(n) = sexpr::find(prop, "do_not_autoplace") {
                    f.do_not_autoplace = sexpr::txt(n, 1).map(|v| v == "yes").unwrap_or(true);
                }
                if let Some(effects) = sexpr::find(prop, "effects") {
                    read_effects(&mut f, effects);
                }
                if is_mandatory_field(&f.name) {
                    if let Some(slot) = out.fields.iter_mut().find(|x| x.name == f.name) {
                        *slot = f;
                    } else {
                        out.fields.push(f);
                    }
                    continue;
                }
                // a user field: a second one of the same name gets `name_1` ... `name_9`, then is dropped
                let mut unique = f.name.clone();
                let mut n = 0;
                while out.fields.iter().any(|x| x.name == unique) && n < 9 {
                    n += 1;
                    unique = format!("{}_{n}", f.name);
                }
                if !out.fields.iter().any(|x| x.name == unique) {
                    f.name = unique;
                    out.fields.push(f);
                }
            }
        }
    }
    out
}

fn build(name: &str, item: &[Sexpr], warnings: &mut Vec<String>) -> LibrarySymbol {
    let extends = sexpr::find(item, "extends").and_then(|e| sexpr::txt(e, 1)).map(str::to_string);

    // A derived symbol keeps no drawing of its own (`SaveSymbol` writes none): what it draws, its units and its pin text options are its root's, which `flatten` takes.
    let (graphics, pins, unit_count, alt_style) = if extends.is_some() {
        let own = drawn_items(item, warnings);
        if !own.0.is_empty() || !own.1.is_empty() {
            warnings.push("a derived symbol has a drawing of its own, which is not used: it shows its parent's".to_string());
        }
        (Vec::new(), Vec::new(), 1, false)
    } else {
        drawn_items(item, warnings)
    };

    let power = |node: &[Sexpr]| node.iter().any(|it| it.as_list().is_some_and(|l| sexpr::tag(l) == Some("power")) || it.text() == Some("power"));
    let pin_numbers = sexpr::find(item, "pin_numbers");
    let pin_names = sexpr::find(item, "pin_names");
    let props = read_properties(item);
    let mut sym = LibrarySymbol {
        lib_id: name.to_string(),
        power: power(item),
        in_bom: yes_no(item, "in_bom", true),
        on_board: yes_no(item, "on_board", true),
        pin_numbers_hidden: pin_numbers.map(hidden).unwrap_or(false),
        pin_names_hidden: pin_names.map(hidden).unwrap_or(false),
        keywords: props.keywords.clone(),
        footprint_filters: props.footprint_filters.clone(),
        unit_count: unit_count.max(1),
        has_alternate_body_style: alt_style,
        graphics,
        pins,
        extends,
        ..LibrarySymbol::default()
    };
    if let Some(offset) = pin_names.and_then(|p| sexpr::find(p, "offset")).and_then(|o| sexpr::num(o, 1)) {
        sym.pin_name_offset_mm = offset;
    }
    // The fields. A root symbol has its Reference prefix ("U" when the file says none, as it always was); a derived symbol's empty Reference or Description is the parent's.
    let derived = sym.extends.is_some();
    let mut fields = props.fields;
    if let Some(legacy) = &props.legacy_description {
        match fields.iter_mut().find(|f| f.name == "Description") {
            Some(f) if f.text.is_empty() => f.text = legacy.clone(),
            Some(_) => {}
            None => fields.push(LibrarySymbolField { hidden: true, ..LibrarySymbolField::named("Description", legacy) }),
        }
    }
    sym.set_fields(fields);
    if !derived && sym.reference_prefix.is_empty() {
        sym.reference_prefix = "U".to_string();
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
        alternates: pin_alternates(item),
    })
}

/// A pin's `(alternate "name" <type> <shape>)` forms (`SCH_IO_KICAD_SEXPR_PARSER::parseSymbol`, `T_alternate`): kept by name -- the file's `std::map`, so a name given twice keeps its
/// last definition -- and sorted by it, as the map holds them. A nameless one is what the library cache's writer strips ("a bug somewhere in the alternate pin code"), so it is not read.
fn pin_alternates(pin: &[Sexpr]) -> Vec<eda_model::ir::PinAlternate> {
    let mut map: std::collections::BTreeMap<String, eda_model::ir::PinAlternate> = std::collections::BTreeMap::new();
    for alt in sexpr::find_all(pin, "alternate") {
        let Some(name) = sexpr::txt(alt, 1).filter(|n| !n.is_empty()) else { continue };
        let electrical_type = sexpr::txt(alt, 2).unwrap_or("passive").to_string();
        let shape = sexpr::txt(alt, 3).filter(|s| !s.is_empty()).unwrap_or("line").to_string();
        map.insert(name.to_string(), eda_model::ir::PinAlternate { name: name.to_string(), electrical_type, shape });
    }
    map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export_kicad_sym, export_kicad_sym_library};
    use eda_model::sym_fields::{FieldHAlign, FieldVAlign, LibrarySymbolField};

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
            alternates: Vec::new(),
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
            extends: None,
            fields: vec![],
        };
        sym.assign_missing_ids();
        sym
    }

    /// `(alternate "name" type shape)` of a pin: written in the map's order after the name and the number (`savePin`), read back by name -- a name given twice keeps its last
    /// definition (`pin->GetAlternates()[ alt.m_Name ] = alt`), a nameless one is not read -- and a file KiCad wrote reads as the same alternates.
    #[test]
    fn a_pins_alternate_functions_are_written_as_kicad_writes_them_and_read_back() {
        use eda_model::ir::PinAlternate;
        let alt = |name: &str, t: &str, shape: &str| PinAlternate { name: name.into(), electrical_type: t.into(), shape: shape.into() };
        let mut sym = LibrarySymbol::new_empty("Test:Mcu");
        let mut p = pin("7", 1, 1, false);
        p.alternates = vec![alt("UART_TX", "output", "line"), alt("GPIO7", "bidirectional", "inverted"), alt("", "input", "line"), alt("ADC1", "input", "clock")];
        sym.pins = vec![p, pin("8", 1, 1, false)];
        sym.has_alternate_body_style = false;
        sym.unit_count = 1;
        sym.assign_missing_ids();
        let text = export_kicad_sym(&sym);
        // by name, the nameless one left out
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} is written:\n{text}"));
        assert!(at("(alternate \"ADC1\" input clock)") < at("(alternate \"GPIO7\" bidirectional inverted)") && at("(alternate \"GPIO7\"") < at("(alternate \"UART_TX\" output line)"), "{text}");
        assert!(!text.contains("(alternate \"\""), "an alternate with no name is not written");
        assert!(text.contains("(number \"7\" (effects (font (size 0.9 0.9)))) (alternate \"ADC1\""), "after the number, in the pin's own parentheses: {text}");
        let back = parse_library_symbols(&text).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let names: Vec<_> = back.symbols[0].pins.iter().find(|p| p.number == "7").unwrap().alternates.iter().map(|a| (a.name.as_str(), a.electrical_type.as_str(), a.shape.as_str())).collect();
        assert_eq!(names, [("ADC1", "input", "clock"), ("GPIO7", "bidirectional", "inverted"), ("UART_TX", "output", "line")]);
        assert!(back.symbols[0].pins.iter().find(|p| p.number == "8").unwrap().alternates.is_empty());

        // a file as KiCad writes it: a name given twice keeps the last
        let kicad = r#"(kicad_symbol_lib (version 20231120) (generator "kicad_symbol_editor")
            (symbol "Mcu" (symbol "Mcu_1_1" (pin bidirectional line (at 0 0 0) (length 2.54)
                (name "PA0" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))
                (alternate "TIM2_CH1" bidirectional line) (alternate "ADC_IN0" input line) (alternate "TIM2_CH1" output clock)))))"#;
        let parsed = parse_library_symbols(kicad).unwrap();
        let alts = &parsed.symbols[0].pins[0].alternates;
        assert_eq!(alts.iter().map(|a| (a.name.as_str(), a.electrical_type.as_str(), a.shape.as_str())).collect::<Vec<_>>(), [("ADC_IN0", "input", "line"), ("TIM2_CH1", "output", "clock")]);
        assert!(parsed.warnings.is_empty(), "alternates are no longer a warning: {:?}", parsed.warnings);
        // and written again says the same
        let again = parse_library_symbols(&export_kicad_sym(&parsed.symbols[0])).unwrap();
        assert_eq!(again.symbols[0].pins[0].alternates, *alts);
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
    fn a_derived_symbol_is_read_as_derived_and_flattens_onto_its_parents_drawing() {
        let text = r#"(kicad_symbol_lib (version 20231120)
            (symbol "Child" (extends "Base") (property "Description" "derived" (at 0 0 0)) (property "Value" "CH" (at 1 2 0) (effects (font (size 1 1) bold) (justify left bottom))))
            (symbol "Base" (property "Datasheet" "http://x" (at 0 0 0))
                (symbol "Base_0_1" (rectangle (start -1 -1) (end 1 1) (stroke (width 0.2) (type default)) (fill (type background)))
                    (bezier (pts (xy 0 0) (xy 1 1) (xy 2 1) (xy 3 0)))
                    (text_box "x" (at 0 0 0) (size 1 1)))
                (symbol "Base_1_1" (pin power_in line (at 0 -3.81 90) (length 2.54) (name "GND") (number "1")))))"#;
        let parsed = parse_library_symbols(text).unwrap();
        // the base first, though the file lists the derived symbol before it
        assert_eq!(parsed.symbols.iter().map(|s| s.lib_id.as_str()).collect::<Vec<_>>(), ["Base", "Child"]);
        let child = &parsed.symbols[1];
        assert_eq!(child.extends.as_deref(), Some("Base"));
        assert!(child.pins.is_empty() && child.graphics.is_empty(), "a derived symbol keeps no drawing of its own");
        assert_eq!((child.description.as_str(), child.datasheet.as_str()), ("derived", ""), "its own properties only: the Datasheet is the parent's until flattened");
        let value = child.field("Value").unwrap();
        assert_eq!((value.text.as_str(), value.at.x, value.at.y, value.size_mm, value.bold, value.h_align, value.v_align), ("CH", 1.0, 2.0, 1.0, true, FieldHAlign::Left, FieldVAlign::Bottom));
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);
        assert!(parsed.warnings.iter().any(|w| w.contains("bezier")) && parsed.warnings.iter().any(|w| w.contains("text box")));

        let section = eda_model::ir::SymbolLibrarySection { symbols: parsed.symbols };
        let flat = section.flatten("Child").unwrap();
        assert_eq!((flat.pins.len(), flat.graphics.len()), (1, 1), "the base's pin and rectangle");
        assert_eq!((flat.datasheet.as_str(), flat.description.as_str()), ("http://x", "derived"));
        assert_eq!(flat.field("Value").unwrap().text, "CH");
    }

    #[test]
    fn every_property_is_a_field_with_its_place_and_look_and_the_ki_ones_are_the_symbols_members() {
        let text = r#"(symbol "Chip"
            (property "Reference" "IC" (at -1.5 3 90) (effects (font (size 1.5 1.5) italic) (justify right)))
            (property "Value" "Chip" (at 0 -3 0) (show_name yes) (do_not_autoplace yes) (effects (font (size 1.27 1.27))))
            (property "Footprint" "Pkg:Chip" (at 0 0 0) (hide yes) (effects (font (size 1.27 1.27))))
            (property "Datasheet" "http://ds" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
            (property "Vendor" "ACME" (at 2 4 0) (effects (font (size 0.8 0.8))))
            (property "Vendor" "twin" (at 2 5 0))
            (property private "Secret" "s" (at 0 0 0))
            (property "ki_keywords" "a b" (at 0 0 0))
            (property "ki_description" "old style" (at 0 0 0))
            (property "ki_fp_filters" "SOIC* DIP*" (at 0 0 0))
            (property "ki_locked" "" (at 0 0 0)))"#;
        let sym = &parse_library_symbols(text).unwrap().symbols[0];
        assert_eq!((sym.reference_prefix.as_str(), sym.datasheet.as_str(), sym.keywords.as_str()), ("IC", "http://ds", "a b"));
        assert_eq!(sym.description, "old style", "a file of KiCad 7 kept the description in ki_description");
        assert_eq!(sym.footprint_filters, ["SOIC*", "DIP*"]);
        let r = sym.field("Reference").unwrap();
        assert_eq!((r.at.x, r.at.y, r.angle_deg, r.size_mm, r.italic, r.h_align), (-1.5, 3.0, 90.0, 1.5, true, FieldHAlign::Right));
        let v = sym.field("Value").unwrap();
        assert!(v.show_name && v.do_not_autoplace && !v.hidden);
        assert!(sym.field("Footprint").unwrap().hidden, "(hide yes) as a form of the property");
        assert!(sym.field("Datasheet").unwrap().hidden, "(hide yes) among the effects");
        let users: Vec<(String, String)> = sym.user_fields().into_iter().map(|f| (f.name, f.text)).collect();
        assert_eq!(users, [("Vendor".to_string(), "ACME".to_string()), ("Vendor_1".to_string(), "twin".to_string()), ("Secret".to_string(), "s".to_string())], "a second Vendor is renamed Vendor_1; private is dropped; the ki_ properties are no fields");
    }

    #[test]
    fn a_derived_symbol_is_written_as_kicad_writes_it_and_reads_back_the_same() {
        let mut base = LibrarySymbol { lib_id: "Test:Base".into(), reference_prefix: "U".into(), pins: vec![pin("1", 1, 1, false)], ..LibrarySymbol::default() };
        base.assign_missing_ids();
        let mut child = LibrarySymbol { lib_id: "Test:Child".into(), extends: Some("Test:Base".into()), keywords: "kw".into(), ..LibrarySymbol::default() };
        let mut fields = child.all_fields();
        fields[1].text = "CHILD".into();
        fields[1].at = SPoint { x: 1.5, y: -2.5 };
        fields[1].show_name = true;
        fields[3].text = "http://child".into();
        fields.push(LibrarySymbolField { name: "MPN".into(), text: "X-9".into(), hidden: true, bold: true, h_align: FieldHAlign::Right, ..Default::default() });
        child.set_fields(fields);
        child.reference_prefix.clear();
        let text = export_kicad_sym_library(&[&base, &child]);
        let block = &text[text.find("(symbol \"Child\"").unwrap()..];
        assert!(block.starts_with("(symbol \"Child\" (extends \"Base\")"), "KiCad's own `(symbol \"Name\" (extends \"Parent\")`: {block}");
        assert!(!block.contains("_1_1") && !block.contains("(pin "), "no drawing under a derived symbol");
        assert!(!block.contains("pin_names") && !block.contains("in_bom"), "no root options either: they are the root's");
        assert!(block.contains("(property \"Value\" \"CHILD\" (at 1.5 -2.5 0) (show_name yes)"), "{block}");
        assert!(block.contains("(property \"MPN\" \"X-9\" (at 0 0 0) (effects (font (size 1.27 1.27) bold) (justify right) (hide yes)))"), "{block}");
        assert!(block.contains("(property \"ki_keywords\" \"kw\""));

        let parsed = parse_library_symbols(&text).unwrap();
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.symbols.iter().map(|s| s.lib_id.as_str()).collect::<Vec<_>>(), ["Base", "Child"]);
        let back = &parsed.symbols[1];
        assert_eq!(back.extends.as_deref(), Some("Base"), "the file names the parent by its bare name: the library the symbol lands in is the caller's");
        assert_eq!((back.keywords.as_str(), back.datasheet.as_str()), ("kw", "http://child"));
        assert_eq!(back.field("Value").unwrap().at.y, -2.5);
        assert_eq!(back.field("MPN").unwrap().h_align, FieldHAlign::Right);
        assert_eq!(back.reference_prefix, "", "an empty Reference of a derived symbol stays empty: it is the parent's");
        // and writing what was read gives the same text again
        let again = export_kicad_sym_library(&[&parsed.symbols[0], &parsed.symbols[1]]);
        assert_eq!(again.replace("\"Base\"", "\"Test:Base\""), text.replace("\"Base\"", "\"Test:Base\""), "{again}\n---\n{text}");
    }

    #[test]
    fn the_fields_of_a_root_symbol_round_trip_with_their_layout() {
        let mut sym = LibrarySymbol { lib_id: "Test:R".into(), reference_prefix: "R".into(), datasheet: "http://r".into(), description: "a resistor".into(), ..LibrarySymbol::default() };
        let mut f = sym.all_fields();
        f[0].at = SPoint { x: 2.032, y: 0.0 };
        f[0].angle_deg = 90.0;
        f[1].text = "10k".into();
        f[1].at = SPoint { x: -2.0, y: 0.0 };
        f[1].v_align = FieldVAlign::Top;
        f[2].text = "Resistor_SMD:R_0603".into();
        f.push(LibrarySymbolField { name: "Tolerance".into(), text: "1%".into(), at: SPoint { x: 0.0, y: -5.0 }, size_mm: 1.0, italic: true, ..Default::default() });
        sym.set_fields(f);
        let back = &parse_library_symbols(&export_kicad_sym(&sym)).unwrap().symbols[0];
        assert_eq!(back.all_fields(), sym.all_fields());
        assert_eq!(back.fields, sym.fields, "the list the editor made is the list the reader makes");
    }

    #[test]
    fn refuses_a_file_that_is_not_a_symbol_library() {
        assert!(parse_library_symbols("(kicad_pcb (version 1))").is_err());
        assert!(parse_library_symbols("not an s-expression").is_err());
        assert!(parse_library_symbols("(kicad_symbol_lib (version 20231120))").unwrap_err().contains("no symbol"));
    }
}
