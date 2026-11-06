//! A symbol cross-reference (`${R1:VALUE}`) that follows the symbol when its reference changes.
//!
//! KiCad stores a cross-reference by the symbol's place in the hierarchy, not by its designator: the dialogs that take a text (`DIALOG_TEXT_PROPERTIES`,
//! `DIALOG_TABLE_PROPERTIES`, `DIALOG_TABLECELL_PROPERTIES`, ...) turn `${R1:VALUE}` into `${/<sheet uuid path>/<symbol uuid>:VALUE}` when they write the text
//! (`SCHEMATIC::ConvertRefsToKIIDs`) and back when they show it (`ConvertKIIDsToRefs`), so a symbol annotated again from R1 to R5 is still the one the text names,
//! and the text shows and edits as `${R5:VALUE}`. This studio names a symbol by its reference (the reference is its id, and the uuid the writer gives it comes from
//! it), so the stored text keeps the designator, the writer turns it into KiCad's form (`eda_kicad::sch_crossref::refs_to_kiids`), and a rename rewrites the
//! designators the texts name ([`rename_cross_references`]): the text then shows, edits and is written as KiCad's would be.
//!
//! Ported from `eeschema/schematic.cpp`: the scan of `ConvertRefsToKIIDs` (an escaped `\${..}` or `\@{..}` copied as it is; the end of a `${..}` found by counting the
//! braces of `_{`, `^{` and `~{` markup inside it; a token with a `:` is a cross-reference, the designator before its first `:`).

use std::collections::BTreeMap;

/// The designator `renames` gives `reference`: its own entry, or that of the symbol it is a unit of (`U1A` of a renamed `U1`: `GetRef( path, true )` names a unit of a
/// symbol of several by the reference and the unit's letters) when `reference` has no entry of its own.
fn renamed(reference: &str, renames: &BTreeMap<String, String>) -> Option<String> {
    if let Some(n) = renames.get(reference) {
        return Some(n.clone());
    }
    let stem = reference.trim_end_matches(|c: char| c.is_ascii_uppercase());
    if stem.len() == reference.len() || stem.is_empty() || !stem.ends_with(|c: char| c.is_ascii_digit() || c == '?') {
        return None;
    }
    renames.get(stem).map(|n| format!("{n}{}", &reference[stem.len()..]))
}

