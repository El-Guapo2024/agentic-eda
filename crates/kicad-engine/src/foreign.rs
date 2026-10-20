//! Other EDA formats in, through kicad-cli: `kicad-cli pcb import` and `kicad-cli sch import`.
//!
//! KiCad reads Eagle, Altium, CADSTAR, PADS, P-CAD, Fabmaster, EasyEDA and the rest with the `PCB_IO` and `SCH_IO` plugins of `PCB_IO_MGR` and `SCH_IO_MGR`
//! (`pcbnew/pcb_io/*`, `eeschema/sch_io/*`). Those are KiCad's and are not ported here: the installed kicad-cli runs them (`kicad-cli pcb import [--format ..]
//! [--layer-map FILE] [--report-format json --report-file F] -o OUT IN`, and the same under `sch`), and this module hands back what it made -- the KiCad file's text and
//! kicad-cli's own account of the import -- for [`eda_kicad::import_kicad_pcb`] and [`eda_kicad::load_sheet_file`] to read into a design. Nothing is converted in Rust.
//!
//! Every run goes through [`cli_command`] (none of the user's KiCad settings) in a folder of the caller's: kicad-cli writes more than the file it was asked for -- the
//! footprints of a board in a `<name>-import-fps.pretty` folder and an `fp-lib-table` beside it, the symbols of a schematic in a `.kicad_sym` with an `sym-lib-table`, one
//! file per sheet of a hierarchy -- and a second run in the same folder trips over the first's library cache ("has unexpected sub-folders"), so a run gets a folder to
//! itself ([`with_scratch`] makes one).
//!
//! # What kicad-cli says
//!
//! The import report (`--report-format json`) names the format it recognised, counts what was converted (`statistics`), lists `errors` and `warnings`, and, for a board,
//! says where every source layer went (`layer_mapping`: the KiCad layer, or `UNDEFINED`, and whether the mapping was `auto` or `explicit` -- a layer named in the map file).
//! What the importers report as they go (an Altium symbol with no library entry, a font KiCad lacks, a layer with no user layer to take it) goes to the terminal as
//! `HH:MM:SS AM: Error: ...` lines (`LOAD_INFO_REPORTER`) and not into the report; both are read, and [`ForeignImport::findings`] is what the user is shown.
//!
//! # The layer map
//!
//! `DIALOG_MAP_LAYERS` (`pcbnew/dialogs/dialog_map_layers.cpp`) is the dialog KiCad's board importers call back to with the layers of the source file. kicad-cli answers that
//! callback itself with the plugin's best guess (`AutoMapLayer`) and takes a JSON file of `{ "source layer": "KiCad layer" }` that overrides it for the layers it names
//! ([`ForeignOptions::layer_map`]); a layer it does not name keeps the guess. So a first run with no map shows the guess (`ImportReport::layers`), and a second run with the
//! person's answers makes the board.
//!
//! # The design rules
//!
//! `pcb import` writes the board alone, and a `.kicad_pcb` holds none of the design rules and net classes an importer sets (Eagle's, Altium's): they are in the board's design
//! settings, which KiCad saves with the project. The project-level `kicad-cli import -o STEM IN` writes that project (`STEM.kicad_pro`); it pairs nothing, refuses a second
//! schematic and prints no JSON report, so a board's text and report come from `pcb import` and only the project from there ([`ForeignOptions::project`]).

use super::{cli_command, cli_version, fail, file_stem, limit, need_cli, output_within, work_dir, write_file, EXPORT_TIMEOUT};
use eda_model::CheckResult;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which side of a project a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Board,
    Schematic,
}

impl Kind {
    /// The kicad-cli command group: `pcb` or `sch`.
    pub fn scope(self) -> &'static str {
        match self {
            Kind::Board => "pcb",
            Kind::Schematic => "sch",
        }
    }

    /// The extension of the KiCad file the import makes.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Board => "kicad_pcb",
            Kind::Schematic => "kicad_sch",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::Schematic => "schematic",
        }
    }
}

