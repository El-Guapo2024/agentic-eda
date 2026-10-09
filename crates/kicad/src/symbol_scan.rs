//! A quick look into a `.kicad_sym` file: what the Symbol Chooser's tree and search need to know about every symbol
//! (its name, description, keywords, default footprint, units, pins) without building the s-expression tree of the
//! whole file.
//!
//! KiCad.app ships 223 symbol libraries, 220 MB, the biggest 15 MB. `symbol_lib::parse_symbol_library` reads a whole file
//! into `LibSymbol`s, graphics and all, which is right for the handful of symbols a design uses and far too slow for
//! "search every library for LM358". One pass over the bytes ([`scan_symbols`]) does for the chooser: it tracks the
//! parenthesis depth, skips strings whole, and at the two depths that matter picks out
//!
//!   * depth 1: each top-level `(symbol "Name" ...)` -- its byte span, so the one symbol the person picks can be cut out of
//!     the text and parsed alone ([`parse_symbol`]);
//!   * depth 2, inside a symbol: its `(property "Key" "Value" ...)` forms (`LIB_SYMBOL`'s fields, the ones
//!     `LIB_SYMBOL::cacheSearchTerms` and `GetChooserFields` read: Reference, Value, Footprint, Datasheet, Description,
//!     `ki_keywords`, `ki_fp_filters`), `(extends "Base")`, `(power)`, and the unit sub-blocks `(symbol "Name_<unit>_<style>")`;
//!   * depth 3, inside a unit sub-block: its `(pin ...)` forms, each with the `(number "N")` it carries.
//!
//! Nothing below depth 3 is looked at except to keep the count of parentheses. The result is a few hundred bytes per symbol
//! instead of the symbol's whole drawing.
//!
//! A derived symbol (`(extends "Base")`: `AMS1117-3.3` of `AP1117-15`) has no drawing of its own: [`inherit`] gives it its
//! base's units, pins and whatever property it leaves empty, the way KiCad flattens a derived symbol.

use std::collections::HashMap;
use std::ops::Range;

use eda_model::ir::LibrarySymbol;

/// What the chooser knows about one symbol of a library file, found without parsing its drawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolSummary {
    /// The bare name, `(symbol "NAME" ...)` (the library nickname is the file's).
    pub name: String,
    /// Where `(symbol "NAME" ...)` starts and ends in the file's text, parentheses included.
    pub span: Range<usize>,
    /// `(extends "Base")`: the symbol this one is derived from.
    pub extends: Option<String>,
    pub reference: String,
    pub value: String,
    /// The default footprint, `Lib:Name` (empty for most symbols).
    pub footprint: String,
    pub datasheet: String,
    pub description: String,
    /// `ki_keywords`, space separated.
    pub keywords: String,
    /// `ki_fp_filters`, space separated wildcard patterns (`SOIC*3.9x4.9mm*`).
    pub fp_filters: String,
    /// `(power)`: a power symbol (`LIB_SYMBOL::IsPower`).
    pub power: bool,
    /// How many units it has: the highest `<unit>` of its `Name_<unit>_<style>` sub-blocks (0 when it draws nothing itself).
    pub units: u32,
    /// The pin numbers, unique, sorted, of every unit in the first body style (`GetGraphicalPins( 0, 1 )`, numbers deduplicated
    /// as the footprint chooser's netlist mail does): what the footprint chooser filters pad counts by.
    pub pin_numbers: Vec<String>,
    /// A second body style (DeMorgan) is drawn.
    pub alternate_body_style: bool,
}

impl SymbolSummary {
    /// How many units to offer: at least one.
    pub fn unit_count(&self) -> u32 {
        self.units.max(1)
    }
    pub fn pin_count(&self) -> usize {
        self.pin_numbers.len()
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    i
}

/// The end of the bare word that starts at `i`.
fn word_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && !is_ws(b[i]) && !matches!(b[i], b'(' | b')' | b'"') {
        i += 1;
    }
    i
}

