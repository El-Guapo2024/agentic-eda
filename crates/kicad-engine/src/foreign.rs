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
    /// Source layers of a board to leave out, with what is on them: the layers the person left in the Unmatched list of `DIALOG_MAP_LAYERS`, which answers the importer
    /// with `UNDEFINED_LAYER` for them. kicad-cli's map cannot say that, so they are mapped to a user layer nothing else goes to ([`scratch_layer`]) and that layer is taken
    /// out of the board kicad-cli wrote (`eda_kicad::drop_board_layer`).
    pub drop_layers: Vec<String>,
    /// The other files of a project imported with the board (an Eagle `.sch` beside its `.brd`, a CADSTAR `.csa` beside its `.cpa`): the project run is given them too
    /// (`kicad-cli import -o STEM BOARD SCHEMATIC`, as KiCad's "Import Non-KiCad Project" imports a project's files together), which converts the schematic and gives the
    /// board the schematic's net names (`ApplyImportedNetNameMap`): see [`ForeignImport::project_board`] and [`ForeignImport::project_schematic`]. Implies `project`.
    pub partners: Vec<PathBuf>,
    /// Which board of a project file that holds several (an EasyEDA Pro project): an index into [`ForeignImport::boards`], the project's own order (by the boards' ids).
    /// `pcb import` cannot choose (KiCad's importer asks the person through `DIALOG_IMPORT_CHOOSE_PROJECT`, a callback kicad-cli does not answer: `bad_function_call`), so a
    /// file of several boards goes through the project command, which converts every board ([`ForeignImport::boards`] names them).
    pub board: Option<usize>,
}

/// The user layer the layers to leave out are sent to: the highest `User.N` of KiCad 10's 45 that neither the map nor the importer's guesses (`taken`) send a layer to.
pub fn scratch_layer<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: std::collections::BTreeSet<&str> = taken.into_iter().collect();
    (1..=45).rev().map(|n| format!("User.{n}")).find(|l| !taken.contains(l.as_str())).unwrap_or_else(|| "User.45".to_string())
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
    /// The custom DRC rules the importer wrote (`.kicad_dru`), see [`imported_rules`]; `None` when it wrote none.
    pub rules: Option<String>,
    /// The board the project run made with [`ForeignOptions::partners`] (the schematic's net names on it), or the board of [`ForeignOptions::board`]; `None` otherwise.
    pub project_board: Option<String>,
    /// The schematic the project run made of the files it was given (`<stem>.kicad_sch`, its sheets beside it); `None` when it made none.
    pub project_schematic: Option<PathBuf>,
    /// The boards of a project file that holds several, in the project's order (each by the name kicad-cli gave its file, the board's title): when there is more than one and
    /// no [`ForeignOptions::board`] was asked for, the import is not done ([`Self::text`] is empty) and the person is asked which (`DIALOG_IMPORT_CHOOSE_PROJECT`).
    pub boards: Vec<String>,
}

