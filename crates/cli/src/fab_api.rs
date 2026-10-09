//! `/api/fab/*`: the backend for the studio's Plot, Generate Drill Files and
//! Footprint Position Files dialogs (`web/studio/src/components/
//! PlotDialog.tsx`, `GenerateDrillDialog.tsx`, `FootprintPositionDialog.tsx`).
//!
//! Gerbers, drill files, position files and BOMs are kicad-cli's job
//! (docs/ARCHITECTURE.md, "Engines"): each dialog's JSON becomes kicad-cli
//! arguments (the `*_args` builders below, shared with the CLI's own `eda fab
//! ...` verbs, so a dialog and the CLI produce identical output for
//! identical input) and runs through `crate::kicad_engine`. Nothing here
//! writes a Gerber, a drill file or a placement file itself. Output lands
//! in `<dir>/export/kicad/<kind>/`; the reply names the files, relative to
//! `dir`, so the dialog can show them. `POST /api/fab/kicad` is the same
//! engine with raw arguments.

use crate::board;
use crate::kicad_engine;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// An engine result as the dialogs' reply: `{ ok, files, engine }` or
/// `{ ok: false, message }`.
pub(crate) fn reply(r: Result<Value, Vec<eda_model::CheckResult>>) -> Value {
    r.unwrap_or_else(|e| err(board::reasons(&e)))
}

/// kicad-cli's names for the layers a fab house wants by default: every
/// copper layer, mask, paste, silkscreen, the board outline (what JLCPCB's
/// own upload instructions ask for).
pub(crate) fn default_gerber_layers(copper: &[String]) -> Vec<String> {
    let mut v: Vec<String> = copper.to_vec();
    v.extend(["F.Mask", "B.Mask", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "Edge.Cuts"].map(String::from));
    v
}

/// `kicad-cli pcb export gerbers --layers ...`.
pub(crate) fn gerber_args(layers: &[String]) -> Vec<String> {
    vec!["--layers".into(), layers.join(",")]
}

/// `kicad-cli pcb export drill` (Excellon).
pub(crate) fn drill_args(separate_th: bool, report: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["--format".into(), "excellon".into()];
    if separate_th {
        a.push("--excellon-separate-th".into());
    }
    if report {
        a.push("--generate-report".into());
    }
    a
}

/// `kicad-cli pcb export pos`.
pub(crate) fn pos_args(format: &str, side: &str, units_mm: bool, smd_only: bool, exclude_fp_th: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["--format".into(), if format == "ascii" { "ascii" } else { "csv" }.into()];
    a.extend(["--side".into(), match side { "front" => "front", "back" => "back", _ => "both" }.into()]);
    a.extend(["--units".into(), if units_mm { "mm" } else { "in" }.into()]);
    if smd_only {
        a.push("--smd-only".into());
    }
    if exclude_fp_th {
        a.push("--exclude-fp-th".into());
    }
    a
}

/// `kicad-cli sch export bom` in JLCPCB's column layout (Comment,
/// Designator, Footprint, LCSC Part #), grouped by what makes two parts
/// interchangeable. A part marked Do Not Populate is not on an assembly BOM (`--exclude-dnp`), and one excluded from the BOM never is
/// (the symbol's `in_bom no`, which a footprint's "Exclude from bill of materials" sets too).
pub(crate) fn bom_args() -> Vec<String> {
    ["--fields", "Value,Reference,Footprint,LCSC", "--labels", "Comment,Designator,Footprint,LCSC Part #", "--group-by", "Value,Footprint,LCSC", "--sort-field", "Reference", "--exclude-dnp"].map(String::from).to_vec()
}

