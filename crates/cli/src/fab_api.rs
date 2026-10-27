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

/// `kicad-cli pcb export gerbers --layers ...` with the board's plot settings (`plot`, the board's `PCB_PLOT_PARAMS`), which is what
/// KiCad's Plot dialog plots with (`DIALOG_PLOT::Plot` -> `m_editFrame->GetPlotSettings()`): each Gerber option of the block becomes
/// kicad-cli's flag for it (`PCB_PLOTTER::PlotJobToPlotOpts` is the way back), so a board that says `(usegerberextensions no)` -- KiCad's
/// default -- is plotted as `-F_Cu.gbr`, not as kicad-cli's own default `-F_Cu.gtl`.
///
/// Differs: kicad-cli's `--board-plot-params` would read the block itself, but it plots every copper layer the board has whatever the
/// selection says and ignores `--layers` (`PCBNEW_JOBS_HANDLER::JobExportGerbers`: `( GetLayerSelection() & AllNonCuMask() ) |
/// ( GetEnabledLayers() & AllCuMask() )`), where the dialog plots the layers checked in it; so the flags are passed one by one. The
/// legacy `plotreference`/`plotvalue` tokens are not read by `PCB_PLOT_PARAMS_PARSER` (both stay on), and kicad-cli always writes the
/// Gerber job file (`m_createJobsFile`), whatever `creategerberjobfile` says.
pub(crate) fn gerber_args(layers: &[String], plot: &eda_model::plot_params::PlotParams) -> Vec<String> {
    let mut a: Vec<String> = vec!["--layers".into(), layers.join(",")];
    let on = |key: &str, default: bool| plot.flag(key).unwrap_or(default);
    if !plot.plot_on_all_layers.is_empty() {
        a.extend(["--common-layers".into(), plot.plot_on_all_layers.join(",")]);
    }
    if !on("usegerberextensions", false) {
        a.push("--no-protel-ext".into());
    }
    if !on("usegerberattributes", true) {
        a.push("--no-x2".into());
    }
    if !on("usegerberadvancedattributes", true) {
        a.push("--no-netlist".into());
    }
    if on("disableapertmacros", false) {
        a.push("--disable-aperture-macros".into());
    }
    if on("subtractmaskfromsilk", false) {
        a.push("--subtract-soldermask".into());
    }
    // `case T_gerberprecision: parseInt( gbrDefaultPrecision - 1, gbrDefaultPrecision )`: clamped to 5..6, so 4 is 5.
    let precision = plot.item("gerberprecision").and_then(|i| i.trim_start_matches("(gerberprecision").trim_end_matches(')').trim().parse::<i32>().ok()).unwrap_or(6);
    a.extend(["--precision".into(), if precision <= 5 { "5" } else { "6" }.into()]);
    if on("plotframeref", false) {
        a.push("--include-border-title".into());
    }
    if on("sketchpadsonfab", false) {
        a.push("--sketch-pads-on-fab-layers".into());
        if on("plotpadnumbers", false) {
            a.push("--sketch-pad-numbers".into());
        }
    }
    // The three DNP flags as the dialog reads them back (`DIALOG_PLOT::init_Dialog`: "Plot DNP" is hide || sketch || crossout, then the
    // "Hide" radio if hide is set, else "Cross-out"; `applyPlotSettings`: hide = plot && hide, sketch = crossout = plot && cross-out).
    let (hide, sketch, crossout) = (on("hidednponfab", false), on("sketchdnponfab", true), on("crossoutdnponfab", true));
    if hide {
        a.push("--hide-DNP-footprints-on-fab-layers".into());
    } else if sketch || crossout {
        a.push("--sketch-DNP-footprints-on-fab-layers".into());
        a.push("--crossout-DNP-footprints-on-fab-layers".into());
    }
    a
}

/// The board's plot settings: an imported board's own block, else `PCB_PLOT_PARAMS`' defaults.
pub(crate) fn plot_params_of(design: &eda_model::ir::Design) -> eda_model::plot_params::PlotParams {
    eda_model::plot_params::PlotParams::of_design(design)
}

