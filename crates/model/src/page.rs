//! A document's paper -- KiCad's `PAGE_INFO` (common/page_info.cpp, commit 8303b2ad): the standard paper
//! formats, portrait or landscape, and the "User" size; what `(paper "A4")`, `(paper "A4" portrait)` and
//! `(paper "User" 300 200)` in a `.kicad_pcb` / `.kicad_sch` say. The Page Settings dialog edits it
//! (`DIALOG_PAGES_SETTINGS`), the drawing sheet is laid out on it and kicad-cli plots on it.
//!
//! Sizes are kept in µm like every other length of the IR. KiCad stores them in mils (`MMsize( 297, 210 )` is
//! 11692.9 mils), but a metric format is exactly its millimetres here and an imperial one is a whole number of
//! mils, so nothing is rounded.

use crate::ir::Um;
use serde::{Deserialize, Serialize};

/// The formats of `PAGE_INFO::standardPageSizes` a document can be set to, with their landscape size
/// `(name, width, height)`. (`GERBER`, the 32000 mil square the Gerber viewer uses, is not offered.)
pub const PAGE_FORMATS: &[(&str, Um, Um)] = &[
    ("A5", 210_000, 148_000),
    ("A4", 297_000, 210_000),
    ("A3", 420_000, 297_000),
    ("A2", 594_000, 420_000),
    ("A1", 841_000, 594_000),
    ("A0", 1_189_000, 841_000),
    ("A", 279_400, 215_900),
    ("B", 431_800, 279_400),
    ("C", 558_800, 431_800),
    ("D", 863_600, 558_800),
    ("E", 1_117_600, 863_600),
    ("USLetter", 279_400, 215_900),
    ("USLegal", 355_600, 215_900),
    ("USLedger", 431_800, 279_400),
];

/// `PAGE_SIZE_TYPE::User`: a custom size, given as its own width and height.
pub const USER_PAPER: &str = "User";
/// `MIN_PAGE_SIZE_MILS` (1000 mils).
pub const MIN_PAGE_SIZE_UM: Um = 25_400;
/// `MAX_PAGE_SIZE_PCBNEW_MILS` (48000 mils): the largest user size of a board.
pub const MAX_PAGE_SIZE_PCBNEW_UM: Um = 1_219_200;
/// `MAX_PAGE_SIZE_EESCHEMA_MILS` (120000 mils): the largest user size of a schematic.
pub const MAX_PAGE_SIZE_EESCHEMA_UM: Um = 3_048_000;

/// A document's paper: `PAGE_INFO` reduced to what it saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSettings {
    /// KiCad's name: a standard one of [`PAGE_FORMATS`], or [`USER_PAPER`].
    pub paper: String,
    /// A standard format laid on its side (`(paper "A4" portrait)`); the size of a user format says its own orientation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub portrait: bool,
    /// A user format's width and height, µm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_size_um: Option<(Um, Um)>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Default for PageSettings {
    /// What a new document has: A4, landscape.
    fn default() -> Self {
        PageSettings { paper: "A4".into(), portrait: false, user_size_um: None }
    }
}

impl PageSettings {
    /// A4 landscape, `PAGE_INFO`'s default: what a document with no page of its own is on.
    pub fn is_default(&self) -> bool {
        *self == PageSettings::default()
    }

    /// The paper's width and height, µm, in its orientation (`PAGE_INFO::GetSizeMils`).
    pub fn size_um(&self) -> Result<(Um, Um), String> {
        if self.paper == USER_PAPER {
            return self.user_size_um.ok_or_else(|| "a user paper size needs its width and height".to_string());
        }
        let (_, w, h) = PAGE_FORMATS.iter().find(|(name, _, _)| *name == self.paper).ok_or_else(|| format!("unknown paper size '{}'", self.paper))?;
        Ok(if self.portrait { (*h, *w) } else { (*w, *h) })
    }

