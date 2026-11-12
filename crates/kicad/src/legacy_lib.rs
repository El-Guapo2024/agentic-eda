//! KiCad's legacy symbol library writer (`.lib`), which the legacy rescue writes its rescue library with: 8303b2ad
//! `eeschema/sch_io/kicad_legacy/sch_io_kicad_legacy_lib_cache.cpp` (`Save`, `SaveSymbol`, `saveField`, `savePin`, `saveArc`, `saveCircle`,
//! `saveRectangle`, `savePolyLine`, `saveBezier`, `saveText`). "Some of the rescue code still uses the legacy format as an intermediary, so we have
//! to keep this code."
//!
//! The symbols are given as their s-expression (`(symbol "Name" ..)` of a `.kicad_sym`, flattened, [`crate::legacy_sch::flattened_symbol`]): the library
//! symbols the studio reads are the KiCad-format copies kicad-cli makes of the legacy libraries, so the s-expression is what holds every drawing item.
//! **Differs**: a fill of a colour or a hatch, which `fill_tab` has no letter for (KiCad indexes past its end), is written `F`; a text box and a
//! rounded rectangle's radius, which the legacy format cannot hold, are left out as `SaveSymbol`'s switch leaves them.

use crate::sexpr::{self, Sexpr};

/// `LIBFILE_IDENT`, `LIB_VERSION_MAJOR`, `LIB_VERSION_MINOR`.
const HEADER: &str = "EESchema-LIBRARY Version 2.4";

/// `schIUScale.IUToMils` of a length in millimetres: `KiROUND( iu / 254.0 )`.
fn mils(mm: f64) -> i64 {
    let iu = (mm * 10_000.0).round();
    (iu / 254.0).round() as i64
}

fn xy(node: &[Sexpr], tag: &str) -> (f64, f64) {
    sexpr::find(node, tag).map(|a| (sexpr::num(a, 1).unwrap_or(0.0), sexpr::num(a, 2).unwrap_or(0.0))).unwrap_or((0.0, 0.0))
}

fn width(node: &[Sexpr]) -> i64 {
    sexpr::find(node, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(mils).unwrap_or(0)
}

/// `fill_tab[ GetFillMode() - 1 ]`: none `N`, outline `F`, background `f`.
fn fill(node: &[Sexpr]) -> char {
    match sexpr::find(node, "fill").and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)) {
        Some("outline") | Some("color") | Some("hatch") | Some("reverse_hatch") | Some("cross_hatch") => 'F',
        Some("background") => 'f',
        _ => 'N',
    }
}

fn yes(node: &[Sexpr], tag: &str) -> bool {
    node.iter().any(|a| a.text() == Some(tag)) || sexpr::find(node, tag).is_some_and(|h| sexpr::txt(h, 1) != Some("no"))
}

/// `EscapedUTF8`: the text in quotes, `"` and `\` escaped.
fn escaped(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text effects of a field or a text: (size, hidden, h justify, v justify, italic, bold).
fn effects(node: &[Sexpr]) -> (f64, bool, char, char, bool, bool) {
    let e = sexpr::find(node, "effects");
    let font = e.and_then(|e| sexpr::find(e, "font"));
    let size = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 2).or_else(|| sexpr::num(s, 1))).unwrap_or(1.27);
    let hidden = yes(node, "hide") || e.is_some_and(|e| yes(e, "hide"));
    let justify: Vec<&str> = e.and_then(|e| sexpr::find(e, "justify")).map(|j| j.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
    let h = if justify.contains(&"left") { 'L' } else if justify.contains(&"right") { 'R' } else { 'C' };
    let v = if justify.contains(&"bottom") { 'B' } else if justify.contains(&"top") { 'T' } else { 'C' };
    let italic = font.is_some_and(|f| yes(f, "italic"));
    let bold = font.is_some_and(|f| yes(f, "bold"));
    (size, hidden, h, v, italic, bold)
}

