//! KiCad's "Newstroke" vector font -- decoder ported from
//! `common/font/stroke_font.cpp` (`STROKE_FONT::loadNewStrokeFont`,
//! `STROKE_FONT::GetTextAsGlyphs`), sharing *data* with the React studio's
//! own port (`web/studio/src/components/text/strokeFont.ts`) rather than
//! hand-copying glyph strokes a second time: both read the exact same
//! generated `web/studio/src/kicad/strokeFont.json`
//! (`web/studio/tools/extract-strokefont.js`, run against the real KiCad
//! source snapshot), so there is exactly one place glyph data can drift,
//! and it isn't this file.
//!
//! Scope cut: this only has to answer "what line segments does KiCad
//! actually stroke for this text", for DRC collision -- not to *render*
//! anything. So unlike `strokeFont.ts`'s `drawStrokeText`, it skips markup
//! (`~{...}`, sub/superscript, italic): PCB reference designators and this
//! model's free-standing `Text` items are always plain strings in
//! practice, so this is a real scope cut, not a silent mismatch on
//! anything this workspace produces.
//!
//! **Integration status** (see the task report): this decoder is correct
//! and unit-tested on its own (glyph decode, advance width, left/centre
//! justify, the `?` fallback), but wiring it into `board.rs`'s silk items
//! -- so `silk_over_copper`/`silk_overlap` collide against real glyph
//! strokes instead of `board.rs`'s calibrated bounding box -- surfaced a
//! `silk_over_copper` regression (a full oracle sweep went from 59/35 to
//! 288/35, including new false positives on boards kicad-cli calls
//! silk-clean) that was not root-caused in the time available: neither
//! the vertical anchor convention (baseline vs. this string's own ink
//! centre, both tried) nor the glyph bounding boxes themselves (checked
//! directly, plausible sizes) explain it, and the false collisions are
//! exclusively against *tracks*, never pads, which narrows but does not
//! yet pin down where the remaining bug is -- most likely in exactly
//! which rotation/anchor combination `crates/kicad/src/pcb.rs`'s real
//! `(property "Reference" ...)` writer intends, which this module's two
//! tried conventions did not reproduce. `board.rs` still uses its
//! calibrated bounding box; nothing calls `layout_strokes`/
//! `layout_strokes_vcenter` yet. Left in place, tested, so a future
//! attempt starts from a correct decoder rather than re-porting one.

use crate::kimath::Seg;
use eda_model::ir::{Point, TextJustify, Um};
use std::collections::HashMap;
use std::sync::OnceLock;

/// `STROKE_FONT_SCALE`, stroke_font.cpp.
const SCALE: f64 = 1.0 / 21.0;
/// `FONT_OFFSET`, stroke_font.cpp -- moves the glyph origin to the baseline.
const FONT_OFFSET: f64 = -8.0;
const R_CODE: i64 = b'R' as i64;

struct Glyph {
    /// Advance width, font-design units (already `* SCALE`; the caller
    /// multiplies by its own size to get real units).
    width: f64,
    /// One entry per pen-down run; each point already has `glyphStartX`
    /// subtracted and `FONT_OFFSET` applied -- exactly
    /// `loadNewStrokeFont`'s per-glyph loop.
    strokes: Vec<Vec<(f64, f64)>>,
}

#[derive(serde::Deserialize)]
struct FontData {
    glyphs: HashMap<String, String>,
}

fn font_data() -> &'static FontData {
    static DATA: OnceLock<FontData> = OnceLock::new();
    DATA.get_or_init(|| {
        let raw = include_str!("../../../web/studio/src/kicad/strokeFont.json");
        serde_json::from_str(raw).expect("strokeFont.json is checked in and generated; it must parse")
    })
}

/// Decodes one raw stroke-data string (e.g. `"H\\QFSFUGVHWJXNXSWWVYUZS[Q[OZNYMWLSLNMJNHOGQF"`)
/// into a [`Glyph`]. Ported from `STROKE_FONT::loadNewStrokeFont`'s per-glyph loop.
fn decode_glyph(raw: &str) -> Glyph {
    let cc: Vec<i64> = raw.chars().map(|c| c as i64).collect();
    if cc.len() < 2 {
        return Glyph { width: 0.6, strokes: Vec::new() };
    }
    let glyph_start_x = (cc[0] - R_CODE) as f64 * SCALE;
    let glyph_end_x = (cc[1] - R_CODE) as f64 * SCALE;
    let width = glyph_end_x - glyph_start_x;

    let mut strokes: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let mut i = 2;
    while i + 1 < cc.len() {
        let (a, b) = (cc[i], cc[i + 1]);
        if a == 32 && b == R_CODE {
            if !current.is_empty() {
                strokes.push(std::mem::take(&mut current));
            }
        } else {
            let x = (a - R_CODE) as f64 * SCALE - glyph_start_x;
            let y = ((b - R_CODE) as f64 + FONT_OFFSET) * SCALE;
            current.push((x, y));
        }
        i += 2;
    }
    if !current.is_empty() {
        strokes.push(current);
    }
    Glyph { width, strokes }
}

