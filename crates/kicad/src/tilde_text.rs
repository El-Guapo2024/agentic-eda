//! KiCad's `~` for an empty text in a schematic or a symbol library older than 20250318 (`SEXPR_SCHEMATIC_FILE_VERSION` /
//! `SEXPR_SYMBOL_LIB_FILE_VERSION` 20250318, "~ no longer means empty text"): `SCH_IO_KICAD_SEXPR_PARSER` reads a `~` as the empty text, in such a file
//! only, at each place it wrote one for an empty text -- a field's value (`parseProperty`, `parseSchField`: `m_requiredVersion < 20250318 && CurStr() ==
//! "~"`), a library pin's name and number (`parseSymbolPin`'s `T_name`, `T_number`) and an instance's value and footprint (`parseSchematicSymbol`'s and
//! `parseSchSymbolInstances`' `T_value`, `T_footprint`). From 20250318 on a `~` is the text `~` (a KiCad 5 legacy pin's name, which KiCad's legacy reader
//! keeps: kicad-cli names such a net `Net-(C5-~-Pad1)`). [`normalize`] rewrites an older tree so, before the readers see it, as `overbar::normalize` does for
//! the overbars; a newer one, or a form with no version of its own (the clipboard's, a cache), is left as it is (`parseHeader` takes the current version).
use crate::sexpr::{self, Sexpr};

/// `SEXPR_SCHEMATIC_FILE_VERSION` / `SEXPR_SYMBOL_LIB_FILE_VERSION` 20250318.
pub const TILDE_IS_TEXT_VERSION: i64 = 20250318;

/// Whether a tree whose root is `root` reads `~` as the empty text: it says a version, and an older one.
pub(crate) fn tilde_is_empty(root: &[Sexpr]) -> bool {
    sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).is_some_and(|v| (v as i64) < TILDE_IS_TEXT_VERSION)
}

/// Rewrite every `~` the parser of the tree's version reads as empty into the empty text (see the module docs); nothing for a newer tree.
pub(crate) fn normalize(tree: &mut Sexpr) {
    let old = tree.as_list().is_some_and(tilde_is_empty);
    if old {
        empty_tildes(tree, false);
    }
}

/// The places: a `(property [private] "name" "~" ..)`'s value, a library pin's `(name "~" ..)` and `(number "~" ..)`, an instance's `(value "~")` and
/// `(footprint "~")`.
fn empty_tildes(node: &mut Sexpr, in_pin: bool) {
    let Sexpr::List(items) = node else { return };
    let tag = sexpr::tag(items).map(str::to_string);
    let at = match tag.as_deref() {
        Some("property") => Some(if sexpr::txt(items, 1) == Some("private") && items.get(3).is_some_and(|n| n.text().is_some()) { 3 } else { 2 }),
        Some("name") | Some("number") if in_pin => Some(1),
        Some("value") | Some("footprint") => Some(1),
        _ => None,
    };
    if let Some(i) = at {
        if let Some(a) = items.get_mut(i) {
            if a.text() == Some("~") {
                sexpr::set_text(a, "");
            }
        }
    }
    // a library pin (`(pin <type> <shape> ..)`), not a placed symbol's `(pin "1" (uuid ..))` nor a sheet's `(pin "name" input ..)`, whose texts are not read so
    let pin = tag.as_deref() == Some("pin") && sexpr::find(items, "name").is_some();
    for child in items.iter_mut().skip(1) {
        empty_tildes(child, pin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized(text: &str) -> String {
        let mut t = sexpr::parse(text).unwrap();
        normalize(&mut t);
        sexpr::to_text(&t)
    }

    #[test]
    fn an_older_file_reads_a_tilde_as_empty_where_the_parser_does_and_a_newer_one_keeps_it() {
        let body = r#"(lib_symbols (symbol "Device:R" (property "Datasheet" "~" (at 0 0 0)) (property private "Note" "~") (symbol "R_1_1" (pin passive line (at 0 0 0) (length 1) (name "~" (effects (font (size 1.27 1.27)))) (number "~"))))) (symbol (lib_id "Device:R") (property "Value" "~") (pin "1" (uuid "u")) (instances (project "p" (path "/a" (reference "R1") (unit 1) (value "~") (footprint "~"))))) (text "~" (at 0 0 0)) (sheet (pin "~" input (at 0 0 0)))"#;
        let old = normalized(&format!("(kicad_sch (version 20250114) {body})"));
        assert_eq!(old.matches("\"~\"").count(), 2, "only the free text's and the sheet pin's stay: {old}");
        assert!(old.contains(r#"(property "Datasheet" """#) && old.contains(r#"(property private "Note" "")"#) && old.contains(r#"(name """#) && old.contains(r#"(number "")"#), "{old}");
        assert!(old.contains(r#"(value "")"#) && old.contains(r#"(footprint "")"#) && old.contains(r#"(property "Value" "")"#), "{old}");
        assert!(old.contains(r#"(text "~""#) && old.contains(r#"(pin "~" input"#), "a free text and a sheet pin's name are not read so: {old}");
        let new = format!("(kicad_sch (version 20250318) {body})");
        assert_eq!(normalized(&new), sexpr::to_text(&sexpr::parse(&new).unwrap()), "20250318 and later: `~` is a text");
        let none = format!("(kicad_sch {body})");
        assert_eq!(normalized(&none), sexpr::to_text(&sexpr::parse(&none).unwrap()), "no version: the current format");
    }
}