impl ForeignImport {
    /// A project file of several boards was given with no board chosen: the person is to be asked which ([`Self::boards`]).
    pub fn needs_board_choice(&self) -> bool {
        self.boards.len() > 1 && self.text.is_empty()
    }
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
            "has_rules": self.rules.is_some(),
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
    // A board of a project file of several, chosen: the project command converts them all (`pcb import` cannot choose).
    if kind == Kind::Board && opts.board.is_some() {
        return import_project_board(&cli, work, input, opts, Vec::new());
    }
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
    // The layers to leave out go to a user layer nothing else goes to, which is taken out of the board afterwards (`ForeignOptions::drop_layers`).
    let dropping = kind == Kind::Board && !opts.drop_layers.is_empty();
    let scratch = scratch_layer(opts.layer_map.values().map(String::as_str));
    let mut layer_map = opts.layer_map.clone();
    if dropping {
        for l in &opts.drop_layers {
            layer_map.insert(l.clone(), scratch.clone());
        }
    }
    if kind == Kind::Board && !layer_map.is_empty() {
        let map_path = work.join("layer-map.json");
        let map: serde_json::Map<String, Value> = layer_map.iter().map(|(from, to)| (from.clone(), Value::String(to.clone()))).collect();
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
        // `PCB_IO_EASYEDAPRO::LoadBoard` of a project of several boards calls the chooser callback (`m_choose_project_handler`), which kicad-cli does not set: the project
        // command converts every board instead, and the person chooses.
        if kind == Kind::Board && said.contains("bad_function_call") {
            return import_project_board(&cli, work, input, opts, log);
        }
        // A kicad-cli with no `import` command (before the 10.99 nightlies) answers with its usage text.
        if said.contains("Failed to parse 'import'") || said.contains("Unknown argument") || said.contains("did you mean") {
            return Err(fail("kicad_cli_import_missing", "kicad-cli", format!("this kicad-cli ({}) has no `{} import` command (it came with the 10.99 nightlies): {}", cli_version(&cli), kind.scope(), said.lines().next().unwrap_or(""))));
        }
        if said.contains("importer recognizes the file format") {
            return Err(fail("kicad_cli_import_format", &input.display().to_string(), format!("kicad-cli {what}: {}", why())));
        }
        return Err(fail("kicad_cli_import", &what, format!("kicad-cli {what} failed on {}: {}", input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), why())));
    }
    let mut text = std::fs::read_to_string(&output).map_err(|e| fail("kicad_cli_import", &output.display().to_string(), format!("kicad-cli {what} wrote a file that cannot be read: {e}")))?;
    // The report is written when the job succeeds; one that is missing or unreadable only leaves the counts and the layers out.
    let mut report = std::fs::read_to_string(&report_path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| ImportReport::from_json(&v)).unwrap_or_default();
    let mut log = log;
    if dropping {
        // A layer the importer guessed onto the scratch layer itself (one of its own user layers) would go too: say so, it is not what was asked.
        for l in report.layers.iter().filter(|l| l.kicad.as_deref() == Some(scratch.as_str()) && !opts.drop_layers.contains(&l.source)) {
            log.push(LogLine { severity: Severity::Warning, text: format!("Layer '{}' went to {scratch}, which the layers left out went to: what was on it is left out too", l.source) });
        }
        if let Some((stripped, _)) = eda_kicad::drop_board_layer(&text, &scratch) {
            text = stripped;
            write_file(&output, &text)?;
        }
        // the layers left out are `UNDEFINED` in the report, as the dialog's callback would have made them; one kicad-cli did not send to the scratch layer (not a target the
        // importer permits it) kept its place, and kicad-cli's report already warns of it
        for l in report.layers.iter_mut().filter(|l| opts.drop_layers.contains(&l.source) && l.kicad.as_deref() == Some(scratch.as_str())) {
            l.kicad = None;
            l.explicit = true;
        }
    }
    let mut files: Vec<String> = Vec::new();
    collect_files(&out_dir, &out_dir, &mut files);
    files.retain(|f| Path::new(f) != output.strip_prefix(&out_dir).unwrap_or(&output));
    files.sort();
    let run = if kind == Kind::Board && (opts.project || !opts.partners.is_empty()) { make_project(&cli, work, input, opts, &mut log) } else { None };
    let rules = if kind == Kind::Board { imported_rules(&output, &work.join("project").join("board.kicad_pro"), input) } else { None };
    let (project, mut project_board, project_schematic) = match run {
        Some(r) => (r.project, (!opts.partners.is_empty()).then(|| r.boards.first().and_then(|b| std::fs::read_to_string(r.dir.join(b)).ok())).flatten(), r.schematic),
        None => (None, None, None),
    };
    if dropping {
        project_board = project_board.map(|t| eda_kicad::drop_board_layer(&t, &scratch).map_or(t, |(s, _)| s));
    }
    Ok(ForeignImport { engine: format!("kicad-cli {}", cli_version(&cli)), kind, output, text, report, log, files, project, rules, project_board, project_schematic, boards: Vec::new() })
}

/// What the project command (`kicad-cli import -o <dir>/board INPUT..`) made: the project, its boards (the project's `boards`, in its order) and its schematic.
struct ProjectRun {
    dir: PathBuf,
    project: Option<String>,
    boards: Vec<String>,
    schematic: Option<PathBuf>,
    printed: String,
}

