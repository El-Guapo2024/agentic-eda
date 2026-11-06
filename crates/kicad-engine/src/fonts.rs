//! The outline fonts a board's texts use, found as KiCad finds them and embedded as KiCad embeds them (UI-LEFT P26, GAPS.md items 24 and 30).
//!
//! KiCad 8303b2ad:
//!   - `BOARD::GetFonts` (`pcbnew/board.cpp`): every drawing of the board that is an `EDA_TEXT` (a text, a text box, a dimension) whose font is an outline font
//!     (`font->IsOutline()`) that may be embedded (`GetEmbeddingPermission()` is `EDITABLE` or `INSTALLABLE`). `FOOTPRINT::GetFonts` does the same over a footprint's
//!     graphic items and fields.
//!   - `BOARD::EmbedFonts`: `GetEmbeddedFiles()->AddFile( font->GetFileName(), false )` for each, typed `FONT` -- a file of that name that is there stays.
//!   - `PCB_IO_KICAD_SEXPR::FormatBoardToFormatter`: on every save, "If the user wants fonts embedded, make sure that they are added to the board. Otherwise, remove any fonts that
//!     were previously embedded": `EmbedFonts()` or `ClearEmbeddedFonts()`. [`with_board_fonts`] is that step, run on the copy of the design the derived `.kicad_pcb` is
//!     written from (`crate::export_board_with`); the other half (no fonts written when the box is off) is `EmbeddedFiles::to_write`.
//!   - `PANEL_EMBEDDED_FILES::onFontEmbedClick`: ticking "Embed fonts" takes the text's fonts into the panel's copy at once (`AddFile( font->GetFileName(), true )`), which
//!     `GET /api/embedded/fonts` lists and the `apply` verb carries (`crates/cli/src/embedded_api.rs`).
//!
//! A text's font is `FONT::GetFont( face, bold, italic )` (`common/font/font.cpp`): no face, or one starting with "KiCad Font", is the stroke font; any other face is
//! `OUTLINE_FONT::LoadFont`, which asks fontconfig (`FONTCONFIG::FindFont`, `common/font/fontconfig.cpp`) and loads whatever file it matches -- a substitute too, so a face
//! the machine lacks embeds the font fontconfig puts in its place, as KiCad's does. Per the port-vs-reuse rule the match is fontconfig's own: `fc-match` runs the same
//! `FcConfigSubstitute( .., FcMatchPattern )`, `FcDefaultSubstitute` and `FcFontMatch` on the same pattern (the family, a `Bold` style and an `Italic` style) that `FindFont`
//! builds. The licence is read from the file's `OS/2` table as `OUTLINE_FONT::GetEmbeddingPermission` reads it through FreeType ([`embedding_permission`]).
//!
//! Differences: `FindFont` also hands fontconfig the fonts the board already embeds (`FcConfigAppFontAddFile` of their cache files), so a face only an embedded font has is
//! matched to that file; here only the fonts installed on the machine are matched (a board whose embedded font is not installed keeps the embedded file, under its own
//! name, but a text in that face finds a substitute). A dimension's text has no face in this model (`Dimension`), so only texts and text boxes are asked. A WOFF or WOFF2
//! file is not read for its licence (no system font on Linux is one) and is taken as restricted.

use eda_model::embedded_files::{EmbeddedFile, EmbeddedFileType, EmbeddedFiles};
use eda_model::ir::Design;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError};

/// `KICAD_FONT_NAME` (`include/font/kicad_font_name.h`): the stroke font's name.
pub const KICAD_FONT_NAME: &str = "KiCad Font";

/// One text's font as `EDA_TEXT::GetFont` asks for it: the face and the text's bold and italic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontUse {
    pub face: String,
    pub bold: bool,
    pub italic: bool,
}

/// `OUTLINE_FONT::EMBEDDING_PERMISSION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Installable,
    Editable,
    PrintPreviewOnly,
    Restricted,
}

impl Permission {
    /// What `BOARD::GetFonts` takes: `permission == PERMISSION::EDITABLE || permission == PERMISSION::INSTALLABLE`.
    pub fn may_embed(self) -> bool {
        matches!(self, Permission::Installable | Permission::Editable)
    }