/// What a run is asked for beyond the file.
#[derive(Debug, Clone, Default)]
pub struct ForeignOptions {
    /// `--format`: the importer to use (`altium`, `eagle`, `cadstar`, `pads`, ...); `None` is kicad-cli's `auto`, which tells the format from the file.
    pub format: Option<String>,
    /// `--layer-map`: source layer name -> KiCad layer name, for a board. Layers that are not named keep the importer's guess.
    pub layer_map: BTreeMap<String, String>,
    /// Also have kicad-cli make the project of a board (`kicad-cli import`: a second run of the importer, a second or two): the design rules and the net classes the importer set
    /// (Eagle's design rules and its net classes, Altium's rules ...) are in the `.kicad_pro` it writes, and in no other file -- `pcb import` writes the board alone.
    pub project: bool,
}

/// One source layer of a board and where it went (`layer_mapping` of the report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerMapping {
    pub source: String,
    /// The KiCad layer's name; `None` when kicad-cli left the layer out (`UNDEFINED`).
    pub kicad: Option<String>,
    /// The layer was named in the layer map (`"method": "explicit"`); otherwise it is the importer's guess (`"auto"`).
    pub explicit: bool,
}

/// kicad-cli's import report (`--report-format json`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub source_file: String,
    /// The format kicad-cli recognised: `Eagle`, `Altium Designer`, `CADSTAR PCB Archive`, `PADS`, `P-Cad`, `Fabmaster`, `EasyEDA / JLCEDA Std`, ...
    pub source_format: String,
    pub output_file: String,
    /// What was converted: `footprints`, `tracks`, `vias`, `zones` for a board; `symbols`, `sheets`, `renamed_board_nets` for a schematic.
    pub statistics: BTreeMap<String, i64>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// A board's source layers, in the order of the report (sorted by name).
    pub layers: Vec<LayerMapping>,
}

impl ImportReport {
    /// Read the report kicad-cli wrote. A key it lacks (an older or newer kicad-cli) is an empty field, not an error.
    pub fn from_json(v: &Value) -> ImportReport {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let lines = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let statistics = v.get("statistics").and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, n)| n.as_i64().map(|n| (k.clone(), n))).collect()).unwrap_or_default();
        let layers = v
            .get("layer_mapping")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(source, entry)| LayerMapping {
                        source: source.clone(),
                        kicad: entry.get("kicad_layer").and_then(Value::as_str).filter(|l| !l.is_empty() && *l != "UNDEFINED").map(str::to_string),
                        explicit: entry.get("method").and_then(Value::as_str).is_some_and(|m| m != "auto"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        ImportReport { source_file: text("source_file"), source_format: text("source_format"), output_file: text("output_file"), statistics, errors: lines("errors"), warnings: lines("warnings"), layers }
    }
}

/// How a message of the importers weighs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// One line the importers printed while they ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub severity: Severity,
    pub text: String,
}

/// One thing the person is told about an import: the report's errors and warnings and the importers' own error and warning lines, a message once however often it was
/// said (`count`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
    pub count: usize,
}

/// The progress lines of kicad-cli's own job (`JOB_...`), not the importers'.
fn is_progress(line: &str) -> bool {
    (line.starts_with("Importing '") && line.contains(" using ")) || line.starts_with("Successfully saved imported ")
}

/// The text after the `HH:MM:SS AM: ` stamp the importers' reporter puts before a line (`wxLog`'s `%X`, so the digits and the AM/PM follow the machine's locale), or the line
/// as it is when it has none.
fn strip_stamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut groups = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        groups += 1;
        if i < bytes.len() && (bytes[i] == b':' || bytes[i] == b'.') && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            i += 1;
        } else {
            break;
        }
    }
    if groups < 2 {
        return line;
    }
    let rest = line[i..].trim_start_matches(' ');
    let rest = ["AM", "PM", "am", "pm"].iter().find_map(|m| rest.strip_prefix(m)).unwrap_or(rest);
    rest.strip_prefix(": ").unwrap_or(line)
}

/// Sort what kicad-cli printed into [`LogLine`]s: an `Error: ` or `Warning: ` after the stamp says what it is, anything else is information. The job's own progress lines
/// and blank lines are left out.
pub fn parse_log(printed: &str) -> Vec<LogLine> {
    printed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_progress(l))
        .map(|l| {
            let body = strip_stamp(l);
            if let Some(t) = body.strip_prefix("Error: ") {
                LogLine { severity: Severity::Error, text: t.trim().to_string() }
            } else if let Some(t) = body.strip_prefix("Warning: ") {
                LogLine { severity: Severity::Warning, text: t.trim().to_string() }
            } else {
                LogLine { severity: Severity::Info, text: body.trim().to_string() }
            }
        })
        .collect()
}

