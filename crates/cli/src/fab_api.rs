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

/// The Plot dialog's formats, in its choice's order (`DIALOG_PLOT::getPlotFormat`: Gerber, Postscript, SVG, DXF, PDF, PNG). HPGL, which
/// `PLOT_FORMAT` still numbers, is no longer offered by the dialog ("no longer supported") nor plotted by kicad-cli (`pcb export hpgl`:
/// "No longer supported as of KiCad 10.0.").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlotFormat {
    Gerber,
    PostScript,
    Svg,
    Dxf,
    Pdf,
    Png,
}

impl PlotFormat {
    /// The name the studio's request and `eda fab plot --format` use.
    pub(crate) fn from_name(name: &str) -> Option<PlotFormat> {
        Some(match name {
            "gerber" | "gerbers" => PlotFormat::Gerber,
            "ps" | "postscript" => PlotFormat::PostScript,
            "svg" => PlotFormat::Svg,
            "dxf" => PlotFormat::Dxf,
            "pdf" => PlotFormat::Pdf,
            "png" => PlotFormat::Png,
            _ => return None,
        })
    }

    /// `(outputformat n)`, `PLOT_FORMAT` (HPGL 0, Gerber 1, POST 2, DXF 3, PDF 4, SVG 5, PNG 6), as `DIALOG_PLOT::TransferDataToWindow` selects
    /// it: anything else (HPGL, "no longer supported") leaves the choice at its first item, Gerber.
    pub(crate) fn from_outputformat(n: i64) -> PlotFormat {
        match n {
            2 => PlotFormat::PostScript,
            3 => PlotFormat::Dxf,
            4 => PlotFormat::Pdf,
            5 => PlotFormat::Svg,
            6 => PlotFormat::Png,
            _ => PlotFormat::Gerber,
        }
    }

    /// kicad-cli's `pcb export` command for it.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            PlotFormat::Gerber => "gerbers",
            PlotFormat::PostScript => "ps",
            PlotFormat::Svg => "svg",
            PlotFormat::Dxf => "dxf",
            PlotFormat::Pdf => "pdf",
            PlotFormat::Png => "png",
        }
    }
}

/// What the Plot dialog plots with that `PCB_PLOT_PARAMS::Format` does not write into the board, so KiCad keeps it only in the session's
/// settings (`m_svgFitPagetoBoard`, `m_PDFBackgroundColor`, `m_DXFExportAsMultiLayeredFile`) or in the dialog's controls (the PostScript fine
/// scale adjusts and width correction, `m_XScaleAdjust`, `m_YScaleAdjust`, `m_PSWidthAdjust`): the studio's dialog keeps them and sends them
/// with each plot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlotSession {
    /// SVG Options > "Fit page to board".
    pub svg_fit_page_to_board: bool,
    /// PDF Options > "Background Color", a CSS colour (`COLOR4D::ToCSSString`); `None` is `COLOR4D::UNSPECIFIED`.
    pub pdf_background_color: Option<String>,
    /// DXF Options > "Single document".
    pub dxf_single_document: bool,
    /// Postscript Options > "X scale factor:", "Y scale factor:" and "Track width correction:" (mm), as `applyPlotSettings` constrained them.
    pub ps_x_scale: f64,
    pub ps_y_scale: f64,
    pub ps_width_adjust_mm: f64,
}

impl Default for PlotSession {
    /// `PCB_PLOT_PARAMS`' defaults: no fit, no background, one file per layer, scales 1 and no width correction.
    fn default() -> Self {
        PlotSession { svg_fit_page_to_board: false, pdf_background_color: None, dxf_single_document: false, ps_x_scale: 1.0, ps_y_scale: 1.0, ps_width_adjust_mm: 0.0 }
    }
}

impl PlotSession {
    /// From a `POST /api/fab/plot` body; a colour that is no CSS colour kicad-cli reads (`#rrggbb`, `#rrggbbaa`, `rgb(..)`, `rgba(..)`) is none.
    pub(crate) fn of_request(req: &Value) -> PlotSession {
        let d = PlotSession::default();
        let num = |k: &str, dflt: f64| req.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(dflt);
        let color = req.get("pdf_background_color").and_then(Value::as_str).map(str::trim).filter(|c| is_css_color(c)).map(str::to_string);
        PlotSession {
            svg_fit_page_to_board: req.get("svg_fit_page_to_board").and_then(Value::as_bool).unwrap_or(d.svg_fit_page_to_board),
            pdf_background_color: color,
            dxf_single_document: req.get("dxf_single_document").and_then(Value::as_bool).unwrap_or(d.dxf_single_document),
            ps_x_scale: num("ps_x_scale", d.ps_x_scale),
            ps_y_scale: num("ps_y_scale", d.ps_y_scale),
            ps_width_adjust_mm: num("ps_width_adjust_mm", d.ps_width_adjust_mm),
        }
    }
}

/// `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)` or `rgba(r,g,b,a)`: what `kicad-cli pcb export pdf --bg-color` reads.
fn is_css_color(c: &str) -> bool {
    if let Some(hex) = c.strip_prefix('#') {
        return (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|h| h.is_ascii_hexdigit());
    }
    let inner = c.strip_prefix("rgba(").or_else(|| c.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')'));
    inner.is_some_and(|i| {
        let parts: Vec<&str> = i.split(',').map(str::trim).collect();
        (parts.len() == 3 || parts.len() == 4) && parts.iter().all(|p| p.parse::<f64>().is_ok())
    })
}

