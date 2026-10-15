//! A symbol cross-reference in a text we write, in the form KiCad stores it.
//!
//! The model keeps a cross-reference as the user types it, `${R1:VALUE}`, and the studio resolves it by reference designator
//! (`eda_model::text_vars`). KiCad stores it by the symbol's place in the hierarchy instead: `SCHEMATIC::ConvertRefsToKIIDs`
//! (`eeschema/schematic.cpp`) turns the typed form into `${/<sheet uuid path>/<symbol uuid>:VALUE}` when a text is edited, and that is
//! what its files hold. A KiCad of 10.99.0-5334 crashes plotting a file whose cross-reference names a designator rather than a path
//! (`SCHEMATIC::resolveCrossReference` builds a `KIID_PATH` from the designator and the stack overflows unwinding the parse error), so
//! [`refs_to_kiids`] writes every cross-reference the way KiCad itself would have saved it.
//!
//! A cross-reference that names no symbol cannot have a path. KiCad shows it as `<Unresolved: R9>`, and so does the studio
//! (`TextVarScope::resolve_cross_reference`); that text is what gets written, so the file plots as the studio draws it and no kicad-cli
//! stumbles over it.

use crate::sexpr;
use std::collections::BTreeMap;

/// Rewrite the symbol cross-references of every file of one schematic (the root and its sheets) into KiCad's stored form.
pub fn refs_to_kiids(files: &mut [(String, String)]) {
    let paths = symbol_paths(files);
    for (_, text) in files.iter_mut() {
        if text.contains("${") {
            *text = rewrite(text, &paths);
        }
    }
}

/// Every placed symbol's designator -> `/<sheet path>/<symbol uuid>`, read off the `instances` blocks the files carry: the designator
/// with its unit letter (`U1A`, what `GetRef( .., true )` gives) and, for the first unit met, the designator alone (`U1`, the parent
/// reference `resolveCrossReference` falls back to). Power symbols (`#PWR01`) are left out, as `SYMBOL_FILTER_NON_POWER` leaves them.
fn symbol_paths(files: &[(String, String)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (_, text) in files {
        let Ok(tree) = sexpr::parse(text) else { continue };
        let Some(root) = tree.as_list() else { continue };
        for sym in sexpr::find_all(root, "symbol").filter(|s| sexpr::find(s, "lib_id").is_some()) {
            let Some(uuid) = sexpr::find(sym, "uuid").and_then(|u| sexpr::txt(u, 1)) else { continue };
            let Some(instances) = sexpr::find(sym, "instances") else { continue };
            for project in sexpr::find_all(instances, "project") {
                for path in sexpr::find_all(project, "path") {
                    let (Some(sheet_path), Some(reference)) = (sexpr::txt(path, 1), sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1))) else { continue };
                    if reference.starts_with('#') {
                        continue;
                    }
                    let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0) as u32;
                    let kiid = format!("{}/{uuid}", sheet_path.trim_end_matches('/'));
                    out.entry(format!("{reference}{}", unit_letter(unit))).or_insert_with(|| kiid.clone());
                    out.entry(reference.to_string()).or_insert(kiid);
                }
            }
        }
    }
    out
}

