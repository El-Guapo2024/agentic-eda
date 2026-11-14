//! A board text's effective shape as KiCad's DRC sees it: the stroke font's glyph segments, to the nanometre (`EDA_TEXT::GetEffectiveTextShape`, the stroke
//! callback of `CALLBACK_GAL::DrawGlyph`: one `SHAPE_SEGMENT( a, b, penWidth )` per pair of consecutive points of each glyph stroke).
//!
//! KiCad 25ec2335: `common/font/stroke_font.cpp` (`loadNewStrokeFont`, `buildGlyphBoundingBox`, `GetTextAsGlyphs`), `common/font/glyph.cpp::STROKE_GLYPH::Transform`,
//! `common/font/font.cpp` (`Draw`, `getLinePositions`, `drawSingleLineText`, `boundingBoxSingleLine`), `common/callback_gal.cpp::DrawGlyph`, `common/eda_text.cpp`
//! (`GetEffectiveTextShape`, `GetEffectiveTextPenWidth`), `common/gr_text.cpp` (`ClampTextPenSize`, `GetPenSizeForBold`, `GetPenSizeForNormal`),
//! `pcbnew/pcb_text.cpp::GetDrawRotation`, `libs/kimath/src/trigo.cpp::RotatePoint( double*, double*, .. )`, `libs/kimath/include/math/vector2d.h` (the
//! `VECTOR2I( VECTOR2D )` conversion truncates). The glyph data is `common/newstroke_font.cpp`'s, as `web/studio/src/kicad/strokeFont.json` holds it (the one copy
//! the studio's font code reads).
//!
//! Ported too: several lines (`getLinePositions`' interline, `METRICS::m_InterlinePitch` 1.68 times the stroke font's legacy 0.9583, the line spacing), tabs
//! (`GetTextAsGlyphs`' four-column stops), markup (`MARKUP_PARSER`'s grammar; `drawMarkup`: a subscript or superscript at `m_superSubSize` 0.8 of the size and
//! 0.15 / -0.35 of it off the baseline, an overbar's bar at `m_OverbarHeight` 1.23 above it), text variables ([`expand_vars`], `PCB_TEXT::GetShownText`'s
//! resolver) and a knockout text's box ([`knockout_box`]).
//!
//! An outline font (UI-LEFT-3 C3-3): a text whose face is not the stroke font is laid out by the same `FONT` code with `OUTLINE_FONT`'s glyphs
//! ([`crate::outline_font`]): `GetInterline` its own, no stroke-font fudge on the first line's offset, no italic tilt (fontconfig picks the italic face, or
//! fakes it), each run's glyphs the face's polygons ([`glyph_polys`]); an overbar is still a stroke (`STROKE_GLYPH`) as wide as the pen ([`segments`]).
//! Moved here from `eda-kicad` (which re-exports it as `stroke_text`) so the DRC, connectivity and model crates measure texts the same way.

use crate::outline_font::{BBox, GlyphPoly, OutlineFont, RunStyle};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// `STROKE_FONT_SCALE`.
const SCALE: f64 = 1.0 / 21.0;
/// `FONT_OFFSET`.
const FONT_OFFSET: i64 = -8;
/// `ITALIC_TILT`.
const ITALIC_TILT: f64 = 1.0 / 8.0;
/// `BOLD_STROKE_MULTIPLIER`.
const BOLD_STROKE_MULTIPLIER: f64 = 1.6;

struct Glyph {
    /// `glyphWidth`, the bounding box's right end (`BoundingBox().GetEnd().x`).
    width: f64,
    strokes: Vec<Vec<[f64; 2]>>,
}

fn font() -> &'static HashMap<u32, Glyph> {
    static FONT: OnceLock<HashMap<u32, Glyph>> = OnceLock::new();
    FONT.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct Data {
            glyphs: HashMap<String, String>,
        }
        let data: Data = serde_json::from_str(include_str!("../../../web/studio/src/kicad/strokeFont.json")).unwrap_or(Data { glyphs: HashMap::new() });
        data.glyphs.iter().filter_map(|(k, raw)| Some((k.parse::<u32>().ok()?, decode(raw)))).collect()
    })
}

