//! Pictures of a design as KiCad draws them: `kicad-cli sch export svg|png` of the derived schematic and `kicad-cli pcb export svg|png` of the
//! derived board. `eda schematic`'s `schematic.svg`, the studio's `GET /api/schematic.svg` and the vision-model judge's `judge_<stage>.{svg,png}`
//! are these files: nothing in this program draws a schematic or a board picture of its own (docs/ARCHITECTURE.md, "Engines"; the hand-drawn
//! `eda-render` sheet and the judge's `render_board_svg` are kept inactive, behind `EDA_RUST_PICTURES=1`).
//!
//! A missing kicad-cli is the `kicad_cli_missing` failure every kicad-cli export gives, with its reason; the caller says what was not drawn.

use super::{cli_command, cli_has_export, export_board, export_schematic, fail, file_stem, limit, need_cli, output_within, write_file, EXPORT_TIMEOUT};
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use std::path::{Path, PathBuf};

/// The file formats a picture comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFormat {
    Svg,
    Png,
}

impl PictureFormat {
    pub fn ext(self) -> &'static str {
        match self {
            PictureFormat::Svg => "svg",
            PictureFormat::Png => "png",
        }
    }
}

/// The resolution of a PNG picture. An A4 page at 150 dpi is 1754 x 1240 pixels, about the 1600 pixels the judge's pictures always had (and what
/// the vision model's API keeps of an image without scaling it down).
pub const PICTURE_DPI: u32 = 150;

/// KiCad's default PCB background (`builtin_color_themes.h`, `LAYER_PCB_BACKGROUND`: `CSS_COLOR( 0, 16, 35, 1 )`), for a rasteriser that has to
/// fill the transparent background of a board's SVG plot the way the PCB editor shows it.
pub const PCB_BACKGROUND: &str = "#001023";

/// Whether this kicad-cli plots PNG itself (`sch export png`, `pcb export png`: `JOB_EXPORT_PCB_PNG`, `PCBNEW_JOBS_HANDLER::JobExportPng`). Older
/// nightlies have only SVG; their pictures are rasterised by the caller.
pub fn plots_png(scope: &str) -> bool {
    super::find_cli().is_some_and(|cli| cli_has_export(&cli, scope, "png"))
}

