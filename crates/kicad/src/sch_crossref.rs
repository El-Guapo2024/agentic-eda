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

/// What `GetRef( &aPath, true )` adds to a reference for a placed unit (`SCH_SYMBOL::SubReference( unit )`): the unit in the project's notation
/// (`U1A`, `U1.1`, ...) or the unit's display name, given the symbol's lib id and unit; `None` for a symbol of one unit (`GetUnitCount() > 1` is false).
pub type UnitSuffix<'a> = dyn Fn(&str, u32) -> Option<String> + 'a;

/// Rewrite the symbol cross-references of every file of one schematic (the root and its sheets) into KiCad's stored form. `suffix` names a placed unit
/// as the project does (r7: `${U1.1:FIELD}` in a project of `.1` notation, `${U1A:FIELD}` in one of `A`).
pub fn refs_to_kiids(files: &mut [(String, String)], suffix: &UnitSuffix<'_>) {
    let paths = symbol_paths(files, suffix);
    for (_, text) in files.iter_mut() {
        if text.contains("${") {
            *text = rewrite(text, &paths);
        }
    }
}

/// Every placed symbol's designator -> `/<sheet path>/<symbol uuid>`, read off the `instances` blocks the files carry: the designator
/// with its unit as `GetRef( .., true )` gives it (`ConvertRefsToKIIDs` compares the typed reference with that: `U1A`, or `U1.1` in a project of that
/// notation, [`UnitSuffix`]) and, for the first unit met, the designator alone (`U1`, what KiCad 25ec2335's `resolveCrossReference` falls back to by
/// `GetRef( .., false )`). Power symbols (`#PWR01`) are left out, as `SYMBOL_FILTER_NON_POWER` leaves them.
fn symbol_paths(files: &[(String, String)], suffix: &UnitSuffix<'_>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (_, text) in files {
        let Ok(tree) = sexpr::parse(text) else { continue };
        let Some(root) = tree.as_list() else { continue };
        for sym in sexpr::find_all(root, "symbol").filter(|s| sexpr::find(s, "lib_id").is_some()) {
            let Some(uuid) = sexpr::find(sym, "uuid").and_then(|u| sexpr::txt(u, 1)) else { continue };
            let lib_id = sexpr::find(sym, "lib_id").and_then(|l| sexpr::txt(l, 1)).unwrap_or("");
            let Some(instances) = sexpr::find(sym, "instances") else { continue };
            for project in sexpr::find_all(instances, "project") {
                for path in sexpr::find_all(project, "path") {
                    let (Some(sheet_path), Some(reference)) = (sexpr::txt(path, 1), sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1))) else { continue };
                    if reference.starts_with('#') {
                        continue;
                    }
                    let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0) as u32;
                    let kiid = format!("{}/{uuid}", sheet_path.trim_end_matches('/'));
                    if let Some(sub) = suffix(lib_id, unit) {
                        out.entry(format!("{reference}{sub}")).or_insert_with(|| kiid.clone());
                    }
                    out.entry(reference.to_string()).or_insert(kiid);
                }
            }
        }
    }
    out
}

/// `ConvertRefsToKIIDs` over the text of one file: each `${REF:...}` outside an escaped `\${...}` (written `\\${` in the file) gets its
/// designator replaced by the symbol's path; a token that already starts with a path is left as it is.
fn rewrite(text: &str, paths: &BTreeMap<String, String>) -> String {
    rewrite_tokens(text, &|token| convert(token, paths))
}

