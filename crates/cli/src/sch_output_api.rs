//! `/api/sch/*`: the backend for the studio's File > Plot... (eeschema's
//! `DIALOG_PLOT_SCHEMATIC`) and File > Export > Netlist...
//! (`DIALOG_EXPORT_NETLIST`) dialogs (`web/studio/src/components/
//! PlotSchematicDialog.tsx`, `ExportNetlistDialog.tsx`).
//!
//! Plots and netlists are kicad-cli's job (docs/ARCHITECTURE.md,
//! "Engines"): each dialog's JSON becomes `kicad-cli sch export svg|pdf|
//! netlist` arguments and runs through `crate::kicad_engine` on the
//! exported schematic. Nothing here draws or writes a plot or a netlist
//! itself. Output lands in `<dir>/export/kicad/sch-<kind>/`; the reply
//! names the files relative to `dir`. Both are read-only exports: nothing
//! edits `design.json`, so there is no `/api/cmd` verb or undo entry.
//!
//! A board started from an intent alone has no `schematic` section yet:
//! like `GET /api/schematic.svg`, the schematic the engine would derive
//! from the intent is what gets plotted/exported, so the dialogs work on a
//! fresh board.

use crate::fab_api::{err, reply};
use crate::kicad_engine;
use eda_model::ir::Design;
use serde_json::Value;
use std::path::Path;

fn bool_of(req: &Value, key: &str, default: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// The page number of the sheet at `path` (placement ids from the root down) as the derived file stores it, which is what kicad-cli's
/// `--pages` matches: `SCH_SHEET_LIST::TrimToPageNumbers` keeps a sheet when `sheet.GetPageNumber()` is in the list. That is the number the
/// user gave the sheet (Edit Sheet Page Number, an imported file's `(page ..)`), else its virtual page in the walk's order
/// (`SheetEntry::effective_page`, the number the writer writes; the walk orders each screen's sheets by place, `SCH_SCREEN::GetSheets`).
/// `None`: no sheet at `path`.
///
/// KiCad's "Plot Current Page" plots `m_schematic->CurrentSheet()` itself (`SCH_PLOTTER::createSVGFiles` and the other formats:
/// `sheetList.push_back( m_schematic->CurrentSheet() )`); kicad-cli has no sheet-path option, so its page number stands for it. The one
/// difference: two sheets with the same page number are both plotted. (Round 4's X4-2: this used to be the sheet's place in the stored list,
/// counted over sheets with a screen only, so a renumbered or reordered sheet plotted another page.)
pub(crate) fn page_of(design: &Design, path: &[&str]) -> Option<String> {
    design.schematic.as_ref()?;
    eda_model::sheet_list::walk(design).into_iter().find(|e| e.path.iter().map(String::as_str).eq(path.iter().copied())).map(|e| e.effective_page())
}

/// `kicad-cli sch export svg|pdf` arguments for the plot dialog's options.
pub(crate) fn plot_args(req: &Value, page: Option<String>) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if !bool_of(req, "color", true) {
        a.push("--black-and-white".into());
    }
    if !bool_of(req, "plot_drawing_sheet", true) {
        a.push("--exclude-drawing-sheet".into());
    }
    // `m_useBackgroundColor` only has an effect in colour mode.
    if bool_of(req, "color", true) && !bool_of(req, "background", true) {
        a.push("--no-background-color".into());
    }
    let pages: Vec<String> = req.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if !pages.is_empty() {
        a.extend(["--pages".into(), pages.join(",")]);
    } else if !bool_of(req, "plot_all", true) {
        if let Some(p) = page {
            a.extend(["--pages".into(), p]);
        }
    }
    a
}

