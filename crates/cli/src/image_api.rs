//! The images of the board (`PCB_REFERENCE_IMAGE`) and of the schematic (`SCH_BITMAP`) as the studio asks for them (GAPS.md item 24, package P16).
//!
//! The state (`GET /api/state`, `GET /api/schematic`) does not carry the pixels: an image is described there by its place, its scale, its size in pixels, its definition and a
//! `hash` of the file, and the browser fetches the file itself once per hash:
//!
//!   - `GET /api/pcb_image?id=<id>&h=<hash>`: the file of the board's reference image `id`;
//!   - `GET /api/sch_image?id=<id>&h=<hash>[&sheet=<id>/<id>/..]`: the file of the image `id` on the sheet (the root when `sheet` is empty).
//!
//! The answer is the PNG, JPEG, GIF or BMP file the design embeds, with its own media type; the browser decodes and draws it.

use std::path::Path;

use eda_model::image_info::{self, content_hash, header_of_base64};
use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
use serde_json::{json, Value};

use crate::model3d_api::Reply;

/// The schematic's graphics as the state sends them: each as stored, except that an image is described and not carried -- `px` (its size in pixels), `ppi` and the `hash` the
/// browser asks `GET /api/sch_image` by.
pub(crate) fn sch_graphics_json(graphics: &[SchGraphic]) -> Value {
    let mut v = serde_json::to_value(graphics).unwrap_or(Value::Null);
    if let Some(list) = v.as_array_mut() {
        for (item, g) in list.iter_mut().zip(graphics) {
            if let SchGraphicKind::Image { data, .. } = &g.shape {
                let shape = &mut item["shape"];
                if let Some(o) = shape.as_object_mut() {
                    o.remove("data");
                }
                let h = header_of_base64(data);
                shape["px"] = json!([h.map_or(0, |h| h.width), h.map_or(0, |h| h.height)]);
                shape["ppi"] = json!(h.map_or(image_info::DEFAULT_PPI, |h| h.ppi));
                shape["hash"] = json!(content_hash(data));
            }
        }
    }
    v
}

pub(crate) fn query_value(target: &str, key: &str) -> String {
    let query = target.split('?').nth(1).unwrap_or("");
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .map(|v| percent_decode(v))
        .unwrap_or_default()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn not_found(what: &str) -> Reply {
    Reply { status: "404 Not Found", kind: "text/plain", body: what.as_bytes().to_vec() }
}

/// The file of an image whose base64 text is `data`.
fn file_reply(data: &str) -> Reply {
    let Some(bytes) = image_info::base64_decode(data) else { return not_found("the image data is not base64") };
    match image_info::sniff(&bytes) {
        Some(format) => Reply { status: "200 OK", kind: format.mime(), body: bytes },
        None => not_found("the image data is not a PNG, JPEG, GIF or BMP file"),
    }
}

/// `GET /api/pcb_image`.
pub(crate) fn pcb_image(dir: &Path, target: &str) -> Reply {
    let id = query_value(target, "id");
    match crate::board::load(dir) {
        Ok((_, design, _)) => match design.drawings.as_ref().and_then(|d| d.board_items.images.iter().find(|i| i.id == id)) {
            Some(img) => file_reply(&img.data),
            None => not_found("no reference image with this id"),
        },
        Err(e) => not_found(&crate::board::reasons(&e)),
    }
}

/// `GET /api/sch_image`.
pub(crate) fn sch_image(dir: &Path, target: &str) -> Reply {
    let (id, sheet) = (query_value(target, "id"), query_value(target, "sheet"));
    match crate::board::load(dir) {
        Ok((_, design, _)) => {
            let (section, _) = crate::studio::resolve_sheet(&design, &sheet);
            match section.extras.graphics.iter().find(|g| g.id == id).map(|g| &g.shape) {
                Some(SchGraphicKind::Image { data, .. }) => file_reply(data),
                _ => not_found("no image with this id on the sheet"),
            }
        }
        Err(e) => not_found(&crate::board::reasons(&e)),
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;

    #[test]
    fn the_state_describes_an_image_and_leaves_its_pixels_out() {
        let data = image_info::base64_encode(&image_info::solid_png(60, 40, [1, 2, 3], 254));
        let mut rect = SchGraphic::new(SchGraphicKind::Rectangle { start: eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO }, end: eda_model::ir::Point { x: mm(0.01), y: mm(0.01) }, corner_radius_nm: Nm::ZERO });
        rect.id = "shp_a".into();
        let mut img = SchGraphic::new(SchGraphicKind::Image { at: eda_model::ir::Point { x: Nm::new(5000), y: Nm::new(6000) }, scale_ppm: 500_000, data: data.clone() });
        img.id = "img_a".into();
        let v = sch_graphics_json(&[rect, img]);
        assert_eq!(v[0]["shape"]["type"], "rectangle");
        let s = &v[1]["shape"];
        assert_eq!((s["type"].as_str(), s["px"].clone(), s["ppi"].as_u64(), s["scale_ppm"].as_i64()), (Some("image"), json!([60, 40]), Some(254), Some(500_000)));
        assert!(s.get("data").is_none(), "the pixels are fetched, not sent: {v}");
        assert_eq!(s["hash"], content_hash(&data));
    }

    #[test]
    fn the_query_is_read_with_its_percent_escapes() {
        assert_eq!(query_value("/api/sch_image?id=img_a&h=00&sheet=a%2Fb", "sheet"), "a/b");
        assert_eq!(query_value("/api/sch_image?id=img_a&h=00", "sheet"), "");
        assert_eq!(query_value("/api/pcb_image?id=img_a%20b", "id"), "img_a b");
    }

    #[test]
    fn a_file_is_answered_with_its_own_media_type_and_garbage_is_not_found() {
        let png = image_info::solid_png(4, 4, [0, 0, 0], 300);
        let r = file_reply(&image_info::base64_encode(&png));
        assert_eq!((r.status, r.kind, r.body), ("200 OK", "image/png", png));
        assert_eq!(file_reply("AAAA").status, "404 Not Found");
        assert_eq!(file_reply("not base64!").status, "404 Not Found");
    }
}