    /// The checks `DIALOG_PAGES_SETTINGS::TransferDataFromWindow` makes: the paper is a known one and a user size is between
    /// `MIN_PAGE_SIZE_MILS` and the editor's maximum (`max_um`, [`MAX_PAGE_SIZE_PCBNEW_UM`] or [`MAX_PAGE_SIZE_EESCHEMA_UM`]).
    pub fn validate(&self, max_um: Um) -> Result<(), String> {
        let (w, h) = self.size_um()?;
        if self.paper == USER_PAPER {
            for (what, v) in [("width", w), ("height", h)] {
                if !(MIN_PAGE_SIZE_UM..=max_um).contains(&v) {
                    return Err(format!("the paper {what} must be between {:.1} mm and {:.1} mm", MIN_PAGE_SIZE_UM as f64 / 1000.0, max_um as f64 / 1000.0));
                }
            }
            if self.portrait {
                return Err("a user paper size has no orientation of its own: it is its width and height".to_string());
            }
        } else if self.user_size_um.is_some() {
            return Err(format!("a {} page has no user size", self.paper));
        }
        Ok(())
    }

    /// A schematic sheet's page. The sheet keeps its paper's *name* in its title block (`TitleBlock::paper`, which the layout engine chooses
    /// per sheet and Page Settings edits) and, only when the name cannot say it all -- a portrait paper, a user size -- the full settings in
    /// `SchExtras::page` (`page`). The page is the full settings when there are, else what the name says, else A4 landscape.
    pub fn of_sheet(page: Option<&PageSettings>, paper_name: &str) -> PageSettings {
        if let Some(p) = page {
            return p.clone();
        }
        PageSettings::from_args(paper_name, None, false).unwrap_or_default()
    }

    /// The two halves a sheet keeps of this page (see [`PageSettings::of_sheet`]): the name for `TitleBlock::paper` -- empty for plain A4, as
    /// a sheet that never set one -- and the full settings when the name alone is not enough.
    pub fn sheet_parts(&self) -> (String, Option<PageSettings>) {
        let name = if self.is_default() { String::new() } else { self.paper.clone() };
        let full = (self.portrait || self.paper == USER_PAPER).then(|| self.clone());
        (name, full)
    }

    /// The `(paper ...)` element of a `.kicad_pcb` / `.kicad_sch` (`PAGE_INFO::Format`): the name, the width and height in mm for a user
    /// size and `portrait` for a standard format on its side.
    pub fn to_sexpr(&self) -> String {
        let mut s = format!("(paper \"{}\"", self.paper);
        if let (USER_PAPER, Some((w, h))) = (self.paper.as_str(), self.user_size_um) {
            s.push_str(&format!(" {} {}", fmt_mm(w), fmt_mm(h)));
        }
        if self.paper != USER_PAPER && self.portrait {
            s.push_str(" portrait");
        }
        s.push(')');
        s
    }

    /// Reads a `(paper "A4" portrait)` / `(paper "User" 300 200)` element's arguments (what follows the keyword): the name, then either
    /// the user size in mm or the `portrait` flag. `None` when the name is not a paper KiCad has.
    pub fn from_args(name: &str, user_mm: Option<(f64, f64)>, portrait: bool) -> Option<PageSettings> {
        let known = PAGE_FORMATS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name));
        if let Some((n, _, _)) = known {
            return Some(PageSettings { paper: (*n).to_string(), portrait, user_size_um: None });
        }
        if name.eq_ignore_ascii_case(USER_PAPER) {
            let (w, h) = user_mm?;
            return Some(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some(((w * 1000.0).round() as Um, (h * 1000.0).round() as Um)) });
        }
        None
    }
}