/// `SCHEMATIC::ConvertKIIDsToRefs` over the files of one schematic as KiCad saved them: each `${/<sheet path>/<symbol uuid>:...}` names the symbol by
/// the reference `GetRef( &aPath, true )` gives it on that path (the unit in the project's notation `ids`, or the unit's display name, for a symbol of several
/// units), the form the model keeps a cross-reference in as the user types it, which [`refs_to_kiids`] writes back with the derived file's uuids. A path no
/// placed symbol has is looked up by its last uuid (`ResolveItem` finds the item anywhere); one that names nothing is left as it is.
pub fn kiids_to_refs(files: &mut [(String, String)], ids: &eda_model::annotate::SubpartIds) {
    if !files.iter().any(|(_, t)| t.contains("${/")) {
        return;
    }
    let mut by_path: BTreeMap<String, String> = BTreeMap::new();
    let mut by_uuid: BTreeMap<String, String> = BTreeMap::new();
    for (_, text) in files.iter() {
        let Ok(tree) = sexpr::parse(text) else { continue };
        let Some(root) = tree.as_list() else { continue };
        // the file's library symbols: how many units each has and their display names (`LIB_SYMBOL::GetUnitCount`, `GetUnitDisplayName`)
        let mut units: BTreeMap<String, (u32, BTreeMap<u32, String>)> = BTreeMap::new();
        if let Some(libs) = sexpr::find(root, "lib_symbols") {
            for lib in sexpr::find_all(libs, "symbol") {
                let Some(name) = sexpr::txt(lib, 1) else { continue };
                let stem = name.rsplit(':').next().unwrap_or(name);
                let mut count = 1;
                let mut names = BTreeMap::new();
                for sub in sexpr::find_all(lib, "symbol") {
                    let Some(sub_name) = sexpr::txt(sub, 1) else { continue };
                    let unit = sub_name.strip_prefix(stem).and_then(|r| r.strip_prefix('_')).and_then(|r| r.split('_').next()).and_then(|u| u.parse::<u32>().ok()).unwrap_or(0);
                    count = count.max(unit);
                    if let Some(display) = sexpr::find(sub, "unit_name").and_then(|u| sexpr::txt(u, 1)).filter(|d| !d.is_empty()) {
                        names.insert(unit, display.to_string());
                    }
                }
                units.insert(name.to_string(), (count, names));
            }
        }
        for sym in sexpr::find_all(root, "symbol").filter(|s| sexpr::find(s, "lib_id").is_some()) {
            let Some(uuid) = sexpr::find(sym, "uuid").and_then(|u| sexpr::txt(u, 1)) else { continue };
            let lib = sexpr::find(sym, "lib_name").or_else(|| sexpr::find(sym, "lib_id")).and_then(|l| sexpr::txt(l, 1)).unwrap_or("");
            let (count, names) = units.get(lib).cloned().unwrap_or((1, BTreeMap::new()));
            let Some(instances) = sexpr::find(sym, "instances") else { continue };
            for project in sexpr::find_all(instances, "project") {
                for path in sexpr::find_all(project, "path") {
                    let (Some(sheet_path), Some(reference)) = (sexpr::txt(path, 1), sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1))) else { continue };
                    let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0) as u32;
                    // `SubReference( unit )`: a display name after the separator, else the project's notation; only for a symbol of several units
                    let sub = match names.get(&unit) {
                        _ if count <= 1 => String::new(),
                        Some(display) => ids.separator_text(true) + display,
                        None => ids.sub_reference(unit, true),
                    };
                    let shown = format!("{reference}{sub}");
                    by_path.entry(format!("{}/{uuid}", sheet_path.trim_end_matches('/'))).or_insert_with(|| shown.clone());
                    by_uuid.entry(uuid.to_string()).or_insert(shown);
                }
            }
        }
    }
    for (_, text) in files.iter_mut() {
        if text.contains("${/") {
            *text = rewrite_tokens(text, &|token| {
                let (path, remainder) = token.split_once(':')?;
                if !path.starts_with('/') {
                    return None;
                }
                let found = by_path.get(path).or_else(|| path.rsplit('/').next().and_then(|u| by_uuid.get(u)))?;
                Some(format!("${{{found}:{remainder}}}"))
            });
        }
    }
}

