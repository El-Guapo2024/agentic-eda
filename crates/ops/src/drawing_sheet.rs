//! Page Settings' "Drawing sheet:" (`DIALOG_PAGES_SETTINGS::OnWksFileSelection` / `SavePageSettings`, common/dialogs/dialog_page_settings.cpp, commit
//! 8303b2ad): the `.kicad_wks` file the board or the schematic draws its frame and title block from, as one undo step ([`crate::Cmd::SetDrawingSheet`]).
//!
//! KiCad keeps the name in the project (`BASE_SCREEN::m_DrawingSheetFileName`, saved as `pcbnew.page_layout_descr_file` by the board editor and
//! `schematic.page_layout_descr_file` by the schematic editor) -- relative to the project when the file is in it (`fn.MakeRelativeTo( m_projectPath )`),
//! else shortened with a path variable (`NormalizePath`), else whole; an empty name is the default sheet. The studio picks the file and its short name
//! (`crates/cli/src/drawing_sheet_api.rs`); this verb keeps what it is given in the design (`DrawingsSection::drawing_sheet`,
//! `SchExtras::drawing_sheet`), the source the derived `.kicad_pro` and kicad-cli's `--drawing-sheet` are written from.
//!
//! Differs: KiCad's dialog changes the file outside the undo stack (a project setting), together with the paper and the title block on OK; here it is
//! an undoable verb of its own, which the dialog sends in one batch with `set_board_page` / `set_schematic_page` when both change. A change that
//! changes nothing is refused, so no empty undo step is pushed.

use eda_model::ir::Design;
use eda_model::CheckResult;

fn fail(check: &str, hint: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, "drawing_sheet", hint)]
}

/// The name as kept: trimmed, and `None` for nothing (`m_DrawingSheetFileName` empty -- KiCad's default sheet). A name must be a `.kicad_wks` file
/// (`FILEEXT::DrawingSheetFileWildcard`, the only files the dialog's browser offers) and one line.
pub fn normalized(path: Option<&str>) -> Result<Option<String>, Vec<CheckResult>> {
    let Some(p) = path.map(str::trim).filter(|p| !p.is_empty()) else { return Ok(None) };
    if p.contains(['\n', '\r', '"']) {
        return Err(fail("ops_bad_drawing_sheet", "a drawing sheet file name is one line, with no double quote"));
    }
    if !p.to_ascii_lowercase().ends_with(".kicad_wks") {
        return Err(fail("ops_bad_drawing_sheet", &format!("'{p}' is not a drawing sheet file (*.kicad_wks)")));
    }
    Ok(Some(p.to_string()))
}

/// The board's (`schematic` false) or the schematic's drawing sheet file. Refused when it is already that one, or when there is no schematic to set it on.
pub fn set_drawing_sheet(design: &mut Design, schematic: bool, path: Option<&str>) -> Result<(), Vec<CheckResult>> {
    let new = normalized(path)?;
    if schematic {
        let sch = design.schematic.as_mut().ok_or_else(|| fail("ops_no_schematic", "this board has no schematic section yet"))?;
        if sch.extras.drawing_sheet == new {
            return Err(fail("ops_drawing_sheet", "the drawing sheet is already this one"));
        }
        sch.extras.drawing_sheet = new;
    } else {
        let current = design.drawings.as_ref().and_then(|d| d.drawing_sheet.clone());
        if current == new {
            return Err(fail("ops_drawing_sheet", "the drawing sheet is already this one"));
        }
        design.drawings.get_or_insert_with(Default::default).drawing_sheet = new;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built from JSON so a field the IR gains later (with its serde default) does not break these.
    fn design(with_schematic: bool) -> Design {
        let mut v = serde_json::json!({ "schema": 1, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 }, "routing": { "tracks": [], "vias": [] } });
        if with_schematic {
            v["schematic"] = serde_json::json!({ "symbols": [], "wires": [], "labels": [] });
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn the_board_and_the_schematic_each_keep_their_own_sheet() {
        let mut d = design(true);
        set_drawing_sheet(&mut d, false, Some(" sheets/frame.kicad_wks ")).unwrap();
        set_drawing_sheet(&mut d, true, Some("${KICAD10_TEMPLATE_DIR}/A3_ISO5457-1999_ISO7200-2004_EN.kicad_wks")).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().drawing_sheet.as_deref(), Some("sheets/frame.kicad_wks"), "trimmed, kept as given");
        assert_eq!(d.schematic.as_ref().unwrap().extras.drawing_sheet.as_deref(), Some("${KICAD10_TEMPLATE_DIR}/A3_ISO5457-1999_ISO7200-2004_EN.kicad_wks"));
        let back: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.drawings.unwrap().drawing_sheet.as_deref(), Some("sheets/frame.kicad_wks"), "kept in design.json");
    }

    #[test]
    fn nothing_is_the_default_sheet_and_an_unchanged_one_is_refused() {
        let mut d = design(false);
        assert!(set_drawing_sheet(&mut d, false, None).is_err(), "the default already");
        set_drawing_sheet(&mut d, false, Some("a.kicad_wks")).unwrap();
        assert!(set_drawing_sheet(&mut d, false, Some("a.kicad_wks")).is_err(), "the same again");
        set_drawing_sheet(&mut d, false, Some("")).unwrap();
        assert_eq!(d.drawings.as_ref().unwrap().drawing_sheet, None);
        assert!(set_drawing_sheet(&mut d, true, Some("a.kicad_wks")).is_err(), "no schematic yet");
    }

    #[test]
    fn only_a_drawing_sheet_file_is_taken() {
        let mut d = design(false);
        assert!(set_drawing_sheet(&mut d, false, Some("board.kicad_pcb")).is_err());
        assert!(set_drawing_sheet(&mut d, false, Some("a\nb.kicad_wks")).is_err());
        assert!(set_drawing_sheet(&mut d, false, Some("Frame.KICAD_WKS")).is_ok(), "the extension in any case");
    }
}
