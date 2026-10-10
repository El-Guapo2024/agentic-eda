//! kicad-cli as the batch engine (docs/ARCHITECTURE.md, "Engines").
//!
//! `design.json` is the master; this crate exports the current revision of
//! it to derived KiCad files in a scratch directory, runs kicad-cli on
//! them, and points the report back at our own item ids through the
//! exporter's uuid map (`eda_kicad::export_kicad_pcb_mapped`). The derived
//! files are scratch output, rewritten on every run -- never edited, never
//! read back as a design.
//!
//! There is one answer per question: DRC, ERC, plots, Gerbers, drill,
//! position files, STEP, netlist and BOM all come from kicad-cli. Nothing
//! in this workspace re-implements any of them.
//!
//! The scratch directory is the caller's choice. The studio and the CLI
//! pass `<board dir>/.kicad` (kept for inspection); the gates, which work
//! on in-memory designs, use [`drc_scratch`] (a fresh temp directory,
//! removed afterwards).

use eda_kicad::{export_kicad_pcb_mapped, export_kicad_pro_for, export_kicad_sch_tree_mapped, ExportMeta};
use eda_model::ir::{Design, DrcExclusion};
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

fn fail(check: &str, location: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, msg.into())]
}

// ------------------------------------------------------------------ the binary

/// `EDA_KICAD_CLI`, else `kicad-cli` on `PATH`, else the macOS bundle.
pub fn find_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn need_cli() -> Result<PathBuf, Vec<CheckResult>> {
    find_cli().ok_or_else(|| fail("kicad_cli_missing", "kicad-cli", "kicad-cli not found (set EDA_KICAD_CLI, or install KiCad nightly with tools/install-kicad-nightly.sh)"))
}

// ------------------------------------------------------------- the time limit

/// How long a `kicad-cli pcb drc` or `sch erc` may run before it is killed. A
/// DRC takes about 4 s on a 30-part board and an ERC about 2.5 s, so a run that
/// is still going after two minutes is stuck, not slow. Without a limit it
/// would hold the studio's one kicad-cli lane (`crates/cli/src/kicad_lane.rs`),
/// and every DRC, ERC and export queued behind it, for good.
pub const REPORT_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a `kicad-cli ... export` (Gerbers, drill, positions, plots, STEP,
/// statistics, ...) may run before it is killed. A STEP export of a board with
/// real 3D models is the slow one (OpenCascade loads every model), hence more
/// than a DRC.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(300);

/// `kicad-cli version` answers at once; one that does not is not going to.
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);

/// Seconds (fractions allowed) that replace [`REPORT_TIMEOUT`], [`EXPORT_TIMEOUT`]
/// and the version probe's limit: a slow machine with a big board asking for
/// more time, or a test with a fake kicad-cli that hangs asking for less.
pub const TIMEOUT_ENV: &str = "EDA_KICAD_TIMEOUT_SECS";

fn limit(default: Duration) -> Duration {
    std::env::var(TIMEOUT_ENV).ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| s.is_finite() && *s > 0.0).map(Duration::from_secs_f64).unwrap_or(default)
}

/// The subcommand a command runs, for a message: `pcb export gerbers`.
fn describe(cmd: &Command) -> String {
    cmd.get_args().map(|a| a.to_string_lossy().into_owned()).take_while(|a| !a.starts_with('-')).collect::<Vec<_>>().join(" ")
}

/// Signal kicad-cli's whole process group (it is spawned as the leader of its
/// own, below), not just its pid: whatever it started -- a helper, a `sleep` in
/// a stand-in script -- would otherwise outlive it, still holding the pipes and
/// the scratch files. Shells out to `kill` rather than adding a libc
/// dependency for one syscall; `Child::kill` follows for the pid itself.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg("-9").arg("--").arg(format!("-{}", child.id())).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}

/// Read a child's pipe to the end on a thread of its own: a chatty kicad-cli
/// that fills an undrained pipe blocks on its own write, which looks exactly
/// like a hang. The bytes arrive on the channel when the pipe closes.
fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = tx.send(bytes);
    });
    rx
}

/// `cmd.output()` with a clock on it: a run still going after `limit` is
/// killed (its whole process group) and answers with a `kicad_cli_timeout`
/// failure naming the command and the limit, so the caller, and the lane behind
/// it, go on instead of waiting for good.
fn output_within(mut cmd: Command, limit: Duration) -> Result<Output, Vec<CheckResult>> {
    let what = describe(&cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| fail("kicad_cli_run", "kicad-cli", e.to_string()))?;
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + limit;
    let exited: Option<ExitStatus> = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                kill_tree(&mut child);
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(fail("kicad_cli_run", "kicad-cli", format!("waiting for kicad-cli {what}: {e}")));
            }
        }
    };
    // The pipes close with the process; a grandchild that kept one open must not hold this up.
    let bytes = |rx: std::sync::mpsc::Receiver<Vec<u8>>| rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let (stdout, stderr) = (bytes(stdout), bytes(stderr));
    match exited {
        Some(status) => Ok(Output { status, stdout, stderr }),
        None => {
            let said: String = String::from_utf8_lossy(&stderr).trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            Err(fail(
                "kicad_cli_timeout",
                "kicad-cli",
                format!(
                    "kicad-cli {what} did not finish within {} s and was killed (a stuck run; set {TIMEOUT_ENV} to give a slow one more time){}",
                    limit.as_secs_f64(),
                    if said.is_empty() { String::new() } else { format!(": {said}") }
                ),
            ))
        }
    }
}

fn per_cli<T: Clone>(cache: &'static OnceLock<Mutex<HashMap<PathBuf, T>>>, cli: &Path, compute: impl FnOnce() -> T) -> T {
    let m = cache.get_or_init(Default::default);
    if let Some(v) = m.lock().ok().and_then(|g| g.get(cli).cloned()) {
        return v;
    }
    let v = compute();
    if let Ok(mut g) = m.lock() {
        g.insert(cli.to_path_buf(), v.clone());
    }
    v
}

/// `kicad-cli version`, e.g. "10.99.0". One process spawn per binary per
/// run of this program.
pub fn cli_version(cli: &Path) -> String {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    per_cli(&CACHE, cli, || {
        let mut cmd = Command::new(cli);
        cmd.arg("version");
        output_within(cmd, limit(VERSION_TIMEOUT)).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    })
}

// ----------------------------------------------------------- the derived files

fn work_dir(work: &Path) -> Result<(), Vec<CheckResult>> {
    std::fs::create_dir_all(work).map_err(|e| fail("kicad_engine_dir", &work.display().to_string(), format!("cannot create {}: {e}", work.display())))
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), Vec<CheckResult>> {
    std::fs::write(path, bytes).map_err(|e| fail("kicad_engine_write", &path.display().to_string(), e.to_string()))
}

/// Today's date as `YYYY-MM-DD` from the system clock (civil-from-days, no
/// extra crate) -- the date stamped into every derived KiCad file.
pub fn today() -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// The studio's Appearance settings for the board whose derived files are written into `work`: `appearance.json` beside `design.json`, found only when `work` is that
/// board's `.kicad` folder (the scratch folders of the gates and of the tests belong to no board and get none).
fn studio_appearance(work: &Path) -> Option<Value> {
    if work.file_name().and_then(|n| n.to_str()) != Some(".kicad") {
        return None;
    }
    let text = std::fs::read_to_string(work.parent()?.join("appearance.json")).ok()?;
    serde_json::from_str(&text).ok().filter(Value::is_object)
}

/// The project file next to the derived board/schematic: our design rules
/// and ERC pin map, plus the custom-rule file an imported project carried,
/// so kicad-cli judges against this design's own rules and not its
/// hard-coded floors.
fn write_project(work: &Path, stem: &str, design: &Design, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
    write_project_with(work, stem, design, model, None)
}

/// [`write_project`], with the board's net classes in the project file when [`export_board`] moved them there (a class with a colour: see `eda_kicad::appearance`).
fn write_project_with(work: &Path, stem: &str, design: &Design, model: &ConstraintModel, classes: Option<&eda_kicad::appearance::SplitClasses>) -> Result<(), Vec<CheckResult>> {
    let mut pro_text = export_kicad_pro_for(design, model);
    // What the person chose in the studio's Appearance panel (layers and objects shown, colours, presets, views) goes into the derived project as KiCad keeps it:
    // the net colours, layer presets and viewports in the project file, the rest in the project's local settings (`<stem>.kicad_prl`).
    let appearance = studio_appearance(work);
    if appearance.is_some() || classes.is_some() {
        if let Ok(mut pro) = serde_json::from_str::<Value>(&pro_text) {
            let before = pro.clone();
            if let Some(a) = &appearance {
                eda_kicad::appearance::merge_into_project(&mut pro, a);
            }
            if let Some(split) = classes {
                eda_kicad::appearance::put_classes(&mut pro, split);
            }
            if pro != before {
                pro_text = format!("{}\n", serde_json::to_string_pretty(&pro).unwrap_or_default());
            }
        }
    }
    write_file(&work.join(format!("{stem}.kicad_pro")), pro_text)?;
    let prl = work.join(format!("{stem}.kicad_prl"));
    match appearance.as_ref().and_then(|a| eda_kicad::appearance::local_settings(a, stem)) {
        Some(local) => write_file(&prl, format!("{}\n", serde_json::to_string_pretty(&local).unwrap_or_default()))?,
        None => {
            let _ = std::fs::remove_file(&prl);
        }
    }
    let dru = work.join(format!("{stem}.kicad_dru"));
    match model.board.custom_rules_text.as_deref() {
        Some(text) => write_file(&dru, text)?,
        None => {
            let _ = std::fs::remove_file(&dru);
        }
    }
    Ok(())
}

/// A file stem for the derived files: the project's own name, so the plots,
/// Gerbers and position files kicad-cli writes carry it (`<stem>-F_Cu.gtl`);
/// anything that is not a plain file-name character becomes `_`.
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    if s.is_empty() || s.starts_with('.') { "board".to_string() } else { s }
}

