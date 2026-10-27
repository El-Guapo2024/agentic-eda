//! KiCad 10's bold text, read back as older files have it.
//!
//! Up to file version 20260825 a text's `(thickness T)` is the width its strokes are drawn at, `bold` or not, and `bold` is a flag beside it (the stroke font draws the same
//! glyphs). From 20260826 on, a bold text's `(thickness T)` is the stroke width *before* the bold factor and the strokes are drawn at `T * 1.6` (`(bold yes)`, and the
//! same 1.6 is the ratio of KiCad's automatic widths, `GetPenSizeForBold` to `GetPenSizeForNormal`: a fifth of the glyph against an eighth). The two meanings were measured on
//! kicad-cli 10.99.0-5334's own upgrade of a file of bold texts of several sizes (`pcb upgrade --force`): a bold `(thickness 0.15)` of 20241229 is a bold
//! `(thickness 0.09375)` of 20260901, a bold `(thickness 0.2)` is `0.125` whatever the size, and a bold text with no thickness stays without one.
//!
//! [`normalize`] rewrites a newer tree into the older meaning, so every reader of text in this crate works on either: the thickness of each bold `font` is multiplied back.
//! The writer writes the older file version, whose bold text has the same meaning as the file it was read from, so kicad-cli (which reads both) draws the same strokes.

use crate::sexpr::Sexpr;

/// The first board file version whose bold text has a stroke width that is a base to be multiplied.
pub const FIRST_BOLD_MULTIPLIER: i64 = 20260826;

/// The factor a bold text's strokes are widened by.
pub const BOLD_FACTOR: f64 = 1.6;

/// Multiply back the `(thickness ..)` of every bold `(font ..)` of a tree of file version `FIRST_BOLD_MULTIPLIER` or later; an older tree is left as it is.
pub fn normalize(tree: &mut Sexpr) {
    let version = match tree {
        Sexpr::List(root) => root.iter().filter_map(Sexpr::as_list).find(|l| crate::sexpr::tag(l) == Some("version")).and_then(|v| crate::sexpr::num(v, 1)).unwrap_or(0.0) as i64,
        Sexpr::Atom(_) => 0,
    };
    if version >= FIRST_BOLD_MULTIPLIER {
        widen(tree);
    }
}

fn widen(node: &mut Sexpr) {
    let Sexpr::List(items) = node else { return };
    if crate::sexpr::tag(items) == Some("font") && is_bold(items) {
        // "Outline faces never baked bold into the width; skip them." (`MigrateLegacyBoldStrokeWidth`: `!isStrokeFont()`), nor a width of 1 nm or less (`thickness <= 1`).
        if !is_stroke_font(items) {
            return;
        }
        for item in items.iter_mut() {
            if let Sexpr::List(l) = item {
                if crate::sexpr::tag(l) == Some("thickness") {
                    if let Some(t) = crate::sexpr::num(l, 1).filter(|&t| t > 0.000001) {
                        l[1] = Sexpr::Atom(format_mm(t * BOLD_FACTOR));
                    }
                }
            }
        }
        return;
    }
    for item in items.iter_mut() {
        widen(item);
    }
}

/// A font with no `(face ..)`, or the KiCad font named: `EDA_TEXT::isStrokeFont`, which during load keys off the stored face name, `FONT::IsStroke`
/// (`aFontName == _( "Default Font" ) || aFontName == KICAD_FONT_NAME`, "KiCad Font"). Any other face is an outline font.
fn is_stroke_font(font: &[Sexpr]) -> bool {
    match crate::sexpr::find(font, "face").and_then(|f| crate::sexpr::txt(f, 1)) {
        None => true,
        Some(face) => face.is_empty() || face == "Default Font" || face == "KiCad Font",
    }
}

/// `(bold yes)`, the form of the newer files, or the bare `bold` of the older ones.
fn is_bold(font: &[Sexpr]) -> bool {
    font.iter().skip(1).any(|c| match c {
        Sexpr::Atom(a) => a == "bold",
        Sexpr::List(l) => crate::sexpr::tag(l) == Some("bold") && crate::sexpr::txt(l, 1) != Some("no"),
    })
}