fn copper_layers(dir: &Path) -> Result<Vec<String>, String> {
    let (_, _, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    Ok(model.board.layers.clone())
}

/// `POST /api/fab/gerbers`: `{"layers": ["F.Cu", "B.Cu", ...]}` (omitted or
/// empty = [`default_gerber_layers`], the Plot dialog's own default
/// checklist).
pub fn gerbers(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let mut layers: Vec<String> = req.get("layers").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if layers.is_empty() {
        layers = match copper_layers(dir) {
            Ok(c) => default_gerber_layers(&c),
            Err(e) => return err(e),
        };
    }
    reply(kicad_engine::export(dir, "gerbers", &with_aux_origin("gerbers", gerber_args(&layers), &req)))
}

/// `use_aux_origin: true` in a plot dialog's JSON: measure from the drill/place file origin
/// (`pcbnew.EditorControl.drillOrigin`) instead of the board's absolute origin -- the Plot dialog's "Use drill/place file
/// origin", the drill dialog's "Drill origin" and the position dialog's "Position file origin". Each is a different
/// kicad-cli flag.
pub(crate) fn with_aux_origin(kind: &str, mut args: Vec<String>, req: &Value) -> Vec<String> {
    if req.get("use_aux_origin").and_then(Value::as_bool).unwrap_or(false) {
        match kind {
            "drill" => args.extend(["--drill-origin".to_string(), "plot".into()]),
            _ => args.push("--use-drill-file-origin".into()),
        }
    }
    args
}

/// `POST /api/fab/drill`: `{"separate_th": bool, "use_aux_origin": bool}`.
pub fn drill(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    reply(kicad_engine::export(dir, "drill", &with_aux_origin("drill", drill_args(req.get("separate_th").and_then(Value::as_bool).unwrap_or(false), false), &req)))
}

/// `POST /api/fab/pos`: `{"format": "csv"|"ascii", "side": "front"|"back"|"both",
/// "units_mm": bool, "smd_only": bool, "exclude_fp_th": bool, "exclude_dnp": bool}`. A footprint marked "exclude from position files" is never
/// in the file; the other three options leave out SMD-less, through-hole and Do-Not-Populate footprints.
pub fn pos(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let mut args = pos_args(
        req.get("format").and_then(Value::as_str).unwrap_or("csv"),
        req.get("side").and_then(Value::as_str).unwrap_or("both"),
        req.get("units_mm").and_then(Value::as_bool).unwrap_or(true),
        req.get("smd_only").and_then(Value::as_bool).unwrap_or(false),
        req.get("exclude_fp_th").and_then(Value::as_bool).unwrap_or(false),
    );
    // `PLACE_FILE_EXPORTER`'s `m_excludeDNP`: footprints marked Do Not Populate stay out of the file.
    if req.get("exclude_dnp").and_then(Value::as_bool).unwrap_or(false) {
        args.push("--exclude-dnp".into());
    }
    reply(kicad_engine::export(dir, "pos", &with_aux_origin("pos", args, &req)))
}

/// `POST /api/fab/bom`: no body.
pub fn bom(dir: &Path) -> Value {
    reply(kicad_engine::export_sch(dir, "bom", &bom_args()))
}

/// `POST /api/fab/kicad`: `{"kind": "<pcb export kind>", "args": [...]}`,
/// any `kicad-cli pcb export` subcommand with its own arguments.
pub fn kicad(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let kind = req["kind"].as_str().unwrap_or("");
    let args: Vec<String> = req["args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    reply(kicad_engine::export(dir, kind, &args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_plot_is_copper_mask_paste_silk_and_the_outline() {
        let l = default_gerber_layers(&["F.Cu".into(), "In1.Cu".into(), "B.Cu".into()]);
        assert_eq!(l.join(","), "F.Cu,In1.Cu,B.Cu,F.Mask,B.Mask,F.Paste,B.Paste,F.SilkS,B.SilkS,Edge.Cuts");
        assert_eq!(gerber_args(&l), vec!["--layers", "F.Cu,In1.Cu,B.Cu,F.Mask,B.Mask,F.Paste,B.Paste,F.SilkS,B.SilkS,Edge.Cuts"]);
    }

    #[test]
    fn drill_options_become_flags() {
        assert_eq!(drill_args(false, false), vec!["--format", "excellon"]);
        assert_eq!(drill_args(true, true), vec!["--format", "excellon", "--excellon-separate-th", "--generate-report"]);
    }

    #[test]
    fn the_drill_place_file_origin_is_a_different_flag_for_each_output() {
        let on = serde_json::json!({ "use_aux_origin": true });
        let off = serde_json::json!({});
        assert_eq!(with_aux_origin("gerbers", vec!["--layers".into(), "F.Cu".into()], &on), vec!["--layers", "F.Cu", "--use-drill-file-origin"]);
        assert_eq!(with_aux_origin("pos", vec![], &on), vec!["--use-drill-file-origin"]);
        assert_eq!(with_aux_origin("drill", drill_args(false, false), &on), vec!["--format", "excellon", "--drill-origin", "plot"]);
        assert_eq!(with_aux_origin("drill", drill_args(false, false), &off), vec!["--format", "excellon"], "unset: the absolute origin, kicad-cli's default");
    }

    #[test]
    fn position_options_become_flags() {
        assert_eq!(pos_args("ascii", "front", false, true, true), vec!["--format", "ascii", "--side", "front", "--units", "in", "--smd-only", "--exclude-fp-th"]);
        assert_eq!(pos_args("whatever", "sideways", true, false, false), vec!["--format", "csv", "--side", "both", "--units", "mm"]);
    }
}
