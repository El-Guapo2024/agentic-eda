//! Page Settings (`common.Control.pageSettings`): the paper and the title block of the board or of the schematic, as
//! one undo step each. `DIALOG_PAGES_SETTINGS::SavePageSettings` (common/dialogs/dialog_page_settings.cpp) does
//! `m_parent->SetPageSettings( m_pageInfo )` and `SetTitleBlock( m_tb )` together, and `BOARD_EDITOR_CONTROL::PageSettings` /
//! `SCH_EDITOR_CONTROL::PageSetup` wrap both in one undo item ("Page Settings"); [`crate::Board::apply`] calls these for
//! `Cmd::SetBoardPage` and `Cmd::SetSchematicPage`.
//!
//! Where this differs from the source: the dialog's drawing sheet file (a custom `.kicad_wks`) and the schematic dialog's
//! "export to other sheets" boxes are not ported -- the drawing sheet is KiCad's default one and the editor works on one sheet.
//! A change that changes nothing is an error, so no empty undo step is pushed.

use eda_model::ir::{Design, TitleBlock};
use eda_model::page::{PageSettings, MAX_PAGE_SIZE_EESCHEMA_UM, MAX_PAGE_SIZE_PCBNEW_UM};
use eda_model::CheckResult;

fn fail(check: &str, location: &str, hint: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, hint)]
}

/// A title block with nothing in it is no title block, and trailing empty comments are not kept (the file has no `comment 9` of "").
fn normalized(tb: &TitleBlock) -> Option<TitleBlock> {
    let mut tb = tb.clone();
    tb.comments.truncate(9);
    while tb.comments.last().is_some_and(|c| c.is_empty()) {
        tb.comments.pop();
    }
    (tb != TitleBlock::default()).then_some(tb)
}

/// The board's paper and title block (`BOARD::SetPageSettings` / `SetTitleBlock`). A4 landscape and an empty title block are the
/// defaults and are not kept.
pub fn set_board_page(design: &mut Design, page: &PageSettings, title_block: &TitleBlock) -> Result<(), Vec<CheckResult>> {
    page.validate(MAX_PAGE_SIZE_PCBNEW_UM).map_err(|m| fail("ops_page_settings", "page", &m))?;
    let new_page = (!page.is_default()).then(|| page.clone());
    let new_tb = normalized(title_block);
    let current = design.drawings.as_ref();
    if current.and_then(|d| d.page.as_ref()) == new_page.as_ref() && current.and_then(|d| d.title_block.as_ref()) == new_tb.as_ref() {
        return Err(fail("ops_page_settings", "page", "the page settings are already like this"));
    }
    let d = design.drawings.get_or_insert_with(Default::default);
    d.page = new_page;
    d.title_block = new_tb;
    Ok(())
}

