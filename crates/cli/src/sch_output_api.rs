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

/// The colour themes kicad-cli plots with, in the order the dialog's "Color theme:" lists them (`SETTINGS_MANAGER::GetColorSettingsList`, sorted
/// by name): the two built-in ones (`COLOR_SETTINGS::CreateBuiltinColorSettings`), each by the key `--theme` takes and the name it shows. kicad-cli
/// runs on a settings folder of its own (`eda_kicad_engine::cli_command`), so it knows no user or system theme.
pub(crate) const THEMES: [(&str, &str); 2] = [("_builtin_classic", "KiCad Classic"), ("_builtin_default", "KiCad Default")];

/// The Plot Schematic dialog's formats (`DIALOG_PLOT_SCHEMATIC::getPlotFileFormat`: Postscript, PDF, SVG, DXF, PNG) by kicad-cli's
/// `sch export` command. HPGL is no format of the dialog ("no longer supported"); kicad-cli still has `sch export hpgl`, whose pen options do
/// nothing ("Deprecated.  Has no effect."), so it is not offered either.
pub(crate) const FORMATS: [&str; 5] = ["ps", "pdf", "svg", "dxf", "png"];

/// `kicad-cli sch export ps|pdf|svg|dxf|png` arguments for the plot dialog's options (`DIALOG_PLOT_SCHEMATIC::plotSchematic`'s `SCH_PLOT_OPTS`,
/// which kicad-cli's job fills the same way, `CLI::SCH_EXPORT_PLOT_COMMAND::doPerform`):
/// - "Output mode:" Black and White (`--black-and-white`); in colour, "Color theme:" (`--theme`, a [`THEMES`] key);
/// - "Plot drawing sheet" (`--exclude-drawing-sheet` when off);
/// - "Plot background color" (`--no-background-color` when off), for the formats the dialog enables it on (`onColorMode`: PostScript, PDF, SVG,
///   PNG; kicad-cli reads the flag on those four only) and only in colour (`m_useBackgroundColor && plotter->GetColorMode()`);
/// - PDF Options (`--exclude-pdf-property-popups`, `--exclude-pdf-hierarchical-links`, `--exclude-pdf-metadata` when off);
/// - PNG Options (`--dpi`, `--no-antialias`);
/// - Plot Current Page (`--pages` with the sheet's page number, [`page_of`]).
///
/// Differs (no kicad-cli flag; the dialog shows each disabled with the reason): "Page size:" (kicad-cli plots at the schematic's size,
/// `m_pageSizeSelect = PAGE_SIZE_AUTO`), "Minimum line width:", DXF's "Export units:" (kicad-cli always exports in mm, as KiCad's own comment
/// in `onPlotFormatSelection` says), "Open file after plot" and the design variant (the studio has none). The hop-over and the default font
/// are the defaults of both (no hop-over scale, the KiCad font).
pub(crate) fn plot_args_for(format: &str, req: &Value, page: Option<String>) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let color = bool_of(req, "color", true);
    if !color {
        a.push("--black-and-white".into());
    } else if let Some(theme) = req.get("theme").and_then(Value::as_str).filter(|t| THEMES.iter().any(|(k, _)| k == t)) {
        a.extend(["--theme".into(), theme.into()]);
    }
    if !bool_of(req, "plot_drawing_sheet", true) {
        a.push("--exclude-drawing-sheet".into());
    }
    // `m_useBackgroundColor` only has an effect in colour mode.
    if format != "dxf" && color && !bool_of(req, "background", true) {
        a.push("--no-background-color".into());
    }
    if format == "pdf" {
        for (key, flag) in [("pdf_property_popups", "--exclude-pdf-property-popups"), ("pdf_hierarchical_links", "--exclude-pdf-hierarchical-links"), ("pdf_metadata", "--exclude-pdf-metadata")] {
            if !bool_of(req, key, true) {
                a.push(flag.into());
            }
        }
    }
    if format == "png" {
        // `m_pngDPI`: a spin control of 72 .. 2400 (`MIN_PNG_DPI`, `MAX_PNG_DPI`), 300 to start.
        let dpi = req.get("png_dpi").and_then(Value::as_i64).unwrap_or(300).clamp(72, 2400);
        a.extend(["--dpi".into(), dpi.to_string()]);
        if !bool_of(req, "png_antialias", true) {
            a.push("--no-antialias".into());
        }
    }
    a.extend(pages_args(req, page));
    a
}