/// `POST /api/sch/plot`: `{"format": "svg"|"pdf", "color": bool,
/// "plot_drawing_sheet": bool, "background": bool, "plot_all": bool,
/// "sheet_path": "<placement id>/<id>/..." (when `plot_all` is false),
/// "pages": ["1", "3"]}` -- the `DIALOG_PLOT_SCHEMATIC` options
/// (`SCH_PLOT_OPTS`) kicad-cli has. Omitted fields take `SCH_PLOT_OPTS`'s
/// own defaults (colour, drawing sheet, background, every sheet).
pub fn plot(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let kind = match req.get("format").and_then(Value::as_str) {
        Some("pdf") => "pdf",
        Some("svg") | None => "svg",
        Some(other) => return err(format!("unknown plot format {other:?} (expected \"svg\" or \"pdf\")")),
    };
    let page = if bool_of(&req, "plot_all", true) {
        None
    } else {
        let (design, _) = match kicad_engine::load_with_schematic(dir) {
            Ok(v) => v,
            Err(e) => return err(crate::board::reasons(&e)),
        };
        let sheet_path = req.get("sheet_path").and_then(Value::as_str).unwrap_or("");
        let ids: Vec<&str> = sheet_path.split('/').filter(|s| !s.is_empty()).collect();
        match page_of(&design, &ids) {
            Some(p) => Some(p),
            None => return err(format!("no sheet at path {sheet_path:?}")),
        }
    };
    reply(kicad_engine::export_sch(dir, kind, &plot_args(&req, page)))
}

/// `POST /api/sch/netlist`: `{"format": "kicad"|"xml"}` -- KiCad's `.net`
/// (`kicadsexpr`) or the generic `.xml` (`kicadxml`).
pub fn netlist(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let format = match req.get("format").and_then(Value::as_str) {
        Some("xml") => "kicadxml",
        Some("kicad") | None => "kicadsexpr",
        Some(other) => return err(format!("unknown netlist format {other:?} (expected \"kicad\" or \"xml\")")),
    };
    reply(kicad_engine::export_sch(dir, "netlist", &["--format".to_string(), format.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plot_options_become_kicad_cli_flags() {
        assert!(plot_args(&json!({}), None).is_empty(), "defaults are kicad-cli's own");
        assert_eq!(plot_args(&json!({ "color": false, "plot_drawing_sheet": false }), None), vec!["--black-and-white", "--exclude-drawing-sheet"]);
        assert_eq!(plot_args(&json!({ "background": false }), None), vec!["--no-background-color"]);
        // No background flag in black and white: it only applies in colour mode.
        assert_eq!(plot_args(&json!({ "color": false, "background": false }), None), vec!["--black-and-white"]);
        assert_eq!(plot_args(&json!({ "plot_all": false }), Some("1".into())), vec!["--pages", "1"]);
        assert_eq!(plot_args(&json!({ "pages": ["2", "3"] }), None), vec!["--pages", "2,3"]);
    }

    /// Round 4's X4-2: "current page only" passes the sheet's own page number, which kicad-cli's `--pages` matches
    /// (`SCH_SHEET_LIST::TrimToPageNumbers` compares `GetPageNumber()`): A, drawn at x=100 and numbered "5", is page "5"; B, drawn at x=50 with
    /// no number, is the second sheet of the walk (`SCH_SCREEN::GetSheets` orders by x), so page "2", whatever the stored order.
    #[test]
    fn plot_current_page_passes_the_sheets_own_page_number() {
        use eda_model::ir::{Point, Provenance, SchematicSection, SheetInstance};
        use eda_model::units::Nm;
        let placement = |id: &str, x_mm: f64, page: &str| SheetInstance {
            id: id.into(),
            name: id.to_uppercase(),
            file: format!("{id}.kicad_sch"),
            at: Point { x: Nm::from_mm(x_mm), y: Nm::ZERO },
            size: (Nm::new(1000), Nm::new(1000)),
            pins: vec![],
            page: page.into(),
        };
        let root = SchematicSection { sheets: vec![placement("a", 100.0, "5"), placement("b", 50.0, "")], ..Default::default() };
        let contents = [("a.kicad_sch".to_string(), SchematicSection::default()), ("b.kicad_sch".to_string(), SchematicSection::default())].into_iter().collect();
        let design = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: Some(contents),
            bus_aliases: vec![],
            symbol_library: None,
        };
        assert_eq!(page_of(&design, &["a"]).as_deref(), Some("5"));
        assert_eq!(page_of(&design, &["b"]).as_deref(), Some("2"));
        assert_eq!(page_of(&design, &[]).as_deref(), Some("1"));
        assert_eq!(page_of(&design, &["c"]), None);
        assert_eq!(plot_args(&json!({ "plot_all": false }), Some("5".into())), vec!["--pages", "5"]);
    }
}