/// `loadNewStrokeFont`'s loop over one glyph's text.
fn decode(raw: &str) -> Glyph {
    let c: Vec<i64> = raw.bytes().map(|b| b as i64).collect();
    let r = b'R' as i64;
    if c.len() < 2 {
        return Glyph { width: 0.0, strokes: Vec::new() };
    }
    let start_x = (c[0] - r) as f64 * SCALE;
    let end_x = (c[1] - r) as f64 * SCALE;
    let mut strokes: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut pen_down = false;
    let mut i = 2;
    while i + 1 < c.len() {
        if c[i] == b' ' as i64 && c[i + 1] == r {
            pen_down = false;
        } else {
            if !pen_down {
                strokes.push(Vec::new());
                pen_down = true;
            }
            let x = (c[i] - r) as f64 * SCALE - start_x;
            let y = (c[i + 1] - r + FONT_OFFSET) as f64 * SCALE;
            if let Some(s) = strokes.last_mut() {
                s.push([x, y]);
            }
        }
        i += 2;
    }
    Glyph { width: end_x - start_x, strokes }
}

/// The glyph of a character, `?` for one the font does not have (`dd < 0 || dd >= size`).
fn glyph(ch: char) -> Option<&'static Glyph> {
    let f = font();
    f.get(&(ch as u32)).or_else(|| f.get(&('?' as u32)))
}

/// Horizontal and vertical justification: `-1` left / top, `0` centre, `1` right / bottom.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextShape {
    /// The shown text (`GetShownText`: its variables already expanded, [`expand_vars`]).
    pub text: String,
    /// `GetDrawPos`: the text's position on the board.
    pub pos: [i64; 2],
    /// `GetTextSize`: width, height.
    pub size: [i64; 2],
    /// `GetTextThickness`, the attribute (`m_StrokeWidth`).
    pub thickness: i64,
    /// `GetDrawRotation`, degrees.
    pub angle: f64,
    pub mirror: bool,
    pub italic: bool,
    pub bold: bool,
    pub h_align: i8,
    pub v_align: i8,
    /// `m_LineSpacing` (1.0 unless the file says `line_spacing`).
    pub line_spacing: f64,
    /// `GetFont()`'s face: none or "KiCad Font..." is the stroke font, any other an outline font ([`font_of`]).
    pub face: Option<String>,
}

/// The font a text is drawn in (`EDA_TEXT::GetDrawFont`): the stroke font, or the outline font `FONT::GetFont( face, bold, italic )` loads.
#[derive(Debug, Clone)]
pub enum Font {
    Stroke,
    Outline(Arc<OutlineFont>),
}

/// `EDA_TEXT::GetDrawFont`: the text's face, bold and italic, or the stroke font when it has none or fontconfig finds no file.
pub fn font_of(t: &TextShape) -> Font {
    match OutlineFont::load(t.face.as_deref(), t.bold, t.italic) {
        Some(f) => Font::Outline(f),
        None => Font::Stroke,
    }
}

/// `PCB_TEXT::GetDrawRotation`: a footprint's text kept upright in `]-90, 90]`, any other normalized.
pub fn draw_rotation(angle: f64, keep_upright_in_footprint: bool) -> f64 {
    let mut a = angle;
    if keep_upright_in_footprint {
        while a > 90.0 {
            a -= 180.0;
        }
        while a <= -90.0 {
            a += 180.0;
        }
        a
    } else {
        normalize(a)
    }
}

fn normalize(mut a: f64) -> f64 {
    a %= 720.0;
    while a < -0.0 {
        a += 360.0;
    }
    while a >= 360.0 {
        a -= 360.0;
    }
    a
}

/// `EDA_TEXT::GetEffectiveTextPenWidth( 0 )`.
pub fn pen_width(t: &TextShape) -> i64 {
    let kiround = |v: f64| v.round() as i64;
    let mut pen = t.thickness;
    if pen <= 1 {
        pen = if t.bold { kiround(t.size[0] as f64 / 5.0) } else { kiround(t.size[0] as f64 / 8.0) };
    } else if t.bold {
        pen = kiround(pen as f64 * BOLD_STROKE_MULTIPLIER);
    }
    // `ClampTextPenSize( penWidth, GetTextSize() )`
    pen.min(kiround(t.size[0].abs().min(t.size[1].abs()) as f64 * 0.25))
}

