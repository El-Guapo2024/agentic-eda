//! A document's paper -- KiCad's `PAGE_INFO` (common/page_info.cpp, commit 8303b2ad): the standard paper
//! formats, portrait or landscape, and the "User" size; what `(paper "A4")`, `(paper "A4" portrait)` and
//! `(paper "User" 300 200)` in a `.kicad_pcb` / `.kicad_sch` say. The Page Settings dialog edits it
//! (`DIALOG_PAGES_SETTINGS`), the drawing sheet is laid out on it and kicad-cli plots on it.
//!
//! Sizes are kept in nm like every other length of the IR. KiCad stores them in mils (`MMsize( 297, 210 )` is
//! 11692.9 mils), but a metric format is exactly its millimetres here and an imperial one is a whole number of
//! mils, so nothing is rounded.

use crate::ir::Nm;
use serde::{Deserialize, Serialize};

/// The formats of `PAGE_INFO::standardPageSizes` a document can be set to, with their landscape size
/// `(name, width, height)`. (`GERBER`, the 32000 mil square the Gerber viewer uses, is not offered.)
pub const PAGE_FORMATS: &[(&str, Nm, Nm)] = &[
    ("A5", Nm::new(210_000_000), Nm::new(148_000_000)),
    ("A4", Nm::new(297_000_000), Nm::new(210_000_000)),
    ("A3", Nm::new(420_000_000), Nm::new(297_000_000)),
    ("A2", Nm::new(594_000_000), Nm::new(420_000_000)),
    ("A1", Nm::new(841_000_000), Nm::new(594_000_000)),
    ("A0", Nm::new(1_189_000_000), Nm::new(841_000_000)),
    ("A", Nm::new(279_400_000), Nm::new(215_900_000)),
    ("B", Nm::new(431_800_000), Nm::new(279_400_000)),
    ("C", Nm::new(558_800_000), Nm::new(431_800_000)),
    ("D", Nm::new(863_600_000), Nm::new(558_800_000)),
    ("E", Nm::new(1_117_600_000), Nm::new(863_600_000)),
    ("USLetter", Nm::new(279_400_000), Nm::new(215_900_000)),
    ("USLegal", Nm::new(355_600_000), Nm::new(215_900_000)),
    ("USLedger", Nm::new(431_800_000), Nm::new(279_400_000)),
];

/// `PAGE_SIZE_TYPE::User`: a custom size, given as its own width and height.
pub const USER_PAPER: &str = "User";
/// `MIN_PAGE_SIZE_MILS` (1000 mils).
pub const MIN_PAGE_SIZE_NM: Nm = Nm::new(25_400_000);
/// `MAX_PAGE_SIZE_PCBNEW_MILS` (48000 mils): the largest user size of a board.
pub const MAX_PAGE_SIZE_PCBNEW_NM: Nm = Nm::new(1_219_200_000);
/// `MAX_PAGE_SIZE_EESCHEMA_MILS` (120000 mils): the largest user size of a schematic.
pub const MAX_PAGE_SIZE_EESCHEMA_NM: Nm = Nm::new(3_048_000_000);

/// A document's paper: `PAGE_INFO` reduced to what it saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSettings {
    /// KiCad's name: a standard one of [`PAGE_FORMATS`], or [`USER_PAPER`].
    pub paper: String,
    /// A standard format laid on its side (`(paper "A4" portrait)`); the size of a user format says its own orientation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub portrait: bool,
    /// A user format's width and height, nm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "user_size_um", deserialize_with = "crate::units::de")]
    pub user_size_nm: Option<(Nm, Nm)>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Default for PageSettings {
    /// What a new document has: A4, landscape.
    fn default() -> Self {
        PageSettings { paper: "A4".into(), portrait: false, user_size_nm: None }
    }
}

impl PageSettings {
    /// A4 landscape, `PAGE_INFO`'s default: what a document with no page of its own is on.
    pub fn is_default(&self) -> bool {
        *self == PageSettings::default()
    }

    /// The paper's width and height, nm, in its orientation (`PAGE_INFO::GetSizeMils`).
    pub fn size_nm(&self) -> Result<(Nm, Nm), String> {
        if self.paper == USER_PAPER {
            return self.user_size_nm.ok_or_else(|| "a user paper size needs its width and height".to_string());
        }
        let (_, w, h) = PAGE_FORMATS.iter().find(|(name, _, _)| *name == self.paper).ok_or_else(|| format!("unknown paper size '{}'", self.paper))?;
        Ok(if self.portrait { (*h, *w) } else { (*w, *h) })
    }

