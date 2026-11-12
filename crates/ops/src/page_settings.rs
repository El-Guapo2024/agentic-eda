//! Page Settings (`common.Control.pageSettings`): the paper and the title block of the board or of the schematic, as
//! one undo step each. `DIALOG_PAGES_SETTINGS::SavePageSettings` (common/dialogs/dialog_page_settings.cpp) does
//! `m_parent->SetPageSettings( m_pageInfo )` and `SetTitleBlock( m_tb )` together, and `BOARD_EDITOR_CONTROL::PageSettings` /
//! `SCH_EDITOR_CONTROL::PageSetup` wrap both in one undo item ("Page Settings"); [`crate::Board::apply`] calls these for
//! `Cmd::SetBoardPage` and `Cmd::SetSchematicPage`.
//!
//! The schematic dialog's "export to other sheets" boxes are [`PageExport`] ([`set_schematic_page_on`]); the drawing sheet file is its own verb
//! (`crate::drawing_sheet`, outside the undo history as KiCad's project setting is). A change that changes nothing is an error, so no empty undo
//! step is pushed.

use eda_model::ir::{Design, TitleBlock};
use eda_model::page::{PageSettings, MAX_PAGE_SIZE_EESCHEMA_NM, MAX_PAGE_SIZE_PCBNEW_NM};
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
    page.validate(MAX_PAGE_SIZE_PCBNEW_NM).map_err(|m| fail("ops_page_settings", "page", &m))?;
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
    set_schematic_page_on(design, None, page, title_block, &PageExport::default())
}

/// The schematic Page Settings' "Export to other sheets" boxes (KiCad 25ec2335 `DIALOG_EESCHEMA_PAGE_SETTINGS`: `m_PaperExport`, `m_RevisionExport`,
/// `m_DateExport`, `m_TitleExport`, `m_CompanyExport`, `m_Comment1Export` .. `m_Comment9Export`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageExport {
    #[serde(default)]
    pub paper: bool,
    #[serde(default)]
    pub revision: bool,
    #[serde(default)]
    pub date: bool,
    #[serde(default)]
    pub title: bool,
    #[serde(default)]
    pub company: bool,
    /// Comment 1 .. 9.
    #[serde(default)]
    pub comments: Vec<bool>,
}

impl PageExport {
    pub fn is_empty(&self) -> bool {
        !(self.paper || self.revision || self.date || self.title || self.company || self.comments.iter().any(|c| *c))
    }
}