/// `selectionToScale` (`dialog_plot.cpp`): the Scaling choice's Auto, 1:1, 3:2, 2:1, 3:1 -- kicad-cli's `--scale`, 0 for auto.
pub(crate) fn selection_to_scale(selection: i64) -> &'static str {
    match selection {
        0 => "0",
        2 => "1.5",
        3 => "2",
        4 => "3",
        _ => "1",
    }
}

/// `LSET::UIOrder`: the copper stack from the front, then `TechAndUserUIOrder` (the adhesives, pastes, silkscreens, masks, the user drawing
/// layers, Edge.Cuts, Margin, the courtyards, the fab layers, then `User.1` ..): the order the dialog's checklist lists the layers in and plots
/// them in (`GetLayerSelection().UIOrder()`, the pages of a one-document PDF).
pub(crate) fn ui_order(layers: &[String]) -> Vec<String> {
    const TECH: [&str; 18] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];
    let rank = |l: &str| -> u32 {
        if l == "F.Cu" {
            0
        } else if l == "B.Cu" {
            31
        } else if let Some(n) = l.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<u32>().ok()) {
            n
        } else if let Some(i) = TECH.iter().position(|t| *t == l) {
            100 + i as u32
        } else if let Some(n) = l.strip_prefix("User.").and_then(|n| n.parse::<u32>().ok()) {
            200 + n
        } else {
            u32::MAX
        }
    };
    let mut v: Vec<String> = Vec::new();
    for l in layers {
        if !v.contains(l) {
            v.push(l.clone());
        }
    }
    v.sort_by_key(|l| rank(l));
    v
}

/// The three DNP flags and the pad sketches, the same on every format (`DIALOG_PLOT::init_Dialog` / `applyPlotSettings`, see [`gerber_args`]).
fn fab_layer_args(plot: &eda_model::plot_params::PlotParams, a: &mut Vec<String>) {
    let on = |key: &str, default: bool| plot.flag(key).unwrap_or(default);
    if on("sketchpadsonfab", false) {
        a.push("--sketch-pads-on-fab-layers".into());
        if on("plotpadnumbers", false) {
            a.push("--sketch-pad-numbers".into());
        }
    }
    let (hide, sketch, crossout) = (on("hidednponfab", false), on("sketchdnponfab", true), on("crossoutdnponfab", true));
    if hide {
        a.push("--hide-DNP-footprints-on-fab-layers".into());
    } else if sketch || crossout {
        a.push("--sketch-DNP-footprints-on-fab-layers".into());
        a.push("--crossout-DNP-footprints-on-fab-layers".into());
    }
}

