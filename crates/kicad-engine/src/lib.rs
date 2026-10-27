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
//! position files, STEP, netlist and BOM all come from kicad-cli, and so does
//! reading a board or a schematic of another EDA format ([`foreign`]:
//! `kicad-cli pcb import` and `sch import`). Nothing in this workspace
//! re-implements any of them.
//!
//! The scratch directory is the caller's choice. The studio and the CLI
//! pass `<board dir>/.kicad` (kept for inspection); the gates, which work
//! on in-memory designs, use [`drc_scratch`] (a fresh temp directory,
//! removed afterwards).

pub mod footprint_check;
// Other EDA formats in: `kicad-cli pcb import` and `kicad-cli sch import` (docs/ARCHITECTURE.md, "Engines").
pub mod foreign;
pub use foreign::{import_foreign_pcb, import_foreign_sch, ForeignImport, ForeignOptions};

use eda_kicad::zone_fills::KicadFills;
use eda_kicad::{export_kicad_pcb_mapped_with, export_kicad_pro_for, export_kicad_sch_tree_mapped, ExportMeta};
use eda_model::ir::{Design, DrcExclusion, ErcExclusion, Point};
use eda_model::units::Nm;
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

// ------------------------------------------------------------- KiCad's own settings

/// A `kicad-cli` command that reads none of the user's KiCad settings.
///
/// kicad-cli loads the settings of the KiCad it belongs to (`~/Library/Preferences/kicad/<version>` on macOS, `~/.config/kicad/<version>` on
/// Linux): the global library tables, the path variables, the colour themes and every editor's defaults. A DRC, an ERC or an export of a derived
/// project would then answer differently from one person's KiCad to the next, and loading the global tables costs a run seconds (a DRC of a
/// 30-part board took 6 s with them and 1.5 s without, 2026-10-13). So every run gets a settings folder of this program's through
/// `KICAD_CONFIG_HOME` (`SETTINGS_MANAGER::calculateUserSettingsPath`, which appends the version): KiCad writes its defaults there on first use,
/// and nothing else writes to it.
///
/// That folder has no global library table: the derived files carry their footprints and symbols, and 3D models resolve through the path
/// variables KiCad sets from its own install (`${KICAD10_3DMODEL_DIR}`). The library checks (`lib_footprint_issues`, `lib_footprint_mismatch`,
/// `lib_symbol_issues`, `lib_symbol_mismatch`, `footprint_link_issues`) are the one use of a table, and every derived project sets them to `ignore` unless the design
/// says otherwise; when `project` (the `.kicad_pro` the run reads) turns one on, the run gets a second folder whose tables are a fresh install's:
/// one entry each, naming the install's default table, as KiCad's first start writes them.
///
/// `EDA_KICAD_CONFIG_HOME` moves the folders (default: the system's temporary directory); `EDA_KICAD_USER_CONFIG=1` gives kicad-cli the user's
/// settings back.
pub fn cli_command(cli: &Path, project: Option<&Path>) -> Command {
    let mut cmd = Command::new(cli);
    if std::env::var_os("EDA_KICAD_USER_CONFIG").is_some() {
        return cmd;
    }
    let libraries = project.is_some_and(library_checks_on);
    if let Some(dir) = settings_home(cli, libraries) {
        cmd.env("KICAD_CONFIG_HOME", dir);
    }
    cmd
}

/// Whether `project` turns a library check on: a severity other than `ignore` for one of them. A project that does not name them (a scratch
/// project of this program's, such as the Footprint Checker's, which reads only its own checks) counts as leaving them off.
fn library_checks_on(project: &Path) -> bool {
    let Ok(pro) = std::fs::read_to_string(project).map(|t| serde_json::from_str::<Value>(&t).unwrap_or(Value::Null)) else { return false };
    let on = |severities: &Value, key: &str| severities.get(key).and_then(Value::as_str).is_some_and(|s| s != "ignore");
    let board = &pro["board"]["design_settings"]["rule_severities"];
    let schematic = &pro["erc"]["rule_severities"];
    ["lib_footprint_issues", "lib_footprint_mismatch"].iter().any(|k| on(board, k)) || ["lib_symbol_issues", "lib_symbol_mismatch", "footprint_link_issues"].iter().any(|k| on(schematic, k))
}

/// The settings folder of [`cli_command`]: `<base>/empty`, or `<base>/libraries` with a fresh install's global library tables in it.
fn settings_home(cli: &Path, libraries: bool) -> Option<PathBuf> {
    let base = std::env::var_os("EDA_KICAD_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("eda-kicad-config"));
    let dir = base.join(if libraries { "libraries" } else { "empty" });
    std::fs::create_dir_all(&dir).ok()?;
    if libraries {
        seed_library_tables(cli, &dir);
    }
    Some(dir)
}

/// The global `fp-lib-table` and `sym-lib-table` a fresh install of the KiCad that `cli` belongs to starts with: each names the install's own
/// default table (`template/fp-lib-table`, `template/sym-lib-table`) as one library of type `Table`. Written once per settings version; a KiCad
/// whose templates cannot be found gets none, and its library checks report what they find.
fn seed_library_tables(cli: &Path, home: &Path) {
    let version = settings_version(&cli_version(cli));
    let Some(template) = template_dir(cli) else { return };
    let dir = home.join(&version);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (file, head) in [("fp-lib-table", "fp_lib_table"), ("sym-lib-table", "sym_lib_table")] {
        let target = dir.join(file);
        let source = template.join(file);
        if target.exists() || !source.exists() {
            continue;
        }
        let text = format!(
            "({head}\n\t(version 7)\n\t(lib (name \"KiCad\") (type \"Table\") (uri \"{}\") (options \"\") (descr \"KiCad Default Libraries\"))\n)\n",
            source.display()
        );
        // Written beside the target and renamed into place, so a run that starts meanwhile never reads half a table.
        let tmp = dir.join(format!(".{file}.{}", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &target).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// The settings version KiCad keeps its settings under: the major and minor of `kicad-cli version` ("10.99.0" is "10.99", "9.0.4" is "9.0").
fn settings_version(version: &str) -> String {
    let mut parts = version.trim().split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) if !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()) => format!("{major}.{}", minor.chars().take_while(char::is_ascii_digit).collect::<String>()),
        _ => "0.0".to_string(),
    }
}

/// The install's `template` folder, where its default library tables are: beside `Contents/MacOS` in a macOS bundle
/// (`KiCad.app/Contents/SharedSupport/template`), under `share/kicad` beside `bin` elsewhere (`/usr/share/kicad/template`).
fn template_dir(cli: &Path) -> Option<PathBuf> {
    let cli = std::fs::canonicalize(cli).unwrap_or_else(|_| cli.to_path_buf());
    let parent = cli.parent()?;
    let up = parent.parent()?;
    [up.join("SharedSupport/template"), up.join("share/kicad/template")].into_iter().find(|d| d.join("fp-lib-table").exists())
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-settings-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_settings_version_is_the_major_and_minor() {
        assert_eq!(settings_version("10.99.0"), "10.99");
        assert_eq!(settings_version("9.0.4"), "9.0");
        assert_eq!(settings_version("10.0.0-rc1"), "10.0");
        assert_eq!(settings_version(""), "0.0");
    }

    #[test]
    fn a_project_turns_the_library_checks_on_only_by_naming_one() {
        let d = scratch("pro");
        let write = |name: &str, v: Value| {
            let p = d.join(name);
            std::fs::write(&p, v.to_string()).unwrap();
            p
        };
        let derived = write("derived.kicad_pro", json!({
            "board": { "design_settings": { "rule_severities": { "lib_footprint_issues": "ignore", "lib_footprint_mismatch": "ignore", "clearance": "error" } } },
            "erc": { "rule_severities": { "lib_symbol_issues": "ignore", "lib_symbol_mismatch": "ignore" } },
        }));
        assert!(!library_checks_on(&derived), "every derived project ignores them by default");
        let board_on = write("board.kicad_pro", json!({ "board": { "design_settings": { "rule_severities": { "lib_footprint_mismatch": "warning" } } } }));
        assert!(library_checks_on(&board_on));
        let sch_on = write("sch.kicad_pro", json!({ "erc": { "rule_severities": { "lib_symbol_mismatch": "error" } } }));
        assert!(library_checks_on(&sch_on));
        let link_on = write("link.kicad_pro", json!({ "erc": { "rule_severities": { "footprint_link_issues": "warning" } } }));
        assert!(library_checks_on(&link_on), "the Footprint fields' libraries are the global table's");
        let scratch_project = write("scratch.kicad_pro", json!({ "board": { "design_settings": { "rule_severities": { "malformed_courtyard": "error" } } } }));
        assert!(!library_checks_on(&scratch_project), "a project that does not name them leaves them off");
        assert!(!library_checks_on(&d.join("missing.kicad_pro")));
    }

    #[test]
    fn the_install_template_folder_is_found_in_a_mac_bundle_and_a_unix_tree() {
        let d = scratch("tpl");
        let mac = d.join("KiCad.app/Contents");
        std::fs::create_dir_all(mac.join("MacOS")).unwrap();
        std::fs::create_dir_all(mac.join("SharedSupport/template")).unwrap();
        std::fs::write(mac.join("SharedSupport/template/fp-lib-table"), "(fp_lib_table)").unwrap();
        std::fs::write(mac.join("MacOS/kicad-cli"), "").unwrap();
        assert_eq!(template_dir(&mac.join("MacOS/kicad-cli")).map(|p| p.ends_with("SharedSupport/template")), Some(true));
        let unix = d.join("usr");
        std::fs::create_dir_all(unix.join("bin")).unwrap();
        std::fs::create_dir_all(unix.join("share/kicad/template")).unwrap();
        std::fs::write(unix.join("share/kicad/template/fp-lib-table"), "(fp_lib_table)").unwrap();
        std::fs::write(unix.join("bin/kicad-cli"), "").unwrap();
        assert_eq!(template_dir(&unix.join("bin/kicad-cli")).map(|p| p.ends_with("share/kicad/template")), Some(true));
        assert_eq!(template_dir(&d.join("nowhere/kicad-cli")), None);
    }

    #[test]
    fn every_run_gets_a_settings_folder_of_its_own() {
        // Through the environment, as a test cannot know where the system keeps its temporary files for another process.
        let cli = PathBuf::from("/nonexistent/kicad-cli");
        let cmd = cli_command(&cli, None);
        let home = cmd.get_envs().find(|(k, _)| *k == "KICAD_CONFIG_HOME").and_then(|(_, v)| v).map(PathBuf::from);
        if std::env::var_os("EDA_KICAD_USER_CONFIG").is_some() {
            assert!(home.is_none());
        } else {
            let home = home.expect("KICAD_CONFIG_HOME is set");
            assert!(home.ends_with("empty") && home.is_dir(), "{}", home.display());
        }
    }
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
        let mut cmd = cli_command(cli, None);
        cmd.arg("version");
        output_within(cmd, limit(VERSION_TIMEOUT)).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    })
}