/// `EDA_ANGLE( VECTOR2D )` in degrees.
fn angle_of(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        0.0
    } else if y == 0.0 {
        if x >= 0.0 {
            0.0
        } else {
            -180.0
        }
    } else if x == 0.0 {
        if y >= 0.0 {
            90.0
        } else {
            -90.0
        }
    } else if x == y {
        if x >= 0.0 {
            45.0
        } else {
            -135.0
        }
    } else if x == -y {
        if x >= 0.0 {
            -45.0
        } else {
            135.0
        }
    } else {
        y.atan2(x).to_degrees()
    }
}

fn normalize180(mut a: f64) -> f64 {
    while a <= -180.0 {
        a += 360.0;
    }
    while a > 180.0 {
        a -= 360.0;
    }
    a
}

/// The centre of the circle through three points.
fn circumcenter(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> (f64, f64) {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-12 {
        return ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0);
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    ((a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d, (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d)
}

/// The unit and body style of a drawing unit `<name>_<unit>_<body style>`.
fn unit_of(name: &str) -> (i64, i64) {
    let mut parts = name.rsplit('_');
    let b = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let u = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (u, b)
}

/// `SCH_IO_KICAD_LEGACY_LIB_CACHE::Save`: the header, each symbol (sorted by name, as `LIB_SYMBOL_MAP` keeps them), the end.
pub fn write_legacy_library(symbols: &[Sexpr]) -> String {
    let mut sorted: Vec<&Sexpr> = symbols.iter().collect();
    sorted.sort_by_key(|s| s.as_list().and_then(|l| sexpr::txt(l, 1)).unwrap_or("").to_string());
    let mut out = format!("{HEADER}\n#encoding utf-8\n");
    for s in sorted {
        if let Some(l) = s.as_list() {
            out.push_str(&save_symbol(l));
        }
    }
    out.push_str("#\n#End Library\n");
    out
}

/// `SaveSymbol` of one flattened symbol.
fn save_symbol(sym: &[Sexpr]) -> String {
    let props: Vec<&[Sexpr]> = sexpr::find_all(sym, "property").collect();
    let prop = |name: &str| props.iter().find(|p| sexpr::txt(p, 1) == Some(name)).copied();
    let text_of = |name: &str| prop(name).and_then(|p| sexpr::txt(p, 2)).unwrap_or("").to_string();
    let value = text_of("Value");
    let reference = text_of("Reference");
    let mut out = format!("#\n# {value}\n#\n");
    let pin_names = sexpr::find(sym, "pin_names");
    let offset = pin_names.and_then(|p| sexpr::find(p, "offset")).and_then(|o| sexpr::num(o, 1)).map(mils).unwrap_or(20);
    let show_names = !pin_names.is_some_and(|p| yes(p, "hide"));
    let show_numbers = !sexpr::find(sym, "pin_numbers").is_some_and(|p| yes(p, "hide"));
    let units: Vec<&[Sexpr]> = sexpr::find_all(sym, "symbol").collect();
    let unit_count = units.iter().map(|u| unit_of(sexpr::txt(u, 1).unwrap_or("")).0).max().unwrap_or(1).max(1);
    let locked = sexpr::find(sym, "units_locked").is_some_and(|u| sexpr::txt(u, 1) != Some("no"));
    let power = sexpr::find(sym, "power").is_some_and(|p| sexpr::txt(p, 1).is_none_or(|k| k == "global"));
    out.push_str(&format!(
        "DEF {value} {} 0 {offset} {} {} {unit_count} {} {}\n",
        if reference.is_empty() { "~".to_string() } else { reference.clone() },
        if show_numbers { 'Y' } else { 'N' },
        if show_names { 'Y' } else { 'N' },
        if locked { 'L' } else { 'F' },
        if power { 'P' } else { 'N' }
    ));
    // the fields by ordinal: Reference, Value, Footprint, Datasheet, Description, then the user fields in their order ("FieldIDs in legacy libraries
    // must be consecutive, and include user fields"); `ki_keywords` and `ki_fp_filters` are not fields
    let mandatory = ["Reference", "Value", "Footprint", "Datasheet", "Description"];
    let mut ordered: Vec<(Option<&[Sexpr]>, &str)> = mandatory.iter().map(|n| (prop(n), *n)).collect();
    for p in &props {
        let name = sexpr::txt(p, 1).unwrap_or("");
        if !mandatory.contains(&name) && !name.starts_with("ki_") {
            ordered.push((Some(*p), name));
        }
    }
    for (id, (p, name)) in ordered.iter().enumerate() {
        out.push_str(&save_field(*p, name, id, mandatory.contains(name)));
    }
    let filters = text_of("ki_fp_filters");
    if !filters.trim().is_empty() {
        out.push_str("$FPLIST\n");
        for f in filters.split_whitespace() {
            out.push_str(&format!(" {f}\n"));
        }
        out.push_str("$ENDFPLIST\n");
    }
    // "Sort the draw items": `LIB_ITEMS_CONTAINER::sort` orders by type (shapes, texts, pins) -- the shapes and texts first, the pins after
    let mut draw: Vec<String> = Vec::new();
    let mut pins: Vec<String> = Vec::new();
    for u in &units {
        let (unit, body) = unit_of(sexpr::txt(u, 1).unwrap_or(""));
        for item in u.iter().filter_map(Sexpr::as_list) {
            match sexpr::tag(item) {
                Some("arc") => draw.push(save_arc(item, unit, body)),
                Some("circle") => {
                    let (cx, cy) = xy(item, "center");
                    let r = sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1)).unwrap_or(0.0);
                    draw.push(format!("C {} {} {} {unit} {body} {} {}\n", mils(cx), mils(cy), mils(r), width(item), fill(item)));
                }
                Some("rectangle") => {
                    let (sx, sy) = xy(item, "start");
                    let (ex, ey) = xy(item, "end");
                    draw.push(format!("S {} {} {} {} {unit} {body} {} {}\n", mils(sx), mils(sy), mils(ex), mils(ey), width(item), fill(item)));
                }
                Some("polyline") => {
                    let pts: Vec<(f64, f64)> = sexpr::find(item, "pts").map(|p| sexpr::find_all(p, "xy").map(|x| (sexpr::num(x, 1).unwrap_or(0.0), sexpr::num(x, 2).unwrap_or(0.0))).collect()).unwrap_or_default();
                    let mut l = format!("P {} {unit} {body} {}", pts.len(), width(item));
                    for (x, y) in &pts {
                        l.push_str(&format!(" {} {}", mils(*x), mils(*y)));
                    }
                    l.push_str(&format!(" {}\n", fill(item)));
                    draw.push(l);
                }
                Some("bezier") => {
                    let pts: Vec<(f64, f64)> = sexpr::find(item, "pts").map(|p| sexpr::find_all(p, "xy").map(|x| (sexpr::num(x, 1).unwrap_or(0.0), sexpr::num(x, 2).unwrap_or(0.0))).collect()).unwrap_or_default();
                    let mut l = format!("B 4 {unit} {body} {}", width(item));
                    for (x, y) in pts.iter().take(4) {
                        l.push_str(&format!(" {} {}", mils(*x), mils(*y)));
                    }
                    l.push_str(&format!(" {}\n", fill(item)));
                    draw.push(l);
                }
                Some("text") => draw.push(save_text(item, unit, body)),
                Some("pin") => pins.push(save_pin(item, unit, body)),
                _ => {}
            }
        }
    }
    if !draw.is_empty() || !pins.is_empty() {
        out.push_str("DRAW\n");
        for l in draw.iter().chain(pins.iter()) {
            out.push_str(l);
        }
        out.push_str("ENDDRAW\n");
    }
    out.push_str("ENDDEF\n");
    out
}

/// `saveField`: `F<n> "text" x y size H|V V|I hjust vjust+italic+bold`, a user field's name when it is not `Field<n>`. A mandatory field the symbol
/// does not have is the empty one KiCad makes (at 0 0, 50 mils, visible, centred).
fn save_field(p: Option<&[Sexpr]>, name: &str, id: usize, mandatory: bool) -> String {
    let Some(p) = p else { return format!("F{id} \"\" 0 0 50 H V C CNN\n") };
    let text = sexpr::txt(p, 2).unwrap_or("");
    let at = sexpr::find(p, "at");
    let (x, y, angle) = at.map(|a| (sexpr::num(a, 1).unwrap_or(0.0), sexpr::num(a, 2).unwrap_or(0.0), sexpr::num(a, 3).unwrap_or(0.0))).unwrap_or((0.0, 0.0, 0.0));
    let (size, hidden, h, v, italic, bold) = effects(p);
    let mut l = format!(
        "F{id} {} {} {} {} {} {} {} {}{}{}",
        escaped(text),
        mils(x),
        mils(y),
        mils(size),
        if (angle.rem_euclid(180.0)).abs() < 1e-9 { 'H' } else { 'V' },
        if hidden { 'I' } else { 'V' },
        h,
        v,
        if italic { 'I' } else { 'N' },
        if bold { 'B' } else { 'N' }
    );
    if !mandatory && !name.is_empty() && name != format!("Field{id}") {
        l.push_str(&format!(" {}", escaped(name)));
    }
    l.push('\n');
    l
}

/// `saveArc`: the centre, radius and the two angles as `CalcArcAngles( endAngle, startAngle )` gives them (each `Normalize180`), the ends.
fn save_arc(item: &[Sexpr], unit: i64, body: i64) -> String {
    let (sx, sy) = xy(item, "start");
    let (mx, my) = xy(item, "mid");
    let (ex, ey) = xy(item, "end");
    // KiCad's own frame is y down: the library's y negated
    let (s, m, e) = ((sx, -sy), (mx, -my), (ex, -ey));
    let c = circumcenter(s, m, e);
    let r = ((s.0 - c.0).powi(2) + (s.1 - c.1).powi(2)).sqrt();
    // `CalcArcAngles( aStartAngle = endAngle, aEndAngle = startAngle )`
    let first = angle_of(s.0 - c.0, s.1 - c.1);
    let mut second = angle_of(e.0 - c.0, e.1 - c.1);
    if second == first {
        second = first + 360.0;
    }
    while second < first {
        second += 360.0;
    }
    let (end_angle, start_angle) = (normalize180(first), normalize180(second));
    format!(
        "A {} {} {} {} {} {unit} {body} {} {} {} {} {} {}\n",
        mils(c.0),
        mils(-c.1),
        mils(r),
        (start_angle * 10.0).round() as i64,
        (end_angle * 10.0).round() as i64,
        width(item),
        fill(item),
        mils(sx),
        mils(sy),
        mils(ex),
        mils(ey)
    )
}

/// `saveText`: quoted when it holds a space, `~` or `"` (a `"` written as two apostrophes).
fn save_text(item: &[Sexpr], unit: i64, body: i64) -> String {
    let mut text = sexpr::txt(item, 1).unwrap_or("").to_string();
    if text.contains(' ') || text.contains('~') || text.contains('"') {
        text = format!("\"{}\"", text.replace('"', "''"));
    }
    let at = sexpr::find(item, "at");
    let (x, y, angle) = at.map(|a| (sexpr::num(a, 1).unwrap_or(0.0), sexpr::num(a, 2).unwrap_or(0.0), sexpr::num(a, 3).unwrap_or(0.0))).unwrap_or((0.0, 0.0, 0.0));
    let (size, hidden, h, v, italic, bold) = effects(item);
    format!(
        "T {} {} {} {} {} {unit} {body} {text} {} {} {h} {v}\n",
        (angle * 10.0).round() as i64,
        mils(x),
        mils(y),
        mils(size),
        i32::from(hidden),
        if italic { "Italic" } else { "Normal" },
        i32::from(bold)
    )
}

/// `savePin`.
fn save_pin(item: &[Sexpr], unit: i64, body: i64) -> String {
    let etype = match sexpr::txt(item, 1) {
        Some("output") => 'O',
        Some("bidirectional") => 'B',
        Some("tri_state") => 'T',
        Some("passive") => 'P',
        Some("unspecified") => 'U',
        Some("power_in") => 'W',
        Some("power_out") => 'w',
        Some("open_collector") => 'C',
        Some("open_emitter") => 'E',
        Some("no_connect") => 'N',
        _ => 'I',
    };
    let shape = match sexpr::txt(item, 2) {
        Some("inverted") => "I",
        Some("clock") => "C",
        Some("inverted_clock") => "IC",
        Some("input_low") => "L",
        Some("clock_low") => "CL",
        Some("output_low") => "V",
        Some("edge_clock_high") => "F",
        Some("non_logic") => "X",
        _ => "",
    };
    let at = sexpr::find(item, "at");
    let (x, y, angle) = at.map(|a| (sexpr::num(a, 1).unwrap_or(0.0), sexpr::num(a, 2).unwrap_or(0.0), sexpr::num(a, 3).unwrap_or(0.0))).unwrap_or((0.0, 0.0, 0.0));
    let orient = match angle.rem_euclid(360.0).round() as i64 {
        90 => 'U',
        180 => 'L',
        270 => 'D',
        _ => 'R',
    };
    let length = sexpr::find(item, "length").and_then(|l| sexpr::num(l, 1)).unwrap_or(2.54);
    let name_node = sexpr::find(item, "name");
    let number_node = sexpr::find(item, "number");
    let name = name_node.and_then(|n| sexpr::txt(n, 1)).unwrap_or("");
    let number = number_node.and_then(|n| sexpr::txt(n, 1)).unwrap_or("");
    let size_of = |n: Option<&[Sexpr]>| n.map(|n| effects(n).0).unwrap_or(1.27);
    let hidden = yes(item, "hide");
    let mut l = format!(
        "X {} {} {} {} {} {orient} {} {} {unit} {body} {etype}",
        if name.is_empty() { "~" } else { name },
        if number.is_empty() { "~" } else { number },
        mils(x),
        mils(y),
        mils(length),
        mils(size_of(number_node)),
        mils(size_of(name_node))
    );
    if !shape.is_empty() || hidden {
        l.push(' ');
    }
    if hidden {
        l.push('N');
    }
    l.push_str(shape);
    l.push('\n');
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_is_written_as_the_legacy_library_writes_it() {
        let lib = "(kicad_symbol_lib (symbol \"R\" (pin_numbers hide) (pin_names (offset 0)) (property \"Reference\" \"R\" (at 2.032 0 90) (effects (font (size 1.27 1.27)))) (property \"Value\" \"R\" (at 0 0 90) (effects (font (size 1.27 1.27)))) (property \"Footprint\" \"\" (at -1.778 0 90) (effects (font (size 1.27 1.27)) (hide yes))) (property \"Datasheet\" \"~\" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes))) (property \"ki_fp_filters\" \"R_*\" (at 0 0 0)) (symbol \"R_0_1\" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none)))) (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\" (effects (font (size 1.27 1.27)))) (number \"1\" (effects (font (size 1.27 1.27))))) (pin power_in line (at 0 -3.81 90) (length 1.27) (hide yes) (name \"VCC\" (effects (font (size 1.27 1.27)))) (number \"2\" (effects (font (size 1.27 1.27))))))))";
        let tree = sexpr::parse(lib).unwrap();
        let sym = sexpr::find(tree.as_list().unwrap(), "symbol").unwrap();
        let text = write_legacy_library(&[Sexpr::List(sym.to_vec())]);
        assert!(text.starts_with("EESchema-LIBRARY Version 2.4\n#encoding utf-8\n#\n# R\n#\nDEF R R 0 0 N Y 1 F N\n"), "{text}");
        assert!(text.contains("F0 \"R\" 80 0 50 V V C CNN\n") && text.contains("F2 \"\" -70 0 50 V I C CNN\n") && text.contains("F4 \"\" 0 0 50 H V C CNN\n"), "{text}");
        assert!(text.contains("$FPLIST\n R_*\n$ENDFPLIST\n"));
        assert!(text.contains("S -40 -100 40 100 0 1 10 N\n"), "{text}");
        assert!(text.contains("X ~ 1 0 150 50 D 50 50 1 1 P\n") && text.contains("X VCC 2 0 -150 50 U 50 50 1 1 W N\n"), "{text}");
        assert!(text.ends_with("ENDDRAW\nENDDEF\n#\n#End Library\n"));
    }
}