const FALLBACK_CODEPOINT: u32 = b'?' as u32;

/// Ported from `STROKE_FONT::GetTextAsGlyphs`: any codepoint outside the
/// loaded font falls back to `?`.
fn get_glyph(codepoint: u32) -> Glyph {
    let data = font_data();
    match data.glyphs.get(&codepoint.to_string()) {
        Some(raw) => decode_glyph(raw),
        None if codepoint == FALLBACK_CODEPOINT => Glyph { width: 0.6, strokes: Vec::new() },
        None => get_glyph(FALLBACK_CODEPOINT),
    }
}

/// One decoded Newstroke glyph for callers outside this crate that lay out
/// text themselves (the schematic plotter, `eda_kicad::plotter`, ports
/// `STROKE_FONT::GetTextAsGlyphs`' markup/cursor logic on top of this):
/// the advance width and the pen-down runs, both in font-design units
/// (multiply by the text size). Same fallback to `?` as [`get_glyph`].
pub struct GlyphStrokes {
    /// Advance width (`STROKE_GLYPH::BoundingBox().GetEnd().x`).
    pub width: f64,
    /// Pen-down runs, y-down with the baseline at y = 0.
    pub strokes: Vec<Vec<(f64, f64)>>,
}

/// Public view of [`get_glyph`] -- see [`GlyphStrokes`].
pub fn glyph_strokes(codepoint: u32) -> GlyphStrokes {
    let g = get_glyph(codepoint);
    GlyphStrokes { width: g.width, strokes: g.strokes }
}

/// World-space pen-stroke segments for `text`, set at `size_um` and
/// anchored (baseline-left, before `justify`) at `anchor` -- the same
/// convention `web/studio`'s canvas painter uses for free board text
/// (`drawStrokeText(ctx, t.content, t.x, t.y, ...)` with `t.x, t.y` passed
/// straight through). `rot_millideg`/`mirror` use exactly
/// `eda_model::footprint::to_board`'s convention (this crate's other
/// local-to-board transforms all share it): rotation is
/// `(lx*cos-ly*sin, lx*sin+ly*cos)` with `mirror` flipping local x first.
/// A caller whose own rotation field uses the opposite sign convention
/// (this model's free-standing `Text::angle`, unlike `FootprintInstance::
/// rot` -- see `crates/kicad/src/pcb.rs`'s writer, which negates one but
/// not the other before emitting the same KiCad `(at x y angle)` field)
/// must negate it before calling this function.
///
/// Returns `None` for text with no ink at all (empty, or all spaces), so
/// callers never have to handle a degenerate empty `Shape::Strokes`.
pub fn layout_strokes(text: &str, anchor: Point, size_um: Um, rot_millideg: i64, mirror: bool, justify: TextJustify) -> Option<Vec<Seg>> {
    layout_strokes_anchored(text, anchor, size_um, rot_millideg, mirror, justify, VAnchor::Baseline)
}

/// Vertical anchor convention: where `anchor`'s y lands relative to the
/// text's own ink.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VAnchor {
    /// `anchor` is the baseline -- KiCad's bare `gr_text`/free `PCB_TEXT`
    /// convention (confirmed by `web/studio`'s own canvas painter, which
    /// passes a free `Text` item's raw `(t.x, t.y)` straight through with
    /// no vertical offset of its own).
    Baseline,
    /// `anchor` is the vertical centre of this specific string's own ink
    /// (its glyphs' actual min/max y, not a fixed font-metric guess) --
    /// KiCad's `(property "Reference" ...)`/`(property "Value" ...)`
    /// convention: unlike bare text, a footprint field has no vertical-
    /// justify token in the file at all, and `crates/kicad/src/pcb.rs`'s
    /// writer picks the *same* 700 µm offset magnitude for `Above` and
    /// `Below` -- which only leaves a safe gap on both sides when the
    /// glyph is centred on it. A baseline anchor would put nearly all of
    /// an all-caps/digit refdes string's ink on the ascent side, leaving
    /// a generous gap on `Above` but almost none -- or a small overlap,
    /// for a tall enough string -- on `Below`; empirically confirmed via
    /// the oracle (see the task report).
    Center,
}