    pub fn word(self) -> &'static str {
        match self {
            Permission::Installable => "installable",
            Permission::Editable => "editable",
            Permission::PrintPreviewOnly => "print_preview_only",
            Permission::Restricted => "restricted",
        }
    }
}

/// A font file fontconfig matched for a [`FontUse`]: the file, the face in it, and the family it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFont {
    pub path: PathBuf,
    pub index: u32,
    pub family: String,
}

/// A font the texts use: what was asked, the file it is, its licence and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFont {
    pub uses: Vec<FontUse>,
    pub path: PathBuf,
    /// The name the file is embedded under (`wxFileName::GetFullName` of the path: `AddFile` files it so).
    pub name: String,
    pub family: String,
    pub permission: Permission,
    pub size: u64,
}

/// `FT_FSTYPE_*` (`freetype/freetype.h`).
const FSTYPE_RESTRICTED_LICENSE: u16 = 0x0002;
const FSTYPE_PREVIEW_AND_PRINT: u16 = 0x0004;
const FSTYPE_EDITABLE: u16 = 0x0008;
const FSTYPE_BITMAP_EMBEDDING_ONLY: u16 = 0x0200;

fn be16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// The `fsType` of the `OS/2` table of face `index` of an OpenType / TrueType file or collection (`FT_Get_Sfnt_Table( m_face, FT_SFNT_OS2 )->fsType`); `None` when the file
/// is no such font or has no `OS/2` table.
fn os2_fs_type(bytes: &[u8], index: u32) -> Option<u16> {
    let tag = bytes.get(0..4)?;
    let font_at = if tag == b"ttcf" {
        // a collection: `numFonts` at 8, then one offset per font
        let count = be32(bytes, 8)?;
        if index >= count {
            return None;
        }
        be32(bytes, 12 + 4 * index as usize)? as usize
    } else if tag == [0, 1, 0, 0] || tag == b"OTTO" || tag == b"true" || tag == b"typ1" {
        0
    } else {
        return None;
    };
    let tables = be16(bytes, font_at + 4)? as usize;
    for i in 0..tables {
        let rec = font_at + 12 + 16 * i;
        if bytes.get(rec..rec + 4)? == b"OS/2" {
            let at = be32(bytes, rec + 8)? as usize;
            // `TT_OS2`: version, xAvgCharWidth, usWeightClass, usWidthClass, then fsType
            return be16(bytes, at + 8);
        }
    }
    None
}

/// `OUTLINE_FONT::GetEmbeddingPermission` (`common/font/outline_font.cpp`), on the bytes of the font file and the face in it.
pub fn embedding_permission(bytes: &[u8], index: u32) -> Permission {
    // "If this table isn't present, we can't assume anything"
    let Some(fs_type) = os2_fs_type(bytes, index) else { return Permission::Restricted };
    // "We don't support bitmap fonts, so this disables embedding"
    if fs_type & FSTYPE_BITMAP_EMBEDDING_ONLY != 0 {
        return Permission::Restricted;
    }
    // "Per the OpenType spec, only bits 0-3 of fsType define the embedding license."
    let bits = fs_type & 0x000F;
    if bits == 0 {
        return Permission::Installable;
    }
    if bits & FSTYPE_EDITABLE != 0 {
        return Permission::Editable;
    }
    if bits & FSTYPE_PREVIEW_AND_PRINT != 0 {
        return Permission::PrintPreviewOnly;
    }
    let _ = FSTYPE_RESTRICTED_LICENSE;
    Permission::Restricted
}

