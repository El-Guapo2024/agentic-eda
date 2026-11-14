//! An outline (TrueType / OpenType) font as KiCad's `OUTLINE_FONT` lays text out in it, for the server's text geometry (UI-LEFT-3 C3-3): the knockouts,
//! clearances, cuts, line breaks and boxes that measured every text in the stroke font whatever its face.
//!
//! KiCad 8303b2ad, `common/font/outline_font.cpp`:
//!   - `OUTLINE_FONT::LoadFont`: `FONTCONFIG::FindFont` ([`crate::fontconfig::match_font`]) gives the file and face index; `FF_MISSING_BOLD` / `FF_MISSING_ITAL`
//!     make a fake bold / italic. `FONT::GetFont` falls back to the stroke font when no file is found ([`OutlineFont::load`] is `None` then).
//!   - `loadFace`: `FT_Set_Char_Size( face, 0, faceSize(), GLYPH_RESOLUTION, 0 )` -- `faceSize()` is 16 points times `m_charSizeScaler` 64 times
//!     `m_outlineFontSizeCompensation` 1.4, the int 1433; 1152 dpi; so 358.25 pixels to the em. FreeType's TrueType loader rounds that to whole pixels when the
//!     font's `head.flags` asks (bit 3, `tt_size_reset`), and rounds the size's ascender and descender to whole pixels (`FT_PIX_ROUND`; a CFF font's are
//!     `FT_PIX_CEIL` / `FT_PIX_FLOOR`, `ft_recompute_scaled_metrics`).
//!   - `GetInterline`: `aGlyphHeight * ( face->height / face->units_per_EM )` (an integer division) through `METRICS::GetInterline` (1.68 of it).
//!   - `GetTextAsGlyphs`: tabs "locked to the nearest 4th column (in space-widths)", 4 x 0.6 of the size, counted from the text's origin; each run between them
//!     through `getTextAsGlyphsUnlocked`.
//!   - `getTextAsGlyphsUnlocked`: the run shaped by HarfBuzz (`hb_shape` with the default features) at the face size (a sub- or superscript's at
//!     `subscriptSize()`, `KiROUND( 1433 * 0.64 )`); each glyph loaded (`FT_Load_Glyph`, a fake italic's 12 degree `FT_Set_Transform`, a fake bold's
//!     `FT_Outline_Embolden( .., 1 << 6 )`), decomposed and flattened (`OUTLINE_DECOMPOSER`, `BEZIER_POLY::GetPoly( .., m_FontErrorSize )` = 2 of its units),
//!     moved by the cursor (`cursor.x += x_advance * GLYPH_SIZE_SCALER`, an int), a subscript 0.25 of the scaler down and a superscript 0.45 up, scaled by
//!     `size / faceSize() * 1.4` (y flipped), moved to the position, mirrored about the origin, turned about it; a contour whose winding agrees with the
//!     outline's orientation is an outline, any other a hole of the first outline holding its first point (one inside none is an outline: "Some lovely TTF
//!     fonts decided that winding didn't matter"). The run's box is the position less the size's ascender to the cursor's end plus its descender; it
//!     returns the cursor's end.
//!
//! The file is read here (no font crate): `head`, `hhea`, `OS/2` (FreeType's `sfnt_load_face` choice of metrics), `cmap` (formats 4 and 12), `hmtx`, `glyf` /
//! `loca` (composites placed) and `CFF ` (Type 2 charstrings, a CID font's local subroutines by its FDSelect) -- the same reading as the studio's
//! `web/studio/src/kicad-port/sfnt.ts`, which the canvases use.
//!
//! Differences (what HarfBuzz and FreeType do that is not copied, there being no font crate): the shaping is the font's `cmap` with GSUB's single and ligature
//! substitutions of `ccmp`, `rlig`, `liga` and `clig` and GPOS's pair kerning (`kern`), or the legacy `kern` table for a font with no GPOS, a mark's advance
//! zeroed (`HB_OT_SHAPE_ZERO_WIDTH_MARKS_BY_GDEF_LATE`); contextual substitutions (`calt`), mark attachment, Unicode normalisation and complex scripts are not
//! done (KiCad drops HarfBuzz's offsets anyway: only `x_advance` moves the cursor). FreeType's hinting at 358 pixels to the em is not applied (it moves a point
//! by under a pixel there: under 1/700 of the size). A face with no `glyf` or `CFF ` outlines (a Type 1 font) is not read: the text stays in the stroke font,
//! as the studio's canvas leaves it.

use crate::bezier::bezier_poly_d;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// `OUTLINE_FONT::faceSize()`: `16 * m_charSizeScaler (64) * m_outlineFontSizeCompensation (1.4)`, an int.
pub const FACE_SIZE: i64 = 1433;
/// `OUTLINE_FONT::subscriptSize()`: `KiROUND( faceSize() * m_subscriptSuperscriptSize (0.64) )`.
pub const SUBSCRIPT_SIZE: i64 = 917;
/// `m_outlineFontSizeCompensation`.
pub const SIZE_COMPENSATION: f64 = 1.4;
/// `GLYPH_SIZE_SCALER`: `GLYPH_DEFAULT_DPI (72) / GLYPH_RESOLUTION (1152)`.
const GLYPH_SIZE_SCALER: f64 = 72.0 / 1152.0;
/// `ADVANCED_CFG::m_FontErrorSize`: the error a glyph's curves are flattened to, in `OUTLINE_DECOMPOSER`'s units.
const FONT_ERROR_SIZE: f64 = 2.0;
/// `m_subscriptVerticalOffset`, `m_superscriptVerticalOffset`.
const SUBSCRIPT_VERTICAL_OFFSET: f64 = -0.25;
const SUPERSCRIPT_VERTICAL_OFFSET: f64 = 0.45;
/// `GetTextAsGlyphs`' `TAB_WIDTH`.
const TAB_WIDTH: f64 = 4.0 * 0.6;
/// `METRICS::m_InterlinePitch`.
const INTERLINE_PITCH: f64 = 1.68;

// ------------------------------------------------------------------------------------------------------------------------------- the file

fn u8_at(b: &[u8], at: usize) -> Option<u8> {
    b.get(at).copied()
}
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn i16_at(b: &[u8], at: usize) -> Option<i16> {
    u16_at(b, at).map(|v| v as i16)
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    u32_at(b, at).map(|v| v as i32)
}

/// What a point of an `FT_Outline` is: on the curve, a conic's control point (`FT_CURVE_TAG_CONIC`) or a cubic's (`FT_CURVE_TAG_CUBIC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    On,
    Conic,
    Cubic,
}

/// One point of an `FT_Outline`, font units, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FtPoint {
    pub x: f64,
    pub y: f64,
    pub tag: Tag,
}

pub type FtContour = Vec<FtPoint>;

type Span = (usize, usize);

enum CffLocal {
    One(Vec<Span>),
    Cid { fds: Vec<Vec<Span>>, select: usize, format: u8 },
}

struct Cff {
    charstrings: Vec<Span>,
    gsubrs: Vec<Span>,
    local: CffLocal,
}

/// Face `index` of an sfnt file or collection: its metrics as FreeType sets them and where its tables are.
pub struct Face {
    data: Vec<u8>,
    /// `face->units_per_EM`, `ascender`, `descender`, `height` (`sfnt_load_face`: the `OS/2` typo metrics when `fsSelection` has `USE_TYPO_METRICS`, else
    /// `hhea`'s, else -- both zero -- the typo ones, else the Windows ones; `height` is ascender - descender + the line gap).
    pub units_per_em: u16,
    pub ascender: i32,
    pub descender: i32,
    pub height: i32,
    head_flags: u16,
    num_hmetrics: u16,
    hmtx: Option<usize>,
    loca: Option<usize>,
    glyf: Option<usize>,
    long_loca: bool,
    cmap: Option<usize>,
    cff: Option<Cff>,
    gsub: Option<usize>,
    gpos: Option<usize>,
    gdef: Option<usize>,
    kern: Option<usize>,
}

impl Face {
    /// Face `index` of `data`; `None` when it is no TrueType or OpenType font, or one whose outlines are not read here.
    pub fn parse(data: Vec<u8>, index: u32) -> Option<Face> {
        let b = &data[..];
        let mut at = 0usize;
        if b.get(0..4)? == b"ttcf" {
            if index >= u32_at(b, 8)? {
                return None;
            }
            at = u32_at(b, 12 + 4 * index as usize)? as usize;
        }
        let mut tables: HashMap<[u8; 4], usize> = HashMap::new();
        for i in 0..u16_at(b, at + 4)? as usize {
            let rec = at + 12 + 16 * i;
            let tag: [u8; 4] = b.get(rec..rec + 4)?.try_into().ok()?;
            tables.insert(tag, u32_at(b, rec + 8)? as usize);
        }
        let head = *tables.get(b"head")?;
        let hhea = *tables.get(b"hhea")?;
        let units_per_em = u16_at(b, head + 18)?;
        let head_flags = u16_at(b, head + 16)?;
        let long_loca = i16_at(b, head + 50)? == 1;
        let mut ascender = i16_at(b, hhea + 4)? as i32;
        let mut descender = i16_at(b, hhea + 6)? as i32;
        let mut height = ascender - descender + i16_at(b, hhea + 8)? as i32;
        if let Some(&os2) = tables.get(b"OS/2") {
            if let (Some(fs), Some(ta), Some(td), Some(tg)) = (u16_at(b, os2 + 62), i16_at(b, os2 + 68), i16_at(b, os2 + 70), i16_at(b, os2 + 72)) {
                let (ta, td, tg) = (ta as i32, td as i32, tg as i32);
                if fs & 128 != 0 {
                    (ascender, descender, height) = (ta, td, ta - td + tg);
                } else if ascender == 0 && descender == 0 {
                    if ta != 0 || td != 0 {
                        (ascender, descender, height) = (ta, td, ta - td + tg);
                    } else if let (Some(wa), Some(wd)) = (u16_at(b, os2 + 74), u16_at(b, os2 + 76)) {
                        (ascender, descender, height) = (wa as i32, -(wd as i32), wa as i32 + wd as i32);
                    }
                }
            }
        }
        let num_hmetrics = u16_at(b, hhea + 34)?;
        // the Unicode subtable: Windows full Unicode (format 12) over Windows BMP (format 4), then the Unicode platform's
        let mut cmap = None;
        if let Some(&c) = tables.get(b"cmap") {
            let mut rank = -1;
            for i in 0..u16_at(b, c + 2)? as usize {
                let rec = c + 4 + 8 * i;
                let (platform, encoding) = (u16_at(b, rec)?, u16_at(b, rec + 2)?);
                let off = c + u32_at(b, rec + 4)? as usize;
                let format = u16_at(b, off)?;
                let r = match (platform, encoding, format) {
                    (3, 10, 12) => 4,
                    (3, 1, 4) => 3,
                    (0, _, 12) => 2,
                    (0, _, 4) => 1,
                    _ => -1,
                };
                if r > rank {
                    (cmap, rank) = (Some(off), r);
                }
            }
        }
        let glyf = tables.get(b"glyf").copied();
        let loca = tables.get(b"loca").copied();
        let cff = if glyf.is_none() { tables.get(b"CFF ").and_then(|&c| read_cff(b, c)) } else { None };
        if (glyf.is_none() || loca.is_none()) && cff.is_none() {
            return None;
        }
        Some(Face {
            units_per_em,
            ascender,
            descender,
            height,
            head_flags,
            num_hmetrics,
            hmtx: tables.get(b"hmtx").copied(),
            loca,
            glyf,
            long_loca,
            cmap,
            cff,
            gsub: tables.get(b"GSUB").copied(),
            gpos: tables.get(b"GPOS").copied(),
            gdef: tables.get(b"GDEF").copied(),
            kern: tables.get(b"kern").copied(),
            data,
        })
    }

