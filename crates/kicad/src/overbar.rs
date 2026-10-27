//! The overbar notation of a board or footprint file older than 20210606, read as KiCad reads it.
//!
//! Up to file version 20210605 an overbar was written `~RESET~` (a lone `~` toggles it, `~~` is a tilde); from 20210606 it is `~{RESET}`. KiCad's s-expression parser
//! converts the old notation on read wherever a string can carry one (8303b2ad `pcb_io_kicad_sexpr_parser.cpp`, each `if( m_requiredVersion < 20210606 )`):
//! - `parseNETINFO_ITEM`: the board's `(net <code> <name>)`;
//! - `parseNETCLASS`, `T_add_net`: a net class's `(add_net <name>)`;
//! - `parsePAD`, `T_net`: a pad's `(net <code> <name>)` (or `(net <name>)`);
//! - `parseNet`: an item's `(net <name>)` (the name form only; a net code is not converted);
//! - `parseEDA_TEXT`: the text of every item whose `(effects ..)` is read, at the moment it is: `gr_text`/`fp_text` (`parsePCB_TEXT_effects`), a footprint's
//!   `property` (a field), `gr_text_box`/`fp_text_box` and `table_cell` (`parseTextBoxContent`), a dimension's `gr_text`. A text with no `(effects ..)` is not converted.
//!
//! The version is the file's `(version ..)` (`parseHeader`; a board with none is 20201115, a footprint file with none 0), raised by a footprint's own `(version ..)` from
//! the place it is read on (`parseFOOTPRINT_unchecked`: `m_requiredVersion = std::max( m_requiredVersion, this_version )`, never lowered again), so the walk here goes in
//! file order. [`normalize`] rewrites the tree before the readers see it, so every reader of net names and texts in this crate has the new notation, as KiCad's model does;
//! the writer writes a newer version, which kicad-cli then reads unconverted, as KiCad's own save would have written it.

use crate::sexpr::{self, Sexpr};

/// The first file version whose overbars are written `~{..}`.
pub const NEW_OVERBAR_NOTATION: i64 = 20210606;

/// Convert the old overbar notation of every name and text of a board or footprint file older than [`NEW_OVERBAR_NOTATION`]; a newer tree is left as it is.
pub fn normalize(tree: &mut Sexpr) {
    let Sexpr::List(root) = tree else { return };
    let mut version = match root.iter().filter_map(Sexpr::as_list).find(|l| sexpr::tag(l) == Some("version")).and_then(|v| sexpr::num(v, 1)) {
        Some(v) => v as i64,
        // `parseHeader`: "Last version before we started writing version #s"; a footprint file's parser starts at 0.
        None if sexpr::tag(root) == Some("kicad_pcb") => 20201115,
        None => 0,
    };
    walk(root, &mut version);
}

fn walk(list: &mut [Sexpr], version: &mut i64) {
    let tag = sexpr::tag(list).map(str::to_string);
    let tag = tag.as_deref();
    // The version is never lowered: nothing from here on is converted.
    if *version >= NEW_OVERBAR_NOTATION {
        return;
    }
    match tag {
        Some("net") => {
            // `(net <code> <name>)` or `(net <name>)`: the name, never the code.
            let idx = if sexpr::num(list, 1).is_some() { 2 } else { 1 };
            convert_at(list, idx);
        }
        Some("add_net") => convert_at(list, 1),
        Some("gr_text") | Some("gr_text_box") | Some("fp_text_box") | Some("table_cell") if has_effects(list) => convert_at(list, 1),
        // `(fp_text <type> <text> ..)` and a field's `(property <name> <value> ..)`.
        Some("fp_text") | Some("property") if has_effects(list) => convert_at(list, 2),
        _ => {}
    }
    let footprint = matches!(tag, Some("footprint") | Some("module"));
    for child in list.iter_mut().skip(1) {
        let Sexpr::List(l) = child else { continue };
        if footprint && sexpr::tag(l) == Some("version") {
            if let Some(v) = sexpr::num(l, 1) {
                *version = (*version).max(v as i64);
            }
            continue;
        }
        walk(l, version);
    }
}

fn has_effects(list: &[Sexpr]) -> bool {
    list.iter().filter_map(Sexpr::as_list).any(|l| sexpr::tag(l) == Some("effects"))
}

fn convert_at(list: &mut [Sexpr], idx: usize) {
    let Some(old) = sexpr::txt(list, idx) else { return };
    let new = convert_to_new_overbar_notation(old);
    if new != old {
        sexpr::set_text(&mut list[idx], &new);
    }
}