/// mm with up to six decimals and no trailing zeros (`FormatDouble2Str`).
fn fmt_mm(um: Um) -> String {
    let mut s = format!("{:.6}", um as f64 / 1000.0);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_sizes_are_landscape_and_turn_over_for_portrait() {
        let a4 = PageSettings::default();
        assert_eq!(a4.size_um().unwrap(), (297_000, 210_000));
        assert_eq!(PageSettings { portrait: true, ..a4.clone() }.size_um().unwrap(), (210_000, 297_000));
        assert_eq!(PageSettings { paper: "USLetter".into(), ..a4.clone() }.size_um().unwrap(), (279_400, 215_900));
        assert!(PageSettings { paper: "A9".into(), ..a4 }.size_um().is_err());
    }

    #[test]
    fn every_listed_format_is_landscape() {
        for (name, w, h) in PAGE_FORMATS {
            assert!(w > h, "{name} must be defined landscape, like PAGE_INFO::standardPageSizes");
        }
    }

    #[test]
    fn a_user_size_is_checked_against_the_editors_limits() {
        let user = |w: Um, h: Um| PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some((w, h)) };
        assert!(user(300_000, 200_000).validate(MAX_PAGE_SIZE_PCBNEW_UM).is_ok());
        assert!(user(25_000, 200_000).validate(MAX_PAGE_SIZE_PCBNEW_UM).is_err(), "under 1000 mils");
        assert!(user(1_300_000, 200_000).validate(MAX_PAGE_SIZE_PCBNEW_UM).is_err(), "over 48000 mils on a board");
        assert!(user(1_300_000, 200_000).validate(MAX_PAGE_SIZE_EESCHEMA_UM).is_ok(), "but fine on a schematic");
        assert!(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: None }.validate(MAX_PAGE_SIZE_PCBNEW_UM).is_err());
        assert!(PageSettings { paper: "A4".into(), portrait: false, user_size_um: Some((1, 1)) }.validate(MAX_PAGE_SIZE_PCBNEW_UM).is_err());
        assert!(PageSettings::default().validate(MAX_PAGE_SIZE_PCBNEW_UM).is_ok());
    }

    #[test]
    fn the_paper_element_is_what_page_info_format_writes() {
        assert_eq!(PageSettings::default().to_sexpr(), "(paper \"A4\")");
        assert_eq!(PageSettings { portrait: true, ..Default::default() }.to_sexpr(), "(paper \"A4\" portrait)");
        assert_eq!(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some((300_000, 200_500)) }.to_sexpr(), "(paper \"User\" 300 200.5)");
    }

    #[test]
    fn a_sheet_keeps_the_name_and_only_what_the_name_cannot_say() {
        let a3 = PageSettings { paper: "A3".into(), ..Default::default() };
        assert_eq!(a3.sheet_parts(), ("A3".to_string(), None), "a standard landscape paper is its name");
        assert_eq!(PageSettings::default().sheet_parts(), (String::new(), None), "plain A4 is stored as nothing");
        let portrait = PageSettings { paper: "A3".into(), portrait: true, user_size_um: None };
        assert_eq!(portrait.sheet_parts(), ("A3".to_string(), Some(portrait.clone())));
        let user = PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some((300_000, 200_000)) };
        assert_eq!(user.sheet_parts(), (USER_PAPER.to_string(), Some(user.clone())));
        // and reads back
        assert_eq!(PageSettings::of_sheet(None, "A3"), a3);
        assert_eq!(PageSettings::of_sheet(None, ""), PageSettings::default());
        assert_eq!(PageSettings::of_sheet(None, "nonsense"), PageSettings::default(), "an unknown name is A4, as the layout engine reads it");
        assert_eq!(PageSettings::of_sheet(Some(&portrait), "A3"), portrait);
        assert_eq!(PageSettings::of_sheet(Some(&user), "User"), user);
    }

    #[test]
    fn the_paper_element_reads_back() {
        assert_eq!(PageSettings::from_args("A3", None, true), Some(PageSettings { paper: "A3".into(), portrait: true, user_size_um: None }));
        assert_eq!(PageSettings::from_args("User", Some((300.0, 200.5)), false), Some(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some((300_000, 200_500)) }));
        assert_eq!(PageSettings::from_args("User", None, false), None);
        assert_eq!(PageSettings::from_args("Z9", None, false), None);
        assert_eq!(PageSettings::from_args("usletter", None, false).map(|p| p.paper), Some("USLetter".to_string()));
    }
}