/// A family name as fontconfig's pattern syntax wants it: `\`, `-`, `:` and `,` escaped (`FcNameParse`).
fn pattern_escape(family: &str) -> String {
    let mut out = String::with_capacity(family.len());
    for c in family.chars() {
        if matches!(c, '\\' | '-' | ':' | ',') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The pattern `FONTCONFIG::FindFont` builds: "If the original font name contains any of these, then it is bold, regardless of whether we are looking for bold or not",
/// then `FC_STYLE` "Bold" and "Italic" as asked and `FC_FAMILY` the name.
pub fn find_font_pattern(face: &str, bold: bool, italic: bool) -> String {
    let lower = face.to_lowercase();
    let bold = bold || ["bold", "heavy", "black", "thick", "dark"].iter().any(|w| lower.contains(w));
    let styles: Vec<&str> = [bold.then_some("Bold"), italic.then_some("Italic")].into_iter().flatten().collect();
    if styles.is_empty() {
        pattern_escape(face)
    } else {
        format!("{}:style={}", pattern_escape(face), styles.join(","))
    }
}

fn found_cache() -> &'static Mutex<HashMap<String, Option<FoundFont>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<FoundFont>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// `FONTCONFIG::FindFont`'s file through `fc-match` (`EDA_FC_MATCH` names another binary): `None` when fontconfig matches nothing (`FF_ERROR`, no font loaded).
pub fn find_font(face: &str, bold: bool, italic: bool) -> Option<FoundFont> {
    let pattern = find_font_pattern(face, bold, italic);
    if let Some(hit) = found_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&pattern) {
        return hit.clone();
    }
    let bin = std::env::var("EDA_FC_MATCH").unwrap_or_else(|_| "fc-match".to_string());
    let found = Command::new(bin).arg("-f").arg("%{file}\t%{index}\t%{family[0]}\n").arg(&pattern).output().ok().filter(|o| o.status.success()).and_then(|o| {
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let line = text.lines().next()?.to_string();
        let mut parts = line.split('\t');
        let path = parts.next().filter(|p| !p.is_empty())?;
        let index = parts.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
        let family = parts.next().unwrap_or("").to_string();
        Some(FoundFont { path: PathBuf::from(path), index, family })
    });
    found_cache().lock().unwrap_or_else(PoisonError::into_inner).insert(pattern, found.clone());
    found
}

/// `FONT::GetFont`'s stroke test: no face, or one that names the stroke font.
fn is_stroke(face: Option<&str>) -> bool {
    match face {
        None => true,
        Some(f) => f.is_empty() || f.starts_with(KICAD_FONT_NAME),
    }
}

/// The fonts `BOARD::GetFonts` walks: each drawing of the board that is an `EDA_TEXT` -- a text (its face, bold and italic in `TextExtra`) and a text box -- with an outline
/// face. A dimension's text has no face in this model.
pub fn board_font_uses(design: &Design) -> Vec<FontUse> {
    let mut out: Vec<FontUse> = Vec::new();
    let Some(d) = design.drawings.as_ref() else { return out };
    for t in &d.texts {
        if let Some(e) = d.text_extra(&t.id) {
            if !is_stroke(e.face.as_deref()) {
                out.push(FontUse { face: e.face.clone().unwrap_or_default(), bold: e.bold, italic: e.italic });
            }
        }
    }
    for b in &d.board_items.text_boxes {
        if !is_stroke(b.face.as_deref()) {
            out.push(FontUse { face: b.face.clone().unwrap_or_default(), bold: b.bold, italic: b.italic });
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `BOARD::GetFonts`: the files of the fonts `uses` asks for that may be embedded, one per file (`std::set<OUTLINE_FONT*>` keeps one font per face, bold and italic; two that
/// are the same file are one file to `AddFile`), and the faces left out with the reason.
pub fn text_fonts(uses: &[FontUse]) -> (Vec<TextFont>, Vec<(FontUse, String)>) {
    let mut by_path: BTreeMap<PathBuf, TextFont> = BTreeMap::new();
    let mut skipped: Vec<(FontUse, String)> = Vec::new();
    for u in uses {
        let Some(found) = find_font(&u.face, u.bold, u.italic) else {
            // `LoadFont` returns no font and `GetFont` falls back to the stroke font: nothing to embed ("Error loading font '%s'.")
            skipped.push((u.clone(), format!("Error loading font '{}'.", u.face)));
            continue;
        };
        if let Some(have) = by_path.get_mut(&found.path) {
            have.uses.push(u.clone());
            continue;
        }
        let bytes = match std::fs::read(&found.path) {
            Ok(b) => b,
            Err(e) => {
                skipped.push((u.clone(), format!("{}: {e}", found.path.display())));
                continue;
            }
        };
        let permission = embedding_permission(&bytes, found.index);
        if !permission.may_embed() {
            skipped.push((u.clone(), format!("the licence of {} does not allow embedding it ({})", found.path.display(), permission.word())));
            continue;
        }
        let name = found.path.file_name().and_then(|n| n.to_str()).unwrap_or("font").to_string();
        by_path.insert(found.path.clone(), TextFont { uses: vec![u.clone()], path: found.path.clone(), name, family: found.family.clone(), permission, size: bytes.len() as u64 });
    }
    (by_path.into_values().collect(), skipped)
}

fn encoded_cache() -> &'static Mutex<HashMap<(PathBuf, u64, u128), EmbeddedFile>> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, u64, u128), EmbeddedFile>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The file of a font as `EMBEDDED_FILES::AddFile` keeps it (read, `CompressAndEncode`), typed `FONT` as `EmbedFonts` types it. Kept by path, size and time, so a board saved
/// again does not compress the same font again.
pub fn font_file(path: &Path) -> Result<EmbeddedFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let key = (path.to_path_buf(), meta.len(), stamp);
    if let Some(hit) = encoded_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return Ok(hit.clone());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.is_empty() {
        return Err(format!("Failed to add file '{}'.", path.display()));
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("font");
    let mut file = eda_kicad::embedded::encode_new(name, &bytes).map_err(|e| e.to_string())?;
    file.kind = EmbeddedFileType::Font;
    let mut cache = encoded_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > 64 {
        cache.clear();
    }
    cache.insert(key, file.clone());
    Ok(file)
}

/// `BOARD::EmbedFonts` on `list`: each font of `fonts` added unless a file of its name is there (`AddFile( .., false )`). Returns the names added.
pub fn embed_fonts(list: &mut EmbeddedFiles, fonts: &[TextFont]) -> Vec<String> {
    let mut added = Vec::new();
    for f in fonts {
        if list.has_file(&f.name) {
            continue;
        }
        if let Ok(file) = font_file(&f.path) {
            if list.insert(file) {
                added.push(f.name.clone());
            }
        }
    }
    added
}

/// `PCB_IO_KICAD_SEXPR::FormatBoardToFormatter`'s first step: the design whose board embeds fonts with the fonts its texts use added to the board's list (`EmbedFonts`);
/// the design as it is when the board does not embed fonts (the writer drops the fonts then, `ClearEmbeddedFonts`) or when there is nothing to add.
pub fn with_board_fonts(design: &Design) -> Cow<'_, Design> {
    let Some(d) = design.drawings.as_ref() else { return Cow::Borrowed(design) };
    if !d.embedded.fonts {
        return Cow::Borrowed(design);
    }
    let uses = board_font_uses(design);
    if uses.is_empty() {
        return Cow::Borrowed(design);
    }
    let (fonts, _) = text_fonts(&uses);
    if fonts.iter().all(|f| d.embedded.has_file(&f.name)) {
        return Cow::Borrowed(design);
    }
    let mut out = design.clone();
    if let Some(d) = out.drawings.as_mut() {
        embed_fonts(&mut d.embedded, &fonts);
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-table sfnt with an `OS/2` table whose `fsType` is `fs_type`.
    fn sfnt(fs_type: u16) -> Vec<u8> {
        let mut b = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        b.extend_from_slice(b"OS/2");
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&28u32.to_be_bytes());
        b.extend_from_slice(&10u32.to_be_bytes());
        b.extend_from_slice(&[0, 4, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&fs_type.to_be_bytes());
        b
    }

    /// `OUTLINE_FONT::GetEmbeddingPermission`'s cases: installable (0), editable (8, also with the no-subsetting bit), preview and print (4), restricted (2), bitmap only
    /// (0x200) and no `OS/2` table at all.
    #[test]
    fn the_licence_is_read_from_the_os2_table_as_kicad_reads_it() {
        assert_eq!(embedding_permission(&sfnt(0), 0), Permission::Installable);
        assert_eq!(embedding_permission(&sfnt(0x0008), 0), Permission::Editable);
        assert_eq!(embedding_permission(&sfnt(0x0108), 0), Permission::Editable, "bits 8-9 are modifiers, masked off");
        assert_eq!(embedding_permission(&sfnt(0x000C), 0), Permission::Editable, "editable wins over preview and print");
        assert_eq!(embedding_permission(&sfnt(0x0004), 0), Permission::PrintPreviewOnly);
        assert_eq!(embedding_permission(&sfnt(0x0002), 0), Permission::Restricted);
        assert_eq!(embedding_permission(&sfnt(0x0200), 0), Permission::Restricted);
        assert_eq!(embedding_permission(b"%!PS-AdobeFont-1.0", 0), Permission::Restricted, "a Type 1 font has no OS/2 table");
        // a collection of two: the second face's table
        let one = sfnt(0x0002);
        let two = sfnt(0);
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&[0, 1, 0, 0]);
        ttc.extend_from_slice(&2u32.to_be_bytes());
        let base = 20u32;
        ttc.extend_from_slice(&base.to_be_bytes());
        ttc.extend_from_slice(&(base + one.len() as u32).to_be_bytes());
        // the table offsets in a collection are from the start of the file
        let mut a = one.clone();
        a[20..24].copy_from_slice(&(base + 28).to_be_bytes());
        let mut b = two.clone();
        b[20..24].copy_from_slice(&(base + one.len() as u32 + 28).to_be_bytes());
        ttc.extend_from_slice(&a);
        ttc.extend_from_slice(&b);
        assert_eq!(embedding_permission(&ttc, 0), Permission::Restricted);
        assert_eq!(embedding_permission(&ttc, 1), Permission::Installable);
        assert_eq!(embedding_permission(&ttc, 2), Permission::Restricted, "no such face");
    }

    /// `FONTCONFIG::FindFont`'s pattern: a name with "bold" (or heavy, black, thick, dark) in it is bold; the styles; fontconfig's escapes.
    #[test]
    fn the_pattern_is_find_fonts() {
        assert_eq!(find_font_pattern("DejaVu Sans", false, false), "DejaVu Sans");
        assert_eq!(find_font_pattern("DejaVu Sans", true, true), "DejaVu Sans:style=Bold,Italic");
        assert_eq!(find_font_pattern("Arial Black", false, true), "Arial Black:style=Bold,Italic");
        assert_eq!(find_font_pattern("A-B:C,D", false, false), "A\\-B\\:C\\,D");
    }

    /// `FONT::GetFont`: no face and the stroke font's name are the stroke font, nothing to embed; a text box's face counts as a text's.
    #[test]
    fn only_outline_faces_of_texts_and_text_boxes_are_asked() {
        let mut d: Design = serde_json::from_str(r#"{"schema":2,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
        let dr = d.drawings.get_or_insert_with(Default::default);
        for (id, face) in [("t1", Some("DejaVu Sans")), ("t2", Some("KiCad Font")), ("t3", None), ("t4", Some("DejaVu Sans"))] {
            dr.texts.push(serde_json::from_value(serde_json::json!({ "id": id, "content": "x", "at": { "x": 0, "y": 0 }, "layer": "F.SilkS", "size_nm": 1_000_000, "stroke_width": 150_000 })).unwrap());
            dr.text_extras.push(eda_model::board_text::TextExtra { id: id.into(), face: face.map(str::to_string), bold: id == "t4", ..Default::default() });
        }
        let mut tb = eda_model::pcb_items::TextBox::new("F.SilkS", Default::default(), Default::default(), Default::default(), Default::default());
        tb.face = Some("Liberation Serif".into());
        dr.board_items.text_boxes.push(tb);
        let uses = board_font_uses(&d);
        assert_eq!(uses, vec![FontUse { face: "DejaVu Sans".into(), bold: false, italic: false }, FontUse { face: "DejaVu Sans".into(), bold: true, italic: false }, FontUse { face: "Liberation Serif".into(), bold: false, italic: false }]);
    }
}