/// `LIB_SYMBOL::LetterSubReference( aUnit, 'A' )`: 1 is A, 26 is Z, 27 is AA.
fn unit_letter(unit: u32) -> String {
    let mut n = unit.max(1);
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

/// `ConvertRefsToKIIDs` over the text of one file: each `${REF:...}` outside an escaped `\${...}` (written `\\${` in the file) gets its
/// designator replaced by the symbol's path; a token that already starts with a path is left as it is.
fn rewrite(text: &str, paths: &BTreeMap<String, String>) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 64);
    let mut i = 0;
    let mut copied = 0;
    while i + 1 < b.len() {
        if b[i] == b'$' && b[i + 1] == b'{' {
            let escaped = i >= 2 && b[i - 1] == b'\\' && b[i - 2] == b'\\';
            let Some(end) = token_end(b, i + 2) else { break };
            if !escaped {
                let token = &text[i + 2..end];
                if let Some(replacement) = convert(token, paths) {
                    out.push_str(&text[copied..i]);
                    out.push_str(&replacement);
                    copied = end + 1;
                }
            }
            i = end + 1;
            continue;
        }
        i += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// Where a `${...}` that starts at `from` ends: its `}`, counting the `_{`, `^{` and `~{` of KiCad's text markup inside it as
/// `ConvertRefsToKIIDs` does.
fn token_end(b: &[u8], from: usize) -> Option<usize> {
    let mut nesting = 0i32;
    for i in from..b.len() {
        if b[i] == b'{' && i > 0 && matches!(b[i - 1], b'_' | b'^' | b'~') {
            nesting += 1;
        }
        if b[i] == b'}' {
            nesting -= 1;
            if nesting < 0 {
                return Some(i);
            }
        }
        if b[i] == b'"' && (i == 0 || b[i - 1] != b'\\') {
            return None; // the quoted string ended first: no token here
        }
    }
    None
}

/// The written form of one token, or `None` to leave it as it is.
fn convert(token: &str, paths: &BTreeMap<String, String>) -> Option<String> {
    let (reference, remainder) = token.split_once(':')?;
    if reference.is_empty() || reference.starts_with('/') || reference.contains('{') {
        return None;
    }
    Some(match paths.get(reference) {
        Some(path) => format!("${{{path}:{remainder}}}"),
        // `resolveCrossReference`'s answer when no symbol has the designator.
        None => format!("<Unresolved: {reference}>"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(symbols: &str) -> String {
        format!("(kicad_sch (version 20250114) (uuid \"aaaaaaaa-0000-4000-8000-000000000000\") {symbols})")
    }

    fn symbol(uuid: &str, reference: &str, unit: u32, value: &str) -> String {
        format!(
            r#"(symbol (lib_id "Device:R") (at 0 0 0) (unit {unit}) (uuid "{uuid}") (property "Reference" "{reference}") (property "Value" "{value}")
                (instances (project "p" (path "/aaaaaaaa-0000-4000-8000-000000000000" (reference "{reference}") (unit {unit})))))"#
        )
    }

    #[test]
    fn a_designator_becomes_the_symbols_path() {
        let mut files = vec![(
            "root.kicad_sch".to_string(),
            sheet(&format!("{} {}", symbol("11111111-0000-4000-8000-000000000001", "R1", 1, "10k"), symbol("22222222-0000-4000-8000-000000000002", "R2", 1, "${R1:VALUE} and ${R1:Footprint:V2}"))),
        )];
        refs_to_kiids(&mut files);
        let text = &files[0].1;
        assert!(text.contains(r#""${/aaaaaaaa-0000-4000-8000-000000000000/11111111-0000-4000-8000-000000000001:VALUE} and ${/aaaaaaaa-0000-4000-8000-000000000000/11111111-0000-4000-8000-000000000001:Footprint:V2}""#), "{text}");
    }

    #[test]
    fn a_unit_letter_and_the_parent_reference_both_find_a_multi_unit_symbol() {
        let mut files = vec![(
            "root.kicad_sch".to_string(),
            sheet(&format!(
                "{} {} {}",
                symbol("33333333-0000-4000-8000-000000000003", "U1", 1, "LM358"),
                symbol("44444444-0000-4000-8000-000000000004", "U1", 2, "LM358"),
                symbol("55555555-0000-4000-8000-000000000005", "R3", 1, "${U1B:VALUE} ${U1:VALUE}")
            )),
        )];
        refs_to_kiids(&mut files);
        let text = &files[0].1;
        assert!(text.contains("${/aaaaaaaa-0000-4000-8000-000000000000/44444444-0000-4000-8000-000000000004:VALUE} ${/aaaaaaaa-0000-4000-8000-000000000000/33333333-0000-4000-8000-000000000003:VALUE}"), "{text}");
    }

    #[test]
    fn an_unknown_designator_is_written_as_kicad_shows_it_and_the_rest_is_left_alone() {
        let mut files = vec![(
            "root.kicad_sch".to_string(),
            sheet(&format!(
                "{} (text \"${{R9:VALUE}} ${{REV}} \\\\${{R1:VALUE}} ${{/x/y:VALUE}} ${{#PWR01:VALUE}}\")",
                symbol("11111111-0000-4000-8000-000000000001", "R1", 1, "10k")
            )),
        )];
        refs_to_kiids(&mut files);
        let text = &files[0].1;
        // unknown -> KiCad's text; a plain variable, an escaped token and a path stay; a power symbol is not looked up
        assert!(text.contains(r#""<Unresolved: R9> ${REV} \\${R1:VALUE} ${/x/y:VALUE} <Unresolved: #PWR01>""#), "{text}");
    }

    #[test]
    fn a_symbol_on_a_sheet_is_found_from_the_root() {
        let child = "(kicad_sch (version 20250114) (uuid \"bbbbbbbb-0000-4000-8000-000000000000\") (symbol (lib_id \"Device:C\") (at 0 0 0) (unit 1) (uuid \"66666666-0000-4000-8000-000000000006\") (property \"Reference\" \"C7\")
            (instances (project \"p\" (path \"/aaaaaaaa-0000-4000-8000-000000000000/cccccccc-0000-4000-8000-000000000000\" (reference \"C7\") (unit 1))))))";
        let mut files = vec![("root.kicad_sch".to_string(), sheet("(text \"C = ${C7:VALUE}\")")), ("child.kicad_sch".to_string(), child.to_string())];
        refs_to_kiids(&mut files);
        assert!(files[0].1.contains("${/aaaaaaaa-0000-4000-8000-000000000000/cccccccc-0000-4000-8000-000000000000/66666666-0000-4000-8000-000000000006:VALUE}"), "{}", files[0].1);
    }

    #[test]
    fn unit_letters_count_as_kicad_counts_them() {
        assert_eq!((unit_letter(1), unit_letter(2), unit_letter(26), unit_letter(27), unit_letter(28)), ("A".into(), "B".into(), "Z".into(), "AA".into(), "AB".into()));
    }
}