/// `RotatePoint( VECTOR2D&, aCentre, angle )` in doubles, `EDA_ANGLE::Sin`/`Cos` exact at the multiples of 45 degrees.
fn rotate(p: [f64; 2], c: [f64; 2], degrees: f64) -> [f64; 2] {
    let a = normalize(degrees);
    let (ox, oy) = (p[0] - c[0], p[1] - c[1]);
    let (x, y) = if a == 0.0 {
        (ox, oy)
    } else if a == 90.0 {
        (oy, -ox)
    } else if a == 180.0 {
        (-ox, -oy)
    } else if a == 270.0 {
        (-oy, ox)
    } else {
        let (s, co) = sin_cos(a);
        (oy * s + ox * co, oy * co - ox * s)
    };
    [x + c[0], y + c[1]]
}

fn sin_cos(a: f64) -> (f64, f64) {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    match a {
        v if v == 45.0 => (h, h),
        v if v == 135.0 => (h, -h),
        v if v == 225.0 => (-h, -h),
        v if v == 315.0 => (-h, h),
        v => v.to_radians().sin_cos(),
    }
}

/// `RotatePoint( VECTOR2I&, aCentre, angle )` on integers (`KiROUND` of the turned offset).
fn rotate_int(p: [i64; 2], c: [i64; 2], degrees: f64) -> [i64; 2] {
    let a = normalize(degrees);
    let (x, y) = (p[0] - c[0], p[1] - c[1]);
    let (rx, ry) = if a == 0.0 {
        (x, y)
    } else if a == 90.0 {
        (y, -x)
    } else if a == 180.0 {
        (-x, -y)
    } else if a == 270.0 {
        (-y, x)
    } else {
        let (s, co) = sin_cos(a);
        (((y as f64) * s + (x as f64) * co).round() as i64, ((y as f64) * co - (x as f64) * s).round() as i64)
    };
    [rx + c[0], ry + c[1]]
}

// ------------------------------------------------------------------------------------------------------------------------------------ markup

/// `MARKUP::NODE`: a run of text, or a subscript, superscript or overbar holding its own nodes.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Text(String),
    Mark(char, Vec<Node>),
}

/// The `markup` rule at `i`: `^{`, `_{` or `~{` and its nodes up to the matching `}` (`braces`: `until< '}', sor< anyStringWithinBraces, markup > >`);
/// `None` when it does not close, and the characters stay text (`anyString`).
fn parse_mark(c: &[char], i: usize) -> Option<(Node, usize)> {
    if i + 1 >= c.len() || !matches!(c[i], '^' | '_' | '~') || c[i + 1] != '{' {
        return None;
    }
    let mut j = i + 2;
    let mut nodes = Vec::new();
    let mut run = String::new();
    loop {
        if j >= c.len() {
            return None;
        }
        if c[j] == '}' {
            if !run.is_empty() {
                nodes.push(Node::Text(std::mem::take(&mut run)));
            }
            return Some((Node::Mark(c[i], nodes), j + 1));
        }
        if let Some((n, next)) = parse_mark(c, j) {
            if !run.is_empty() {
                nodes.push(Node::Text(std::mem::take(&mut run)));
            }
            nodes.push(n);
            j = next;
            continue;
        }
        // `escapeSequence` (`{identifier}`) is text within braces too
        if c[j] == '{' {
            if let Some(k) = (j + 1..c.len()).find(|&k| c[k] == '}') {
                if k > j + 1 && c[j + 1..k].iter().all(|ch| ch.is_ascii_alphanumeric() || *ch == '_') && !c[j + 1].is_ascii_digit() {
                    run.extend(&c[j..=k]);
                    j = k + 1;
                    continue;
                }
            }
        }
        run.push(c[j]);
        j += 1;
    }
}

/// `MARKUP_PARSER::Parse`: the line's nodes; an empty mark is dropped (`discard_empty`).
fn parse_markup(text: &str) -> Vec<Node> {
    let c: Vec<char> = text.chars().collect();
    let mut nodes = Vec::new();
    let mut run = String::new();
    let mut i = 0;
    while i < c.len() {
        if let Some((n, next)) = parse_mark(&c, i) {
            if !run.is_empty() {
                nodes.push(Node::Text(std::mem::take(&mut run)));
            }
            nodes.push(n);
            i = next;
        } else {
            run.push(c[i]);
            i += 1;
        }
    }
    if !run.is_empty() {
        nodes.push(Node::Text(run));
    }
    fn prune(n: Vec<Node>) -> Vec<Node> {
        n.into_iter()
            .filter_map(|x| match x {
                Node::Mark(k, ch) => {
                    let ch = prune(ch);
                    (!ch.is_empty()).then_some(Node::Mark(k, ch))
                }
                t => Some(t),
            })
            .collect()
    }
    prune(nodes)
}

