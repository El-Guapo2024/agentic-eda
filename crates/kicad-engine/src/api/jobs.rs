//! Board export jobs on the kept server (`RunBoardJobExport*`, `pcbnew/api/api_handler_pcb.cpp`): the job kicad-cli would run (`JOB_EXPORT_PCB_DRILL`,
//! `JOB_EXPORT_PCB_POS`, `JOB_EXPORT_PCB_IPCD356`, through `KIWAY::ProcessJob` as `ExecuteBoardJob` runs it) without starting a kicad-cli for it.
//!
//! A job message carries every setting itself (proto3: an unset field is zero, not kicad-cli's default), so [`board_job`] builds it from the arguments a
//! per-run `kicad-cli pcb export <kind>` would get, with kicad-cli's defaults for the ones not given (`kicad/cli/command_pcb_export_drill.cpp`,
//! `command_pcb_export_pos.cpp`, `command_pcb_export_ipcd356.cpp`). It knows the arguments those commands list; any other argument, or a value kicad-cli
//! would refuse, gives `None`, and the export runs per-run, where kicad-cli answers it itself.
//!
//! Three kinds go through here: `drill`, `pos` and `ipcd356`. The others stay per-run for now (their messages carry plot settings, layer lists and
//! variants, each a mapping of its own to prove against kicad-cli first): docs/parity/GAPS.md.

use super::proto::kiapi::board::jobs::{
    DrillFormat, DrillGerberPrecision, DrillMapFormat, DrillOrigin, DrillZerosFormat, ExcellonFormatOptions, PositionFormat, PositionSide, RunBoardJobExportDrill, RunBoardJobExportIpcD356, RunBoardJobExportPosition,
};
use super::proto::kiapi::common::types::{DocumentSpecifier, JobStatus, RunJobResponse, RunJobSettings, Units};
use super::server::{with_server, ApiError};
use std::path::Path;

/// One board job, ready but for its document and output path.
#[derive(Debug, Clone, PartialEq)]
pub enum BoardJob {
    Drill(RunBoardJobExportDrill),
    Position(RunBoardJobExportPosition),
    IpcD356(RunBoardJobExportIpcD356),
}

/// The kinds [`board_job`] builds.
pub const KINDS: &[&str] = &["drill", "pos", "ipcd356"];

/// Arguments as kicad-cli's argparse reads them: `--flag`, `--opt value` and `--opt=value`.
struct Args<'a> {
    args: &'a [String],
}