/// The kinds `kicad-cli <scope> export` has, as its `--help` lists them: the choices in braces on the usage line (`Usage: pcb export [--help]
/// {3dpdf,bom,...,specctra,...}`), else the first word of each line under "Subcommands:" (the help of an argparse without the brace list). A
/// kicad-cli that does not answer has none.
pub fn parse_export_kinds(help: &str) -> Vec<String> {
    let usage = help.lines().find(|l| l.trim_start().starts_with("Usage:")).unwrap_or("");
    if let (Some(a), Some(z)) = (usage.find('{'), usage.rfind('}')) {
        if a < z {
            return usage[a + 1..z].split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        }
    }
    help.lines()
        .skip_while(|l| l.trim() != "Subcommands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

/// Whether `kicad-cli <scope> export <kind>` exists in the kicad-cli at `cli`: one `kicad-cli <scope> export --help` per binary and scope per run
/// of this program, its answer kept like [`cli_version`]'s. Not every kicad-cli has every exporter (`pcb export specctra` came into the 10.99
/// nightlies after 10.99.0-5334), so a caller with a fallback of its own asks first.
pub fn cli_has_export(cli: &Path, scope: &str, kind: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), Vec<String>>>> = OnceLock::new();
    let m = CACHE.get_or_init(Default::default);
    let key = (cli.to_path_buf(), scope.to_string());
    let cached = m.lock().ok().and_then(|g| g.get(&key).cloned());
    let kinds = cached.unwrap_or_else(|| {
        let mut cmd = cli_command(cli, None);
        cmd.args([scope, "export", "--help"]);
        let kinds = output_within(cmd, limit(VERSION_TIMEOUT)).map(|o| parse_export_kinds(&format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))).unwrap_or_default();
        if let Ok(mut g) = m.lock() {
            g.insert(key, kinds.clone());
        }
        kinds
    });
    kinds.iter().any(|k| k == kind)
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
/// hard-coded floors; with the board's net classes in the project file when [`export_board`] moved them there (a class with a colour: see
/// `eda_kicad::appearance`), and always for a schematic ([`export_schematic`]).
fn write_project_with(work: &Path, stem: &str, design: &Design, model: &ConstraintModel, classes: Option<&eda_kicad::appearance::SplitClasses>) -> Result<(), Vec<CheckResult>> {
    let mut pro_text = export_kicad_pro_for(design, model);
    // What the person chose in the studio's Appearance panel (layers and objects shown, colours, presets, views) goes into the derived project as KiCad keeps it:
    // the net colours, layer presets and viewports in the project file, the rest in the project's local settings (`<stem>.kicad_prl`).
    // (a net it names under the intent's name is the net's under the board's: `with_board_net_names`)
    let appearance = studio_appearance(work).map(|a| eda_kicad::appearance::with_board_net_names(&a, model));
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
    // The board's library tables and path variables, so kicad-cli reads the libraries the studio does (`eda_kicad::library_table::derive_project`). Only the
    // studio's own `<board>/.kicad` has a board behind it; the gates' scratch folders and the tests' have no tables.
    if let Some(board_dir) = work.file_name().filter(|n| *n == ".kicad").and(work.parent()) {
        if let Ok(mut pro) = serde_json::from_str::<Value>(&pro_text) {
            let derived = eda_kicad::library_table::derive_project(board_dir, work, &mut pro).map_err(|e| fail("kicad_engine_write", &work.display().to_string(), e))?;
            if derived {
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
    // The simulator's workbook, under the name the project's `schematic.ngspice.workbook_filename` gives it (`SIMULATOR_FRAME_UI::InitWorkbook` reads it from there).
    if let Some(sch) = design.schematic.as_ref() {
        if let (Some(text), Some(ng)) = (sch.extras.sim_workbook.as_ref(), sch.extras.ngspice.as_ref()) {
            let name = Path::new(&ng.workbook_filename);
            if !ng.workbook_filename.is_empty() && name.is_relative() && name.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
                write_file(&work.join(name), text)?;
            }
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
pub fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    if s.is_empty() || s.starts_with('.') { "board".to_string() } else { s }
}

/// The fills kicad-cli made of the board whose derived files are written into `work` (`eda_kicad::zone_fills`): found only when `work` is that board's `.kicad`
/// folder, where they are kept (the scratch folders of the gates and of the tests belong to no board and get none).
fn studio_zone_fills(work: &Path) -> Option<KicadFills> {
    if work.file_name().and_then(|n| n.to_str()) != Some(".kicad") {
        return None;
    }
    eda_kicad::zone_fills::load(work.parent()?)
}

/// Export the design as `<stem>.kicad_pcb` (+ project); returns the pcb path
/// and the uuid -> our id map. The zones carry the fill kicad-cli made of the board (Fill All Zones, kept beside `design.json`) for as long as they are the zones it
/// filled; a board without one has its zones unfilled.
fn export_board(design: &Design, model: &ConstraintModel, work: &Path, stem: &str) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    let cache = studio_zone_fills(work);
    export_board_with(design, model, work, stem, cache.as_ref())
}

/// [`export_board`] with the fills of the zones given.
fn export_board_with(design: &Design, model: &ConstraintModel, work: &Path, stem: &str, fills: Option<&KicadFills>) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    work_dir(work)?;
    let date = today();
    let (pcb, map) = export_kicad_pcb_mapped_with(design, model, &ExportMeta { date: &date, title: stem }, fills)?;
    // A net class with a colour (Appearance > Net Classes) can only reach KiCad in the project file, and KiCad reads the project's classes only when the board has
    // none of its own: so then the classes move there whole, and kicad-cli judges the board by the same numbers (`eda_kicad::appearance::split_net_classes`).
    let class_colors = studio_appearance(work).map(|a| eda_kicad::appearance::class_colors(&a)).unwrap_or_default();
    let split = if class_colors.is_empty() { None } else { eda_kicad::appearance::split_net_classes(&pcb, &class_colors) };
    let path = work.join(format!("{stem}.kicad_pcb"));
    write_file(&path, split.as_ref().map_or(pcb.as_str(), |s| s.pcb.as_str()))?;
    write_project_with(work, stem, design, model, split.as_ref())?;
    Ok((path, map))
}

/// Export the design's schematic as `<stem>.kicad_sch` (+ project), and one file for every screen of a hierarchical design, at its name from the root's
/// folder (a screen's key: `ampli_ht/filter.kicad_sch` is written in `ampli_ht/`, and its placing file names it relative to its own folder), so kicad-cli
/// reads the whole tree.
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
        // a screen in a subfolder of the root's (QA `complex_hierarchy_shared`'s `ampli_ht/`): the folder is made first
        if let Some(parent) = work.join(name).parent().filter(|p| *p != work) {
            std::fs::create_dir_all(parent).map_err(|e| fail("kicad_engine_write", &parent.display().to_string(), e.to_string()))?;
        }
        write_file(&work.join(name), text)?;
    }
    // A schematic's project has no board to hold the net classes, so they are in the project file (`net_settings.classes`, `netclass_patterns`), where KiCad keeps
    // them: its ERC knows the classes a label names (`TestMissingNetclasses`) and its netlist puts a net in its class.
    let classes = project_net_classes(model);
    write_project_with(work, stem, design, model, Some(&classes))?;
    Ok((work.join(&root_name), map))
}

/// The board's net classes in the project file's form (`NETCLASS` in `net_settings.classes`, mm): each class with its name, its precedence (`priority`, the
/// board's order; Default last) and the numbers it sets, and each of its net patterns as a `netclass_patterns` entry.
fn project_net_classes(model: &ConstraintModel) -> eda_kicad::appearance::SplitClasses {
    let mm = eda_model::units::to_mm;
    let mut classes = vec![json!({ "name": "Default", "priority": i64::from(i32::MAX) })];
    let mut patterns = Vec::new();
    for (i, c) in model.board.net_classes.iter().filter(|c| c.name != "Default").enumerate() {
        let mut class = serde_json::Map::new();
        class.insert("name".into(), json!(c.name));
        class.insert("priority".into(), json!(i));
        for (key, v) in [("track_width", c.track_width), ("clearance", c.clearance), ("via_diameter", c.via_diameter), ("via_drill", c.via_drill), ("microvia_diameter", c.microvia_diameter), ("microvia_drill", c.microvia_drill), ("diff_pair_width", c.diff_pair_width), ("diff_pair_gap", c.diff_pair_gap)] {
            if let Some(v) = v {
                class.insert(key.into(), json!(mm(v)));
            }
        }
        classes.push(Value::Object(class));
        patterns.extend(c.nets.iter().map(|n| json!({ "netclass": c.name, "pattern": n })));
    }
    eda_kicad::appearance::SplitClasses { pcb: String::new(), classes, patterns }
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
    let mut cmd = cli_command(&cli, None);
    cmd.args(["pcb", "export", "vrml", "--units", "mm", "--models-dir", "m", "-f", "-o"]).arg(work.join("models.wrl")).arg(&board);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(fail("kicad_cli_export", "pcb export vrml", format!("kicad-cli pcb export vrml failed: {}", said.trim())));
    }
    Ok(models.iter().map(|m| vrml_named(&models_dir, m)).collect())
}

// --------------------------------------------------------------------- reports

/// One item a violation names: kicad-cli's own description and position,
/// plus our id for it (`None` when it is not one of ours, e.g. a zone
/// fill KiCad computed).
#[derive(Debug, Clone)]
pub struct Item {
    pub description: String,
    pub pos: Point,
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
    /// Where the violation's marker may sit: the positions worth writing an exclusion for ([`marker_candidates`]).
    /// kicad-cli's report does not give the marker's position, only its items'.
    pub markers_nm: Vec<Point>,
    /// An ERC violation's sheet: the path (`ERC_SHEET::uuid_path`) of the sheet kicad-cli's report lists it under. Empty for a DRC one.
    pub sheet: String,
    /// An ERC violation's marker as KiCad keeps an exclusion (`SCH_MARKER`, `kiapi.schematic.ErcMarker`), when the report says enough to write it to the
    /// letter ([`eda_kicad::erc_exclusions::marker`]); `None` when it does not, and for a DRC violation.
    pub marker: Option<eda_model::ir::ErcMarker>,
}