/// What a finished run made.
#[derive(Debug, Clone)]
pub struct ForeignImport {
    /// `kicad-cli 10.99.0`
    pub engine: String,
    pub kind: Kind,
    /// The KiCad file kicad-cli wrote (inside the caller's folder).
    pub output: PathBuf,
    /// Its text.
    pub text: String,
    pub report: ImportReport,
    /// Everything kicad-cli printed, line by line.
    pub log: Vec<LogLine>,
    /// The other files it wrote beside the output (the footprint or symbol library it made, the files of the sheets of a hierarchy, the library table), relative to the output's folder.
    pub files: Vec<String>,
    /// The text of the `.kicad_pro` `kicad-cli import` wrote for the board ([`ForeignOptions::project`]): its design rules and net classes; `None` when it was not asked for or could not be made.
    pub project: Option<String>,
}

impl ForeignImport {
    /// The report's errors and warnings and the importers' error and warning lines, errors first, each message once (`count` says how often). Information lines
    /// ("Setting schematic symbol 'R1 10k' library identifier to ..") are not findings; they stay in [`Self::log`].
    pub fn findings(&self) -> Vec<Finding> {
        let mut all: Vec<(Severity, String)> = Vec::new();
        all.extend(self.report.errors.iter().map(|m| (Severity::Error, m.clone())));
        all.extend(self.report.warnings.iter().map(|m| (Severity::Warning, m.clone())));
        all.extend(self.log.iter().filter(|l| l.severity != Severity::Info).map(|l| (l.severity, l.text.clone())));
        let mut out: Vec<Finding> = Vec::new();
        for (severity, message) in all {
            match out.iter_mut().find(|f| f.severity == severity && f.message == message) {
                Some(f) => f.count += 1,
                None => out.push(Finding { severity, message, count: 1 }),
            }
        }
        out.sort_by_key(|f| f.severity); // stable: the order of saying is kept within a severity
        out
    }

    /// The reply the studio sends the page and `eda import-kicad --report` writes: the format, the counts, the findings and, for a board, where the layers went.
    pub fn to_json(&self) -> Value {
        let findings = self.findings();
        json!({
            "engine": self.engine,
            "kind": self.kind.name(),
            "format": self.report.source_format,
            "source": self.report.source_file,
            "statistics": self.report.statistics,
            "errors": findings.iter().filter(|f| f.severity == Severity::Error).map(|f| f.count).sum::<usize>(),
            "warnings": findings.iter().filter(|f| f.severity == Severity::Warning).map(|f| f.count).sum::<usize>(),
            "findings": findings.iter().map(|f| json!({ "severity": f.severity.name(), "message": f.message, "count": f.count })).collect::<Vec<_>>(),
            "layers": self.report.layers.iter().map(|l| json!({ "source": l.source, "kicad": l.kicad, "explicit": l.explicit })).collect::<Vec<_>>(),
            "files": self.files,
            "has_project": self.project.is_some(),
        })
    }
}

/// The name an uploaded or chosen file is kept under in a run's folder: its last path component, with no separators and nothing hidden. kicad-cli tells many formats apart
/// by the extension (`.PcbDoc`, `.brd`, `.cpa`), so the name is kept as it came.
pub fn upload_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    (!last.is_empty() && !last.starts_with('.') && last != "..").then(|| last.chars().filter(|c| !c.is_control()).collect())
}

