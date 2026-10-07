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
use eda_model::ir::{Design, SchematicSection};
use serde_json::Value;
use std::path::Path;

fn bool_of(req: &Value, key: &str, default: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// KiCad's page number for the sheet at `path` (placement ids from the
/// root down): the root is page 1, then every sheet in depth-first order
/// (`SCH_SHEET_LIST::BuildSheetList`).
pub(crate) fn page_of(design: &Design, path: &[&str]) -> Option<usize> {
    fn visit<'a>(design: &'a Design, sch: &'a SchematicSection, ids: &mut Vec<&'a str>, n: &mut usize, want: &[&str]) -> Option<usize> {
        if ids.len() > 32 {
            return None; // a file that (indirectly) places itself
        }
        for child in &sch.sheets {
            let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) else { continue };
            *n += 1;
            ids.push(child.id.as_str());
            if ids.as_slice() == want {
                return Some(*n);
            }
            if let Some(p) = visit(design, content, ids, n, want) {
                return Some(p);
            }
            ids.pop();
        }
        None
    }
    let root = design.schematic.as_ref()?;
    if path.is_empty() {
        return Some(1);
    }
    visit(design, root, &mut Vec::new(), &mut 1, path)
}

/// `kicad-cli sch export svg|pdf` arguments for the plot dialog's options.
pub(crate) fn plot_args(req: &Value, page: Option<usize>) -> Vec<String> {
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
            a.extend(["--pages".into(), p.to_string()]);
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
        assert_eq!(plot_args(&json!({ "plot_all": false }), Some(1)), vec!["--pages", "1"]);
        assert_eq!(plot_args(&json!({ "pages": ["2", "3"] }), None), vec!["--pages", "2,3"]);
    }
}