    /// The checks `DIALOG_PAGES_SETTINGS::TransferDataFromWindow` makes: the paper is a known one and a user size is between
    /// `MIN_PAGE_SIZE_MILS` and the editor's maximum (`max_nm`, [`MAX_PAGE_SIZE_PCBNEW_NM`] or [`MAX_PAGE_SIZE_EESCHEMA_NM`]).
    pub fn validate(&self, max_nm: Nm) -> Result<(), String> {
        let (w, h) = self.size_nm()?;
        if self.paper == USER_PAPER {
            for (what, v) in [("width", w), ("height", h)] {
                if !(MIN_PAGE_SIZE_NM..=max_nm).contains(&v) {
                    return Err(format!("the paper {what} must be between {:.1} mm and {:.1} mm", crate::units::to_mm(MIN_PAGE_SIZE_NM), crate::units::to_mm(max_nm)));
                }
            }
            if self.portrait {
                return Err("a user paper size has no orientation of its own: it is its width and height".to_string());
            }
        } else if self.user_size_nm.is_some() {
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
        if let (USER_PAPER, Some((w, h))) = (self.paper.as_str(), self.user_size_nm) {
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
            return Some(PageSettings { paper: (*n).to_string(), portrait, user_size_nm: None });
        }
        if name.eq_ignore_ascii_case(USER_PAPER) {
            let (w, h) = user_mm?;
            return Some(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((crate::units::mm(w), crate::units::mm(h))) });
        }
        None
    }
}

/// mm with up to six decimals and no trailing zeros (`FormatDouble2Str`).
fn fmt_mm(nm: Nm) -> String {
    let mut s = format!("{:.6}", crate::units::to_mm(nm));
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
        assert_eq!(a4.size_nm().unwrap(), (Nm::new(297_000_000), Nm::new(210_000_000)));
        assert_eq!(PageSettings { portrait: true, ..a4.clone() }.size_nm().unwrap(), (Nm::new(210_000_000), Nm::new(297_000_000)));
        assert_eq!(PageSettings { paper: "USLetter".into(), ..a4.clone() }.size_nm().unwrap(), (Nm::new(279_400_000), Nm::new(215_900_000)));
        assert!(PageSettings { paper: "A9".into(), ..a4 }.size_nm().is_err());
    }

    #[test]
    fn every_listed_format_is_landscape() {
        for (name, w, h) in PAGE_FORMATS {
            assert!(w > h, "{name} must be defined landscape, like PAGE_INFO::standardPageSizes");
        }
    }

    #[test]
    fn a_user_size_is_checked_against_the_editors_limits() {
        let user = |w: Nm, h: Nm| PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((w, h)) };
        assert!(user(Nm::new(300_000_000), Nm::new(200_000_000)).validate(MAX_PAGE_SIZE_PCBNEW_NM).is_ok());
        assert!(user(Nm::new(25_000_000), Nm::new(200_000_000)).validate(MAX_PAGE_SIZE_PCBNEW_NM).is_err(), "under 1000 mils");
        assert!(user(Nm::new(1_300_000_000), Nm::new(200_000_000)).validate(MAX_PAGE_SIZE_PCBNEW_NM).is_err(), "over 48000 mils on a board");
        assert!(user(Nm::new(1_300_000_000), Nm::new(200_000_000)).validate(MAX_PAGE_SIZE_EESCHEMA_NM).is_ok(), "but fine on a schematic");
        assert!(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: None }.validate(MAX_PAGE_SIZE_PCBNEW_NM).is_err());
        assert!(PageSettings { paper: "A4".into(), portrait: false, user_size_nm: Some((Nm::new(1000), Nm::new(1000))) }.validate(MAX_PAGE_SIZE_PCBNEW_NM).is_err());
        assert!(PageSettings::default().validate(MAX_PAGE_SIZE_PCBNEW_NM).is_ok());
    }

    #[test]
    fn the_paper_element_is_what_page_info_format_writes() {
        assert_eq!(PageSettings::default().to_sexpr(), "(paper \"A4\")");
        assert_eq!(PageSettings { portrait: true, ..Default::default() }.to_sexpr(), "(paper \"A4\" portrait)");
        assert_eq!(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((Nm::new(300_000_000), Nm::new(200_500_000))) }.to_sexpr(), "(paper \"User\" 300 200.5)");
    }

    #[test]
    fn a_sheet_keeps_the_name_and_only_what_the_name_cannot_say() {
        let a3 = PageSettings { paper: "A3".into(), ..Default::default() };
        assert_eq!(a3.sheet_parts(), ("A3".to_string(), None), "a standard landscape paper is its name");
        assert_eq!(PageSettings::default().sheet_parts(), (String::new(), None), "plain A4 is stored as nothing");
        let portrait = PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None };
        assert_eq!(portrait.sheet_parts(), ("A3".to_string(), Some(portrait.clone())));
        let user = PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((Nm::new(300_000_000), Nm::new(200_000_000))) };
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
        assert_eq!(PageSettings::from_args("A3", None, true), Some(PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None }));
        assert_eq!(PageSettings::from_args("User", Some((300.0, 200.5)), false), Some(PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((Nm::new(300_000_000), Nm::new(200_500_000))) }));
        assert_eq!(PageSettings::from_args("User", None, false), None);
        assert_eq!(PageSettings::from_args("Z9", None, false), None);
        assert_eq!(PageSettings::from_args("usletter", None, false).map(|p| p.paper), Some("USLetter".to_string()));
    }
}