/// The format kicad-cli named as it imported (`Importing '..' using EasyEDA / JLCEDA Pro format...`, `JOB_PCB_IMPORT`'s line), for a run with no JSON report.
fn format_said(printed: &str) -> String {
    printed.lines().find_map(|l| l.split_once(" using ").and_then(|(_, f)| f.trim().strip_suffix(" format...")).map(str::to_string)).unwrap_or_default()
}

/// A board of a project file that holds several (`ForeignOptions::board`; `boards` lists them): the project command converts every board of the file, and the one chosen is the
/// import. With none chosen and more than one there, nothing is imported and the boards are named for the person to choose ([`ForeignImport::needs_board_choice`]).
fn import_project_board(cli: &Path, work: &Path, input: &Path, opts: &ForeignOptions, mut log: Vec<LogLine>) -> Result<ForeignImport, Vec<CheckResult>> {
    let what = "pcb import";
    let run = match run_project(cli, work, &[input.to_path_buf()], opts) {
        Ok(r) => r,
        Err(said) => {
            let errors: Vec<String> = log.iter().filter(|l| l.severity == Severity::Error).map(|l| l.text.clone()).chain(std::iter::once(said)).collect();
            return Err(fail("kicad_cli_import", what, format!("kicad-cli {what} failed on {}: {}", input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), errors.join(" "))));
        }
    };
    log = parse_log(&run.printed);
    let names: Vec<String> = run.boards.iter().map(|b| b.trim_end_matches(".kicad_pcb").to_string()).collect();
    let report = ImportReport { source_file: input.display().to_string(), source_format: format_said(&run.printed), ..Default::default() };
    let engine = format!("kicad-cli {}", cli_version(cli));
    let chosen = match opts.board {
        Some(i) => Some(i),
        None if run.boards.len() == 1 => Some(0),
        None => None,
    };
    let Some(i) = chosen else {
        return Ok(ForeignImport { engine, kind: Kind::Board, output: run.dir.clone(), text: String::new(), report, log, files: run.boards.clone(), project: run.project, rules: None, project_board: None, project_schematic: run.schematic, boards: names });
    };
    let Some(file) = run.boards.get(i) else {
        return Err(fail("kicad_cli_import_board", what, format!("the file holds {} board(s): there is no board {}", run.boards.len(), i + 1)));
    };
    let output = run.dir.join(file);
    let text = std::fs::read_to_string(&output).map_err(|e| fail("kicad_cli_import", &output.display().to_string(), format!("kicad-cli import wrote a file that cannot be read: {e}")))?;
    let rules = imported_rules(&output, &run.dir.join("board.kicad_pro"), input);
    Ok(ForeignImport { engine, kind: Kind::Board, output, text: text.clone(), report, log, files: Vec::new(), project: run.project, rules, project_board: Some(text), project_schematic: run.schematic, boards: names })
}

