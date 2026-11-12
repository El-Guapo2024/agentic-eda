//! Page Settings in the KiCad files: the `(paper ...)` and `(title_block ...)` elements of a `.kicad_pcb` and a `.kicad_sch`.
//! `PAGE_INFO::Format` / `TITLE_BLOCK::Format` write them, `PAGE_INFO::SetType` and the `PCB_IO_KICAD_SEXPR_PARSER` /
//! `SCH_IO_KICAD_SEXPR_PARSER` read them. The model is `eda_model::page::PageSettings` and `eda_model::ir::TitleBlock`; the
//! schematic writer has its own title block code (it always writes a title and a date), this one is the board's.

use std::fmt::Write as _;

use eda_model::ir::TitleBlock;
use eda_model::page::PageSettings;

use crate::sexpr::{self, Sexpr};
use crate::sexpr_str;

/// `(paper "A4" portrait)` / `(paper "User" 300 200)` of the file's top level. `None` when there is none or it names a paper KiCad
/// has not got; the default (A4 landscape) is a page like any other here -- callers keep `None` for it.
///
/// A `User` size is read as `PCB_IO_KICAD_SEXPR_PARSER::parsePAGE_INFO` / `SCH_IO_KICAD_SEXPR_PARSER::parsePAGE_INFO` read it ("Perform some controls
/// to avoid crashes if the size is edited by hands"): each side clamped to `MIN_PAGE_SIZE_MM` (25.4) and the editor's maximum (`max`:
/// `MAX_PAGE_SIZE_PCBNEW_MM`, 1219.2, or `MAX_PAGE_SIZE_EESCHEMA_MM`, 3048), then turned by a `portrait` after it (`PAGE_INFO::SetPortrait` swaps the
/// sides). KiCad never writes `portrait` for a `User` page (`PAGE_INFO::Format`), so only a hand-edited file has it; the model keeps the turned
/// size, which says its own orientation (round-7 board-io F4).
pub(crate) fn import_page(root: &[Sexpr], max: eda_model::ir::Nm) -> Option<PageSettings> {
    let paper = sexpr::find(root, "paper")?;
    let name = sexpr::txt(paper, 1)?;
    let portrait = (2..paper.len()).any(|i| sexpr::txt(paper, i) == Some("portrait"));
    let (min_mm, max_mm) = (eda_model::units::to_mm(eda_model::page::MIN_PAGE_SIZE_NM), eda_model::units::to_mm(max));
    let user = sexpr::num(paper, 2).zip(sexpr::num(paper, 3)).map(|(w, h)| (w.clamp(min_mm, max_mm), h.clamp(min_mm, max_mm)));
    if name.eq_ignore_ascii_case(eda_model::page::USER_PAPER) {
        let (w, h) = user?;
        return PageSettings::from_args(name, Some(if portrait { (h, w) } else { (w, h) }), false);
    }
    PageSettings::from_args(name, user, portrait)
}

/// A drawing sheet file name as the design keeps it (Page Settings, `pcbnew.page_layout_descr_file` / `schematic.page_layout_descr_file`), for a project file that
/// is not in the board's folder (the derived one in `<board>/.kicad`): a name relative to the board's folder, or starting `${KIPRJMOD}`, becomes the whole path
/// (`FILENAME_RESOLVER::ResolvePath` takes a relative one against the project's folder); a name with another path variable, or a whole one, stays as it is
/// (kicad-cli expands the variable itself).
pub fn drawing_sheet_in(board_dir: &std::path::Path, stored: &str) -> String {
    let rest = stored.strip_prefix("${KIPRJMOD}").map(|r| r.trim_start_matches(['/', '\\']));
    match rest {
        Some(r) => crate::library_table::absolute(std::path::Path::new(r), board_dir).display().to_string(),
        // an embedded sheet (`kicad-embed://name`) is the document's own file, named as it is
        None if stored.starts_with("${") || stored.starts_with("$(") || stored.starts_with("kicad-embed://") || std::path::Path::new(stored).is_absolute() => stored.to_string(),
        None => crate::library_table::absolute(std::path::Path::new(stored), board_dir).display().to_string(),
    }
}