const SUB: u8 = 1;
const SUPER: u8 = 2;

/// What one pass over a line lays down: the glyph strokes (each a list of points, in doubles) of `STROKE_GLYPH::Transform`, an outline font's glyph polygons.
struct Layout<'a> {
    t: &'a TextShape,
    font: Font,
    origin: [f64; 2],
    tilt: f64,
    strokes: Vec<Vec<[f64; 2]>>,
    polys: Vec<GlyphPoly>,
    /// `drawMarkup`'s `aBoundingBox`: the runs' boxes merged, when asked for.
    bbox: Option<BBox>,
}

impl Layout<'_> {
    fn place(&self, p: [f64; 2]) -> [f64; 2] {
        let mut q = p;
        if self.t.mirror {
            q[0] = self.origin[0] - (q[0] - self.origin[0]);
        }
        if self.t.angle != 0.0 {
            q = rotate(q, self.origin, self.t.angle);
        }
        q
    }

    /// `FONT::GetTextAsGlyphs` of the text's font: the run's glyphs from `pos`, the cursor's end.
    fn run(&mut self, text: &str, pos: [i64; 2], style: u8, draw: bool) -> Option<[i64; 2]> {
        if let Font::Outline(f) = &self.font {
            let f = f.clone();
            let st = RunStyle { sub: style & SUB != 0, sup: style & SUPER != 0 };
            let origin = [self.origin[0] as i64, self.origin[1] as i64];
            let sink = if draw { Some(&mut self.polys) } else { None };
            let mut b = BBox::default();
            let end = f.text_as_glyphs(Some(&mut b), sink, text, self.t.size, pos, self.t.angle, self.t.mirror, origin, st);
            if let Some(acc) = self.bbox.as_mut() {
                acc.merge(&b);
            }
            return Some(end);
        }
        self.stroke_run(text, pos, style, draw)
    }

    /// `STROKE_FONT::GetTextAsGlyphs`: the run's glyphs from `pos`, the cursor's end (`VECTOR2I( cursor.x, aPosition.y )`).
    fn stroke_run(&mut self, text: &str, pos: [i64; 2], style: u8, draw: bool) -> Option<[i64; 2]> {
        let kiround = |v: f64| v.round() as i64;
        let size = [self.t.size[0] as f64, self.t.size[1] as f64];
        let mut glyph_size = size;
        let mut cursor = pos;
        if style & (SUB | SUPER) != 0 {
            glyph_size = [glyph_size[0] * 0.8, glyph_size[1] * 0.8];
            let offset = if style & SUB != 0 { 0.15 } else { -0.35 };
            cursor[1] = (cursor[1] as f64 + glyph_size[1] * offset) as i64;
        }
        let space_width = glyph(' ')?.width;
        let mut char_count: i64 = 0;
        for ch in text.chars() {
            if ch == '\t' {
                char_count = (char_count / 4 + 1) * 4 - 1;
                let mut new_cursor = (pos[0] as f64 + (self.t.size[0] * char_count) as f64 + size[0] * space_width) as i64;
                while new_cursor <= cursor[0] {
                    char_count += 4;
                    new_cursor += self.t.size[0] * 4;
                }
                cursor[0] = new_cursor;
            } else if ch == ' ' {
                cursor[0] += kiround(glyph_size[0] * space_width);
            } else {
                let g = glyph(ch)?;
                if draw {
                    for stroke in &g.strokes {
                        let pts = stroke
                            .iter()
                            .map(|p| {
                                let mut q = [p[0] * glyph_size[0], p[1] * glyph_size[1]];
                                if self.tilt != 0.0 {
                                    q[0] -= q[1] * self.tilt;
                                }
                                self.place([q[0] + cursor[0] as f64, q[1] + cursor[1] as f64])
                            })
                            .collect();
                        self.strokes.push(pts);
                    }
                }
                cursor[0] += kiround(g.width * glyph_size[0]);
            }
            char_count += 1;
        }
        if let Some(acc) = self.bbox.as_mut() {
            // `aBBox->SetOrigin( aPosition ); aBBox->SetEnd( cursor.x - KiROUND( glyphSize.x * INTER_CHAR ), cursor.y - glyphSize.y ); Normalize()`
            let mut b = BBox::at(pos);
            b.merge_point([cursor[0] - kiround(glyph_size[0] * 0.2), (cursor[1] as f64 - glyph_size[1]) as i64]);
            acc.merge(&b);
        }
        Some([cursor[0], pos[1]])
    }

    /// `drawMarkup`: each node at the cursor, an overbar's bar from where it began to where it ended, `GetOverbarVerticalPosition` above the baseline and
    /// `0.1` of the size in from each end.
    fn nodes(&mut self, nodes: &[Node], pos: [i64; 2], style: u8, draw: bool) -> Option<[i64; 2]> {
        let mut next = pos;
        for n in nodes {
            match n {
                Node::Text(s) => next = self.run(s, next, style, draw)?,
                Node::Mark(kind, children) => {
                    let st = match kind {
                        '_' => style | SUB,
                        '^' => style | SUPER,
                        _ => style,
                    };
                    let start = next;
                    next = self.nodes(children, next, st, draw)?;
                    if *kind == '~' && draw {
                        let trim = self.t.size[0] as f64 * 0.1;
                        let offset = self.t.size[1] as f64 * 1.23;
                        let a = [start[0] as f64 + trim, start[1] as f64 - offset];
                        let b = [next[0] as f64 - trim, next[1] as f64 - offset];
                        let (a, b) = (self.place(a), self.place(b));
                        self.strokes.push(vec![a, b]);
                    }
                }
            }
        }
        Some(next)
    }
}