/// Export the design as `<stem>.kicad_pcb` (+ project); returns the pcb path
/// and the uuid -> our id map.
fn export_board(design: &Design, model: &ConstraintModel, work: &Path, stem: &str) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    work_dir(work)?;
    let date = today();
    let (pcb, map) = export_kicad_pcb_mapped(design, model, &ExportMeta { date: &date, title: stem })?;
    // A net class with a colour (Appearance > Net Classes) can only reach KiCad in the project file, and KiCad reads the project's classes only when the board has
    // none of its own: so then the classes move there whole, and kicad-cli judges the board by the same numbers (`eda_kicad::appearance::split_net_classes`).
    let class_colors = studio_appearance(work).map(|a| eda_kicad::appearance::class_colors(&a)).unwrap_or_default();
    let split = if class_colors.is_empty() { None } else { eda_kicad::appearance::split_net_classes(&pcb, &class_colors) };
    let path = work.join(format!("{stem}.kicad_pcb"));
    write_file(&path, split.as_ref().map_or(pcb.as_str(), |s| s.pcb.as_str()))?;
    write_project_with(work, stem, design, model, split.as_ref())?;
    Ok((path, map))
}

/// Export the design's schematic as `<stem>.kicad_sch` (+ project), and one file beside it for every sheet of a hierarchical
/// design (`Sheetfile` names are relative to the root's folder), so kicad-cli reads the whole tree.
fn export_schematic(design: &Design, model: &ConstraintModel, work: &Path, stem: &str) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    work_dir(work)?;
    let date = today();
    let root_name = format!("{stem}.kicad_sch");
    let (files, map) = export_kicad_sch_tree_mapped(design, model, &ExportMeta { date: &date, title: stem }, &root_name)?;
    // Sheet files of an earlier export that this design no longer has would still be read if a sheet named them; clear the folder's.
    if let Ok(entries) = std::fs::read_dir(work) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "kicad_sch") && p.file_name().and_then(|n| n.to_str()) != Some(root_name.as_str()) && !files.iter().any(|(n, _)| Some(n.as_str()) == p.file_name().and_then(|n| n.to_str())) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    for (name, text) in &files {
        write_file(&work.join(name), text)?;
    }
    write_project(work, stem, design, model)?;
    Ok((work.join(&root_name), map))
}

/// Run `f` with a fresh temp directory, removed afterwards.
pub fn with_scratch<T>(f: impl FnOnce(&Path) -> T) -> T {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("eda-kicad-{}-{}-{nanos}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let r = f(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

// ------------------------------------------------------------ 3D models as VRML

/// A KiCad board with one footprint per model (side by side, 30 mm apart, each carrying nothing but a `(model ...)`), the smallest board `kicad-cli pcb export
/// vrml` will read the models of. `models` are the paths of the model files as KiCad's 3D cache is to open them.
pub fn models_board_text(models: &[PathBuf]) -> String {
    let quote = |p: &Path| format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""));
    let mut pcb = String::from(
        "(kicad_pcb\n\t(version 20241229)\n\t(generator \"eda-kicad\")\n\t(generator_version \"9.0\")\n\t(general\n\t\t(thickness 1.6)\n\t\t(legacy_teardrops no)\n\t)\n\t(paper \"A4\")\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(31 \"B.Cu\" signal)\n\t\t(37 \"F.SilkS\" user)\n\t\t(39 \"F.Mask\" user)\n\t\t(44 \"Edge.Cuts\" user)\n\t)\n",
    );
    for (i, model) in models.iter().enumerate() {
        pcb.push_str(&format!(
            "\t(footprint \"eda:model{i}\"\n\t\t(layer \"F.Cu\")\n\t\t(uuid \"00000000-0000-0000-0000-{i:012x}\")\n\t\t(at {} 0)\n\t\t(model {}\n\t\t\t(offset\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)\n\t\t\t(scale\n\t\t\t\t(xyz 1 1 1)\n\t\t\t)\n\t\t\t(rotate\n\t\t\t\t(xyz 0 0 0)\n\t\t\t)\n\t\t)\n\t)\n",
            i * 30,
            quote(model)
        ));
    }
    pcb.push_str(")\n");
    pcb
}

/// The VRML file kicad-cli wrote for `model` into `dir`: `<file stem>.wrl` (commas, dots and dashes are kept: `PhoenixContact_MC_1,5_6-G-5.08_..._Horizontal.wrl`),
/// else the same with every character that is not a plain file-name character turned into `_`, else one that differs only in case.
fn vrml_named(dir: &Path, model: &Path) -> Option<PathBuf> {
    let stem = model.file_stem()?.to_string_lossy().into_owned();
    let exact = dir.join(format!("{stem}.wrl"));
    if exact.is_file() {
        return Some(exact);
    }
    let plain = file_stem(&stem);
    let sanitized = dir.join(format!("{plain}.wrl"));
    if sanitized.is_file() {
        return Some(sanitized);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "wrl") && p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(&stem)))
}

/// Converts 3D model files (STEP and whatever else KiCad's 3D cache reads) to VRML, the format KiCad's own libraries are written in and three.js's
/// `VRMLLoader` reads: `kicad-cli pcb export vrml --models-dir` on a board that carries one footprint per model writes every model as a `.wrl` of its own,
/// in units of 0.1 inch, z up from the footprint -- the model with nothing of the board in it. One kicad-cli run converts them all (a run costs a second or
/// two before it loads anything), so the caller hands over every model it wants at once. The files are written under `work`; the answer has one entry per
/// model, `None` for one kicad-cli could not read. Two models of one file name would share a `.wrl`: the caller keeps them in separate runs.
pub fn convert_models_to_vrml(work: &Path, models: &[PathBuf]) -> Result<Vec<Option<PathBuf>>, Vec<CheckResult>> {
    if models.is_empty() {
        return Ok(Vec::new());
    }
    let cli = need_cli()?;
    work_dir(work)?;
    let board = work.join("models.kicad_pcb");
    let models_dir = work.join("m");
    let _ = std::fs::remove_dir_all(&models_dir);
    write_file(&board, models_board_text(models))?;
    let mut cmd = Command::new(&cli);
    cmd.args(["pcb", "export", "vrml", "--units", "mm", "--models-dir", "m", "-f", "-o"]).arg(work.join("models.wrl")).arg(&board);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(fail("kicad_cli_export", "pcb export vrml", format!("kicad-cli pcb export vrml failed: {}", said.trim())));
    }
    Ok(models.iter().map(|m| vrml_named(&models_dir, m)).collect())
}

// --------------------------------------------------------------------- reports

/// One item a violation names: kicad-cli's own description and position
/// (µm), plus our id for it (`None` when it is not one of ours, e.g. a zone
/// fill KiCad computed).
#[derive(Debug, Clone)]
pub struct Item {
    pub description: String,
    pub pos: (i64, i64),
    pub id: Option<String>,
    pub uuid: String,
}

/// One kicad-cli report entry. `kind` is KiCad's own settings key
/// (`clearance`, `courtyards_overlap`, `pin_not_connected`, ...).
#[derive(Debug, Clone, Default)]
pub struct Violation {
    pub kind: String,
    pub description: String,
    /// `error` or `warning` (`exclusion` or `ignore` when KiCad says so): the severity of the check, as the report gives it. A waived
    /// violation keeps the severity it would have had; [`excluded`](Self::excluded) says it is waived.
    pub severity: String,
    pub items: Vec<Item>,
    /// `RC_JSON::VIOLATION::excluded`: the project lists this violation as waived and kicad-cli matched it. A report that asks for
    /// exclusions (`--severity-exclusions`, which `--severity-all` includes) holds them; no gate or count may treat one as a finding.
    pub excluded: bool,
    /// The exclusion's comment (`RC_JSON::VIOLATION::comment`).
    pub comment: String,
    /// Where the violation's marker may sit, in nanometres: the positions worth writing an exclusion for ([`marker_candidates`]).
    /// kicad-cli's report does not give the marker's position, only its items'.
    pub markers_nm: Vec<(i64, i64)>,
}