/// `text` with every cross-reference to a symbol of `renames` (old designator -> new) naming the new designator, as KiCad's stored path keeps naming the symbol; `None`
/// when nothing in it changes. All of `renames` apply at once, so two symbols that trade references trade their cross-references too.
pub fn rename_cross_references(text: &str, renames: &BTreeMap<String, String>) -> Option<String> {
    if renames.is_empty() || !text.contains("${") {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len() + 8);
    let mut changed = false;
    let mut i = 0;
    while i < n {
        // "Check for escaped expressions: \${ or \@{ -- These should be copied verbatim"
        if chars[i] == '\\' && i + 2 < n && chars[i + 2] == '{' && (chars[i + 1] == '$' || chars[i + 1] == '@') {
            out.extend(&chars[i..i + 3]);
            let mut depth = 1;
            i += 3;
            while i < n && depth > 0 {
                match chars[i] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if chars[i] == '$' && i + 1 < n && chars[i + 1] == '{' {
            let mut token = String::new();
            let mut nesting = 0i32;
            let mut j = i + 2;
            let mut closed = false;
            while j < n {
                if chars[j] == '{' && matches!(chars[j - 1], '_' | '^' | '~') {
                    nesting += 1;
                }
                if chars[j] == '}' {
                    nesting -= 1;
                    if nesting < 0 {
                        closed = true;
                        break;
                    }
                }
                token.push(chars[j]);
                j += 1;
            }
            let replacement = token.split_once(':').and_then(|(reference, remainder)| renamed(reference, renames).map(|r| format!("{r}:{remainder}")));
            match replacement {
                Some(r) if r != token => {
                    changed = true;
                    out.push_str("${");
                    out.push_str(&r);
                }
                _ => {
                    out.push_str("${");
                    out.push_str(&token);
                }
            }
            if closed {
                out.push('}');
            }
            i = j + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    changed.then_some(out)
}

/// `SCHEMATIC::ConvertKIIDsToRefs`: every cross-reference that names a symbol by its path (`${/<sheet uuid>/.../<symbol uuid>:VALUE}`, or by its uuid alone) named by
/// the reference `reference_of` gives the path's last uuid (`ResolveItem( path.back() )`), as KiCad shows it in a dialog; a token whose item is not a symbol stays as
/// it is. `None` when nothing changes. (KiCad names a unit of a symbol of several by its letter too, `GetRef( &sheetPath, true )`; the reference alone names the
/// same symbol to the resolver here.) An escaped `\${..}` is copied as it is, and a token ends at its first `}`, as KiCad scans it.
pub fn kiids_to_refs(text: &str, reference_of: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if !text.contains("${") {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut i = 0;
    while i < n {
        if chars[i] == '\\' && i + 2 < n && chars[i + 2] == '{' && (chars[i + 1] == '$' || chars[i + 1] == '@') {
            out.extend(&chars[i..i + 3]);
            let mut depth = 1;
            i += 3;
            while i < n && depth > 0 {
                match chars[i] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if chars[i] == '$' && i + 1 < n && chars[i + 1] == '{' {
            let mut j = i + 2;
            while j < n && chars[j] != '}' {
                j += 1;
            }
            let token: String = chars[i + 2..j].iter().collect();
            let converted = token.split_once(':').and_then(|(path, remainder)| {
                let last = path.rsplit('/').find(|s| !s.is_empty())?;
                reference_of(last).map(|r| format!("{r}:{remainder}"))
            });
            out.push_str("${");
            match converted {
                Some(c) => {
                    changed = true;
                    out.push_str(&c);
                }
                None => out.push_str(&token),
            }
            if j < n {
                out.push('}');
            }
            i = j + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_cross_reference_follows_its_symbol_and_nothing_else_moves() {
        let m = map(&[("R1", "R5")]);
        assert_eq!(rename_cross_references("V = ${R1:VALUE} (${R10:VALUE}, ${TITLE})", &m).as_deref(), Some("V = ${R5:VALUE} (${R10:VALUE}, ${TITLE})"));
        assert_eq!(rename_cross_references("${R2:VALUE}", &m), None);
        // an escaped one is text, and a field of a field keeps the rest of the token
        assert_eq!(rename_cross_references("\\${R1:VALUE} ${R1:NET_NAME(2)}", &m).as_deref(), Some("\\${R1:VALUE} ${R5:NET_NAME(2)}"));
        // markup braces inside the token do not end it
        assert_eq!(rename_cross_references("${R1:~{VALUE}}x", &m).as_deref(), Some("${R5:~{VALUE}}x"));
    }

    #[test]
    fn a_unit_of_a_renamed_symbol_and_a_swap_follow_too() {
        assert_eq!(rename_cross_references("${U1B:VALUE}", &map(&[("U1", "U7")])).as_deref(), Some("${U7B:VALUE}"));
        assert_eq!(rename_cross_references("${R1:VALUE}/${R2:VALUE}", &map(&[("R1", "R2"), ("R2", "R1")])).as_deref(), Some("${R2:VALUE}/${R1:VALUE}"));
        // a reference that only looks like a unit of a renamed one, but ends in no digit, is not one
        assert_eq!(rename_cross_references("${CONNB:VALUE}", &map(&[("CONN", "J1")])), None);
    }

    #[test]
    fn a_symbols_path_reads_as_its_reference_and_anything_else_stays() {
        let of = |u: &str| (u == "bbbbbbbb-0000-4000-8000-000000000002").then(|| "R7".to_string());
        assert_eq!(kiids_to_refs("V=${/aaaaaaaa-0000-4000-8000-000000000001/bbbbbbbb-0000-4000-8000-000000000002:VALUE} ${TITLE}", &of).as_deref(), Some("V=${R7:VALUE} ${TITLE}"));
        assert_eq!(kiids_to_refs("${bbbbbbbb-0000-4000-8000-000000000002:Footprint}", &of).as_deref(), Some("${R7:Footprint}"));
        assert_eq!(kiids_to_refs("${/aaaa/cccc:VALUE} ${R1:VALUE} \\${/x/bbbbbbbb-0000-4000-8000-000000000002:V}", &of), None);
    }
}