/// `kicad-cli pcb export <format>` arguments for the Plot dialog's plot of `layers` with the board's plot settings (`DIALOG_PLOT::Plot` plots
/// with `m_plotOpts`, which `applyPlotSettings` has just stored) and the session's ([`PlotSession`]); kicad-cli's job for each command sets
/// the same `PCB_PLOT_PARAMS` (`PCB_PLOTTER::PlotJobToPlotOpts`) as the dialog's controls do:
/// - every format: "Plot on All Layers" (`--common-layers`, in the list's order), "Plot drawing sheet" (`--include-border-title`; SVG's
///   command plots it unless `--exclude-drawing-sheet`), "Subtract soldermask from silkscreen", the pad sketches and the DNP options;
/// - Gerber: [`gerber_args`] and "Use drill/place file origin" (`--use-drill-file-origin`);
/// - the others: "Drill marks:" (`--drill-shape-opt`) and "Scaling:" (`--scale`, [`selection_to_scale`]); "Mirrored plot" and "Negative
///   plot" (PostScript, SVG, PDF, PNG); black and white as `applyPlotSettings` sets it (SVG and PDF as their Output mode says, PNG never,
///   PostScript always);
/// - PostScript: one file per layer (`--mode-multi`, `transferPlotParamsToJob`'s `GEN_MODE::MULTI`), the fine scale adjusts, the width
///   correction (mm) and "Force A4 output";
/// - SVG: one file per layer (`--mode-multi`) and "Fit page to board";
/// - DXF: "Single document" (`--mode-single`, else `--mode-multi`), "Plot graphic items using their contours" (`--use-contours`), "Export
///   units:" (`--output-units`) and "Use drill/place file origin" (`--use-drill-origin`);
/// - PDF: "Single document" (`--mode-multipage`: one page per layer in one file, else `--mode-separate`), the property popups
///   (`--no-property-popups`, front and back together) and "Background Color" (`--bg-color`, colour mode only);
/// - PNG: "DPI:" (`--dpi`) and "Anti-alias" (`--no-antialias`).
///
/// Differs (no kicad-cli flag; the dialog shows each disabled with the reason): "Generate Gerber job file" (kicad-cli always writes it),
/// "Use drill/place file origin" in PostScript, SVG, PDF and PNG (none of those commands has it; KiCad's PostScript plot turns it off too),
/// SVG's "Precision:" (kicad-cli plots with `SVG_PRECISION_DEFAULT`, 4), PDF's "Generate metadata from AUTHOR && SUBJECT variables" (always on,
/// `JOB_EXPORT_PCB_PDF::m_pdfMetadata`), the two property popup boxes apart (one flag clears both), DXF's "Use KiCad font to plot text"
/// (always the KiCad font, `JOB_EXPORT_PCB_DXF::m_useKiCadFont`) and DXF's black and white (KiCad's dialog plots DXF in black, kicad-cli in the
/// layers' colours). `--use-contours` also sets the graphic items' sketch mode (`m_plotGraphicItemsUsingContours`), which the dialog leaves
/// filled. The colour theme is kicad-cli's PCB Editor default (the dialog plots with the PCB editor's theme, `cfg->m_ColorTheme`).
pub(crate) fn plot_args(format: PlotFormat, layers: &[String], plot: &eda_model::plot_params::PlotParams, session: &PlotSession) -> Vec<String> {
    let on = |key: &str, default: bool| plot.flag(key).unwrap_or(default);
    let layers = ui_order(layers);
    if format == PlotFormat::Gerber {
        let mut a = gerber_args(&layers, plot);
        if on("useauxorigin", false) {
            a.push("--use-drill-file-origin".into());
        }
        return a;
    }
    let mut a: Vec<String> = vec!["--layers".into(), layers.join(",")];
    if !plot.plot_on_all_layers.is_empty() {
        a.extend(["--common-layers".into(), plot.plot_on_all_layers.join(",")]);
    }
    match format {
        PlotFormat::Svg => {
            if !on("plotframeref", false) {
                a.push("--exclude-drawing-sheet".into());
            }
        }
        _ => {
            if on("plotframeref", false) {
                a.push("--include-border-title".into());
            }
        }
    }
    if on("subtractmaskfromsilk", false) {
        a.push("--subtract-soldermask".into());
    }
    fab_layer_args(plot, &mut a);
    a.extend(["--drill-shape-opt".into(), plot.int("drillshape").unwrap_or(1).to_string()]);
    a.extend(["--scale".into(), selection_to_scale(plot.int("scaleselection").unwrap_or(1)).into()]);
    let mirror_negative = |a: &mut Vec<String>| {
        if on("mirror", false) {
            a.push("--mirror".into());
        }
        if on("psnegative", false) {
            a.push("--negative".into());
        }
    };
    let black_and_white = on("plot_black_and_white", true);
    match format {
        PlotFormat::PostScript => {
            a.push("--mode-multi".into());
            a.push("--black-and-white".into());
            mirror_negative(&mut a);
            a.extend(["--track-width-correction".into(), format!("{}", session.ps_width_adjust_mm)]);
            a.extend(["--x-scale-factor".into(), format!("{}", session.ps_x_scale)]);
            a.extend(["--y-scale-factor".into(), format!("{}", session.ps_y_scale)]);
            if on("psa4output", false) {
                a.push("--force-a4".into());
            }
        }
        PlotFormat::Svg => {
            a.push("--mode-multi".into());
            if black_and_white {
                a.push("--black-and-white".into());
            }
            mirror_negative(&mut a);
            if session.svg_fit_page_to_board {
                a.push("--fit-page-to-board".into());
            }
        }
        PlotFormat::Dxf => {
            a.push(if session.dxf_single_document { "--mode-single" } else { "--mode-multi" }.into());
            if on("dxfpolygonmode", true) {
                a.push("--use-contours".into());
            }
            a.extend(["--output-units".into(), if on("dxfimperialunits", true) { "in" } else { "mm" }.into()]);
            if on("useauxorigin", false) {
                a.push("--use-drill-origin".into());
            }
        }
        PlotFormat::Pdf => {
            a.push(if on("pdf_single_document", false) { "--mode-multipage" } else { "--mode-separate" }.into());
            if black_and_white {
                a.push("--black-and-white".into());
            }
            mirror_negative(&mut a);
            if !on("pdf_front_fp_property_popups", true) && !on("pdf_back_fp_property_popups", true) {
                a.push("--no-property-popups".into());
            }
            if let (false, Some(c)) = (black_and_white, session.pdf_background_color.as_ref()) {
                a.extend(["--bg-color".into(), c.clone()]);
            }
        }
        PlotFormat::Png => {
            mirror_negative(&mut a);
            a.extend(["--dpi".into(), plot.int("pngdpi").unwrap_or(300).to_string()]);
            if !on("pngantialias", true) {
                a.push("--no-antialias".into());
            }
        }
        PlotFormat::Gerber => unreachable!(),
    }
    a
}

/// The dialogs' "Output directory:" as the plot writes into it (`DIALOG_PLOT::Plot`, `DIALOG_GENDRILL::genDrillAndMapFiles`: `ExpandTextVars`
/// with the board's resolver, then `EnsureFileDirectoryExists( &outputDir, boardFilename )`, which makes a relative folder the board file's and
/// creates it): `typed` with its `${..}` expanded by `resolve` (`BOARD::ResolveTextVar`), relative to the board's folder `dir`. Differs: an
/// empty folder is the studio's `export/kicad/<kind>/` (KiCad's is the board file's own folder, which here holds `design.json` and its
/// history), and a folder outside the board's (an absolute path elsewhere, a `..` that climbs out) is refused, since the studio is served
/// to a browser and writes only into the board it serves: "Could not write plot files to folder '%s'.", KiCad's words for a folder it cannot
/// make, with the reason. `ExpandEnvVarSubstitutions` is not run.
pub(crate) fn output_folder(dir: &Path, typed: &str, default_kind: &str, resolve: &eda_model::text_vars::Resolver) -> Result<std::path::PathBuf, String> {
    let expanded = eda_model::text_vars::expand_text_vars(typed, Some(resolve), 0, 0).replace('\\', "/");
    let expanded = expanded.trim();
    if expanded.is_empty() {
        return Ok(dir.join("export").join("kicad").join(default_kind));
    }
    let refuse = || format!("Could not write plot files to folder '{expanded}'.\nThe output directory must be inside the board's folder.");
    let rel = match Path::new(expanded).strip_prefix(dir) {
        Ok(inside) => inside.to_path_buf(),
        Err(_) if Path::new(expanded).is_absolute() => return Err(refuse()),
        Err(_) => Path::new(expanded).to_path_buf(),
    };
    let mut clean = std::path::PathBuf::new();
    for part in rel.components() {
        match part {
            std::path::Component::Normal(p) => clean.push(p),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !clean.pop() {
                    return Err(refuse());
                }
            }
            _ => return Err(refuse()),
        }
    }
    Ok(dir.join(clean))
}