/// The board's `(title_block ...)` (`TITLE_BLOCK::Format`): only the fields that are set, comments as `(comment N "...")`.
/// Empty when the title block has nothing in it.
pub(crate) fn write_board_title_block(out: &mut String, tb: &TitleBlock) {
    let comments: Vec<(usize, &String)> = tb.comments.iter().enumerate().filter(|(_, c)| !c.is_empty()).collect();
    if tb.title.is_empty() && tb.date.is_empty() && tb.rev.is_empty() && tb.company.is_empty() && comments.is_empty() {
        return;
    }
    writeln!(out, "\t(title_block").unwrap();
    for (tag, value) in [("title", &tb.title), ("date", &tb.date), ("rev", &tb.rev), ("company", &tb.company)] {
        if !value.is_empty() {
            writeln!(out, "\t\t({tag} {})", sexpr_str(value)).unwrap();
        }
    }
    for (i, c) in comments {
        writeln!(out, "\t\t(comment {} {})", i + 1, sexpr_str(c)).unwrap();
    }
    writeln!(out, "\t)").unwrap();
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    
    use super::*;

    fn page_of(text: &str) -> Option<PageSettings> {
        let root = sexpr::parse(text).unwrap();
        import_page(root.as_list().unwrap(), eda_model::page::MAX_PAGE_SIZE_PCBNEW_NM)
    }

    #[test]
    fn a_drawing_sheet_name_is_made_whole_for_a_project_elsewhere() {
        let dir = std::path::Path::new("/work/board");
        assert_eq!(drawing_sheet_in(dir, "frame.kicad_wks"), "/work/board/frame.kicad_wks");
        assert_eq!(drawing_sheet_in(dir, "../shared/frame.kicad_wks"), "/work/shared/frame.kicad_wks");
        assert_eq!(drawing_sheet_in(dir, "${KIPRJMOD}/sheets/f.kicad_wks"), "/work/board/sheets/f.kicad_wks");
        assert_eq!(drawing_sheet_in(dir, "${KICAD10_TEMPLATE_DIR}/A4.kicad_wks"), "${KICAD10_TEMPLATE_DIR}/A4.kicad_wks");
        assert_eq!(drawing_sheet_in(dir, "/usr/share/kicad/template/A4.kicad_wks"), "/usr/share/kicad/template/A4.kicad_wks");
    }

    #[test]
    fn a_user_page_is_clamped_as_kicad_reads_it() {
        // round-7 board-io F4: `parsePAGE_INFO` clamps each side to 25.4 .. 1219.2 mm on a board, 3048 mm on a schematic
        assert_eq!(page_of("(kicad_pcb (paper \"User\" 10 2000))"), Some(PageSettings { paper: "User".into(), portrait: false, user_size_nm: Some((mm(25.4), mm(1219.2))) }));
        let root = sexpr::parse("(kicad_sch (paper \"User\" 10 2000))").unwrap();
        assert_eq!(import_page(root.as_list().unwrap(), eda_model::page::MAX_PAGE_SIZE_EESCHEMA_NM).and_then(|p| p.user_size_nm), Some((mm(25.4), mm(2000.0))));
    }

    #[test]
    fn a_portrait_user_page_is_turned() {
        // `PAGE_INFO::SetPortrait( true )` swaps the sides; KiCad never writes it for a User page, a hand-edited file may
        assert_eq!(page_of("(kicad_pcb (paper \"User\" 300 200 portrait))"), Some(PageSettings { paper: "User".into(), portrait: false, user_size_nm: Some((mm(200.0), mm(300.0))) }));
        assert_eq!(page_of("(kicad_pcb (paper \"User\" 300 200))").and_then(|p| p.user_size_nm), Some((mm(300.0), mm(200.0))));
    }

    #[test]
    fn the_paper_element_is_read() {
        assert_eq!(page_of("(kicad_pcb (paper \"A3\" portrait))"), Some(PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None }));
        assert_eq!(page_of("(kicad_pcb (paper \"User\" 300 200.5))"), Some(PageSettings { paper: "User".into(), portrait: false, user_size_nm: Some((mm(300.0), mm(200.5))) }));
        assert_eq!(page_of("(kicad_pcb (paper \"A4\"))"), Some(PageSettings::default()));
        assert_eq!(page_of("(kicad_pcb (version 1))"), None);
    }

    #[test]
    fn the_board_title_block_writes_only_what_is_set() {
        let mut out = String::new();
        write_board_title_block(&mut out, &TitleBlock::default());
        assert_eq!(out, "", "an empty one is not written");
        write_board_title_block(&mut out, &TitleBlock { title: "Blinky".into(), rev: "B".into(), comments: vec!["one".into(), "".into(), "three".into()], ..Default::default() });
        assert_eq!(out, "\t(title_block\n\t\t(title \"Blinky\")\n\t\t(rev \"B\")\n\t\t(comment 1 \"one\")\n\t\t(comment 3 \"three\")\n\t)\n");
    }
}
