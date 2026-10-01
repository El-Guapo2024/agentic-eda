//! `eda fab gerbers|drill|pos|bom <board dir>` -- fabrication outputs for a
//! board held in a directory (`eda board new`/`eda board serve`'s own
//! directory, see `board.rs`'s module doc), written straight from the
//! board's own `design.json`/intent rather than through a pipeline
//! `-o out` run. Shares `crate::board::load` with every other `eda board`
//! verb, so a file written here always reflects the board exactly as the
//! last `eda board`/studio command left it.
//!
//! ```text
//! eda fab gerbers <dir> [--layers F.Cu,B.Cu,...]
//! eda fab drill   <dir> [--separate-th]
//! eda fab pos     <dir> [--format csv|ascii] [--side front|back|both] [--units mm|in]
//! eda fab bom     <dir>
//! ```
//!
//! Every verb writes into `<dir>/export/` and prints the files it wrote --
//! the same convention (and, for the three verbs that take options, the
//! same option set) `crate::studio`'s `/api/fab/*` endpoints use, so the
//! CLI and the studio's Plot / Generate Drill Files / Footprint Position
//! Files dialogs produce identical output for identical input.

use crate::board;
use eda_model::CheckResult;
use std::path::{Path, PathBuf};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Vec<CheckResult>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| fail("io", &parent.display().to_string(), e.to_string()))?;
    }
    std::fs::write(path, bytes).map_err(|e| fail("io", &path.display().to_string(), e.to_string()))
}

fn title_of(dir: &Path) -> Result<String, Vec<CheckResult>> {
    let (meta, _, _) = board::load(dir)?;
    Ok(Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string())
}

fn fab_meta(title: &str) -> eda_fab::gerber::FabMeta {
    let date = eda::now_rfc3339();
    eda_fab::gerber::FabMeta { title: title.into(), date, rev: "rev?".into(), generator_version: env!("CARGO_PKG_VERSION").into() }
}

pub fn run(argv: &[String]) -> Result<(), Vec<CheckResult>> {
    let verb = argv.first().ok_or_else(|| fail("fab", "usage", "usage: eda fab <gerbers|drill|pos|bom> <board dir> [options]"))?;
    let dir = argv.get(1).map(PathBuf::from).ok_or_else(|| fail("fab", "usage", "missing board directory"))?;
    let opts = &argv[2.min(argv.len())..];
    match verb.as_str() {
        "gerbers" => gerbers(&dir, opts),
        "drill" => drill(&dir, opts),
        "pos" => pos(&dir, opts),
        "bom" => bom(&dir),
        other => Err(fail("fab", "usage", format!("unknown verb {other:?}: expected gerbers, drill, pos or bom"))),
    }
}

fn flag_value<'a>(opts: &'a [String], name: &str) -> Option<&'a str> {
    opts.iter().position(|a| a == name).and_then(|i| opts.get(i + 1)).map(String::as_str)
}
fn has_flag(opts: &[String], name: &str) -> bool {
    opts.iter().any(|a| a == name)
}

fn gerbers(dir: &Path, opts: &[String]) -> Result<(), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let title = title_of(dir)?;
    let meta = fab_meta(&title);
    let layers = match flag_value(opts, "--layers") {
        Some(list) => parse_layers(list, &model.board.layers)?,
        None => eda_fab::gerber::default_jlc_layers(model.board.layers.len().max(2)),
    };
    let files = eda_fab::gerber::plot_all(&design, &model, &meta, &layers)?;
    let out_dir = dir.join("export");
    let mut written = Vec::new();
    for f in &files {
        let path = out_dir.join(&f.filename);
        write(&path, f.content.as_bytes())?;
        written.push(path);
    }
    let job_path = out_dir.join(format!("{title}-job.gbrjob"));
    write(&job_path, eda_fab::job::write_job(&design, &model, &meta, &files).as_bytes())?;
    written.push(job_path);
    print_written(&written);
    Ok(())
}