/// `kicad-cli pcb export drill` (Excellon), as the Generate Drill Files dialog asks for it (`DIALOG_GENDRILL::genDrillAndMapFiles`):
/// `merge_pth_npth` is "PTH and NPTH in single file" (`m_Check_Merge_PTH_NPTH`, unchecked by default, so two files), and oval holes are
/// routed (`SetRouteModeForOvalHoles( !m_altDrillMode )`, `G00`/`M15`..) unless "Use alternate drill mode" (`alt_drill_mode`, `G85`) is checked.
/// kicad-cli's own defaults are the other way round on both (`--excellon-oval-format alternate`, one file).
pub(crate) fn drill_args(merge_pth_npth: bool, alt_drill_mode: bool, report: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["--format".into(), "excellon".into()];
    a.extend(["--excellon-oval-format".into(), if alt_drill_mode { "alternate" } else { "route" }.into()]);
    if !merge_pth_npth {
        a.push("--excellon-separate-th".into());
    }
    if report {
        a.push("--generate-report".into());
    }
    a
}

/// `kicad-cli pcb export pos`. `format` is `ascii`, `csv` or `gerber` (Gerber X3, `CreateGerberFiles`: units and the footprint filters do
/// not apply to it, kicad-cli's `--help` says "ascii or csv only").
pub(crate) fn pos_args(format: &str, side: &str, units_mm: bool, smd_only: bool, exclude_fp_th: bool) -> Vec<String> {
    let format = match format {
        "ascii" => "ascii",
        "gerber" => "gerber",
        _ => "csv",
    };
    let mut a: Vec<String> = vec!["--format".into(), format.into()];
    a.extend(["--side".into(), match side { "front" => "front", "back" => "back", _ => "both" }.into()]);
    if format == "gerber" {
        return a;
    }
    a.extend(["--units".into(), if units_mm { "mm" } else { "in" }.into()]);
    if smd_only {
        a.push("--smd-only".into());
    }
    if exclude_fp_th {
        a.push("--exclude-fp-th".into());
    }
    a
}

/// The footprints in a placement file kicad-cli wrote: an ASCII file's lines that are not `#` comments, a CSV's lines after its header.
pub(crate) fn place_file_rows(text: &str, csv: bool) -> usize {
    let lines = text.lines().filter(|l| !l.trim().is_empty());
    if csv {
        lines.skip(1).count()
    } else {
        lines.filter(|l| !l.starts_with('#')).count()
    }
}