    fn b(&self) -> &[u8] {
        &self.data
    }

    /// The glyph of a code point (0, `.notdef`, for none).
    pub fn glyph_of(&self, c: u32) -> u16 {
        self.glyph_of_opt(c).unwrap_or(0)
    }

    fn glyph_of_opt(&self, c: u32) -> Option<u16> {
        let b = self.b();
        let sub = self.cmap?;
        if u16_at(b, sub)? == 12 {
            let groups = u32_at(b, sub + 12)? as usize;
            for g in 0..groups {
                let rec = sub + 16 + 12 * g;
                let (start, end) = (u32_at(b, rec)?, u32_at(b, rec + 4)?);
                if c >= start && c <= end {
                    return Some((u32_at(b, rec + 8)? + (c - start)) as u16);
                }
            }
            return Some(0);
        }
        if c > 0xFFFF {
            return Some(0);
        }
        let segs = u16_at(b, sub + 6)? as usize / 2;
        for s in 0..segs {
            if c > u16_at(b, sub + 14 + 2 * s)? as u32 {
                continue;
            }
            let start = u16_at(b, sub + 16 + 2 * segs + 2 * s)? as u32;
            if c < start {
                return Some(0);
            }
            let delta = i16_at(b, sub + 16 + 4 * segs + 2 * s)? as i32;
            let range_at = sub + 16 + 6 * segs + 2 * s;
            let range = u16_at(b, range_at)? as usize;
            if range == 0 {
                return Some(((c as i32 + delta) & 0xFFFF) as u16);
            }
            let g = u16_at(b, range_at + range + 2 * (c - start) as usize)? as i32;
            return Some(if g != 0 { ((g + delta) & 0xFFFF) as u16 } else { 0 });
        }
        Some(0)
    }

    /// `hmtx`'s advance of a glyph, font units.
    pub fn advance_of(&self, g: u16) -> u16 {
        match self.hmtx {
            Some(h) if self.num_hmetrics > 0 => u16_at(self.b(), h + 4 * (g.min(self.num_hmetrics - 1) as usize)).unwrap_or(0),
            _ => 0,
        }
    }

    fn glyf_at(&self, g: u16) -> Option<usize> {
        let (glyf, loca) = (self.glyf?, self.loca?);
        let b = self.b();
        let g = g as usize;
        let (off, next) = if self.long_loca {
            (u32_at(b, loca + 4 * g)? as usize, u32_at(b, loca + 4 * g + 4)? as usize)
        } else {
            (u16_at(b, loca + 2 * g)? as usize * 2, u16_at(b, loca + 2 * g + 2)? as usize * 2)
        };
        (next > off).then_some(glyf + off)
    }

    /// A glyph's outline as FreeType loads it (`FT_Load_Glyph( .., FT_LOAD_NO_BITMAP )`), font units, y up: `Ok(None)` for a glyph with no outline (a space),
    /// `Err` for one that cannot be read (KiCad's tofu then).
    pub fn outline_of(&self, g: u16) -> Result<Option<Vec<FtContour>>, ()> {
        if self.glyf.is_some() {
            return self.glyf_outline(g, 0).ok_or(());
        }
        let Some(cff) = &self.cff else { return Err(()) };
        let Some(&cs) = cff.charstrings.get(g as usize) else { return Err(()) };
        let local: &[Span] = match &cff.local {
            CffLocal::One(l) => l,
            CffLocal::Cid { fds, select, format } => {
                let b = self.b();
                let fd = if *format == 0 {
                    u8_at(b, select + 1 + g as usize)
                } else {
                    let n = u16_at(b, select + 1).unwrap_or(0) as usize;
                    (0..n).find_map(|r| {
                        let first = u16_at(b, select + 3 + 3 * r)?;
                        let next = u16_at(b, select + 3 + 3 * (r + 1))?;
                        (g >= first && g < next).then(|| u8_at(b, select + 5 + 3 * r)).flatten()
                    })
                };
                fd.and_then(|f| fds.get(f as usize)).map(|v| &v[..]).unwrap_or(&[])
            }
        };
        type2_outline(self.b(), cs, &cff.gsubrs, local).map(|c| (!c.is_empty()).then_some(c)).ok_or(())
    }