/// A length in millimetres as KiCad writes one: nanometres at most, no trailing zeros.
fn format_mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thickness_of(text: &str) -> Vec<String> {
        let mut tree = crate::sexpr::parse(text).unwrap();
        normalize(&mut tree);
        let mut found = Vec::new();
        fn walk(n: &Sexpr, found: &mut Vec<String>) {
            if let Sexpr::List(l) = n {
                if crate::sexpr::tag(l) == Some("thickness") {
                    found.push(crate::sexpr::txt(l, 1).unwrap_or("").to_string());
                }
                l.iter().for_each(|c| walk(c, found));
            }
        }
        walk(&tree, &mut found);
        found
    }

    /// From 20260826 a bold text's thickness is the width before the bold factor: the values kicad-cli 10.99.0-5334 wrote when it upgraded bold texts of the older meaning.
    #[test]
    fn a_newer_files_bold_thickness_is_widened_back_to_the_older_meaning() {
        let new = |font: &str| format!("(kicad_pcb (version 20260901) (gr_text \"a\" (effects (font (size 1 1) {font}))))");
        assert_eq!(thickness_of(&new("(thickness 0.09375) (bold yes)")), ["0.15"]);
        assert_eq!(thickness_of(&new("(thickness 0.125) (bold yes) (italic yes)")), ["0.2"]);
        assert_eq!(thickness_of(&new("(thickness 0.025) (bold yes)")), ["0.04"]);
        assert_eq!(thickness_of(&new("(thickness 0.1905) (bold yes)")), ["0.3048"]);
        // A text that is not bold, a bold one with no thickness, and `(bold no)` are left alone.
        assert_eq!(thickness_of(&new("(thickness 0.15)")), ["0.15"]);
        assert_eq!(thickness_of(&new("(thickness 0.15) (bold no)")), ["0.15"]);
        assert!(thickness_of(&new("(bold yes)")).is_empty());
        // Every font in the tree, a footprint's included.
        let text = "(kicad_pcb (version 20260826) (footprint \"x\" (property \"Reference\" \"R1\" (effects (font (size 1 1) (thickness 0.0625) bold)))) (gr_text \"a\" (effects (font (thickness 0.2) (bold yes)))))";
        assert_eq!(thickness_of(text), ["0.1", "0.32"]);
    }

    /// 25ec2335 `EDA_TEXT::MigrateLegacyBoldStrokeWidth`: "Outline faces never baked bold into the width; skip them." kicad-cli 10.99.0's upgrade of a bold `(thickness 0.2)`
    /// writes 0.125 in the KiCad font and 0.2 in `(face "DejaVu Sans")`. The KiCad font named is a stroke font.
    #[test]
    fn an_outline_fonts_bold_thickness_is_left_as_it_is() {
        let new = |font: &str| format!("(kicad_pcb (version 20260901) (gr_text \"a\" (effects (font {font} (size 1 1) (thickness 0.2) (bold yes)))))");
        assert_eq!(thickness_of(&new("(face \"Arial\")")), ["0.2"]);
        assert_eq!(thickness_of(&new("(face \"DejaVu Sans\")")), ["0.2"]);
        assert_eq!(thickness_of(&new("(face \"KiCad Font\")")), ["0.32"]);
        assert_eq!(thickness_of(&new("")), ["0.32"]);
        // A footprint file, through its reader.
        let fp = "(footprint \"X\" (version 20260901) (fp_text user \"a\" (at 0 0) (layer \"F.SilkS\") (effects (font (face \"Arial\") (size 1 1) (thickness 0.2) (bold yes)))))";
        assert_eq!(thickness_of(fp), ["0.2"]);
    }

    /// A file of an older version is read as it is, whether it says `bold` or `(bold yes)`.
    #[test]
    fn an_older_files_bold_thickness_is_left_as_it_is() {
        for version in [20241229, 20260825] {
            let text = format!("(kicad_pcb (version {version}) (gr_text \"a\" (effects (font (size 1 1) (thickness 0.15) bold))) (gr_text \"b\" (effects (font (thickness 0.2) (bold yes)))))");
            assert_eq!(thickness_of(&text), ["0.15", "0.2"], "version {version}");
        }
    }
}
