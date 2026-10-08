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
pub(crate) fn import_page(root: &[Sexpr]) -> Option<PageSettings> {
    let paper = sexpr::find(root, "paper")?;
    let name = sexpr::txt(paper, 1)?;
    let user = sexpr::num(paper, 2).zip(sexpr::num(paper, 3));
    let portrait = (2..paper.len()).any(|i| sexpr::txt(paper, i) == Some("portrait"));
    PageSettings::from_args(name, user, portrait)
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
    use super::*;

    fn page_of(text: &str) -> Option<PageSettings> {
        let root = sexpr::parse(text).unwrap();
        import_page(root.as_list().unwrap())
    }

    #[test]
    fn the_paper_element_is_read() {
        assert_eq!(page_of("(kicad_pcb (paper \"A3\" portrait))"), Some(PageSettings { paper: "A3".into(), portrait: true, user_size_um: None }));
        assert_eq!(page_of("(kicad_pcb (paper \"User\" 300 200.5))"), Some(PageSettings { paper: "User".into(), portrait: false, user_size_um: Some((300_000, 200_500)) }));
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