    /// `TT_Load_Simple_Glyph`, `TT_Process_Composite_Component`: a simple glyph's points with their on-curve flags; a composite one its components', each moved
    /// (`ARGS_ARE_XY_VALUES`) and scaled as its record says -- a component placed by matching points is placed at its own origin. `Some(None)` for an empty glyph.
    fn glyf_outline(&self, g: u16, depth: u32) -> Option<Option<Vec<FtContour>>> {
        let b = self.b();
        let Some(p) = self.glyf_at(g) else { return Some(None) };
        if depth > 8 {
            return Some(None);
        }
        let n = i16_at(b, p)?;
        if n >= 0 {
            let n = n as usize;
            let mut ends = Vec::with_capacity(n);
            for i in 0..n {
                ends.push(u16_at(b, p + 10 + 2 * i)? as usize);
            }
            let count = if n > 0 { ends[n - 1] + 1 } else { 0 };
            let mut at = p + 10 + 2 * n;
            at += 2 + u16_at(b, at)? as usize;
            let mut flags: Vec<u8> = Vec::with_capacity(count);
            while flags.len() < count {
                let f = u8_at(b, at)?;
                at += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let r = u8_at(b, at)?;
                    at += 1;
                    for _ in 0..r {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(count);
            let mut xs = Vec::with_capacity(count);
            let mut x = 0i32;
            for &f in &flags {
                if f & 2 != 0 {
                    let d = u8_at(b, at)? as i32;
                    at += 1;
                    x += if f & 16 != 0 { d } else { -d };
                } else if f & 16 == 0 {
                    x += i16_at(b, at)? as i32;
                    at += 2;
                }
                xs.push(x);
            }
            let mut ys = Vec::with_capacity(count);
            let mut y = 0i32;
            for &f in &flags {
                if f & 4 != 0 {
                    let d = u8_at(b, at)? as i32;
                    at += 1;
                    y += if f & 32 != 0 { d } else { -d };
                } else if f & 32 == 0 {
                    y += i16_at(b, at)? as i32;
                    at += 2;
                }
                ys.push(y);
            }
            let mut out = Vec::new();
            let mut first = 0usize;
            for &end in &ends {
                let c: FtContour = (first..=end.min(count.saturating_sub(1)))
                    .filter(|&i| i < count)
                    .map(|i| FtPoint { x: xs[i] as f64, y: ys[i] as f64, tag: if flags[i] & 1 != 0 { Tag::On } else { Tag::Conic } })
                    .collect();
                if !c.is_empty() {
                    out.push(c);
                }
                first = end + 1;
            }
            return Some((!out.is_empty()).then_some(out));
        }
        let mut out = Vec::new();
        let mut at = p + 10;
        loop {
            let flags = u16_at(b, at)?;
            let glyph = u16_at(b, at + 2)?;
            at += 4;
            let (mut dx, mut dy);
            if flags & 1 != 0 {
                (dx, dy) = (i16_at(b, at)? as f64, i16_at(b, at + 2)? as f64);
                at += 4;
            } else {
                (dx, dy) = (u8_at(b, at)? as i8 as f64, u8_at(b, at + 1)? as i8 as f64);
                at += 2;
            }
            if flags & 2 == 0 {
                (dx, dy) = (0.0, 0.0);
            }
            let f2 = |o: usize| i16_at(b, o).map(|v| v as f64 / 16384.0);
            let (mut a, mut bb, mut c, mut d) = (1.0, 0.0, 0.0, 1.0);
            if flags & 8 != 0 {
                a = f2(at)?;
                d = a;
                at += 2;
            } else if flags & 0x40 != 0 {
                (a, d) = (f2(at)?, f2(at + 2)?);
                at += 4;
            } else if flags & 0x80 != 0 {
                (a, bb, c, d) = (f2(at)?, f2(at + 2)?, f2(at + 4)?, f2(at + 6)?);
                at += 8;
            }
            for contour in self.glyf_outline(glyph, depth + 1)?.unwrap_or_default() {
                out.push(contour.into_iter().map(|pt| FtPoint { x: pt.x * a + pt.y * c + dx, y: pt.x * bb + pt.y * d + dy, tag: pt.tag }).collect());
            }
            if flags & 0x20 == 0 {
                break;
            }
        }
        Some((!out.is_empty()).then_some(out))
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- CFF

/// An INDEX of the CFF table: where each of its objects starts and ends, and where the INDEX ends.
fn cff_index(b: &[u8], at: usize) -> Option<(Vec<Span>, usize)> {
    let count = u16_at(b, at)? as usize;
    if count == 0 {
        return Some((Vec::new(), at + 2));
    }
    let size = u8_at(b, at + 2)? as usize;
    let off = |i: usize| -> Option<usize> {
        let mut r = 0usize;
        for k in 0..size {
            r = r * 256 + u8_at(b, at + 3 + i * size + k)? as usize;
        }
        Some(r)
    };
    let base = at + 3 + (count + 1) * size - 1;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        items.push((base + off(i)?, base + off(i + 1)?));
    }
    Some((items, base + off(count)?))
}

/// A CFF DICT: its operators (12 x as 1200 + x) and their operands.
fn cff_dict(b: &[u8], from: usize, to: usize) -> HashMap<u32, Vec<f64>> {
    let mut out = HashMap::new();
    let mut ops: Vec<f64> = Vec::new();
    let mut at = from;
    while at < to {
        let Some(v) = u8_at(b, at) else { break };
        at += 1;
        match v {
            0..=21 => {
                let op = if v == 12 {
                    let x = u8_at(b, at).unwrap_or(0) as u32;
                    at += 1;
                    1200 + x
                } else {
                    v as u32
                };
                out.insert(op, std::mem::take(&mut ops));
            }
            28 => {
                ops.push(i16_at(b, at).unwrap_or(0) as f64);
                at += 2;
            }
            29 => {
                ops.push(i32_at(b, at).unwrap_or(0) as f64);
                at += 4;
            }
            30 => {
                let mut s = String::new();
                'real: loop {
                    let Some(byte) = u8_at(b, at) else { break };
                    at += 1;
                    for n in [byte >> 4, byte & 15] {
                        match n {
                            0..=9 => s.push((b'0' + n) as char),
                            10 => s.push('.'),
                            11 => s.push('E'),
                            12 => s.push_str("E-"),
                            14 => s.push('-'),
                            15 => break 'real,
                            _ => {}
                        }
                    }
                }
                ops.push(s.parse().unwrap_or(0.0));
            }
            32..=246 => ops.push(v as f64 - 139.0),
            247..=250 => {
                ops.push((v as f64 - 247.0) * 256.0 + u8_at(b, at).unwrap_or(0) as f64 + 108.0);
                at += 1;
            }
            251..=254 => {
                ops.push(-(v as f64 - 251.0) * 256.0 - u8_at(b, at).unwrap_or(0) as f64 - 108.0);
                at += 1;
            }
            _ => {}
        }
    }
    out
}

fn subr_bias(n: usize) -> i64 {
    if n < 1240 {
        107
    } else if n < 33900 {
        1131
    } else {
        32768
    }
}

/// The CFF table at `at`: the first font of its Top DICT INDEX, its charstrings, its global and local subroutines (a CID font's per Font DICT, by its FDSelect).
fn read_cff(b: &[u8], at: usize) -> Option<Cff> {
    let hdr = u8_at(b, at + 2)? as usize;
    let (_, names_end) = cff_index(b, at + hdr)?;
    let (tops, tops_end) = cff_index(b, names_end)?;
    let (_, strings_end) = cff_index(b, tops_end)?;
    let (gsubrs, _) = cff_index(b, strings_end)?;
    let top = *tops.first()?;
    let dict = cff_dict(b, top.0, top.1);
    let cs = *dict.get(&17)?.first()? as usize;
    let (charstrings, _) = cff_index(b, at + cs)?;
    let priv_subrs = |d: &HashMap<u32, Vec<f64>>| -> Vec<Span> {
        let Some(p) = d.get(&18).filter(|p| p.len() >= 2) else { return Vec::new() };
        let (size, off) = (p[0] as usize, p[1] as usize);
        let pd = cff_dict(b, at + off, at + off + size);
        match pd.get(&19).and_then(|v| v.first()) {
            Some(&s) => cff_index(b, at + off + s as usize).map(|x| x.0).unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let local = match (dict.get(&1236).and_then(|v| v.first()), dict.get(&1237).and_then(|v| v.first())) {
        (Some(&fa), Some(&fs)) => {
            let fds = cff_index(b, at + fa as usize)?.0.iter().map(|&(a, e)| priv_subrs(&cff_dict(b, a, e))).collect();
            let select = at + fs as usize;
            CffLocal::Cid { fds, select, format: u8_at(b, select)? }
        }
        _ => CffLocal::One(priv_subrs(&dict)),
    };
    Some(Cff { charstrings, gsubrs, local })
}

/// A Type 2 charstring run as FreeType's CFF decoder runs it (`cff_decoder_parse_charstrings`): the contours it draws, a cubic's two control points tagged
/// `Cubic`. `None` when the charstring cannot be read.
fn type2_outline(b: &[u8], cs: Span, gsubrs: &[Span], lsubrs: &[Span]) -> Option<Vec<FtContour>> {
    struct St<'a> {
        b: &'a [u8],
        out: Vec<FtContour>,
        cur: Option<FtContour>,
        x: f64,
        y: f64,
        stack: Vec<f64>,
        n_stems: usize,
        have_width: bool,
        ended: bool,
        gsubrs: &'a [Span],
        lsubrs: &'a [Span],
    }
    impl St<'_> {
        fn close(&mut self) {
            if let Some(mut c) = self.cur.take() {
                if c.len() > 1 {
                    let (a, z) = (c[0], c[c.len() - 1]);
                    if z.tag == Tag::On && a.x == z.x && a.y == z.y {
                        c.pop();
                    }
                }
                if !c.is_empty() {
                    self.out.push(c);
                }
            }
        }
        fn move_to(&mut self, dx: f64, dy: f64) {
            self.close();
            self.x += dx;
            self.y += dy;
            self.cur = Some(vec![FtPoint { x: self.x, y: self.y, tag: Tag::On }]);
        }
        fn line_to(&mut self, dx: f64, dy: f64) {
            self.x += dx;
            self.y += dy;
            let p = FtPoint { x: self.x, y: self.y, tag: Tag::On };
            self.cur.get_or_insert_with(Vec::new).push(p);
        }
        #[allow(clippy::too_many_arguments)]
        fn curve_to(&mut self, dx1: f64, dy1: f64, dx2: f64, dy2: f64, dx3: f64, dy3: f64) {
            let (x1, y1) = (self.x + dx1, self.y + dy1);
            let (x2, y2) = (x1 + dx2, y1 + dy2);
            self.x = x2 + dx3;
            self.y = y2 + dy3;
            let (x, y) = (self.x, self.y);
            let c = self.cur.get_or_insert_with(Vec::new);
            c.push(FtPoint { x: x1, y: y1, tag: Tag::Cubic });
            c.push(FtPoint { x: x2, y: y2, tag: Tag::Cubic });
            c.push(FtPoint { x, y, tag: Tag::On });
        }
        fn stems(&mut self) {
            if !self.have_width && self.stack.len() % 2 == 1 {
                self.stack.remove(0);
            }
            self.have_width = true;
            self.n_stems += self.stack.len() >> 1;
            self.stack.clear();
        }
        fn s(&self, i: usize) -> f64 {
            self.stack.get(i).copied().unwrap_or(0.0)
        }
        fn run(&mut self, from: usize, to: usize, depth: u32) -> Option<()> {
            let mut at = from;
            while at < to && !self.ended {
                let v = u8_at(self.b, at)?;
                at += 1;
                if v >= 32 || v == 28 {
                    if v == 28 {
                        self.stack.push(i16_at(self.b, at)? as f64);
                        at += 2;
                    } else if v <= 246 {
                        self.stack.push(v as f64 - 139.0);
                    } else if v <= 250 {
                        self.stack.push((v as f64 - 247.0) * 256.0 + u8_at(self.b, at)? as f64 + 108.0);
                        at += 1;
                    } else if v <= 254 {
                        self.stack.push(-(v as f64 - 251.0) * 256.0 - u8_at(self.b, at)? as f64 - 108.0);
                        at += 1;
                    } else {
                        self.stack.push(i32_at(self.b, at)? as f64 / 65536.0);
                        at += 4;
                    }
                    continue;
                }
                match v {
                    1 | 3 | 18 | 23 => self.stems(),
                    19 | 20 => {
                        self.stems();
                        at += (self.n_stems + 7) >> 3;
                    }
                    21 => {
                        if !self.have_width && self.stack.len() > 2 {
                            self.stack.remove(0);
                        }
                        self.have_width = true;
                        let (a, c) = (self.s(0), self.s(1));
                        self.move_to(a, c);
                        self.stack.clear();
                    }
                    22 | 4 => {
                        if !self.have_width && self.stack.len() > 1 {
                            self.stack.remove(0);
                        }
                        self.have_width = true;
                        let a = self.s(0);
                        if v == 22 {
                            self.move_to(a, 0.0);
                        } else {
                            self.move_to(0.0, a);
                        }
                        self.stack.clear();
                    }
                    5 => {
                        let s = std::mem::take(&mut self.stack);
                        for p in s.chunks_exact(2) {
                            self.line_to(p[0], p[1]);
                        }
                    }
                    6 | 7 => {
                        let s = std::mem::take(&mut self.stack);
                        let mut horizontal = v == 6;
                        for d in s {
                            if horizontal {
                                self.line_to(d, 0.0);
                            } else {
                                self.line_to(0.0, d);
                            }
                            horizontal = !horizontal;
                        }
                    }
                    8 => {
                        let s = std::mem::take(&mut self.stack);
                        for p in s.chunks_exact(6) {
                            self.curve_to(p[0], p[1], p[2], p[3], p[4], p[5]);
                        }
                    }
                    24 => {
                        let s = std::mem::take(&mut self.stack);
                        let mut i = 0;
                        while i + 5 < s.len().saturating_sub(2) {
                            self.curve_to(s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]);
                            i += 6;
                        }
                        self.line_to(s.get(i).copied().unwrap_or(0.0), s.get(i + 1).copied().unwrap_or(0.0));
                    }
                    25 => {
                        let s = std::mem::take(&mut self.stack);
                        let mut i = 0;
                        while i + 1 < s.len().saturating_sub(6) {
                            self.line_to(s[i], s[i + 1]);
                            i += 2;
                        }
                        let g = |k: usize| s.get(i + k).copied().unwrap_or(0.0);
                        self.curve_to(g(0), g(1), g(2), g(3), g(4), g(5));
                    }
                    26 | 27 => {
                        let s = std::mem::take(&mut self.stack);
                        let mut i = 0;
                        let mut d1 = 0.0;
                        if s.len() % 2 == 1 {
                            d1 = s[0];
                            i = 1;
                        }
                        while i + 3 < s.len() {
                            if v == 26 {
                                self.curve_to(d1, s[i], s[i + 1], s[i + 2], 0.0, s[i + 3]);
                            } else {
                                self.curve_to(s[i], d1, s[i + 1], s[i + 2], s[i + 3], 0.0);
                            }
                            d1 = 0.0;
                            i += 4;
                        }
                    }
                    30 | 31 => {
                        // vhcurveto / hvcurveto: the curves alternate, the last one taking an odd argument for its end's other coordinate
                        let s = std::mem::take(&mut self.stack);
                        let mut vertical = v == 30;
                        let mut i = 0;
                        while i + 3 < s.len() {
                            let last = if s.len() - (i + 4) == 1 { s[i + 4] } else { 0.0 };
                            if vertical {
                                self.curve_to(0.0, s[i], s[i + 1], s[i + 2], s[i + 3], last);
                            } else {
                                self.curve_to(s[i], 0.0, s[i + 1], s[i + 2], last, s[i + 3]);
                            }
                            i += 4;
                            vertical = !vertical;
                        }
                    }
                    10 | 29 => {
                        let subrs = if v == 10 { self.lsubrs } else { self.gsubrs };
                        let idx = self.stack.pop().unwrap_or(0.0) as i64 + subr_bias(subrs.len());
                        if let Some(&(a, e)) = usize::try_from(idx).ok().and_then(|i| subrs.get(i)) {
                            if depth < 10 {
                                self.run(a, e, depth + 1)?;
                            }
                        }
                    }
                    11 => return Some(()),
                    14 => {
                        self.close();
                        self.ended = true;
                        return Some(());
                    }
                    12 => {
                        let op = u8_at(self.b, at)?;
                        at += 1;
                        let s = std::mem::take(&mut self.stack);
                        // the flex operators (Type 2 Charstring Format, 4.1): two curves each
                        if op == 35 && s.len() >= 12 {
                            self.curve_to(s[0], s[1], s[2], s[3], s[4], s[5]);
                            self.curve_to(s[6], s[7], s[8], s[9], s[10], s[11]);
                        } else if op == 34 && s.len() >= 7 {
                            self.curve_to(s[0], 0.0, s[1], s[2], s[3], 0.0);
                            self.curve_to(s[4], 0.0, s[5], -s[2], s[6], 0.0);
                        } else if op == 36 && s.len() >= 9 {
                            self.curve_to(s[0], s[1], s[2], s[3], s[4], 0.0);
                            self.curve_to(s[5], 0.0, s[6], s[7], s[8], -(s[1] + s[3] + s[7]));
                        } else if op == 37 && s.len() >= 11 {
                            let dx = s[0] + s[2] + s[4] + s[6] + s[8];
                            let dy = s[1] + s[3] + s[5] + s[7] + s[9];
                            self.curve_to(s[0], s[1], s[2], s[3], s[4], s[5]);
                            if dx.abs() > dy.abs() {
                                self.curve_to(s[6], s[7], s[8], s[9], s[10], -dy);
                            } else {
                                self.curve_to(s[6], s[7], s[8], s[9], -dx, s[10]);
                            }
                        }
                    }
                    _ => self.stack.clear(),
                }
            }
            Some(())
        }
    }
    let mut st = St { b, out: Vec::new(), cur: None, x: 0.0, y: 0.0, stack: Vec::new(), n_stems: 0, have_width: false, ended: false, gsubrs, lsubrs };
    st.run(cs.0, cs.1, 0)?;
    st.close();
    Some(st.out)
}

// ------------------------------------------------------------------------------------------------------------------------------- shaping

/// One glyph of a shaped run: the glyph and HarfBuzz's `x_advance` in font units (the `hmtx` advance plus the kerning; a mark's zero).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaped {
    pub glyph: u16,
    pub advance: f64,
}

