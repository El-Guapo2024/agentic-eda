//! `eda fab gerbers|drill|pos|bom <board dir>` -- fabrication outputs for a
//! board held in a directory (`eda board new`/`eda board serve`'s own
//! directory, see `board.rs`'s module doc), written by kicad-cli from the
//! board's own `design.json` (docs/ARCHITECTURE.md, "Engines"): the flags
//! below become kicad-cli arguments (`crate::fab_api`'s builders, shared
//! with the studio's dialogs) and run through `crate::kicad_engine`.
//!
//! ```text
//! eda fab gerbers <dir> [--layers F.Cu,B.Cu,...]
//! eda fab drill   <dir> [--separate-th] [--generate-report]
//! eda fab pos     <dir> [--format csv|ascii] [--side front|back|both] [--units mm|in] [--smd-only] [--exclude-fp-th]
//! eda fab bom     <dir>
//! ```
//!
//! Every verb writes into `<dir>/export/kicad/<kind>/` and prints the files
//! it wrote. Any other kicad-cli export is `eda board export --kicad <kind>`.

use crate::{board, fab_api, kicad_engine};
use eda_model::CheckResult;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

pub fn run(argv: &[String]) -> Result<(), Vec<CheckResult>> {
    let verb = argv.first().ok_or_else(|| fail("fab", "usage", "usage: eda fab <gerbers|drill|pos|bom> <board dir> [options]"))?;
    let dir = argv.get(1).map(PathBuf::from).ok_or_else(|| fail("fab", "usage", "missing board directory"))?;
    let opts = &argv[2.min(argv.len())..];
    match verb.as_str() {
        "gerbers" => {
            let layers: Vec<String> = match flag_value(opts, "--layers") {
                Some(list) => list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
                None => {
                    let (_, _, model) = board::load(&dir)?;
                    fab_api::default_gerber_layers(&model.board.layers)
                }
            };
            if layers.is_empty() {
                return Err(fail("fab", "--layers", "--layers named no layers"));
            }
            print_written(&dir, kicad_engine::export(&dir, "gerbers", &fab_api::gerber_args(&layers))?)
        }
        "drill" => print_written(&dir, kicad_engine::export(&dir, "drill", &fab_api::drill_args(has_flag(opts, "--separate-th"), has_flag(opts, "--generate-report")))?),
        "pos" => {
            let format = flag_value(opts, "--format").unwrap_or("csv");
            if !matches!(format, "csv" | "ascii") {
                return Err(fail("fab", "--format", format!("unknown position format {format:?}: expected csv or ascii")));
            }
            let side = flag_value(opts, "--side").unwrap_or("both");
            if !matches!(side, "front" | "back" | "both") {
                return Err(fail("fab", "--side", format!("unknown side {side:?}: expected front, back or both")));
            }
            let args = fab_api::pos_args(format, side, flag_value(opts, "--units") != Some("in"), has_flag(opts, "--smd-only"), has_flag(opts, "--exclude-fp-th"));
            print_written(&dir, kicad_engine::export(&dir, "pos", &args)?)
        }
        "bom" => print_written(&dir, kicad_engine::export_sch(&dir, "bom", &fab_api::bom_args())?),
        other => Err(fail("fab", "usage", format!("unknown verb {other:?}: expected gerbers, drill, pos or bom"))),
    }
}

fn flag_value<'a>(opts: &'a [String], name: &str) -> Option<&'a str> {
    opts.iter().position(|a| a == name).and_then(|i| opts.get(i + 1)).map(String::as_str)
}

fn has_flag(opts: &[String], name: &str) -> bool {
    opts.iter().any(|a| a == name)
}

fn print_written(dir: &Path, reply: Value) -> Result<(), Vec<CheckResult>> {
    for f in reply["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        println!("wrote {}", dir.join(f).display());
    }
    Ok(())
}
