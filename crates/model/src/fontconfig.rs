//! `FONTCONFIG::FindFont` (`common/font/fontconfig.cpp`, KiCad 8303b2ad) through fontconfig's own `fc-match`: the file a text's face, bold and italic load
//! (`OUTLINE_FONT::LoadFont`), and the verdict that makes a fake bold or italic. Moved here from `eda-kicad-engine`'s `fonts.rs` (which re-exports it) so the
//! outline font geometry every crate measures texts with ([`crate::outline_font`]) finds the same file the embedding does (UI-LEFT-3 C3-3).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError};

/// A font file fontconfig matched for a text's face: the file, the face in it, and the family it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFont {
    pub path: PathBuf,
    pub index: u32,
    pub family: String,
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

/// `FONTCONFIG::FF_RESULT` of a match that found a file: the face asked for, another in its place, or it without the bold or the italic asked for (which
/// `OUTLINE_FONT::LoadFont` then fakes: `SetFakeBold`, `SetFakeItal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindResult {
    Ok,
    Substitute,
    MissingBold,
    MissingItal,
    MissingBoldItal,
}

impl FindResult {
    pub fn word(self) -> &'static str {
        match self {
            FindResult::Ok => "ok",
            FindResult::Substitute => "substitute",
            FindResult::MissingBold => "missing_bold",
            FindResult::MissingItal => "missing_ital",
            FindResult::MissingBoldItal => "missing_bold_ital",
        }
    }

    /// `if( retval == FF_MISSING_BOLD || retval == FF_MISSING_BOLD_ITAL ) font->SetFakeBold();`
    pub fn fake_bold(self) -> bool {
        matches!(self, FindResult::MissingBold | FindResult::MissingBoldItal)
    }

    /// `if( retval == FF_MISSING_ITAL || retval == FF_MISSING_BOLD_ITAL ) font->SetFakeItal();`
    pub fn fake_ital(self) -> bool {
        matches!(self, FindResult::MissingItal | FindResult::MissingBoldItal)
    }
}

/// What `FONTCONFIG::FindFont` makes of fontconfig's match: the file, its family names (`getAllFamilyStrings`), its style, and the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMatch {
    pub found: FoundFont,
    pub families: Vec<String>,
    pub style: String,
    pub result: FindResult,
}

/// `FONTCONFIG::FindFont`'s verdict on a match of `families` in `style` for `face` (bold and italic as asked, "bold" in the name making it bold): the style's words say
/// whether the face is bold ("thin", "light", "regular", "roman" and "book" are not; "medium", "semibold" and "demibold" are what was asked; "bold", "heavy", "black",
/// "thick" and "dark" are) and italic ("italic", "oblique", "slant"); a family starting with the name asked for gives `FF_MISSING_*`, `FF_SUBSTITUTE` or `FF_OK`;
/// none of them is a substitute -- unless the family and style written out are the name asked for, case aside ("If we missed a case but the matching found the original
/// font name, then we are not substituting").
pub fn classify_match(face: &str, bold: bool, italic: bool, families: &[String], style: &str) -> FindResult {
    let lower = face.to_lowercase();
    let bold = bold || ["bold", "heavy", "black", "thick", "dark"].iter().any(|w| lower.contains(w));
    let s = style.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
    let has_bold = if has(&["thin", "light", "regular", "roman", "book"]) {
        false
    } else if has(&["medium", "semibold", "demibold"]) {
        bold
    } else {
        has(&["bold", "heavy", "black", "thick", "dark"])
    };
    let has_ital = has(&["italic", "oblique", "slant"]);
    let mut result = FindResult::Substitute;
    if families.iter().any(|f| f.to_lowercase().starts_with(&lower)) {
        result = if bold && !has_bold && italic && !has_ital {
            FindResult::MissingBoldItal
        } else if bold && !has_bold {
            FindResult::MissingBold
        } else if italic && !has_ital {
            FindResult::MissingItal
        } else if bold != has_bold || italic != has_ital {
            FindResult::Substitute
        } else {
            FindResult::Ok
        };
    }
    if result == FindResult::Substitute {
        // `fontName` is the first family and the style, ':' between (`styleStr.Replace( ' ', ':' )`), written back with spaces
        let mut name = families.first().cloned().unwrap_or_default();
        if !style.is_empty() {
            name = format!("{name} {style}");
        }
        if name.to_lowercase() == face.to_lowercase() {
            result = FindResult::Ok;
        }
    }
    result
}

/// The comma-separated values of a fontconfig format's element (`%{family}`), a `\,` kept in its value.
pub fn fc_values(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.last_mut().expect("one").extend(chars.next()),
            ',' => out.push(String::new()),
            _ => out.last_mut().expect("one").push(c),
        }
    }
    out.into_iter().filter(|v| !v.is_empty()).collect()
}

/// `FONTCONFIG::FindFont` through `fc-match`: the file fontconfig matches the pattern `FindFont` builds to, its families and style, and the verdict; `None` for no
/// file (`FF_ERROR`: `FONT::GetFont` then falls back to the stroke font).
pub fn match_font(face: &str, bold: bool, italic: bool) -> Option<FontMatch> {
    // `FONT::GetFont` keeps each font it loaded (`s_fontMap`); every text measured asks again, so the answer is kept too
    static CACHE: OnceLock<Mutex<HashMap<(String, bool, bool), Option<FontMatch>>>> = OnceLock::new();
    let key = (face.to_string(), bold, italic);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return hit.clone();
    }
    let found = match_font_uncached(face, bold, italic);
    cache.lock().unwrap_or_else(PoisonError::into_inner).insert(key, found.clone());
    found
}

fn match_font_uncached(face: &str, bold: bool, italic: bool) -> Option<FontMatch> {
    let pattern = find_font_pattern(face, bold, italic);
    let bin = std::env::var("EDA_FC_MATCH").unwrap_or_else(|_| "fc-match".to_string());
    let out = Command::new(bin).arg("-f").arg("%{file}\t%{index}\t%{family}\t%{style[0]}\n").arg(&pattern).output().ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text.lines().next()?.to_string();
    let mut parts = line.split('\t');
    let path = parts.next().filter(|p| !p.is_empty())?;
    let index = parts.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
    let families = fc_values(parts.next().unwrap_or(""));
    let style = parts.next().unwrap_or("").to_string();
    let result = classify_match(face, bold, italic, &families, &style);
    Some(FontMatch { found: FoundFont { path: PathBuf::from(path), index, family: families.first().cloned().unwrap_or_default() }, families, style, result })
}