fn coverage(b: &[u8], at: usize, g: u16) -> Option<usize> {
    match u16_at(b, at)? {
        1 => {
            let n = u16_at(b, at + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let v = u16_at(b, at + 4 + 2 * mid)?;
                if v == g {
                    return Some(mid);
                }
                if v < g {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            None
        }
        2 => {
            let n = u16_at(b, at + 2)? as usize;
            for r in 0..n {
                let rec = at + 4 + 6 * r;
                let (s, e) = (u16_at(b, rec)?, u16_at(b, rec + 2)?);
                if g >= s && g <= e {
                    return Some(u16_at(b, rec + 4)? as usize + (g - s) as usize);
                }
            }
            None
        }
        _ => None,
    }
}

fn class_of(b: &[u8], at: usize, g: u16) -> u16 {
    let f = || -> Option<u16> {
        match u16_at(b, at)? {
            1 => {
                let start = u16_at(b, at + 2)?;
                let n = u16_at(b, at + 4)?;
                if g >= start && g < start.saturating_add(n) {
                    return u16_at(b, at + 6 + 2 * (g - start) as usize);
                }
                Some(0)
            }
            2 => {
                let n = u16_at(b, at + 2)? as usize;
                for r in 0..n {
                    let rec = at + 4 + 6 * r;
                    if g >= u16_at(b, rec)? && g <= u16_at(b, rec + 2)? {
                        return u16_at(b, rec + 4);
                    }
                }
                Some(0)
            }
            _ => Some(0),
        }
    };
    f().unwrap_or(0)
}

/// A lookup of GSUB or GPOS: its type (an extension's resolved), flag and subtables.
struct Lookup {
    kind: u16,
    flag: u16,
    subtables: Vec<usize>,
}

impl Face {
    /// GDEF's glyph class 3: a mark.
    fn is_mark(&self, g: u16) -> bool {
        let Some(gdef) = self.gdef else { return false };
        match u16_at(self.b(), gdef + 4) {
            Some(off) if off != 0 => class_of(self.b(), gdef + off as usize, g) == 3,
            _ => false,
        }
    }

    /// The lookups of `features` for the run's script (`latn` for a run with a Latin letter, else `DFLT`; then whichever there is) and the default language
    /// system, the required feature's too, in lookup-list order (`hb_ot_map_builder_t`'s stage, collapsed).
    fn lookups(&self, table: usize, features: &[&[u8; 4]], latin: bool, ext_kind: u16) -> Vec<Lookup> {
        let b = self.b();
        let f = || -> Option<Vec<Lookup>> {
            let scripts = table + u16_at(b, table + 4)? as usize;
            let feats = table + u16_at(b, table + 6)? as usize;
            let lookups = table + u16_at(b, table + 8)? as usize;
            let n = u16_at(b, scripts)? as usize;
            let find = |tag: &[u8; 4]| (0..n).find(|&i| b.get(scripts + 2 + 6 * i..scripts + 6 + 6 * i) == Some(&tag[..]));
            let order: [&[u8; 4]; 3] = if latin { [b"latn", b"DFLT", b"dflt"] } else { [b"DFLT", b"dflt", b"latn"] };
            let si = order.iter().find_map(|t| find(t)).or(if n > 0 { Some(0) } else { None })?;
            let script = scripts + u16_at(b, scripts + 2 + 6 * si + 4)? as usize;
            let lang_off = u16_at(b, script)? as usize;
            if lang_off == 0 {
                return Some(Vec::new());
            }
            let lang = script + lang_off;
            let mut idx: Vec<u16> = Vec::new();
            let required = u16_at(b, lang + 2)?;
            let mut feature_ids: Vec<u16> = (0..u16_at(b, lang + 4)? as usize).filter_map(|i| u16_at(b, lang + 6 + 2 * i)).collect();
            if required != 0xFFFF {
                feature_ids.push(required);
            }
            for fi in feature_ids {
                let rec = feats + 2 + 6 * fi as usize;
                let tag = b.get(rec..rec + 4)?;
                if fi != required && !features.iter().any(|t| &t[..] == tag) {
                    continue;
                }
                let ft = feats + u16_at(b, rec + 4)? as usize;
                for k in 0..u16_at(b, ft + 2)? as usize {
                    idx.push(u16_at(b, ft + 4 + 2 * k)?);
                }
            }
            idx.sort_unstable();
            idx.dedup();
            let mut out = Vec::new();
            for li in idx {
                let l = lookups + u16_at(b, lookups + 2 + 2 * li as usize)? as usize;
                let mut kind = u16_at(b, l)?;
                let flag = u16_at(b, l + 2)?;
                let mut subtables = Vec::new();
                for s in 0..u16_at(b, l + 4)? as usize {
                    let mut st = l + u16_at(b, l + 6 + 2 * s)? as usize;
                    if kind == ext_kind {
                        let k = u16_at(b, st + 2)?;
                        st += u32_at(b, st + 4)? as usize;
                        if subtables.is_empty() {
                            kind = k;
                        }
                    }
                    subtables.push(st);
                }
                out.push(Lookup { kind, flag, subtables });
            }
            Some(out)
        };
        f().unwrap_or_default()
    }

    fn skip(&self, flag: u16, g: u16) -> bool {
        flag & 0x8 != 0 && self.is_mark(g)
    }

    /// GSUB's single (type 1) and ligature (type 4) substitutions.
    fn substitute(&self, glyphs: &mut Vec<u16>, latin: bool) {
        let Some(gsub) = self.gsub else { return };
        let b = self.b();
        for lk in self.lookups(gsub, &[b"ccmp", b"rlig", b"liga", b"clig"], latin, 7) {
            let mut i = 0;
            while i < glyphs.len() {
                if self.skip(lk.flag, glyphs[i]) {
                    i += 1;
                    continue;
                }
                let mut done = false;
                for &st in &lk.subtables {
                    let applied = (|| -> Option<bool> {
                        let format = u16_at(b, st)?;
                        let cov = coverage(b, st + u16_at(b, st + 2)? as usize, glyphs[i]);
                        let Some(ci) = cov else { return Some(false) };
                        match (lk.kind, format) {
                            (1, 1) => {
                                glyphs[i] = (glyphs[i] as i32 + i16_at(b, st + 4)? as i32) as u16;
                                Some(true)
                            }
                            (1, 2) => {
                                glyphs[i] = u16_at(b, st + 6 + 2 * ci)?;
                                Some(true)
                            }
                            (4, 1) => {
                                let set = st + u16_at(b, st + 6 + 2 * ci)? as usize;
                                for l in 0..u16_at(b, set)? as usize {
                                    let lig = set + u16_at(b, set + 2 + 2 * l)? as usize;
                                    let out_glyph = u16_at(b, lig)?;
                                    let comps = u16_at(b, lig + 2)? as usize;
                                    let mut at = i;
                                    let mut matched = vec![i];
                                    let mut ok = true;
                                    for c in 1..comps {
                                        at += 1;
                                        while at < glyphs.len() && self.skip(lk.flag, glyphs[at]) {
                                            at += 1;
                                        }
                                        if at >= glyphs.len() || glyphs[at] != u16_at(b, lig + 4 + 2 * (c - 1))? {
                                            ok = false;
                                            break;
                                        }
                                        matched.push(at);
                                    }
                                    if ok {
                                        glyphs[i] = out_glyph;
                                        for &m in matched[1..].iter().rev() {
                                            glyphs.remove(m);
                                        }
                                        return Some(true);
                                    }
                                }
                                Some(false)
                            }
                            _ => Some(false),
                        }
                    })()
                    .unwrap_or(false);
                    if applied {
                        done = true;
                        break;
                    }
                }
                let _ = done;
                i += 1;
            }
        }
    }

    fn value_size(vf: u16) -> usize {
        2 * (vf & 0xFF).count_ones() as usize
    }

    /// A ValueRecord's XAdvance (format bit 0x4).
    fn x_advance(&self, at: usize, vf: u16) -> f64 {
        if vf & 0x4 == 0 {
            return 0.0;
        }
        let before = 2 * (vf & 0x3).count_ones() as usize;
        i16_at(self.b(), at + before).unwrap_or(0) as f64
    }

    /// GPOS's pair adjustment (type 2, formats 1 and 2) of `kern`: `adv` gets each pair's first and second value's XAdvance.
    fn kern_gpos(&self, glyphs: &[u16], adv: &mut [f64], latin: bool) {
        let Some(gpos) = self.gpos else { return };
        let b = self.b();
        for lk in self.lookups(gpos, &[b"kern"], latin, 9) {
            if lk.kind != 2 {
                continue;
            }
            let mut i = 0;
            while i < glyphs.len() {
                if self.skip(lk.flag, glyphs[i]) {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                while j < glyphs.len() && self.skip(lk.flag, glyphs[j]) {
                    j += 1;
                }
                if j >= glyphs.len() {
                    break;
                }
                let mut next = j;
                for &st in &lk.subtables {
                    let hit = (|| -> Option<Option<(f64, f64, bool)>> {
                        let format = u16_at(b, st)?;
                        let Some(ci) = coverage(b, st + u16_at(b, st + 2)? as usize, glyphs[i]) else { return Some(None) };
                        let (vf1, vf2) = (u16_at(b, st + 4)?, u16_at(b, st + 6)?);
                        let (s1, s2) = (Self::value_size(vf1), Self::value_size(vf2));
                        match format {
                            1 => {
                                let set = st + u16_at(b, st + 10 + 2 * ci)? as usize;
                                let n = u16_at(b, set)? as usize;
                                let rec_size = 2 + s1 + s2;
                                for r in 0..n {
                                    let rec = set + 2 + rec_size * r;
                                    if u16_at(b, rec)? == glyphs[j] {
                                        return Some(Some((self.x_advance(rec + 2, vf1), self.x_advance(rec + 2 + s1, vf2), vf2 != 0)));
                                    }
                                }
                                Some(None)
                            }
                            2 => {
                                let c1 = class_of(b, st + u16_at(b, st + 8)? as usize, glyphs[i]) as usize;
                                let cd2 = st + u16_at(b, st + 10)? as usize;
                                if coverage(b, st + u16_at(b, st + 2)? as usize, glyphs[i]).is_none() {
                                    return Some(None);
                                }
                                let c2 = class_of(b, cd2, glyphs[j]) as usize;
                                let (n1, n2) = (u16_at(b, st + 12)? as usize, u16_at(b, st + 14)? as usize);
                                if c1 >= n1 || c2 >= n2 {
                                    return Some(None);
                                }
                                let rec = st + 16 + (c1 * n2 + c2) * (s1 + s2);
                                Some(Some((self.x_advance(rec, vf1), self.x_advance(rec + s1, vf2), vf2 != 0)))
                            }
                            _ => Some(None),
                        }
                    })()
                    .flatten();
                    if let Some((a1, a2, has2)) = hit {
                        adv[i] += a1;
                        adv[j] += a2;
                        if has2 {
                            next = j + 1;
                        }
                        break;
                    }
                }
                i = next;
            }
        }
    }

    /// The legacy `kern` table's horizontal format 0 pairs (HarfBuzz's fallback for a font with no GPOS).
    fn kern_legacy(&self, glyphs: &[u16], adv: &mut [f64]) {
        let Some(k) = self.kern else { return };
        let b = self.b();
        let mut f = || -> Option<()> {
            if u16_at(b, k)? != 0 {
                return Some(());
            }
            let mut at = k + 4;
            for _ in 0..u16_at(b, k + 2)? {
                let len = u16_at(b, at + 2)? as usize;
                let cov = u16_at(b, at + 4)?;
                if cov >> 8 == 0 && cov & 1 != 0 && cov & 0x4 == 0 {
                    let n = u16_at(b, at + 6)? as usize;
                    for i in 0..glyphs.len().saturating_sub(1) {
                        let key = ((glyphs[i] as u32) << 16) | glyphs[i + 1] as u32;
                        let (mut lo, mut hi) = (0usize, n);
                        while lo < hi {
                            let mid = (lo + hi) / 2;
                            let rec = at + 14 + 6 * mid;
                            let v = u32_at(b, rec)?;
                            if v == key {
                                adv[i] += i16_at(b, rec + 4)? as f64;
                                break;
                            }
                            if v < key {
                                lo = mid + 1;
                            } else {
                                hi = mid;
                            }
                        }
                    }
                }
                at += len;
            }
            Some(())
        };
        let _ = f();
    }

    /// `hb_shape` of a run, as far as it is done here (the module's doc): the glyphs and their advances, font units.
    pub fn shape(&self, text: &str) -> Vec<Shaped> {
        let latin = text.chars().any(|c| c.is_alphabetic() && (c as u32) < 0x0250);
        let mut glyphs: Vec<u16> = text.chars().map(|c| self.glyph_of(c as u32)).collect();
        self.substitute(&mut glyphs, latin);
        let mut adv: Vec<f64> = glyphs.iter().map(|&g| self.advance_of(g) as f64).collect();
        if self.gpos.is_some() {
            self.kern_gpos(&glyphs, &mut adv, latin);
            for (i, &g) in glyphs.iter().enumerate() {
                if self.is_mark(g) {
                    adv[i] = 0.0;
                }
            }
        } else {
            self.kern_legacy(&glyphs, &mut adv);
        }
        glyphs.into_iter().zip(adv).map(|(glyph, advance)| Shaped { glyph, advance }).collect()
    }

    /// The 26.6 pixels to the em FreeType sets for `FT_Set_Char_Size( face, 0, scaler, 1152, 0 )`: `scaler * 1152 / 72`, whole pixels for a TrueType font whose
    /// `head.flags` bit 3 asks for rounded ppems (`tt_size_reset`).
    pub fn ppem26(&self, scaler: i64) -> i64 {
        let p = scaler * 16;
        if self.glyf.is_some() && self.head_flags & 8 != 0 {
            ((p + 32) >> 6) << 6
        } else {
            p
        }
    }

    /// The size's `metrics.ascender` and `descender` at `scaler`, 26.6 pixels: `FT_PIX_ROUND` for a TrueType font, `FT_PIX_CEIL` / `FT_PIX_FLOOR` for another.
    pub fn size_ascender_descender(&self, scaler: i64) -> (i64, i64) {
        let s = self.ppem26(scaler) as f64 / self.units_per_em.max(1) as f64;
        let (a, d) = (self.ascender as f64 * s, self.descender as f64 * s);
        if self.glyf.is_some() {
            (((a + 32.0) / 64.0).floor() as i64 * 64, ((d + 32.0) / 64.0).floor() as i64 * 64)
        } else {
            ((a / 64.0).ceil() as i64 * 64, (d / 64.0).floor() as i64 * 64)
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- outlines

/// `FT_Outline_Get_Orientation`: the sign of the area the control points span, y up -- `1` TrueType (clockwise), `-1` PostScript (counter-clockwise), `0` none.
fn ft_orientation(contours: &[FtContour]) -> i32 {
    let mut area = 0.0;
    for c in contours {
        let Some(mut prev) = c.last().copied() else { continue };
        for &cur in c {
            area += (cur.y - prev.y) * (cur.x + prev.x);
            prev = cur;
        }
    }
    if area > 0.0 {
        -1
    } else if area < 0.0 {
        1
    } else {
        0
    }
}

/// `FT_Outline_EmboldenXY( outline, strength, strength )` (FreeType's `src/base/ftoutln.c`): every point moved half the strength up and right and along the
/// bisector of its two edges, outwards, by up to half the strength (`web/studio/src/kicad-port/sfnt.ts::ftEmbolden`).
fn ft_embolden(contours: &[FtContour], strength: f64) -> Vec<FtContour> {
    let (xs, ys) = (strength / 2.0, strength / 2.0);
    let orientation = ft_orientation(contours);
    if orientation == 0 || strength == 0.0 {
        return contours.to_vec();
    }
    contours
        .iter()
        .map(|src| {
            let mut pts = src.clone();
            let n = pts.len();
            if n == 0 {
                return pts;
            }
            let last = n - 1;
            let mut l_in = 0.0;
            let mut in_v = (0.0, 0.0);
            let mut anchor = (0.0, 0.0);
            let mut l_anchor = 0.0;
            let (mut i, mut j, mut k): (usize, usize, isize) = (last, 0, -1);
            while j != i && i as isize != k {
                let (out, l_out);
                if j as isize != k {
                    let (ox, oy) = (src[j].x - src[i].x, src[j].y - src[i].y);
                    let l = ox.hypot(oy);
                    if l == 0.0 {
                        j = if j < last { j + 1 } else { 0 };
                        continue;
                    }
                    out = (ox / l, oy / l);
                    l_out = l;
                } else {
                    out = anchor;
                    l_out = l_anchor;
                }
                if l_in != 0.0 {
                    if k < 0 {
                        k = i as isize;
                        anchor = in_v;
                        l_anchor = l_in;
                    }
                    let mut d = in_v.0 * out.0 + in_v.1 * out.1;
                    let (mut sx, mut sy) = (0.0, 0.0);
                    // "shift only if turn is less than ~160 degrees"
                    if d > -0.9375 {
                        d += 1.0;
                        sx = in_v.1 + out.1;
                        sy = in_v.0 + out.0;
                        if orientation == 1 {
                            sx = -sx;
                        } else {
                            sy = -sy;
                        }
                        let mut q = out.0 * in_v.1 - out.1 * in_v.0;
                        if orientation == 1 {
                            q = -q;
                        }
                        let l = f64::min(l_in, l_out);
                        sx = if xs * q <= l * d { sx * xs / d } else { sx * l / q };
                        sy = if ys * q <= l * d { sy * ys / d } else { sy * l / q };
                    }
                    while i != j {
                        pts[i].x = src[i].x + xs + sx;
                        pts[i].y = src[i].y + ys + sy;
                        i = if i < last { i + 1 } else { 0 };
                    }
                } else {
                    i = j;
                }
                in_v = out;
                l_in = l_out;
                j = if j < last { j + 1 } else { 0 };
            }
            pts
        })
        .collect()
}

/// `FT_Outline_Decompose` through `OUTLINE_DECOMPOSER` (`common/font/outline_decomposer.cpp`): each contour from its first on-curve point (or the middle of its
/// first and last conic points), a line to every on-curve point, a conic or cubic flattened by `BEZIER_POLY::GetPoly( .., m_FontErrorSize )`, a repeated point
/// left out; the closing point dropped.
fn ft_decompose(contours: &[FtContour]) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let flatten = |ctrl: &[(f64, f64)]| bezier_poly_d(ctrl, FONT_ERROR_SIZE);
    for c in contours {
        if c.is_empty() || c[0].tag == Tag::Cubic {
            continue;
        }
        let mut pts: Vec<(f64, f64)> = Vec::new();
        let add = |pts: &mut Vec<(f64, f64)>, p: (f64, f64)| {
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        };
        let mut first: isize = 0;
        let mut limit = c.len() as isize - 1;
        let mut start = (c[0].x, c[0].y);
        if c[0].tag == Tag::Conic {
            let lp = c[limit as usize];
            if lp.tag == Tag::On {
                start = (lp.x, lp.y);
                limit -= 1;
            } else {
                start = ((c[0].x + lp.x) / 2.0, (c[0].y + lp.y) / 2.0);
            }
            first = -1;
        }
        add(&mut pts, start);
        let mut last = start;
        let mut i = first;
        let mut closed = false;
        while i < limit {
            i += 1;
            let p = c[i as usize];
            match p.tag {
                Tag::On => {
                    last = (p.x, p.y);
                    add(&mut pts, last);
                }
                Tag::Conic => {
                    let mut control = (p.x, p.y);
                    loop {
                        if i < limit {
                            i += 1;
                            let q = c[i as usize];
                            if q.tag == Tag::On {
                                for r in flatten(&[last, control, (q.x, q.y)]) {
                                    add(&mut pts, r);
                                }
                                last = (q.x, q.y);
                                break;
                            }
                            let m = ((control.0 + q.x) / 2.0, (control.1 + q.y) / 2.0);
                            for r in flatten(&[last, control, m]) {
                                add(&mut pts, r);
                            }
                            last = m;
                            control = (q.x, q.y);
                            continue;
                        }
                        for r in flatten(&[last, control, start]) {
                            add(&mut pts, r);
                        }
                        closed = true;
                        break;
                    }
                    if closed {
                        break;
                    }
                }
                Tag::Cubic => {
                    let c1 = (p.x, p.y);
                    let Some(p2) = c.get(i as usize + 1) else { break };
                    let c2 = (p2.x, p2.y);
                    i += 2;
                    if i <= limit {
                        let e = (c[i as usize].x, c[i as usize].y);
                        for r in flatten(&[last, c1, c2, e]) {
                            add(&mut pts, r);
                        }
                        last = e;
                        continue;
                    }
                    for r in flatten(&[last, c1, c2, start]) {
                        add(&mut pts, r);
                    }
                    closed = true;
                    break;
                }
            }
        }
        if !closed {
            add(&mut pts, start);
        }
        if pts.len() > 1 && pts[0] == pts[pts.len() - 1] {
            pts.pop();
        }
        if !pts.is_empty() {
            out.push(pts);
        }
    }
    out
}

/// `OUTLINE_DECOMPOSER::winding`: `1` for a clockwise contour (y up), `-1` counter-clockwise, `0` for none.
fn winding(c: &[(f64, f64)]) -> i32 {
    if c.len() < 2 {
        return 0;
    }
    let mut sum = 0.0;
    for i in 0..c.len() {
        let (p1, p2) = (c[i], c[(i + 1) % c.len()]);
        sum += (p2.0 - p1.0) * (p2.1 + p1.1);
    }
    if sum > 0.0 {
        1
    } else if sum < 0.0 {
        -1
    } else {
        0
    }
}

/// One glyph polygon of a text: an outline and its holes, nanometres (`OUTLINE_GLYPH`, a `SHAPE_POLY_SET`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlyphPoly {
    pub outline: Vec<[i64; 2]>,
    pub holes: Vec<Vec<[i64; 2]>>,
}

/// `SHAPE_LINE_CHAIN::PointInside` (even-odd).
pub fn point_inside(c: &[[i64; 2]], p: [i64; 2]) -> bool {
    let mut inside = false;
    let n = c.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (c[i][0] as f64, c[i][1] as f64);
        let (xj, yj) = (c[j][0] as f64, c[j][1] as f64);
        if (yi > p[1] as f64) != (yj > p[1] as f64) && (p[0] as f64) < (xj - xi) * (p[1] as f64 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn area2(c: &[[i64; 2]]) -> i128 {
    let n = c.len();
    (0..n).map(|i| c[i][0] as i128 * c[(i + 1) % n][1] as i128 - c[(i + 1) % n][0] as i128 * c[i][1] as i128).sum()
}

fn cross3(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    (b[0] - a[0]) as i128 * (c[1] - a[1]) as i128 - (b[1] - a[1]) as i128 * (c[0] - a[0]) as i128
}

fn segs_cross(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    if a == c || a == d || b == c || b == d {
        return false;
    }
    let (d1, d2, d3, d4) = (cross3(c, d, a), cross3(c, d, b), cross3(a, b, c), cross3(a, b, d));
    ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
}

/// A glyph polygon in triangles (`OUTLINE_GLYPH::Triangulate`, what `CALLBACK_GAL` hands `EDA_TEXT::GetEffectiveTextShape` as `SHAPE_SIMPLE`s): its holes
/// bridged into the outline (`SHAPE_POLY_SET::Fracture`), then ears clipped. KiCad's `POLYGON_TRIANGULATION` (an ear cutter with its own order) may cut other
/// diagonals; the triangles' corners are the polygon's own either way.
pub fn triangulate(g: &GlyphPoly) -> Vec<[[i64; 2]; 3]> {
    let mut ring: Vec<[i64; 2]> = g.outline.clone();
    ring.dedup();
    if ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }
    if area2(&ring) < 0 {
        ring.reverse();
    }
    let mut holes: Vec<Vec<[i64; 2]>> = g
        .holes
        .iter()
        .filter(|h| h.len() >= 3)
        .map(|h| {
            let mut h = h.clone();
            if area2(&h) > 0 {
                h.reverse();
            }
            h
        })
        .collect();
    holes.sort_by_key(|h| std::cmp::Reverse(h.iter().map(|p| p[0]).max().unwrap_or(0)));
    for hi in 0..holes.len() {
        let hole = holes[hi].clone();
        let (hk, &hp) = hole.iter().enumerate().max_by_key(|(_, p)| (p[0], -p[1])).expect("three points");
        // the nearest ring vertex the bridge reaches without crossing an edge of the ring or of a hole still to come
        let mut best: Option<(i128, usize)> = None;
        for (rk, &rp) in ring.iter().enumerate() {
            let d = (rp[0] - hp[0]) as i128 * (rp[0] - hp[0]) as i128 + (rp[1] - hp[1]) as i128 * (rp[1] - hp[1]) as i128;
            if best.is_some_and(|b| b.0 <= d) {
                continue;
            }
            let n = ring.len();
            let blocked = (0..n).any(|i| segs_cross(hp, rp, ring[i], ring[(i + 1) % n]))
                || holes[hi..].iter().any(|h| (0..h.len()).any(|i| segs_cross(hp, rp, h[i], h[(i + 1) % h.len()])));
            if !blocked {
                best = Some((d, rk));
            }
        }
        let Some((_, rk)) = best else { continue };
        let mut joined: Vec<[i64; 2]> = ring[..=rk].to_vec();
        for k in 0..=hole.len() {
            joined.push(hole[(hk + k) % hole.len()]);
        }
        joined.push(ring[rk]);
        joined.extend_from_slice(&ring[rk + 1..]);
        ring = joined;
    }
    let mut idx: Vec<usize> = (0..ring.len()).collect();
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < 4 * ring.len() * ring.len() + 16 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (a, b, c) = (ring[idx[(i + n - 1) % n]], ring[idx[i]], ring[idx[(i + 1) % n]]);
            if cross3(a, b, c) <= 0 {
                continue;
            }
            let inside = idx.iter().any(|&k| {
                let p = ring[k];
                p != a && p != b && p != c && cross3(a, b, p) >= 0 && cross3(b, c, p) >= 0 && cross3(c, a, p) >= 0
            });
            if !inside {
                out.push([a, b, c]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // a degenerate remainder (collinear or touching points): cut the flattest corner off
            let i = (0..n).min_by_key(|&i| cross3(ring[idx[(i + n - 1) % n]], ring[idx[i]], ring[idx[(i + 1) % n]]).abs()).unwrap_or(0);
            let (a, b, c) = (ring[idx[(i + n - 1) % n]], ring[idx[i]], ring[idx[(i + 1) % n]]);
            if cross3(a, b, c) != 0 {
                out.push([a, b, c]);
            }
            idx.remove(i);
        }
    }
    if idx.len() == 3 && cross3(ring[idx[0]], ring[idx[1]], ring[idx[2]]) != 0 {
        out.push([ring[idx[0]], ring[idx[1]], ring[idx[2]]]);
    }
    out
}

/// A glyph's contours as `GLYPH_DATA` keeps them: flattened, in `OUTLINE_DECOMPOSER`'s units, each filled (`contourIsFilled`) or a hole.
struct GlyphData {
    contours: Vec<(Vec<(f64, f64)>, bool)>,
}

/// A text's box as `BOX2I::Merge` grows it (unset until the first merge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BBox {
    pub init: bool,
    pub min: [i64; 2],
    pub max: [i64; 2],
}

impl BBox {
    pub fn at(p: [i64; 2]) -> BBox {
        BBox { init: true, min: p, max: p }
    }
    pub fn merge_point(&mut self, p: [i64; 2]) {
        if !self.init {
            *self = BBox::at(p);
            return;
        }
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }
    pub fn merge(&mut self, o: &BBox) {
        if !o.init {
            return;
        }
        if !self.init {
            *self = *o;
            return;
        }
        self.merge_point(o.min);
        self.merge_point(o.max);
    }
    pub fn size(&self) -> [i64; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

/// A run's style: a subscript, a superscript (`TEXT_STYLE::SUBSCRIPT`, `SUPERSCRIPT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunStyle {
    pub sub: bool,
    pub sup: bool,
}

/// `RotatePoint( VECTOR2D&, aCentre, aAngle )`: exact on the multiples of 90 degrees.
pub fn rotate_d(p: [f64; 2], c: [f64; 2], degrees: f64) -> [f64; 2] {
    let mut a = degrees % 360.0;
    if a < 0.0 {
        a += 360.0;
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
        let (s, co) = a.to_radians().sin_cos();
        (oy * s + ox * co, oy * co - ox * s)
    };
    [x + c[0], y + c[1]]
}

/// A loaded outline font: the face fontconfig matched for a text's face, bold and italic, and whether its bold or italic is faked.
pub struct OutlineFont {
    pub face: Face,
    pub path: PathBuf,
    pub index: u32,
    pub fake_bold: bool,
    pub fake_ital: bool,
    glyphs: Mutex<HashMap<(u16, bool), Arc<GlyphData>>>,
}

impl std::fmt::Debug for OutlineFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutlineFont").field("path", &self.path).field("index", &self.index).field("fake_bold", &self.fake_bold).field("fake_ital", &self.fake_ital).finish()
    }
}

/// `FONT::GetFont`'s stroke test: no face, or one starting with "KiCad Font".
pub fn is_stroke_face(face: Option<&str>) -> bool {
    match face {
        None => true,
        Some(f) => f.is_empty() || f.starts_with("KiCad Font"),
    }
}

impl OutlineFont {
    /// `FONT::GetFont( face, bold, italic )` when it is an outline font: `OUTLINE_FONT::LoadFont`'s file (kept, as `s_fontMap` keeps it); `None` for the stroke
    /// font -- no face, "KiCad Font", or a face fontconfig finds no readable file for.
    pub fn load(face: Option<&str>, bold: bool, italic: bool) -> Option<Arc<OutlineFont>> {
        if is_stroke_face(face) {
            return None;
        }
        let face = face?;
        static FONTS: OnceLock<Mutex<HashMap<(String, bool, bool), Option<Arc<OutlineFont>>>>> = OnceLock::new();
        let fonts = FONTS.get_or_init(Default::default);
        let key = (face.to_string(), bold, italic);
        if let Some(hit) = fonts.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return hit.clone();
        }
        let loaded = crate::fontconfig::match_font(face, bold, italic).and_then(|m| {
            let bytes = std::fs::read(&m.found.path).ok()?;
            OutlineFont::from_bytes(bytes, m.found.index, m.result.fake_bold(), m.result.fake_ital()).map(|mut f| {
                f.path = m.found.path.clone();
                Arc::new(f)
            })
        });
        fonts.lock().unwrap_or_else(PoisonError::into_inner).insert(key, loaded.clone());
        loaded
    }

    /// A font from a file's bytes (face `index`), its bold and italic faked as asked.
    pub fn from_bytes(bytes: Vec<u8>, index: u32, fake_bold: bool, fake_ital: bool) -> Option<OutlineFont> {
        Some(OutlineFont { face: Face::parse(bytes, index)?, path: PathBuf::new(), index, fake_bold, fake_ital, glyphs: Mutex::new(HashMap::new()) })
    }

    /// `OUTLINE_FONT::GetInterline( aGlyphHeight )`: `METRICS::GetInterline( aGlyphHeight * ( height / units_per_EM ) )`, the division an integer one.
    pub fn interline(&self, glyph_height: f64) -> f64 {
        let f = &self.face;
        let ratio = if f.units_per_em != 0 { (f.height / f.units_per_em as i32) as f64 } else { 1.0 };
        glyph_height * ratio * INTERLINE_PITCH
    }

    /// A glyph loaded at `scaler` as `getTextAsGlyphsUnlocked` loads it (slanted, emboldened, decomposed), kept (`s_glyphCache`).
    fn glyph(&self, g: u16, supersub: bool) -> Arc<GlyphData> {
        if let Some(hit) = self.glyphs.lock().unwrap_or_else(PoisonError::into_inner).get(&(g, supersub)) {
            return hit.clone();
        }
        let scaler = if supersub { SUBSCRIPT_SIZE } else { FACE_SIZE };
        // font units to `OUTLINE_DECOMPOSER`'s: 26.6 pixels at the size, times `GLYPH_SIZE_SCALER`
        let k = self.face.ppem26(scaler) as f64 / self.face.units_per_em.max(1) as f64 * GLYPH_SIZE_SCALER;
        let data = match self.face.outline_of(g) {
            Ok(None) => GlyphData { contours: Vec::new() },
            Ok(Some(contours)) => {
                let mut c: Vec<FtContour> = contours.iter().map(|c| c.iter().map(|p| FtPoint { x: p.x * k, y: p.y * k, tag: p.tag }).collect()).collect();
                if self.fake_ital {
                    // "Create a 12 degree slant": `xx = cos( -12 )`, `xy = -sin( -12 )`
                    let a = (-std::f64::consts::PI * 12.0 / 180.0) as f32 as f64;
                    let (xx, xy) = (a.cos(), -a.sin());
                    for p in c.iter_mut().flatten() {
                        p.x = xx * p.x + xy * p.y;
                    }
                }
                if self.fake_bold {
                    // `FT_Outline_Embolden( &face->glyph->outline, 1 << 6 )`: one 26.6 pixel
                    c = ft_embolden(&c, 64.0 * GLYPH_SIZE_SCALER);
                }
                let orientation = ft_orientation(&c);
                let flat = ft_decompose(&c);
                GlyphData {
                    contours: flat
                        .into_iter()
                        .map(|f| {
                            let w = winding(&f);
                            let filled = match orientation {
                                1 => w == 1,
                                -1 => w == -1,
                                _ => false,
                            };
                            (f, filled)
                        })
                        .collect(),
                }
            }
            Err(()) => {
                // the tofu: a box with a hole, at the glyph's advance
                let s = scaler as f64;
                let adv = self.face.advance_of(g) as f64 * k;
                let (x0, y0, x1, y1) = (s * 0.03, 0.0, adv - s * 0.02, s * 0.72);
                let outer = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
                let (hx0, hy0) = (x0 + s * 0.06, y0 + s * 0.06);
                let (hw, hh) = ((x1 - x0) - s * 0.06, (y1 - y0) - s * 0.06);
                let hole = vec![(hx0, hy0), (hw, hy0), (hw, hh), (hx0, hh)];
                GlyphData { contours: vec![(outer, true), (hole, false)] }
            }
        };
        let data = Arc::new(data);
        self.glyphs.lock().unwrap_or_else(PoisonError::into_inner).insert((g, supersub), data.clone());
        data
    }

    /// `OUTLINE_FONT::GetTextAsGlyphs`: the text cut at its tabs, each "locked to the nearest 4th column (in space-widths)" from `origin`, each run through
    /// [`OutlineFont::run`]; the cursor's end.
    #[allow(clippy::too_many_arguments)]
    pub fn text_as_glyphs(&self, bbox: Option<&mut BBox>, mut glyphs: Option<&mut Vec<GlyphPoly>>, text: &str, size: [i64; 2], position: [i64; 2], angle: f64, mirror: bool, origin: [i64; 2], style: RunStyle) -> [i64; 2] {
        let mut pos = position;
        let mut local = BBox::at(position);
        let mut run = String::new();
        for c in text.chars() {
            if c == '\t' {
                if !run.is_empty() {
                    pos = self.run(Some(&mut local), glyphs.as_deref_mut(), &run, size, pos, angle, mirror, origin, style);
                    run.clear();
                }
                let tab = (size[0] as f64 * TAB_WIDTH).round() as i64;
                let intrusion = if tab != 0 { (pos[0] - origin[0]) % tab } else { 0 };
                pos[0] += tab - intrusion;
            } else {
                run.push(c);
            }
        }
        if !run.is_empty() {
            pos = self.run(Some(&mut local), glyphs.as_deref_mut(), &run, size, pos, angle, mirror, origin, style);
        }
        if let Some(b) = bbox {
            *b = local;
        }
        pos
    }

    /// `OUTLINE_FONT::getTextAsGlyphsUnlocked` (the module's doc).
    #[allow(clippy::too_many_arguments)]
    fn run(&self, bbox: Option<&mut BBox>, glyphs: Option<&mut Vec<GlyphPoly>>, text: &str, size: [i64; 2], position: [i64; 2], angle: f64, mirror: bool, origin: [i64; 2], style: RunStyle) -> [i64; 2] {
        let supersub = style.sub || style.sup;
        let scaler = if supersub { SUBSCRIPT_SIZE } else { FACE_SIZE };
        // HarfBuzz's advances are at the size asked for (`scaler * 16`, 26.6), not FreeType's rounded ppem (kicad-cli's overbar ends match them to the nanometre)
        let ppem = (scaler * 16) as f64;
        let upem = self.face.units_per_em.max(1) as f64;
        let scale = [size[0] as f64 / FACE_SIZE as f64 * SIZE_COMPENSATION, -(size[1] as f64) / FACE_SIZE as f64 * SIZE_COMPENSATION];
        let shaped = self.face.shape(text);
        let mut cursor = [0i64, 0i64];
        let mut out = glyphs;
        for s in &shaped {
            if let Some(out) = out.as_deref_mut() {
                let data = self.glyph(s.glyph, supersub);
                let mut polys: Vec<GlyphPoly> = Vec::new();
                let mut holes: Vec<Vec<[i64; 2]>> = Vec::new();
                for (c, filled) in &data.contours {
                    let chain: Vec<[i64; 2]> = c
                        .iter()
                        .map(|&(x, y)| {
                            let mut pt = [x + cursor[0] as f64, y + cursor[1] as f64];
                            if style.sub {
                                pt[1] += SUBSCRIPT_VERTICAL_OFFSET * scaler as f64;
                            } else if style.sup {
                                pt[1] += SUPERSCRIPT_VERTICAL_OFFSET * scaler as f64;
                            }
                            pt = [pt[0] * scale[0] + position[0] as f64, pt[1] * scale[1] + position[1] as f64];
                            if mirror {
                                pt[0] = origin[0] as f64 - (pt[0] - origin[0] as f64);
                            }
                            if angle != 0.0 {
                                pt = rotate_d(pt, [origin[0] as f64, origin[1] as f64], angle);
                            }
                            // `SHAPE_LINE_CHAIN::Append( int, int )` of the doubles
                            [pt[0] as i64, pt[1] as i64]
                        })
                        .collect();
                    if *filled {
                        polys.push(GlyphPoly { outline: chain, holes: Vec::new() });
                    } else {
                        holes.push(chain);
                    }
                }
                for h in holes {
                    if h.is_empty() {
                        continue;
                    }
                    match polys.iter_mut().find(|p| point_inside(&p.outline, h[0])) {
                        Some(p) => p.holes.push(h),
                        None => polys.push(GlyphPoly { outline: h, holes: Vec::new() }),
                    }
                }
                out.extend(polys);
            }
            // HarfBuzz's `x_advance`, 26.6 at the size; `cursor.x += x_advance * GLYPH_SIZE_SCALER` on an int
            let x_advance = (s.advance * ppem / upem).round();
            cursor[0] = (cursor[0] as f64 + x_advance * GLYPH_SIZE_SCALER) as i64;
        }
        let (asc26, desc26) = self.face.size_ascender_descender(scaler);
        let ascender = (asc26 as f64 * GLYPH_SIZE_SCALER).abs() as i64;
        let descender = (desc26 as f64 * GLYPH_SIZE_SCALER).abs() as i64;
        if let Some(b) = bbox {
            b.merge_point([position[0], position[1] - (ascender as f64 * scale[1].abs()) as i64]);
            b.merge_point([position[0] + (cursor[0] as f64 * scale[0]) as i64, position[1] + (descender as f64 * scale[1].abs()) as i64]);
        }
        [(position[0] as f64 + cursor[0] as f64 * scale[0]) as i64, (position[1] as f64 - cursor[1] as f64 * scale[1]) as i64]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEJAVU: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

    fn dejavu(bold: bool, ital: bool) -> Option<OutlineFont> {
        OutlineFont::from_bytes(std::fs::read(DEJAVU).ok()?, 0, bold, ital)
    }

    /// DejaVu Sans as FreeType reads it: 2048 units to the em, `hhea`'s metrics, `cmap`, `hmtx`, an 'o' an outline with a hole.
    #[test]
    fn a_truetype_face_is_read_as_freetype_reads_it() {
        let Some(f) = dejavu(false, false) else { return };
        assert_eq!(f.face.units_per_em, 2048);
        assert_eq!((f.face.ascender, f.face.descender), (1901, -483));
        let a = f.face.glyph_of('A' as u32);
        assert_ne!(a, 0);
        assert_eq!(f.face.advance_of(a), 1401);
        let mut polys = Vec::new();
        f.text_as_glyphs(None, Some(&mut polys), "o", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].holes.len(), 1);
        // the 'o' sits on the baseline (y down: above it is negative), x-height about 0.55 em of 1.4 mm
        let ys: Vec<i64> = polys[0].outline.iter().map(|p| p[1]).collect();
        assert!(*ys.iter().max().unwrap() > 0 && *ys.iter().max().unwrap() < 40_000, "{ys:?}");
        assert!((*ys.iter().min().unwrap() + 760_000).abs() < 40_000, "{ys:?}");
    }

    /// The triangles cover the glyph: their area is the outline's less its holes.
    #[test]
    fn a_glyph_is_cut_into_triangles() {
        let Some(f) = dejavu(false, false) else { return };
        for ch in ["o", "B", "8", "W", "%"] {
            let mut p = Vec::new();
            f.text_as_glyphs(None, Some(&mut p), ch, [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
            for g in &p {
                let want = area2(&g.outline).abs() - g.holes.iter().map(|h| area2(h).abs()).sum::<i128>();
                let got: i128 = triangulate(g).iter().map(|t| area2(t).abs()).sum();
                assert!((got - want).abs() <= want / 1000, "{ch}: {got} {want}");
            }
        }
    }

    /// HarfBuzz's kerning: DejaVu Sans' GPOS pulls "AV" together.
    #[test]
    fn pairs_are_kerned() {
        let Some(f) = dejavu(false, false) else { return };
        let av: f64 = f.face.shape("AV").iter().map(|s| s.advance).sum();
        let sum = (f.face.advance_of(f.face.glyph_of('A' as u32)) + f.face.advance_of(f.face.glyph_of('V' as u32))) as f64;
        assert!(av < sum, "{av} {sum}");
        let end = f.text_as_glyphs(None, None, "AV", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
        assert!((end[0] as f64 - av / 2048.0 * 1_400_000.0).abs() < 2_000.0, "{end:?}");
    }

    /// A fake bold grows each glyph by one pixel of 358 to the em up and right; a fake italic leans its top right.
    #[test]
    fn fake_bold_and_italic_change_the_outline() {
        let (Some(plain), Some(bold), Some(ital)) = (dejavu(false, false), dejavu(true, false), dejavu(false, true)) else { return };
        let width = |f: &OutlineFont| {
            let mut p = Vec::new();
            f.text_as_glyphs(None, Some(&mut p), "l", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
            let xs: Vec<i64> = p.iter().flat_map(|g| g.outline.iter().map(|q| q[0])).collect();
            (xs.iter().max().unwrap() - xs.iter().min().unwrap(), p)
        };
        let (w0, _) = width(&plain);
        let (w1, _) = width(&bold);
        let grow = (w1 - w0) as f64;
        assert!((grow - 1_400_000.0 / 358.0).abs() < 300.0, "{grow}");
        let (_, slanted) = width(&ital);
        let top = slanted[0].outline.iter().min_by_key(|q| q[1]).unwrap();
        let bottom = slanted[0].outline.iter().max_by_key(|q| q[1]).unwrap();
        assert!(top[0] > bottom[0] + 200_000, "{top:?} {bottom:?}");
    }

    /// The run's box: the advance wide, from the size's ascender above the baseline to its descender below; a tab locks to the 4th column.
    #[test]
    fn the_box_and_tabs() {
        let Some(f) = dejavu(false, false) else { return };
        let mut b = BBox::default();
        let end = f.text_as_glyphs(Some(&mut b), None, "Ab", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
        assert_eq!(b.max[0], end[0]);
        assert!((b.min[1] + 1_300_000).abs() < 10_000, "{b:?}");
        assert!((b.max[1] - 330_000).abs() < 10_000, "{b:?}");
        let tab = f.text_as_glyphs(None, None, "A\tb", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
        let b_adv = f.text_as_glyphs(None, None, "b", [1_000_000, 1_000_000], [0, 0], 0.0, false, [0, 0], RunStyle::default());
        assert_eq!(tab[0], 2_400_000 + b_adv[0]);
    }
}