/// `--layers` names a KiCad canonical layer token ("F.Cu", "B.Mask", ...);
/// anything not recognised is a hard error rather than silently dropped,
/// same reasoning as `eda_kicad::pcb`'s own unknown-net rule: a flag the
/// caller typed that quietly did nothing is worse than one that fails.
/// `pub(crate)`: shared with `fab_api`'s `/api/fab/gerbers`, so a studio
/// dialog and this CLI verb accept the same layer names.
pub(crate) fn parse_layers(list: &str, copper: &[String]) -> Result<Vec<eda_fab::gerber::GerberLayer>, Vec<CheckResult>> {
    use eda_fab::gerber::GerberLayer;
    use eda_model::ir::Side;
    let mut out = Vec::new();
    for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(i) = copper.iter().position(|l| l == name) {
            out.push(GerberLayer::Copper(i));
            continue;
        }
        out.push(match name {
            "F.Mask" => GerberLayer::Mask(Side::Top),
            "B.Mask" => GerberLayer::Mask(Side::Bottom),
            "F.Paste" => GerberLayer::Paste(Side::Top),
            "B.Paste" => GerberLayer::Paste(Side::Bottom),
            "F.SilkS" | "F.Silkscreen" => GerberLayer::Silk(Side::Top),
            "B.SilkS" | "B.Silkscreen" => GerberLayer::Silk(Side::Bottom),
            "Edge.Cuts" => GerberLayer::EdgeCuts,
            other => return Err(fail("fab", "--layers", format!("unknown layer {other:?}"))),
        });
    }
    if out.is_empty() {
        return Err(fail("fab", "--layers", "--layers named no layers"));
    }
    Ok(out)
}

fn drill(dir: &Path, opts: &[String]) -> Result<(), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let title = title_of(dir)?;
    let meta = eda_fab::drill::DrillMeta { title: title.clone(), date: eda::now_rfc3339(), generator_version: env!("CARGO_PKG_VERSION").into() };
    let drill_opts = eda_fab::drill::DrillOptions { separate_th: has_flag(opts, "--separate-th") };
    let files = eda_fab::drill::write_drill(&design, &model, &meta, drill_opts)?;
    let out_dir = dir.join("export");
    let mut written = Vec::new();
    for f in &files {
        let path = out_dir.join(&f.filename);
        write(&path, f.content.as_bytes())?;
        written.push(path);
    }
    if has_flag(opts, "--generate-report") {
        let report = eda_fab::drill::drill_report(&design, &model)?;
        let path = out_dir.join(format!("{title}-drill-report.txt"));
        write(&path, report.as_bytes())?;
        written.push(path);
    }
    print_written(&written);
    Ok(())
}

fn pos(dir: &Path, opts: &[String]) -> Result<(), Vec<CheckResult>> {
    use eda_fab::position::{PosFormat, PosOptions, PosSide};
    let (_, design, model) = board::load(dir)?;
    let title = title_of(dir)?;
    let format = match flag_value(opts, "--format") {
        Some("ascii") => PosFormat::Ascii,
        Some("csv") | None => PosFormat::Csv,
        Some(other) => return Err(fail("fab", "--format", format!("unknown position format {other:?}: expected csv or ascii"))),
    };
    let side = match flag_value(opts, "--side") {
        Some("front") => PosSide::Front,
        Some("back") => PosSide::Back,
        Some("both") | None => PosSide::Both,
        Some(other) => return Err(fail("fab", "--side", format!("unknown side {other:?}: expected front, back or both"))),
    };
    let units_mm = flag_value(opts, "--units") != Some("in");
    let opts = PosOptions { format, side, units_mm, smd_only: has_flag(opts, "--smd-only"), exclude_fp_th: has_flag(opts, "--exclude-fp-th") };
    let meta = eda_fab::position::PosMeta { date: eda::now_rfc3339(), generator_version: env!("CARGO_PKG_VERSION").into() };
    let content = eda_fab::position::write_pos(&design, &model, &meta, opts)?;
    let ext = if format == PosFormat::Ascii { "pos" } else { "csv" };
    let path = dir.join("export").join(format!("{title}.{ext}"));
    write(&path, content.as_bytes())?;
    print_written(&[path]);
    Ok(())
}

fn bom(dir: &Path) -> Result<(), Vec<CheckResult>> {
    let (_, _, model) = board::load(dir)?;
    let title = title_of(dir)?;
    let path = dir.join("export").join(format!("{title}-bom.csv"));
    write(&path, eda_fab::bom_csv(&model).as_bytes())?;
    print_written(&[path]);
    Ok(())
}

fn print_written(paths: &[PathBuf]) {
    for p in paths {
        println!("wrote {}", p.display());
    }
}
