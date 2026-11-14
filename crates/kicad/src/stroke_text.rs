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
//! Not ported (the text has no shape here, its markers stay the studio's): markup (`~{..}`, `_{..}`, `^{..}`), several lines, tabs, text variables, an outline
//! font, a knockout text.

use std::collections::HashMap;
use std::sync::OnceLock;

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
#[derive(Debug, Clone, PartialEq)]
pub struct TextShape {
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
        a %= 720.0;
        while a < -0.0 {
            a += 360.0;
        }
        while a >= 360.0 {
            a -= 360.0;
        }
        a
    }
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
    let mut a = degrees % 720.0;
    while a < -0.0 {
        a += 360.0;
    }
    while a >= 360.0 {
        a -= 360.0;
    }
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
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let (s, co) = match a {
            v if v == 45.0 => (h, h),
            v if v == 135.0 => (h, -h),
            v if v == 225.0 => (-h, -h),
            v if v == 315.0 => (-h, h),
            v => v.to_radians().sin_cos(),
        };
        (oy * s + ox * co, oy * co - ox * s)
    };
    [x + c[0], y + c[1]]
}

/// The segments of a text's shape (`GetEffectiveTextShape`), each `[a, b]` in nanometres, as wide as [`pen_width`]. `None` for a text this port does not draw.
pub fn segments(t: &TextShape) -> Option<Vec<[[i64; 2]; 2]>> {
    if t.text.is_empty() || t.text.contains('\n') || t.text.contains('\t') || t.text.contains("${") || ["~{", "_{", "^{"].iter().any(|m| t.text.contains(m)) {
        return None;
    }
    let kiround = |v: f64| v.round() as i64;
    let size = [t.size[0] as f64, t.size[1] as f64];
    let space_width = glyph(' ')?.width;
    // `GetTextAsGlyphs`' advance of the whole line (`boundingBoxSingleLine`: the cursor's end)
    let advance = |ch: char| -> i64 {
        if ch == ' ' {
            kiround(size[0] * space_width)
        } else {
            glyph(ch).map_or(0, |g| kiround(g.width * size[0]))
        }
    };
    let line_x: i64 = t.text.chars().map(advance).sum();
    // `getLinePositions`
    let height = (t.size[1] as f64 * 1.17) as i64;
    let mut offset = [0i64, t.size[1]];
    offset[0] = (offset[0] as f64 + t.thickness as f64 / 1.52) as i64;
    offset[1] = (offset[1] as f64 - t.thickness as f64 * 0.052) as i64;
    match t.v_align {
        0 => offset[1] -= height / 2,
        1 => offset[1] -= height,
        _ => {}
    }
    let mut line_offset = offset;
    match t.h_align {
        0 => line_offset[0] = -line_x / 2,
        1 => line_offset[0] = -(line_x + offset[0]),
        _ => {}
    }
    let origin = [t.pos[0] as f64, t.pos[1] as f64];
    let mut cursor = [t.pos[0] + line_offset[0], t.pos[1] + line_offset[1]];
    let tilt = if t.italic { ITALIC_TILT } else { 0.0 };
    let mut out = Vec::new();
    for ch in t.text.chars() {
        if ch != ' ' {
            let g = glyph(ch)?;
            for stroke in &g.strokes {
                // `STROKE_GLYPH::Transform`
                let pts: Vec<[i64; 2]> = stroke
                    .iter()
                    .map(|p| {
                        let mut q = [p[0] * size[0], p[1] * size[1]];
                        if tilt != 0.0 {
                            q[0] -= q[1] * tilt;
                        }
                        q = [q[0] + cursor[0] as f64, q[1] + cursor[1] as f64];
                        if t.mirror {
                            q[0] = origin[0] - (q[0] - origin[0]);
                        }
                        if t.angle != 0.0 {
                            q = rotate(q, origin, t.angle);
                        }
                        // the stroke callback takes `VECTOR2I`: a cast, which truncates
                        [q[0] as i64, q[1] as i64]
                    })
                    .collect();
                out.extend(pts.windows(2).map(|w| [w[0], w[1]]));
            }
        }
        cursor[0] += advance(ch);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "U1" at 1 mm, 0.15 mm thick, centred: the glyphs' first stroke and the pen, as `GetEffectiveTextShape` makes them.
    #[test]
    fn a_reference_is_drawn_by_the_stroke_font() {
        let t = TextShape { text: "U1".into(), pos: [5_000_000, 3_500_000], size: [1_000_000, 1_000_000], thickness: 150_000, angle: 0.0, mirror: false, italic: false, bold: false, h_align: 0, v_align: 0 };
        let segs = segments(&t).unwrap();
        assert!(segs.len() > 4, "{segs:?}");
        assert_eq!(pen_width(&t), 150_000);
        // centred: the strokes straddle the anchor
        assert!(segs.iter().any(|s| s[0][0] < 5_000_000) && segs.iter().any(|s| s[0][0] > 5_000_000));
        assert!(segs.iter().all(|s| (s[0][1] - 3_500_000).abs() < 800_000));
        assert_eq!(segments(&TextShape { text: "~{RST}".into(), ..t.clone() }), None);
        assert_eq!(draw_rotation(180.0, true), 0.0);
        assert_eq!(draw_rotation(-90.0, true), 90.0);
    }
}