/// Each `${...}` of `text` outside an escaped `\${...}` that `convert` has another form for, replaced by it.
fn rewrite_tokens(text: &str, convert: &dyn Fn(&str) -> Option<String>) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut copied = 0;
    while i + 1 < b.len() {
        if b[i] == b'$' && b[i + 1] == b'{' {
            let escaped = i >= 2 && b[i - 1] == b'\\' && b[i - 2] == b'\\';
            let Some(end) = token_end(b, i + 2) else { break };
            if !escaped {
                if let Some(replacement) = convert(&text[i + 2..end]) {
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

/// The other way, on what the importer read (`SCHEMATIC::ConvertKIIDsToRefs`, which KiCad's dialogs run when they show a text): a cross-reference the file
/// stores by a symbol's path names the symbol by its reference, which is how the studio keeps it, resolves it and edits it. `item_ids` is the importer's file
/// uuid -> item id (a symbol's id is its reference); every text of every screen a KiCad dialog stores by path
/// (`eda_model::cross_refs::for_each_cross_reference_text`: texts, text boxes, table cells, labels and the fields of labels, sheets and symbols) is read so. A path whose last uuid is no symbol stays
/// as it is (a symbol of a sheet not read yet: the tree reader runs this again once every sheet is in).
pub(crate) fn kiids_to_refs_in(design: &mut eda_model::ir::Design, item_ids: &BTreeMap<String, String>) {
    use eda_model::cross_refs::kiids_to_refs;
    let screens = || design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    let symbols: std::collections::BTreeSet<String> = screens().flat_map(|s| s.symbols.iter().map(|x| x.id.clone())).collect();
    if symbols.is_empty() {
        return;
    }
    let of = |u: &str| item_ids.get(u).filter(|id| symbols.contains(*id)).cloned();
    let fix_screen = |sch: &mut eda_model::ir::SchematicSection| {
        eda_model::cross_refs::for_each_cross_reference_text(sch, |text| {
            if let Some(n) = kiids_to_refs(text, &of) {
                *text = n;
            }
        })
    };
    if let Some(s) = design.schematic.as_mut() {
        fix_screen(s);
    }
    if let Some(c) = design.sheet_contents.as_mut() {
        for s in c.values_mut() {
            fix_screen(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LIB_SYMBOL::LetterSubReference( aUnit, 'A' )`: 1 is A, 26 is Z, 27 is AA.
    fn unit_letter(unit: u32) -> String {
        eda_model::annotate::unit_letters(unit)
    }

    /// KiCad's default notation (letters from `A`, no separator) for every symbol, whatever its unit count.
    fn letter_suffix(_lib_id: &str, unit: u32) -> Option<String> {
        Some(unit_letter(unit))
    }

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
        refs_to_kiids(&mut files, &letter_suffix);
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
        refs_to_kiids(&mut files, &letter_suffix);
        let text = &files[0].1;
        assert!(text.contains("${/aaaaaaaa-0000-4000-8000-000000000000/44444444-0000-4000-8000-000000000004:VALUE} ${/aaaaaaaa-0000-4000-8000-000000000000/33333333-0000-4000-8000-000000000003:VALUE}"), "{text}");
    }

    /// r7 (GAPS item 1): in a project whose unit notation is `.1`, `GetRef( .., true )` is `U1.2`, so `${U1.2:VALUE}` is the form that finds unit 2 and
    /// the letter form names no symbol; a symbol of one unit takes no suffix.
    #[test]
    fn a_unit_in_the_projects_notation_finds_the_symbol() {
        let mut files = vec![(
            "root.kicad_sch".to_string(),
            sheet(&format!(
                "{} {} {}",
                symbol("33333333-0000-4000-8000-000000000003", "U1", 1, "LM358").replace("Device:R", "Amp:Dual"),
                symbol("44444444-0000-4000-8000-000000000004", "U1", 2, "LM358").replace("Device:R", "Amp:Dual"),
                symbol("55555555-0000-4000-8000-000000000005", "R3", 1, "${U1.2:VALUE} ${U1B:VALUE} ${U1:VALUE} ${R3:VALUE} ${R3.1:VALUE}")
            )),
        )];
        let dotted = |lib_id: &str, unit: u32| (lib_id != "Device:R").then(|| format!(".{unit}"));
        refs_to_kiids(&mut files, &dotted);
        let text = &files[0].1;
        let root = "/aaaaaaaa-0000-4000-8000-000000000000";
        assert!(
            text.contains(&format!("${{{root}/44444444-0000-4000-8000-000000000004:VALUE}} <Unresolved: U1B> ${{{root}/33333333-0000-4000-8000-000000000003:VALUE}} ${{{root}/55555555-0000-4000-8000-000000000005:VALUE}} <Unresolved: R3.1>")),
            "{text}"
        );
    }

    /// `ConvertKIIDsToRefs`: a path read from a file is the reference `GetRef( &aPath, true )` gives -- the unit in the project's notation for a symbol of
    /// several units (the file's library symbol says how many), none for one unit; a path no symbol has keeps its form, and so does an escaped token.
    #[test]
    fn a_path_read_from_a_file_becomes_the_reference_it_names() {
        let lib = r#"(lib_symbols (symbol "Amp:Dual" (symbol "Dual_1_1") (symbol "Dual_2_1")) (symbol "Device:R" (symbol "R_0_1") (symbol "R_1_1")))"#;
        let root = "/aaaaaaaa-0000-4000-8000-000000000000";
        let text = format!(
            "(kicad_sch (version 20250114) (uuid \"aaaaaaaa-0000-4000-8000-000000000000\") {lib} {} {} {})",
            symbol("33333333-0000-4000-8000-000000000003", "U1", 1, "LM358").replace("Device:R", "Amp:Dual"),
            symbol("44444444-0000-4000-8000-000000000004", "U1", 2, "LM358").replace("Device:R", "Amp:Dual"),
            symbol("55555555-0000-4000-8000-000000000005", "R3", 1, &format!("${{{root}/44444444-0000-4000-8000-000000000004:VALUE}} ${{{root}/55555555-0000-4000-8000-000000000005:VALUE}} ${{/x/99999999-0000-4000-8000-000000000009:VALUE}} \\\\${{{root}/55555555-0000-4000-8000-000000000005:VALUE}}"))
        );
        let read = |ids: eda_model::annotate::SubpartIds| {
            let mut files = vec![("root.kicad_sch".to_string(), text.clone())];
            kiids_to_refs(&mut files, &ids);
            files.pop().unwrap().1
        };
        let letters = read(eda_model::annotate::SubpartIds::default());
        assert!(letters.contains(&format!("\"${{U1B:VALUE}} ${{R3:VALUE}} ${{/x/99999999-0000-4000-8000-000000000009:VALUE}} \\\\${{{root}/55555555")), "{letters}");
        let dotted = read(eda_model::annotate::SubpartIds { separator: '.' as u32, first_id: '1' as u32 });
        assert!(dotted.contains("\"${U1.2:VALUE} ${R3:VALUE}"), "{dotted}");
        // and written back, the reference finds the same symbol
        let mut files = vec![("root.kicad_sch".to_string(), dotted)];
        refs_to_kiids(&mut files, &|lib: &str, unit: u32| (lib == "Amp:Dual").then(|| format!(".{unit}")));
        assert!(files[0].1.contains(&format!("${{{root}/44444444-0000-4000-8000-000000000004:VALUE}}")), "{}", files[0].1);
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
        refs_to_kiids(&mut files, &letter_suffix);
        let text = &files[0].1;
        // unknown -> KiCad's text; a plain variable, an escaped token and a path stay; a power symbol is not looked up
        assert!(text.contains(r#""<Unresolved: R9> ${REV} \\${R1:VALUE} ${/x/y:VALUE} <Unresolved: #PWR01>""#), "{text}");
    }

    #[test]
    fn a_symbol_on_a_sheet_is_found_from_the_root() {
        let child = "(kicad_sch (version 20250114) (uuid \"bbbbbbbb-0000-4000-8000-000000000000\") (symbol (lib_id \"Device:C\") (at 0 0 0) (unit 1) (uuid \"66666666-0000-4000-8000-000000000006\") (property \"Reference\" \"C7\")
            (instances (project \"p\" (path \"/aaaaaaaa-0000-4000-8000-000000000000/cccccccc-0000-4000-8000-000000000000\" (reference \"C7\") (unit 1))))))";
        let mut files = vec![("root.kicad_sch".to_string(), sheet("(text \"C = ${C7:VALUE}\")")), ("child.kicad_sch".to_string(), child.to_string())];
        refs_to_kiids(&mut files, &letter_suffix);
        assert!(files[0].1.contains("${/aaaaaaaa-0000-4000-8000-000000000000/cccccccc-0000-4000-8000-000000000000/66666666-0000-4000-8000-000000000006:VALUE}"), "{}", files[0].1);
    }

    #[test]
    fn unit_letters_count_as_kicad_counts_them() {
        assert_eq!((unit_letter(1), unit_letter(2), unit_letter(26), unit_letter(27), unit_letter(28)), ("A".into(), "B".into(), "Z".into(), "AA".into(), "AB".into()));
    }
}