impl Violation {
    fn from_json(v: &Value, map: &HashMap<String, String>, units_mm: bool) -> Violation {
        let to_um = |x: f64| if units_mm { (x * 1000.0).round() as i64 } else { (x * 25_400.0).round() as i64 };
        let items = v["items"]
            .as_array()
            .map(|is| {
                is.iter()
                    .map(|it| {
                        let uuid = it["uuid"].as_str().unwrap_or("").to_string();
                        Item {
                            description: it["description"].as_str().unwrap_or("").to_string(),
                            pos: (to_um(it["pos"]["x"].as_f64().unwrap_or(0.0)), to_um(it["pos"]["y"].as_f64().unwrap_or(0.0))),
                            id: map.get(&uuid).cloned(),
                            uuid,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let to_nm = |x: f64| if units_mm { (x * 1_000_000.0).round() as i64 } else { (x * 25_400_000.0).round() as i64 };
        let at_nm: Vec<(i64, i64)> = v["items"].as_array().map(|is| is.iter().map(|it| (to_nm(it["pos"]["x"].as_f64().unwrap_or(0.0)), to_nm(it["pos"]["y"].as_f64().unwrap_or(0.0)))).collect()).unwrap_or_default();
        Violation {
            kind: v["type"].as_str().unwrap_or("?").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            severity: v["severity"].as_str().unwrap_or("error").to_string(),
            items,
            excluded: v["excluded"].as_bool().unwrap_or(false),
            comment: v["comment"].as_str().unwrap_or("").to_string(),
            markers_nm: marker_candidates(&at_nm),
        }
    }

    /// The studio's DRC shape: `type`, `description`, `severity`, `items`
    /// (each with a `[x, y]` µm `pos`, our `id` or `null`, the KiCad `uuid`), `excluded` and its `comment`, and `marker_nm`: where the
    /// violation's marker may sit, in nanometres, which an exclusion keeps (see [`marker_candidates`]).
    pub fn to_json(&self) -> Value {
        json!({
            "type": self.kind,
            "description": self.description,
            "severity": self.severity,
            "items": self.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.0, i.pos.1], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
            "excluded": self.excluded,
            "comment": self.comment,
            "marker_nm": self.markers_nm.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(),
        })
    }

    /// The uuids of the items, in the order the report names them: with the check, what identifies the violation to an exclusion
    /// (`DrcExclusion::items`).
    pub fn item_uuids(&self) -> Vec<String> {
        self.items.iter().map(|i| i.uuid.clone()).collect()
    }

    /// Also try the positions the design itself gives for the items: the ends and the middle of a track segment, where a trace's marker
    /// sits (`DRC_TEST_PROVIDER_TRACK_WIDTH` reports `(start + end) / 2`). The report gives a track at its start only.
    fn add_design_anchors(&mut self, design: &Design) {
        let ids: Vec<String> = self.items.iter().filter_map(|i| i.id.clone()).collect();
        for id in ids {
            self.markers_nm.extend(track_anchors(design, &id));
        }
        self.markers_nm.sort_unstable();
        self.markers_nm.dedup();
    }

    /// Mark the violation waived when `exclusions` hold it. KiCad matches an exclusion to a marker by the marker's own text; kicad-cli has
    /// already done that for the exclusions whose position it could guess (`excluded` is its answer), and this is the studio's own
    /// match -- by the check and the items -- for the rest. Returns whether the studio's list holds it.
    fn apply_exclusions(&mut self, exclusions: &[DrcExclusion]) -> bool {
        let uuids = self.item_uuids();
        match exclusions.iter().find(|e| e.matches(&self.kind, &uuids)) {
            Some(e) => {
                self.excluded = true;
                self.comment = e.comment.clone();
                true
            }
            None => false,
        }
    }
}

/// The positions a violation's marker may sit at, in nanometres, from where its items are: each item's own (a via, a pad, a footprint,
/// the first pad of an unconnected pair, a track's start) and the middle of the first two. kicad-cli's report carries no marker position
/// -- `RC_ITEM::GetJsonViolation` writes `EDA_ITEM::GetPosition` of the items -- and KiCad matches an exclusion to a marker by the exact
/// text of the marker, position included, so these are the guesses an exclusion offers (`eda_kicad::drc_exclusions`).
pub fn marker_candidates(items_nm: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = items_nm.to_vec();
    if let [a, b, ..] = items_nm {
        out.push(((a.0 + b.0) / 2, (a.1 + b.1) / 2));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The ends and the middle of one segment of the track `id` names (`<track id>` for the first segment, `<track id>#<n>` for the n-th), in
/// nanometres. Empty for an id that is not a track's. The middle is KiCad's `(start + end) / 2` on integers.
fn track_anchors(design: &Design, id: &str) -> Vec<(i64, i64)> {
    let (base, n) = match id.rsplit_once('#') {
        Some((b, n)) if n.parse::<usize>().is_ok() => (b, n.parse::<usize>().unwrap_or(0)),
        _ => (id, 0),
    };
    let Some(track) = design.routing.as_ref().and_then(|r| r.tracks.iter().find(|t| t.id == base)) else {
        return Vec::new();
    };
    let nm = |p: &eda_model::ir::Point| (p.x * 1000, p.y * 1000);
    let (Some(a), Some(b)) = (track.pts.get(n), track.pts.get(n + 1)) else {
        return Vec::new();
    };
    let (a, b) = (nm(a), nm(b));
    vec![a, b, ((a.0 + b.0) / 2, (a.1 + b.1) / 2)]
}

fn counts_of<'a>(vs: impl Iterator<Item = &'a Violation>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in vs.filter(|v| !v.excluded) {
        *m.entry(v.kind.clone()).or_default() += 1;
    }
    m
}

/// A check set to Ignore (`DRC_REPORT`'s `ignored_checks`, the Ignored Tests tab): its settings key and KiCad's name for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredCheck {
    pub key: String,
    pub description: String,
}

impl IgnoredCheck {
    fn from_json(v: &Value) -> IgnoredCheck {
        IgnoredCheck { key: v["key"].as_str().unwrap_or("").to_string(), description: v["description"].as_str().unwrap_or("").to_string() }
    }

    fn to_json(&self) -> Value {
        json!({ "key": self.key, "description": self.description })
    }
}

/// `kicad-cli pcb drc` on one design revision.
#[derive(Debug, Clone)]
pub struct DrcReport {
    /// "kicad-cli 10.99.0".
    pub engine: String,
    pub zones_refilled_by_kicad: bool,
    pub violations: Vec<Violation>,
    pub unconnected_items: Vec<Violation>,
    /// `--schematic-parity`: the board's differences from the schematic (`DRC_TEST_PROVIDER_SCHEMATIC_PARITY`: a missing, extra or
    /// duplicate footprint, a pad on another net than the schematic's, a footprint other than the symbol's, ...). Empty when the test did
    /// not run -- see [`parity`](Self::parity).
    pub schematic_parity: Vec<Violation>,
    /// How the parity test went: `None` when it was not asked for, `Some(None)` when kicad-cli ran it, `Some(Some(why))` when it was asked for and
    /// kicad-cli could not (no annotated schematic next to the board, no netlist), in its own words.
    pub parity: Option<Option<String>>,
    /// The checks whose severity is Ignore, which kicad-cli does not run (`ignored_checks`): the Ignored Tests tab.
    pub ignored_checks: Vec<IgnoredCheck>,
}

impl DrcReport {
    pub fn counts(&self) -> BTreeMap<String, usize> {
        counts_of(self.violations.iter().chain(self.unconnected_items.iter()))
    }

    /// `{ engine, zones_refilled_by_kicad, violations, unconnected_items, schematic_parity, schematic_parity_run, ignored_checks, counts }`;
    /// `schematic_parity_error` too when the test was asked for and could not run. See [`to_json_with`](Self::to_json_with).
    pub fn to_json(&self) -> Value {
        self.to_json_with(&[])
    }

    /// [`to_json`](Self::to_json) with the violations the design waived (`exclusions`) marked `excluded`, each with its `comment`, and a
    /// `kicad_matched` flag saying whether kicad-cli itself matched it (the exclusion's marker position was one of its guesses). The
    /// counts leave the waived ones out, as KiCad's badges do.
    pub fn to_json_with(&self, exclusions: &[DrcExclusion]) -> Value {
        let mut report = self.clone();
        let mut kicad_matched = Vec::new();
        for list in [&mut report.violations, &mut report.unconnected_items, &mut report.schematic_parity] {
            for v in list.iter_mut() {
                let by_kicad = v.excluded;
                v.apply_exclusions(exclusions);
                kicad_matched.push(by_kicad);
            }
        }
        let mut matched = kicad_matched.into_iter();
        let mut shape = |v: &Violation| {
            let mut j = v.to_json();
            j["kicad_matched"] = json!(matched.next().unwrap_or(false));
            j
        };
        let violations: Vec<Value> = report.violations.iter().map(&mut shape).collect();
        let unconnected: Vec<Value> = report.unconnected_items.iter().map(&mut shape).collect();
        let parity: Vec<Value> = report.schematic_parity.iter().map(&mut shape).collect();
        let mut out = json!({
            "engine": report.engine,
            "zones_refilled_by_kicad": report.zones_refilled_by_kicad,
            "violations": violations,
            "unconnected_items": unconnected,
            "schematic_parity": parity,
            "schematic_parity_run": matches!(report.parity, Some(None)),
            "ignored_checks": report.ignored_checks.iter().map(IgnoredCheck::to_json).collect::<Vec<_>>(),
            "counts": report.counts(),
        });
        if let Some(Some(why)) = &report.parity {
            out["schematic_parity_error"] = json!(why);
        }
        out
    }
}

fn run_report(cmd: Command, report: &Path, what: &str) -> Result<Value, Vec<CheckResult>> {
    run_report_with_output(cmd, report, what).map(|(raw, _)| raw)
}

/// [`run_report`], also giving what kicad-cli printed (its progress lines and the reasons it skipped a test).
fn run_report_with_output(cmd: Command, report: &Path, what: &str) -> Result<(Value, String), Vec<CheckResult>> {
    let _ = std::fs::remove_file(report);
    let out = output_within(cmd, limit(REPORT_TIMEOUT))?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(report).map_err(|_| fail(&format!("kicad_cli_{what}"), "kicad-cli", format!("no report: {said}")))?;
    let raw = serde_json::from_str(&text).map_err(|e| fail(&format!("kicad_cli_{what}"), &report.display().to_string(), e.to_string()))?;
    Ok((raw, said))
}

/// What a `kicad-cli pcb drc` run is asked for beyond the default.
#[derive(Debug, Clone, Copy, Default)]
pub struct DrcOptions {
    /// "Refill all zones before performing DRC" (`--refill-zones`): see [`drc`].
    pub refill_zones: bool,
    /// "Test for parity between PCB and schematic" (`--schematic-parity`): the derived schematic goes next to the derived board, where kicad-cli looks for
    /// it, and the report's `schematic_parity` holds what differs.
    pub schematic_parity: bool,
}

/// `kicad-cli pcb drc` on `design` as it is.
///
/// `refill_zones` (KiCad's "Refill all zones before performing DRC") is
/// opt-in. By default KiCad judges the fills the exported board already
/// carries -- the ones our own filler computed and the studio shows -- and
/// that matters: kicad-cli 10.99 skips the courtyard checks
/// (`courtyards_overlap`, `pth_inside_courtyard`, ...) on a run that
/// refills, so a default refill would silently turn the placement gates off.
/// A kicad-cli without `--refill-zones` (9.0) falls back to the same
/// no-refill run. Every kicad-cli start costs a third of a second, so this
/// neither asks for its version nor probes its options: the version comes
/// off the report.
pub fn drc(design: &Design, model: &ConstraintModel, work: &Path, refill_zones: bool) -> Result<DrcReport, Vec<CheckResult>> {
    drc_with(design, model, work, DrcOptions { refill_zones, schematic_parity: false })
}

/// [`drc`] with options. The violations the design waived (`design.drawings.drc_exclusions`) are in the derived project
/// ([`eda_kicad::export_kicad_pro_for`]), so kicad-cli flags the ones it can match as `excluded`; the report asks for every severity,
/// exclusions included, and each [`Violation`] says whether it is one.
pub fn drc_with(design: &Design, model: &ConstraintModel, work: &Path, opts: DrcOptions) -> Result<DrcReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (pcb, map) = export_board(design, model, work, "board")?;
    if opts.schematic_parity {
        // kicad-cli reads the schematic next to the board (`<board stem>.kicad_sch`) to make the netlist the parity test compares with.
        if design.schematic.is_none() {
            return Err(fail("kicad_cli_parity", "schematic", "the parity test compares the board with the schematic, and this design has none"));
        }
        export_schematic(design, model, work, "board")?;
    }
    let report = work.join("drc.json");
    let run = |refill: bool| {
        let mut cmd = Command::new(&cli);
        cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]);
        if refill {
            cmd.arg("--refill-zones");
        }
        if opts.schematic_parity {
            cmd.arg("--schematic-parity");
        }
        cmd.arg("-o").arg(&report).arg(&pcb);
        run_report_with_output(cmd, &report, "drc")
    };
    let ((raw, said), refilled) = if opts.refill_zones {
        match run(true) {
            Ok(raw) => (raw, true),
            Err(e) if e.iter().any(|c| c.hint.as_deref().is_some_and(|h| h.contains("refill-zones"))) => (run(false)?, false),
            Err(e) => return Err(e),
        }
    } else {
        (run(false)?, false)
    };
    let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
    let read = |key: &str| -> Vec<Violation> {
        let mut list: Vec<Violation> = raw[key].as_array().map(|vs| vs.iter().map(|v| Violation::from_json(v, &map, units_mm)).collect()).unwrap_or_default();
        for v in &mut list {
            v.add_design_anchors(design);
        }
        list
    };
    let version = raw["kicad_version"].as_str().map(str::to_string).unwrap_or_else(|| cli_version(&cli));
    // `JobExportDrc` prints "Found N schematic parity issues" only when it ran the test; when it could not (no annotated schematic, no
    // netlist) it says why on a line of its own and goes on without.
    let parity = opts.schematic_parity.then(|| {
        if said.contains("schematic parity issues") {
            None
        } else {
            Some(said.lines().map(str::trim).find(|l| l.contains("parity")).map(str::to_string).unwrap_or_else(|| "kicad-cli did not run the parity test".to_string()))
        }
    });
    Ok(DrcReport {
        engine: format!("kicad-cli {version}"),
        zones_refilled_by_kicad: refilled,
        violations: read("violations"),
        unconnected_items: read("unconnected_items"),
        schematic_parity: if opts.schematic_parity { read("schematic_parity") } else { Vec::new() },
        parity,
        ignored_checks: raw["ignored_checks"].as_array().map(|cs| cs.iter().map(IgnoredCheck::from_json).collect()).unwrap_or_default(),
    })
}

/// [`drc`] in a throwaway directory -- for a design that only exists in
/// memory (the gates).
pub fn drc_scratch(design: &Design, model: &ConstraintModel) -> Result<DrcReport, Vec<CheckResult>> {
    with_scratch(|work| drc(design, model, work, false))
}

/// `kicad-cli sch erc` on one design revision, violations from every sheet
/// flattened.
#[derive(Debug, Clone)]
pub struct ErcReport {
    pub engine: String,
    pub violations: Vec<Violation>,
    /// The checks whose severity is Ignore, which kicad-cli does not run (`ignored_checks`): the Ignored Tests tab.
    pub ignored_checks: Vec<IgnoredCheck>,
}

impl ErcReport {
    /// The studio's ERC shape: `check`, `severity`, `location` (the first
    /// item's id), `hint`, plus the full `items` list. A finding whose
    /// `(check, location)` is in `exclusions` reports as `"excluded"`: still
    /// listed, no longer an error or a warning.
    pub fn to_json(&self, exclusions: &[(String, String)]) -> Value {
        let mut counts = BTreeMap::<String, usize>::new();
        let violations: Vec<Value> = self
            .violations
            .iter()
            .map(|v| {
                let location = v.items.iter().find_map(|i| i.id.clone()).unwrap_or_default();
                let excluded = !location.is_empty() && exclusions.iter().any(|(c, l)| *c == v.kind && *l == location);
                if !excluded {
                    *counts.entry(v.kind.clone()).or_default() += 1;
                }
                let item_desc: Vec<&str> = v.items.iter().map(|i| i.description.as_str()).filter(|d| !d.is_empty()).collect();
                json!({
                    "check": v.kind,
                    "severity": if excluded || v.excluded || v.severity == "exclusion" { "excluded" } else { v.severity.as_str() },
                    // What the finding is when it is not excluded: the report folds the two together, and un-excluding must give the severity back.
                    "base_severity": if v.severity == "warning" { "warning" } else { "error" },
                    "location": location,
                    "hint": format!("{}{}{}", v.description, if item_desc.is_empty() { "" } else { ": " }, item_desc.join("; ")),
                    "items": v.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.0, i.pos.1], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "engine": self.engine, "violations": violations, "counts": counts, "ignored_checks": self.ignored_checks.iter().map(IgnoredCheck::to_json).collect::<Vec<_>>() })
    }
}

/// `kicad-cli sch erc` on `design`'s schematic.
pub fn erc(design: &Design, model: &ConstraintModel, work: &Path) -> Result<ErcReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (sch, map) = export_schematic(design, model, work, "board")?;
    let report = work.join("erc.json");
    let mut cmd = Command::new(&cli);
    cmd.args(["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(&sch);
    let raw = run_report(cmd, &report, "erc")?;
    let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
    let mut violations = Vec::new();
    for sheet in raw["sheets"].as_array().into_iter().flatten() {
        for v in sheet["violations"].as_array().into_iter().flatten() {
            violations.push(Violation::from_json(v, &map, units_mm));
        }
    }
    let version = raw["kicad_version"].as_str().map(str::to_string).unwrap_or_else(|| cli_version(&cli));
    let ignored_checks = raw["ignored_checks"].as_array().map(|cs| cs.iter().map(IgnoredCheck::from_json).collect()).unwrap_or_default();
    Ok(ErcReport { engine: format!("kicad-cli {version}"), violations, ignored_checks })
}

// ------------------------------------------------------------------ statistics

/// `kicad-cli pcb export stats` options -- the Board Statistics dialog's three
/// checkboxes, plus the report's length unit.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatsOptions {
    pub exclude_footprints_without_pads: bool,
    pub subtract_holes_from_board: bool,
    pub subtract_holes_from_copper: bool,
    /// Lengths in inches (kicad-cli has `mm` and `in`); default millimetres.
    pub inches: bool,
}

/// `kicad-cli pcb export stats` on `design`: the JSON report (`json`) or the
/// text report KiCad's own dialog saves (`!json`), as text.
pub fn stats(design: &Design, model: &ConstraintModel, work: &Path, opts: StatsOptions, json: bool) -> Result<String, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (pcb, _) = export_board(design, model, work, "board")?;
    let out_file = work.join(if json { "stats.json" } else { "stats.txt" });
    let _ = std::fs::remove_file(&out_file);
    let mut cmd = Command::new(&cli);
    cmd.args(["pcb", "export", "stats", "--format", if json { "json" } else { "report" }, "--units", if opts.inches { "in" } else { "mm" }]);
    if opts.exclude_footprints_without_pads {
        cmd.arg("--exclude-footprints-without-pads");
    }
    if opts.subtract_holes_from_board {
        cmd.arg("--subtract-holes-from-board");
    }
    if opts.subtract_holes_from_copper {
        cmd.arg("--subtract-holes-from-copper");
    }
    cmd.arg("-o").arg(&out_file).arg(&pcb);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    std::fs::read_to_string(&out_file).map_err(|_| fail("kicad_cli_stats", "kicad-cli", format!("no report: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))
}

// ------------------------------------------------------------ custom rules

/// What kicad-cli says about a `.kicad_dru` text ([`check_rules`]).
#[derive(Debug, Clone)]
pub struct RulesCheck {
    /// "kicad-cli 10.99.0".
    pub engine: String,
    /// Every rule in the text loaded.
    pub valid: bool,
    /// How many top-level forms the text has (the `(version 1)` header is one).
    pub forms: usize,
    /// When the text does not load: the first form kicad-cli cannot load.
    pub bad: Option<BadRuleForm>,
}

/// The first top-level form of a rules text that kicad-cli does not load.
#[derive(Debug, Clone)]
pub struct BadRuleForm {
    /// Position among the text's top-level forms, from 0.
    pub index: usize,
    /// The line the form starts on, from 1.
    pub line: usize,
    /// The form's text (cut at 400 characters).
    pub text: String,
}

impl RulesCheck {
    /// `{ ok, engine, valid, forms, bad: { index, line, text } | null, message }`.
    pub fn to_json(&self) -> Value {
        let message = match (&self.bad, self.valid) {
            (_, true) => format!("kicad-cli loaded all {} part(s) of the rules.", self.forms),
            (Some(b), false) => format!("kicad-cli could not load the rules; the first part it refuses starts on line {}.", b.line),
            (None, false) => "kicad-cli could not load the rules: the parentheses do not balance.".to_string(),
        };
        json!({
            "ok": true,
            "engine": self.engine,
            "valid": self.valid,
            "forms": self.forms,
            "bad": self.bad.as_ref().map(|b| json!({ "index": b.index, "line": b.line, "text": b.text })),
            "message": message,
        })
    }
}

/// The byte ranges of the top-level `( ... )` forms of an s-expression text; `None` when the parentheses do not balance
/// (or a string is left open). Strings and `#` comments are skipped, nothing else is read: which forms are rules, and
/// whether they make sense, is kicad-cli's to say.
pub fn top_level_forms(text: &str) -> Option<Vec<(usize, usize)>> {
    let bytes = text.as_bytes();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    let mut forms = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return None;
                }
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            b')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    forms.push((start, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    (depth == 0).then_some(forms)
}

/// Two nets 0.8 mm apart on one layer inside an outline: nothing is wrong with it under any ordinary rule, and
/// a rule asking for 1000 mm of clearance has to say so -- which it does only if the rules file loaded.
const RULES_PROBE_PCB: &str = r#"(kicad_pcb
	(version 20241229)
	(generator "eda-probe")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(net 1 "A")
	(net 2 "B")
	(segment (start 10 10) (end 15 10) (width 0.2) (layer "F.Cu") (net 1) (uuid "6a1d0000-0000-4000-8000-000000000001"))
	(segment (start 10 11) (end 15 11) (width 0.2) (layer "F.Cu") (net 2) (uuid "6a1d0000-0000-4000-8000-000000000002"))
	(gr_rect (start 5 5) (end 20 16) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "6a1d0000-0000-4000-8000-000000000003"))
)
"#;

/// A rule that fires on the probe board, appended after the text being checked.
const RULES_PROBE_SENTINEL: &str = "(rule \"eda_probe_sentinel\"\n  (constraint clearance (min 1000mm)))\n";

/// Does kicad-cli load `rules` (a `.kicad_dru` text) whole? It does when the sentinel rule appended after it takes
/// effect on the probe board. kicad-cli has no rules checker of its own, and a rules file that does not parse is
/// dropped without a word (DRC then runs on the board's implicit rules alone), so a rule that must show itself is the
/// only way to see that the file was read.
fn rules_load(cli: &Path, dir: &Path, rules: &str) -> Result<bool, Vec<CheckResult>> {
    work_dir(dir)?;
    write_file(&dir.join("probe.kicad_pcb"), RULES_PROBE_PCB)?;
    write_file(&dir.join("probe.kicad_dru"), format!("{rules}\n{RULES_PROBE_SENTINEL}"))?;
    let report = dir.join("probe.drc.json");
    let mut cmd = Command::new(cli);
    cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(dir.join("probe.kicad_pcb"));
    let raw = run_report(cmd, &report, "rules")?;
    Ok(raw["violations"].as_array().is_some_and(|vs| vs.iter().any(|v| v["type"].as_str() == Some("clearance"))))
}

/// Ask kicad-cli whether `text` (a `.kicad_dru`) loads: once for the whole text, and, if it does not, once per step of a
/// binary search over its top-level forms to find the first form it refuses. Runs on a scratch probe board; the
/// design is not involved.
pub fn check_rules(text: &str) -> Result<RulesCheck, Vec<CheckResult>> {
    let cli = need_cli()?;
    let engine = format!("kicad-cli {}", cli_version(&cli));
    let Some(forms) = top_level_forms(text) else {
        return Ok(RulesCheck { engine, valid: false, forms: 0, bad: None });
    };
    with_scratch(|dir| {
        if rules_load(&cli, dir, text)? {
            return Ok(RulesCheck { engine, valid: true, forms: forms.len(), bad: None });
        }
        // The first k forms load for k = 0 (the header alone) and not for k = forms.len(); find where that changes.
        let upto = |k: usize| -> String { forms[..k].iter().map(|&(a, b)| &text[a..b]).collect::<Vec<_>>().join("\n") };
        let (mut lo, mut hi) = (0usize, forms.len());
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if rules_load(&cli, dir, &upto(mid))? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (a, b) = forms[hi - 1];
        let line = 1 + text[..a].matches('\n').count();
        let shown: String = text[a..b].chars().take(400).collect();
        Ok(RulesCheck { engine, valid: false, forms: forms.len(), bad: Some(BadRuleForm { index: hi - 1, line, text: shown }) })
    })
}

// --------------------------------------------------------------------- exports

/// kicad-cli `pcb export` subcommands that write a directory of files; every
/// other kind writes one file named `board.<ext>`.
const DIR_KINDS: &[&str] = &["gerbers", "drill"];

fn pcb_ext(kind: &str) -> &'static str {
    match kind {
        "pos" => "pos",
        "step" => "step",
        "stpz" => "stpz",
        "brep" => "brep",
        "xao" => "xao",
        "glb" => "glb",
        "stl" => "stl",
        "ply" => "ply",
        "u3d" => "u3d",
        "vrml" => "wrl",
        "pdf" | "3dpdf" => "pdf",
        "svg" => "svg",
        "dxf" => "dxf",
        "ps" => "ps",
        "png" => "png",
        "ipc2581" => "xml",
        "ipcd356" => "d356",
        "odb" => "zip",
        "gencad" => "cad",
        "idf" => "emn",
        "stats" | "stackup" => "txt",
        _ => "out",
    }
}