/// `--pages`: the pages asked for, else the current page's (Plot Current Page).
fn pages_args(req: &Value, page: Option<String>) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
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

/// `POST /api/sch/plot`: `{"format": "ps"|"pdf"|"svg"|"dxf"|"png", "color": bool, "theme": "_builtin_default", "plot_drawing_sheet": bool,
/// "background": bool, "pdf_property_popups": bool, "pdf_hierarchical_links": bool, "pdf_metadata": bool, "png_dpi": n, "png_antialias": bool,
/// "plot_all": bool, "sheet_path": "<placement id>/<id>/..." (when `plot_all` is false), "pages": ["1", "3"], "output_directory": ".."}` --
/// the `DIALOG_PLOT_SCHEMATIC` options (`SCH_PLOT_OPTS`) kicad-cli has ([`plot_args_for`]). Omitted fields take `SCH_PLOT_OPTS`'s own
/// defaults (colour, drawing sheet, background, every sheet); an omitted format is SVG. The output directory is the dialog's (relative to
/// the board's folder, KiCad's "relative to the schematic main file location", [`crate::fab_api::output_folder`]); empty is
/// `export/kicad/sch-<format>/`.
pub fn plot(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let kind = match req.get("format").and_then(Value::as_str) {
        None => "svg",
        Some(f) => match FORMATS.iter().find(|k| **k == f) {
            Some(k) => k,
            None => return err(format!("unknown plot format {f:?} (expected ps, pdf, svg, dxf or png)")),
        },
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
    let out_dir = match output_dir_of(dir, &req, &format!("sch-{kind}")) {
        Ok(d) => d,
        Err(why) => return err(why),
    };
    let mut v = reply(kicad_engine::export_sch_into(dir, kind, &plot_args_for(kind, &req, page), &out_dir));
    if v["ok"].as_bool().unwrap_or(false) {
        v["format"] = Value::String(kind.into());
    }
    v
}

/// The dialog's "Output directory:" (`output_directory`), resolved as the board's plot folder is ([`crate::fab_api::output_folder`], with the
/// schematic's text variables as `DIALOG_PLOT_SCHEMATIC::getOutputPath` expands them: here the board's, which carry the same title block and
/// project variables).
fn output_dir_of(dir: &Path, req: &Value, default_kind: &str) -> Result<std::path::PathBuf, String> {
    let typed = req.get("output_directory").and_then(Value::as_str).unwrap_or("");
    crate::fab_api::request_output_folder(dir, typed, default_kind)
}

/// The Export Netlist dialog's pages (`DIALOG_EXPORT_NETLIST`'s notebook: KiCad, OrcadPCB2, Allegro, CadStar, PADS, SPICE, SPICE Model) by the
/// studio's name, with kicad-cli's `--format` for each (`JOB_EXPORT_SCH_NETLIST::FORMAT`). `xml` (`kicadxml`, the intermediate netlist) is
/// kicad-cli's too but no page of KiCad's dialog (it feeds the BOM and custom exporters).
pub(crate) const NETLIST_FORMATS: [(&str, &str); 8] = [
    ("kicad", "kicadsexpr"),
    ("orcadpcb2", "orcadpcb2"),
    ("allegro", "allegro"),
    ("cadstar", "cadstar"),
    ("pads", "pads"),
    ("spice", "spice"),
    ("spicemodel", "spicemodel"),
    ("xml", "kicadxml"),
];

/// `POST /api/sch/netlist`: `{"format": "kicad"|"orcadpcb2"|"allegro"|"cadstar"|"pads"|"spice"|"spicemodel"|"xml"}` -- `kicad-cli sch export
/// netlist --format ..` ([`NETLIST_FORMATS`]), named `<name>.<ext>` by the format (`eda_kicad_engine::netlist_ext`, `FilenamePrms`' extension;
/// KiCad's dialog asks for the name in a save dialog that starts with it). The SPICE page's options ("Use current sheet as root", the four
/// "Save all .." directives, "Run external simulator command:") have no kicad-cli flag: its job writes the netlist with the simulation command
/// and none of them (`EESCHEMA_JOBS_HANDLER::JobExportNetlist`: `OPTION_SIM_COMMAND`), which is the page with every box unchecked.
pub fn netlist(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = req.get("format").and_then(Value::as_str).unwrap_or("kicad");
    let Some((_, format)) = NETLIST_FORMATS.iter().find(|(n, cli)| *n == name || *cli == name) else {
        return err(format!("unknown netlist format {name:?} (expected kicad, orcadpcb2, allegro, cadstar, pads, spice, spicemodel or xml)"));
    };
    reply(kicad_engine::export_sch(dir, "netlist", &["--format".to_string(), format.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// X3 (UI-LEFT): every format of the dialog and each of its options that kicad-cli has.
    #[test]
    fn every_schematic_plot_format_and_option_becomes_kicad_clis_flag() {
        assert_eq!(plot_args_for("pdf", &json!({ "theme": "_builtin_classic" }), None), vec!["--theme", "_builtin_classic"], "colour, the theme, the background plotted");
        assert_eq!(plot_args_for("pdf", &json!({ "theme": "nonsense" }), None), Vec::<String>::new(), "a theme kicad-cli does not know is not passed");
        assert_eq!(plot_args_for("svg", &json!({ "color": false, "theme": "_builtin_classic", "background": false }), None), vec!["--black-and-white"], "the theme and the background are colour-mode options");
        assert_eq!(plot_args_for("ps", &json!({ "background": false }), None), vec!["--no-background-color"]);
        assert!(plot_args_for("dxf", &json!({ "background": false }), None).is_empty(), "DXF has no background option (onColorMode, kicad-cli)");
        assert_eq!(
            plot_args_for("pdf", &json!({ "pdf_property_popups": false, "pdf_hierarchical_links": false, "pdf_metadata": false }), None),
            vec!["--exclude-pdf-property-popups", "--exclude-pdf-hierarchical-links", "--exclude-pdf-metadata"]
        );
        assert!(plot_args_for("svg", &json!({ "pdf_metadata": false }), None).is_empty(), "the PDF options are the PDF's");
        assert_eq!(plot_args_for("png", &json!({}), None), vec!["--dpi", "300"]);
        assert_eq!(plot_args_for("png", &json!({ "png_dpi": 5000, "png_antialias": false }), None), vec!["--dpi", "2400", "--no-antialias"], "the spin control's range");
        assert_eq!(plot_args_for("pdf", &json!({ "plot_all": false, "plot_drawing_sheet": false }), Some("2".into())), vec!["--exclude-drawing-sheet", "--pages", "2"]);
        assert_eq!(THEMES.map(|(_, n)| n), ["KiCad Classic", "KiCad Default"], "sorted by name, as GetColorSettingsList sorts them");
    }

    #[test]
    fn plot_options_become_kicad_cli_flags() {
        assert!(plot_args_for("pdf", &json!({}), None).is_empty(), "defaults are kicad-cli's own");
        assert_eq!(plot_args_for("pdf", &json!({ "color": false, "plot_drawing_sheet": false }), None), vec!["--black-and-white", "--exclude-drawing-sheet"]);
        assert_eq!(plot_args_for("pdf", &json!({ "background": false }), None), vec!["--no-background-color"]);
        // No background flag in black and white: it only applies in colour mode.
        assert_eq!(plot_args_for("pdf", &json!({ "color": false, "background": false }), None), vec!["--black-and-white"]);
        assert_eq!(plot_args_for("pdf", &json!({ "plot_all": false }), Some("1".into())), vec!["--pages", "1"]);
        assert_eq!(plot_args_for("pdf", &json!({ "pages": ["2", "3"] }), None), vec!["--pages", "2,3"]);
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
        assert_eq!(plot_args_for("pdf", &json!({ "plot_all": false }), Some("5".into())), vec!["--pages", "5"]);
    }
}