/// `CreateAsciiFiles`' report, word for word: each file with its component count, the full count of a two-file run, then "Done.". `files` are
/// (path, rows); two of them are the top's and the bottom's.
pub(crate) fn place_report(files: &[(String, usize)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    if files.len() == 1 {
        lines.push(format!("Placement file: '{}'.", files[0].0));
        lines.push(format!("Component count: {}.", files[0].1));
    } else {
        for (i, (f, n)) in files.iter().enumerate() {
            lines.push(if i == 0 { format!("Front (top side) placement file: '{f}'.") } else { format!("Back (bottom side) placement file: '{f}'.") });
            lines.push(format!("Component count: {n}."));
        }
        if files.len() == 2 {
            lines.push(format!("Full component count: {}.", files.iter().map(|(_, n)| n).sum::<usize>()));
        }
    }
    lines.push("Done.".into());
    lines.join("\n")
}

/// The footprints of a Gerber X3 placement file: one `%TO.CRot,` per component (`PLACEFILE_GERBER_WRITER::CreatePlaceFile`'s `cmp_count`).
pub(crate) fn gerber_place_count(text: &str) -> usize {
    text.lines().filter(|l| l.trim_start().starts_with("%TO.CRot,")).count()
}

/// `DIALOG_GEN_FOOTPRINT_POSITION::CreateGerberFiles`' report, word for word: the top's file and its count, the bottom's and its count, the
/// full count, then "Done.". `files` are (path, count).
pub(crate) fn gerber_place_report(files: &[(String, usize)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (i, (f, n)) in files.iter().enumerate() {
        let top = if files.len() == 2 { i == 0 } else { f.contains("-pnp_top") };
        lines.push(if top { format!("Front (top side) placement file: '{f}'.") } else { format!("Back (bottom side) placement file: '{f}'.") });
        lines.push(format!("Component count: {n}."));
    }
    lines.push(format!("Full component count: {}.", files.iter().map(|(_, n)| n).sum::<usize>()));
    lines.push("Done.".into());
    lines.join("\n")
}

/// `kicad-cli sch export bom` in JLCPCB's column layout (Comment,
/// Designator, Footprint, LCSC Part #), grouped by what makes two parts
/// interchangeable. A part marked Do Not Populate is not on an assembly BOM (`--exclude-dnp`), and one excluded from the BOM never is
/// (the symbol's `in_bom no`, which a footprint's "Exclude from bill of materials" sets too).
pub(crate) fn bom_args() -> Vec<String> {
    ["--fields", "Value,Reference,Footprint,LCSC", "--labels", "Comment,Designator,Footprint,LCSC Part #", "--group-by", "Value,Footprint,LCSC", "--sort-field", "Reference", "--exclude-dnp"].map(String::from).to_vec()
}

/// `POST /api/fab/gerbers`: `{"layers": ["F.Cu", "B.Cu", ...]}` (omitted or
/// empty = [`default_gerber_layers`], the Plot dialog's own default
/// checklist). `"check_zones": true` is the dialog's "Check zone fills before plotting": KiCad's filler runs first when some zone has no current fill
/// (`crate::fill_api::ensure_filled`), so the copper plotted is the pour as it is now.
pub fn gerbers(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if req.get("check_zones").and_then(Value::as_bool).unwrap_or(false) {
        if let Err(why) = crate::fill_api::ensure_filled(dir) {
            return err(format!("Zones not filled before plotting: {why}"));
        }
    }
    let (design, model) = match board::load(dir) {
        Ok((_, d, m)) => (d, m),
        Err(e) => return err(board::reasons(&e)),
    };
    let plot = plot_params_of(&design);
    let mut layers: Vec<String> = req.get("layers").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if layers.is_empty() {
        layers = board_plot_layers(&plot, &model.board.layers);
    }
    // The dialog's "Use drill/place file origin" starts as the board's `useauxorigin`; a request that says nothing plots as the board says.
    let mut req = req;
    if req.get("use_aux_origin").is_none() && plot.flag("useauxorigin").unwrap_or(false) {
        req["use_aux_origin"] = Value::Bool(true);
    }
    reply(kicad_engine::export(dir, "gerbers", &with_aux_origin("gerbers", gerber_args(&layers, &plot), &req)))
}

/// The layers the board's plot settings select, of those the board has (its copper layers, and every non-copper layer, which a
/// `.kicad_pcb` always defines): the Plot dialog's checklist as `DIALOG_PLOT::init_Dialog` fills it from `GetLayerSelection()`.
pub(crate) fn board_plot_layers(plot: &eda_model::plot_params::PlotParams, copper: &[String]) -> Vec<String> {
    plot.board_layers(copper)
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

/// `POST /api/fab/drill`: `{"merge_pth_npth": bool, "alt_drill_mode": bool, "use_aux_origin": bool}`, the Generate Drill Files dialog's
/// "PTH and NPTH in single file", "Use alternate drill mode" and Origin (each unchecked / Absolute by default, as in KiCad's dialog).
pub fn drill(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut v = reply(kicad_engine::export(dir, "drill", &with_aux_origin("drill", drill_args(flag("merge_pth_npth"), flag("alt_drill_mode"), false), &req)));
    if v["ok"].as_bool().unwrap_or(false) {
        let files: Vec<String> = v["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        v["message"] = Value::String(drill_report(&files));
    }
    v
}

/// `EXCELLON_WRITER::CreateDrillandMapFilesSet`'s report, word for word: "Created file '%s'" for each drill file (no full stop), "Created
/// file '%s'." for each map file (`GENDRILL_WRITER_BASE::CreateMapFilesSet`), then `ReportTail( _( "Done." ) )`.
/// The files come sorted by name; the writer writes the plated layer pairs first and the NPTH file last (`CreateDrillandMapFilesSet`'s
/// `hole_sets`, `doing_npth` last), so that is the order here.
pub(crate) fn drill_report(files: &[String]) -> String {
    let mut files: Vec<&String> = files.iter().collect();
    files.sort_by_key(|f| f.contains("-NPTH"));
    let mut lines: Vec<String> = files.iter().map(|f| if f.ends_with(".drl") { format!("Created file '{f}'") } else { format!("Created file '{f}'.") }).collect();
    lines.push("Done.".into());
    lines.join("\n")
}

/// `POST /api/fab/pos`: `{"format": "csv"|"ascii", "side": "front"|"back"|"both",
/// "units_mm": bool, "smd_only": bool, "exclude_fp_th": bool, "exclude_dnp": bool, "single_file": bool}`. A footprint marked "exclude from position files" is never
/// in the file; the other three options leave out SMD-less, through-hole and Do-Not-Populate footprints. `single_file` is the dialog's
/// "Generate single file with both front and back positions" (checked by default): off with both sides, the top and the bottom are two
/// files, named as KiCad names them (`<name>-all-pos.csv`, `<name>-top-pos.csv`, `<name>-bottom-pos.csv`).
pub fn pos(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let format = req.get("format").and_then(Value::as_str).unwrap_or("csv");
    let gerber = format == "gerber";
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut args = pos_args(format, req.get("side").and_then(Value::as_str).unwrap_or("both"), req.get("units_mm").and_then(Value::as_bool).unwrap_or(true), flag("smd_only"), flag("exclude_fp_th"));
    // `PLACE_FILE_EXPORTER`'s `m_excludeDNP`: footprints marked Do Not Populate stay out of the file.
    if flag("exclude_dnp") {
        args.push("--exclude-dnp".into());
    }
    // "Use negative X coordinates for footprints on bottom layer" (`m_negateXcb`, ASCII and CSV) and "Include board edge layer" (`m_cbIncludeBoardEdge`, Gerber X3).
    if flag("negate_x") && !gerber {
        args.push("--bottom-negate-x".into());
    }
    if flag("board_edge") && gerber {
        args.push("--gerber-board-edge".into());
    }
    let single_file = req.get("single_file").and_then(Value::as_bool).unwrap_or(true);
    let args = if gerber { args } else { with_aux_origin("pos", args, &req) };
    let mut v = match kicad_engine::export_pos(dir, &args, single_file) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let files: Vec<String> = v["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    if gerber {
        // `CreateGerberFiles` has no candidate test. Its counts are what `CreatePlaceFile` returns, one per footprint plotted, and each
        // footprint's position flash carries exactly one `%TO.CRot,..*%` (`GBR_CMP_PNP_METADATA::FormatCmpPnPMetadata`).
        let counted: Vec<(String, usize)> = files.iter().map(|f| (f.clone(), std::fs::read_to_string(dir.join(f)).map(|t| gerber_place_count(&t)).unwrap_or(0))).collect();
        v["message"] = Value::String(gerber_place_report(&counted));
        return v;
    }
    let counted: Vec<(String, usize)> = files.iter().map(|f| (f.clone(), std::fs::read_to_string(dir.join(f)).map(|t| place_file_rows(&t, format == "csv")).unwrap_or(0))).collect();
    // `CreateAsciiFiles`: "Test for any footprint candidate in list" first, and write nothing when there is none.
    if counted.iter().all(|(_, n)| *n == 0) {
        for f in &files {
            let _ = std::fs::remove_file(dir.join(f));
        }
        return err("No footprint for automated placement.");
    }
    v["message"] = Value::String(place_report(&counted));
    v
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
        // `PCB_PLOT_PARAMS`' default selection, of the layers a 4-layer board has, in `PCB_LAYER_ID` order.
        let plot = eda_model::plot_params::PlotParams::kicad_default();
        assert_eq!(board_plot_layers(&plot, &["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()]).join(","), "F.Cu,F.Mask,B.Cu,B.Mask,In1.Cu,F.SilkS,In2.Cu,B.SilkS,F.Paste,B.Paste,Edge.Cuts");
    }

    /// X3 (r2-editors-exports): the plot uses the board's plot settings, as KiCad's Plot dialog does.
    #[test]
    fn the_plot_uses_the_boards_plot_settings() {
        let l: Vec<String> = vec!["F.Cu".into(), "B.Cu".into()];
        let plot = eda_model::plot_params::PlotParams::kicad_default();
        assert_eq!(
            gerber_args(&l, &plot),
            vec!["--layers", "F.Cu,B.Cu", "--no-protel-ext", "--precision", "6", "--sketch-DNP-footprints-on-fab-layers", "--crossout-DNP-footprints-on-fab-layers"],
            "KiCad's defaults: .gbr names, X2 with netlist attributes, DNP footprints sketched and crossed out on the fab layers"
        );
        let mut own = plot.clone();
        for (k, v) in [("usegerberextensions", "yes"), ("subtractmaskfromsilk", "yes"), ("gerberprecision", "5"), ("usegerberattributes", "no"), ("sketchdnponfab", "no"), ("crossoutdnponfab", "no")] {
            own.set_item(k, v);
        }
        assert_eq!(gerber_args(&l, &own), vec!["--layers", "F.Cu,B.Cu", "--no-x2", "--subtract-soldermask", "--precision", "5"]);
    }

    /// X1, X2 (r2-editors-exports): the Generate Drill Files dialog's defaults are KiCad's, not kicad-cli's.
    #[test]
    fn legacy_plot_tokens_and_the_dnp_flags_are_read_as_kicad_reads_them() {
        let l = vec!["F.Cu".to_string()];
        // X3-2: `(excludeedgelayer no)` is the outline on every layer.
        let mut p = eda_model::plot_params::PlotParams { items: vec!["(excludeedgelayer no)".into()], ..Default::default() };
        p = p.normalized();
        let a = gerber_args(&l, &p);
        assert_eq!(a[2..4], ["--common-layers", "Edge.Cuts"]);
        // X3-5: 4 is clamped to 5.
        let p = eda_model::plot_params::PlotParams { items: vec!["(gerberprecision 4)".into()], ..Default::default() };
        assert!(gerber_args(&l, &p).windows(2).any(|w| w == ["--precision", "5"]));
        // X3-6: hide + sketch + crossout is hide only; sketch alone is sketch and cross-out.
        let dnp = |h: &str, s: &str, c: &str| {
            let p = eda_model::plot_params::PlotParams { items: vec![format!("(hidednponfab {h})"), format!("(sketchdnponfab {s})"), format!("(crossoutdnponfab {c})")], ..Default::default() };
            gerber_args(&l, &p).into_iter().filter(|f| f.contains("DNP")).collect::<Vec<_>>()
        };
        assert_eq!(dnp("yes", "yes", "yes"), vec!["--hide-DNP-footprints-on-fab-layers"]);
        assert_eq!(dnp("no", "yes", "no"), vec!["--sketch-DNP-footprints-on-fab-layers", "--crossout-DNP-footprints-on-fab-layers"]);
        assert!(dnp("no", "no", "no").is_empty());
    }

    #[test]
    fn the_gerber_placement_report_counts_the_components() {
        let top = "%TF.FileFunction,Component,L1,Top*%\n%TO.C,R1*%\n%TO.CRot,0*%\nD03*\n%TO.C,R2*%\n%TO.CRot,90*%\nD03*\n";
        assert_eq!(gerber_place_count(top), 2);
        let report = gerber_place_report(&[("o/b-pnp_top.gbr".into(), 2), ("o/b-pnp_bottom.gbr".into(), 1)]);
        assert_eq!(
            report,
            "Front (top side) placement file: 'o/b-pnp_top.gbr'.\nComponent count: 2.\nBack (bottom side) placement file: 'o/b-pnp_bottom.gbr'.\nComponent count: 1.\nFull component count: 3.\nDone."
        );
    }

    #[test]
    fn the_drill_report_is_the_writers() {
        assert_eq!(drill_report(&["o/b-NPTH.drl".into(), "o/b-PTH.drl".into()]), "Created file 'o/b-PTH.drl'\nCreated file 'o/b-NPTH.drl'\nDone.");
    }

    #[test]
    fn drill_defaults_are_the_dialogs() {
        assert_eq!(drill_args(false, false, false), vec!["--format", "excellon", "--excellon-oval-format", "route", "--excellon-separate-th"], "slots routed, PTH and NPTH in two files");
        assert_eq!(drill_args(true, true, true), vec!["--format", "excellon", "--excellon-oval-format", "alternate", "--generate-report"]);
    }

    #[test]
    fn the_drill_place_file_origin_is_a_different_flag_for_each_output() {
        let on = serde_json::json!({ "use_aux_origin": true });
        let off = serde_json::json!({});
        assert_eq!(with_aux_origin("gerbers", vec!["--layers".into(), "F.Cu".into()], &on), vec!["--layers", "F.Cu", "--use-drill-file-origin"]);
        assert_eq!(with_aux_origin("pos", vec![], &on), vec!["--use-drill-file-origin"]);
        assert_eq!(with_aux_origin("drill", vec![], &on), vec!["--drill-origin", "plot"]);
        assert!(with_aux_origin("drill", vec![], &off).is_empty(), "unset: the absolute origin, kicad-cli's default");
    }

    /// X8 (r2-editors-exports): `CreateAsciiFiles`' candidate count and report.
    #[test]
    fn placement_rows_and_the_report_are_kicads() {
        assert_eq!(place_file_rows("### Footprint positions\n## Unit = mm\n# Ref Val\n## End\n", false), 0, "a header-only ASCII file has no footprint");
        assert_eq!(place_file_rows("# Ref Val\nR1 1k R_0603 1 2 0 top\n## End\n", false), 1);
        assert_eq!(place_file_rows("Ref,Val,Package,PosX,PosY,Rot,Side\n", true), 0);
        assert_eq!(place_file_rows("Ref,Val,Package,PosX,PosY,Rot,Side\n\"R1\",\"1k\",x,1,2,0,top\n", true), 1);
        assert_eq!(place_report(&[("a-all-pos.csv".into(), 3)]), "Placement file: 'a-all-pos.csv'.\nComponent count: 3.\nDone.");
        assert_eq!(
            place_report(&[("a-top.pos".into(), 2), ("a-bottom.pos".into(), 1)]),
            "Front (top side) placement file: 'a-top.pos'.\nComponent count: 2.\nBack (bottom side) placement file: 'a-bottom.pos'.\nComponent count: 1.\nFull component count: 3.\nDone."
        );
        assert_eq!(pos_args("gerber", "front", true, true, true), vec!["--format", "gerber", "--side", "front"], "units and filters are ASCII/CSV only");
    }

    #[test]
    fn position_options_become_flags() {
        assert_eq!(pos_args("ascii", "front", false, true, true), vec!["--format", "ascii", "--side", "front", "--units", "in", "--smd-only", "--exclude-fp-th"]);
        assert_eq!(pos_args("whatever", "sideways", true, false, false), vec!["--format", "csv", "--side", "both", "--units", "mm"]);
    }
}
