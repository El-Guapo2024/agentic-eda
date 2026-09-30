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
/// only the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    List(Vec<Sexpr>),
    Atom(String),
}

impl Sexpr {
    pub fn as_list(&self) -> Option<&[Sexpr]> {
        match self {
            Sexpr::List(v) => Some(v),
            Sexpr::Atom(_) => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Sexpr::Atom(s) => Some(s),
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
                items.push(Sexpr::Atom(parse_quoted(input, bytes, pos)?));
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

/// Contents of a `"..."` string, unescaping `\\` and `\"` (the only two
/// escapes our own writer ever produces -- see `sexpr_str` in `lib.rs`).
/// Every delimiter byte we scan for (`"`, `\`) is ASCII, and ASCII bytes
/// never occur as part of a multi-byte UTF-8 sequence, so slicing on these
/// byte offsets always lands on a `char` boundary.
fn parse_quoted(input: &str, bytes: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut out = String::new();
    loop {
        if *pos >= bytes.len() {
            return Err(format!("unterminated string starting near line {}", line_of(input, *pos)));
        }
        match bytes[*pos] {
            b'"' => {
                *pos += 1;
                return Ok(out);
            }
            b'\\' if *pos + 1 < bytes.len() => {
                let next = bytes[*pos + 1];
                match next {
                    b'"' | b'\\' => out.push(next as char),
                    // Unknown escape: keep it literal rather than guessing.
                    _ => {
                        out.push('\\');
                        out.push(next as char);
                    }
                }
                *pos += 2;
            }
            _ => {
                // Copy one UTF-8 character's worth of bytes at a time so we
                // never split a multi-byte sequence.
                let rest = &input[*pos..];
                let ch = rest.chars().next().expect("pos < bytes.len()");
                out.push(ch);
                *pos += ch.len_utf8();
            }
        }
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