/// Write `bytes` into `work` under [`upload_name`] and give the path -- the file a browser handed over, ready for [`import_foreign_pcb`] / [`import_foreign_sch`].
pub fn stage_upload(work: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, Vec<CheckResult>> {
    let Some(name) = upload_name(name) else { return Err(fail("foreign_import_name", name, "the file has no usable name")) };
    let dir = work.join("in");
    work_dir(&dir)?;
    let path = dir.join(name);
    write_file(&path, bytes)?;
    Ok(path)
}

/// `kicad-cli pcb import` of `input`, into a fresh folder under `work`. The board is [`ForeignImport::text`]; read it with [`eda_kicad::import_kicad_pcb`].
pub fn import_foreign_pcb(work: &Path, input: &Path, opts: &ForeignOptions) -> Result<ForeignImport, Vec<CheckResult>> {
    import_foreign(Kind::Board, work, input, opts)
}

/// `kicad-cli sch import` of `input`, into a fresh folder under `work`. The schematic is [`ForeignImport::output`] (read it, with the sheets and the library beside it, by
/// [`eda_kicad::load_sheet_file`]); `opts.layer_map` is for boards and is ignored.
pub fn import_foreign_sch(work: &Path, input: &Path, opts: &ForeignOptions) -> Result<ForeignImport, Vec<CheckResult>> {
    import_foreign(Kind::Schematic, work, input, opts)
}

/// Whether kicad-cli said it knows no importer for the file (as opposed to an importer that failed on it): the caller may try the other kind.
pub fn is_unrecognized(errors: &[CheckResult]) -> bool {
    errors.iter().any(|c| c.check == "kicad_cli_import_format")
}

fn import_foreign(kind: Kind, work: &Path, input: &Path, opts: &ForeignOptions) -> Result<ForeignImport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let what = format!("{} import", kind.scope());
    if !input.is_file() {
        return Err(fail("kicad_cli_import_input", &input.display().to_string(), format!("kicad-cli {what}: there is no file '{}'", input.display())));
    }
    work_dir(work)?;
    // A folder of its own for every run: kicad-cli's library cache is written beside the output as `<name>-import-fps.pretty.tmp` and renamed, and a second run there fails.
    let out_dir = work.join(format!("{}-out", kind.scope()));
    let _ = std::fs::remove_dir_all(&out_dir);
    work_dir(&out_dir)?;
    let stem = file_stem(&input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let output = out_dir.join(format!("{stem}.{}", kind.extension()));
    let report_path = work.join(format!("{}-import-report.json", kind.scope()));
    let _ = std::fs::remove_file(&report_path);

    let mut cmd = cli_command(&cli, None);
    cmd.arg(kind.scope()).arg("import");
    if let Some(format) = opts.format.as_deref().map(str::trim).filter(|f| !f.is_empty() && *f != "auto") {
        // kicad-cli refuses a name it does not know ("Invalid format: bogus"); only plain names are handed over.
        if !format.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(fail("kicad_cli_import_format_name", format, format!("'{format}' is not an import format name (altium, eagle, cadstar, pads, ...)")));
        }
        cmd.args(["--format", format]);
    }
    if kind == Kind::Board && !opts.layer_map.is_empty() {
        let map_path = work.join("layer-map.json");
        let map: serde_json::Map<String, Value> = opts.layer_map.iter().map(|(from, to)| (from.clone(), Value::String(to.clone()))).collect();
        write_file(&map_path, serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default())?;
        cmd.arg("--layer-map").arg(&map_path);
    }
    cmd.args(["--report-format", "json", "--report-file"]).arg(&report_path).arg("-o").arg(&output).arg(input);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    let printed = format!("{stdout}\n{stderr}");
    let log = parse_log(&printed);

    // What kicad-cli says when it fails: the importers' error lines (stderr) and the job's own last word (stdout: "Failed to load board: ..").
    let why = || {
        let errors: Vec<&str> = log.iter().filter(|l| l.severity == Severity::Error).map(|l| l.text.as_str()).collect();
        let last_of = |text: &str| text.lines().map(str::trim).filter(|l| !l.is_empty() && !is_progress(l)).last().map(str::to_string);
        let last = last_of(&stdout).or_else(|| last_of(&stderr)).unwrap_or_else(|| "it printed nothing".to_string());
        let mut parts: Vec<String> = errors.iter().take(3).map(|e| e.to_string()).collect();
        if !parts.iter().any(|p| *p == last) && !strip_stamp(&last).starts_with("Error: ") {
            parts.push(last);
        }
        parts.join(" ")
    };
    if !out.status.success() || !output.is_file() {
        let said = printed.trim();
        // A kicad-cli with no `import` command (before the 10.99 nightlies) answers with its usage text.
        if said.contains("Failed to parse 'import'") || said.contains("Unknown argument") || said.contains("did you mean") {
            return Err(fail("kicad_cli_import_missing", "kicad-cli", format!("this kicad-cli ({}) has no `{} import` command (it came with the 10.99 nightlies): {}", cli_version(&cli), kind.scope(), said.lines().next().unwrap_or(""))));
        }
        if said.contains("importer recognizes the file format") {
            return Err(fail("kicad_cli_import_format", &input.display().to_string(), format!("kicad-cli {what}: {}", why())));
        }
        return Err(fail("kicad_cli_import", &what, format!("kicad-cli {what} failed on {}: {}", input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), why())));
    }
    let text = std::fs::read_to_string(&output).map_err(|e| fail("kicad_cli_import", &output.display().to_string(), format!("kicad-cli {what} wrote a file that cannot be read: {e}")))?;
    // The report is written when the job succeeds; one that is missing or unreadable only leaves the counts and the layers out.
    let report = std::fs::read_to_string(&report_path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| ImportReport::from_json(&v)).unwrap_or_default();
    let mut files: Vec<String> = Vec::new();
    collect_files(&out_dir, &out_dir, &mut files);
    files.retain(|f| Path::new(f) != output.strip_prefix(&out_dir).unwrap_or(&output));
    files.sort();
    let mut log = log;
    let project = if kind == Kind::Board && opts.project { make_project(&cli, work, input, opts, &mut log) } else { None };
    Ok(ForeignImport { engine: format!("kicad-cli {}", cli_version(&cli)), kind, output, text, report, log, files, project })
}