/// `FONT::GetInterline( aGlyphHeight )` of the text's font: the stroke font's 1.68 heights times its legacy 0.9583, an outline font's
/// [`OutlineFont::interline`].
pub fn font_interline(font: &Font, glyph_height: f64) -> f64 {
    match font {
        Font::Stroke => glyph_height * 1.68 * 0.9583,
        Font::Outline(f) => f.interline(glyph_height),
    }
}

/// `FONT::getLinePositions`: each line's start, and the lines' parsed nodes.
fn line_positions(t: &TextShape, font: &Font) -> Option<Vec<([i64; 2], Vec<Node>)>> {
    let lines: Vec<Vec<Node>> = t.text.split('\n').map(parse_markup).collect();
    let interline = (font_interline(font, t.size[1] as f64) * t.line_spacing) as i64;
    let mut height: i64 = 0;
    let mut extents = Vec::new();
    let mut probe = Layout { t, font: font.clone(), origin: [0.0, 0.0], tilt: 0.0, strokes: Vec::new(), polys: Vec::new(), bbox: None };
    for (i, nodes) in lines.iter().enumerate() {
        let pos = [t.pos[0], t.pos[1] + i as i64 * interline];
        let end = probe.nodes(nodes, pos, 0, false)?;
        extents.push(end[0] - pos[0]);
        height = if i == 0 { (height as f64 + t.size[1] as f64 * 1.17) as i64 } else { height + interline };
    }
    let mut offset = [0i64, t.size[1]];
    if matches!(font, Font::Stroke) {
        // "Fudge factors to match 6.0 positioning"
        offset[0] = (offset[0] as f64 + t.thickness as f64 / 1.52) as i64;
        offset[1] = (offset[1] as f64 - t.thickness as f64 * 0.052) as i64;
    }
    match t.v_align {
        0 => offset[1] -= height / 2,
        1 => offset[1] -= height,
        _ => {}
    }
    Some(
        lines
            .into_iter()
            .enumerate()
            .map(|(i, nodes)| {
                let mut lo = offset;
                lo[1] += i as i64 * interline;
                match t.h_align {
                    0 => lo[0] = -extents[i] / 2,
                    1 => lo[0] = -(extents[i] + offset[0]),
                    _ => {}
                }
                ([t.pos[0] + lo[0], t.pos[1] + lo[1]], nodes)
            })
            .collect(),
    )
}

/// The segments of a text's shape (`GetEffectiveTextShape` through `FONT::Draw`), each `[a, b]` in nanometres, as wide as [`pen_width`]: every line, its
/// tabs, its markup (a subscript or superscript at 0.8 of the size off the baseline, an overbar's bar). `None` for a character the font cannot give.
/// For an outline font these are only its overbars ([`shape`] has the glyphs too).
pub fn segments(t: &TextShape) -> Option<Vec<[[i64; 2]; 2]>> {
    shape(t).map(|s| s.segments)
}