/// The board's own text variables (`BOARD::ResolveTextVar`: the title block, the project's variables, `${PROJECTNAME}`, ..), for a folder name.
fn board_resolver(dir: &Path) -> Box<dyn Fn(&str) -> Option<String>> {
    let Ok((meta, design, model)) = board::load(dir) else { return Box::new(|_| None) };
    let project = crate::text_vars_json::project(dir, &meta.intent, eda_model::text_vars::Clock::utc_now());
    let shown = crate::text_vars_json::PcbShown::new(dir, &design, &model, project);
    let board = shown.board().clone();
    Box::new(move |token| board.resolve(token, 0))
}

/// A dialog's own "Output directory:" (the placement and Plot Schematic dialogs keep theirs in the dialog, `DIALOG_SHIM`'s saved control
/// state), as [`output_folder`] resolves it.
pub(crate) fn request_output_folder(dir: &Path, typed: &str, kind: &str) -> Result<std::path::PathBuf, String> {
    if typed.trim().is_empty() {
        return Ok(dir.join("export").join("kicad").join(kind));
    }
    output_folder(dir, typed, kind, &*board_resolver(dir))
}

/// The board's output folder for `kind` ([`output_folder`] of the plot settings' `outputdirectory`, which the Plot and Generate Drill Files
/// dialogs share).
pub(crate) fn board_output_folder(dir: &Path, plot: &eda_model::plot_params::PlotParams, kind: &str) -> Result<std::path::PathBuf, String> {
    output_folder(dir, &plot.output_directory(), kind, &*board_resolver(dir))
}

/// The plot's report as `PCB_PLOTTER::Plot` writes it, word for word: "Plotted to '%s'." for each file in the order the layers are plotted
/// (`UIOrder`; a one-document plot is one file), the Gerber job file's "Created Gerber job file '%s'." (`GERBER_JOBFILE_WRITER::CreateJobFile`),
/// then `ReportTail( _( "Done." ) )`. `files` are the files the run wrote (sorted by name); `layers` the plotted ones, by the names the files
/// carry (`BuildPlotFileName`: the layer's name with its dots made underscores).
pub(crate) fn plot_report(files: &[String], layer_suffixes: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut left: Vec<&String> = files.iter().filter(|f| !f.ends_with(".gbrjob")).collect();
    for suffix in layer_suffixes {
        let tail = format!("-{suffix}.");
        if let Some(i) = left.iter().position(|f| Path::new(f.as_str()).file_name().is_some_and(|n| n.to_string_lossy().contains(&tail))) {
            lines.push(format!("Plotted to '{}'.", left.remove(i)));
        }
    }
    lines.extend(left.iter().map(|f| format!("Plotted to '{f}'.")));
    lines.extend(files.iter().filter(|f| f.ends_with(".gbrjob")).map(|f| format!("Created Gerber job file '{f}'.")));
    lines.push("Done.".into());
    lines.join("\n")
}

/// `BOARD::GetLayerName` as `BuildPlotFileName` puts it in a file name: the name the board gives the layer, else KiCad's standard one
/// (`F.Silkscreen`), its dots made underscores.
fn layer_file_suffix(model: &eda_model::ConstraintModel, layer: &str) -> String {
    let name = model.board.layer_setup.names.get(layer).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| eda_model::text_vars::standard_layer_name(layer));
    name.replace('.', "_")
}

