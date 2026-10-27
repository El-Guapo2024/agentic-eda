//! The SVG parser of the import: KiCad's own nanosvg (`third_party/nanosvg`, from `kicad-src-25ec2335/thirdparty/nanosvg`, zlib licence), built by
//! `build.rs` with `cc` and called as `SVG_IMPORT_PLUGIN::LoadFromMemory` calls it: `nsvgParse(text, "mm", SVG_DPI)` with `SVG_DPI` 96. Its
//! `NSVGshape`s come back through `cpp/nanosvg_shim.cpp` as the plugin's [`SShape`]s.
//!
//! Where this is not KiCad:
//! * text: KiCad's nanosvg draws `text` and `tspan` through Fontconfig, FreeType and HarfBuzz, which this build leaves out (see `build.rs`), so
//!   text draws nothing;
//! * the messages: KiCad says nothing about what nanosvg leaves out; this says "SVG text elements are not currently supported." (and the same for
//!   `image` and `use`) when the file has one, found by a plain scan of the text for the element's start tag;
//! * a fill with a gradient: the plugin reads the gradient pointer's top byte as the fill's alpha (`shape->fill.color` is a union); here a
//!   gradient fill is filled, as the earlier port did;
//! * a text with no `<svg` start tag is not an SVG file (`Err`), where KiCad would import nothing.

use super::{Paint, Parsed, SPath, SShape, NOT_SVG};
use crate::graphics_import::{FillRule, Rgba};
use std::ffi::c_void;

type ShapeCb = extern "C" fn(ud: *mut c_void, fill_type: i32, fill_color: u32, stroke_type: i32, stroke_color: u32, stroke_width: f32, dashes: *const f32, ndashes: i32, fill_rule: i32, flags: i32);
type PathCb = extern "C" fn(ud: *mut c_void, pts: *const f32, npts: i32, closed: i32);

extern "C" {
    fn ksvg_parse(text: *const u8, len: usize, on_shape: ShapeCb, on_path: PathCb, ud: *mut c_void) -> i32;
}

/// `NSVG_PAINT_*`, `NSVG_FILLRULE_EVENODD`, `NSVG_FLAGS_VISIBLE` (`nanosvg.h`).
const PAINT_NONE: i32 = 0;
const PAINT_COLOR: i32 = 1;
const FILLRULE_EVENODD: i32 = 1;
const FLAGS_VISIBLE: i32 = 0x01;

/// The paint as the plugin reads it: a colour is `0xAABBGGRR` (`fillColor.r = (icolor >> 0) & 0xFF` ... `a = (icolor >> 24)`).
fn paint(kind: i32, color: u32) -> Paint {
    match kind {
        PAINT_NONE => Paint::None,
        PAINT_COLOR => Paint::Color(Rgba {
            r: (color & 0xFF) as f64 / 255.0,
            g: ((color >> 8) & 0xFF) as f64 / 255.0,
            b: ((color >> 16) & 0xFF) as f64 / 255.0,
            a: ((color >> 24) & 0xFF) as f64 / 255.0,
        }),
        _ => Paint::Gradient,
    }
}

extern "C" fn on_shape(ud: *mut c_void, fill_type: i32, fill_color: u32, stroke_type: i32, stroke_color: u32, stroke_width: f32, dashes: *const f32, ndashes: i32, fill_rule: i32, flags: i32) {
    // SAFETY: `ud` is the `Vec<SShape>` of `parse`, alive for the C call; `dashes` points at nanosvg's `strokeDashArray[8]`.
    let shapes = unsafe { &mut *(ud as *mut Vec<SShape>) };
    let n = ndashes.clamp(0, 8) as usize;
    let dash = if n == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(dashes, n) }.iter().map(|&d| d as f64).collect() };
    shapes.push(SShape {
        paths: Vec::new(),
        fill: paint(fill_type, fill_color),
        stroke: paint(stroke_type, stroke_color),
        stroke_width: stroke_width as f64,
        dash,
        fill_rule: if fill_rule == FILLRULE_EVENODD { FillRule::EvenOdd } else { FillRule::NonZero },
        visible: flags & FLAGS_VISIBLE != 0,
    });
}

extern "C" fn on_path(ud: *mut c_void, pts: *const f32, npts: i32, closed: i32) {
    // SAFETY: as in `on_shape`; `pts` holds `npts` points (2 floats each), owned by nanosvg for the call.
    let shapes = unsafe { &mut *(ud as *mut Vec<SShape>) };
    let n = npts.max(0) as usize;
    let xy = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(pts, 2 * n) } };
    let pts = xy.chunks_exact(2).map(|c| (c[0] as f64, c[1] as f64)).collect();
    if let Some(shape) = shapes.last_mut() {
        shape.paths.push(SPath { pts, closed: closed != 0 });
    }
}

/// Whether the text has a start tag `<name` (followed by a space, `/` or `>`) outside a comment.
fn has_element(text: &str, name: &str) -> bool {
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if rest.starts_with("!--") {
            match rest.find("-->") {
                Some(j) => rest = &rest[j + 3..],
                None => return false,
            }
            continue;
        }
        if let Some(after) = rest.strip_prefix(name) {
            if after.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
                return true;
            }
        }
    }
    false
}

/// Reads the shapes of an SVG text with KiCad's nanosvg. `Err` is a text with no `<svg>` element.
pub(super) fn parse(text: &str) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !has_element(text, "svg") {
        return Err(NOT_SVG.into());
    }
    let mut shapes: Vec<SShape> = Vec::new();
    // SAFETY: `text` and `shapes` outlive the call; nanosvg copies nothing out after it returns.
    let rc = unsafe { ksvg_parse(text.as_ptr(), text.len(), on_shape, on_path, &mut shapes as *mut Vec<SShape> as *mut c_void) };
    if rc != 0 {
        return Err(format!("nanosvg could not read the file ({rc})."));
    }
    let messages = ["text", "image", "use"]
        .iter()
        .filter(|el| has_element(text, el))
        .map(|el| format!("SVG {el} elements are not currently supported."))
        .collect();
    Ok(Parsed { shapes, messages })
}