impl Violation {
    fn from_json(v: &Value, map: &HashMap<String, String>, units_mm: bool) -> Violation {
        let to_nm = |x: f64| Nm::kiround(if units_mm { x * 1_000_000.0 } else { x * 25_400_000.0 });
        let pos = |it: &Value| Point { x: to_nm(it["pos"]["x"].as_f64().unwrap_or(0.0)), y: to_nm(it["pos"]["y"].as_f64().unwrap_or(0.0)) };
        let items = v["items"]
            .as_array()
            .map(|is| {
                is.iter()
                    .map(|it| {
                        let uuid = it["uuid"].as_str().unwrap_or("").to_string();
                        Item {
                            description: it["description"].as_str().unwrap_or("").to_string(),
                            pos: pos(it),
                            id: map.get(&uuid).cloned(),
                            uuid,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let at_nm: Vec<Point> = v["items"].as_array().map(|is| is.iter().map(pos).collect()).unwrap_or_default();
        Violation {
            kind: v["type"].as_str().unwrap_or("?").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            severity: v["severity"].as_str().unwrap_or("error").to_string(),
            items,
            excluded: v["excluded"].as_bool().unwrap_or(false),
            comment: v["comment"].as_str().unwrap_or("").to_string(),
            markers_nm: marker_candidates(&at_nm),
            sheet: String::new(),
            marker: None,
        }
    }

    /// The items of an ERC violation as a marker names them: uuid, position in nanometres and name. The schematic's internal unit is a nanometre in the
    /// nightly the studio runs (10.99.0-5334) and its ERC report prints millimetres as they are; the nightly before it printed a hundredth of that
    /// (`ERC_REPORT::WriteJsonReport` built its `UNITS_PROVIDER` on the board's scale while the schematic's unit was 100 nm). Nothing when the report is
    /// not in millimetres.
    fn marker_items(v: &Value, units_mm: bool) -> Vec<eda_kicad::erc_exclusions::MarkerItem> {
        if !units_mm {
            return Vec::new();
        }
        let nm = |x: f64| Nm::kiround(x * 1_000_000.0);
        v["items"]
            .as_array()
            .map(|is| {
                is.iter()
                    .map(|it| eda_kicad::erc_exclusions::MarkerItem {
                        uuid: it["uuid"].as_str().unwrap_or("").to_string(),
                        x_nm: nm(it["pos"]["x"].as_f64().unwrap_or(0.0)),
                        y_nm: nm(it["pos"]["y"].as_f64().unwrap_or(0.0)),
                        description: it["description"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The studio's DRC shape: `type`, `description`, `severity`, `items`
    /// (each with a `[x, y]` nm `pos`, our `id` or `null`, the KiCad `uuid`), `excluded` and its `comment`, and `marker_nm`: where the
    /// violation's marker may sit, in nanometres, which an exclusion keeps (see [`marker_candidates`]).
    pub fn to_json(&self) -> Value {
        json!({
            "type": self.kind,
            "description": self.description,
            "severity": self.severity,
            "items": self.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.x, i.pos.y], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
            "excluded": self.excluded,
            "comment": self.comment,
            "marker_nm": self.markers_nm.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>(),
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
pub fn marker_candidates(items: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = items.to_vec();
    if let [a, b, ..] = items {
        out.push(Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The ends and the middle of one segment of the track `id` names (`<track id>` for the first segment, `<track id>#<n>` for the n-th), in
/// nanometres. Empty for an id that is not a track's. The middle is KiCad's `(start + end) / 2` on integers.
fn track_anchors(design: &Design, id: &str) -> Vec<Point> {
    let (base, n) = match id.rsplit_once('#') {
        Some((b, n)) if n.parse::<usize>().is_ok() => (b, n.parse::<usize>().unwrap_or(0)),
        _ => (id, 0),
    };
    let Some(track) = design.routing.as_ref().and_then(|r| r.tracks.iter().find(|t| t.id == base)) else {
        return Vec::new();
    };
    let (Some(a), Some(b)) = (track.pts.get(n), track.pts.get(n + 1)) else {
        return Vec::new();
    };
    vec![*a, *b, Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }]
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
    /// The fills kicad-cli made when the run refilled the zones first (`DrcOptions::refill_zones`): the caller keeps them beside the design.
    pub zone_fills: Option<KicadFills>,
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
    /// "Refill all zones before performing DRC": see [`drc`].
    pub refill_zones: bool,
    /// "Test for parity between PCB and schematic" (`--schematic-parity`): the derived schematic goes next to the derived board, where kicad-cli looks for
    /// it, and the report's `schematic_parity` holds what differs.
    pub schematic_parity: bool,
}

/// `kicad-cli pcb drc` on `design` as it is.
///
/// The DRC judges the fills the exported board carries: the ones kicad-cli made when the zones were last filled (Fill All Zones, kept beside the design), or none for
/// a zone that was never filled, as in KiCad. `refill_zones` (KiCad's "Refill all zones before performing DRC") fills them first: kicad-cli's filler runs on a copy
/// of the board ([`refill_zones`]) and the DRC then runs on the board with that fill in it, without `--refill-zones`, so the fill it judges is the one that comes
/// back in [`DrcReport::zone_fills`] for the caller to keep (the canvas, the plots and the next DRC see the same copper). (An earlier note here said a run that
/// refills skips the courtyard checks; it does not on kicad-cli 25ec2335: `fill_bad` with one footprint moved onto the other reports `courtyards_overlap` either way.) A
/// kicad-cli that cannot refill (9.0) gives the DRC of the board as it is. Every kicad-cli start costs a third of a second, so this neither asks for its version nor
/// probes its options: the version comes off the report.
pub fn drc(design: &Design, model: &ConstraintModel, work: &Path, refill_zones: bool) -> Result<DrcReport, Vec<CheckResult>> {
    drc_with(design, model, work, DrcOptions { refill_zones, schematic_parity: false })
}

/// [`drc`] with options. The violations the design waived (`design.drawings.drc_exclusions`) are in the derived project
/// ([`eda_kicad::export_kicad_pro_for`]), so kicad-cli flags the ones it can match as `excluded`; the report asks for every severity,
/// exclusions included, and each [`Violation`] says whether it is one.
pub fn drc_with(design: &Design, model: &ConstraintModel, work: &Path, opts: DrcOptions) -> Result<DrcReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    // "Refill all zones before performing DRC": kicad-cli's filler runs first, on a copy of the board (`refill_zones`), and the DRC then judges the board with that fill
    // in it -- not `--refill-zones` on the DRC run itself, whose fill would be one nobody keeps. A kicad-cli that cannot refill (9.0) gives the DRC of the board as it is.
    let refilled = if opts.refill_zones {
        match refill_zones(design, model, work) {
            Ok(fills) => Some(fills),
            Err(e) if e.iter().any(|c| c.check == "kicad_cli_refill") => None,
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    let cache = if refilled.is_none() { studio_zone_fills(work) } else { None };
    let (pcb, map) = export_board_with(design, model, work, "board", refilled.as_ref().or(cache.as_ref()))?;
    if opts.schematic_parity {
        // kicad-cli reads the schematic next to the board (`<board stem>.kicad_sch`) to make the netlist the parity test compares with.
        if design.schematic.is_none() {
            return Err(fail("kicad_cli_parity", "schematic", "the parity test compares the board with the schematic, and this design has none"));
        }
        export_schematic(design, model, work, "board")?;
    }
    let report = work.join("drc.json");
    // No `--refill-zones` on this run: the fills it judges are the ones the board carries (above), which the caller keeps.
    let mut cmd = cli_command(&cli, Some(&pcb.with_extension("kicad_pro")));
    cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]);
    if opts.schematic_parity {
        cmd.arg("--schematic-parity");
    }
    cmd.arg("-o").arg(&report).arg(&pcb);
    let (raw, said) = run_report_with_output(cmd, &report, "drc")?;
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
        zones_refilled_by_kicad: refilled.is_some(),
        zone_fills: refilled,
        violations: read("violations"),
        unconnected_items: read("unconnected_items"),
        schematic_parity: if opts.schematic_parity { read("schematic_parity") } else { Vec::new() },
        parity,
        ignored_checks: raw["ignored_checks"].as_array().map(|cs| cs.iter().map(IgnoredCheck::from_json).collect()).unwrap_or_default(),
    })
}

/// Fill All Zones (B) by KiCad's own filler: `kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board (`refill/refill.kicad_pcb` in
/// `work`, so the board the other runs read is left as it is), then the `(filled_polygon ..)` of every zone read back from the board kicad-cli
/// saved. There is no other filler: this is what a fill is. The fills are returned for the design as it is now, each zone's with the fingerprint of the zone it was
/// made for (`eda_kicad::zone_fills::zone_signature`); keeping them is the caller's (the studio keeps them beside `design.json`, `eda_kicad::zone_fills::store`).
/// A board with no zone to fill does not start kicad-cli.
///
/// The copy starts with no fill at all (kicad-cli replaces every one), and its project ignores every DRC check: the refill is all this run is for. Its DRC report is not
/// looked at.
pub fn refill_zones(design: &Design, model: &ConstraintModel, work: &Path) -> Result<KicadFills, Vec<CheckResult>> {
    let cli = need_cli()?;
    let engine = format!("kicad-cli {}", cli_version(&cli));
    if !design.routing.as_ref().is_some_and(|r| r.zones.iter().any(eda_kicad::zone_fills::fillable)) {
        return Ok(KicadFills::new(engine, String::new(), 0, Default::default()));
    }
    let started = Instant::now();
    let stem = "refill";
    // The copy lives in a folder of its own, with the project's rules but none of the board's library tables, and its project ignores every check, the library checks
    // too, so [`cli_command`] gives the run the empty settings folder: a DRC job does not start before the footprint libraries are enumerated (every `.kicad_mod` of every
    // library of the global table is parsed, 5 s and more of a 7 s run) and a fill needs none of them. Measured on a 30-part board with a GND pour on both layers: 8.1 s
    // with the user's libraries, 1.6 s without, the same fill.
    let scratch = work.join("refill");
    let (pcb, map) = export_board_with(design, model, &scratch, stem, None)?;
    let project = scratch.join(format!("{stem}.kicad_pro"));
    ignore_every_check(&project)?;
    let report = scratch.join("refill.json");
    let _ = std::fs::remove_file(&report);
    let mut cmd = cli_command(&cli, Some(&project));
    cmd.args(["pcb", "drc", "--refill-zones", "--save-board", "--format", "json", "--units", "mm", "-o"]).arg(&report).arg(&pcb);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // An old kicad-cli does not know the options. One that does has written the refilled board over the copy: a file of its own making, not the one we wrote.
    let refilled = std::fs::read_to_string(&pcb).map_err(|e| fail("kicad_cli_refill", &pcb.display().to_string(), e.to_string()))?;
    if !out.status.success() || refilled.contains("(generator \"eda-kicad\")") {
        let why = said.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("it printed nothing");
        return Err(fail("kicad_cli_refill", "kicad-cli", format!("kicad-cli could not refill the zones (it needs --refill-zones and --save-board): {why}")));
    }
    // The teardrops come back without the uuid they were written with (`format( const ZONE* )` writes none for one): they are found by their layer and outline.
    let mut map = map;
    map.extend(eda_kicad::zone_fills::teardrop_keys(design));
    let zones = eda_kicad::zone_fills::entries_by_zone(design, eda_kicad::zone_fills::scan_zone_fills(&refilled), &map);
    Ok(KicadFills::new(engine, String::new(), started.elapsed().as_millis() as u64, zones))
}

/// Every DRC check set to `ignore` in a derived project, so kicad-cli runs none of them.
fn ignore_every_check(project: &Path) -> Result<(), Vec<CheckResult>> {
    let text = std::fs::read_to_string(project).map_err(|e| fail("kicad_engine_write", &project.display().to_string(), e.to_string()))?;
    let mut pro: Value = serde_json::from_str(&text).map_err(|e| fail("kicad_engine_write", &project.display().to_string(), e.to_string()))?;
    pro["board"]["design_settings"]["rule_severities"] = Value::Object(eda_model::rules::DRC_CHECKS.iter().map(|(key, _)| (key.to_string(), Value::String("ignore".into()))).collect());
    write_file(project, format!("{}\n", serde_json::to_string_pretty(&pro).unwrap_or_default()))
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
    /// The violations the studio lists: kicad-cli's, without the markers its own exclusions made that are not violations. KiCad turns an exclusion
    /// that matches no marker into an excluded marker of its own (`SCHEMATIC::ResolveERCExclusions`), so the report can hold an excluded entry
    /// that no check found: one of an item that is gone (no items), a copy of a violation whose own marker the exclusion did not match (the same
    /// items, and the real one listed beside it, not excluded), or a second marker for one. What is left of those is the one stale marker KiCad
    /// would show -- an exclusion whose violation is gone -- and a check set to Ignore has none. A violation is told by its marker's sheet too
    /// (`SetSheetSpecificPath`): the two placements of a screen two sheets show name the same items, and the excluded one is not a copy of the
    /// other. (Not by the sheet the report lists it under: kicad-cli lists the stray marker of a check that belongs to no sheet under another one.)
    fn listed(&self) -> Vec<&Violation> {
        let key = |v: &Violation| (v.kind.clone(), v.items.iter().map(|i| i.uuid.clone()).collect::<Vec<_>>(), v.marker.as_ref().map(|m| m.sheet.clone()).unwrap_or_default());
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for v in &self.violations {
            if v.excluded {
                let k = key(v);
                let ignored = self.ignored_checks.iter().any(|c| c.key == v.kind);
                let copy = self.violations.iter().any(|w| !w.excluded && key(w) == k);
                if v.items.is_empty() || ignored || copy || !seen.insert(k) {
                    continue;
                }
            }
            out.push(v);
        }
        out
    }

    /// The studio's ERC shape: `check`, `severity`, `location` (the first
    /// item's id), `hint`, plus the full `items` list. A finding that kicad-cli
    /// reports as excluded -- the project lists its marker (`erc.erc_exclusions`) -- or whose
    /// `(check, location)` is in `exclusions` reports as `"excluded"`: still listed, no longer an
    /// error or a warning, with the exclusion's `comment`; `kicad_matched` says whether kicad-cli
    /// itself matched it. `marker` is the marker its exclusion would go into the project as, when the
    /// report says enough to write it (`eda_kicad::erc_exclusions`), and `sheet` the sheet the
    /// report lists it under.
    pub fn to_json(&self, exclusions: &[ErcExclusion]) -> Value {
        let mut counts = BTreeMap::<String, usize>::new();
        let violations: Vec<Value> = self
            .listed()
            .into_iter()
            .map(|v| {
                let location = v.items.iter().find_map(|i| i.id.clone()).unwrap_or_default();
                // `(check, location)`, and the sheet of the marker where both know one: a screen two sheets show has one id per item, and KiCad's marker of
                // each placement keeps its own sheet path (`SetSheetSpecificPath`), so the exclusion of one placement leaves the other's finding alone.
                let studio = if location.is_empty() { None } else { exclusions.iter().find(|e| e.check == v.kind && e.location == location && eda_kicad::erc_exclusions::same_sheet(e.marker.as_ref(), v.marker.as_ref())) };
                let by_kicad = v.excluded || v.severity == "exclusion";
                let excluded = by_kicad || studio.is_some();
                if !excluded {
                    *counts.entry(v.kind.clone()).or_default() += 1;
                }
                let comment = studio.map(|e| e.comment.as_str()).unwrap_or(v.comment.as_str());
                let item_desc: Vec<&str> = v.items.iter().map(|i| i.description.as_str()).filter(|d| !d.is_empty()).collect();
                let mut j = json!({
                    "check": v.kind,
                    "severity": if excluded { "excluded" } else { v.severity.as_str() },
                    // What the finding is when it is not excluded: the report folds the two together, and un-excluding must give the severity back.
                    "base_severity": if v.severity == "warning" { "warning" } else { "error" },
                    "location": location,
                    "hint": format!("{}{}{}", v.description, if item_desc.is_empty() { "" } else { ": " }, item_desc.join("; ")),
                    "items": v.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.x, i.pos.y], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
                    "comment": if excluded { comment } else { "" },
                    "kicad_matched": by_kicad,
                });
                if let Some(marker) = &v.marker {
                    j["marker"] = serde_json::to_value(marker).unwrap_or(Value::Null);
                }
                if !v.sheet.is_empty() {
                    j["sheet"] = json!(v.sheet);
                }
                j
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
    let mut cmd = cli_command(&cli, Some(&sch.with_extension("kicad_pro")));
    cmd.args(["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(&sch);
    let raw = run_report(cmd, &report, "erc")?;
    let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
    let sheet_count = raw["sheets"].as_array().map_or(0, Vec::len);
    let mut violations = Vec::new();
    for sheet in raw["sheets"].as_array().into_iter().flatten() {
        let path = sheet["uuid_path"].as_str().unwrap_or("");
        for v in sheet["violations"].as_array().into_iter().flatten() {
            let mut violation = Violation::from_json(v, &map, units_mm);
            violation.sheet = path.to_string();
            violation.marker = eda_kicad::erc_exclusions::marker(&violation.kind, &violation.severity, &Violation::marker_items(v, units_mm), path, sheet_count);
            violations.push(violation);
        }
    }
    let version = raw["kicad_version"].as_str().map(str::to_string).unwrap_or_else(|| cli_version(&cli));
    let ignored_checks = raw["ignored_checks"].as_array().map(|cs| cs.iter().map(IgnoredCheck::from_json).collect()).unwrap_or_default();
    Ok(ErcReport { engine: format!("kicad-cli {version}"), violations, ignored_checks })
}

// ------------------------------------------------------------------ saved reports

/// The unit a saved report prints lengths in (`--units`): the editor's own, `GetUserUnits()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportUnits {
    #[default]
    Mm,
    Mils,
    In,
}

impl ReportUnits {
    fn arg(self) -> &'static str {
        match self {
            ReportUnits::Mm => "mm",
            ReportUnits::Mils => "mils",
            ReportUnits::In => "in",
        }
    }
}

/// What a checker dialog's "Save..." asks of kicad-cli (`DIALOG_ERC::OnSaveReport`, `DIALOG_DRC::OnSaveReport`): the file format and the Show boxes -- the severities
/// the dialog lists, which is what its report holds (`RC_ITEMS_PROVIDER::GetSeverities`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ReportOptions {
    /// A `.json` file (`WriteJsonReport`); else the text report, `.rpt` (`WriteTextReport`).
    pub json: bool,
    pub errors: bool,
    pub warnings: bool,
    pub exclusions: bool,
    pub units: ReportUnits,
}

impl ReportOptions {
    /// `--format`, `--units` and the severity flags of `kicad-cli sch erc` / `pcb drc`: all three boxes are `--severity-all`; kicad-cli's own default (errors and
    /// warnings) is what none checked asks for.
    fn args(&self) -> Vec<&'static str> {
        let mut args = vec!["--format", if self.json { "json" } else { "report" }, "--units", self.units.arg()];
        if self.errors && self.warnings && self.exclusions {
            args.push("--severity-all");
        } else {
            if self.errors {
                args.push("--severity-error");
            }
            if self.warnings {
                args.push("--severity-warning");
            }
            if self.exclusions {
                args.push("--severity-exclusions");
            }
        }
        args
    }

    fn extension(&self) -> &'static str {
        if self.json {
            "json"
        } else {
            "rpt"
        }
    }
}

/// A report kicad-cli wrote for a dialog's "Save...": the name the dialog suggests (`ERC.rpt`, `DRC.json`) and the file's text.
#[derive(Debug, Clone)]
pub struct SavedReport {
    pub file_name: String,
    pub text: String,
}

/// Run `cmd` (a `kicad-cli ... -o <out>`) and read the report it wrote.
fn run_saved_report(cmd: Command, out: &Path, what: &str) -> Result<String, Vec<CheckResult>> {
    let _ = std::fs::remove_file(out);
    let run = output_within(cmd, limit(REPORT_TIMEOUT))?;
    std::fs::read_to_string(out).map_err(|_| fail(&format!("kicad_cli_{what}"), "kicad-cli", format!("no report: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr))))
}

/// The ERC report file of a dialog's "Save..." (`ERC_REPORT::WriteTextReport` / `WriteJsonReport`): `kicad-cli sch erc` on the design's schematic -- the exclusions in
/// the derived project -- asked for `opts`' format, units and severities. A run, not the report on screen: a marker deleted from the list is in it.
pub fn erc_report(design: &Design, model: &ConstraintModel, work: &Path, opts: &ReportOptions) -> Result<SavedReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (sch, _) = export_schematic(design, model, work, "board")?;
    let out = work.join(format!("erc-report.{}", opts.extension()));
    let mut cmd = cli_command(&cli, Some(&sch.with_extension("kicad_pro")));
    cmd.args(["sch", "erc"]).args(opts.args()).arg("-o").arg(&out).arg(&sch);
    Ok(SavedReport { file_name: format!("ERC.{}", opts.extension()), text: run_saved_report(cmd, &out, "erc")? })
}

/// The DRC report file of a dialog's "Save..." (`DRC_REPORT::WriteTextReport` / `WriteJsonReport`): `kicad-cli pcb drc` on the derived board, with the dialog's own
/// options (`refill_zones`, `schematic_parity`) and `opts`' format, units and severities.
pub fn drc_report(design: &Design, model: &ConstraintModel, work: &Path, drc: DrcOptions, opts: &ReportOptions) -> Result<SavedReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (pcb, _) = export_board(design, model, work, "board")?;
    if drc.schematic_parity {
        if design.schematic.is_none() {
            return Err(fail("kicad_cli_parity", "schematic", "the parity test compares the board with the schematic, and this design has none"));
        }
        export_schematic(design, model, work, "board")?;
    }
    let out = work.join(format!("drc-report.{}", opts.extension()));
    let mut cmd = cli_command(&cli, Some(&pcb.with_extension("kicad_pro")));
    cmd.args(["pcb", "drc"]).args(opts.args());
    if drc.refill_zones {
        cmd.arg("--refill-zones");
    }
    if drc.schematic_parity {
        cmd.arg("--schematic-parity");
    }
    cmd.arg("-o").arg(&out).arg(&pcb);
    Ok(SavedReport { file_name: format!("DRC.{}", opts.extension()), text: run_saved_report(cmd, &out, "drc")? })
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
    let mut cmd = cli_command(&cli, None);
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
/// effect on the probe board. kicad-cli has no rules checker of its own: an older one drops a rules file that does not
/// parse without a word (DRC then runs on the board's implicit rules alone), a newer one stops the DRC with no report,
/// so a rule that must show itself is the one way to see that the file was read either way.
fn rules_load(cli: &Path, dir: &Path, rules: &str) -> Result<bool, Vec<CheckResult>> {
    work_dir(dir)?;
    write_file(&dir.join("probe.kicad_pcb"), RULES_PROBE_PCB)?;
    write_file(&dir.join("probe.kicad_dru"), format!("{rules}\n{RULES_PROBE_SENTINEL}"))?;
    let report = dir.join("probe.drc.json");
    let mut cmd = cli_command(cli, None);
    cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(dir.join("probe.kicad_pcb"));
    match run_report(cmd, &report, "rules") {
        Ok(raw) => Ok(raw["violations"].as_array().is_some_and(|vs| vs.iter().any(|v| v["type"].as_str() == Some("clearance")))),
        // A newer kicad-cli (10.99 by build 5334) stops a DRC whose rules do not compile ("DRC incomplete: could not compile custom design
        // rules") and writes no report, where an older one dropped the file without a word: either way the rules did not load.
        Err(e) if e.iter().any(|c| c.hint.as_deref().is_some_and(|h| h.contains("could not compile custom design rules"))) => Ok(false),
        Err(e) => Err(e),
    }
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
        "bom" => "csv",
        _ => "out",
    }
}

/// The extension kicad-cli's output carries for these arguments. ODB++ and IPC-2581 are named by the compression asked
/// for, since kicad-cli writes whatever name it is given: an ODB++ is a `.zip` or a `.tgz`, or (`--compression none`) a
/// folder with no extension; an IPC-2581 is an `.xml`, or a `.zip` when compressed. A placement file is named by its format.
fn pcb_ext_for(kind: &str, args: &[String]) -> &'static str {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
    match kind {
        "odb" => match value("--compression") {
            Some("tgz") => "tgz",
            Some("none") => "",
            _ => "zip",
        },
        "ipc2581" if args.iter().any(|a| a == "--compress") => "zip",
        // `DIALOG_GEN_FOOTPRINT_POSITION::CreateFiles`: a CSV placement file is a `.csv`; the ASCII one (kicad-cli's default) a `.pos`.
        "pos" if value("--format") == Some("csv") => "csv",
        _ => pcb_ext(kind),
    }
}

/// The placement files a run of `kicad-cli pcb export pos` writes, named as `DIALOG_GEN_FOOTPRINT_POSITION` names them
/// (`PLACE_FILE_EXPORTER::DecorateFilename`: `-all` for both sides in one file, `-top`/`-bottom` for one side or for the two-file
/// mode, and `CreateAsciiFiles`'s CSV `-pos` before the extension). `single_file` is the dialog's "Generate single file with both front
/// and back positions" (checked by default); with it off and both sides asked, the top and the bottom are two files.
///
/// The Gerber X3 format (`CreateGerberFiles`) is always one file per side ("Gerber P&P files are always separated"), named by
/// `PLACEFILE_GERBER_WRITER::GetPlaceFileName`: `<name>-pnp_top.gbr` and `<name>-pnp_bottom.gbr`.
pub fn place_file_names(stem: &str, side: &str, format: &str, single_file: bool) -> Vec<String> {
    if format == "gerber" {
        return match side {
            "front" => vec![format!("{stem}-pnp_top.gbr")],
            "back" => vec![format!("{stem}-pnp_bottom.gbr")],
            _ => vec![format!("{stem}-pnp_top.gbr"), format!("{stem}-pnp_bottom.gbr")],
        };
    }
    let decorated = |front: bool, back: bool| -> String {
        let base = match (front, back) {
            (true, true) => format!("{stem}-all"),
            (true, false) => format!("{stem}-top"),
            (false, true) => format!("{stem}-bottom"),
            (false, false) => stem.to_string(),
        };
        // `fn.SetExt( FILEEXT::FootprintPlaceFileExtension )` for the ASCII file, `fn.SetName( ... + "-pos" ); fn.SetExt( "csv" )` for the CSV.
        if format == "csv" {
            format!("{base}-pos.csv")
        } else {
            format!("{base}.pos")
        }
    };
    match side {
        "front" => vec![decorated(true, false)],
        "back" => vec![decorated(false, true)],
        _ if single_file => vec![decorated(true, true)],
        _ => vec![decorated(true, false), decorated(false, true)],
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

/// One `kicad-cli <scope> export <kind> [args] -o <out> <input>` run; `out` is the file (or the folder with its trailing slash) passed
/// to `-o` as it is.
fn run_export_command(cli: &Path, scope: &str, kind: &str, args: &[String], input: &Path, out: &str) -> Result<Output, Vec<CheckResult>> {
    let mut cmd = cli_command(cli, None);
    cmd.args([scope, "export", kind]).args(args).arg("-o").arg(out).arg(input);
    output_within(cmd, limit(EXPORT_TIMEOUT))
}

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
    let out = run_export_command(cli, scope, kind, args, input, &out_arg)?;
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
    // What kicad-cli said on the way (its warnings -- "Board outline is malformed. Run DRC for a full analysis." -- and its progress), as
    // KiCad's STEP dialog shows the exporter's output in its log window (`DIALOG_EXPORT_STEP_LOG`): kept in the reply, not thrown away.
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    let mut reply = json!({ "ok": true, "engine": format!("kicad-cli {}", cli_version(cli)), "files": files });
    if !log.is_empty() {
        reply["log"] = Value::String(log);
    }
    Ok(reply)
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
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
    // A placement file's name is decorated by the dialog's rules (`place_file_names`), so its export is its own function; one file of
    // both sides (KiCad's default) unless the caller asks otherwise.
    if kind == "pos" && matches!(value("--format"), None | Some("ascii") | Some("csv")) {
        return export_pcb_placement(design, model, work, root, name, args, true);
    }
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (pcb, _) = export_board(design, model, work, &stem)?;
    let out_dir = root.join("export").join("kicad").join(kind);
    run_export(&cli, "pcb", kind, args, &pcb, &out_dir, DIR_KINDS.contains(&kind), pcb_ext_for(kind, args), &stem, root)
}

/// `kicad-cli pcb export pos` with the names `DIALOG_GEN_FOOTPRINT_POSITION` gives its files ([`place_file_names`]): `<name>-all`, or
/// `<name>-top` and `<name>-bottom` with `single_file` off and both sides asked. kicad-cli writes exactly the file its `-o` names, so
/// the two-file mode is two runs -- the second's `--side back` over the same board. Returns `{ ok, engine, files }` as [`export_pcb`].
pub fn export_pcb_placement(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, name: &str, args: &[String], single_file: bool) -> Result<Value, Vec<CheckResult>> {
    check_kind("pos")?;
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (pcb, _) = export_board(design, model, work, &stem)?;
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
    let names = place_file_names(&stem, value("--side").unwrap_or("both"), value("--format").unwrap_or("ascii"), single_file);
    let out_dir = root.join("export").join("kicad").join("pos");
    std::fs::create_dir_all(&out_dir).map_err(|e| fail("kicad_engine_dir", "export", e.to_string()))?;
    let mut files: Vec<String> = Vec::new();
    for (i, file_name) in names.iter().enumerate() {
        let run_args: Vec<String> = if names.len() == 2 {
            // The two-file mode asks kicad-cli for one side at a time; the first file is the top's, the second the bottom's.
            args_with_side(args, if i == 0 { "front" } else { "back" })
        } else {
            args.to_vec()
        };
        let target = out_dir.join(file_name);
        // A file an earlier run left must not pass for this one's.
        let _ = std::fs::remove_file(&target);
        let out = run_export_command(&cli, "pcb", "pos", &run_args, &pcb, &target.to_string_lossy())?;
        if !out.status.success() || !target.is_file() {
            return Err(fail(
                "kicad_cli_export",
                "pos",
                format!("kicad-cli pcb export pos failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
            ));
        }
        files.push(target.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| target.to_string_lossy().to_string()));
    }
    Ok(json!({ "ok": true, "engine": format!("kicad-cli {}", cli_version(&cli)), "files": files }))
}

/// `args` with `--side` set to `side` (`PLACE_FILE_EXPORTER`'s one side per run).
fn args_with_side(args: &[String], side: &str) -> Vec<String> {
    let mut out = args.to_vec();
    match out.iter().position(|a| a == "--side") {
        Some(i) if i + 1 < out.len() => out[i + 1] = side.to_string(),
        Some(_) => out.push(side.to_string()),
        None => out.extend(["--side".into(), side.into()]),
    }
    out
}

/// What kicad-cli prints on a Specctra export that worked: not a warning. (Read off kicad-cli 10.99 nightly 25ec2335f2; its Specctra job is later than the
/// KiCad source this port follows, 8303b2ad, which has none.)
const SPECCTRA_OK: &str = "Successfully created Specctra DSN file";

/// `kicad-cli pcb export specctra -o <output> <board>` (File > Export > Specctra DSN..., `DSN::ExportBoardToSpecctraFile`) on `design`: the derived board
/// written into `work` as for [`export_pcb`] (named `name`), kicad-cli run in `cwd`. `output` is passed as it is given, because KiCad names the design in
/// the file by it (`(pcb <output>`, `m_pcbname = aFilename`): the studio's export runs in the folder of the file with the bare file name, `eda board
/// export specctra -o FILE` in the caller's folder with FILE. The options are the ones `kicad-cli pcb export specctra --help` lists: `--output` and
/// `--define-var`, which the derived project has no use for.
///
/// Returns the lines kicad-cli printed besides its success line: the warnings of `ExportBoardToSpecctraFile` (`wxLogWarning`, such as "Board outline is
/// malformed. Run DRC for a full analysis."). A failure is kicad-cli's own words ("Failed to create output directory", the `IO_ERROR` text of `FromBOARD`).
pub fn export_specctra(design: &Design, model: &ConstraintModel, work: &Path, name: &str, cwd: &Path, output: &str) -> Result<Vec<String>, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (pcb, _) = export_board(design, model, work, &file_stem(name))?;
    // kicad-cli runs in `cwd`, so the board it reads is named absolutely.
    let pcb = std::fs::canonicalize(&pcb).unwrap_or(pcb);
    let target = cwd.join(output);
    // A file an earlier export left must not pass for this one's.
    let _ = std::fs::remove_file(&target);
    let mut cmd = cli_command(&cli, None);
    cmd.current_dir(cwd).args(["pcb", "export", "specctra", "-o", output]).arg(&pcb);
    let out = output_within(cmd, limit(EXPORT_TIMEOUT))?;
    let said: Vec<String> = [&out.stdout, &out.stderr].iter().flat_map(|b| String::from_utf8_lossy(b).lines().map(|l| l.trim().to_string()).collect::<Vec<_>>()).filter(|l| !l.is_empty()).collect();
    if !out.status.success() || !target.is_file() {
        let words = said.join("\n");
        return Err(fail("kicad_cli_export", "specctra", if words.is_empty() { format!("kicad-cli pcb export specctra failed ({})", out.status) } else { words }));
    }
    Ok(said.into_iter().filter(|l| l != SPECCTRA_OK).collect())
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

/// kicad-cli's two netlists the simulator reads (`crates/sim`): `--format spice` (the circuit, `NETLIST_EXPORTER_SPICE` with
/// `OPTION_SIM_COMMAND`) and `--format kicadxml` (the symbols' fields and the nets' pins), of `design`'s schematic, in `<work>/sim/`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimNetlists {
    pub spice: String,
    pub xml: String,
    /// What kicad-cli said making the SPICE netlist (the exporter's reporter: a symbol without a model, a library not found).
    pub messages: Vec<String>,
}

pub fn sim_netlists(design: &Design, model: &ConstraintModel, work: &Path, name: &str) -> Result<SimNetlists, Vec<CheckResult>> {
    let cli = need_cli()?;
    let stem = file_stem(name);
    let (sch, _) = export_schematic(design, model, work, &stem)?;
    let dir = work.join("sim");
    std::fs::create_dir_all(&dir).map_err(|e| fail("kicad_engine_dir", "sim", e.to_string()))?;
    let mut messages = Vec::new();
    let mut texts = Vec::new();
    for (format, ext) in [("spice", "cir"), ("kicadxml", "xml")] {
        let out = dir.join(format!("{stem}.{ext}"));
        let _ = std::fs::remove_file(&out);
        let mut cmd = cli_command(&cli, None);
        cmd.args(["sch", "export", "netlist", "--format", format, "-o"]).arg(&out).arg(&sch);
        let run = output_within(cmd, limit(EXPORT_TIMEOUT))?;
        let text = std::fs::read_to_string(&out).map_err(|_| {
            fail("kicad_cli_export", "netlist", format!("kicad-cli sch export netlist --format {format} failed: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr)))
        })?;
        if format == "spice" {
            let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
            messages.extend(said.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string));
        }
        texts.push(text);
    }
    let xml = texts.pop().unwrap_or_default();
    let spice = texts.pop().unwrap_or_default();
    Ok(SimNetlists { spice, xml, messages })
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
    let mut cmd = cli_command(&cli, None);
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

/// `kicad-cli sch export svg` or `pcb export svg` on a file as it is -- a design block's schematic or board, which is not the design's -- into `out_dir`, and the
/// SVG's text. `scope` is `sch` or `pcb`. The schematic is drawn without the drawing sheet or a background; the board is its area only (`--page-size-mode 2`), one file
/// (`--mode-single`) of the copper, silkscreen, courtyard and outline layers. This is what the Design Blocks panel's preview shows (`SCH_DESIGN_BLOCK_PREVIEW_WIDGET`,
/// `PCB_DESIGN_BLOCK_PREVIEW_WIDGET`); a run takes about a second.
pub fn export_file_svg(scope: &str, input: &Path, out_dir: &Path) -> Result<String, Vec<CheckResult>> {
    let cli = need_cli()?;
    if scope != "sch" && scope != "pcb" {
        return Err(fail("kicad_cli_export", scope, "an svg preview is of a schematic (sch) or a board (pcb)"));
    }
    let stem = file_stem(&input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "block".into()));
    let args: Vec<String> = if scope == "sch" {
        ["--no-background-color", "--exclude-drawing-sheet"].iter().map(|a| a.to_string()).collect()
    } else {
        ["--mode-single", "--layers", "F.Cu,B.Cu,F.SilkS,B.SilkS,F.Courtyard,B.Courtyard,Edge.Cuts", "--page-size-mode", "2", "--exclude-drawing-sheet"].iter().map(|a| a.to_string()).collect()
    };
    run_export(&cli, scope, "svg", &args, input, out_dir, scope == "sch", "svg", &stem, out_dir)?;
    let produced = out_dir.join(format!("{stem}.svg"));
    // kicad-cli names a schematic's svg after the file it was given (`<name>.svg`); look for the one it wrote when the name differs
    let found = if produced.is_file() {
        Some(produced)
    } else {
        std::fs::read_dir(out_dir).ok().and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "svg")))
    };
    let path = found.ok_or_else(|| fail("kicad_cli_export", "svg", "kicad-cli wrote no svg"))?;
    std::fs::read_to_string(&path).map_err(|e| fail("kicad_cli_export", "svg", format!("could not read {}: {e}", path.display())))
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
    use eda_model::units::{mm, Nm};
    use super::*;

    fn pt(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    #[test]
    fn the_export_kinds_are_read_from_the_usage_line_or_the_subcommand_list() {
        let nightly = "Usage: pcb export [--help] {3dpdf,bom,drill,specctra,step}\n\nExport utilities\n\nSubcommands:\n  3dpdf  Export PDF\n";
        assert_eq!(parse_export_kinds(nightly), ["3dpdf", "bom", "drill", "specctra", "step"]);
        let listed = "Usage: pcb export [-h] VAR\n\nSubcommands:\n  drill      Generate Drill Files\n  gerbers    Plot Gerbers\n\nOther\n";
        assert_eq!(parse_export_kinds(listed), ["drill", "gerbers"]);
        assert!(parse_export_kinds("").is_empty());
        assert!(!parse_export_kinds(listed).iter().any(|k| k == "specctra"));
    }

    #[test]
    fn the_export_probe_asks_each_kicad_cli_once_per_scope() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_engine_export_probe_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut env = Env::take();
        // A kicad-cli without IDF (an older one) that counts how often it is asked.
        let count = dir.join("asked");
        env.set("FAKE_KICAD_ASKED", &count);
        let cli = script(&dir, "no_idf.sh", "echo x >> \"$FAKE_KICAD_ASKED\"\necho 'Usage: pcb export [--help] {drill,gencad,gerbers,step}'");
        assert!(!cli_has_export(&cli, "pcb", "idf"));
        assert!(cli_has_export(&cli, "pcb", "gencad"));
        assert_eq!(std::fs::read_to_string(&count).unwrap().lines().count(), 1, "one probe per binary and scope");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        assert_eq!(m["items"][0]["pos"], json!([1_500_000, -2_000_000]));
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
        assert_eq!(v.markers_nm, vec![pt(1_500_000, -2_000_000), pt(2_500_000, -999_500), pt(3_500_000, 1_000)]);
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
            tracks: vec![eda_model::ir::Track { id: "t7".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.1), pts: vec![eda_model::ir::Point { x: mm(1.0), y: mm(2.0) }, eda_model::ir::Point { x: Nm::new(3_001_000), y: mm(2.0) }, eda_model::ir::Point { x: Nm::new(3_001_000), y: mm(9.0) }], arc_mid_offset: None }],
            vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default(),
        });
        // The first segment is the track's own id, the next ones `<id>#n`: `DRC_TEST_PROVIDER_TRACK_WIDTH` puts a trace's marker at (start + end) / 2.
        assert_eq!(track_anchors(&design, "t7"), vec![pt(1_000_000, 2_000_000), pt(3_001_000, 2_000_000), pt(2_000_500, 2_000_000)]);
        assert_eq!(track_anchors(&design, "t7#1"), vec![pt(3_001_000, 2_000_000), pt(3_001_000, 9_000_000), pt(3_001_000, 5_500_000)]);
        assert!(track_anchors(&design, "t7#2").is_empty(), "there is no third segment");
        assert!(track_anchors(&design, "R1.1").is_empty() && track_anchors(&design, "t8").is_empty());

        let mut v = Violation { kind: "track_width".into(), items: vec![Item { description: "Track".into(), pos: pt(1_000_000, 2_000_000), id: Some("t7".into()), uuid: "u".into() }], markers_nm: vec![pt(1_000_000, 2_000_000)], ..Default::default() };
        v.add_design_anchors(&design);
        assert_eq!(v.markers_nm, vec![pt(1_000_000, 2_000_000), pt(2_000_500, 2_000_000), pt(3_001_000, 2_000_000)], "the start (the report's) is listed once");
    }

    #[test]
    fn the_studio_marks_a_waived_violation_by_its_check_and_items_and_says_whether_kicad_matched_it() {
        let item = |uuid: &str| Item { description: uuid.into(), pos: Point::default(), id: None, uuid: uuid.into() };
        let found = |kind: &str, uuids: &[&str], by_kicad: bool| Violation { kind: kind.into(), severity: "error".into(), items: uuids.iter().map(|u| item(u)).collect(), excluded: by_kicad, ..Default::default() };
        let rep = DrcReport {
            engine: "kicad-cli test".into(),
            zones_refilled_by_kicad: false,
            zone_fills: None,
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

    /// `empty_design` with one copper pour on F.Cu, no net: enough for `refill_zones` to have something to fill.
    fn design_with_a_pour() -> (Design, ConstraintModel) {
        let (mut design, model) = empty_design();
        design.routing = Some(
            serde_json::from_value(json!({
                "tracks": [], "vias": [],
                "zones": [{ "id": "pour", "net": "", "layer": "F.Cu", "outline": [{ "x": 1000000, "y": 1000000 }, { "x": 9000000, "y": 1000000 }, { "x": 9000000, "y": 9000000 }, { "x": 1000000, "y": 9000000 }] }],
            }))
            .unwrap(),
        );
        (design, model)
    }

    /// The stand-in kicad-cli of the refill tests: writes what it was given to `$FAKE_LOG`, answers a DRC with an empty report and, asked for `--refill-zones
    /// --save-board`, saves the board it was given with a `(filled_polygon ..)` in its zone and its own generator, as the real one does.
    const FILLING_CLI: &str = r#"out=""; pcb=""; prev=""; refill=no; save=no
for a in "$@"; do
  if [ "$prev" = "-o" ]; then out="$a"; fi
  if [ "$a" = "--refill-zones" ]; then refill=yes; fi
  if [ "$a" = "--save-board" ]; then save=yes; fi
  pcb="$a"; prev="$a"
done
echo "$refill $save config=$KICAD_CONFIG_HOME $*" >> "$FAKE_LOG"
cp "$pcb" "$pcb.given"
echo '{"coordinate_units":"mm","kicad_version":"10.99.0-fake","violations":[],"unconnected_items":[]}' > "$out"
if [ $refill = yes ] && [ $save = yes ] && [ -z "$FAKE_CANNOT_REFILL" ]; then
  awk '/\(polygon/ && !done { print "(filled_polygon (layer \"F.Cu\") (pts (xy 1 1) (xy 9 1) (xy 9 9)))"; done = 1 } { gsub(/\(generator "eda-kicad"\)/, "(generator \"pcbnew\")"); print }' "$pcb" > "$pcb.saved" && mv "$pcb.saved" "$pcb"
fi"#;

    #[test]
    fn fill_all_zones_runs_kicad_clis_filler_on_a_copy_through_cli_command_and_reads_its_fills_back() {
        let mut env = Env::take();
        let dir = std::env::temp_dir().join(format!("eda_kicad_engine_refill_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("calls.log");
        env.set("FAKE_LOG", &log);
        env.set("EDA_KICAD_CLI", script(&dir, "filling.sh", FILLING_CLI));
        env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
        let work = dir.join("work");

        // A board with no pour has nothing to fill: kicad-cli is not started.
        let (bare, model) = empty_design();
        let none = refill_zones(&bare, &model, &work).expect("nothing to fill");
        assert!(none.zones.is_empty() && !log.exists(), "kicad-cli was started for a board with no zone");

        let (design, model) = design_with_a_pour();
        let fills = refill_zones(&design, &model, &work).expect("a refill");
        assert_eq!(fills.engine, "kicad-cli 10.99.0-fake");
        let entry = &fills.zones["pour"];
        assert_eq!(entry.sig, eda_kicad::zone_fills::zone_signature(&design.routing.as_ref().unwrap().zones[0]), "the fill is the fill of the zone as it is now");
        assert_eq!(entry.layers["F.Cu"][0].pts, vec![[1_000_000, 1_000_000], [9_000_000, 1_000_000], [9_000_000, 9_000_000]], "nanometres, as kicad-cli stored them");

        // One launch, the filler's: the copy of the board in a folder of its own, saved, with the settings folder of `cli_command` (no user libraries to enumerate).
        let calls = std::fs::read_to_string(&log).unwrap();
        assert_eq!(calls.lines().count(), 1, "{calls}");
        assert!(calls.starts_with("yes yes "), "--refill-zones and --save-board: {calls}");
        assert!(calls.contains("pcb drc --refill-zones --save-board"), "{calls}");
        if std::env::var_os("EDA_KICAD_USER_CONFIG").is_none() {
            let home = dir.join("settings").join("empty");
            assert!(calls.contains(&format!("config={}", home.display())), "the empty settings folder of cli_command, not a folder of the refill's own: {calls}");
            assert!(home.is_dir());
        }
        assert!(work.join("refill").join("refill.kicad_pcb").exists() && !work.join("board.kicad_pcb").exists(), "the board the other runs read is left as it is");
        let given = std::fs::read_to_string(work.join("refill").join("refill.kicad_pcb.given")).unwrap();
        assert!(given.contains("(generator \"eda-kicad\")") && !given.contains("(filled_polygon"), "kicad-cli was given the board unfilled, as we write it");
        let pro: Value = serde_json::from_str(&std::fs::read_to_string(work.join("refill").join("refill.kicad_pro")).unwrap()).unwrap();
        let severities = pro["board"]["design_settings"]["rule_severities"].as_object().unwrap();
        assert!(!severities.is_empty() && severities.values().all(|v| v == "ignore"), "the copy's project ignores every check: the refill is all the run is for");

        // A kicad-cli that cannot refill (9.0 does not know the options) leaves the board as we wrote it: said so, never taken for a fill.
        env.set("FAKE_CANNOT_REFILL", "1");
        let e = refill_zones(&design, &model, &work).unwrap_err();
        assert_eq!(e[0].check, "kicad_cli_refill", "{e:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_drc_refills_first_on_a_copy_and_judges_the_board_with_that_fill_in_it_not_with_refill_zones() {
        let mut env = Env::take();
        let dir = std::env::temp_dir().join(format!("eda_kicad_engine_drc_refill_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("calls.log");
        env.set("FAKE_LOG", &log);
        env.set("EDA_KICAD_CLI", script(&dir, "filling.sh", FILLING_CLI));
        env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
        let (design, model) = design_with_a_pour();
        let work = dir.join("work");

        let report = drc(&design, &model, &work, true).expect("refill, then DRC");
        assert!(report.zones_refilled_by_kicad);
        assert_eq!(report.zone_fills.as_ref().map(|k| k.counts()), Some((1, 1)), "the fills come back for the caller to keep");
        let calls: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(String::from).collect();
        assert_eq!(calls.len(), 2, "the refill, then the DRC: {calls:?}");
        assert!(calls[0].starts_with("yes yes "), "{calls:?}");
        assert!(calls[1].starts_with("no no ") && !calls[1].contains("--refill-zones"), "the DRC judges the fill the refill made and the caller keeps: {calls:?}");
        let judged = std::fs::read_to_string(work.join("board.kicad_pcb.given")).unwrap();
        assert!(judged.contains("(filled_polygon"), "the DRC judges the board with the fill in it");
        let flat = judged.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("(filled_polygon (layer \"F.Cu\") (pts (xy 1 1) (xy 9 1) (xy 9 9) ) )"), "KiCad's own ring, as it stored it");

        // Not asked for: the board as it is, the fills kept beside the design (none here), no refill.
        let _ = std::fs::remove_file(&log);
        let plain = drc(&design, &model, &work, false).expect("a plain DRC");
        assert!(!plain.zones_refilled_by_kicad && plain.zone_fills.is_none());
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
        assert!(!std::fs::read_to_string(work.join("board.kicad_pcb.given")).unwrap().contains("(filled_polygon"), "a zone never filled is exported unfilled, as in KiCad");

        // A kicad-cli that cannot refill gives the DRC of the board as it is.
        env.set("FAKE_CANNOT_REFILL", "1");
        let fallback = drc(&design, &model, &work, true).expect("the DRC without a refill");
        assert!(!fallback.zones_refilled_by_kicad && fallback.zone_fills.is_none());
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
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 1".into(), pos: Point::default(), id: Some("U1.1".into()), uuid: "a".into() }], ..Default::default() },
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 2".into(), pos: Point::default(), id: Some("U1.2".into()), uuid: "b".into() }], ..Default::default() },
            ],
            ignored_checks: vec![],
        };
        let j = rep.to_json(&[ErcExclusion { check: "pin_not_connected".into(), location: "U1.2".into(), comment: "unused".into(), ..Default::default() }]);
        assert_eq!(j["violations"][0]["severity"], "error");
        assert_eq!(j["violations"][1]["severity"], "excluded");
        assert_eq!(j["counts"]["pin_not_connected"], 1);
        assert_eq!(j["violations"][0]["location"], "U1.1");
        assert_eq!(j["violations"][1]["base_severity"], "error", "an excluded finding says what it is when it is not");
        assert_eq!(j["violations"][0]["hint"], "Pin not connected: Pin 1");
        assert_eq!(j["violations"][1]["comment"], "unused", "the exclusion's comment comes with it");
        assert_eq!(j["violations"][1]["kicad_matched"], false, "the studio's own exclusion: kicad-cli's project did not hold it");
        assert_eq!(j["violations"][0]["comment"], "");
    }

    /// p13-p16 F5: a screen two sheets show has one id per item, so its two placements' findings share `(check, location)`; their markers keep their own
    /// sheets (`SetSheetSpecificPath`), and the exclusion of one placement leaves the other an error.
    #[test]
    fn an_erc_exclusion_of_one_placement_leaves_the_other_placements_finding() {
        let at = |sheet: &str| eda_model::ir::ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [Nm::new(1), Nm::new(2)], items: vec!["u-a".into()], sheet: vec!["root".into(), sheet.into()], main_sheet: vec!["root".into(), sheet.into()], ..Default::default() };
        let placed = |sheet: &str| Violation { marker: Some(at(sheet)), sheet: format!("/root/{sheet}"), ..erc_violation("pin_not_connected", vec![erc_item("Pin 2", Some("R1.2"), "u-a")], false) };
        let rep = ErcReport { engine: "kicad-cli test".into(), violations: vec![placed("s1"), placed("s2")], ignored_checks: vec![] };
        let j = rep.to_json(&[ErcExclusion { check: "pin_not_connected".into(), location: "R1.2".into(), marker: Some(at("s1")), ..Default::default() }]);
        assert_eq!((j["violations"][0]["severity"].as_str(), j["violations"][1]["severity"].as_str()), (Some("excluded"), Some("error")), "{j:#}");
        assert_eq!(j["counts"]["pin_not_connected"], 1);
        // an exclusion with no marker (one of a check whose marker is not known) still keys on (check, location) alone
        let j = rep.to_json(&[ErcExclusion { check: "pin_not_connected".into(), location: "R1.2".into(), ..Default::default() }]);
        assert_eq!((j["violations"][0]["severity"].as_str(), j["violations"][1]["severity"].as_str()), (Some("excluded"), Some("excluded")));
    }

    /// kicad-cli's own ERC report items for the tests below.
    fn erc_item(desc: &str, id: Option<&str>, uuid: &str) -> Item {
        Item { description: desc.into(), pos: Point::default(), id: id.map(str::to_string), uuid: uuid.into() }
    }

    fn erc_violation(kind: &str, items: Vec<Item>, excluded: bool) -> Violation {
        Violation { kind: kind.into(), description: "x".into(), severity: "error".into(), items, excluded, comment: if excluded { "from kicad".into() } else { String::new() }, ..Default::default() }
    }

    #[test]
    fn a_finding_kicad_cli_matched_is_excluded_with_its_comment_and_counted_by_nobody() {
        let a = erc_item("Pin 1", Some("U1.1"), "u-a");
        let rep = ErcReport { engine: "kicad-cli test".into(), violations: vec![erc_violation("pin_not_connected", vec![a.clone()], true), erc_violation("pin_not_connected", vec![erc_item("Pin 2", Some("U1.2"), "u-b")], false)], ignored_checks: vec![] };
        let j = rep.to_json(&[ErcExclusion { check: "pin_not_connected".into(), location: "U1.1".into(), comment: "reviewed".into(), marker: Some(eda_model::ir::ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [Nm::new(1), Nm::new(2)], items: vec!["u-a".into()], ..Default::default() }) }]);
        assert_eq!(j["violations"][0]["severity"], "excluded");
        assert_eq!(j["violations"][0]["kicad_matched"], true, "kicad-cli itself matched the marker text");
        assert_eq!(j["violations"][0]["comment"], "reviewed", "the studio's comment is the exclusion's");
        assert_eq!(j["counts"]["pin_not_connected"], 1, "only the other one counts");
        // A finding kicad-cli excluded with no exclusion of the studio's behind it (a foreign project) keeps kicad-cli's comment.
        let j = rep.to_json(&[]);
        assert_eq!((j["violations"][0]["severity"].as_str(), j["violations"][0]["comment"].as_str()), (Some("excluded"), Some("from kicad")));
    }

    /// `SCHEMATIC::ResolveERCExclusions` makes an excluded marker of a text that matched nothing, and kicad-cli's report lists it: it is no violation.
    #[test]
    fn the_markers_an_unmatched_exclusion_made_are_not_findings() {
        let real = erc_violation("footprint_link_issues", vec![erc_item("Symbol C1", Some("C1"), "u-c1")], false);
        let copy = erc_violation("footprint_link_issues", vec![erc_item("Symbol C1", Some("C1"), "u-c1")], true);
        let gone = erc_violation("pin_not_connected", vec![], true);
        let stale = erc_violation("wire_dangling", vec![erc_item("Wire", Some("w9"), "u-w")], true);
        let stale_twice = erc_violation("wire_dangling", vec![erc_item("Wire", Some("w9"), "u-w")], true);
        let ignored = erc_violation("simulation_model_issue", vec![erc_item("Symbol R1", Some("R1"), "u-r1")], true);
        let rep = ErcReport { engine: "kicad-cli test".into(), violations: vec![real, copy, gone, stale, stale_twice, ignored], ignored_checks: vec![IgnoredCheck { key: "simulation_model_issue".into(), description: "x".into() }] };
        let j = rep.to_json(&[ErcExclusion { check: "footprint_link_issues".into(), location: "C1".into(), ..Default::default() }]);
        let listed: Vec<(&str, &str)> = j["violations"].as_array().unwrap().iter().map(|v| (v["check"].as_str().unwrap(), v["severity"].as_str().unwrap())).collect();
        assert_eq!(listed, vec![("footprint_link_issues", "excluded"), ("wire_dangling", "excluded")], "the real finding (waived by the studio), and the one stale marker of an exclusion whose violation is gone");
        assert_eq!(j["counts"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn an_erc_entry_offers_the_marker_its_exclusion_would_be_and_the_sheet_it_is_under() {
        // The report's positions are millimetres as they are, and the marker's are nanometres: 115.57 mm is 115570000.
        let v = json!({ "type": "pin_not_connected", "description": "Pin not connected", "severity": "error",
            "items": [{ "description": "Symbol #PWR03 Pin 1 [Power input, Line]", "pos": { "x": 115.57, "y": 110.49 }, "uuid": "693a38f5-591c-4c06-9786-e8973d13af99" }] });
        let items = Violation::marker_items(&v, true);
        assert_eq!((items[0].x_nm, items[0].y_nm), (mm(115.57), mm(110.49)));
        assert!(Violation::marker_items(&v, false).is_empty(), "inches: not known");
        let mut violation = Violation::from_json(&v, &HashMap::new(), true);
        violation.sheet = "/r".into();
        violation.marker = eda_kicad::erc_exclusions::marker("pin_not_connected", "error", &items, "/r", 1);
        let rep = ErcReport { engine: "kicad-cli test".into(), violations: vec![violation], ignored_checks: vec![] };
        let j = rep.to_json(&[]);
        assert_eq!(
            j["violations"][0]["marker"],
            json!({ "error_type": "ERCET_PIN_NOT_CONNECTED", "at_nm": [115_570_000, 110_490_000], "items": ["693a38f5-591c-4c06-9786-e8973d13af99"], "sheet": ["r"], "main_sheet": ["r"] })
        );
        assert_eq!(j["violations"][0]["sheet"], "/r");
    }

    #[test]
    fn a_saved_report_asks_kicad_cli_for_its_format_its_units_and_the_severities_the_dialog_shows() {
        let all = ReportOptions { json: false, errors: true, warnings: true, exclusions: true, units: ReportUnits::Mm };
        assert_eq!(all.args(), ["--format", "report", "--units", "mm", "--severity-all"], "the three boxes are all severities");
        assert_eq!(all.extension(), "rpt");
        let some = ReportOptions { json: true, errors: true, warnings: false, exclusions: true, units: ReportUnits::Mils };
        assert_eq!(some.args(), ["--format", "json", "--units", "mils", "--severity-error", "--severity-exclusions"]);
        assert_eq!(some.extension(), "json");
        assert_eq!(ReportOptions { units: ReportUnits::In, ..Default::default() }.args(), ["--format", "report", "--units", "in"], "none asked for: kicad-cli's own default");
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

    /// `DIALOG_GEN_FOOTPRINT_POSITION::CreateAsciiFiles` / `CreateFiles` (pcbnew/dialogs/dialog_gen_footprint_position.cpp): a CSV placement file is a
    /// `.csv` (`fn.SetExt( wxT( "csv" ) )`), an ASCII one a `.pos` (`FILEEXT::FootprintPlaceFileExtension`).
    #[test]
    fn a_csv_placement_file_is_a_csv_and_an_ascii_one_a_pos() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pcb_ext_for("pos", &a(&["--format", "csv", "--side", "both"])), "csv");
        assert_eq!(pcb_ext_for("pos", &a(&["--format", "ascii", "--side", "front"])), "pos");
        assert_eq!(pcb_ext_for("pos", &a(&[])), "pos", "kicad-cli's own default format is ascii");
    }

    /// `PLACE_FILE_EXPORTER::DecorateFilename` and `DIALOG_GEN_FOOTPRINT_POSITION`'s "Generate single file with both front and back
    /// positions" (checked by default): `<name>-all`, `-top`, `-bottom`, the CSV adding its `-pos` before the extension.
    #[test]
    fn placement_files_are_named_all_top_and_bottom_as_kicad_names_them() {
        let names = |side: &str, format: &str, single: bool| place_file_names("board", side, format, single).join(", ");
        assert_eq!(names("both", "csv", true), "board-all-pos.csv", "one file, both sides: KiCad's own default");
        assert_eq!(names("both", "ascii", true), "board-all.pos");
        assert_eq!(names("both", "csv", false), "board-top-pos.csv, board-bottom-pos.csv", "the checkbox off: the top and the bottom as two files");
        assert_eq!(names("both", "ascii", false), "board-top.pos, board-bottom.pos");
        assert_eq!(names("front", "csv", false), "board-top-pos.csv", "one side asked: the single-file choice does not apply");
        assert_eq!(names("back", "ascii", true), "board-bottom.pos");
        // X8 (r2-editors-exports): Gerber X3 is always the two sides apart, `PLACEFILE_GERBER_WRITER::GetPlaceFileName`'s `-pnp_top`/`-pnp_bottom`.
        assert_eq!(names("both", "gerber", true), "board-pnp_top.gbr, board-pnp_bottom.gbr");
    }

    /// The mode is one kicad-cli run per file: kicad-cli writes exactly the name its `-o` has, so the two sides become two runs, each
    /// with its own `--side`.
    #[test]
    fn a_placement_export_runs_kicad_cli_once_per_file_and_says_which_side_each_run_asked_for() {
        let dir = board_dir("pos-names");
        let (design, model) = empty_design();
        let mut env = Env::take();
        env.set(
            "EDA_KICAD_CLI",
            script(
                &dir,
                "pos.sh",
                "out=\"\"; prev=\"\"; side=\"both\"\nfor a in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi\n  if [ \"$prev\" = \"--side\" ]; then side=\"$a\"; fi\n  prev=\"$a\"\ndone\nprintf 'Ref,Val,PosX,PosY,Rot,Side\\n' > \"$out\"\necho \"$side\" >> \"$FAKE_POS_LOG\"",
            ),
        );
        let log = dir.join("sides.log");
        env.set("FAKE_POS_LOG", &log);
        let work = dir.join("work");
        let args = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let files = |v: &Value| v["files"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
        let both = args(&["--format", "csv", "--side", "both"]);

        let one = export_pcb_placement(&design, &model, &work, &dir, "board", &both, true).expect("the single file");
        assert_eq!(files(&one), vec!["export/kicad/pos/board-all-pos.csv"]);
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "both", "one run, both sides");

        let _ = std::fs::remove_file(&log);
        let two = export_pcb_placement(&design, &model, &work, &dir, "board", &both, false).expect("the two files");
        assert_eq!(files(&two), vec!["export/kicad/pos/board-top-pos.csv", "export/kicad/pos/board-bottom-pos.csv"]);
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "front\nback", "a run per side");
        assert!(dir.join("export/kicad/pos/board-top-pos.csv").is_file() && dir.join("export/kicad/pos/board-bottom-pos.csv").is_file());

        let _ = std::fs::remove_file(&log);
        let top = export_pcb_placement(&design, &model, &work, &dir, "board", &args(&["--format", "ascii", "--side", "front"]), false).expect("the top only");
        assert_eq!(files(&top), vec!["export/kicad/pos/board-top.pos"], "one side asked: one file whatever the single-file choice");
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "front");
        let _ = std::fs::remove_dir_all(&dir);
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
                    "viewports": [{ "name": "Home", "x": 1000, "y": 2000, "w": 3000, "h": 4000 }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let work = dir.join(".kicad");
        write_project_with(&work, "b", &design, &model, None).unwrap();
        let pro: Value = serde_json::from_str(&std::fs::read_to_string(work.join("b.kicad_pro")).unwrap()).unwrap();
        assert_eq!(pro["net_settings"]["net_colors"]["GND"], "rgb(1, 2, 3)");
        assert_eq!(pro["board"]["layer_presets"][0]["name"], "Mine");
        assert_eq!(pro["board"]["layer_presets"][0]["layers"], json!([2]));
        assert_eq!(pro["board"]["viewports"][0]["w"], 3000.0, "the studio's nanometres are the project file's");
        assert!(pro["board"]["design_settings"]["rules"].is_object(), "the design rules are still there");
        let prl: Value = serde_json::from_str(&std::fs::read_to_string(work.join("b.kicad_prl")).unwrap()).unwrap();
        assert_eq!(prl["board"]["visible_items"], json!(["tracks"]));
        assert_eq!(prl["board"]["visible_layers"], "ffffffff_ffffffff_ffffffff_fffffffb");
        assert_eq!(prl["board"]["net_color_mode"], 2);
        assert_eq!(prl["board"]["hidden_nets"], json!(["GND"]));
        // The studio clears everything: the next export leaves no local settings file behind.
        std::fs::write(dir.join("appearance.json"), r#"{"version":1,"local":{},"project":{}}"#).unwrap();
        write_project_with(&work, "b", &design, &model, None).unwrap();
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
        write_project_with(&dir.join(".kicad"), "b", &design, &model, None).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".kicad").join("b.kicad_pro")).unwrap(), export_kicad_pro_for(&design, &model));
        assert!(!dir.join(".kicad").join("b.kicad_prl").exists());
        // A scratch folder is no board's `.kicad`: whatever lies beside it is not that board's.
        let loose = board_dir("loose");
        std::fs::write(loose.join("appearance.json"), r#"{"local":{"visible_items":["tracks"]},"project":{}}"#).unwrap();
        let scratch = loose.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        write_project_with(&scratch, "b", &design, &model, None).unwrap();
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
    /// is the process's, shared by every test thread, so only one of these lives at a time; a test that runs kicad-cli
    /// takes one too (`footprint_check`), so another test's short time limit is never its own.
    pub(crate) struct Env {
        keys: Vec<&'static str>,
        _only_one: std::sync::MutexGuard<'static, ()>,
    }
    impl Env {
        pub(crate) fn take() -> Env {
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