/// `~X~` to `~{X}`, as 8303b2ad `common/string_utils.cpp::ConvertToNewOverbarNotation`: a lone `~` (the legacy empty string) is kept; `~~` is a tilde; `~~{` is written
/// `~~{}` so its brace does not open an overbar; a `~{` (a string already converted) returns the string unchanged; a space, `}` or `)` ends an overbar, and so does the
/// end of the string.
pub fn convert_to_new_overbar_notation(old: &str) -> String {
    if old == "~" {
        return old.to_string();
    }
    let chars: Vec<char> = old.chars().collect();
    let mut out = String::with_capacity(old.len());
    let mut in_overbar = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '~' {
            let next = chars.get(i + 1).copied();
            if next == Some('~') {
                if chars.get(i + 2) == Some(&'{') {
                    // "This way the subsequent opening curly brace will not start an overbar." The loop goes on at the second tilde, as KiCad's does.
                    out.push_str("~~{}");
                    i += 1;
                    continue;
                }
                // "Two subsequent tildes mean a tilde."
                out.push('~');
                i += 2;
                continue;
            } else if next == Some('{') {
                // "more likely this is a case of double notation conversion.  Bail out."
                return old.to_string();
            } else {
                out.push_str(if in_overbar { "}" } else { "~{" });
                in_overbar = !in_overbar;
                i += 1;
                continue;
            }
        } else if matches!(ch, ' ' | '}' | ')') && in_overbar {
            // "Spaces were used to terminate overbar as well"
            out.push('}');
            in_overbar = false;
        }
        out.push(ch);
        i += 1;
    }
    if in_overbar {
        out.push('}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_notation_converts_as_kicads_converter() {
        assert_eq!(convert_to_new_overbar_notation("~RESET~"), "~{RESET}");
        assert_eq!(convert_to_new_overbar_notation("a~b~c"), "a~{b}c");
        assert_eq!(convert_to_new_overbar_notation("~~"), "~");
        assert_eq!(convert_to_new_overbar_notation("~"), "~");
        assert_eq!(convert_to_new_overbar_notation("~{x}"), "~{x}");
        assert_eq!(convert_to_new_overbar_notation("~CS"), "~{CS}");
        assert_eq!(convert_to_new_overbar_notation("~WR RD"), "~{WR} RD");
        assert_eq!(convert_to_new_overbar_notation("A~~B"), "A~B");
        assert_eq!(convert_to_new_overbar_notation("x/~CS~/y"), "x/~{CS}/y");
        assert_eq!(convert_to_new_overbar_notation("plain"), "plain");
        // `~~{`: KiCad writes `~~{}` and goes on at the second tilde, which meets `~{` and bails out with the string as it was.
        assert_eq!(convert_to_new_overbar_notation("~~{a}"), "~~{a}");
    }

    fn texts(text: &str) -> Vec<String> {
        let mut tree = sexpr::parse(text).unwrap();
        normalize(&mut tree);
        let mut found = Vec::new();
        fn walk(n: &Sexpr, found: &mut Vec<String>) {
            if let Sexpr::List(l) = n {
                if let Some(t) = sexpr::tag(l) {
                    if matches!(t, "net" | "add_net" | "gr_text" | "fp_text" | "property") {
                        found.push(sexpr::to_text(n));
                    }
                }
                l.iter().for_each(|c| walk(c, found));
            }
        }
        walk(&tree, &mut found);
        found
    }

    #[test]
    fn an_old_files_names_and_texts_are_converted_and_a_newer_ones_are_not() {
        let board = |v: &str| {
            format!(
                "(kicad_pcb {v} (net 1 \"~RESET~\") (net_class Default \"\" (add_net \"~RESET~\")) (gr_text \"~CS~\" (at 0 0) (effects (font (size 1 1)))) (gr_text \"~NOFX~\" (at 0 0)) \
                 (footprint \"R\" (fp_text reference \"~R1~\" (at 0 0) (effects (font (size 1 1)))) (pad \"1\" smd rect (net 1 \"~RESET~\"))) (segment (net 1)))"
            )
        };
        let old = texts(&board("(version 20210228)"));
        assert_eq!(
            old,
            [
                "(net 1 \"~{RESET}\")",
                "(add_net \"~{RESET}\")",
                "(gr_text \"~{CS}\" (at 0 0) (effects (font (size 1 1))))",
                "(gr_text \"~NOFX~\" (at 0 0))",
                "(fp_text reference \"~{R1}\" (at 0 0) (effects (font (size 1 1))))",
                "(net 1 \"~{RESET}\")",
                "(net 1)"
            ]
        );
        // No `(version ..)`: `parseHeader`'s 20201115, converted.
        assert_eq!(texts(&board(""))[0], "(net 1 \"~{RESET}\")");
        let new = texts(&board("(version 20211014)"));
        assert_eq!(new[0], "(net 1 \"~RESET~\")");
        assert_eq!(new[2], "(gr_text \"~CS~\" (at 0 0) (effects (font (size 1 1))))");
    }

    /// A footprint's own `(version ..)` raises the version from where it is read (`std::max`), for the rest of the file.
    #[test]
    fn a_footprints_own_newer_version_stops_the_conversion_from_where_it_is_read() {
        let text = "(kicad_pcb (version 20210228) (footprint \"R\" (fp_text user \"~A~\" (effects)) (version 20211014) (fp_text user \"~B~\" (effects))) (gr_text \"~C~\" (effects)))";
        assert_eq!(texts(text), ["(fp_text user \"~{A}\" (effects))", "(fp_text user \"~B~\" (effects))", "(gr_text \"~C~\" (effects))"]);
        // A footprint file with no version is converted (the parser's 0).
        assert_eq!(texts("(footprint \"R\" (fp_text value \"~V~\" (effects)))"), ["(fp_text value \"~{V}\" (effects))"]);
    }
}
