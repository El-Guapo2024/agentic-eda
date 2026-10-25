//! A board text's `(effects ..)`, read and written whole: the reader and writer of [`eda_model::board_text::TextExtra`].
//!
//! `Text` holds a square size, a stroke width, a horizontal justification and a mirror; `EDA_TEXT::Format` says more (`(size HEIGHT WIDTH)`, `(line_spacing ..)`,
//! `bold`, `italic`, `justify top|bottom`, `(face ..)`), and `(layer "F.Cu" knockout)` is a flag of the layer token. [`read_style`] takes all of it from a `gr_text`;
//! [`effects`] writes it back, with the words of `FieldLayout`'s writer (a footprint's own fields go through the same `effects_with`).

use eda_model::board_text::TextExtra;
use eda_model::fp_edit::FieldLayout;
use eda_model::ir::{Point, Text, TextJustify, TextVAlign, Nm};

use crate::sexpr::{self, Sexpr};

/// What a `gr_text` item says about its looks.
pub(crate) struct ReadStyle {
    /// The glyph height, µm (`Text::size_nm`).
    pub size_nm: Nm,
    pub stroke_width: Nm,
    pub justify: TextJustify,
    pub mirror: bool,
    /// What the text has beyond those, with an empty id (the caller names the text first).
    pub extra: TextExtra,
}

/// `GetPenSizeForNormal` and `GetPenSizeForBold` (`common/eda_text.cpp`): an eighth and a fifth of the glyph width.
fn auto_pen_nm(width_nm: Nm, bold: bool) -> Nm {
    Nm::kiround(width_nm.as_f64() / if bold { 5.0 } else { 8.0 })
}

/// The looks of the `gr_text` `item`.
pub(crate) fn read_style(item: &[Sexpr]) -> ReadStyle {
    // The layout reader of a footprint's fields reads the whole `(effects ..)`; the board is its frame (a top-side footprint turned by nothing).
    let l = crate::fp_fields::parse_layout(item, eda_model::ir::Side::Top, 0);
    let (width, height) = (l.size.0, l.size.1);
    let font = sexpr::find(item, "effects").and_then(|e| sexpr::find(e, "font"));
    let auto_thickness = l.thickness == Nm::ZERO;
    let stroke_width = if auto_thickness { auto_pen_nm(width, l.bold) } else { l.thickness };
    let line_spacing_permille = font.and_then(|f| sexpr::find(f, "line_spacing")).and_then(|s| sexpr::num(s, 1)).map(|v| (v * 1000.0).round() as i64).filter(|v| *v != 1000).unwrap_or(0);
    let face = font.and_then(|f| sexpr::find(f, "face")).and_then(|f| sexpr::txt(f, 1)).filter(|f| !f.is_empty()).map(str::to_string);
    let extra = TextExtra {
        id: String::new(),
        width_nm: (width != height).then_some(width),
        auto_thickness,
        valign: match l.valign {
            -1 => TextVAlign::Top,
            1 => TextVAlign::Bottom,
            _ => TextVAlign::Center,
        },
        bold: l.bold,
        italic: l.italic,
        knockout: l.knockout,
        line_spacing_permille,
        face,
    };
    ReadStyle {
        size_nm: height,
        stroke_width,
        justify: match l.halign {
            -1 => TextJustify::Left,
            1 => TextJustify::Right,
            _ => TextJustify::Center,
        },
        mirror: l.mirror,
        extra,
    }
}

/// `(effects (font [(face ..)] (size H W) [(line_spacing ..)] [(thickness ..)] [bold] [italic]) [(justify ..)])` of `text` (`EDA_TEXT::Format`).
pub(crate) fn effects(text: &Text, extra: Option<&TextExtra>) -> String {
    let default = TextExtra::default();
    let extra = extra.unwrap_or(&default);
    let mut l = FieldLayout::new(Point { x: Nm::ZERO, y: Nm::ZERO }, &text.layer);
    l.size = (extra.width_nm.unwrap_or(text.size_nm), text.size_nm);
    l.thickness = if extra.auto_thickness { Nm::ZERO } else { text.stroke_width };
    l.halign = match text.justify {
        TextJustify::Left => -1,
        TextJustify::Center => 0,
        TextJustify::Right => 1,
    };
    l.valign = match extra.valign {
        TextVAlign::Top => -1,
        TextVAlign::Center => 0,
        TextVAlign::Bottom => 1,
    };
    l.mirror = text.mirror;
    l.bold = extra.bold;
    l.italic = extra.italic;
    crate::fp_fields::effects_with(&l, extra.face.as_deref(), extra.line_spacing_permille)
}