/// The schematic's paper and title block (`SCH_SCREEN::SetPageSettings` / `SetTitleBlock`). A sheet keeps its paper's name in its title
/// block (`TitleBlock::paper`, the one the layout engine writes for a sheet it laid out and the exporter reads) and, only for a paper the
/// name cannot say -- portrait, a user size -- the full settings in `SchExtras::page` (see [`PageSettings::of_sheet`]). The title block
/// passed in carries the dialog's fields; its `paper` is the page's name here, whatever it says.
pub fn set_schematic_page(design: &mut Design, page: &PageSettings, title_block: &TitleBlock) -> Result<(), Vec<CheckResult>> {
    page.validate(MAX_PAGE_SIZE_EESCHEMA_UM).map_err(|m| fail("ops_page_settings", "page", &m))?;
    let sch = design.schematic.as_mut().ok_or_else(|| fail("ops_no_schematic", "schematic", "this board has no schematic section yet"))?;
    let (name, full) = page.sheet_parts();
    let new_tb = normalized(&TitleBlock { paper: name, ..title_block.clone() });
    if sch.extras.page == full && sch.title_block == new_tb {
        return Err(fail("ops_page_settings", "page", "the page settings are already like this"));
    }
    sch.extras.page = full;
    sch.title_block = new_tb;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::page::USER_PAPER;

    /// Built from JSON so a field the IR gains later (with its serde default) does not break these.
    fn design(with_schematic: bool) -> Design {
        let mut v = serde_json::json!({ "schema": 1, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 }, "routing": { "tracks": [], "vias": [] } });
        if with_schematic {
            v["schematic"] = serde_json::json!({ "symbols": [], "wires": [], "labels": [] });
        }
        serde_json::from_value(v).unwrap()
    }

    fn a3() -> PageSettings {
        PageSettings { paper: "A3".into(), portrait: true, user_size_um: None }
    }

    fn tb(title: &str, comments: &[&str]) -> TitleBlock {
        TitleBlock { title: title.into(), comments: comments.iter().map(|c| c.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn the_board_keeps_a_paper_and_a_title_block() {
        let mut d = design(false);
        set_board_page(&mut d, &a3(), &tb("Blinky", &["a", "", ""])).unwrap();
        let drawings = d.drawings.as_ref().unwrap();
        assert_eq!(drawings.page, Some(a3()));
        assert_eq!(drawings.title_block, Some(tb("Blinky", &["a"])), "trailing empty comments are dropped");
    }

    #[test]
    fn the_defaults_are_not_kept_and_an_unchanged_page_is_refused() {
        let mut d = design(false);
        assert!(set_board_page(&mut d, &PageSettings::default(), &TitleBlock::default()).is_err(), "nothing differs from a new board");
        set_board_page(&mut d, &a3(), &TitleBlock::default()).unwrap();
        assert!(set_board_page(&mut d, &a3(), &TitleBlock::default()).is_err(), "the same again");
        set_board_page(&mut d, &PageSettings::default(), &TitleBlock::default()).unwrap();
        let drawings = d.drawings.as_ref().unwrap();
        assert_eq!((drawings.page.as_ref(), drawings.title_block.as_ref()), (None, None), "back to the defaults is stored as nothing");
    }

    #[test]
    fn a_user_size_over_the_boards_limit_is_refused_but_a_schematic_may_have_it() {
        let big = PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_um: Some((1_500_000, 900_000)) };
        let mut d = design(true);
        assert!(set_board_page(&mut d, &big, &TitleBlock::default()).is_err());
        set_schematic_page(&mut d, &big, &TitleBlock::default()).unwrap();
        let sch = d.schematic.as_ref().unwrap();
        assert_eq!(sch.extras.page, Some(big));
        assert_eq!(sch.title_block.as_ref().map(|t| t.paper.as_str()), Some("User"));
    }

    #[test]
    fn a_standard_landscape_paper_is_only_its_name_on_a_sheet() {
        let mut d = design(true);
        let a3_landscape = PageSettings { paper: "A3".into(), portrait: false, user_size_um: None };
        set_schematic_page(&mut d, &a3_landscape, &TitleBlock::default()).unwrap();
        let sch = d.schematic.as_ref().unwrap();
        assert_eq!(sch.extras.page, None);
        assert_eq!(sch.title_block, Some(TitleBlock { paper: "A3".into(), ..Default::default() }), "the title block exists to carry the name");
        assert_eq!(PageSettings::of_sheet(sch.extras.page.as_ref(), &sch.title_block.as_ref().unwrap().paper), a3_landscape);
        assert!(set_schematic_page(&mut d, &a3_landscape, &TitleBlock::default()).is_err(), "the same again");
        // back to A4: stored as nothing, the title block with it
        set_schematic_page(&mut d, &PageSettings::default(), &TitleBlock::default()).unwrap();
        let sch = d.schematic.as_ref().unwrap();
        assert_eq!((sch.extras.page.as_ref(), sch.title_block.as_ref()), (None, None));
    }

    #[test]
    fn the_layout_engines_paper_survives_a_title_block_edit() {
        // The engine named the sheet's paper A3; the dialog sends A3 with the new fields: the name stays, the fields change.
        let mut d = design(true);
        d.schematic.as_mut().unwrap().title_block = Some(TitleBlock { paper: "A3".into(), ..Default::default() });
        let a3_landscape = PageSettings { paper: "A3".into(), portrait: false, user_size_um: None };
        set_schematic_page(&mut d, &a3_landscape, &tb("Renamed", &[])).unwrap();
        assert_eq!(d.schematic.as_ref().unwrap().title_block, Some(TitleBlock { paper: "A3".into(), ..tb("Renamed", &[]) }));
    }

    #[test]
    fn the_schematic_keeps_its_own_paper_and_title_block() {
        let mut d = design(true);
        set_schematic_page(&mut d, &a3(), &tb("Sheet", &[])).unwrap();
        let sch = d.schematic.as_ref().unwrap();
        assert_eq!(sch.extras.page, Some(a3()), "a portrait paper is more than its name");
        assert_eq!(sch.title_block, Some(TitleBlock { paper: "A3".into(), ..tb("Sheet", &[]) }), "the name travels with the title block");
        assert!(d.drawings.is_none(), "the board's settings are untouched");
        assert!(set_schematic_page(&mut design(false), &a3(), &TitleBlock::default()).is_err(), "no schematic to set it on");
    }
}
