//! INACTIVE (2026-10-28): replaced by KiCad's own nanosvg (`third_party/nanosvg`,
//! from `kicad-src-25ec2335/thirdparty/nanosvg`, built by `build.rs` with
//! `cc` and read in [`super::nanosvg`]); kept for a future all-Rust build.
//! The `rust-nanosvg` cargo feature (off by default) makes `read_svg` use
//! it; it compiles in every build and its tests run.
//!
//! What `SVG_IMPORT_PLUGIN` gets from nanosvg, ported from how nanosvg
//! reads a file (not from KiCad's copy, which the 8303b2ad tree this was
//! written against does not have): a small XML reader and nanosvg's
//! attribute, style, colour, unit and transform rules, the shapes it turns
//! into paths of cubic Bezier curves (`path` with `M L H V C S Q T A Z`,
//! `rect` with its rounded corners, `circle`, `ellipse`, `line`,
//! `polyline`, `polygon`), the scale of the picture to the units asked for
//! (millimetres at 96 dpi, `nsvg__scaleToViewbox`) and the paint of each
//! shape. It ignores `style` elements and `class` attributes, `use`,
//! `text`, `image`, clip paths, masks and patterns; `text`, `image` and
//! `use` say so in a message.

use super::{Paint, Parsed, SPath, SShape};
use crate::graphics_import::*;

/// `SVG_DPI`: nanosvg is given 96 dots to the inch and asked for millimetres.
const SVG_DPI: f64 = 96.0;
/// `NSVG_KAPPA90`: the distance of a cubic Bezier curve's control point that makes a quarter circle.
const KAPPA90: f64 = 0.552_284_749_3;
const PI: f64 = std::f64::consts::PI;
/// `NSVG_MAX_DASHES`.
const MAX_DASHES: usize = 8;

/// Reads the shapes of an SVG text, as `nsvgParse(text, "mm", 96)` does. `Err` is a text with no `<svg>` element.
pub(super) fn parse(text: &str) -> Result<Parsed, String> {
    let events = parse_xml(text.strip_prefix('\u{feff}').unwrap_or(text));
    if !events.iter().any(|e| matches!(e, Xml::Start { name, .. } if *name == "svg")) {
        return Err(super::NOT_SVG.into());
    }
    let mut p = Parser::new();
    for ev in &events {
        match ev {
            Xml::Start { name, attrs } => p.start_element(name, attrs),
            Xml::End(name) => p.end_element(name),
        }
    }
    p.scale_to_viewbox();
    Ok(Parsed { shapes: std::mem::take(&mut p.shapes), messages: std::mem::take(&mut p.messages) })
}

// ------------------------------------------------------------------------------------------------------------------------------- XML

#[derive(Debug, PartialEq)]
enum Xml<'a> {
    Start { name: &'a str, attrs: Vec<(&'a str, &'a str)> },
    End(&'a str),
}

/// The elements of an XML text, in order, with their attributes: comments, processing instructions, `<!DOCTYPE ..>` and CDATA are skipped, and the
/// text between tags is not read (nanosvg's own reader keeps only start and end tags that it knows). A tag that ends in `/>` is a start and an end.
fn parse_xml(text: &str) -> Vec<Xml<'_>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let find = |from: usize, pat: &str| text[from..].find(pat).map(|p| from + p);
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i = find(i + 4, "-->").map_or(b.len(), |p| p + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i = find(i + 9, "]]>").map_or(b.len(), |p| p + 3);
            continue;
        }
        if rest.starts_with("<?") {
            i = find(i + 2, "?>").map_or(b.len(), |p| p + 2);
            continue;
        }
        if rest.starts_with("<!") {
            // <!DOCTYPE ... [ ... ]>: up to the `>` that is not inside the brackets.
            let (mut j, mut depth) = (i + 2, 0i32);
            while j < b.len() {
                match b[j] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        // A tag: up to the `>` that is not inside a quoted value.
        let (mut j, mut quote) = (i + 1, 0u8);
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &text[i + 1..j.min(b.len())];
        i = j + 1;
        let inner = inner.trim_start();
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Xml::End(name.split(|c: char| c.is_whitespace()).next().unwrap_or("").trim_end_matches('>')));
            continue;
        }
        let name_end = inner.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(inner.len());
        let name = &inner[..name_end];
        if name.is_empty() {
            continue;
        }
        let mut attrs = Vec::new();
        let mut closed = false;
        let mut s = &inner[name_end..];
        loop {
            s = s.trim_start();
            if s.is_empty() {
                break;
            }
            if s.starts_with('/') {
                closed = true;
                break;
            }
            let Some(name_end) = s.find(|c: char| c.is_whitespace() || c == '=') else { break };
            let aname = &s[..name_end];
            s = &s[name_end..];
            // To the beginning of the value: the next quote.
            let Some(q) = s.find(['"', '\'']) else { break };
            let quote = s.as_bytes()[q] as char;
            s = &s[q + 1..];
            let Some(end) = s.find(quote) else { break };
            attrs.push((aname, &s[..end]));
            s = &s[end + 1..];
        }
        out.push(Xml::Start { name, attrs });
        if closed {
            out.push(Xml::End(name));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------------- numbers

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

/// `nsvg__parseNumber`: the longest number at the start of `s` (sign, digits, a decimal point, an exponent that is not the `em` or `ex` unit),
/// and the rest. An empty number is 0.
fn number_prefix(s: &str) -> (f64, &str) {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') && !(i + 1 < b.len() && (b[i + 1] == b'm' || b[i + 1] == b'x')) {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }
        let digits = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        // An exponent needs a digit; "1e" is the number 1 and then an "e".
        if j > digits {
            i = j;
        }
    }
    let v = s[..i].parse::<f64>().unwrap_or(0.0);
    (if v.is_finite() { v } else { 0.0 }, &s[i..])
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    User,
    Px,
    Pt,
    Pc,
    Mm,
    Cm,
    In,
    Percent,
    Em,
    Ex,
}

/// `nsvg__parseUnits`.
fn parse_units(s: &str) -> Unit {
    let t = s.as_bytes();
    let two = |a: u8, b: u8| t.len() >= 2 && t[0] == a && t[1] == b;
    if two(b'p', b'x') {
        Unit::Px
    } else if two(b'p', b't') {
        Unit::Pt
    } else if two(b'p', b'c') {
        Unit::Pc
    } else if two(b'm', b'm') {
        Unit::Mm
    } else if two(b'c', b'm') {
        Unit::Cm
    } else if two(b'i', b'n') {
        Unit::In
    } else if !t.is_empty() && t[0] == b'%' {
        Unit::Percent
    } else if two(b'e', b'm') {
        Unit::Em
    } else if two(b'e', b'x') {
        Unit::Ex
    } else {
        Unit::User
    }
}

/// `nsvg__parseCoordinateRaw`.
fn parse_coordinate_raw(s: &str) -> (f64, Unit) {
    let (v, rest) = number_prefix(s.trim_start());
    (v, parse_units(rest))
}

/// `nsvg__getNextPathItem`: the next number or command letter of a path or a list of points, and the rest. `None` at the end.
fn next_item(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start_matches(|c: char| is_space(c) || c == ',');
    let c = s.chars().next()?;
    if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
        let (_, rest) = number_prefix(s);
        let len = s.len() - rest.len();
        // A lone sign or point is not a number: take it as one character so the reader goes on.
        let len = if len == 0 { c.len_utf8() } else { len };
        Some((&s[..len], &s[len..]))
    } else {
        Some((&s[..c.len_utf8()], &s[c.len_utf8()..]))
    }
}