fn layout_strokes_anchored(text: &str, anchor: Point, size_um: Um, rot_millideg: i64, mirror: bool, justify: TextJustify, vanchor: VAnchor) -> Option<Vec<Seg>> {
    if text.is_empty() {
        return None;
    }
    let space_width = get_glyph(' ' as u32).width;
    let mut cursor = 0.0f64;
    let mut placed: Vec<(f64, Glyph)> = Vec::new();
    for ch in text.chars() {
        if ch == ' ' {
            cursor += space_width;
            continue;
        }
        let g = get_glyph(ch as u32);
        let w = g.width;
        placed.push((cursor, g));
        cursor += w;
    }
    if placed.is_empty() {
        return None;
    }
    let total_width = cursor;
    let start_x = match justify {
        TextJustify::Left => 0.0,
        TextJustify::Center => -total_width / 2.0,
        TextJustify::Right => -total_width,
    };
    // `VAnchor::Center`: this string's own actual ink extent (not a fixed
    // font-metric guess -- an all-caps/digit refdes has no descenders, so
    // its true midpoint sits well above the font's generic one), shifted
    // so that midpoint lands on `anchor` instead of the baseline.
    let y_shift = match vanchor {
        VAnchor::Baseline => 0.0,
        VAnchor::Center => {
            let (mut y_min, mut y_max) = (f64::MAX, f64::MIN);
            for (_, g) in &placed {
                for stroke in &g.strokes {
                    for &(_, y) in stroke {
                        y_min = y_min.min(y);
                        y_max = y_max.max(y);
                    }
                }
            }
            if y_min > y_max {
                0.0
            } else {
                (y_min + y_max) / 2.0
            }
        }
    };

    let size = size_um as f64;
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let mirror_sign = if mirror { -1.0 } else { 1.0 };
    let to_board = |local_x: f64, local_y: f64| -> Point {
        let lx = local_x * size * mirror_sign;
        let ly = (local_y - y_shift) * size;
        Point { x: anchor.x + (lx * cos - ly * sin).round() as Um, y: anchor.y + (lx * sin + ly * cos).round() as Um }
    };

    let mut segs = Vec::new();
    for (gx0, g) in &placed {
        for stroke in &g.strokes {
            for w in stroke.windows(2) {
                let (x0, y0) = w[0];
                let (x1, y1) = w[1];
                segs.push(Seg::new(to_board(start_x + gx0 + x0, y0), to_board(start_x + gx0 + x1, y1)));
            }
        }
    }
    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

/// [`layout_strokes`], but `anchor` is this string's own vertical ink
/// centre rather than its baseline -- see [`VAnchor::Center`]. Used for
/// footprint `(property ...)` fields (reference/value), never for bare
/// board text (`Text`/`gr_text`), which is baseline-anchored.
pub fn layout_strokes_vcenter(text: &str, anchor: Point, size_um: Um, rot_millideg: i64, mirror: bool, justify: TextJustify) -> Option<Vec<Seg>> {
    layout_strokes_anchored(text, anchor, size_um, rot_millideg, mirror, justify, VAnchor::Center)
}

/// Inputs to [`kicad_text_segments`]: one `EDA_TEXT`'s layout attributes.
pub struct TextLayout<'a> {
    pub text: &'a str,
    /// `GetDrawPos()`, board space.
    pub pos: Point,
    /// Glyph (width, height).
    pub size: (Um, Um),
    /// The file's pen thickness (`TEXT_ATTRIBUTES::m_StrokeWidth`).
    pub thickness: Um,
    /// `GetDrawRotation()`, KiCad's sign convention (counter-clockwise
    /// positive), degrees.
    pub angle_deg: f64,
    pub mirror: bool,
    /// `-1` left, `0` centre, `1` right.
    pub halign: i8,
    /// `-1` top, `0` centre, `1` bottom.
    pub valign: i8,
}

/// `EDA_TEXT::GetEffectiveTextPenWidth`: the pen width the glyph strokes
/// are inflated by.
pub fn effective_pen_width(thickness: Um, size: (Um, Um), bold: bool) -> Um {
    let mut pen = thickness;
    if pen <= 1 {
        let w = size.0 as f64;
        pen = if bold { (w / 5.0).round() as Um } else { (w / 8.0).round() as Um };
    }
    // `ClampTextPenSize`: at most a quarter of the smaller glyph dimension.
    let max = ((size.0.abs().min(size.1.abs())) as f64 * 0.25).round() as Um;
    pen.min(max)
}