/// `POST /api/fab/plot`: the Plot dialog's Plot (`DIALOG_PLOT::Plot`), any format, after the dialog has stored its controls in the board's
/// plot settings (`applyPlotSettings`, the `set_plot_settings` verb): `{"format": "gerber"|"ps"|"svg"|"dxf"|"pdf"|"png", "layers": [..],
/// "check_zones": bool, ..}` plus the [`PlotSession`] values. An omitted format is the board's `outputformat`, omitted layers its selection.
/// "Check zone fills before plotting" runs KiCad's filler first where a zone has no current fill (`ZONE_FILLER_TOOL::CheckAllZones`,
/// `crate::fill_api::ensure_filled`). Writes into the board's output folder ([`board_output_folder`]). "No layer selected, Nothing to plot"
/// when no layer is checked (KiCad's test is on the stored selection, which keeps the copper layers the board lacks, so on most boards it
/// never fires and the plot writes nothing; here the checked layers are tested).
pub fn plot(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (design, model) = match board::load(dir) {
        Ok((_, d, m)) => (d, m),
        Err(e) => return err(board::reasons(&e)),
    };
    let plot = plot_params_of(&design);
    let format = match req.get("format").and_then(Value::as_str) {
        Some(name) => match PlotFormat::from_name(name) {
            Some(f) => f,
            None => return err(format!("unknown plot format {name:?} (expected gerber, ps, svg, dxf, pdf or png)")),
        },
        None => PlotFormat::from_outputformat(plot.int("outputformat").unwrap_or(1)),
    };
    let layers: Vec<String> = match req.get("layers").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => board_plot_layers(&plot, &model.board.layers),
    };
    if layers.is_empty() {
        return err("No layer selected, Nothing to plot");
    }
    let out_dir = match board_output_folder(dir, &plot, format.kind()) {
        Ok(d) => d,
        Err(why) => return err(why),
    };
    if req.get("check_zones").and_then(Value::as_bool).unwrap_or(false) {
        if let Err(why) = crate::fill_api::ensure_filled(dir) {
            return err(format!("Zones not filled before plotting: {why}"));
        }
    }
    let args = plot_args(format, &layers, &plot, &PlotSession::of_request(&req));
    let mut v = reply(kicad_engine::export_into(dir, format.kind(), &args, &out_dir));
    if v["ok"].as_bool().unwrap_or(false) {
        let files: Vec<String> = v["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        let suffixes: Vec<String> = ui_order(&layers).iter().map(|l| layer_file_suffix(&model, l)).collect();
        v["message"] = Value::String(plot_report(&files, &suffixes));
        v["format"] = Value::String(format.kind().into());
    }
    v
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

/// The Generate Drill Files dialog's controls (`dialog_gendrill_base.cpp`, with its defaults): Format Excellon or Gerber X2; for Excellon
/// "Mirror Y axis", "Minimal header", "PTH and NPTH in single file", "Use alternate drill mode for oval holes", Units (Millimeters) and
/// Zeros ("Decimal format (recommended)"); for Gerber X2 "Generate tenting layers"; "Generate map:" (unchecked) and its format (Gerber X2,
/// `m_choiceDrillMap->SetSelection( 1 )`). The Origin is the board's (`with_aux_origin`), the Gerber precision the board's plot settings'
/// (`gerberWriter.SetFormat( m_plotOpts.GetGerberPrecision() )`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrillOptions {
    pub gerber: bool,
    pub mirror_y: bool,
    pub minimal_header: bool,
    pub merge_pth_npth: bool,
    pub alt_drill_mode: bool,
    pub units_mm: bool,
    /// `decimal`, `suppressleading`, `suppresstrailing` or `keep` (`EXCELLON_WRITER::ZEROS_FMT`, the choice's order).
    pub zeros: String,
    pub generate_tenting: bool,
    pub generate_map: bool,
    /// `ps`, `gerberx2`, `dxf`, `svg` or `pdf` (`m_choiceDrillMap`'s Postscript, Gerber X2, DXF, SVG, PDF).
    pub map_format: String,
    pub gerber_precision: i64,
}

impl Default for DrillOptions {
    fn default() -> Self {
        DrillOptions {
            gerber: false,
            mirror_y: false,
            minimal_header: false,
            merge_pth_npth: false,
            alt_drill_mode: false,
            units_mm: true,
            zeros: "decimal".into(),
            generate_tenting: false,
            generate_map: false,
            map_format: "gerberx2".into(),
            gerber_precision: 6,
        }
    }
}

impl DrillOptions {
    /// From a `POST /api/fab/drill` body; an unknown zeros or map format is the default one.
    pub(crate) fn of_request(req: &Value, gerber_precision: i64) -> DrillOptions {
        let d = DrillOptions::default();
        let flag = |k: &str, dflt: bool| req.get(k).and_then(Value::as_bool).unwrap_or(dflt);
        let pick = |k: &str, allowed: &[&str], dflt: &str| req.get(k).and_then(Value::as_str).filter(|v| allowed.contains(v)).unwrap_or(dflt).to_string();
        DrillOptions {
            gerber: req.get("format").and_then(Value::as_str) == Some("gerber"),
            mirror_y: flag("mirror_y", d.mirror_y),
            minimal_header: flag("minimal_header", d.minimal_header),
            merge_pth_npth: flag("merge_pth_npth", d.merge_pth_npth),
            alt_drill_mode: flag("alt_drill_mode", d.alt_drill_mode),
            units_mm: flag("units_mm", d.units_mm),
            zeros: pick("zeros", &["decimal", "suppressleading", "suppresstrailing", "keep"], &d.zeros),
            generate_tenting: flag("generate_tenting", d.generate_tenting),
            generate_map: flag("generate_map", d.generate_map),
            map_format: pick("map_format", &["ps", "gerberx2", "dxf", "svg", "pdf"], &d.map_format),
            gerber_precision: if gerber_precision == 5 { 5 } else { 6 },
        }
    }
}