/// The index just past the closing quote of the string whose opening quote is at `i`.
fn string_end(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() && b[i] != b'"' {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    (i + 1).min(b.len())
}

/// The text of the string whose opening quote is at `i`, with `\"` and `\\` unescaped (the two escapes `sexpr::parse` knows), and the
/// index just past it. `None` when `b[i]` is not a quote, or the string is cut off before its closing one.
pub(crate) fn read_string(text: &str, i: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let end = string_end(b, i);
    if end < i + 2 || b[end - 1] != b'"' {
        return None;
    }
    // Both ends are ASCII quotes, so the slice is on character boundaries.
    let inner = &text[i + 1..end - 1];
    let s = if inner.contains('\\') { inner.replace("\\\"", "\"").replace("\\\\", "\\") } else { inner.to_string() };
    Some((s, end))
}

/// `Name_<unit>_<style>` -> (unit, style); `None` for a name that does not end that way.
fn unit_and_style(sub_name: &str) -> Option<(u32, u32)> {
    let mut parts = sub_name.rsplitn(3, '_');
    let style = parts.next()?.parse().ok()?;
    let unit = parts.next()?.parse().ok()?;
    parts.next()?;
    Some((unit, style))
}

/// Every top-level symbol of `.kicad_sym` text, in file order. A single pass; see the module doc. Junk in, fewer symbols out: a file cut
/// off in the middle of a symbol loses that symbol, never reads past its end.
pub fn scan_symbols(text: &str) -> Vec<SymbolSummary> {
    let b = text.as_bytes();
    let n = b.len();
    let mut out: Vec<SymbolSummary> = Vec::new();
    let mut cur: Option<SymbolSummary> = None;
    let mut numbers: Vec<String> = Vec::new();
    let (mut i, mut depth) = (0usize, 0usize);
    // The body style of the unit sub-block being read, and whether a pin was opened at depth 3 (its `(number ..)` follows at depth 4).
    let (mut style, mut in_pin) = (0u32, false);
    while i < n {
        match b[i] {
            b'"' => i = string_end(b, i),
            b'(' => {
                let tag_end = word_end(b, i + 1);
                let tag = &b[i + 1..tag_end];
                let mut next = tag_end;
                if depth == 1 && tag == b"symbol" {
                    let at = skip_ws(b, tag_end);
                    if let Some((name, after)) = read_string(text, at) {
                        cur = Some(SymbolSummary { name, span: i..n, ..Default::default() });
                        numbers.clear();
                        next = after;
                    }
                } else if let Some(sym) = cur.as_mut() {
                    match (depth, tag) {
                        (2, b"property") => {
                            let key_at = skip_ws(b, tag_end);
                            if let Some((key, after_key)) = read_string(text, key_at) {
                                let val_at = skip_ws(b, after_key);
                                if let Some((value, after_val)) = read_string(text, val_at) {
                                    let slot = match key.as_str() {
                                        "Reference" => Some(&mut sym.reference),
                                        "Value" => Some(&mut sym.value),
                                        "Footprint" => Some(&mut sym.footprint),
                                        "Datasheet" => Some(&mut sym.datasheet),
                                        "Description" => Some(&mut sym.description),
                                        "ki_keywords" => Some(&mut sym.keywords),
                                        "ki_fp_filters" => Some(&mut sym.fp_filters),
                                        _ => None,
                                    };
                                    if let Some(slot) = slot {
                                        *slot = value;
                                    }
                                    next = after_val;
                                }
                            }
                        }
                        (2, b"extends") => {
                            if let Some((base, after)) = read_string(text, skip_ws(b, tag_end)) {
                                sym.extends = Some(base);
                                next = after;
                            }
                        }
                        (2, b"power") => sym.power = true,
                        (2, b"symbol") => {
                            if let Some((sub, after)) = read_string(text, skip_ws(b, tag_end)) {
                                // KiCad names the blocks `<symbol>_<unit>_<style>`; the style 0 / unit 0 blocks are shared by all.
                                if let Some((unit, st)) = unit_and_style(&sub) {
                                    sym.units = sym.units.max(unit);
                                    style = st;
                                    if st >= 2 {
                                        sym.alternate_body_style = true;
                                    }
                                } else {
                                    style = 0;
                                }
                                next = after;
                            }
                        }
                        (3, b"pin") => in_pin = style <= 1,
                        (4, b"number") if in_pin => {
                            if let Some((num, after)) = read_string(text, skip_ws(b, tag_end)) {
                                numbers.push(num);
                                in_pin = false;
                                next = after;
                            }
                        }
                        _ => {}
                    }
                }
                depth += 1;
                i = next;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    if let Some(mut sym) = cur.take() {
                        sym.span.end = i + 1;
                        numbers.sort_unstable();
                        numbers.dedup();
                        sym.pin_numbers = std::mem::take(&mut numbers);
                        out.push(sym);
                    }
                } else if depth <= 3 {
                    // a pin (depth 3) or a unit block (depth 2) closed: no `(number ..)` is pending
                    in_pin = false;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// A derived symbol takes from its base what it does not have: the units, the pins, and every property it leaves empty -- how KiCad flattens it
/// (`LIB_SYMBOL::Flatten`), and what the chooser shows (`AMS1117-3.3` has the pins of `AP1117-15`). One level at a time, to the end of the chain.
pub fn inherit(list: &mut [SymbolSummary]) {
    let index: HashMap<String, usize> = list.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
    for i in 0..list.len() {
        let mut base = list[i].extends.as_ref().and_then(|b| index.get(b)).copied();
        let mut hops = 0;
        while let Some(b) = base {
            if b == i || hops > 8 {
                break;
            }
            let from = list[b].clone();
            let sym = &mut list[i];
            if sym.pin_numbers.is_empty() && sym.units == 0 {
                sym.units = from.units;
                sym.pin_numbers = from.pin_numbers.clone();
                sym.alternate_body_style |= from.alternate_body_style;
            }
            for (mine, theirs) in [
                (&mut sym.reference, &from.reference),
                (&mut sym.footprint, &from.footprint),
                (&mut sym.datasheet, &from.datasheet),
                (&mut sym.description, &from.description),
                (&mut sym.keywords, &from.keywords),
                (&mut sym.fp_filters, &from.fp_filters),
            ] {
                if mine.is_empty() {
                    mine.clone_from(theirs);
                }
            }
            sym.power |= from.power;
            base = from.extends.as_ref().and_then(|e| index.get(e)).copied();
            hops += 1;
        }
    }
}

/// The text of one symbol's form (`(symbol "Name" ...)`), cut out of the file's text by its summary.
pub fn symbol_text<'a>(text: &'a str, s: &SymbolSummary) -> &'a str {
    text.get(s.span.clone()).unwrap_or("")
}

/// The editable definition of the symbol called `name`: only its own form (and the forms of the symbols it extends) are parsed, never the rest
/// of the file. Named by its bare name (`LibrarySymbol::lib_id`), which the caller turns into `Lib:Name`. `None` for a name the file lacks or a
/// form that does not parse.
pub fn parse_symbol(text: &str, summaries: &[SymbolSummary], name: &str) -> Option<LibrarySymbol> {
    let index: HashMap<&str, &SymbolSummary> = summaries.iter().map(|s| (s.name.as_str(), s)).collect();
    // The symbol and, before it, every base it extends (the reader looks sideways for them).
    let mut chain: Vec<&SymbolSummary> = Vec::new();
    let mut at = index.get(name).copied()?;
    chain.push(at);
    while let Some(base) = at.extends.as_ref().and_then(|b| index.get(b.as_str()).copied()) {
        if chain.iter().any(|c| c.name == base.name) || chain.len() > 8 {
            break;
        }
        chain.push(base);
        at = base;
    }
    let mut wrapped = String::from("(kicad_symbol_lib ");
    for s in chain.iter().rev() {
        wrapped.push_str(symbol_text(text, s));
        wrapped.push('\n');
    }
    wrapped.push(')');
    let parsed = crate::symbol_import::parse_library_symbols(&wrapped).ok()?;
    parsed.symbols.into_iter().find(|s| s.lib_id == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str = r##"(kicad_symbol_lib (version 20251024) (generator "kicad_symbol_editor")
        (symbol "LM358" (pin_names (offset 0.254)) (in_bom yes)
            (property "Reference" "U" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (property "Value" "LM358" (at 0 0 0))
            (property "Footprint" "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm" (at 0 0 0) (hide yes))
            (property "Datasheet" "http://www.ti.com/lit/ds/symlink/lm358.pdf")
            (property "Description" "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8/TO-99-8 (with a paren)")
            (property "ki_keywords" "dual opamp")
            (property "ki_fp_filters" "SOIC*3.9x4.9mm*P1.27mm* DIP*W7.62mm*")
            (symbol "LM358_1_1" (polyline (pts (xy -5.08 5.08) (xy 5.08 0) (xy -5.08 -5.08) (xy -5.08 5.08)))
                (pin input line (at -7.62 2.54 0) (length 2.54) (name "+" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
                (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-") (number "2"))
                (pin output line (at 7.62 0 180) (length 2.54) (name "~") (number "1")))
            (symbol "LM358_2_1" (pin input line (at -7.62 2.54 0) (length 2.54) (name "+") (number "5"))
                (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-") (number "6"))
                (pin output line (at 7.62 0 180) (length 2.54) (name "~") (number "7")))
            (symbol "LM358_3_1" (pin power_in line (at -2.54 7.62 270) (length 2.54) (name "V+") (number "8"))
                (pin power_in line (at -2.54 -7.62 90) (length 2.54) (name "V-") (number "4"))))
        (symbol "LM2904" (extends "LM358")
            (property "Value" "LM2904")
            (property "Description" ""))
        (symbol "GND" (power) (property "Reference" "#PWR") (property "Value" "GND")
            (symbol "GND_0_1" (polyline (pts (xy 0 0) (xy 0 -1.27))))
            (symbol "GND_1_1" (pin power_in line (at 0 0 270) (length 0) hide (name "GND") (number "1"))))
        (symbol "Demorgan" (property "Reference" "U") (property "Value" "Demorgan")
            (symbol "Demorgan_1_1" (pin input line (at 0 0 0) (length 1) (name "A") (number "1")))
            (symbol "Demorgan_1_2" (pin input line (at 0 0 0) (length 1) (name "A") (number "1")) (pin output line (at 1 0 0) (length 1) (name "Y") (number "2")))))"##;

    #[test]
    fn scan_picks_out_the_fields_the_chooser_searches() {
        let list = scan_symbols(LIB);
        let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["LM358", "LM2904", "GND", "Demorgan"]);
        let lm = &list[0];
        assert_eq!(lm.reference, "U");
        assert_eq!(lm.value, "LM358");
        assert_eq!(lm.footprint, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert_eq!(lm.datasheet, "http://www.ti.com/lit/ds/symlink/lm358.pdf");
        assert_eq!(lm.description, "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8/TO-99-8 (with a paren)", "a paren inside a string does not end anything");
        assert_eq!(lm.keywords, "dual opamp");
        assert_eq!(lm.fp_filters, "SOIC*3.9x4.9mm*P1.27mm* DIP*W7.62mm*");
        assert_eq!(lm.units, 3);
        assert_eq!(lm.pin_numbers, ["1", "2", "3", "4", "5", "6", "7", "8"], "the pins of every unit, once each");
        assert!(!lm.power && !lm.alternate_body_style && lm.extends.is_none());
        assert_eq!(&LIB[lm.span.clone()][..16], "(symbol \"LM358\" ");
        assert!(LIB[lm.span.clone()].ends_with("(number \"4\"))))"), "the span is the whole form, closing paren included");
    }

    #[test]
    fn a_derived_symbol_takes_its_bases_pins_and_what_it_leaves_empty() {
        let mut list = scan_symbols(LIB);
        let derived = &list[1];
        assert_eq!((derived.extends.as_deref(), derived.units, derived.pin_numbers.len()), (Some("LM358"), 0, 0), "it draws nothing itself");
        assert_eq!(derived.description, "");
        inherit(&mut list);
        let derived = &list[1];
        assert_eq!((derived.units, derived.pin_count()), (3, 8));
        assert_eq!(derived.value, "LM2904", "its own property wins");
        assert_eq!(derived.description, "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8/TO-99-8 (with a paren)", "an empty one is the base's");
        assert_eq!(derived.keywords, "dual opamp");
        assert_eq!(derived.footprint, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
    }

    #[test]
    fn power_symbols_and_alternate_body_styles_are_flagged() {
        let list = scan_symbols(LIB);
        let gnd = &list[2];
        assert!(gnd.power);
        assert_eq!((gnd.units, gnd.pin_count()), (1, 1));
        let dm = &list[3];
        assert!(dm.alternate_body_style, "a `_1_2` block is the second body style");
        assert_eq!(dm.pin_numbers, ["1"], "the pins counted are the first body style's: the second style's pin 2 is not a pin of the part");
        assert_eq!(dm.unit_count(), 1);
    }

    #[test]
    fn junk_and_cut_off_files_lose_symbols_without_reading_past_the_end() {
        assert!(scan_symbols("").is_empty());
        assert!(scan_symbols("(kicad_symbol_lib").is_empty());
        assert!(scan_symbols("(kicad_symbol_lib (symbol").is_empty());
        assert!(scan_symbols("(kicad_symbol_lib (symbol \"A\" (property \"Value\" \"unterminated").is_empty(), "a symbol cut off before its end is dropped");
        let list = scan_symbols("(kicad_symbol_lib (symbol \"A\" (property \"Value\" \"a\")) ))) (symbol \"B\")");
        assert_eq!(list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["A"], "stray closing parens do not open a second library");
        assert_eq!(list[0].value, "a");
        // an escaped quote inside a value
        let list = scan_symbols(r#"(kicad_symbol_lib (symbol "Q" (property "Description" "a \"quoted\" word and a \\ slash")))"#);
        assert_eq!(list[0].description, r#"a "quoted" word and a \ slash"#);
    }

    #[test]
    fn one_symbol_is_parsed_alone_with_the_forms_it_extends() {
        let summaries = scan_symbols(LIB);
        let lm = parse_symbol(LIB, &summaries, "LM358").unwrap();
        assert_eq!(lm.lib_id, "LM358");
        assert_eq!(lm.unit_count, 3);
        assert_eq!(lm.pins.len(), 8);
        assert_eq!(lm.footprint_filters, ["SOIC*3.9x4.9mm*P1.27mm*", "DIP*W7.62mm*"]);
        assert_eq!(lm.keywords, "dual opamp");
        // the derived one has the drawing of its base
        let lm2904 = parse_symbol(LIB, &summaries, "LM2904").unwrap();
        assert_eq!(lm2904.pins.len(), 8);
        // body styles stay in the editable definition
        let dm = parse_symbol(LIB, &summaries, "Demorgan").unwrap();
        assert!(dm.has_alternate_body_style);
        assert!(dm.pins.iter().any(|p| p.body_style == 2 && p.number == "2"));
        assert!(parse_symbol(LIB, &summaries, "Nope").is_none());
    }

    /// The real libraries, when KiCad.app is installed: every library scans, the big ones quickly, and the names agree with the
    /// names-only scan the library trees use.
    #[test]
    fn the_installed_libraries_scan() {
        let root = crate::default_symbol_library_root();
        let Ok(entries) = std::fs::read_dir(&root) else {
            eprintln!("no KiCad symbol libraries at {}; skipping", root.display());
            return;
        };
        let started = std::time::Instant::now();
        let (mut files, mut symbols, mut bytes) = (0usize, 0usize, 0usize);
        for e in entries.filter_map(|e| e.ok()) {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("kicad_sym") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let mut list = scan_symbols(&text);
            inherit(&mut list);
            assert!(!list.is_empty(), "{} lists no symbol", path.display());
            files += 1;
            symbols += list.len();
            bytes += text.len();
            for s in &list {
                assert!(!s.name.is_empty() && s.span.end > s.span.start);
                assert!(s.unit_count() >= 1);
            }
        }
        eprintln!("scanned {files} libraries, {symbols} symbols, {} MB in {:?}", bytes / 1_000_000, started.elapsed());
        assert!(files > 100 && symbols > 10_000);

        let text = std::fs::read_to_string(root.join("Amplifier_Operational.kicad_sym")).unwrap();
        let mut list = scan_symbols(&text);
        inherit(&mut list);
        let lm358 = list.iter().find(|s| s.name == "LM358").expect("LM358 ships with KiCad");
        assert_eq!((lm358.unit_count(), lm358.pin_count()), (3, 8));
        assert!(lm358.description.to_lowercase().contains("operational amplifier"), "{}", lm358.description);
        assert!(lm358.keywords.contains("dual opamp"), "{}", lm358.keywords);
        assert!(lm358.fp_filters.contains("SOIC"), "{}", lm358.fp_filters);
        let parsed = parse_symbol(&text, &list, "LM358").unwrap();
        assert_eq!(parsed.unit_count, 3);
        assert_eq!(parsed.pins.len(), 8);
    }
}