/// The paths kicad-cli reports it wrote, in its order ("Plotted to '<path>'.", one line per page or layer), else every file with `ext` in
/// `out_dir` (a kicad-cli that words it otherwise).
fn plotted(said: &str, out_dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = said
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Plotted to '").and_then(|r| r.strip_suffix("'.")))
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect();
    if files.is_empty() {
        files = std::fs::read_dir(out_dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect();
        files.sort();
    }
    files
}

/// Clears the files of an earlier run from `out_dir` (kicad-cli's plots of one stem), so a page this run did not draw is not taken for one it did.
fn clear(out_dir: &Path, ext: &str) -> Result<(), Vec<CheckResult>> {
    let _ = std::fs::remove_dir_all(out_dir);
    std::fs::create_dir_all(out_dir).map_err(|e| fail("kicad_engine_dir", &out_dir.display().to_string(), format!("cannot create {}: {e} ({ext} plot)", out_dir.display())))
}

/// The design's schematic plotted by `kicad-cli sch export svg|png` with KiCad's defaults (the drawing sheet, the theme's background): one file per
/// sheet, the root's first and then in KiCad's page order (`SCH_SHEET_LIST`), as kicad-cli names them -- `<name>.<ext>` for the root, the sheet's unique
/// file name (`SCHEMATIC::GetUniqueFilenameForCurrentSheet`, `/` made `_`) for each other sheet (`SCH_PLOTTER::getOutputFilenameSingle`). The derived schematic (and its project) is written into `work`, named
/// `name`, so the title block shows that file; the plots go into `out_dir`, which is emptied first. PNG pages are plotted at [`PICTURE_DPI`].
pub fn schematic_plot(design: &Design, model: &ConstraintModel, work: &Path, out_dir: &Path, name: &str, format: PictureFormat) -> Result<Vec<PathBuf>, Vec<CheckResult>> {
    let cli = need_cli()?;
    if design.schematic.is_none() {
        return Err(fail("kicad_cli_export", "schematic", "the design has no schematic to plot"));
    }
    let (sch, _) = export_schematic(design, model, work, &file_stem(name))?;
    clear(out_dir, format.ext())?;
    let mut cmd = cli_command(&cli, None);
    cmd.args(["sch", "export", format.ext()]);
    if format == PictureFormat::Png {
        cmd.args(["--dpi", &PICTURE_DPI.to_string()]);
    }
    // kicad-cli takes a schematic plot's `-o` as a folder (one file per page).
    cmd.arg("-o").arg(format!("{}/", out_dir.display())).arg(&sch);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let files = plotted(&said, out_dir, format.ext());
    if !out.status.success() || files.is_empty() {
        return Err(fail("kicad_cli_export", format.ext(), format!("kicad-cli sch export {} failed: {}", format.ext(), said.trim())));
    }
    Ok(files)
}

/// The layers of a board picture, back to front: the outline, then each side's courtyards, silkscreen and copper (inner layers between), the
/// front last -- the copper and the parts' outlines a reviewer reads a placement or a routing by, as the design-block preview shows a board
/// ([`super::export_file_svg`]).
pub fn board_picture_layers(model: &ConstraintModel) -> Vec<String> {
    let mut layers: Vec<String> = vec!["Edge.Cuts".into(), "B.Courtyard".into(), "B.SilkS".into()];
    let copper = &model.board.layers;
    let mut inner: Vec<String> = copper.iter().filter(|l| *l != "F.Cu" && *l != "B.Cu").cloned().collect();
    inner.reverse();
    layers.push("B.Cu".into());
    layers.extend(inner);
    layers.extend(["F.Cu".to_string(), "F.SilkS".into(), "F.Courtyard".into()]);
    layers
}

/// The `--layers` / `--common-layers` of the board PNG, painted in the order of `layers` ([`board_picture_layers`], back to front): kicad-cli
/// plots the `--layers` entry first and then the common layers in their order (`pcb_plotter.cpp::PCB_PLOTTER::getPlotSequence`, 25ec2335:
/// "Base layer always gets plotted first."; `PCBNEW_JOBS_HANDLER::convertLayerArg` keeps the list's order), so the base is the back-most layer,
/// the outline. With the front copper as the base (round 4's X4-1) it was painted first and the back copper, the inner layers, the back
/// silkscreen and the courtyards covered it. The file kicad-cli writes is then `<board>-Edge_Cuts.png`, read from its "Plotted to" report.
pub fn board_png_layer_args(layers: &[String]) -> Vec<String> {
    let (base, common) = layers.split_first().map(|(b, c)| (b.clone(), c.join(","))).unwrap_or_else(|| ("Edge.Cuts".into(), String::new()));
    let mut args = vec!["--layers".to_string(), base];
    if !common.is_empty() {
        args.extend(["--common-layers".to_string(), common]);
    }
    args
}

/// The arguments of `kicad-cli pcb export png` after the command (the board file last is the caller's): [`board_png_layer_args`], the board
/// scaled to the page on the theme's background with the drills at their real shape, at [`PICTURE_DPI`], with the drawing sheet `sheet`
/// ([`EMPTY_DRAWING_SHEET`]), into the folder `out_dir`.
pub fn board_png_args(layers: &[String], sheet: &Path, out_dir: &Path) -> Vec<String> {
    let mut args = board_png_layer_args(layers);
    args.extend(["--scale", "0", "--bg-color", "--drill-shape-opt", "2", "--dpi"].map(String::from));
    args.push(PICTURE_DPI.to_string());
    args.extend(["--drawing-sheet".to_string(), sheet.display().to_string(), "-o".to_string(), format!("{}/", out_dir.display())]);
    args
}

/// A drawing sheet with nothing on it, for `pcb export png`, which always plots the page's frame and title block (`JOB_EXPORT_PCB_PLOT`'s
/// `m_plotDrawingSheet( true )`; the command has `--include-border-title` but no way to leave it out): with this sheet the frame draws nothing and
/// the board is the whole picture, as `pcb export svg --exclude-drawing-sheet` gives it.
pub const EMPTY_DRAWING_SHEET: &str = "(kicad_wks (version 20231118) (generator \"eda\")\n  (setup (textsize 1.5 1.5) (linewidth 0.15) (textlinewidth 0.15)\n    (left_margin 0) (right_margin 0) (top_margin 0) (bottom_margin 0))\n)\n";

/// The design's board plotted into the one file `target` by kicad-cli, the [`board_picture_layers`] on KiCad's colour theme with the drills at
/// their real shape:
/// - SVG: `pcb export svg --mode-single --page-size-mode 2 --exclude-drawing-sheet` -- the board area only, on a transparent background;
/// - PNG: `pcb export png --scale 0 --bg-color` at [`PICTURE_DPI`] -- the board scaled to the page (KiCad's "auto" plot scale) on the theme's
///   background, with an empty drawing sheet ([`EMPTY_DRAWING_SHEET`]). The command plots one file per `--layers` entry, so the back-most
///   layer is the one layer and the rest are its `--common-layers` ([`board_png_layer_args`]).
///
/// With `routing` off the design's tracks, vias and zones are left out (a placement is judged without them). The derived board is written into
/// `work`, named `name`.
pub fn board_plot(design: &Design, model: &ConstraintModel, work: &Path, target: &Path, name: &str, format: PictureFormat, routing: bool) -> Result<PathBuf, Vec<CheckResult>> {
    let cli = need_cli()?;
    if design.placement.is_none() {
        return Err(fail("kicad_cli_export", "board", "the design has no placement to plot"));
    }
    let unrouted;
    let design = if routing {
        design
    } else {
        let mut d = design.clone();
        d.routing = None;
        unrouted = d;
        &unrouted
    };
    let stem = file_stem(name);
    let (pcb, _) = export_board(design, model, work, &stem)?;
    let layers = board_picture_layers(model);
    let _ = std::fs::remove_file(target);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| fail("kicad_engine_dir", &parent.display().to_string(), e.to_string()))?;
    }
    let mut cmd = cli_command(&cli, None);
    let out_dir = work.join("picture-png");
    match format {
        PictureFormat::Svg => {
            cmd.args(["pcb", "export", "svg", "--mode-single", "--page-size-mode", "2", "--exclude-drawing-sheet", "--drill-shape-opt", "2", "--layers"])
                .arg(layers.join(","))
                .arg("-o")
                .arg(target);
        }
        PictureFormat::Png => {
            clear(&out_dir, "png")?;
            let sheet = work.join("empty.kicad_wks");
            write_file(&sheet, EMPTY_DRAWING_SHEET)?;
            cmd.args(["pcb", "export", "png"]).args(board_png_args(&layers, &sheet, &out_dir));
        }
    }
    cmd.arg(&pcb);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if format == PictureFormat::Png {
        if let Some(file) = plotted(&said, &out_dir, "png").into_iter().next() {
            std::fs::rename(&file, target).or_else(|_| std::fs::copy(&file, target).map(|_| ())).map_err(|e| fail("kicad_engine_write", &target.display().to_string(), e.to_string()))?;
        }
    }
    if !out.status.success() || !target.is_file() {
        return Err(fail("kicad_cli_export", format.ext(), format!("kicad-cli pcb export {} failed: {}", format.ext(), said.trim())));
    }
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// X4-1: kicad-cli paints the `--layers` entry first (`PCB_PLOTTER::getPlotSequence`), so the PNG's base is the back-most layer and the
    /// front copper comes after the back copper.
    #[test]
    fn the_board_png_paints_the_layers_back_to_front() {
        let mut model = ConstraintModel::default();
        model.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
        let args = board_png_layer_args(&board_picture_layers(&model));
        assert_eq!(args, ["--layers", "Edge.Cuts", "--common-layers", "B.Courtyard,B.SilkS,B.Cu,In2.Cu,In1.Cu,F.Cu,F.SilkS,F.Courtyard"]);
    }
}
