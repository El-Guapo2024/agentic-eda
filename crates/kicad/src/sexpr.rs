//! Minimal s-expression reader for KiCad's text formats (`.kicad_pcb`,
//! `.kicad_sch`): parens, bare atoms, and double-quoted strings. No crate
//! dependency -- the grammar is small (see KiCad's own `DSNLEXER`) and we
//! only need to walk the tree looking for known fields, not validate the
//! full grammar the way KiCad's own recursive-descent parser does. Any
//! sub-list whose tag we do not recognize is simply invisible to a `find`
//! call, which is what makes this tolerant of newer fields we have not
//! ported yet (padstacks, teardrops, embedded fonts, ...) without needing
//! to enumerate them.
//!
//! `import.rs` is the semantic layer that turns this generic tree into our
//! model; this module only knows syntax.

/// One node of the tree. A bare atom and a quoted string both become
/// [`Sexpr::Atom`] -- callers never need to know which one the file used,
/// only the text ([`Sexpr::text`]). The one thing the tree keeps of the quotes is
/// [`QUOTED`], on the quoted strings, for [`to_text`] to quote them again.
#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    List(Vec<Sexpr>),
    Atom(String),
}

/// The first character of the text of an atom the file quoted. KiCad's lexer takes `1` and `"1"` for two different tokens (`DSN_NUMBER`, `DSN_STRING`), and `yes` and `"yes"`
/// for a keyword and a string, and the places that want one refuse the other: the pad names of a `(jumper_pad_groups ("1" "2"))` are strings and a number there is an error,
/// `(autoplace_cost180 10)` is a keyword and a string there is an error. A token kept as text ([`to_text`]) comes back as the file had it: quoted if it was, bare if it was.
/// [`Sexpr::text`] and everything built on it leave the mark out; only code that matches `Sexpr::Atom(a)` itself sees it (and `a == "hide"` is then false for a text that says
/// `"hide"`, which is right).
const QUOTED: char = '\u{1}';

impl Sexpr {
    pub fn as_list(&self) -> Option<&[Sexpr]> {
        match self {
            Sexpr::List(v) => Some(v),
            Sexpr::Atom(_) => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Sexpr::Atom(s) => Some(s.strip_prefix(QUOTED).unwrap_or(s)),
            Sexpr::List(_) => None,
        }
    }
}

/// Parse the whole document: exactly one top-level list, e.g.
/// `(kicad_pcb ...)`. Returns the offset of the failure (byte index) in the
/// error string so a caller can turn it into a line number if it wants one.
pub fn parse(input: &str) -> Result<Sexpr, String> {
    let bytes = input.as_bytes();
    let mut pos = 0usize;
    skip_ws(bytes, &mut pos);
    if pos >= bytes.len() || bytes[pos] != b'(' {
        return Err(format!("expected '(' at byte {pos}"));
    }
    pos += 1;
    let items = parse_items(input, bytes, &mut pos)?;
    skip_ws(bytes, &mut pos);
    Ok(Sexpr::List(items))
}

fn line_of(input: &str, pos: usize) -> usize {
    1 + input.as_bytes()[..pos.min(input.len())].iter().filter(|&&b| b == b'\n').count()
}

fn parse_items(input: &str, bytes: &[u8], pos: &mut usize) -> Result<Vec<Sexpr>, String> {
    let mut items = Vec::new();
    loop {
        skip_ws(bytes, pos);
        if *pos >= bytes.len() {
            return Err(format!("unexpected end of file (line {}); unclosed '('", line_of(input, *pos)));
        }
        match bytes[*pos] {
            b')' => {
                *pos += 1;
                return Ok(items);
            }
            b'(' => {
                *pos += 1;
                let sub = parse_items(input, bytes, pos)?;
                items.push(Sexpr::List(sub));
            }
            b'"' => {
                *pos += 1;
                let text = parse_quoted(input, bytes, pos)?;
                items.push(Sexpr::Atom(format!("{QUOTED}{text}")));
            }
            _ => items.push(Sexpr::Atom(parse_atom(input, bytes, pos))),
        }
    }
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\r' | b'\n') {
        *pos += 1;
    }
}

/// A bare token runs until whitespace or a delimiter. KiCad's own lexer
/// treats `(`, `)` and `"` as the only structural delimiters outside a
/// quoted string, so an unquoted atom may itself contain things like `.`,
/// `:`, `-`, `|` or `~` (layer names, lib ids, net names) -- we just copy
/// whatever is there.
fn parse_atom(input: &str, bytes: &[u8], pos: &mut usize) -> String {
    let start = *pos;
    while *pos < bytes.len() && !matches!(bytes[*pos], b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"') {
        *pos += 1;
    }
    input[start..*pos].to_string()
}