/// `kicad-cli pcb export drill` for the dialog's controls (`DIALOG_GENDRILL::genDrillAndMapFiles`; kicad-cli's job,
/// `PCBNEW_JOBS_HANDLER::JobExportDrill`, drives the same writers with the same setters): Excellon's `SetFormat( metric, zeros, .. )`,
/// `SetOptions( mirror, minimal header, offset, merge )` and `SetRouteModeForOvalHoles( !m_altDrillMode )` (the controls `onFileFormatSelection`
/// disables for Gerber X2 are not sent then), Gerber X2's precision and tenting layers, the map and its format. kicad-cli's own defaults
/// differ from the dialog's on two (`--excellon-oval-format alternate`, one file), so both are always given.
pub(crate) fn drill_dialog_args(o: &DrillOptions) -> Vec<String> {
    let mut a: Vec<String> = vec!["--format".into(), if o.gerber { "gerber" } else { "excellon" }.into()];
    if o.gerber {
        a.extend(["--gerber-precision".into(), o.gerber_precision.to_string()]);
        if o.generate_tenting {
            a.push("--generate-tenting".into());
        }
    } else {
        a.extend(["--excellon-units".into(), if o.units_mm { "mm" } else { "in" }.into()]);
        a.extend(["--excellon-zeros-format".into(), o.zeros.clone()]);
        a.extend(["--excellon-oval-format".into(), if o.alt_drill_mode { "alternate" } else { "route" }.into()]);
        if o.mirror_y {
            a.push("--excellon-mirror-y".into());
        }
        if o.minimal_header {
            a.push("--excellon-min-header".into());
        }
        if !o.merge_pth_npth {
            a.push("--excellon-separate-th".into());
        }
    }
    if o.generate_map {
        a.extend(["--generate-map".into(), "--map-format".into(), o.map_format.clone()]);
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

/// `POST /api/fab/drill`: the Generate Drill Files dialog's Generate (`DIALOG_GENDRILL::genDrillAndMapFiles`), with its controls
/// ([`DrillOptions`]: `"format": "excellon"|"gerber"`, `"mirror_y"`, `"minimal_header"`, `"merge_pth_npth"`, `"alt_drill_mode"`, `"units_mm"`,
/// `"zeros"`, `"generate_tenting"`, `"generate_map"`, `"map_format"`) and `"use_aux_origin"`, the Origin. Writes into the board's output
/// folder (the plot settings' `outputdirectory`, which the dialog stores first with its Origin, `updateConfig`).
pub fn drill(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let plot = match board::load(dir) {
        Ok((_, d, _)) => plot_params_of(&d),
        Err(e) => return err(board::reasons(&e)),
    };
    let out_dir = match board_output_folder(dir, &plot, "drill") {
        Ok(d) => d,
        Err(why) => return err(why.replace("plot files", "drill and/or map files")),
    };
    let opts = DrillOptions::of_request(&req, plot.int("gerberprecision").unwrap_or(6));
    let mut v = reply(kicad_engine::export_into(dir, "drill", &with_aux_origin("drill", drill_dialog_args(&opts), &req), &out_dir));
    if v["ok"].as_bool().unwrap_or(false) {
        let files: Vec<String> = v["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        v["message"] = Value::String(drill_report(&files));
    }
    v
}

/// `POST /api/fab/drill_report`: "Generate Report File..." (`DIALOG_GENDRILL::onGenReportFile`): `<board>-drl.rpt` (`fn.GetName() + "-drl"`,
/// `FILEEXT::ReportFileExtension`) in the output folder, written by kicad-cli ([`kicad_engine::export_drill_report`]) with the same body as
/// [`drill`]. KiCad's dialog asks for the file's name in a save dialog; here it is that default name. Its message is KiCad's: "Report file
/// '%s' created." Differs: kicad-cli writes the report with the selected format's writer; KiCad's dialog tests the format the wrong way round
/// (`if( m_rbExcellon->GetValue() == 0 )` builds the `EXCELLON_WRITER` for Gerber X2), so its report is the other writer's.
pub fn drill_report_file(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let plot = match board::load(dir) {
        Ok((_, d, _)) => plot_params_of(&d),
        Err(e) => return err(board::reasons(&e)),
    };
    let out_dir = match board_output_folder(dir, &plot, "drill") {
        Ok(d) => d,
        Err(why) => return err(why.replace("plot files", "drill and/or map files")),
    };
    let stem = eda_kicad_engine::file_stem(&kicad_engine::project_name(dir));
    let report = out_dir.join(format!("{stem}-drl.rpt"));
    let opts = DrillOptions::of_request(&req, plot.int("gerberprecision").unwrap_or(6));
    let args = with_aux_origin("drill", drill_dialog_args(&DrillOptions { generate_map: false, ..opts }), &req);
    match kicad_engine::export_drill_report(dir, &args, &report) {
        Ok(mut v) => {
            let file = v["files"][0].as_str().unwrap_or_default().to_string();
            v["message"] = Value::String(format!("Report file '{file}' created."));
            v
        }
        Err(e) => err(format!("Failed to create file '{}'.\n{}", report.strip_prefix(dir).unwrap_or(&report).display(), board::reasons(&e))),
    }
}

/// The drill writers' report, word for word: `EXCELLON_WRITER::CreateDrillandMapFilesSet`'s "Created file '%s'" for each Excellon drill file
/// (no full stop), "Created file '%s'." for each Gerber X2 drill or protection file (`GERBER_WRITER::CreateDrillandMapFilesSet`) and each map
/// file (`GENDRILL_WRITER_BASE::CreateMapFilesSet`), then `ReportTail( _( "Done." ) )`. The files come sorted by name; the writers write the
/// plated layer pairs first, the NPTH file last (`hole_sets`, `doing_npth` last) and the maps after every drill file, so that is the order here.
pub(crate) fn drill_report(files: &[String]) -> String {
    let mut files: Vec<&String> = files.iter().collect();
    files.sort_by_key(|f| (f.contains("-drl_map"), f.contains("-NPTH")));
    let mut lines: Vec<String> = files.iter().map(|f| if f.ends_with(".drl") { format!("Created file '{f}'") } else { format!("Created file '{f}'.") }).collect();
    lines.push("Done.".into());
    lines.join("\n")
}

/// `POST /api/fab/pos`: `{"format": "csv"|"ascii", "side": "front"|"back"|"both",
/// "units_mm": bool, "smd_only": bool, "exclude_fp_th": bool, "exclude_dnp": bool, "single_file": bool, "output_directory": ".."}`. A footprint marked "exclude from position files" is never
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
    // "Output directory:" (`m_outputDirectoryName`, the dialog's own).
    let out_dir = match request_output_folder(dir, req.get("output_directory").and_then(Value::as_str).unwrap_or(""), "pos") {
        Ok(d) => d,
        Err(why) => return err(why),
    };
    let mut v = match kicad_engine::export_pos_into(dir, &args, single_file, &out_dir) {
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

    /// X1 (UI-LEFT): each of the Plot dialog's formats with KiCad's defaults (`PCB_PLOT_PARAMS`), and each option on: the flag kicad-cli has
    /// for it, in the mode the dialog plots in (one file per layer; one document when "Single document" is checked).
    #[test]
    fn every_plot_format_maps_the_dialogs_options_onto_kicad_clis_flags() {
        use eda_model::plot_params::PlotParams;
        let l: Vec<String> = vec!["B.Cu".into(), "Edge.Cuts".into(), "F.Cu".into()];
        let plot = PlotParams::kicad_default();
        let s = PlotSession::default();
        let args = |f: PlotFormat, p: &PlotParams, s: &PlotSession| plot_args(f, &l, p, s).join(" ");
        let dnp = "--sketch-DNP-footprints-on-fab-layers --crossout-DNP-footprints-on-fab-layers";
        // The layers in `UIOrder`: the copper stack from the front, then the technical layers.
        assert_eq!(args(PlotFormat::Gerber, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts --no-protel-ext --precision 6 {dnp}"));
        assert_eq!(args(PlotFormat::Pdf, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts {dnp} --drill-shape-opt 1 --scale 1 --mode-separate --black-and-white"));
        assert_eq!(args(PlotFormat::Svg, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts --exclude-drawing-sheet {dnp} --drill-shape-opt 1 --scale 1 --mode-multi --black-and-white"), "SVG's command plots the sheet unless told not to");
        assert_eq!(args(PlotFormat::PostScript, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts {dnp} --drill-shape-opt 1 --scale 1 --mode-multi --black-and-white --track-width-correction 0 --x-scale-factor 1 --y-scale-factor 1"));
        assert_eq!(args(PlotFormat::Dxf, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts {dnp} --drill-shape-opt 1 --scale 1 --mode-multi --use-contours --output-units in"));
        assert_eq!(args(PlotFormat::Png, &plot, &s), format!("--layers F.Cu,B.Cu,Edge.Cuts {dnp} --drill-shape-opt 1 --scale 1 --dpi 300"), "PNG is never black and white (applyPlotSettings)");

        let mut own = plot.clone();
        own.plot_on_all_layers = vec!["F.Fab".into(), "Edge.Cuts".into()];
        for (k, v) in [
            ("plotframeref", "yes"),
            ("subtractmaskfromsilk", "yes"),
            ("sketchpadsonfab", "yes"),
            ("plotpadnumbers", "yes"),
            ("hidednponfab", "yes"),
            ("drillshape", "2"),
            ("scaleselection", "2"),
            ("mirror", "yes"),
            ("psnegative", "yes"),
            ("plot_black_and_white", "no"),
            ("psa4output", "yes"),
            ("pdf_single_document", "yes"),
            ("pdf_front_fp_property_popups", "no"),
            ("pdf_back_fp_property_popups", "no"),
            ("dxfpolygonmode", "no"),
            ("dxfimperialunits", "no"),
            ("useauxorigin", "yes"),
            ("pngdpi", "600"),
            ("pngantialias", "no"),
        ] {
            own.set_option(k, v).unwrap();
        }
        let session = PlotSession { svg_fit_page_to_board: true, pdf_background_color: Some("#ffffff".into()), dxf_single_document: true, ps_x_scale: 1.01, ps_y_scale: 0.99, ps_width_adjust_mm: -0.05 };
        let common = "--layers F.Cu,B.Cu,Edge.Cuts --common-layers F.Fab,Edge.Cuts";
        let fab = "--sketch-pads-on-fab-layers --sketch-pad-numbers --hide-DNP-footprints-on-fab-layers";
        assert_eq!(args(PlotFormat::Pdf, &own, &session), format!("{common} --include-border-title --subtract-soldermask {fab} --drill-shape-opt 2 --scale 1.5 --mode-multipage --mirror --negative --no-property-popups --bg-color #ffffff"));
        assert_eq!(args(PlotFormat::Svg, &own, &session), format!("{common} --subtract-soldermask {fab} --drill-shape-opt 2 --scale 1.5 --mode-multi --mirror --negative --fit-page-to-board"));
        assert_eq!(args(PlotFormat::PostScript, &own, &session), format!("{common} --include-border-title --subtract-soldermask {fab} --drill-shape-opt 2 --scale 1.5 --mode-multi --black-and-white --mirror --negative --track-width-correction -0.05 --x-scale-factor 1.01 --y-scale-factor 0.99 --force-a4"), "PostScript is always black and white");
        assert_eq!(args(PlotFormat::Dxf, &own, &session), format!("{common} --include-border-title --subtract-soldermask {fab} --drill-shape-opt 2 --scale 1.5 --mode-single --output-units mm --use-drill-origin"), "no mirror or negative in DXF");
        assert_eq!(args(PlotFormat::Png, &own, &session), format!("{common} --include-border-title --subtract-soldermask {fab} --drill-shape-opt 2 --scale 1.5 --mirror --negative --dpi 600 --no-antialias"));
        assert!(args(PlotFormat::Gerber, &own, &session).ends_with("--use-drill-file-origin"), "the Gerber plot's origin");
        // The background is a colour-mode option, and only one of the two popup boxes off keeps the popups (one flag clears both).
        own.set_option("plot_black_and_white", "yes").unwrap();
        own.set_option("pdf_back_fp_property_popups", "yes").unwrap();
        let pdf = args(PlotFormat::Pdf, &own, &session);
        assert!(!pdf.contains("--bg-color") && !pdf.contains("--no-property-popups") && pdf.contains("--black-and-white"), "{pdf}");
        assert_eq!(["0", "1", "2", "3", "4"].map(|n| selection_to_scale(n.parse().unwrap())), ["0", "1", "1.5", "2", "3"], "Auto, 1:1, 3:2, 2:1, 3:1");
        assert_eq!(PlotFormat::from_outputformat(0), PlotFormat::Gerber, "HPGL, no longer supported: the choice stays at Gerber");
        assert_eq!(PlotSession::of_request(&serde_json::json!({ "pdf_background_color": "url(x)" })).pdf_background_color, None, "no CSS colour");
    }

    /// The dialogs' "Output directory:": relative to the board's folder, its text variables expanded, created by the export; empty is the
    /// studio's `export/kicad/<kind>/`; a folder outside the board's is refused in KiCad's words.
    #[test]
    fn the_output_folder_is_the_boards_own() {
        let dir = Path::new("/boards/b");
        let vars = |t: &str| (t == "REVISION").then(|| "B".to_string());
        assert_eq!(output_folder(dir, "", "gerbers", &vars).unwrap(), dir.join("export/kicad/gerbers"));
        assert_eq!(output_folder(dir, "plots/rev${REVISION}/", "gerbers", &vars).unwrap(), dir.join("plots/revB"));
        assert_eq!(output_folder(dir, "./a/../gerbers", "gerbers", &vars).unwrap(), dir.join("gerbers"));
        assert_eq!(output_folder(dir, "plots\\win", "gerbers", &vars).unwrap(), dir.join("plots/win"), "backslashes as KiCad stores them");
        assert_eq!(output_folder(dir, "/boards/b/out", "gerbers", &vars).unwrap(), dir.join("out"), "an absolute path inside the board's folder");
        let refused = output_folder(dir, "../elsewhere", "gerbers", &vars).unwrap_err();
        assert!(refused.starts_with("Could not write plot files to folder '../elsewhere'."), "{refused}");
        assert!(output_folder(dir, "/tmp/x", "gerbers", &vars).is_err());
    }

    /// `PCB_PLOTTER::Plot`'s report: "Plotted to '%s'." per file in the plotted order, the job file, "Done.".
    #[test]
    fn the_plot_report_is_the_plotters() {
        let files: Vec<String> = ["o/b-B_Cu.gbr", "o/b-Edge_Cuts.gbr", "o/b-F_Cu.gbr", "o/b-job.gbrjob"].map(String::from).to_vec();
        let order: Vec<String> = ["F_Cu", "B_Cu", "Edge_Cuts"].map(String::from).to_vec();
        assert_eq!(plot_report(&files, &order), "Plotted to 'o/b-F_Cu.gbr'.\nPlotted to 'o/b-B_Cu.gbr'.\nPlotted to 'o/b-Edge_Cuts.gbr'.\nCreated Gerber job file 'o/b-job.gbrjob'.\nDone.");
        assert_eq!(plot_report(&["o/b.pdf".to_string()], &order), "Plotted to 'o/b.pdf'.\nDone.", "one document");
        assert_eq!(ui_order(&["User.2".into(), "F.Fab".into(), "In1.Cu".into(), "B.Cu".into(), "F.Cu".into(), "F.SilkS".into(), "User.1".into()]), vec!["F.Cu", "In1.Cu", "B.Cu", "F.SilkS", "F.Fab", "User.1", "User.2"]);
    }

    /// X2 (UI-LEFT): the Generate Drill Files dialog's controls onto `kicad-cli pcb export drill`'s flags.
    #[test]
    fn the_drill_dialogs_controls_become_kicad_clis_flags() {
        let d = DrillOptions::default();
        assert_eq!(drill_dialog_args(&d).join(" "), "--format excellon --excellon-units mm --excellon-zeros-format decimal --excellon-oval-format route --excellon-separate-th", "KiCad's defaults");
        let excellon = DrillOptions { mirror_y: true, minimal_header: true, merge_pth_npth: true, alt_drill_mode: true, units_mm: false, zeros: "suppressleading".into(), generate_map: true, map_format: "pdf".into(), ..d.clone() };
        assert_eq!(
            drill_dialog_args(&excellon).join(" "),
            "--format excellon --excellon-units in --excellon-zeros-format suppressleading --excellon-oval-format alternate --excellon-mirror-y --excellon-min-header --generate-map --map-format pdf"
        );
        let gerber = DrillOptions { gerber: true, generate_tenting: true, mirror_y: true, gerber_precision: 5, ..d.clone() };
        assert_eq!(drill_dialog_args(&gerber).join(" "), "--format gerber --gerber-precision 5 --generate-tenting", "the Excellon-only controls are disabled for Gerber X2");
        let req = serde_json::json!({ "format": "gerber", "zeros": "nonsense", "map_format": "svg", "generate_map": true });
        let o = DrillOptions::of_request(&req, 4);
        assert!(o.gerber && o.zeros == "decimal" && o.map_format == "svg" && o.gerber_precision == 6);
        assert_eq!(drill_report(&["o/b-PTH-drl_map.gbr".into(), "o/b-NPTH.drl".into(), "o/b-PTH.drl".into(), "o/b-NPTH-drl_map.gbr".into()]), "Created file 'o/b-PTH.drl'\nCreated file 'o/b-NPTH.drl'\nCreated file 'o/b-PTH-drl_map.gbr'.\nCreated file 'o/b-NPTH-drl_map.gbr'.\nDone.", "the maps after every drill file");
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