/// `nsvg__isCoordinate`.
fn is_coordinate(item: &str) -> bool {
    let t = item.strip_prefix(['-', '+']).unwrap_or(item);
    t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

// ------------------------------------------------------------------------------------------------------------------------- transforms

/// A transform in nanosvg's layout `[a, b, c, d, e, f]`: `x' = a x + c y + e`, `y' = b x + d y + f`.
type Aff = [f64; 6];
const IDENTITY: Aff = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `nsvg__xformMultiply( t, s )`: `t = t * s` (apply `t` first).
fn xform_multiply(t: &mut Aff, s: &Aff) {
    let t0 = t[0] * s[0] + t[1] * s[2];
    let t2 = t[2] * s[0] + t[3] * s[2];
    let t4 = t[4] * s[0] + t[5] * s[2] + s[4];
    t[1] = t[0] * s[1] + t[1] * s[3];
    t[3] = t[2] * s[1] + t[3] * s[3];
    t[5] = t[4] * s[1] + t[5] * s[3] + s[5];
    t[0] = t0;
    t[2] = t2;
    t[4] = t4;
}

/// `nsvg__xformPremultiply( t, s )`: `t = s * t` (apply `s` first).
fn xform_premultiply(t: &mut Aff, s: &Aff) {
    let mut s2 = *s;
    xform_multiply(&mut s2, t);
    *t = s2;
}

fn xform_point(x: f64, y: f64, t: &Aff) -> (f64, f64) {
    (x * t[0] + y * t[2] + t[4], x * t[1] + y * t[3] + t[5])
}

fn xform_vec(x: f64, y: f64, t: &Aff) -> (f64, f64) {
    (x * t[0] + y * t[2], x * t[1] + y * t[3])
}

/// `nsvg__getAverageScale`.
fn average_scale(t: &Aff) -> f64 {
    let sx = (t[0] * t[0] + t[2] * t[2]).sqrt();
    let sy = (t[1] * t[1] + t[3] * t[3]).sqrt();
    (sx + sy) * 0.5
}

/// The arguments of a transform function: the numbers between its parentheses, and what follows the closing one.
fn transform_args(s: &str) -> Option<(Vec<f64>, &str)> {
    let open = s.find('(')?;
    let close = s[open..].find(')')? + open;
    let mut args = Vec::new();
    let mut p = &s[open + 1..close];
    loop {
        p = p.trim_start_matches(|c: char| !(c == '-' || c == '+' || c == '.' || c.is_ascii_digit()));
        if p.is_empty() {
            break;
        }
        let (v, rest) = number_prefix(p);
        args.push(v);
        p = if rest.len() == p.len() { &p[1..] } else { rest };
    }
    Some((args, &s[close + 1..]))
}

/// `nsvg__parseTransform`: a list of `matrix`, `translate`, `scale`, `rotate`, `skewX` and `skewY`, the first one outermost.
fn parse_transform(s: &str) -> Aff {
    let mut xform = IDENTITY;
    let mut s = s;
    while !s.is_empty() {
        let t: Option<(Aff, &str)> = if s.starts_with("matrix") {
            transform_args(s).filter(|(a, _)| a.len() == 6).map(|(a, r)| ([a[0], a[1], a[2], a[3], a[4], a[5]], r))
        } else if s.starts_with("translate") {
            transform_args(s).filter(|(a, _)| !a.is_empty()).map(|(a, r)| ([1.0, 0.0, 0.0, 1.0, a[0], a.get(1).copied().unwrap_or(0.0)], r))
        } else if s.starts_with("scale") {
            transform_args(s).filter(|(a, _)| !a.is_empty()).map(|(a, r)| ([a[0], 0.0, 0.0, a.get(1).copied().unwrap_or(a[0]), 0.0, 0.0], r))
        } else if s.starts_with("rotate") {
            transform_args(s).filter(|(a, _)| !a.is_empty()).map(|(a, r)| {
                let rad = a[0] / 180.0 * PI;
                let (sn, cs) = rad.sin_cos();
                let (cx, cy) = (a.get(1).copied().unwrap_or(0.0), a.get(2).copied().unwrap_or(0.0));
                ([cs, sn, -sn, cs, cx - cs * cx + sn * cy, cy - sn * cx - cs * cy], r)
            })
        } else if s.starts_with("skewX") {
            transform_args(s).filter(|(a, _)| !a.is_empty()).map(|(a, r)| ([1.0, 0.0, (a[0] / 180.0 * PI).tan(), 1.0, 0.0, 0.0], r))
        } else if s.starts_with("skewY") {
            transform_args(s).filter(|(a, _)| !a.is_empty()).map(|(a, r)| ([1.0, (a[0] / 180.0 * PI).tan(), 0.0, 1.0, 0.0, 0.0], r))
        } else {
            None
        };
        match t {
            Some((t, rest)) => {
                xform_premultiply(&mut xform, &t);
                s = rest;
            }
            None => {
                let mut it = s.chars();
                it.next();
                s = it.as_str();
            }
        }
    }
    xform
}

// ---------------------------------------------------------------------------------------------------------------------------- colours

/// `NSVG_RGB`.
type Rgb = (u8, u8, u8);

/// nanosvg's table of the CSS colour names.
const COLORS: [(&str, Rgb); 147] = [
    ("aliceblue", (240, 248, 255)),
    ("antiquewhite", (250, 235, 215)),
    ("aqua", (0, 255, 255)),
    ("aquamarine", (127, 255, 212)),
    ("azure", (240, 255, 255)),
    ("beige", (245, 245, 220)),
    ("bisque", (255, 228, 196)),
    ("black", (0, 0, 0)),
    ("blanchedalmond", (255, 235, 205)),
    ("blue", (0, 0, 255)),
    ("blueviolet", (138, 43, 226)),
    ("brown", (165, 42, 42)),
    ("burlywood", (222, 184, 135)),
    ("cadetblue", (95, 158, 160)),
    ("chartreuse", (127, 255, 0)),
    ("chocolate", (210, 105, 30)),
    ("coral", (255, 127, 80)),
    ("cornflowerblue", (100, 149, 237)),
    ("cornsilk", (255, 248, 220)),
    ("crimson", (220, 20, 60)),
    ("cyan", (0, 255, 255)),
    ("darkblue", (0, 0, 139)),
    ("darkcyan", (0, 139, 139)),
    ("darkgoldenrod", (184, 134, 11)),
    ("darkgray", (169, 169, 169)),
    ("darkgreen", (0, 100, 0)),
    ("darkgrey", (169, 169, 169)),
    ("darkkhaki", (189, 183, 107)),
    ("darkmagenta", (139, 0, 139)),
    ("darkolivegreen", (85, 107, 47)),
    ("darkorange", (255, 140, 0)),
    ("darkorchid", (153, 50, 204)),
    ("darkred", (139, 0, 0)),
    ("darksalmon", (233, 150, 122)),
    ("darkseagreen", (143, 188, 143)),
    ("darkslateblue", (72, 61, 139)),
    ("darkslategray", (47, 79, 79)),
    ("darkslategrey", (47, 79, 79)),
    ("darkturquoise", (0, 206, 209)),
    ("darkviolet", (148, 0, 211)),
    ("deeppink", (255, 20, 147)),
    ("deepskyblue", (0, 191, 255)),
    ("dimgray", (105, 105, 105)),
    ("dimgrey", (105, 105, 105)),
    ("dodgerblue", (30, 144, 255)),
    ("firebrick", (178, 34, 34)),
    ("floralwhite", (255, 250, 240)),
    ("forestgreen", (34, 139, 34)),
    ("fuchsia", (255, 0, 255)),
    ("gainsboro", (220, 220, 220)),
    ("ghostwhite", (248, 248, 255)),
    ("gold", (255, 215, 0)),
    ("goldenrod", (218, 165, 32)),
    ("gray", (128, 128, 128)),
    ("grey", (128, 128, 128)),
    ("green", (0, 128, 0)),
    ("greenyellow", (173, 255, 47)),
    ("honeydew", (240, 255, 240)),
    ("hotpink", (255, 105, 180)),
    ("indianred", (205, 92, 92)),
    ("indigo", (75, 0, 130)),
    ("ivory", (255, 255, 240)),
    ("khaki", (240, 230, 140)),
    ("lavender", (230, 230, 250)),
    ("lavenderblush", (255, 240, 245)),
    ("lawngreen", (124, 252, 0)),
    ("lemonchiffon", (255, 250, 205)),
    ("lightblue", (173, 216, 230)),
    ("lightcoral", (240, 128, 128)),
    ("lightcyan", (224, 255, 255)),
    ("lightgoldenrodyellow", (250, 250, 210)),
    ("lightgray", (211, 211, 211)),
    ("lightgreen", (144, 238, 144)),
    ("lightgrey", (211, 211, 211)),
    ("lightpink", (255, 182, 193)),
    ("lightsalmon", (255, 160, 122)),
    ("lightseagreen", (32, 178, 170)),
    ("lightskyblue", (135, 206, 250)),
    ("lightslategray", (119, 136, 153)),
    ("lightslategrey", (119, 136, 153)),
    ("lightsteelblue", (176, 196, 222)),
    ("lightyellow", (255, 255, 224)),
    ("lime", (0, 255, 0)),
    ("limegreen", (50, 205, 50)),
    ("linen", (250, 240, 230)),
    ("magenta", (255, 0, 255)),
    ("maroon", (128, 0, 0)),
    ("mediumaquamarine", (102, 205, 170)),
    ("mediumblue", (0, 0, 205)),
    ("mediumorchid", (186, 85, 211)),
    ("mediumpurple", (147, 112, 219)),
    ("mediumseagreen", (60, 179, 113)),
    ("mediumslateblue", (123, 104, 238)),
    ("mediumspringgreen", (0, 250, 154)),
    ("mediumturquoise", (72, 209, 204)),
    ("mediumvioletred", (199, 21, 133)),
    ("midnightblue", (25, 25, 112)),
    ("mintcream", (245, 255, 250)),
    ("mistyrose", (255, 228, 225)),
    ("moccasin", (255, 228, 181)),
    ("navajowhite", (255, 222, 173)),
    ("navy", (0, 0, 128)),
    ("oldlace", (253, 245, 230)),
    ("olive", (128, 128, 0)),
    ("olivedrab", (107, 142, 35)),
    ("orange", (255, 165, 0)),
    ("orangered", (255, 69, 0)),
    ("orchid", (218, 112, 214)),
    ("palegoldenrod", (238, 232, 170)),
    ("palegreen", (152, 251, 152)),
    ("paleturquoise", (175, 238, 238)),
    ("palevioletred", (219, 112, 147)),
    ("papayawhip", (255, 239, 213)),
    ("peachpuff", (255, 218, 185)),
    ("peru", (205, 133, 63)),
    ("pink", (255, 192, 203)),
    ("plum", (221, 160, 221)),
    ("powderblue", (176, 224, 230)),
    ("purple", (128, 0, 128)),
    ("red", (255, 0, 0)),
    ("rosybrown", (188, 143, 143)),
    ("royalblue", (65, 105, 225)),
    ("saddlebrown", (139, 69, 19)),
    ("salmon", (250, 128, 114)),
    ("sandybrown", (244, 164, 96)),
    ("seagreen", (46, 139, 87)),
    ("seashell", (255, 245, 238)),
    ("sienna", (160, 82, 45)),
    ("silver", (192, 192, 192)),
    ("skyblue", (135, 206, 235)),
    ("slateblue", (106, 90, 205)),
    ("slategray", (112, 128, 144)),
    ("slategrey", (112, 128, 144)),
    ("snow", (255, 250, 250)),
    ("springgreen", (0, 255, 127)),
    ("steelblue", (70, 130, 180)),
    ("tan", (210, 180, 140)),
    ("teal", (0, 128, 128)),
    ("thistle", (216, 191, 216)),
    ("tomato", (255, 99, 71)),
    ("turquoise", (64, 224, 208)),
    ("violet", (238, 130, 238)),
    ("wheat", (245, 222, 179)),
    ("white", (255, 255, 255)),
    ("whitesmoke", (245, 245, 245)),
    ("yellow", (255, 255, 0)),
    ("yellowgreen", (154, 205, 50)),
];

/// `nsvg__parseColor`: `#rgb`, `#rrggbb`, `rgb(r, g, b)` (each part a number or a percentage), else a colour name; a name nanosvg does not know is grey.
fn parse_color(value: &str) -> Rgb {
    let s = value.trim_start();
    if let Some(hex) = s.strip_prefix('#') {
        let h: Vec<u8> = hex.chars().take_while(|c| c.is_ascii_hexdigit()).map(|c| c.to_digit(16).unwrap() as u8).collect();
        // `sscanf( "%x" )` reads as many digits as there are: three is the short form, anything longer is read as six.
        if h.len() == 3 {
            return (h[0] * 17, h[1] * 17, h[2] * 17);
        }
        if h.len() >= 6 {
            return (h[0] * 16 + h[1], h[2] * 16 + h[3], h[4] * 16 + h[5]);
        }
        return (0, 0, 0);
    }
    if s.len() >= 4 && s[..4].eq_ignore_ascii_case("rgb(") {
        let inner = s[4..].split(')').next().unwrap_or("");
        let part = |p: &str| -> u8 {
            let p = p.trim();
            let (v, rest) = number_prefix(p);
            let v = if rest.trim_start().starts_with('%') { v * 255.0 / 100.0 } else { v };
            v.clamp(0.0, 255.0) as u8
        };
        let mut it = inner.split(',');
        let (r, g, b) = (it.next().map_or(0, part), it.next().map_or(0, part), it.next().map_or(0, part));
        return (r, g, b);
    }
    let name = s.trim_end().to_ascii_lowercase();
    COLORS.iter().find(|(n, _)| *n == name).map_or((128, 128, 128), |(_, c)| *c)
}

fn parse_opacity(value: &str) -> f64 {
    number_prefix(value.trim_start()).0.clamp(0.0, 1.0)
}

// ------------------------------------------------------------------------------------------------------------------------------ state

#[derive(Clone)]
struct Attr {
    xform: Aff,
    fill_color: Rgb,
    stroke_color: Rgb,
    fill_opacity: f64,
    stroke_opacity: f64,
    stroke_width: f64,
    dash: Vec<f64>,
    fill_rule: FillRule,
    /// 0: none, 1: a colour, 2: a gradient (`url(...)`).
    has_fill: u8,
    has_stroke: u8,
    fill_url: String,
    stroke_url: String,
    font_size: f64,
    visible: bool,
}

impl Attr {
    /// The attributes of a document before anything sets one: filled black, no stroke, a stroke width of 1.
    fn initial() -> Attr {
        Attr {
            xform: IDENTITY,
            fill_color: (0, 0, 0),
            stroke_color: (0, 0, 0),
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            dash: Vec::new(),
            fill_rule: FillRule::NonZero,
            has_fill: 1,
            has_stroke: 0,
            fill_url: String::new(),
            stroke_url: String::new(),
            font_size: 16.0,
            visible: true,
        }
    }
}


#[derive(Clone, Copy, PartialEq)]
enum Align {
    Min,
    Mid,
    Max,
}

#[derive(Clone, Copy, PartialEq)]
enum AlignType {
    Meet,
    Slice,
    None,
}

struct Parser {
    attrs: Vec<Attr>,
    /// The path being built (`p->pts`): points of cubic curves.
    pts: Vec<(f64, f64)>,
    shapes: Vec<SShape>,
    plist: Vec<SPath>,
    gradients: Vec<String>,
    defs: bool,
    // The size of the picture and its view box (`image->width`, `viewMinx` ...), in pixels.
    width: f64,
    height: f64,
    view_min_x: f64,
    view_min_y: f64,
    view_width: f64,
    view_height: f64,
    align_x: Align,
    align_y: Align,
    align_type: AlignType,
    messages: Vec<String>,
}

impl Parser {
    fn new() -> Parser {
        Parser {
            attrs: vec![Attr::initial()],
            pts: Vec::new(),
            shapes: Vec::new(),
            plist: Vec::new(),
            gradients: Vec::new(),
            defs: false,
            width: 0.0,
            height: 0.0,
            view_min_x: 0.0,
            view_min_y: 0.0,
            view_width: 0.0,
            view_height: 0.0,
            align_x: Align::Mid,
            align_y: Align::Mid,
            align_type: AlignType::Meet,
            messages: Vec::new(),
        }
    }

    fn attr(&mut self) -> &mut Attr {
        self.attrs.last_mut().expect("the base attributes are never popped")
    }

    fn push_attr(&mut self) {
        let a = self.attrs.last().cloned().expect("the base attributes are never popped");
        self.attrs.push(a);
    }

    fn pop_attr(&mut self) {
        if self.attrs.len() > 1 {
            self.attrs.pop();
        }
    }

    fn actual_length(&self) -> f64 {
        (self.view_width * self.view_width + self.view_height * self.view_height).sqrt() / 2.0_f64.sqrt()
    }

    /// `nsvg__convertToPixels`.
    fn pixels(&self, c: (f64, Unit), orig: f64, length: f64) -> f64 {
        let fs = self.attrs.last().map_or(16.0, |a| a.font_size);
        match c.1 {
            Unit::User | Unit::Px => c.0,
            Unit::Pt => c.0 / 72.0 * SVG_DPI,
            Unit::Pc => c.0 / 6.0 * SVG_DPI,
            Unit::Mm => c.0 / 25.4 * SVG_DPI,
            Unit::Cm => c.0 / 2.54 * SVG_DPI,
            Unit::In => c.0 * SVG_DPI,
            Unit::Em => c.0 * fs,
            Unit::Ex => c.0 * fs * 0.52,
            Unit::Percent => orig + c.0 / 100.0 * length,
        }
    }

    fn coordinate(&self, s: &str, orig: f64, length: f64) -> f64 {
        self.pixels(parse_coordinate_raw(s), orig, length)
    }

    // ------------------------------------------------------------------------------------------------------------------ attributes

    /// `nsvg__parseAttr`: whether `name` is one of the presentation attributes (and was applied).
    fn parse_attr(&mut self, name: &str, value: &str) -> bool {
        match name {
            "style" => self.parse_style(value),
            "display" => {
                // "Don't reset ->visible on display:inline, one display:none is enough to hide the node."
                if value == "none" {
                    self.attr().visible = false;
                }
            }
            "fill" => {
                if value == "none" {
                    self.attr().has_fill = 0;
                } else if value.starts_with("url(") {
                    let url = parse_url(value);
                    let a = self.attr();
                    a.has_fill = 2;
                    a.fill_url = url;
                } else {
                    let c = parse_color(value);
                    let a = self.attr();
                    a.has_fill = 1;
                    a.fill_color = c;
                }
            }
            "opacity" => {}
            "fill-opacity" => self.attr().fill_opacity = parse_opacity(value),
            "stroke" => {
                if value == "none" {
                    self.attr().has_stroke = 0;
                } else if value.starts_with("url(") {
                    let url = parse_url(value);
                    let a = self.attr();
                    a.has_stroke = 2;
                    a.stroke_url = url;
                } else {
                    let c = parse_color(value);
                    let a = self.attr();
                    a.has_stroke = 1;
                    a.stroke_color = c;
                }
            }
            "stroke-width" => {
                let l = self.actual_length();
                let w = self.coordinate(value, 0.0, l);
                self.attr().stroke_width = w;
            }
            "stroke-dasharray" => {
                let l = self.actual_length();
                let dash = self.parse_dash_array(value, l);
                self.attr().dash = dash;
            }
            "stroke-opacity" => self.attr().stroke_opacity = parse_opacity(value),
            // Read by nanosvg and of no use to an importer: the dash offset, the caps and joins, the miter limit, the colours of a gradient's stops, the ids.
            "stroke-dashoffset" | "stroke-linecap" | "stroke-linejoin" | "stroke-miterlimit" | "stop-color" | "stop-opacity" | "offset" | "id" => {}
            "fill-rule" => self.attr().fill_rule = if value == "evenodd" { FillRule::EvenOdd } else { FillRule::NonZero },
            "font-size" => {
                let l = self.actual_length();
                let s = self.coordinate(value, 0.0, l);
                self.attr().font_size = s;
            }
            "transform" => {
                let x = parse_transform(value);
                xform_premultiply(&mut self.attr().xform, &x);
            }
            _ => return false,
        }
        true
    }

    /// `nsvg__parseStyle`: `name: value; name: value`.
    fn parse_style(&mut self, style: &str) {
        for decl in style.split(';') {
            if let Some((name, value)) = decl.split_once(':') {
                self.parse_attr(name.trim(), value.trim());
            }
        }
    }

    /// `nsvg__parseAttribs`.
    fn parse_attribs(&mut self, attrs: &[(&str, &str)]) {
        for (name, value) in attrs {
            if *name == "style" {
                self.parse_style(value);
            } else {
                self.parse_attr(name, value);
            }
        }
    }

    /// `nsvg__parseStrokeDashArray`.
    fn parse_dash_array(&self, s: &str, length: f64) -> Vec<f64> {
        if s.starts_with('n') {
            return Vec::new();
        }
        let mut dash = Vec::new();
        for item in s.split(|c: char| is_space(c) || c == ',').filter(|t| !t.is_empty()) {
            if dash.len() < MAX_DASHES {
                dash.push(self.coordinate(item, 0.0, length).abs());
            }
        }
        if dash.iter().sum::<f64>() <= 1e-6 {
            dash.clear();
        }
        dash
    }

    // ------------------------------------------------------------------------------------------------------------------------ paths

    fn reset_path(&mut self) {
        self.pts.clear();
    }

    fn add_point(&mut self, x: f64, y: f64) {
        self.pts.push((x, y));
    }

    /// `nsvg__moveTo`: a move after a move replaces the point.
    fn move_to(&mut self, x: f64, y: f64) {
        if let Some(last) = self.pts.last_mut() {
            *last = (x, y);
        } else {
            self.add_point(x, y);
        }
    }

    /// `nsvg__lineTo`: a cubic curve whose control points are at the thirds of the line.
    fn line_to(&mut self, x: f64, y: f64) {
        if let Some(&(px, py)) = self.pts.last() {
            let (dx, dy) = (x - px, y - py);
            self.add_point(px + dx / 3.0, py + dy / 3.0);
            self.add_point(x - dx / 3.0, y - dy / 3.0);
            self.add_point(x, y);
        }
    }

    fn cubic_bez_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        if !self.pts.is_empty() {
            self.add_point(cx1, cy1);
            self.add_point(cx2, cy2);
            self.add_point(x, y);
        }
    }

    /// `nsvg__addPath`: the path being built, transformed, becomes a path of the shape being built. A path under four points, or whose number of
    /// points is not 1 + 3n, is dropped; a closed one gets a line back to where it began.
    fn add_path(&mut self, closed: bool) {
        if self.pts.len() < 4 {
            return;
        }
        if closed {
            let (x, y) = self.pts[0];
            self.line_to(x, y);
        }
        if self.pts.len() % 3 != 1 {
            return;
        }
        let xf = self.attrs.last().map_or(IDENTITY, |a| a.xform);
        let pts = self.pts.iter().map(|&(x, y)| xform_point(x, y, &xf)).collect();
        self.plist.push(SPath { pts, closed });
    }

    /// `nsvg__addShape`: the paths built so far, with the paint of the attributes in force, are a shape.
    fn add_shape(&mut self) {
        let a = self.attrs.last().cloned().expect("the base attributes are never popped");
        let paths = std::mem::take(&mut self.plist);
        let paint = |has: u8, color: Rgb, opacity: f64, url: &str, gradients: &[String]| -> Paint {
            match has {
                0 => Paint::None,
                1 => Paint::Color(Rgba { r: color.0 as f64 / 255.0, g: color.1 as f64 / 255.0, b: color.2 as f64 / 255.0, a: ((opacity * 255.0) as u32).min(255) as f64 / 255.0 }),
                _ => {
                    if gradients.iter().any(|g| g == url) {
                        Paint::Gradient
                    } else {
                        Paint::None
                    }
                }
            }
        };
        let fill = paint(a.has_fill, a.fill_color, a.fill_opacity, &a.fill_url, &self.gradients);
        let stroke = paint(a.has_stroke, a.stroke_color, a.stroke_opacity, &a.stroke_url, &self.gradients);
        let scale = average_scale(&a.xform);
        self.shapes.push(SShape { paths, fill, stroke, stroke_width: a.stroke_width * scale, dash: a.dash.iter().map(|d| d * scale).collect(), fill_rule: a.fill_rule, visible: a.visible });
    }

    fn path_move_to(&mut self, cp: &mut (f64, f64), args: &[f64], rel: bool) {
        if rel {
            cp.0 += args[0];
            cp.1 += args[1];
        } else {
            *cp = (args[0], args[1]);
        }
        self.move_to(cp.0, cp.1);
    }

    fn path_line_to(&mut self, cp: &mut (f64, f64), args: &[f64], rel: bool) {
        if rel {
            cp.0 += args[0];
            cp.1 += args[1];
        } else {
            *cp = (args[0], args[1]);
        }
        self.line_to(cp.0, cp.1);
    }

    fn path_cubic_bez_to(&mut self, cp: &mut (f64, f64), cp2: &mut (f64, f64), args: &[f64], rel: bool) {
        let (x1, y1, x2, y2, x3, y3) = if rel {
            (cp.0 + args[0], cp.1 + args[1], cp.0 + args[2], cp.1 + args[3], cp.0 + args[4], cp.1 + args[5])
        } else {
            (args[0], args[1], args[2], args[3], args[4], args[5])
        };
        self.cubic_bez_to(x1, y1, x2, y2, x3, y3);
        *cp2 = (x2, y2);
        *cp = (x3, y3);
    }

    fn path_cubic_bez_short_to(&mut self, cp: &mut (f64, f64), cp2: &mut (f64, f64), args: &[f64], rel: bool) {
        let (x1, y1) = *cp;
        let (x2, y2, x3, y3) = if rel { (cp.0 + args[0], cp.1 + args[1], cp.0 + args[2], cp.1 + args[3]) } else { (args[0], args[1], args[2], args[3]) };
        let (cx1, cy1) = (2.0 * x1 - cp2.0, 2.0 * y1 - cp2.1);
        self.cubic_bez_to(cx1, cy1, x2, y2, x3, y3);
        *cp2 = (x2, y2);
        *cp = (x3, y3);
    }

    fn path_quad_bez_to(&mut self, cp: &mut (f64, f64), cp2: &mut (f64, f64), args: &[f64], rel: bool) {
        let (x1, y1) = *cp;
        let (cx, cy, x2, y2) = if rel { (cp.0 + args[0], cp.1 + args[1], cp.0 + args[2], cp.1 + args[3]) } else { (args[0], args[1], args[2], args[3]) };
        self.quad_to_cubic(x1, y1, cx, cy, x2, y2);
        *cp2 = (cx, cy);
        *cp = (x2, y2);
    }

    fn path_quad_bez_short_to(&mut self, cp: &mut (f64, f64), cp2: &mut (f64, f64), args: &[f64], rel: bool) {
        let (x1, y1) = *cp;
        let (x2, y2) = if rel { (cp.0 + args[0], cp.1 + args[1]) } else { (args[0], args[1]) };
        let (cx, cy) = (2.0 * x1 - cp2.0, 2.0 * y1 - cp2.1);
        self.quad_to_cubic(x1, y1, cx, cy, x2, y2);
        *cp2 = (cx, cy);
        *cp = (x2, y2);
    }

    /// A quadratic curve as a cubic one.
    fn quad_to_cubic(&mut self, x1: f64, y1: f64, cx: f64, cy: f64, x2: f64, y2: f64) {
        let (cx1, cy1) = (x1 + 2.0 / 3.0 * (cx - x1), y1 + 2.0 / 3.0 * (cy - y1));
        let (cx2, cy2) = (x2 + 2.0 / 3.0 * (cx - x2), y2 + 2.0 / 3.0 * (cy - y2));
        self.cubic_bez_to(cx1, cy1, cx2, cy2, x2, y2);
    }

    /// `nsvg__pathArcTo`: an elliptical arc as cubic curves of at most a quarter turn each (from canvg, by way of nanosvg).
    fn path_arc_to(&mut self, cp: &mut (f64, f64), args: &[f64], rel: bool) {
        let mut rx = args[0].abs();
        let mut ry = args[1].abs();
        let rotx = args[2] / 180.0 * PI;
        let fa = args[3].abs() > 1e-6;
        let fs = args[4].abs() > 1e-6;
        let (x1, y1) = *cp;
        let (x2, y2) = if rel { (cp.0 + args[5], cp.1 + args[6]) } else { (args[5], args[6]) };
        let dx = x1 - x2;
        let dy = y1 - y2;
        let d = (dx * dx + dy * dy).sqrt();
        if d < 1e-6 || rx < 1e-6 || ry < 1e-6 {
            // The arc degenerates to a line.
            self.line_to(x2, y2);
            *cp = (x2, y2);
            return;
        }
        let (sinrx, cosrx) = rotx.sin_cos();
        // Convert to center point parameterization (http://www.w3.org/TR/SVG11/implnote.html#ArcImplementationNotes).
        let x1p = cosrx * dx / 2.0 + sinrx * dy / 2.0;
        let y1p = -sinrx * dx / 2.0 + cosrx * dy / 2.0;
        let d = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
        if d > 1.0 {
            let d = d.sqrt();
            rx *= d;
            ry *= d;
        }
        let mut s = 0.0;
        let mut sa = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let sb = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        if sa < 0.0 {
            sa = 0.0;
        }
        if sb > 0.0 {
            s = (sa / sb).sqrt();
        }
        if fa == fs {
            s = -s;
        }
        let cxp = s * rx * y1p / ry;
        let cyp = s * -ry * x1p / rx;
        let cx = (x1 + x2) / 2.0 + cosrx * cxp - sinrx * cyp;
        let cy = (y1 + y2) / 2.0 + sinrx * cxp + cosrx * cyp;
        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;
        let vecang = |ux: f64, uy: f64, vx: f64, vy: f64| -> f64 {
            let r = ((ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt())).clamp(-1.0, 1.0);
            (if ux * vy < uy * vx { -1.0 } else { 1.0 }) * r.acos()
        };
        let a1 = vecang(1.0, 0.0, ux, uy);
        let mut da = vecang(ux, uy, vx, vy);
        if !fs && da > 0.0 {
            da -= 2.0 * PI;
        } else if fs && da < 0.0 {
            da += 2.0 * PI;
        }
        let t: Aff = [cosrx, sinrx, -sinrx, cosrx, cx, cy];
        // Split the arc into segments of at most 90 degrees.
        let ndivs = (da.abs() / (PI * 0.5) + 1.0) as usize;
        let mut hda = (da / ndivs as f64) / 2.0;
        if hda < 1e-3 && hda > -1e-3 {
            hda *= 0.5;
        } else {
            hda = (1.0 - hda.cos()) / hda.sin();
        }
        let mut kappa = (4.0 / 3.0 * hda).abs();
        if da < 0.0 {
            kappa = -kappa;
        }
        let (mut px, mut py, mut ptanx, mut ptany) = (0.0, 0.0, 0.0, 0.0);
        for i in 0..=ndivs {
            let a = a1 + da * (i as f64 / ndivs as f64);
            let (dy, dx) = a.sin_cos();
            let (x, y) = xform_point(dx * rx, dy * ry, &t);
            let (tanx, tany) = xform_vec(-dy * rx * kappa, dx * ry * kappa, &t);
            if i > 0 {
                self.cubic_bez_to(px + ptanx, py + ptany, x - tanx, y - tany, x, y);
            }
            px = x;
            py = y;
            ptanx = tanx;
            ptany = tany;
        }
        *cp = (x2, y2);
    }

    /// `nsvg__parsePath`.
    fn parse_path(&mut self, attrs: &[(&str, &str)]) {
        let mut d: Option<&str> = None;
        for (name, value) in attrs {
            if *name == "d" {
                d = Some(value);
            } else {
                self.parse_attr(name, value);
            }
        }
        if let Some(d) = d {
            self.reset_path();
            let mut cp = (0.0, 0.0);
            let mut cp2 = (0.0, 0.0);
            let mut cmd = '\0';
            let mut init_point = false;
            let mut closed_flag = false;
            let mut args: Vec<f64> = Vec::with_capacity(10);
            let mut rargs: usize = 0;
            let args_per = |c: char| -> Option<usize> {
                match c {
                    'm' | 'M' | 'l' | 'L' | 't' | 'T' => Some(2),
                    'h' | 'H' | 'v' | 'V' => Some(1),
                    'c' | 'C' => Some(6),
                    's' | 'S' | 'q' | 'Q' => Some(4),
                    'a' | 'A' => Some(7),
                    'z' | 'Z' => Some(0),
                    _ => None,
                }
            };
            let mut s = d;
            loop {
                // `nsvg__getNextPathItemWhenArc`: an arc's two flags are one character each and need no separator after them.
                let skip = s.trim_start_matches(|c: char| is_space(c) || c == ',');
                let item: &str;
                if matches!(cmd, 'a' | 'A') && (args.len() == 3 || args.len() == 4) && skip.starts_with(['0', '1']) {
                    item = &skip[..1];
                    s = &skip[1..];
                } else {
                    match next_item(s) {
                        Some((it, rest)) => {
                            item = it;
                            s = rest;
                        }
                        None => break,
                    }
                }
                if cmd != '\0' && is_coordinate(item) {
                    if args.len() < 10 {
                        args.push(item.parse::<f64>().unwrap_or(0.0));
                    }
                    if args.len() >= rargs {
                        let rel = cmd.is_ascii_lowercase();
                        match cmd {
                            'm' | 'M' => {
                                self.path_move_to(&mut cp, &args, rel);
                                // "Moveto can be followed by multiple coordinate pairs, which should be treated as linetos."
                                cmd = if cmd == 'm' { 'l' } else { 'L' };
                                rargs = 2;
                                cp2 = cp;
                                init_point = true;
                            }
                            'l' | 'L' => {
                                self.path_line_to(&mut cp, &args, rel);
                                cp2 = cp;
                            }
                            'h' | 'H' => {
                                if rel {
                                    cp.0 += args[0];
                                } else {
                                    cp.0 = args[0];
                                }
                                self.line_to(cp.0, cp.1);
                                cp2 = cp;
                            }
                            'v' | 'V' => {
                                if rel {
                                    cp.1 += args[0];
                                } else {
                                    cp.1 = args[0];
                                }
                                self.line_to(cp.0, cp.1);
                                cp2 = cp;
                            }
                            'c' | 'C' => self.path_cubic_bez_to(&mut cp, &mut cp2, &args, rel),
                            's' | 'S' => self.path_cubic_bez_short_to(&mut cp, &mut cp2, &args, rel),
                            'q' | 'Q' => self.path_quad_bez_to(&mut cp, &mut cp2, &args, rel),
                            't' | 'T' => self.path_quad_bez_short_to(&mut cp, &mut cp2, &args, rel),
                            'a' | 'A' => {
                                self.path_arc_to(&mut cp, &args, rel);
                                cp2 = cp;
                            }
                            _ => {
                                if args.len() >= 2 {
                                    cp = (args[args.len() - 2], args[args.len() - 1]);
                                    cp2 = cp;
                                }
                            }
                        }
                        args.clear();
                    }
                } else {
                    cmd = item.chars().next().unwrap_or('\0');
                    if cmd == 'M' || cmd == 'm' {
                        // Commit the path, and start a new subpath.
                        if !self.pts.is_empty() {
                            self.add_path(closed_flag);
                        }
                        self.reset_path();
                        closed_flag = false;
                        args.clear();
                    } else if !init_point {
                        // "Do not allow other commands until initial point has been set (moveTo called once)."
                        cmd = '\0';
                    }
                    if cmd == 'Z' || cmd == 'z' {
                        closed_flag = true;
                        // Commit the path: the current point goes back to its first point.
                        if !self.pts.is_empty() {
                            cp = self.pts[0];
                            cp2 = cp;
                            self.add_path(closed_flag);
                        }
                        // Start a new subpath where this one began.
                        self.reset_path();
                        self.move_to(cp.0, cp.1);
                        closed_flag = false;
                        args.clear();
                    }
                    match args_per(cmd) {
                        Some(n) => rargs = n,
                        None => {
                            // Command not recognized
                            cmd = '\0';
                            rargs = 0;
                        }
                    }
                }
            }
            // Commit the path.
            if !self.pts.is_empty() {
                self.add_path(closed_flag);
            }
        }
        self.add_shape();
    }

    /// `nsvg__parseRect`.
    fn parse_rect(&mut self, attrs: &[(&str, &str)]) {
        let (mut x, mut y, mut w, mut h, mut rx, mut ry) = (0.0, 0.0, 0.0, 0.0, -1.0_f64, -1.0_f64);
        for (name, value) in attrs {
            if self.parse_attr(name, value) {
                continue;
            }
            match *name {
                "x" => x = self.coordinate(value, self.view_min_x, self.view_width),
                "y" => y = self.coordinate(value, self.view_min_y, self.view_height),
                "width" => w = self.coordinate(value, 0.0, self.view_width),
                "height" => h = self.coordinate(value, 0.0, self.view_height),
                "rx" => rx = self.coordinate(value, 0.0, self.view_width).abs(),
                "ry" => ry = self.coordinate(value, 0.0, self.view_height).abs(),
                _ => {}
            }
        }
        if rx < 0.0 && ry > 0.0 {
            rx = ry;
        }
        if ry < 0.0 && rx > 0.0 {
            ry = rx;
        }
        rx = rx.max(0.0);
        ry = ry.max(0.0);
        rx = rx.min(w / 2.0);
        ry = ry.min(h / 2.0);
        if w != 0.0 && h != 0.0 {
            self.reset_path();
            if rx < 0.00001 || ry < 0.0001 {
                self.move_to(x, y);
                self.line_to(x + w, y);
                self.line_to(x + w, y + h);
                self.line_to(x, y + h);
            } else {
                // Rounded rectangle
                let k = KAPPA90;
                self.move_to(x + rx, y);
                self.line_to(x + w - rx, y);
                self.cubic_bez_to(x + w - rx * (1.0 - k), y, x + w, y + ry * (1.0 - k), x + w, y + ry);
                self.line_to(x + w, y + h - ry);
                self.cubic_bez_to(x + w, y + h - ry * (1.0 - k), x + w - rx * (1.0 - k), y + h, x + w - rx, y + h);
                self.line_to(x + rx, y + h);
                self.cubic_bez_to(x + rx * (1.0 - k), y + h, x, y + h - ry * (1.0 - k), x, y + h - ry);
                self.line_to(x, y + ry);
                self.cubic_bez_to(x, y + ry * (1.0 - k), x + rx * (1.0 - k), y, x + rx, y);
            }
            self.add_path(true);
            self.add_shape();
        }
    }

    /// `nsvg__parseCircle` and `nsvg__parseEllipse`: four cubic curves.
    fn parse_ellipse(&mut self, attrs: &[(&str, &str)], circle: bool) {
        let (mut cx, mut cy, mut rx, mut ry) = (0.0, 0.0, 0.0, 0.0);
        for (name, value) in attrs {
            if self.parse_attr(name, value) {
                continue;
            }
            match *name {
                "cx" => cx = self.coordinate(value, self.view_min_x, self.view_width),
                "cy" => cy = self.coordinate(value, self.view_min_y, self.view_height),
                "r" if circle => {
                    rx = self.coordinate(value, 0.0, self.actual_length()).abs();
                    ry = rx;
                }
                "rx" if !circle => rx = self.coordinate(value, 0.0, self.view_width).abs(),
                "ry" if !circle => ry = self.coordinate(value, 0.0, self.view_height).abs(),
                _ => {}
            }
        }
        if rx > 0.0 && ry > 0.0 {
            let k = KAPPA90;
            self.reset_path();
            self.move_to(cx + rx, cy);
            self.cubic_bez_to(cx + rx, cy + ry * k, cx + rx * k, cy + ry, cx, cy + ry);
            self.cubic_bez_to(cx - rx * k, cy + ry, cx - rx, cy + ry * k, cx - rx, cy);
            self.cubic_bez_to(cx - rx, cy - ry * k, cx - rx * k, cy - ry, cx, cy - ry);
            self.cubic_bez_to(cx + rx * k, cy - ry, cx + rx, cy - ry * k, cx + rx, cy);
            self.add_path(true);
            self.add_shape();
        }
    }

    /// `nsvg__parseLine`.
    fn parse_line(&mut self, attrs: &[(&str, &str)]) {
        let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for (name, value) in attrs {
            if self.parse_attr(name, value) {
                continue;
            }
            match *name {
                "x1" => x1 = self.coordinate(value, self.view_min_x, self.view_width),
                "y1" => y1 = self.coordinate(value, self.view_min_y, self.view_height),
                "x2" => x2 = self.coordinate(value, self.view_min_x, self.view_width),
                "y2" => y2 = self.coordinate(value, self.view_min_y, self.view_height),
                _ => {}
            }
        }
        self.reset_path();
        self.move_to(x1, y1);
        self.line_to(x2, y2);
        self.add_path(false);
        self.add_shape();
    }

    /// `nsvg__parsePoly`: `polyline` (open) or `polygon` (closed).
    fn parse_poly(&mut self, attrs: &[(&str, &str)], close: bool) {
        let mut points: Option<&str> = None;
        for (name, value) in attrs {
            if *name == "points" {
                points = Some(value);
            } else {
                self.parse_attr(name, value);
            }
        }
        self.reset_path();
        if let Some(mut s) = points {
            let mut args: Vec<f64> = Vec::new();
            let mut npts = 0;
            while let Some((item, rest)) = next_item(s) {
                s = rest;
                args.push(item.parse::<f64>().unwrap_or(0.0));
                if args.len() >= 2 {
                    if npts == 0 {
                        self.move_to(args[0], args[1]);
                    } else {
                        self.line_to(args[0], args[1]);
                    }
                    args.clear();
                    npts += 1;
                }
            }
        }
        self.add_path(close);
        self.add_shape();
    }

    /// `nsvg__parseSVG`: the size, the view box and how the picture sits in it.
    fn parse_svg(&mut self, attrs: &[(&str, &str)]) {
        for (name, value) in attrs {
            if self.parse_attr(name, value) {
                continue;
            }
            match *name {
                "width" => self.width = self.coordinate(value, 0.0, 1.0),
                "height" => self.height = self.coordinate(value, 0.0, 1.0),
                "viewBox" => {
                    let nums: Vec<f64> = value.split(|c: char| is_space(c) || c == ',' || c == '%').filter(|t| !t.is_empty()).filter_map(|t| t.parse::<f64>().ok()).collect();
                    if nums.len() >= 4 {
                        self.view_min_x = nums[0];
                        self.view_min_y = nums[1];
                        self.view_width = nums[2];
                        self.view_height = nums[3];
                    }
                }
                "preserveAspectRatio" => self.parse_aspect_ratio(value),
                _ => {}
            }
        }
    }

    fn parse_aspect_ratio(&mut self, value: &str) {
        let s = value.trim_start();
        if s.starts_with("none") {
            // No uniform scaling.
            self.align_type = AlignType::None;
            return;
        }
        let ax = |t: &str| match t {
            "xMin" => Some(Align::Min),
            "xMid" => Some(Align::Mid),
            "xMax" => Some(Align::Max),
            _ => None,
        };
        let ay = |t: &str| match t {
            "YMin" => Some(Align::Min),
            "YMid" => Some(Align::Mid),
            "YMax" => Some(Align::Max),
            _ => None,
        };
        if s.len() >= 8 {
            if let (Some(x), Some(y)) = (ax(&s[..4]), ay(&s[4..8])) {
                self.align_x = x;
                self.align_y = y;
            }
        }
        // Meet or slice: the word after the alignment.
        let rest = s.split_whitespace().nth(1).unwrap_or("");
        self.align_type = if rest.starts_with("slice") { AlignType::Slice } else { AlignType::Meet };
    }

    // ------------------------------------------------------------------------------------------------------------------- elements

    /// `nsvg__startElement`.
    fn start_element(&mut self, el: &str, attrs: &[(&str, &str)]) {
        if self.defs {
            // Skip everything but gradients in defs.
            if el == "linearGradient" || el == "radialGradient" {
                self.note_gradient(attrs);
            }
            return;
        }
        match el {
            "g" => {
                self.push_attr();
                self.parse_attribs(attrs);
            }
            "path" => {
                self.push_attr();
                self.parse_path(attrs);
                self.pop_attr();
            }
            "rect" => {
                self.push_attr();
                self.parse_rect(attrs);
                self.pop_attr();
            }
            "circle" | "ellipse" => {
                self.push_attr();
                self.parse_ellipse(attrs, el == "circle");
                self.pop_attr();
            }
            "line" => {
                self.push_attr();
                self.parse_line(attrs);
                self.pop_attr();
            }
            "polyline" | "polygon" => {
                self.push_attr();
                self.parse_poly(attrs, el == "polygon");
                self.pop_attr();
            }
            "linearGradient" | "radialGradient" => self.note_gradient(attrs),
            "defs" => self.defs = true,
            "svg" => self.parse_svg(attrs),
            "text" | "image" | "use" => {
                let msg = format!("SVG {el} elements are not currently supported.");
                if !self.messages.contains(&msg) {
                    self.messages.push(msg);
                }
            }
            _ => {}
        }
    }

    /// A gradient is known by its `id` from here on (`nsvg__parseGradient`).
    fn note_gradient(&mut self, attrs: &[(&str, &str)]) {
        if let Some((_, id)) = attrs.iter().find(|(n, _)| *n == "id") {
            self.gradients.push((*id).to_string());
        }
    }

    /// `nsvg__endElement`.
    fn end_element(&mut self, el: &str) {
        // The start of what is skipped inside <defs> pushed nothing, so its end pops nothing.
        if self.defs && el != "defs" {
            return;
        }
        match el {
            "g" => self.pop_attr(),
            "defs" => self.defs = false,
            _ => {}
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- bounds

    /// `nsvg__curveBounds` over every path of every shape: where the picture is, when the file says nothing of it.
    fn image_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut grow = |x: f64, y: f64| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for s in &self.shapes {
            for p in &s.paths {
                let mut i = 0;
                while i + 3 < p.pts.len() {
                    let c = [p.pts[i], p.pts[i + 1], p.pts[i + 2], p.pts[i + 3]];
                    let (bx0, by0, bx1, by1) = curve_bounds(&c);
                    grow(bx0, by0);
                    grow(bx1, by1);
                    i += 3;
                }
            }
        }
        b
    }

    /// `nsvg__scaleToViewbox( p, "mm" )`: every point and width of every shape in millimetres.
    fn scale_to_viewbox(&mut self) {
        let bounds = self.image_bounds().unwrap_or((0.0, 0.0, 0.0, 0.0));
        if self.view_width == 0.0 {
            if self.width > 0.0 {
                self.view_width = self.width;
            } else {
                self.view_min_x = bounds.0;
                self.view_width = bounds.2 - bounds.0;
            }
        }
        if self.view_height == 0.0 {
            if self.height > 0.0 {
                self.view_height = self.height;
            } else {
                self.view_min_y = bounds.1;
                self.view_height = bounds.3 - bounds.1;
            }
        }
        if self.width == 0.0 {
            self.width = self.view_width;
        }
        if self.height == 0.0 {
            self.height = self.view_height;
        }
        let mut tx = -self.view_min_x;
        let mut ty = -self.view_min_y;
        let mut sx = if self.view_width > 0.0 { self.width / self.view_width } else { 0.0 };
        let mut sy = if self.view_height > 0.0 { self.height / self.view_height } else { 0.0 };
        // Unit scaling: a pixel in millimetres.
        let us = 1.0 / (1.0 / 25.4 * SVG_DPI);
        let view_align = |content: f64, container: f64, a: Align| match a {
            Align::Min => 0.0,
            Align::Max => container - content,
            Align::Mid => (container - content) * 0.5,
        };
        // Fix aspect ratio
        if self.align_type == AlignType::Meet {
            sx = sx.min(sy);
            sy = sx;
            tx += view_align(self.view_width * sx, self.width, self.align_x) / sx;
            ty += view_align(self.view_height * sy, self.height, self.align_y) / sy;
        } else if self.align_type == AlignType::Slice {
            sx = sx.max(sy);
            sy = sx;
            tx += view_align(self.view_width * sx, self.width, self.align_x) / sx;
            ty += view_align(self.view_height * sy, self.height, self.align_y) / sy;
        }
        sx *= us;
        sy *= us;
        let avgs = (sx + sy) / 2.0;
        for s in &mut self.shapes {
            for p in &mut s.paths {
                for pt in &mut p.pts {
                    pt.0 = (pt.0 + tx) * sx;
                    pt.1 = (pt.1 + ty) * sy;
                }
            }
            s.stroke_width *= avgs;
            for d in &mut s.dash {
                *d *= avgs;
            }
        }
    }
}

/// `nsvg__parseUrl`: the id in `url(#id)`.
fn parse_url(s: &str) -> String {
    let inner = s.strip_prefix("url(").unwrap_or(s);
    let inner = inner.split(')').next().unwrap_or("").trim();
    inner.trim_start_matches('#').to_string()
}

/// A point of a cubic curve.
fn cubic_at(c: &[(f64, f64); 4], t: f64) -> (f64, f64) {
    let (t2, t3, mt) = (t * t, t * t * t, 1.0 - t);
    let (mt2, mt3) = (mt * mt, mt * mt * mt);
    (mt3 * c[0].0 + 3.0 * mt2 * t * c[1].0 + 3.0 * mt * t2 * c[2].0 + t3 * c[3].0, mt3 * c[0].1 + 3.0 * mt2 * t * c[1].1 + 3.0 * mt * t2 * c[2].1 + t3 * c[3].1)
}

/// `nsvg__curveBounds`: the box of a cubic curve, its extremes included.
fn curve_bounds(c: &[(f64, f64); 4]) -> (f64, f64, f64, f64) {
    let mut b = (c[0].0.min(c[3].0), c[0].1.min(c[3].1), c[0].0.max(c[3].0), c[0].1.max(c[3].1));
    let inside = |p: (f64, f64), b: &(f64, f64, f64, f64)| p.0 >= b.0 && p.0 <= b.2 && p.1 >= b.1 && p.1 <= b.3;
    if inside(c[1], &b) && inside(c[2], &b) {
        return b;
    }
    const EPS: f64 = 1e-12;
    for axis in 0..2 {
        let v = |p: (f64, f64)| if axis == 0 { p.0 } else { p.1 };
        let (v0, v1, v2, v3) = (v(c[0]), v(c[1]), v(c[2]), v(c[3]));
        let a = -3.0 * v0 + 9.0 * v1 - 9.0 * v2 + 3.0 * v3;
        let bb = 6.0 * v0 - 12.0 * v1 + 6.0 * v2;
        let cc = 3.0 * v1 - 3.0 * v0;
        let mut roots: Vec<f64> = Vec::new();
        if a.abs() < EPS {
            if bb.abs() > EPS {
                roots.push(-cc / bb);
            }
        } else {
            let b2ac = bb * bb - 4.0 * cc * a;
            if b2ac > EPS {
                roots.push((-bb + b2ac.sqrt()) / (2.0 * a));
                roots.push((-bb - b2ac.sqrt()) / (2.0 * a));
            }
        }
        for t in roots.into_iter().filter(|t| *t > EPS && *t < 1.0 - EPS) {
            let p = cubic_at(c, t);
            let val = v(p);
            if axis == 0 {
                b.0 = b.0.min(val);
                b.2 = b.2.max(val);
            } else {
                b.1 = b.1.min(val);
                b.3 = b.3.max(val);
            }
        }
    }
    b
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_in_every_form_nanosvg_reads() {
        assert_eq!(parse_color("#fa0"), (255, 170, 0));
        assert_eq!(parse_color("#00FF7f"), (0, 255, 127));
        assert_eq!(parse_color("rgb(10%, 20%, 100%)"), (25, 51, 255));
        assert_eq!(parse_color("Navy"), (0, 0, 128));
        assert_eq!(parse_color("notacolour"), (128, 128, 128));
    }
}