/// The project kicad-cli makes of a board (`kicad-cli import -o <stem> [--layer-map F] INPUT`: "Import non-KiCad board and/or schematic files into a new KiCad project"): the importer runs
/// again, and the `.kicad_pro` it writes beside the board has what the board file does not -- the design rules and the net classes the importer set in memory (`OpenProjectFiles` saves the
/// project of a converted board for that reason). It does not pair files (given the `.brd` alone it imports the board alone), refuses a second schematic ("a board and a schematic
/// have already been imported for this project") and writes no JSON report, which is why the board and its report come from `pcb import` and only this file from here. A run that
/// fails (a kicad-cli without the command) leaves the board without its rules and says so in the log; it is not an error.
fn make_project(cli: &Path, work: &Path, input: &Path, opts: &ForeignOptions, log: &mut Vec<LogLine>) -> Option<String> {
    let dir = work.join("project");
    let _ = std::fs::remove_dir_all(&dir);
    work_dir(&dir).ok()?;
    // The stem has no dot in it: kicad-cli takes what follows the last one for an extension and puts its own in its place (a board called `HiFive1.B01` would make `HiFive1.kicad_pro`).
    let stem = "board";
    let mut cmd = cli_command(cli, None);
    cmd.arg("import").arg("-o").arg(dir.join(stem));
    if !opts.layer_map.is_empty() {
        cmd.arg("--layer-map").arg(work.join("layer-map.json"));
    }
    cmd.arg(input);
    let note = |log: &mut Vec<LogLine>, why: String| log.push(LogLine { severity: Severity::Info, text: format!("the project kicad-cli makes of the board (its design rules and net classes) is not there: {why}") });
    let out = match output_within(cmd, limit(EXPORT_TIMEOUT)) {
        Ok(o) => o,
        Err(e) => {
            note(log, e.first().and_then(|c| c.hint.clone()).unwrap_or_default());
            return None;
        }
    };
    match std::fs::read_to_string(dir.join(format!("{stem}.kicad_pro"))) {
        Ok(text) => Some(text),
        Err(_) => {
            let said = String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()).map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("kicad-cli wrote none").to_string();
            note(log, said);
            None
        }
    }
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_files(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

/// The formats kicad-cli names for `--format`, board and schematic: for a person's choice list, not for validation (a newer kicad-cli knows more, and says so).
pub const BOARD_FORMATS: &[&str] = &["auto", "pads", "altium", "eagle", "cadstar", "fabmaster", "pcad", "solidworks", "easypc"];
pub const SCHEMATIC_FORMATS: &[&str] = &["auto", "altium", "eagle", "cadstar", "easyeda", "easyedapro", "ltspice", "pads", "diptrace", "pcad", "orcad", "easypc"];

/// Whether the importer of this report's `source_format` asks which KiCad layer each source layer goes to (a `LAYER_MAPPABLE_PLUGIN` of `pcbnew/pcb_io`: Altium, Eagle, PADS,
/// CADSTAR, Allegro and SolidWorks do, `PCB_IO_MGR`'s others read the layers they know and have no dialog).
pub fn maps_layers(source_format: &str) -> bool {
    let f = source_format.to_ascii_lowercase();
    ["altium", "eagle", "pads", "cadstar", "allegro", "solidworks"].iter().any(|n| f.contains(n))
}