/// Contents of a `"..."` string, with the escapes `DSNLEXER::NextTok` reads (`common/dsnlexer.cpp`, the delimited-string branch): `\"` and `\\`, the
/// control characters `\a \b \f \n \r \t \v`, a hex byte `\xH` or `\xHH` and an octal byte `\O`, `\OO` or `\OOO`. A backslash that starts none of them is a
/// backslash (KiCad's "goofed" escape: the character after it is read as itself).
///
/// The bytes an escape stands for are collected as bytes: a `\xE2\x82\xAC` is the three bytes of a euro sign, as it is in KiCad's lexer. Every delimiter we scan for
/// (`"`, `\`) is ASCII, and ASCII bytes never occur as part of a multi-byte UTF-8 sequence, so copying the other bytes through keeps every character whole.
fn parse_quoted(input: &str, bytes: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut out: Vec<u8> = Vec::new();
    loop {
        if *pos >= bytes.len() {
            return Err(format!("unterminated string starting near line {}", line_of(input, *pos)));
        }
        match bytes[*pos] {
            b'"' => {
                *pos += 1;
                return Ok(String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()));
            }
            b'\\' if *pos + 1 < bytes.len() => {
                let next = bytes[*pos + 1];
                *pos += 2;
                match next {
                    b'"' | b'\\' => out.push(next),
                    b'a' => out.push(0x07),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(0x0b),
                    // One or two hex digits; none at all reads the `x` itself.
                    b'x' => {
                        let digits = bytes[*pos..].iter().take(2).take_while(|b| b.is_ascii_hexdigit()).count();
                        if digits == 0 {
                            out.push(b'x');
                        } else {
                            out.push(u8::from_str_radix(&input[*pos..*pos + digits], 16).expect("hex digits"));
                            *pos += digits;
                        }
                    }
                    // One to three octal digits (the one already read is the first); none at all is a backslash, and the character is read again as itself.
                    b'0'..=b'7' => {
                        let digits = 1 + bytes[*pos..].iter().take(2).take_while(|b| (b'0'..=b'7').contains(b)).count();
                        out.push(u32::from_str_radix(&input[*pos - 1..*pos - 1 + digits], 8).expect("octal digits") as u8);
                        *pos += digits - 1;
                    }
                    _ => {
                        out.push(b'\\');
                        *pos -= 1;
                    }
                }
            }
            b => {
                out.push(b);
                *pos += 1;
            }
        }
    }
}

/// An atom as the file had it: quoted if the file quoted it ([`QUOTED`]), bare if it was bare (a number, a keyword, an old file's layer name). An atom the parser did not make
/// and that cannot be bare (empty, or with a space, a parenthesis or a quote in it) is quoted.
pub(crate) fn atom_text(a: &str) -> String {
    match a.strip_prefix(QUOTED) {
        Some(quoted) => crate::sexpr_str(quoted),
        None if a.is_empty() || a.chars().any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '"')) => crate::sexpr_str(a),
        None => a.to_string(),
    }
}

/// A node back to text, on one line: the tokens an importer keeps as the file had them (`ViaExtra::tokens`, `PadMaskInfo::tokens`).
pub(crate) fn to_text(n: &Sexpr) -> String {
    match n {
        Sexpr::Atom(a) => atom_text(a),
        Sexpr::List(items) => format!("({})", items.iter().map(to_text).collect::<Vec<_>>().join(" ")),
    }
}

/// The first element's text, if this list starts with an atom (its "tag").
pub fn tag(list: &[Sexpr]) -> Option<&str> {
    list.first().and_then(Sexpr::text)
}

/// First child of `list` that is itself a `(tag ...)` list, searched among
/// `list`'s own children (not `list` itself).
pub fn find<'a>(list: &'a [Sexpr], want: &str) -> Option<&'a [Sexpr]> {
    list.iter().find_map(|it| it.as_list().filter(|l| tag(l) == Some(want)))
}

/// Every child of `list` that is a `(tag ...)` list, in file order.
pub fn find_all<'a>(list: &'a [Sexpr], want: &'a str) -> impl Iterator<Item = &'a [Sexpr]> {
    list.iter().filter_map(move |it| it.as_list().filter(|l| tag(l) == Some(want)))
}

/// Positional atom text at index `idx` of a list (index 0 is the tag).
pub fn txt(list: &[Sexpr], idx: usize) -> Option<&str> {
    list.get(idx).and_then(Sexpr::text)
}

