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
}