/// The project command on `inputs` (`kicad-cli import -o <work>/project/board [--layer-map F] INPUT..`): "Import non-KiCad board and/or schematic files into a new KiCad
/// project". Each input is a board or a schematic (kicad-cli tells), at most one of each; a board's net names are the schematic's (`ApplyImportedNetNameMap`); a project file
/// of several boards gives every board, each named by its title. An error, with what kicad-cli said last, when it made nothing (a kicad-cli without the command, a file it
/// refuses).
fn run_project(cli: &Path, work: &Path, inputs: &[PathBuf], opts: &ForeignOptions) -> Result<ProjectRun, String> {
    let dir = work.join("project");
    let _ = std::fs::remove_dir_all(&dir);
    work_dir(&dir).map_err(|e| e.first().and_then(|c| c.hint.clone()).unwrap_or_default())?;
    // The stem has no dot in it: kicad-cli takes what follows the last one for an extension and puts its own in its place (a board called `HiFive1.B01` would make `HiFive1.kicad_pro`).
    let stem = "board";
    let mut cmd = cli_command(cli, None);
    cmd.arg("import").arg("-o").arg(dir.join(stem));
    let map_file = work.join("layer-map.json");
    if (!opts.layer_map.is_empty() || !opts.drop_layers.is_empty()) && map_file.is_file() {
        cmd.arg("--layer-map").arg(&map_file);
    }
    cmd.args(inputs);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT)).map_err(|e| e.first().and_then(|c| c.hint.clone()).unwrap_or_default())?;
    let printed = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let project = std::fs::read_to_string(dir.join(format!("{stem}.kicad_pro"))).ok();
    // `PROJECT_FILE::GetBoards()`: `[[uuid, "file.kicad_pcb"], ..]`
    let mut boards: Vec<String> = project
        .as_deref()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|v| v.get("boards").and_then(Value::as_array).cloned())
        .map(|a| a.iter().filter_map(|b| b.get(1).and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default();
    if boards.is_empty() && dir.join(format!("{stem}.kicad_pcb")).is_file() {
        boards.push(format!("{stem}.kicad_pcb"));
    }
    boards.retain(|b| dir.join(b).is_file());
    let schematic = Some(dir.join(format!("{stem}.kicad_sch"))).filter(|p| p.is_file());
    if project.is_none() && boards.is_empty() && schematic.is_none() {
        return Err(printed.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("kicad-cli wrote none").to_string());
    }
    Ok(ProjectRun { dir, project, boards, schematic, printed })
}

/// The project kicad-cli makes of a board (`kicad-cli import -o <stem> [--layer-map F] INPUT`: "Import non-KiCad board and/or schematic files into a new KiCad project"): the importer runs
/// again, and the `.kicad_pro` it writes beside the board has what the board file does not -- the design rules and the net classes the importer set in memory (`OpenProjectFiles` saves the
/// project of a converted board for that reason). It does not pair files (given the `.brd` alone it imports the board alone), refuses a second schematic ("a board and a schematic
/// have already been imported for this project") and writes no JSON report, which is why the board and its report come from `pcb import` and only this file from here. A run that
/// fails (a kicad-cli without the command) leaves the board without its rules and says so in the log; it is not an error. With [`ForeignOptions::partners`] the run is given the
/// project's other files too (the schematic of an Eagle board), and its board -- with the schematic's net names -- and its schematic are the import's.
fn make_project(cli: &Path, work: &Path, input: &Path, opts: &ForeignOptions, log: &mut Vec<LogLine>) -> Option<ProjectRun> {
    let note = |log: &mut Vec<LogLine>, why: String| log.push(LogLine { severity: Severity::Info, text: format!("the project kicad-cli makes of the board (its design rules and net classes) is not there: {why}") });
    let inputs: Vec<PathBuf> = std::iter::once(input.to_path_buf()).chain(opts.partners.iter().cloned()).collect();
    let run = match run_project(cli, work, &inputs, opts) {
        Ok(r) => r,
        Err(said) => {
            note(log, said);
            return None;
        }
    };
    if run.project.is_none() {
        let said = run.printed.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("kicad-cli wrote none").to_string();
        note(log, said);
    }
    // what the importers said of the partner files (the schematic's) is the import's too
    if !opts.partners.is_empty() {
        log.extend(parse_log(&run.printed).into_iter().filter(|l| l.severity != Severity::Info));
    }
    Some(run)
}

/// The custom DRC rules an importer wrote, which the board file does not carry: `pcbnew_jobs_handler.cpp::JobImport` writes `GetImportedDesignRules()` (EasyEDA
/// Pro, Easy-PC) beside the board it saved (`rulesPath( outputPath ); rulesPath.SetExt( FILEEXT::DesignRulesFileExtension )`), the project run beside its project, and
/// the Eagle and PADS importers beside the file they read (`pcb_io_eagle.cpp::LoadBoard`: `fn.SetExt( wxT( "kicad_dru" ) ); .. rulesFile.Write( m_customRules )`, the
/// class-to-class clearances; `PADS_PCB_CONVERTER::generateDrcRules`, the diff-pair gaps), where the GUI's project, named after the file, finds them
/// (`OpenProjectFiles`, `GetDesignRulesPath`). The first of those that has a rule in it; `None` when none does.
pub fn imported_rules(output: &Path, project: &Path, input: &Path) -> Option<String> {
    [output.with_extension("kicad_dru"), project.with_extension("kicad_dru"), input.with_extension("kicad_dru")]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find(|text| text.contains("(rule "))
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

// ------------------------------------------------------------------------------------------------ libraries

/// Which kind of library [`convert_library`] converts: kicad-cli's `sym upgrade` or `fp upgrade`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    Symbol,
    Footprint,
}

impl LibraryKind {
    /// The kicad-cli command group: `sym` or `fp`.
    pub fn scope(self) -> &'static str {
        match self {
            LibraryKind::Symbol => "sym",
            LibraryKind::Footprint => "fp",
        }
    }
}

