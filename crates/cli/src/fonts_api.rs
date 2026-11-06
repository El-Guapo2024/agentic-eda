//! `GET /api/fonts`: the font families the machine has, for the Font column of Symbol Properties' fields grid (`FIELDS_GRID_TABLE`'s `m_fontAttr`, a
//! `GRID_CELL_COMBOBOX` of `Fontconfig()->ListFonts( fontNames, .. )`, sorted, after "Default Font" and "KiCad Font", which the studio adds).
//!
//! KiCad asks fontconfig in process (`FONTCONFIG::ListFonts`, common/font/fontconfig.cpp). The studio runs `fc-list : family`, fontconfig's own lister, on the
//! machine the server is on, and keeps the first name of each family (fontconfig lists a family's names in its languages, `a,b,c`; KiCad picks the one of the
//! user's language, then the first). No `fc-list`: no families, and the column still takes a name typed in. `{ "ok": true, "families": [..] }`.
//!
//! `GET /api/fonts/match?face=..&bold=1&italic=1` and `GET /api/fonts/file?..`: the outline font a text in that face is drawn with (UI-LEFT-2 CC1), as
//! `OUTLINE_FONT::LoadFont` loads it -- `FONTCONFIG::FindFont`'s file (fontconfig's own match, a substitute too, `eda_kicad_engine::fonts::match_font`) and whether it fakes
//! the bold or the italic -- and the file's bytes, which the canvas loads with the browser's `FontFace` (`components/text/outlineFonts.ts`): the browser may be on another
//! machine with other fonts, and kicad-cli, which plots the texts, reads the server's. No match (`FF_ERROR`): `{ "ok": true, "found": false }`, and the text is drawn in
//! the stroke font, as `FONT::GetFont` falls back to it.

use serde_json::{json, Value};
use std::process::Command;

/// The families `fc-list : family` prints, one name each (the first of a line's comma-separated names), sorted and once.
pub(crate) fn families_of(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = listing.lines().filter_map(|l| l.split(',').next()).map(|f| f.trim().replace("\\-", "-")).filter(|f| !f.is_empty()).collect();
    out.sort();
    out.dedup();
    out
}

pub(crate) fn fonts() -> Value {
    match Command::new("fc-list").args([":", "family"]).output() {
        Ok(o) if o.status.success() => json!({ "ok": true, "families": families_of(&String::from_utf8_lossy(&o.stdout)) }),
        _ => json!({ "ok": true, "families": Vec::<String>::new() }),
    }
}

/// The face, bold and italic a request asks for.
fn asked(target: &str) -> (String, bool, bool) {
    let flag = |k: &str| matches!(crate::image_api::query_value(target, k).as_str(), "1" | "true");
    (crate::image_api::query_value(target, "face"), flag("bold"), flag("italic"))
}

/// `GET /api/fonts/match`: the file `FindFont` gives the face, its family and style, and `FF_RESULT` (`fake_bold` / `fake_italic` when the face lacks them).
pub(crate) fn font_match(target: &str) -> Value {
    let (face, bold, italic) = asked(target);
    if face.trim().is_empty() {
        return json!({ "ok": false, "error": "no face" });
    }
    match eda_kicad_engine::fonts::match_font(&face, bold, italic) {
        Some(m) => json!({
            "ok": true,
            "found": true,
            "file": m.found.path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "index": m.found.index,
            "family": m.found.family,
            "style": m.style,
            "result": m.result.word(),
            "fake_bold": m.result.fake_bold(),
            "fake_italic": m.result.fake_ital(),
        }),
        None => json!({ "ok": true, "found": false }),
    }
}

/// `GET /api/fonts/file`: the bytes of the file `FindFont` gives the face, and its media type; `None` when there is none, or it is no font file the browser reads.
pub(crate) fn font_file(target: &str) -> Option<(Vec<u8>, &'static str)> {
    let (face, bold, italic) = asked(target);
    let m = eda_kicad_engine::fonts::match_font(&face, bold, italic)?;
    let ext = m.found.path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let kind = match ext.as_str() {
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" | "otc" => "font/collection",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => return None,
    };
    std::fs::read(&m.found.path).ok().map(|b| (b, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_familys_first_name_is_kept_once_and_the_list_is_sorted() {
        let listing = "DejaVu Sans\nIPAGothic,IPAゴシック\nDejaVu Sans\nFreeSans\n\n";
        assert_eq!(families_of(listing), ["DejaVu Sans", "FreeSans", "IPAGothic"]);
    }

    /// A request with no face is refused; on a machine whose fontconfig has DejaVu Sans (this box's), the face is found as itself, its bold asked of it is its Bold
    /// file, and its italic -- DejaVu's is "Oblique", which `style=Italic` does not match -- is the Book face slanted (`FF_MISSING_ITAL`), as KiCad loads it.
    #[test]
    fn the_match_is_find_fonts() {
        assert_eq!(font_match("/api/fonts/match")["ok"], false);
        let v = font_match("/api/fonts/match?face=DejaVu%20Sans");
        if v["file"] != "DejaVuSans.ttf" {
            return;
        }
        assert_eq!((v["result"].as_str(), v["fake_italic"].as_bool()), (Some("ok"), Some(false)));
        assert_eq!(font_match("/api/fonts/match?face=DejaVu%20Sans&bold=1")["file"], "DejaVuSans-Bold.ttf");
        let it = font_match("/api/fonts/match?face=DejaVu%20Sans&italic=1");
        assert_eq!((it["file"].as_str(), it["result"].as_str(), it["fake_italic"].as_bool()), (Some("DejaVuSans.ttf"), Some("missing_ital"), Some(true)));
        let (bytes, kind) = font_file("/api/fonts/file?face=DejaVu%20Sans").expect("the file");
        assert_eq!((kind, &bytes[0..4]), ("font/ttf", &[0u8, 1, 0, 0][..]));
    }
}