/// The whole shape of a text (`GetEffectiveTextShape` / `TransformTextToPolySet`): the stroke segments, as wide as [`pen_width`] (every glyph of the stroke
/// font; an outline font's overbars), and an outline font's glyph polygons.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    pub segments: Vec<[[i64; 2]; 2]>,
    pub polys: Vec<GlyphPoly>,
    pub pen: i64,
    /// Whether the text is in an outline font.
    pub outline: bool,
}

/// [`Shape`] of a text: `None` for an empty one or a character the stroke font cannot give.
pub fn shape(t: &TextShape) -> Option<Shape> {
    shape_in(t, font_of(t))
}

/// [`shape`] in a given font.
pub fn shape_in(t: &TextShape, font: Font) -> Option<Shape> {
    if t.text.is_empty() {
        return None;
    }
    let outline = matches!(font, Font::Outline(_));
    let tilt = if t.italic && !outline { ITALIC_TILT } else { 0.0 };
    let lines = line_positions(t, &font)?;
    let mut layout = Layout { t, font, origin: [t.pos[0] as f64, t.pos[1] as f64], tilt, strokes: Vec::new(), polys: Vec::new(), bbox: None };
    for (pos, nodes) in lines {
        layout.nodes(&nodes, pos, 0, true)?;
    }
    // the stroke callback takes `VECTOR2I`: a cast, which truncates
    let trunc = |p: [f64; 2]| [p[0] as i64, p[1] as i64];
    let segments = layout.strokes.iter().flat_map(|s| s.windows(2).map(|w| [trunc(w[0]), trunc(w[1])]).collect::<Vec<_>>()).collect();
    Some(Shape { segments, polys: layout.polys, pen: pen_width(t), outline })
}

/// An outline font's glyph polygons of a text (none for the stroke font).
pub fn glyph_polys(t: &TextShape) -> Vec<GlyphPoly> {
    shape(t).map(|s| s.polys).unwrap_or_default()
}

/// `FONT::StringBoundaryLimits( text, size, thickness, bold, italic )` of one line in `font`: the size of the union of its runs' boxes, a stroke font's grown
/// by 1.5 pens on each side ("to catch diacriticals, descenders, etc."), an outline font's not ("Outline fonts have thickness built in").
pub fn string_boundary_limits(text: &str, size: [i64; 2], thickness: i64, italic: bool, font: &Font) -> [i64; 2] {
    let t = TextShape { text: text.to_string(), size, thickness, italic, line_spacing: 1.0, ..Default::default() };
    let mut layout = Layout { t: &t, font: font.clone(), origin: [0.0, 0.0], tilt: 0.0, strokes: Vec::new(), polys: Vec::new(), bbox: None };
    let mut bbox = BBox::default();
    layout.bbox = Some(BBox::default());
    if layout.nodes(&parse_markup(text), [0, 0], 0, false).is_some() {
        bbox = layout.bbox.unwrap_or_default();
    }
    let mut s = bbox.size();
    if matches!(font, Font::Stroke) {
        let k = (thickness as f64 * 1.5).round() as i64;
        s = [s[0] + 2 * k, s[1] + 2 * k];
    }
    s
}

/// `FONT::boundingBoxSingleLine`'s extent of one line in the text's font: where `drawMarkup` leaves the cursor, from 0.
pub fn line_advance(t: &TextShape) -> i64 {
    let font = font_of(t);
    let mut layout = Layout { t, font, origin: [0.0, 0.0], tilt: 0.0, strokes: Vec::new(), polys: Vec::new(), bbox: None };
    layout.nodes(&parse_markup(&t.text), [0, 0], 0, false).map_or(0, |e| e[0])
}