/// A library of another format (or KiCad's legacy one) converted to KiCad's: `kicad-cli sym upgrade INPUT -o OUTPUT` (a `.kicad_sym` file) or `kicad-cli fp upgrade
/// INPUT -o OUTPUT` (a `.pretty` folder). That is `EESCHEMA_JOBS_HANDLER::JobSymUpgrade` / `PCBNEW_JOBS_HANDLER::JobExportFpUpgrade`, which for a library that is not
/// KiCad's calls `SCH_IO_MGR::ConvertLibrary` / `PCB_IO_MGR::ConvertLibrary` -- the very functions the library tables' Migrate Libraries calls
/// (`PANEL_SYM_LIB_TABLE::onConvertLegacyLibraries`, `PANEL_FP_LIB_TABLE::onMigrateLibraries`): every symbol or footprint the plugin enumerates (`EnumerateSymbolLib`,
/// `FootprintEnumerate`), loaded and saved by KiCad's own writer. The plugin is the one the file's contents say (`GuessPluginTypeFromLibPath`), not a row's type.
///
/// kicad-cli refuses an output that exists ("Output path must not conflict with existing path"), so `output` must not exist; the caller converts into a fresh path and
/// moves the result. The row's options are not passed (kicad-cli has no flag for them; `ConvertLibrary` takes them as `aOldFileProps`): a CADSTAR library that needs its
/// `csa` option is converted without it. Returns kicad-cli's own words on failure ("Unrecognized library type", "Unable to convert library").
/// Whether KiCad's own parser loads the library at `input` whole (UI-LEFT-3 L3-6): `kicad-cli <sym|fp> upgrade INPUT -o <scratch>` loads every item of it
/// through the library's plugin (`SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`, `FP_CACHE::Load`) and answers "Unable to load library" when any file does not parse
/// (kicad-cli says no more: the parser's reason is not printed). `None` when there is no kicad-cli or it did not run. What it writes goes into a scratch
/// folder `scratch` (removed after).
pub fn library_loads(kind: LibraryKind, input: &Path, scratch: &Path) -> Option<bool> {
    let cli = super::find_cli()?;
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).ok()?;
    let output = scratch.join(match kind {
        LibraryKind::Symbol if input.is_dir() => "out.kicad_symdir",
        LibraryKind::Symbol => "out.kicad_sym",
        LibraryKind::Footprint => "out.pretty",
    });
    let mut cmd = cli_command(&cli, None);
    cmd.arg(kind.scope()).arg("upgrade").arg("--force").arg("-o").arg(&output).arg(input);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT)).ok();
    let _ = std::fs::remove_dir_all(scratch);
    let out = out?;
    let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Some(out.status.success() && !printed.contains("Unable to load library"))
}

pub fn convert_library(kind: LibraryKind, input: &Path, output: &Path) -> Result<(), Vec<CheckResult>> {
    let cli = need_cli()?;
    let what = format!("{} upgrade", kind.scope());
    if !input.exists() {
        return Err(fail("kicad_cli_library_input", &input.display().to_string(), format!("Library '{}' not found.", input.display())));
    }
    if output.exists() {
        return Err(fail("kicad_cli_library_output", &output.display().to_string(), "Output path must not conflict with existing path"));
    }
    let mut cmd = cli_command(&cli, None);
    cmd.arg(kind.scope()).arg("upgrade").arg("-o").arg(output).arg(input);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let made = match kind {
        LibraryKind::Symbol => output.is_file(),
        LibraryKind::Footprint => output.is_dir(),
    };
    if out.status.success() && made {
        return Ok(());
    }
    let printed = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let said: Vec<&str> = printed.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("Loading ")).collect();
    let said = if said.is_empty() { "kicad-cli wrote nothing".to_string() } else { said.join(" ") };
    Err(fail("kicad_cli_library_convert", &input.display().to_string(), format!("kicad-cli {what} could not convert {}: {said}", input.display())))
}