/// [`set_schematic_page`] on the screen in view (`focus`: a `sheet_contents` key, `None` the root) -- `SCH_EDITOR_CONTROL::PageSetup` edits the current screen
/// (`m_screen`) -- with `onSavePageSettings`' export: every other screen of the schematic (`SCH_SCREENS( Root() )`) takes the paper (`SetPageSettings(
/// m_pageInfo )`) and each title block field whose box is ticked (`tb2.SetRevision( m_tb.GetRevision() )` ..., `SetComment( i, .. )`), the rest of its title
/// block as it was. Refused when nothing changes anywhere.
pub fn set_schematic_page_on(design: &mut Design, focus: Option<&str>, page: &PageSettings, title_block: &TitleBlock, export: &PageExport) -> Result<(), Vec<CheckResult>> {
    page.validate(MAX_PAGE_SIZE_EESCHEMA_NM).map_err(|m| fail("ops_page_settings", "page", &m))?;
    let no_schematic = || fail("ops_no_schematic", "schematic", "this board has no schematic section yet");
    if design.schematic.is_none() {
        return Err(no_schematic());
    }
    let (name, full) = page.sheet_parts();
    let new_tb = normalized(&TitleBlock { paper: name.clone(), ..title_block.clone() });
    let mut changed = false;
    {
        let sch = match focus {
            None => design.schematic.as_mut(),
            Some(file) => design.sheet_contents.as_mut().and_then(|c| c.get_mut(file)),
        }
        .ok_or_else(no_schematic)?;
        if sch.extras.page != full || sch.title_block != new_tb {
            sch.extras.page = full.clone();
            sch.title_block = new_tb.clone();
            changed = true;
        }
    }
    if !export.is_empty() {
        let src = new_tb.clone().unwrap_or_default();
        let others = design.schematic.iter_mut().filter(|_| focus.is_some()).chain(design.sheet_contents.iter_mut().flat_map(|c| c.iter_mut().filter(|(k, _)| Some(k.as_str()) != focus).map(|(_, v)| v)));
        for screen in others {
            let mut tb2 = screen.title_block.clone().unwrap_or_default();
            let page_before = screen.extras.page.clone();
            if export.paper {
                screen.extras.page = full.clone();
                tb2.paper = name.clone();
            }
            if export.revision {
                tb2.rev = src.rev.clone();
            }
            if export.date {
                tb2.date = src.date.clone();
            }
            if export.title {
                tb2.title = src.title.clone();
            }
            if export.company {
                tb2.company = src.company.clone();
            }
            for (i, on) in export.comments.iter().enumerate().take(9) {
                if *on {
                    if tb2.comments.len() <= i {
                        tb2.comments.resize(i + 1, String::new());
                    }
                    tb2.comments[i] = src.comments.get(i).cloned().unwrap_or_default();
                }
            }
            let tb2 = normalized(&tb2);
            if tb2 != screen.title_block || screen.extras.page != page_before {
                screen.title_block = tb2;
                changed = true;
            }
        }
    }
    if !changed {
        return Err(fail("ops_page_settings", "page", "the page settings are already like this"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
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
        PageSettings { paper: "A3".into(), portrait: true, user_size_nm: None }
    }

    fn tb(title: &str, comments: &[&str]) -> TitleBlock {
        TitleBlock { title: title.into(), comments: comments.iter().map(|c| c.to_string()).collect(), ..Default::default() }
    }

    /// CC8: `onSavePageSettings`' "Export to other sheets": the screen in view takes the dialog's page and title block, every other screen the paper and the
    /// fields whose box is ticked, the rest of its title block kept.
    #[test]
    fn the_export_boxes_carry_the_paper_and_the_ticked_fields_to_every_other_sheet() {
        let mut d = design(true);
        let other: eda_model::ir::SchematicSection = serde_json::from_value(serde_json::json!({ "symbols": [], "wires": [], "labels": [] })).unwrap();
        d.sheet_contents = Some([("sub.kicad_sch".to_string(), other)].into());
        d.sheet_contents.as_mut().unwrap().get_mut("sub.kicad_sch").unwrap().title_block = Some(TitleBlock { title: "Sub".into(), rev: "A".into(), comments: vec!["c1".into(), "c2".into()], ..Default::default() });
        let export = PageExport { paper: true, revision: true, comments: vec![false, true], ..Default::default() };
        let tb = TitleBlock { title: "Root".into(), rev: "B".into(), comments: vec!["x1".into(), "x2".into()], ..Default::default() };
        set_schematic_page_on(&mut d, None, &a3(), &tb, &export).unwrap();
        let sub = &d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"];
        let sub_tb = sub.title_block.clone().unwrap();
        assert_eq!((sub_tb.title.as_str(), sub_tb.rev.as_str(), sub_tb.comments.clone()), ("Sub", "B", vec!["c1".to_string(), "x2".to_string()]), "the ticked fields only");
        assert_eq!(PageSettings::of_sheet(sub.extras.page.as_ref(), &sub_tb.paper), a3(), "and the paper");
        assert_eq!(d.schematic.as_ref().unwrap().title_block.as_ref().unwrap().title, "Root");
        // on the sheet in view: that screen takes the dialog's, the root nothing without a box ticked
        let tb2 = TitleBlock { title: "Sub 2".into(), ..Default::default() };
        set_schematic_page_on(&mut d, Some("sub.kicad_sch"), &a3(), &tb2, &PageExport::default()).unwrap();
        assert_eq!(d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].title_block.as_ref().unwrap().title, "Sub 2");
        assert_eq!(d.schematic.as_ref().unwrap().title_block.as_ref().unwrap().title, "Root");
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
        let big = PageSettings { paper: USER_PAPER.into(), portrait: false, user_size_nm: Some((mm(1500.0), mm(900.0))) };
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
        let a3_landscape = PageSettings { paper: "A3".into(), portrait: false, user_size_nm: None };
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
        let a3_landscape = PageSettings { paper: "A3".into(), portrait: false, user_size_nm: None };
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