/// `EDA_TEXT::GetTextBox( nullptr, -1 )` (`common/eda_text.cpp`) of a text in `font`, unrotated, at its position: `[x0, y0, x1, y1]`. The first line's
/// `StringBoundaryLimits` (each line's for the width when `multiline`), the stroke font's height grown by `KiROUND( extents.y * 0.17 )`, each further line an
/// interline more, an overbar's `extents.y / 6` more; placed by the justification, an italic's `KiROUND( size.y * ITALIC_TILT )` off its width, the
/// vertical fudge (`fudgeFactor`, whatever the font) moving a top or bottom one.
pub fn text_box(t: &TextShape, font: &Font, multiline: bool) -> [i64; 4] {
    let kiround = |v: f64| v.round() as i64;
    let lines: Vec<&str> = if multiline { t.text.split('\n').collect() } else { vec![t.text.as_str()] };
    let thickness = pen_width(t);
    let first = lines.first().copied().unwrap_or("");
    let extents = string_boundary_limits(first, t.size, thickness, t.italic, font);
    let mut size = extents;
    let fudge = kiround(extents[1] as f64 * 0.17);
    if matches!(font, Font::Stroke) {
        size[1] += fudge;
    }
    if multiline && lines.len() > 1 {
        for l in &lines[1..] {
            size[0] = size[0].max(string_boundary_limits(l, t.size, thickness, t.italic, font)[0]);
        }
        size[1] += kiround((lines.len() - 1) as f64 * font_interline(font, t.size[1] as f64));
    }
    if t.text.contains("~{") {
        size[1] += extents[1] / 6;
    }
    let italic_offset = if t.italic { kiround(t.size[1] as f64 * ITALIC_TILT) } else { 0 };
    let mut x = t.pos[0];
    match t.h_align {
        -1 if t.mirror => x -= size[0] - italic_offset,
        0 => x -= (size[0] - italic_offset) / 2,
        1 if !t.mirror => x -= size[0] - italic_offset,
        _ => {}
    }
    let mut y = t.pos[1];
    match t.v_align {
        -1 => y -= fudge,
        0 => y -= size[1] / 2,
        1 => y = y - size[1] + fudge,
        _ => {}
    }
    let (x1, y1) = (x + size[0], y + size[1]);
    [x.min(x1), y.min(y1), x.max(x1), y.max(y1)]
}