/// The extension kicad-cli's output carries for these arguments. ODB++ and IPC-2581 are named by the compression asked
/// for, since kicad-cli writes whatever name it is given: an ODB++ is a `.zip` or a `.tgz`, or (`--compression none`) a
/// folder with no extension; an IPC-2581 is an `.xml`, or a `.zip` when compressed.
fn pcb_ext_for(kind: &str, args: &[String]) -> &'static str {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
    match kind {
        "odb" => match value("--compression") {
            Some("tgz") => "tgz",
            Some("none") => "",
            _ => "zip",
        },
        "ipc2581" if args.iter().any(|a| a == "--compress") => "zip",
        _ => pcb_ext(kind),
    }
}

fn sch_ext(kind: &str) -> &'static str {
    match kind {
        "netlist" => "net",
        "bom" => "csv",
        "pdf" => "pdf",
        "ps" => "ps",
        "dxf" => "dxf",
        "svg" => "svg",
        "png" => "png",
        // the legacy BOM generators' input: the intermediate XML netlist
        "python-bom" => "xml",
        _ => "out",
    }
}

/// `sch export` subcommands that write a directory of files (one per sheet).
const SCH_DIR_KINDS: &[&str] = &["svg", "dxf", "png", "ps"];

/// A kicad-cli export that wrote files: their paths relative to `root`.
#[allow(clippy::too_many_arguments)]
fn run_export(cli: &Path, scope: &str, kind: &str, args: &[String], input: &Path, out_dir: &Path, dir_kind: bool, ext: &str, stem: &str, root: &Path) -> Result<Value, Vec<CheckResult>> {
    std::fs::create_dir_all(out_dir).map_err(|e| fail("kicad_engine_dir", "export", e.to_string()))?;
    let started = std::time::SystemTime::now() - std::time::Duration::from_secs(1);
    // An output with no extension is a folder kicad-cli creates itself (ODB++, uncompressed): one left by the last run is cleared first.
    let target = if dir_kind {
        out_dir.to_path_buf()
    } else if ext.is_empty() {
        let folder = out_dir.join(stem);
        let _ = std::fs::remove_dir_all(&folder);
        folder
    } else {
        out_dir.join(format!("{stem}.{ext}"))
    };
    let mut out_arg = target.to_string_lossy().to_string();
    if dir_kind && !out_arg.ends_with('/') {
        out_arg.push('/');
    }
    let mut cmd = Command::new(cli);
    cmd.args([scope, "export", kind]).args(args).arg("-o").arg(&out_arg).arg(input);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let mut files: Vec<String> = std::fs::read_dir(out_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= started))
        .map(|e| {
            let p = e.path();
            p.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string_lossy().to_string())
        })
        .collect();
    files.sort();
    if !out.status.success() || files.is_empty() {
        return Err(fail("kicad_cli_export", kind, format!("kicad-cli {scope} export {kind} failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))));
    }
    Ok(json!({ "ok": true, "engine": format!("kicad-cli {}", cli_version(cli)), "files": files }))
}

fn check_kind(kind: &str) -> Result<(), Vec<CheckResult>> {
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(fail("kicad_cli_export", kind, "unknown export kind"));
    }
    Ok(())
}

/// `kicad-cli pcb export <kind> [args...]` on `design`, into
/// `<root>/export/kicad/<kind>/`. `name` is the project's name: the derived
/// board is written under it, so kicad-cli names its files after it
/// (`<name>-F_Cu.gtl`, `<name>.pos`). `args` are passed through (e.g. `--layers
/// F.Cu,B.Cu`, `--format csv`). Returns `{ ok, engine, files }`, files
/// relative to `root`.
pub fn export_pcb(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, name: &str, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    check_kind(kind)?;
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (pcb, _) = export_board(design, model, work, &stem)?;
    let out_dir = root.join("export").join("kicad").join(kind);
    run_export(&cli, "pcb", kind, args, &pcb, &out_dir, DIR_KINDS.contains(&kind), pcb_ext_for(kind, args), &stem, root)
}

/// `kicad-cli sch export <kind> [args...]` on `design`'s schematic, into
/// `<root>/export/kicad/sch-<kind>/` (kinds: `netlist`, `bom`, `pdf`, `svg`,
/// `dxf`, `ps`, `png`), named after the project like [`export_pcb`].
pub fn export_sch(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, name: &str, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    check_kind(kind)?;
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (sch, _) = export_schematic(design, model, work, &stem)?;
    let out_dir = root.join("export").join("kicad").join(format!("sch-{kind}"));
    run_export(&cli, "sch", kind, args, &sch, &out_dir, SCH_DIR_KINDS.contains(&kind), sch_ext(kind), &stem, root)
}

/// Puts a BOM preset and a BOM format preset (the JSON KiCad keeps in a project's `schematic.bom_presets` / `schematic.bom_fmt_presets`) into the derived
/// project file, where `kicad-cli sch export bom --preset <name> --format-preset <name>` finds them. The command line itself has no way to say everything a
/// preset does (it cannot sort descending: an explicit `--sort-asc false` crashes kicad-cli), so the table's settings travel as presets.
fn set_bom_presets(work: &Path, stem: &str, preset: &Value, fmt: &Value) -> Result<(), Vec<CheckResult>> {
    let path = work.join(format!("{stem}.kicad_pro"));
    let text = std::fs::read_to_string(&path).map_err(|e| fail("kicad_engine_file", "kicad_pro", e.to_string()))?;
    let mut pro: Value = serde_json::from_str(&text).map_err(|e| fail("kicad_engine_file", "kicad_pro", format!("the derived project file is not JSON: {e}")))?;
    let Some(root) = pro.as_object_mut() else { return Err(fail("kicad_engine_file", "kicad_pro", "the derived project file is not a JSON object")) };
    let schematic = root.entry("schematic").or_insert_with(|| json!({}));
    if !schematic.is_object() {
        *schematic = json!({});
    }
    schematic["bom_presets"] = json!([preset]);
    schematic["bom_fmt_presets"] = json!([fmt]);
    write_file(&path, serde_json::to_string_pretty(&pro).unwrap_or(text))
}

/// `kicad-cli sch export bom` with the Symbol Fields Table's settings, written to `output` (a file under `root`; default
/// `<root>/export/kicad/sch-bom/<name>.csv`). `preset` and `fmt` are KiCad's `BOM_PRESET` / `BOM_FMT_PRESET` JSON, both with a `name`.
/// Returns `{ ok, engine, files }` like [`export_sch`], the one file relative to `root`.
#[allow(clippy::too_many_arguments)]
pub fn export_sch_bom(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, name: &str, preset: &Value, fmt: &Value, output: Option<&Path>) -> Result<Value, Vec<CheckResult>> {
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (sch, _) = export_schematic(design, model, work, &stem)?;
    set_bom_presets(work, &stem, preset, fmt)?;
    let output = output.map(Path::to_path_buf).unwrap_or_else(|| root.join("export").join("kicad").join("sch-bom").join(format!("{stem}.csv")));
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| fail("kicad_engine_dir", "export", format!("could not create '{}': {e}", parent.display())))?;
    }
    let started = std::time::SystemTime::now() - std::time::Duration::from_secs(1);
    let named = |v: &Value| v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let mut cmd = Command::new(&cli);
    cmd.args(["sch", "export", "bom", "--preset"]).arg(named(preset)).arg("--format-preset").arg(named(fmt)).arg("-o").arg(&output).arg(&sch);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let written = output.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= started);
    if !out.status.success() || !written {
        return Err(fail("kicad_cli_export", "bom", format!("kicad-cli sch export bom failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))));
    }
    let rel = output.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| output.to_string_lossy().to_string());
    Ok(json!({ "ok": true, "engine": format!("kicad-cli {}", cli_version(&cli)), "files": [rel] }))
}

/// `kicad-cli sym export svg [args...]` on `kicad_sym`, a library file's text (one symbol, or several), into `<root>/export/kicad/sym-svg/`:
/// one SVG per unit and body style, which kicad-cli names `<symbol>_unit<N>[_demorgan].svg`. Returns `{ ok, engine, files }`, files relative to `root`.
pub fn export_symbol_svg(work: &Path, root: &Path, name: &str, kicad_sym: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let cli = need_cli()?;
    work_dir(work)?;
    let stem = file_stem(name);
    let input = work.join(format!("{stem}.kicad_sym"));
    write_file(&input, kicad_sym)?;
    let out_dir = root.join("export").join("kicad").join("sym-svg");
    run_export(&cli, "sym", "svg", args, &input, &out_dir, true, "svg", &stem, root)
}

/// KiCad's stock plugin scripts (the legacy BOM generators): `EDA_KICAD_PLUGINS`, else the folder of the installed KiCad that kicad-cli belongs to
/// (`Contents/SharedSupport/plugins` in the macOS app, `share/kicad/plugins` elsewhere).
pub fn bom_plugins_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_PLUGINS").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(p);
    }
    let cli = find_cli()?;
    let bin = cli.canonicalize().unwrap_or(cli);
    let up = bin.parent()?.parent()?;
    ["SharedSupport/plugins", "share/kicad/plugins"].iter().map(|rel| up.join(rel)).find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_forms_are_found_past_strings_and_comments_and_unbalanced_text_is_refused() {
        let text = "(version 1)\n# a comment with ( an open paren\n(rule \"a ) b\"\n  (constraint clearance (min 0.2mm)))\n(rule two (condition \"x\\\"y\"))";
        let forms = top_level_forms(text).expect("balanced");
        let texts: Vec<&str> = forms.iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(texts.len(), 3);
        assert_eq!(texts[0], "(version 1)");
        assert!(texts[1].starts_with("(rule \"a ) b\"") && texts[1].ends_with("0.2mm)))"));
        assert!(texts[2].starts_with("(rule two"));
        for bad in ["(version 1", "(version 1))", "(rule \"open string)", ""] {
            assert!(top_level_forms(bad).is_none() || bad.is_empty(), "{bad:?}");
        }
        assert_eq!(top_level_forms("").unwrap().len(), 0);
    }

    #[test]
    fn kicad_cli_says_whether_a_rules_text_loads() {
        // The real kicad-cli, so not while another test has the environment pointing at a fake one or a one-second limit.
        let _env = Env::take();
        if find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return;
        }
        let good = "(version 1)\n(rule \"wide power\"\n  (constraint clearance (min 0.5mm))\n  (condition \"A.NetClass == 'power'\"))\n";
        let r = check_rules(good).unwrap();
        assert!(r.valid, "{r:?}");
        assert_eq!(r.forms, 2);
        // Parentheses that do not balance are refused without asking kicad-cli, which would drop the text without saying so.
        let r = check_rules("(version 1)\n(rule \"x\"").unwrap();
        assert!(!r.valid && r.bad.is_none() && r.forms == 0);
    }

    /// Slow tier: finding the refused rule takes one kicad-cli run per top-level form.
    #[test]
    fn kicad_cli_names_the_part_of_a_rules_text_it_refuses() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return;
        }
        let _env = Env::take();
        if find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return;
        }
        // The second rule's condition is not an expression kicad-cli can compile.
        let bad = "(version 1)\n(rule \"fine\"\n  (constraint clearance (min 0.5mm)))\n(rule \"broken\"\n  (constraint clearance (min 0.5mm))\n  (condition \"A.NetClass == \"))\n(rule \"after\"\n  (constraint track_width (min 0.1mm)))\n";
        let r = check_rules(bad).unwrap();
        assert!(!r.valid, "{r:?}");
        let b = r.bad.expect("the refused part is named");
        assert_eq!((b.index, b.line), (2, 4), "{b:?}");
        assert!(b.text.starts_with("(rule \"broken\""), "{b:?}");

        // No header.
        assert!(!check_rules("(rule \"x\" (constraint clearance (min 1mm)))").unwrap().valid, "kicad-cli wants (version 1) first");
    }

    #[test]
    fn maps_report_items_back_to_our_ids() {
        let mut map = HashMap::new();
        map.insert("u-1".to_string(), "trk_1#2".to_string());
        let v = json!({ "type": "clearance", "description": "x", "severity": "error",
            "items": [{ "description": "Track", "pos": { "x": 1.5, "y": -2.0 }, "uuid": "u-1" },
                      { "description": "Zone", "pos": { "x": 0.0, "y": 0.0 }, "uuid": "u-9" }] });
        let m = Violation::from_json(&v, &map, true).to_json();
        assert_eq!(m["items"][0]["id"], "trk_1#2");
        assert_eq!(m["items"][0]["pos"], json!([1500, -2000]));
        assert!(m["items"][1]["id"].is_null());
    }

    #[test]
    fn a_report_entry_reads_its_exclusion_and_offers_the_positions_its_marker_may_sit_at() {
        let map = HashMap::from([("u-1".to_string(), "R1.1".to_string()), ("u-2".to_string(), "R2.1".to_string())]);
        let v = json!({ "type": "clearance", "description": "x", "severity": "error", "excluded": true, "comment": "fine",
            "items": [{ "description": "Pad 1", "pos": { "x": 1.5, "y": -2.0 }, "uuid": "u-1" },
                      { "description": "Pad 2", "pos": { "x": 3.5, "y": 0.001 }, "uuid": "u-2" }] });
        let v = Violation::from_json(&v, &map, true);
        assert!(v.excluded && v.comment == "fine");
        assert_eq!(v.severity, "error", "a waived violation keeps the severity it would have had");
        // Each item's own position and the middle of the first two, in nanometres.
        assert_eq!(v.markers_nm, vec![(1_500_000, -2_000_000), (2_500_000, -999_500), (3_500_000, 1_000)]);
        let j = v.to_json();
        assert_eq!(j["excluded"], true);
        assert_eq!(j["marker_nm"][0], json!([1_500_000, -2_000_000]));
        // A plain entry is not excluded.
        let plain = Violation::from_json(&json!({ "type": "clearance", "description": "x", "severity": "warning", "items": [] }), &map, true);
        assert!(!plain.excluded && plain.comment.is_empty() && plain.markers_nm.is_empty());
    }

    #[test]
    fn a_track_segment_offers_its_ends_and_its_middle_as_the_markers_position() {
        let mut design = empty_design().0;
        design.routing = Some(eda_model::ir::RoutingSection {
            tracks: vec![eda_model::ir::Track { id: "t7".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 100, pts: vec![eda_model::ir::Point { x: 1_000, y: 2_000 }, eda_model::ir::Point { x: 3_001, y: 2_000 }, eda_model::ir::Point { x: 3_001, y: 9_000 }], arc_mid_offset: None }],
            vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default(),
        });
        // The first segment is the track's own id, the next ones `<id>#n`: `DRC_TEST_PROVIDER_TRACK_WIDTH` puts a trace's marker at (start + end) / 2.
        assert_eq!(track_anchors(&design, "t7"), vec![(1_000_000, 2_000_000), (3_001_000, 2_000_000), (2_000_500, 2_000_000)]);
        assert_eq!(track_anchors(&design, "t7#1"), vec![(3_001_000, 2_000_000), (3_001_000, 9_000_000), (3_001_000, 5_500_000)]);
        assert!(track_anchors(&design, "t7#2").is_empty(), "there is no third segment");
        assert!(track_anchors(&design, "R1.1").is_empty() && track_anchors(&design, "t8").is_empty());

        let mut v = Violation { kind: "track_width".into(), items: vec![Item { description: "Track".into(), pos: (1_000, 2_000), id: Some("t7".into()), uuid: "u".into() }], markers_nm: vec![(1_000_000, 2_000_000)], ..Default::default() };
        v.add_design_anchors(&design);
        assert_eq!(v.markers_nm, vec![(1_000_000, 2_000_000), (2_000_500, 2_000_000), (3_001_000, 2_000_000)], "the start (the report's) is listed once");
    }

    #[test]
    fn the_studio_marks_a_waived_violation_by_its_check_and_items_and_says_whether_kicad_matched_it() {
        let item = |uuid: &str| Item { description: uuid.into(), pos: (0, 0), id: None, uuid: uuid.into() };
        let found = |kind: &str, uuids: &[&str], by_kicad: bool| Violation { kind: kind.into(), severity: "error".into(), items: uuids.iter().map(|u| item(u)).collect(), excluded: by_kicad, ..Default::default() };
        let rep = DrcReport {
            engine: "kicad-cli test".into(),
            zones_refilled_by_kicad: false,
            violations: vec![found("clearance", &["a", "b"], false), found("clearance", &["a", "c"], false), found("annular_width", &["v"], true), found("hole_size", &["v"], false)],
            unconnected_items: vec![found("unconnected_items", &["p", "q"], false)],
            schematic_parity: vec![],
            parity: None,
            ignored_checks: vec![],
        };
        let waived = |check: &str, items: &[&str], comment: &str| DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: vec![], comment: comment.into() };
        let j = rep.to_json_with(&[waived("clearance", &["a", "b"], "slot"), waived("annular_width", &["v"], ""), waived("unconnected_items", &["p", "q"], "")]);
        let flags = |key: &str| j[key].as_array().unwrap().iter().map(|v| (v["excluded"].as_bool().unwrap(), v["kicad_matched"].as_bool().unwrap())).collect::<Vec<_>>();
        assert_eq!(flags("violations"), vec![(true, false), (false, false), (true, true), (false, false)], "the studio's match by (check, items); kicad_matched only where kicad-cli said so");
        assert_eq!(j["violations"][0]["comment"], "slot");
        assert_eq!(flags("unconnected_items"), vec![(true, false)]);
        assert_eq!(j["counts"], json!({ "clearance": 1, "hole_size": 1 }), "a waived violation is not counted");
        // The same items under another check, or in the other order, are another violation.
        let j = rep.to_json_with(&[waived("hole_size", &["x"], ""), waived("clearance", &["b", "a"], "")]);
        assert!(j["violations"].as_array().unwrap().iter().filter(|v| v["excluded"] == true).count() == 1, "only kicad-cli's own flag: {j}");
        assert_eq!(rep.to_json()["violations"][0]["excluded"], false, "nothing waived by the design");
    }

    #[test]
    fn the_parity_test_is_asked_for_with_the_schematic_beside_the_board_and_its_answer_is_read() {
        let mut env = Env::take();
        let dir = std::env::temp_dir().join(format!("eda_kicad_engine_parity_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (design, model) = empty_design();
        // Two canned reports: kicad-cli answers with the one that has parity entries, and prints the line it prints for them, only when
        // it is given `--schematic-parity`.
        let report = |parity: Value| json!({ "coordinate_units": "mm", "kicad_version": "10.99.0-fake", "violations": [], "unconnected_items": [], "schematic_parity": parity,
            "ignored_checks": [{ "key": "missing_courtyard", "description": "Footprint has no courtyard defined" }] });
        std::fs::write(dir.join("plain.json"), report(json!([])).to_string()).unwrap();
        std::fs::write(dir.join("parity.json"), report(json!([{ "type": "net_conflict", "description": "Pad net (VDD) does not match", "severity": "warning", "items": [] }])).to_string()).unwrap();
        env.set("FAKE_REPORT_PLAIN", dir.join("plain.json"));
        env.set("FAKE_REPORT_PARITY", dir.join("parity.json"));
        env.set(
            "EDA_KICAD_CLI",
            script(
                &dir,
                "parity.sh",
                "out=\"\"; prev=\"\"; parity=no\nfor a in \"$@\"; do if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi; if [ \"$a\" = \"--schematic-parity\" ]; then parity=yes; fi; prev=\"$a\"; done\nif [ $parity = yes ]; then echo \"Found 1 schematic parity issues\"; cp \"$FAKE_REPORT_PARITY\" \"$out\"; else cp \"$FAKE_REPORT_PLAIN\" \"$out\"; fi",
            ),
        );
        let work = dir.join("work");
        let plain = drc(&design, &model, &work, false).expect("a plain run");
        assert!(plain.schematic_parity.is_empty() && plain.parity.is_none());
        assert_eq!(plain.ignored_checks, vec![IgnoredCheck { key: "missing_courtyard".into(), description: "Footprint has no courtyard defined".into() }]);
        assert!(!work.join("board.kicad_sch").exists(), "no schematic is needed when the test is not asked for");

        let asked = drc_with(&design, &model, &work, DrcOptions { schematic_parity: true, ..Default::default() }).expect("a parity run");
        assert!(work.join("board.kicad_sch").exists(), "kicad-cli reads the schematic that sits beside the board");
        assert_eq!(asked.parity, Some(None), "it ran");
        assert_eq!(asked.schematic_parity.len(), 1);
        assert_eq!(asked.schematic_parity[0].kind, "net_conflict");
        let j = asked.to_json();
        assert_eq!(j["schematic_parity_run"], true);
        assert_eq!(j["schematic_parity"][0]["type"], "net_conflict");
        assert_eq!(j["ignored_checks"][0]["key"], "missing_courtyard");
        assert!(j.get("schematic_parity_error").is_none());

        // Asked for, with no schematic to compare with: said so, no run.
        let mut bare = design.clone();
        bare.schematic = None;
        let e = drc_with(&bare, &model, &work, DrcOptions { schematic_parity: true, ..Default::default() }).unwrap_err();
        assert_eq!(e[0].check, "kicad_cli_parity");

        // Asked for, and kicad-cli says why it cannot (no annotated schematic): the reason is the report's, and the list is empty.
        env.set("EDA_KICAD_CLI", script(&dir, "noparity.sh", "out=\"\"; prev=\"\"\nfor a in \"$@\"; do if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi; prev=\"$a\"; done\necho \"Schematic parity tests require a fully annotated schematic.\"; cp \"$FAKE_REPORT_PLAIN\" \"$out\""));
        let skipped = drc_with(&design, &model, &work, DrcOptions { schematic_parity: true, ..Default::default() }).expect("the run goes on without the test");
        assert_eq!(skipped.parity, Some(Some("Schematic parity tests require a fully annotated schematic.".to_string())));
        assert_eq!(skipped.to_json()["schematic_parity_error"], "Schematic parity tests require a fully annotated schematic.");
        assert_eq!(skipped.to_json()["schematic_parity_run"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_derived_files_are_named_after_the_project_with_safe_characters() {
        assert_eq!(file_stem("mcu_board_30plus"), "mcu_board_30plus");
        assert_eq!(file_stem("my board/v2"), "my_board_v2");
        assert_eq!(file_stem(""), "board");
        assert_eq!(file_stem(".hidden"), "board");
    }

    #[test]
    fn today_is_a_date() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
    }

    #[test]
    fn erc_json_marks_excluded_findings_and_leaves_them_out_of_the_counts() {
        let rep = ErcReport {
            engine: "kicad-cli test".into(),
            violations: vec![
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 1".into(), pos: (0, 0), id: Some("U1.1".into()), uuid: "a".into() }], ..Default::default() },
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 2".into(), pos: (0, 0), id: Some("U1.2".into()), uuid: "b".into() }], ..Default::default() },
            ],
            ignored_checks: vec![],
        };
        let j = rep.to_json(&[("pin_not_connected".into(), "U1.2".into())]);
        assert_eq!(j["violations"][0]["severity"], "error");
        assert_eq!(j["violations"][1]["severity"], "excluded");
        assert_eq!(j["counts"]["pin_not_connected"], 1);
        assert_eq!(j["violations"][0]["location"], "U1.1");
        assert_eq!(j["violations"][1]["base_severity"], "error", "an excluded finding says what it is when it is not");
        assert_eq!(j["violations"][0]["hint"], "Pin not connected: Pin 1");
    }

    #[test]
    fn scratch_dirs_are_distinct_and_removed() {
        let a = with_scratch(|d| {
            std::fs::create_dir_all(d).unwrap();
            d.to_path_buf()
        });
        let b = with_scratch(|d| d.to_path_buf());
        assert_ne!(a, b);
        assert!(!a.exists());
    }

    #[test]
    fn bom_presets_go_into_the_schematic_section_of_the_derived_project_file() {
        with_scratch(|dir| {
            std::fs::create_dir_all(dir).unwrap();
            // the project file as `write_project` leaves it: JSON with no `schematic` section yet
            std::fs::write(dir.join("p.kicad_pro"), "{\n  \"erc\": {},\n  \"meta\": { \"version\": 1 }\n}").unwrap();
            let preset = json!({ "name": "Studio export", "sort_asc": false });
            let fmt = json!({ "name": "Studio export", "field_delimiter": ";" });
            set_bom_presets(dir, "p", &preset, &fmt).unwrap();
            let pro: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("p.kicad_pro")).unwrap()).unwrap();
            assert_eq!(pro["schematic"]["bom_presets"], json!([preset]));
            assert_eq!(pro["schematic"]["bom_fmt_presets"], json!([fmt]));
            assert_eq!(pro["meta"]["version"], 1, "the rest of the project is untouched");
            // a second export replaces the presets of the first, it does not pile them up
            set_bom_presets(dir, "p", &json!({ "name": "Studio export", "sort_asc": true }), &fmt).unwrap();
            let pro: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("p.kicad_pro")).unwrap()).unwrap();
            assert_eq!(pro["schematic"]["bom_presets"].as_array().map(Vec::len), Some(1));
            assert_eq!(pro["schematic"]["bom_presets"][0]["sort_asc"], true);
        });
    }

    // ---------------------------------------------------------- the time limit

    #[test]
    fn odb_and_ipc2581_outputs_are_named_by_the_compression_asked_for() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pcb_ext_for("odb", &a(&[])), "zip");
        assert_eq!(pcb_ext_for("odb", &a(&["--compression", "zip"])), "zip");
        assert_eq!(pcb_ext_for("odb", &a(&["--compression", "tgz"])), "tgz");
        assert_eq!(pcb_ext_for("odb", &a(&["--units", "mm", "--compression", "none"])), "", "a folder: no extension");
        assert_eq!(pcb_ext_for("ipc2581", &a(&[])), "xml");
        assert_eq!(pcb_ext_for("ipc2581", &a(&["--units", "in", "--compress"])), "zip");
        assert_eq!(pcb_ext_for("step", &a(&["--compress"])), "step", "only those two are named by their options");
        assert_eq!(pcb_ext_for("gencad", &a(&[])), "cad");
    }

    #[test]
    fn the_limits_are_two_minutes_for_a_report_and_five_for_an_export() {
        assert_eq!(REPORT_TIMEOUT, Duration::from_secs(120));
        assert_eq!(EXPORT_TIMEOUT, Duration::from_secs(300));
    }

    fn board_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".kicad")).unwrap();
        dir
    }

    #[test]
    fn what_the_studios_appearance_panel_chose_reaches_the_derived_project() {
        let (design, model) = empty_design();
        let dir = board_dir("appearance");
        std::fs::write(
            dir.join("appearance.json"),
            json!({
                "version": 1,
                "local": { "hidden_layers": ["B.Cu"], "visible_items": ["tracks"], "net_color_mode": 2, "hidden_nets": ["GND"] },
                "project": {
                    "net_colors": { "GND": "rgb(1, 2, 3)" },
                    "layer_presets": [{ "name": "Mine", "activeLayer": null, "flipBoard": true, "layers": ["B.Cu"], "renderLayers": ["tracks"] }],
                    "viewports": [{ "name": "Home", "x": 1, "y": 2, "w": 3, "h": 4 }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let work = dir.join(".kicad");
        write_project(&work, "b", &design, &model).unwrap();
        let pro: Value = serde_json::from_str(&std::fs::read_to_string(work.join("b.kicad_pro")).unwrap()).unwrap();
        assert_eq!(pro["net_settings"]["net_colors"]["GND"], "rgb(1, 2, 3)");
        assert_eq!(pro["board"]["layer_presets"][0]["name"], "Mine");
        assert_eq!(pro["board"]["layer_presets"][0]["layers"], json!([2]));
        assert_eq!(pro["board"]["viewports"][0]["w"], 3000.0, "micrometres become nanometres");
        assert!(pro["board"]["design_settings"]["rules"].is_object(), "the design rules are still there");
        let prl: Value = serde_json::from_str(&std::fs::read_to_string(work.join("b.kicad_prl")).unwrap()).unwrap();
        assert_eq!(prl["board"]["visible_items"], json!(["tracks"]));
        assert_eq!(prl["board"]["visible_layers"], "ffffffff_ffffffff_ffffffff_fffffffb");
        assert_eq!(prl["board"]["net_color_mode"], 2);
        assert_eq!(prl["board"]["hidden_nets"], json!(["GND"]));
        // The studio clears everything: the next export leaves no local settings file behind.
        std::fs::write(dir.join("appearance.json"), r#"{"version":1,"local":{},"project":{}}"#).unwrap();
        write_project(&work, "b", &design, &model).unwrap();
        assert!(!work.join("b.kicad_prl").exists());
        let pro: Value = serde_json::from_str(&std::fs::read_to_string(work.join("b.kicad_pro")).unwrap()).unwrap();
        assert!(pro.get("net_settings").is_none() && pro["board"].get("layer_presets").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_appearance_file_or_a_scratch_folder_gets_the_plain_project() {
        let (design, model) = empty_design();
        // No appearance.json beside the board.
        let dir = board_dir("no-appearance");
        write_project(&dir.join(".kicad"), "b", &design, &model).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".kicad").join("b.kicad_pro")).unwrap(), export_kicad_pro_for(&design, &model));
        assert!(!dir.join(".kicad").join("b.kicad_prl").exists());
        // A scratch folder is no board's `.kicad`: whatever lies beside it is not that board's.
        let loose = board_dir("loose");
        std::fs::write(loose.join("appearance.json"), r#"{"local":{"visible_items":["tracks"]},"project":{}}"#).unwrap();
        let scratch = loose.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        write_project(&scratch, "b", &design, &model).unwrap();
        assert!(!scratch.join("b.kicad_prl").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&loose);
    }

    /// An empty design with a placement and a schematic: enough for every run to get as far as kicad-cli.
    /// Built from JSON so a field added to the IR later (with its serde default) does not break this.
    fn empty_design() -> (Design, ConstraintModel) {
        let design = serde_json::from_value(json!({
            "schema": 1,
            "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 },
            "placement": { "outline": [{ "x": 0, "y": 0 }, { "x": 10000, "y": 0 }, { "x": 10000, "y": 10000 }, { "x": 0, "y": 10000 }], "footprints": [] },
            "schematic": { "symbols": [], "wires": [] },
        }))
        .unwrap();
        (design, ConstraintModel::default())
    }

    /// A stand-in kicad-cli: answers `version`, otherwise runs `body`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = version ]; then echo 10.99.0-fake; exit 0; fi\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The first run of a new executable can take a second on macOS (the system scans it first):
        // run it once here, so a test with a short time limit is not timing that.
        let _ = Command::new(&path).arg("version").output();
        path
    }

    /// Sets environment variables for one test and puts them back however it ends. The environment
    /// is the process's, shared by every test thread, so only one of these lives at a time.
    struct Env {
        keys: Vec<&'static str>,
        _only_one: std::sync::MutexGuard<'static, ()>,
    }
    impl Env {
        fn take() -> Env {
            static ONE: Mutex<()> = Mutex::new(());
            Env { keys: vec![], _only_one: ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }
        }
        fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
            std::env::set_var(key, value);
            self.keys.push(key);
        }
    }
    impl Drop for Env {
        fn drop(&mut self) {
            for key in &self.keys {
                std::env::remove_var(key);
            }
        }
    }

    /// Whether `pid` is gone (a killed process is a zombie for a moment until its parent is reaped).
    fn gone(pid: &str) -> bool {
        let alive = || Command::new("kill").args(["-0", pid]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        let until = Instant::now() + Duration::from_secs(3);
        while alive() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        !alive()
    }

    fn timeout_of(r: Result<impl std::fmt::Debug, Vec<CheckResult>>, what: &str) -> String {
        let e = r.expect_err(what);
        assert_eq!(e[0].check, "kicad_cli_timeout", "{what}: {e:?}");
        e[0].hint.clone().unwrap()
    }

    #[test]
    fn a_kicad_cli_that_hangs_is_killed_with_a_clear_error_for_a_drc_an_erc_and_every_export() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_engine_timeout_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (design, model) = empty_design();
        let mut env = Env::take();
        env.set(TIMEOUT_ENV, "1");
        // Says something, starts a helper (like a real kicad-cli might) and waits for it for two minutes.
        env.set("EDA_KICAD_CLI", script(&dir, "hangs.sh", "echo \"still thinking\" >&2\nsleep 120 &\necho $! > \"$FAKE_KICAD_PID\"\nwait"));
        let pid_file = dir.join("pid");
        env.set("FAKE_KICAD_PID", &pid_file);
        let work = dir.join("work");
        let helper = || std::fs::read_to_string(&pid_file).unwrap().trim().to_string();

        let started = Instant::now();
        let said = timeout_of(drc(&design, &model, &work, false), "a hung DRC");
        assert!(said.contains("kicad-cli pcb drc did not finish within 1 s and was killed"), "{said}");
        assert!(said.contains("still thinking"), "what kicad-cli said before it was killed is kept: {said}");
        assert!(said.contains(TIMEOUT_ENV), "the message says how to give a slow run more time: {said}");
        assert!(gone(&helper()), "the kill reaches what kicad-cli started, not only kicad-cli");

        let said = timeout_of(erc(&design, &model, &work), "a hung ERC");
        assert!(said.contains("kicad-cli sch erc did not finish within 1 s"), "{said}");
        assert!(gone(&helper()));
        let said = timeout_of(export_pcb(&design, &model, &work, &dir, "board", "gerbers", &["--layers".into(), "F.Cu".into()]), "hung Gerbers");
        assert!(said.contains("kicad-cli pcb export gerbers did not finish within 1 s"), "{said}");
        let said = timeout_of(export_sch(&design, &model, &work, &dir, "board", "bom", &[]), "a hung BOM");
        assert!(said.contains("kicad-cli sch export bom did not finish"), "{said}");
        let said = timeout_of(stats(&design, &model, &work, StatsOptions::default(), true), "hung statistics");
        assert!(said.contains("kicad-cli pcb export stats did not finish"), "{said}");
        assert!(gone(&helper()));
        // Five killed runs, each after its second: a hang costs the limit, not minutes.
        assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());

        // The limit is only for a run that does not finish: a quick kicad-cli is answered as ever.
        env.set("EDA_KICAD_CLI", script(&dir, "quick.sh", "out=\"\"; prev=\"\"\nfor a in \"$@\"; do if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi; prev=\"$a\"; done\necho '{\"coordinate_units\":\"mm\",\"kicad_version\":\"10.99.0-fake\",\"violations\":[],\"unconnected_items\":[]}' > \"$out\""));
        let report = drc(&design, &model, &work, false).expect("a DRC that finishes in time");
        assert_eq!(report.engine, "kicad-cli 10.99.0-fake");
        assert!(report.violations.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_within_keeps_what_a_chatty_process_writes_and_does_not_stall_on_a_full_pipe() {
        // 300 kB on each stream is far more than a pipe holds: undrained, the child would block on its own write.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 300000 /dev/zero; head -c 300000 /dev/zero >&2; exit 3"]);
        let out = output_within(cmd, Duration::from_secs(60)).expect("it finishes by itself");
        assert_eq!((out.stdout.len(), out.stderr.len(), out.status.code()), (300_000, 300_000, Some(3)));
    }

    #[test]
    fn a_limit_from_the_environment_must_be_a_positive_number_of_seconds() {
        let mut env = Env::take();
        for (value, expected) in [("2", 2.0), ("0.25", 0.25), (" 30 ", 30.0), ("0", 9.0), ("-1", 9.0), ("soon", 9.0), ("inf", 9.0), ("", 9.0)] {
            env.set(TIMEOUT_ENV, value);
            assert_eq!(limit(Duration::from_secs(9)), Duration::from_secs_f64(expected), "{value:?}");
        }
        std::env::remove_var(TIMEOUT_ENV);
        assert_eq!(limit(Duration::from_secs(9)), Duration::from_secs(9), "unset: the default");
    }
}