impl Args<'_> {
    /// Every argument is one of `flags` or `options` (with its value): else `None`.
    fn check(&self, flags: &[&str], options: &[&str]) -> Option<()> {
        let mut i = 0;
        while i < self.args.len() {
            let a = self.args[i].as_str();
            let (name, inline) = a.split_once('=').map_or((a, false), |(n, _)| (n, true));
            if flags.contains(&name) && !inline {
                i += 1;
            } else if options.contains(&name) {
                i += if inline { 1 } else { 2 };
                if i > self.args.len() {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(())
    }

    fn flag(&self, name: &str) -> bool {
        self.args.iter().any(|a| a == name)
    }

    /// The value of `names` (the last given wins, as argparse keeps it), else `default`.
    fn value<'b>(&'b self, names: &[&str], default: &'b str) -> &'b str {
        let mut v = default;
        let mut i = 0;
        while i < self.args.len() {
            let a = self.args[i].as_str();
            if let Some((n, val)) = a.split_once('=') {
                if names.contains(&n) {
                    v = val;
                }
            } else if names.contains(&a) {
                if let Some(next) = self.args.get(i + 1) {
                    v = next;
                    i += 1;
                }
            }
            i += 1;
        }
        v
    }
}

/// `CLI::PCB_EXPORT_DRILL_COMMAND::doPerform`'s job, from its arguments (`--generate-report`/`--report-path` are left to the per-run kicad-cli).
fn drill(a: &Args) -> Option<RunBoardJobExportDrill> {
    a.check(
        &["--excellon-mirror-y", "--excellon-min-header", "--excellon-separate-th", "--generate-map", "--generate-tenting"],
        &["--format", "--drill-origin", "--excellon-zeros-format", "--excellon-oval-format", "-u", "--excellon-units", "--map-format", "--gerber-precision"],
    )?;
    let format = match a.value(&["--format"], "excellon") {
        "excellon" => DrillFormat::DfExcellon,
        "gerber" => DrillFormat::DfGerber,
        _ => return None,
    };
    let units = match a.value(&["-u", "--excellon-units"], "mm") {
        "mm" => Units::UMm,
        "in" => Units::UInch,
        _ => return None,
    };
    let zeros = match a.value(&["--excellon-zeros-format"], "decimal") {
        "decimal" => DrillZerosFormat::DzfDecimal,
        "suppressleading" => DrillZerosFormat::DzfSuppressLeading,
        "suppresstrailing" => DrillZerosFormat::DzfSuppressTrailing,
        "keep" => DrillZerosFormat::DzfKeepZeros,
        _ => return None,
    };
    let route_oval_holes = match a.value(&["--excellon-oval-format"], "alternate") {
        "route" => true,
        "alternate" => false,
        _ => return None,
    };
    let map_format = match a.value(&["--map-format"], "pdf") {
        "pdf" => DrillMapFormat::DmfPdf,
        "ps" => DrillMapFormat::DmfPostscript,
        "gerberx2" => DrillMapFormat::DmfGerberX2,
        "dxf" => DrillMapFormat::DmfDxf,
        "svg" => DrillMapFormat::DmfSvg,
        _ => return None,
    };
    let origin = match a.value(&["--drill-origin"], "absolute") {
        "absolute" => DrillOrigin::DoAbsolute,
        "plot" => DrillOrigin::DoPlot,
        _ => return None,
    };
    // `m_gerberPrecision = get<int>( ARG_GERBER_PRECISION )`; the handler takes 5 for `DGP_4_5`, 6 for anything else.
    let precision = match a.value(&["--gerber-precision"], "6") {
        "5" => DrillGerberPrecision::Dgp45,
        "6" => DrillGerberPrecision::Dgp46,
        _ => return None,
    };
    Some(RunBoardJobExportDrill {
        job_settings: None,
        format: format as i32,
        units: units as i32,
        origin: origin as i32,
        zeros_format: zeros as i32,
        excellon: Some(ExcellonFormatOptions {
            mirror_y: Some(a.flag("--excellon-mirror-y")),
            minimal_header: Some(a.flag("--excellon-min-header")),
            combine_pth_npth: Some(!a.flag("--excellon-separate-th")),
            route_oval_holes: Some(route_oval_holes),
        }),
        // A map only with `--generate-map` (`m_generateMap`); the handler makes one for any format but DMF_UNKNOWN.
        map_format: if a.flag("--generate-map") { map_format as i32 } else { DrillMapFormat::DmfUnknown as i32 },
        gerber_precision: precision as i32,
        gerber_generate_tenting: Some(a.flag("--generate-tenting")),
        report_format: 0,
        report_filename: None,
    })
}

/// `CLI::PCB_EXPORT_POS_COMMAND::doPerform`'s job: one file (`m_singleFile = true`) named as `-o` names it (`m_nakedFilename = true`). `--variant` is left to
/// the per-run kicad-cli.
fn position(a: &Args) -> Option<RunBoardJobExportPosition> {
    a.check(&["--bottom-negate-x", "--use-drill-file-origin", "--smd-only", "--exclude-fp-th", "--exclude-dnp", "--gerber-board-edge"], &["--side", "--format", "--units"])?;
    let format = match a.value(&["--format"], "ascii") {
        "ascii" => PositionFormat::PfAscii,
        "csv" => PositionFormat::PfCsv,
        "gerber" => PositionFormat::PfGerber,
        _ => return None,
    };
    let units = match a.value(&["--units"], "in") {
        "mm" => Units::UMm,
        "in" => Units::UInch,
        _ => return None,
    };
    let side = match a.value(&["--side"], "both") {
        "both" => PositionSide::PsBoth,
        "front" => PositionSide::PsFront,
        "back" => PositionSide::PsBack,
        _ => return None,
    };
    // kicad-cli refuses `--side both` for a Gerber placement file ("--side both is not supported for gerber format"): its own words, per-run.
    if format == PositionFormat::PfGerber && side == PositionSide::PsBoth {
        return None;
    }
    Some(RunBoardJobExportPosition {
        job_settings: None,
        use_drill_place_file_origin: Some(a.flag("--use-drill-file-origin")),
        smd_only: a.flag("--smd-only"),
        exclude_footprints_with_th: a.flag("--exclude-fp-th"),
        exclude_dnp: a.flag("--exclude-dnp"),
        // Not a kicad-cli option: `JOB_EXPORT_PCB_POS`'s default.
        exclude_from_bom: false,
        negate_bottom_x: a.flag("--bottom-negate-x"),
        single_file: true,
        naked_filename: true,
        side: side as i32,
        units: units as i32,
        format: format as i32,
        include_board_edge_for_gerber: Some(a.flag("--gerber-board-edge")),
        variant: String::new(),
    })
}

/// The job of `kicad-cli pcb export <kind> <args>`, or `None` when this lane does not build it (another kind, an argument it does not map, a value
/// kicad-cli would refuse): the export then runs per-run.
pub fn board_job(kind: &str, args: &[String]) -> Option<BoardJob> {
    let a = Args { args };
    match kind {
        "drill" => drill(&a).map(BoardJob::Drill),
        "pos" => position(&a).map(BoardJob::Position),
        "ipcd356" => a.check(&[], &[]).map(|_| BoardJob::IpcD356(RunBoardJobExportIpcD356 { job_settings: None })),
        _ => None,
    }
}

/// What a job wrote: KiCad's `RunJobResponse` (status, output paths, message).
pub type JobResult = RunJobResponse;

/// Whether a job's answer is a success (`JS_SUCCESS`, or `JS_WARNING` with its message).
pub fn succeeded(r: &RunJobResponse) -> bool {
    matches!(JobStatus::try_from(r.status), Ok(JobStatus::JsSuccess) | Ok(JobStatus::JsWarning))
}

/// Run `jobs` on the board file `pcb`, opened once (and left open: a later call on the same bytes skips the load); each job's output goes where its path says (a folder ending in `/` for `drill`, a file for `pos`
/// and `ipcd356`, as `-o` takes them). The answers come in the jobs' order.
pub fn run_board_jobs(pcb: &Path, jobs: &[(BoardJob, String)]) -> Result<Vec<RunJobResponse>, ApiError> {
    with_server(|c| {
        let doc: DocumentSpecifier = c.open_board_kept(pcb)?;
        let settings = |out: &str| Some(RunJobSettings { document: Some(doc.clone()), output_path: out.to_string() });
        let mut answers = Vec::new();
        let timeout = crate::limit(crate::EXPORT_TIMEOUT);
        let mut run = || -> Result<(), ApiError> {
            for (job, out) in jobs {
                let r: RunJobResponse = match job {
                    BoardJob::Drill(j) => c.call(&RunBoardJobExportDrill { job_settings: settings(out), ..j.clone() }, timeout)?,
                    BoardJob::Position(j) => c.call(&RunBoardJobExportPosition { job_settings: settings(out), ..j.clone() }, timeout)?,
                    BoardJob::IpcD356(_) => c.call(&RunBoardJobExportIpcD356 { job_settings: settings(out) }, timeout)?,
                };
                answers.push(r);
            }
            Ok(())
        };
        // The board stays open for the next export of the same revision (`Client::open_board_kept`); one that failed is closed.
        let result = run();
        if result.is_err() {
            let _ = c.close_all();
        }
        result.map(|_| std::mem::take(&mut answers))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn kicad_clis_defaults_when_no_argument_is_given() {
        let Some(BoardJob::Drill(d)) = board_job("drill", &[]) else { panic!() };
        assert_eq!((d.format, d.units, d.origin, d.zeros_format, d.map_format, d.gerber_precision), (DrillFormat::DfExcellon as i32, Units::UMm as i32, DrillOrigin::DoAbsolute as i32, DrillZerosFormat::DzfDecimal as i32, 0, DrillGerberPrecision::Dgp46 as i32));
        assert_eq!(d.excellon, Some(ExcellonFormatOptions { mirror_y: Some(false), minimal_header: Some(false), combine_pth_npth: Some(true), route_oval_holes: Some(false) }));
        let Some(BoardJob::Position(p)) = board_job("pos", &[]) else { panic!() };
        assert_eq!((p.side, p.units, p.format, p.single_file, p.naked_filename, p.use_drill_place_file_origin), (PositionSide::PsBoth as i32, Units::UInch as i32, PositionFormat::PfAscii as i32, true, true, Some(false)));
        assert!(matches!(board_job("ipcd356", &[]), Some(BoardJob::IpcD356(_))));
    }

    #[test]
    fn the_arguments_kicad_cli_lists_are_mapped_and_any_other_goes_per_run() {
        let Some(BoardJob::Drill(d)) = board_job("drill", &args("--format gerber -u in --excellon-separate-th --generate-map --map-format=svg --gerber-precision 5")) else { panic!() };
        assert_eq!((d.format, d.units, d.map_format, d.gerber_precision), (DrillFormat::DfGerber as i32, Units::UInch as i32, DrillMapFormat::DmfSvg as i32, DrillGerberPrecision::Dgp45 as i32));
        assert_eq!(d.excellon.unwrap().combine_pth_npth, Some(false));
        let Some(BoardJob::Position(p)) = board_job("pos", &args("--side front --format csv --units mm --smd-only")) else { panic!() };
        assert_eq!((p.side, p.format, p.units, p.smd_only), (PositionSide::PsFront as i32, PositionFormat::PfCsv as i32, Units::UMm as i32, true));
        // Left to the per-run kicad-cli: an option this lane does not map, a value kicad-cli refuses, a kind it does not build.
        assert_eq!(board_job("drill", &args("--generate-report")), None);
        assert_eq!(board_job("drill", &args("--format nope")), None);
        assert_eq!(board_job("pos", &args("--variant A")), None);
        assert_eq!(board_job("pos", &args("--format gerber")), None, "gerber needs one side");
        assert_eq!(board_job("ipcd356", &args("--define-var X=1")), None);
        assert_eq!(board_job("gerbers", &[]), None);
        assert_eq!(board_job("drill", &args("--format")), None, "an option without its value");
    }
}