/// A knockout text's box (`PCB_TEXT::buildBoundingHull`): the text's polygons (`TransformOvalToPolygon( .., penWidth, maxError, ERROR_INSIDE )` of each
/// segment) turned back by the draw rotation about the draw position, their bounding box grown by `GetKnockoutTextMargin`, its corners turned again. The
/// knockout's shape is this box less the text (`BooleanSubtract`); a marker against it is placed on the box or inside it.
/// An outline font's glyph polygons are in the box as they are.
pub fn knockout_box(t: &TextShape, max_error: i64) -> Option<[[i64; 2]; 4]> {
    let pen = pen_width(t);
    let sh = shape(t)?;
    let segs = sh.segments;
    let radius = pen / 2;
    // `GetArcToSegmentCount( radius, aError, FULL_CIRCLE )`, rounded up to 8
    let rel = max_error.max(1) as f64 / radius.max(1) as f64;
    let inc = (180.0 / std::f64::consts::PI * (1.0 - rel).acos() * 2.0).min(360.0 / 8.0);
    let n = (((360.0f64 / inc).round() as i64).max(2) + 7) / 8 * 8;
    let delta = 360.0 / n as f64;
    let mut min = [i64::MAX, i64::MAX];
    let mut max = [i64::MIN, i64::MIN];
    for [a, b] in segs {
        let (mut start, mut end) = (a, [b[0] - a[0], b[1] - a[1]]);
        if end[0] < 0 {
            end = [a[0] - b[0], a[1] - b[1]];
            start = b;
        }
        let dir = if end == [0, 0] { 0.0 } else { angle_of(end) };
        let len = {
            let v = (end[0] as i128).pow(2) + (end[1] as i128).pow(2);
            (v as f64).sqrt().round() as i64
        };
        let mut pts = vec![[len, radius]];
        let mut ang = delta / 2.0;
        while ang < 180.0 {
            let c = rotate_int([0, radius], [0, 0], ang);
            pts.push([c[0] + len, c[1]]);
            ang += delta;
        }
        pts.push([len, -radius]);
        pts.push([0, -radius]);
        let mut ang = delta / 2.0;
        while ang < 180.0 {
            pts.push(rotate_int([0, -radius], [0, 0], ang));
            ang += delta;
        }
        pts.push([0, radius]);
        for p in pts {
            // `Rotate( -delta_angle )`, `Move( startp )`, then `Rotate( -GetDrawRotation(), GetDrawPos() )`
            let q = rotate_int(p, [0, 0], -dir);
            let q = [q[0] + start[0], q[1] + start[1]];
            let q = rotate_int(q, t.pos, -t.angle);
            min = [min[0].min(q[0]), min[1].min(q[1])];
            max = [max[0].max(q[0]), max[1].max(q[1])];
        }
    }
    for p in sh.polys.iter().flat_map(|g| g.outline.iter()) {
        let q = rotate_int(*p, t.pos, -t.angle);
        min = [min[0].min(q[0]), min[1].min(q[1])];
        max = [max[0].max(q[0]), max[1].max(q[1])];
    }
    if min[0] > max[0] {
        return None;
    }
    let margin = ((t.thickness as f64 / 2.0).round() as i64).max((t.size[1] as f64 / 9.0).round() as i64);
    let (x0, y0, x1, y1) = (min[0] - margin, min[1] - margin, max[0] + margin, max[1] + margin);
    Some([[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|c| rotate_int(c, t.pos, t.angle)))
}

/// `EDA_ANGLE( VECTOR2I )`, degrees: exact on the axes and the diagonals.
fn angle_of(v: [i64; 2]) -> f64 {
    let (x, y) = (v[0] as f64, v[1] as f64);
    if y == 0.0 {
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

/// `ExpandTextVars` with `resolver` (`PCB_TEXT::GetShownText`'s): each `${TOKEN}` the resolver knows replaced, nested ones from the inside out, the others
/// left as they are.
pub fn expand_vars(text: &str, resolver: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' && i + 1 < c.len() && c[i + 1] == '{' && (i == 0 || c[i - 1] != '\\') {
            // the matching brace
            let mut depth = 0;
            let mut j = i + 1;
            let mut close = None;
            while j < c.len() {
                if c[j] == '{' {
                    depth += 1;
                } else if c[j] == '}' {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
                j += 1;
            }
            if let Some(k) = close {
                let inner: String = c[i + 2..k].iter().collect();
                let token = expand_vars(&inner, resolver);
                match resolver(&token) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push_str("${");
                        out.push_str(&token);
                        out.push('}');
                    }
                }
                i = k + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "U1" at 1 mm, 0.15 mm thick, centred: the glyphs' first stroke and the pen, as `GetEffectiveTextShape` makes them.
    #[test]
    fn a_reference_is_drawn_by_the_stroke_font() {
        let t = TextShape { text: "U1".into(), pos: [5_000_000, 3_500_000], size: [1_000_000, 1_000_000], thickness: 150_000, angle: 0.0, mirror: false, italic: false, bold: false, h_align: 0, v_align: 0, line_spacing: 1.0, face: None };
        let segs = segments(&t).unwrap();
        assert!(segs.len() > 4, "{segs:?}");
        assert_eq!(pen_width(&t), 150_000);
        // centred: the strokes straddle the anchor
        assert!(segs.iter().any(|s| s[0][0] < 5_000_000) && segs.iter().any(|s| s[0][0] > 5_000_000));
        assert!(segs.iter().all(|s| (s[0][1] - 3_500_000).abs() < 800_000));
        // an overbar is one more segment, above the text; a second line is below the first
        let bar = segments(&TextShape { text: "~{U1}".into(), ..t.clone() }).unwrap();
        assert_eq!(bar.len(), segs.len() + 1);
        let two = segments(&TextShape { text: "U1\nU1".into(), ..t.clone() }).unwrap();
        assert_eq!(two.len(), 2 * segs.len());
        assert_eq!(parse_markup("a~{b_{c}}~{}x^{"), vec![Node::Text("a".into()), Node::Mark('~', vec![Node::Text("b".into()), Node::Mark('_', vec![Node::Text("c".into())])]), Node::Text("x^{".into())]);
        assert_eq!(expand_vars("R ${REFERENCE} ${NOPE}", &|k| (k == "REFERENCE").then(|| "U1".to_string())), "R U1 ${NOPE}");
        let ko = knockout_box(&t, 5_000).unwrap();
        assert!(ko[0][0] < segs.iter().map(|s| s[0][0].min(s[1][0])).min().unwrap());
        assert_eq!(draw_rotation(180.0, true), 0.0);
        assert_eq!(draw_rotation(-90.0, true), 90.0);
    }
}

