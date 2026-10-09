//! KiCad's stroke font metrics, as eeschema measures text with them.
//!
//! Only widths are needed to know how much room a piece of schematic text takes: the glyph strokes themselves are
//! `eda_drc::stroke_font`'s business. Both read the same generated `web/studio/src/kicad/strokeFont.json`
//! (`common/newstroke_font.cpp`, extracted by `web/studio/tools/extract-strokefont.js`), so there is one copy of the font data.
//!
//! Everything here is in KiCad's schematic internal units, 0.1 micrometre ([`IU_PER_MM`] per millimetre), with `KiROUND`'s
//! rounding where KiCad rounds, so a width computed here is the width eeschema gets (`STROKE_FONT::GetTextAsGlyphs`,
//! `FONT::StringBoundaryLimits`, `EDA_TEXT::GetTextBox`).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// Schematic internal units per millimetre (`schIUScale`).
pub const IU_PER_MM: f64 = 10_000.0;

/// 50 mils: the size of every field, label and pin text a schematic starts from.
pub const DEFAULT_TEXT_SIZE_IU: i64 = 12_700;

/// `KiROUND`: round half away from zero.
pub fn kiround(v: f64) -> i64 {
    v.round() as i64
}

/// Millimetres to internal units.
pub fn mm_to_iu(mm: f64) -> i64 {
    kiround(mm * IU_PER_MM)
}

#[derive(Deserialize)]
struct FontData {
    glyphs: HashMap<String, String>,
}

/// Advance width of each glyph, in font units (a fraction of the text size), by code point.
fn widths() -> &'static HashMap<u32, f64> {
    static WIDTHS: OnceLock<HashMap<u32, f64>> = OnceLock::new();
    WIDTHS.get_or_init(|| {
        let data: FontData = serde_json::from_str(include_str!("../../../web/studio/src/kicad/strokeFont.json")).expect("strokeFont.json is checked in and generated; it must parse");
        data.glyphs
            .iter()
            .filter_map(|(code, raw)| {
                let code: u32 = code.parse().ok()?;
                let mut chars = raw.chars();
                // `loadNewStrokeFont`: the first two characters are the glyph's start and end x, each `'R'`-based, in 1/21 units.
                let (start, end) = (chars.next()? as i64, chars.next()? as i64);
                Some((code, (end - start) as f64 / 21.0))
            })
            .collect()
    })
}

/// A glyph's advance in font units: the glyph for `c`, or `?` for anything the font does not have.
pub fn advance(c: char) -> f64 {
    let w = widths();
    w.get(&(c as u32)).or_else(|| w.get(&('?' as u32))).copied().unwrap_or(0.6)
}

/// The pen KiCad writes text with when the file gives no thickness: `GetPenSizeForNormal` (an eighth of the size), clamped to a
/// quarter of it (`ClampTextPenSize`).
pub fn default_pen_iu(size_iu: i64) -> i64 {
    kiround(size_iu as f64 / 8.0).min(kiround(size_iu as f64 * 0.25))
}

/// Strip markup: `~{overbar}`, `_{subscript}` and `^{superscript}` draw their content and no braces. Returns the drawn
/// text and whether it has an overbar (which `EDA_TEXT::GetTextBox` makes room for).
pub fn strip_markup(text: &str) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut open = 0usize;
    let mut overbar = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '~' | '_' | '^') && chars.get(i + 1) == Some(&'{') {
            overbar |= c == '~';
            open += 1;
            i += 2;
            continue;
        }
        if c == '}' && open > 0 {
            open -= 1;
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    (out, overbar)
}

/// Width of a run of text at `size_iu`, without the padding `string_boundary_limits` adds: the sum of every glyph's rounded
/// advance, less the gap after the last one (`STROKE_FONT::GetTextAsGlyphs`: `INTER_CHAR` is 0.2).
pub fn text_advance_iu(text: &str, size_iu: i64) -> i64 {
    let (shown, _) = strip_markup(text);
    if shown.is_empty() {
        return 0;
    }
    let size = size_iu as f64;
    let sum: i64 = shown.chars().map(|c| kiround(advance(if c == ' ' { ' ' } else { c }) * size)).sum();
    sum - kiround(size * 0.2)
}

/// `FONT::StringBoundaryLimits` for the stroke font: the size of the box one line of text takes, ink and the pen's
/// reach included (`boundingBox.Inflate( KiROUND( thickness * 1.5 ) )`).
pub fn string_boundary_limits(text: &str, size_iu: i64, thickness_iu: i64) -> (i64, i64) {
    let k = kiround(thickness_iu as f64 * 1.5);
    (text_advance_iu(text, size_iu) + 2 * k, size_iu + 2 * k)
}

/// Where a text's box sits against its anchor, as `GR_TEXT_H_ALIGN_T` / `GR_TEXT_V_ALIGN_T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HJustify {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VJustify {
    Top,
    Center,
    Bottom,
}

/// `EDA_TEXT::GetTextBox` for one unrotated line of stroke-font text anchored at the origin: `(x0, y0, x1, y1)` in IU,
/// y down. The box is a little taller than the ink (a 17 % fudge on the height, which stroke fonts get) and carries the pen's
/// reach on every side; justification moves it against the anchor.
pub fn text_box_iu(text: &str, size_iu: i64, thickness_iu: i64, h: HJustify, v: VJustify) -> (i64, i64, i64, i64) {
    let (w, ext_h) = string_boundary_limits(text, size_iu, thickness_iu);
    let fudge = kiround(ext_h as f64 * 0.17);
    let mut height = ext_h + fudge;
    if strip_markup(text).1 {
        height += ext_h / 6;
    }
    let x0 = match h {
        HJustify::Left => 0,
        HJustify::Center => -(w / 2),
        HJustify::Right => -w,
    };
    let y0 = match v {
        VJustify::Top => -fudge,
        VJustify::Center => -(height / 2),
        VJustify::Bottom => -height + fudge,
    };
    (x0, y0, x0 + w, y0 + height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_match_the_font_table() {
        assert!((advance('R') - 1.0).abs() < 1e-9);
        assert!((advance('1') - 20.0 / 21.0).abs() < 1e-9);
        assert!((advance(' ') - 16.0 / 21.0).abs() < 1e-9);
        assert_eq!(advance('\u{1F600}'), advance('?'));
    }

    #[test]
    fn a_fifty_mil_label_is_a_bit_over_two_millimetres_tall() {
        // 1.27 mm text, pen 0.1588 mm: the extents are 1.746 mm tall, the box 17 % more.
        let (_, ext_h) = string_boundary_limits("R1", DEFAULT_TEXT_SIZE_IU, default_pen_iu(DEFAULT_TEXT_SIZE_IU));
        assert_eq!(ext_h, 12_700 + 2 * 2_382);
        let (x0, y0, x1, y1) = text_box_iu("R1", DEFAULT_TEXT_SIZE_IU, default_pen_iu(DEFAULT_TEXT_SIZE_IU), HJustify::Left, VJustify::Center);
        assert_eq!(x0, 0);
        assert_eq!(y1 - y0, 17_464 + 2_969);
        assert_eq!(y0, -(20_433 / 2));
        // 'R' is a full em, '1' 20/21 of one, the last gap is dropped (0.2 em), and the pen reaches 2382 further on each side
        assert_eq!(x1, 12_700 + 12_095 - 2_540 + 2 * 2_382);
    }

    #[test]
    fn markup_is_not_drawn() {
        assert_eq!(strip_markup("~{CS}"), ("CS".to_string(), true));
        assert_eq!(text_advance_iu("~{CS}", 12_700), text_advance_iu("CS", 12_700));
    }
}