/// One run of marked-up text: `~{overbar}`, `_{subscript}`, `^{superscript}`
/// (`MARKUP::MARKUP_PARSER`). `${VAR}` references are copied through
/// literally (an unresolved variable is drawn as written).
struct Run {
    text: String,
    sub: bool,
    sup: bool,
    over: bool,
}

fn parse_markup(line: &str) -> Vec<Run> {
    let chars: Vec<char> = line.chars().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut stack: Vec<char> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, stack: &Vec<char>, runs: &mut Vec<Run>| {
        if !cur.is_empty() {
            runs.push(Run { text: std::mem::take(cur), sub: stack.contains(&'_'), sup: stack.contains(&'^'), over: stack.contains(&'~') });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if (c == '~' || c == '^' || c == '_') && next == Some('{') {
            flush(&mut cur, &stack, &mut runs);
            stack.push(c);
            i += 2;
            continue;
        }
        if c == '$' && next == Some('{') {
            while i < chars.len() {
                cur.push(chars[i]);
                if chars[i] == '}' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '}' && !stack.is_empty() {
            flush(&mut cur, &stack, &mut runs);
            stack.pop();
            i += 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    flush(&mut cur, &stack, &mut runs);
    runs
}

/// The pen-stroke segments `FONT::Draw` (with `STROKE_FONT::GetTextAsGlyphs`
/// and `FONT::getLinePositions`) produces for a stroke-font text: line
/// layout with the 6.0-compatibility fudge factors (`size.y * 1.17` first
/// line height, `thickness / 1.52` x offset, `thickness * 0.052` y offset),
/// per-glyph rounded advances, `~{}`/`_{}`/`^{}` markup (overbars,
/// sub/superscripts at 0.8 scale), mirror about the anchor and rotation
/// about the anchor. Italics are not drawn.
pub fn kicad_text_segments(t: &TextLayout) -> Vec<Seg> {
    let lines: Vec<&str> = t.text.split('\n').collect();
    let (sx, sy) = (t.size.0 as f64, t.size.1 as f64);
    let space_width = get_glyph(' ' as u32).width;
    // `GetInterline( size.y ) * LEGACY_FACTOR * m_LineSpacing`, truncated to int.
    let interline = (sy * 1.68 * 0.9583) as i64;

    // A placed glyph / bar, relative to its line origin: (x cursor, y offset, glyph size, glyph).
    struct Placed {
        x: i64,
        y: f64,
        sx: f64,
        sy: f64,
        glyph: Glyph,
    }
    struct Bar {
        x0: i64,
        x1: i64,
        y: f64,
        trim: f64,
    }
    struct Line {
        glyphs: Vec<Placed>,
        bars: Vec<Bar>,
        width: i64,
    }
    let mut laid: Vec<Line> = Vec::new();
    for line in &lines {
        let mut cursor: i64 = 0;
        let mut glyphs = Vec::new();
        let mut bars = Vec::new();
        for run in parse_markup(line) {
            let (gsx, gsy, yoff) = if run.sub || run.sup {
                let (a, b) = (sx * 0.8, sy * 0.8);
                (a, b, if run.sub { b * 0.15 } else { -b * 0.35 })
            } else {
                (sx, sy, 0.0)
            };
            let start = cursor;
            for ch in run.text.chars() {
                if ch == ' ' {
                    cursor += (gsx * space_width).round() as i64;
                } else {
                    let g = get_glyph(ch as u32);
                    let adv = (g.width * gsx).round() as i64;
                    glyphs.push(Placed { x: cursor, y: yoff, sx: gsx, sy: gsy, glyph: g });
                    cursor += adv;
                }
            }
            if run.over {
                // `drawMarkup`'s overbar: shortened by 10% of the glyph width at each end,
                // `GetOverbarVerticalPosition` above the baseline.
                bars.push(Bar { x0: start, x1: cursor, y: yoff - gsy * 1.23, trim: gsx * 0.1 });
            }
        }
        laid.push(Line { glyphs, bars, width: cursor });
    }

    // `getLinePositions`
    let mut height = 0.0f64;
    for i in 0..laid.len() {
        height += if i == 0 { sy * 1.17 } else { interline as f64 };
    }
    let th = t.thickness as f64;
    let off_x = th / 1.52;
    let mut off_y = sy - th * 0.052;
    match t.valign {
        -1 => {}
        0 => off_y -= (height / 2.0).trunc(),
        _ => off_y -= height.trunc(),
    }
    let (sin, cos) = (t.angle_deg.to_radians()).sin_cos();
    let origin = (t.pos.x as f64, t.pos.y as f64);
    let xf = |x: f64, y: f64| -> Point {
        let x = if t.mirror { origin.0 - (x - origin.0) } else { x };
        let (dx, dy) = (x - origin.0, y - origin.1);
        Point { x: (origin.0 + dx * cos + dy * sin).round() as Um, y: (origin.1 - dx * sin + dy * cos).round() as Um }
    };
    let mut segs = Vec::new();
    for (i, line) in laid.iter().enumerate() {
        let line_off_x = match t.halign {
            -1 => off_x,
            0 => -(line.width as f64) / 2.0,
            _ => -(line.width as f64 + off_x),
        };
        let line_off_y = off_y + (i as i64 * interline) as f64;
        let (lx, ly) = (origin.0 + line_off_x.trunc(), origin.1 + line_off_y.trunc());
        for p in &line.glyphs {
            for stroke in &p.glyph.strokes {
                let pts: Vec<Point> = stroke.iter().map(|&(px, py)| xf(px * p.sx + lx + p.x as f64, py * p.sy + ly + p.y)).collect();
                for w in pts.windows(2) {
                    segs.push(Seg::new(w[0], w[1]));
                }
            }
        }
        for b in &line.bars {
            segs.push(Seg::new(xf(lx + b.x0 as f64 + b.trim, ly + b.y), xf(lx + b.x1 as f64 - b.trim, ly + b.y)));
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_period_as_a_small_closed_loop() {
        // '.' (codepoint 46): "MWRYSZR[QZRYR[" -- one pen-down run, a tiny closed loop.
        let g = decode_glyph("MWRYSZR[QZRYR[");
        assert_eq!(g.strokes.len(), 1);
        assert!(g.strokes[0].len() >= 4);
    }

    #[test]
    fn space_advances_the_cursor_without_ink() {
        let segs = layout_strokes("  ", Point { x: 0, y: 0 }, 1000, 0, false, TextJustify::Left);
        assert!(segs.is_none());
    }

    #[test]
    fn unrotated_left_justified_text_starts_at_the_anchor() {
        let segs = layout_strokes("I", Point { x: 5000, y: 5000 }, 1000, 0, false, TextJustify::Left).unwrap();
        // Every point must be at or to the right of the anchor for a
        // left-justified, unrotated, unmirrored glyph.
        assert!(segs.iter().all(|s| s.a.x >= 5000 && s.b.x >= 5000), "{segs:?}");
    }

    #[test]
    fn center_justify_straddles_the_anchor() {
        let left = layout_strokes("HI", Point { x: 0, y: 0 }, 1000, 0, false, TextJustify::Left).unwrap();
        let center = layout_strokes("HI", Point { x: 0, y: 0 }, 1000, 0, false, TextJustify::Center).unwrap();
        let left_span = left.iter().map(|s| s.a.x.max(s.b.x)).max().unwrap();
        let center_span = center.iter().map(|s| s.a.x.max(s.b.x)).max().unwrap();
        assert!(center_span < left_span);
    }

    #[test]
    fn unknown_codepoint_falls_back_to_question_mark() {
        let a = layout_strokes("\u{1F600}", Point { x: 0, y: 0 }, 1000, 0, false, TextJustify::Left);
        let b = layout_strokes("?", Point { x: 0, y: 0 }, 1000, 0, false, TextJustify::Left);
        assert_eq!(a.map(|s| s.len()), b.map(|s| s.len()));
    }

    #[test]
    fn overbar_markup_drops_the_braces_and_adds_a_bar() {
        let plain = kicad_text_segments(&TextLayout { text: "AB", pos: Point { x: 0, y: 0 }, size: (1000, 1000), thickness: 150, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        let barred = kicad_text_segments(&TextLayout { text: "~{AB}", pos: Point { x: 0, y: 0 }, size: (1000, 1000), thickness: 150, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        // Same glyph strokes plus exactly one horizontal bar above them.
        assert_eq!(barred.len(), plain.len() + 1);
        let bar = barred.last().unwrap();
        assert_eq!(bar.a.y, bar.b.y);
        assert!(bar.a.y < plain.iter().map(|s| s.a.y.min(s.b.y)).min().unwrap());
    }

    #[test]
    fn centred_text_is_centred_on_its_anchor() {
        let segs = kicad_text_segments(&TextLayout { text: "HELLO", pos: Point { x: 10_000, y: 20_000 }, size: (1000, 1000), thickness: 150, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        let (x0, x1) = (segs.iter().map(|s| s.a.x.min(s.b.x)).min().unwrap(), segs.iter().map(|s| s.a.x.max(s.b.x)).max().unwrap());
        assert!(((x0 + x1) / 2 - 10_000).abs() < 400, "{x0} {x1}");
    }
}