/// Positional atom, parsed as a number (KiCad writes plain decimals).
pub fn num(list: &[Sexpr], idx: usize) -> Option<f64> {
    txt(list, idx).and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lists_and_strings() {
        let s = r#"(kicad_pcb (version 1) (layers (0 "F.Cu" signal) (31 "B.Cu" signal)))"#;
        let tree = parse(s).unwrap();
        let root = tree.as_list().unwrap();
        assert_eq!(tag(root), Some("kicad_pcb"));
        let layers = find(root, "layers").unwrap();
        assert_eq!(layers.len(), 3); // tag + 2 layer entries
        let l0 = layers[1].as_list().unwrap();
        assert_eq!(txt(l0, 1), Some("F.Cu"));
        assert_eq!(txt(l0, 2), Some("signal"));
    }

    #[test]
    fn unescapes_quoted_strings() {
        let s = r#"(net 1 "Net-(U1-Pad1)") (path "a\"b\\c")"#;
        let tree = parse(&format!("({s})")).unwrap();
        let root = tree.as_list().unwrap();
        let net = find(root, "net").unwrap();
        assert_eq!(txt(net, 2), Some("Net-(U1-Pad1)"));
        let path = find(root, "path").unwrap();
        assert_eq!(txt(path, 1), Some("a\"b\\c"));
    }

    /// `DSNLEXER` reads `\n` as a newline and `\t`, `\r`, `\xHH` and `\OOO` as the characters they name; a backslash before anything else is a backslash.
    #[test]
    fn reads_the_escapes_kicads_lexer_reads() {
        let one = |text: &str| -> String {
            let tree = parse(&format!("(t \"{text}\")")).unwrap();
            tree.as_list().unwrap()[1].text().unwrap().to_string()
        };
        assert_eq!(one(r"Actionneur\nPiezo New Amp\nV02"), "Actionneur\nPiezo New Amp\nV02");
        assert_eq!(one(r"a\tb\rc\\d\\n"), "a\tb\rc\\d\\n");
        assert_eq!(one(r"\x41\x4a\x4B\101\60\7"), "AJKA0\u{7}");
        // A euro sign written as the three bytes of its UTF-8, and a goofed escape (`\q`, `\x` with no digit, `\8`).
        assert_eq!(one(r"\xE2\x82\xAC"), "\u{20ac}");
        assert_eq!(one(r"\q\xg\8"), "\\qxg\\8");
        assert_eq!(one("caf\u{e9} \u{20ac}"), "caf\u{e9} \u{20ac}");
    }

    /// A token comes back as the file had it: a string the file quoted stays a string even when it reads like a number or a keyword bare (the pad names of a
    /// `(jumper_pad_groups ("1" "2"))` are strings to KiCad's parser and a number there is an error; the text `"123"` of a text box is one too), and a keyword or a number the file
    /// wrote bare stays bare (`(autoplace_cost180 10)` with a quoted tag is an error), whatever characters it has.
    #[test]
    fn a_token_is_written_back_quoted_where_the_file_quoted_it_and_bare_where_it_did_not() {
        let tree = parse(r#"(t (jumper_pad_groups ("1" "2" "a" "A1")) (x yes 3 -0.5 front_inner_back "yes" "3.5" "F.Cu" "" F.Cu a1) (gr_text_box "123" (justify left top)) (autoplace_cost180 10))"#).unwrap();
        let root = tree.as_list().unwrap();
        let one_line = |tag_name: &str| to_text(&Sexpr::List(find(root, tag_name).unwrap().to_vec()));
        assert_eq!(one_line("jumper_pad_groups"), r#"(jumper_pad_groups ("1" "2" "a" "A1"))"#);
        assert_eq!(one_line("x"), r#"(x yes 3 -0.5 front_inner_back "yes" "3.5" "F.Cu" "" F.Cu a1)"#);
        assert_eq!(one_line("gr_text_box"), r#"(gr_text_box "123" (justify left top))"#);
        assert_eq!(one_line("autoplace_cost180"), "(autoplace_cost180 10)");
        // What reads the tree sees the text, not the mark, and finds a quoted tag as it finds a bare one.
        let groups = find(root, "jumper_pad_groups").unwrap();
        assert_eq!(txt(groups[1].as_list().unwrap(), 0), Some("1"));
        assert_eq!(num(groups[1].as_list().unwrap(), 1), Some(2.0));
        assert_eq!(txt(find(root, "x").unwrap(), 5), Some("yes"));
        assert!(find(root, "x").unwrap().iter().any(|c| c.text() == Some("yes")));
        let quoted_tag = parse(r#"(t ("pad" 1))"#).unwrap();
        assert_eq!(find(quoted_tag.as_list().unwrap(), "pad").and_then(|p| num(p, 1)), Some(1.0));
        // An atom the parser did not make is bare unless it cannot be.
        assert_eq!(atom_text("0.5"), "0.5");
        assert_eq!(atom_text("a b"), "\"a b\"");
        assert_eq!(atom_text(""), "\"\"");
    }

    #[test]
    fn find_all_returns_every_match_in_order() {
        let tree = parse("(x (pad 1) (pad 2) (other) (pad 3))").unwrap();
        let root = tree.as_list().unwrap();
        let pads: Vec<&str> = find_all(root, "pad").map(|p| txt(p, 1).unwrap()).collect();
        assert_eq!(pads, vec!["1", "2", "3"]);
    }

    #[test]
    fn numbers_parse_including_negative_and_decimal() {
        let tree = parse("(at -1.5 20.25 90)").unwrap();
        let root = tree.as_list().unwrap();
        assert_eq!(num(root, 1), Some(-1.5));
        assert_eq!(num(root, 2), Some(20.25));
        assert_eq!(num(root, 3), Some(90.0));
    }

    #[test]
    fn unclosed_paren_is_an_error_not_a_panic() {
        assert!(parse("(kicad_pcb (layers").is_err());
    }
}
