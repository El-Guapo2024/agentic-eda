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

use eda_kicad::{export_kicad_pcb_mapped, export_kicad_pro, export_kicad_sch_mapped, ExportMeta};
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

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
    per_cli(&CACHE, cli, || Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default())
}

// ----------------------------------------------------------- the derived files

fn work_dir(work: &Path) -> Result<(), Vec<CheckResult>> {
    std::fs::create_dir_all(work).map_err(|e| fail("kicad_engine_dir", &work.display().to_string(), format!("cannot create {}: {e}", work.display())))
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), Vec<CheckResult>> {
    std::fs::write(path, bytes).map_err(|e| fail("kicad_engine_write", &path.display().to_string(), e.to_string()))
}

fn today() -> String {
    // YYYY-MM-DD from the system clock (civil-from-days, no extra crate).
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

/// The project file next to the derived board/schematic: our design rules,
/// plus the custom-rule file an imported project carried, so kicad-cli
/// judges against this board's own rules and not its hard-coded floors.
fn write_project(work: &Path, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
    write_file(&work.join("board.kicad_pro"), export_kicad_pro(model))?;
    let dru = work.join("board.kicad_dru");
    match model.board.custom_rules_text.as_deref() {
        Some(text) => write_file(&dru, text)?,
        None => {
            let _ = std::fs::remove_file(&dru);
        }
    }
    Ok(())
}

/// Export the design as `board.kicad_pcb` (+ project); returns the pcb path
/// and the uuid -> our id map.
fn export_board(design: &Design, model: &ConstraintModel, work: &Path) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    work_dir(work)?;
    let date = today();
    let (pcb, map) = export_kicad_pcb_mapped(design, model, &ExportMeta { date: &date, title: "board" })?;
    let path = work.join("board.kicad_pcb");
    write_file(&path, pcb)?;
    write_project(work, model)?;
    Ok((path, map))
}

/// Export the design's schematic as `board.kicad_sch` (+ project).
fn export_schematic(design: &Design, model: &ConstraintModel, work: &Path) -> Result<(PathBuf, HashMap<String, String>), Vec<CheckResult>> {
    work_dir(work)?;
    let date = today();
    let (sch, map) = export_kicad_sch_mapped(design, model, &ExportMeta { date: &date, title: "board" })?;
    let path = work.join("board.kicad_sch");
    write_file(&path, sch)?;
    write_project(work, model)?;
    Ok((path, map))
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
#[derive(Debug, Clone)]
pub struct Violation {
    pub kind: String,
    pub description: String,
    /// `error`, `warning`, `exclusion` or `ignore`, as KiCad reports it.
    pub severity: String,
    pub items: Vec<Item>,
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
        Violation { kind: v["type"].as_str().unwrap_or("?").to_string(), description: v["description"].as_str().unwrap_or("").to_string(), severity: v["severity"].as_str().unwrap_or("error").to_string(), items }
    }

    /// The studio's DRC shape: `type`, `description`, `severity`, `items`
    /// (each with a `[x, y]` µm `pos`, our `id` or `null`, the KiCad `uuid`).
    pub fn to_json(&self) -> Value {
        json!({
            "type": self.kind,
            "description": self.description,
            "severity": self.severity,
            "items": self.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.0, i.pos.1], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
        })
    }
}

fn counts_of<'a>(vs: impl Iterator<Item = &'a Violation>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in vs {
        *m.entry(v.kind.clone()).or_default() += 1;
    }
    m
}

/// `kicad-cli pcb drc` on one design revision.
#[derive(Debug, Clone)]
pub struct DrcReport {
    /// "kicad-cli 10.99.0".
    pub engine: String,
    pub zones_refilled_by_kicad: bool,
    pub violations: Vec<Violation>,
    pub unconnected_items: Vec<Violation>,
}

impl DrcReport {
    pub fn counts(&self) -> BTreeMap<String, usize> {
        counts_of(self.violations.iter().chain(self.unconnected_items.iter()))
    }

    /// `{ engine, zones_refilled_by_kicad, violations, unconnected_items, counts }`.
    pub fn to_json(&self) -> Value {
        json!({
            "engine": self.engine,
            "zones_refilled_by_kicad": self.zones_refilled_by_kicad,
            "violations": self.violations.iter().map(Violation::to_json).collect::<Vec<_>>(),
            "unconnected_items": self.unconnected_items.iter().map(Violation::to_json).collect::<Vec<_>>(),
            "counts": self.counts(),
        })
    }
}

fn run_report(mut cmd: Command, report: &Path, what: &str) -> Result<Value, Vec<CheckResult>> {
    let _ = std::fs::remove_file(report);
    let out = cmd.output().map_err(|e| fail("kicad_cli_run", "kicad-cli", e.to_string()))?;
    let text = std::fs::read_to_string(report).map_err(|_| fail(&format!("kicad_cli_{what}"), "kicad-cli", format!("no report: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))?;
    serde_json::from_str(&text).map_err(|e| fail(&format!("kicad_cli_{what}"), &report.display().to_string(), e.to_string()))
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
    let cli = need_cli()?;
    let (pcb, map) = export_board(design, model, work)?;
    let report = work.join("drc.json");
    let run = |refill: bool| {
        let mut cmd = Command::new(&cli);
        cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]);
        if refill {
            cmd.arg("--refill-zones");
        }
        cmd.arg("-o").arg(&report).arg(&pcb);
        run_report(cmd, &report, "drc")
    };
    let (raw, refilled) = if refill_zones {
        match run(true) {
            Ok(raw) => (raw, true),
            Err(e) if e.iter().any(|c| c.hint.as_deref().is_some_and(|h| h.contains("refill-zones"))) => (run(false)?, false),
            Err(e) => return Err(e),
        }
    } else {
        (run(false)?, false)
    };
    let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
    let read = |key: &str| -> Vec<Violation> { raw[key].as_array().map(|vs| vs.iter().map(|v| Violation::from_json(v, &map, units_mm)).collect()).unwrap_or_default() };
    let version = raw["kicad_version"].as_str().map(str::to_string).unwrap_or_else(|| cli_version(&cli));
    Ok(DrcReport { engine: format!("kicad-cli {version}"), zones_refilled_by_kicad: refilled, violations: read("violations"), unconnected_items: read("unconnected_items") })
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
                    "severity": if excluded { "excluded" } else { v.severity.as_str() },
                    "location": location,
                    "hint": format!("{}{}{}", v.description, if item_desc.is_empty() { "" } else { ": " }, item_desc.join("; ")),
                    "items": v.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.0, i.pos.1], "id": i.id, "uuid": i.uuid })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "engine": self.engine, "violations": violations, "counts": counts })
    }
}

/// `kicad-cli sch erc` on `design`'s schematic.
pub fn erc(design: &Design, model: &ConstraintModel, work: &Path) -> Result<ErcReport, Vec<CheckResult>> {
    let cli = need_cli()?;
    let (sch, map) = export_schematic(design, model, work)?;
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
    Ok(ErcReport { engine: format!("kicad-cli {version}"), violations })
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

fn sch_ext(kind: &str) -> &'static str {
    match kind {
        "netlist" => "net",
        "bom" => "csv",
        "pdf" => "pdf",
        "ps" => "ps",
        "dxf" => "dxf",
        "svg" => "svg",
        "png" => "png",
        _ => "out",
    }
}

/// `sch export` subcommands that write a directory of files (one per sheet).
const SCH_DIR_KINDS: &[&str] = &["svg", "dxf", "png", "ps"];

/// A kicad-cli export that wrote files: their paths relative to `root`.
fn run_export(cli: &Path, scope: &str, kind: &str, args: &[String], input: &Path, out_dir: &Path, dir_kind: bool, ext: &str, root: &Path) -> Result<Value, Vec<CheckResult>> {
    std::fs::create_dir_all(out_dir).map_err(|e| fail("kicad_engine_dir", "export", e.to_string()))?;
    let started = std::time::SystemTime::now() - std::time::Duration::from_secs(1);
    let target = if dir_kind { out_dir.to_path_buf() } else { out_dir.join(format!("board.{ext}")) };
    let mut out_arg = target.to_string_lossy().to_string();
    if dir_kind && !out_arg.ends_with('/') {
        out_arg.push('/');
    }
    let out = Command::new(cli).args([scope, "export", kind]).args(args).arg("-o").arg(&out_arg).arg(input).output().map_err(|e| fail("kicad_cli_run", "kicad-cli", e.to_string()))?;
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
/// `<root>/export/kicad/<kind>/`. `args` are passed through (e.g. `--layers
/// F.Cu,B.Cu`, `--format csv`). Returns `{ ok, engine, files }`, files
/// relative to `root`.
pub fn export_pcb(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    check_kind(kind)?;
    let cli = need_cli()?;
    let (pcb, _) = export_board(design, model, work)?;
    let out_dir = root.join("export").join("kicad").join(kind);
    run_export(&cli, "pcb", kind, args, &pcb, &out_dir, DIR_KINDS.contains(&kind), pcb_ext(kind), root)
}

/// `kicad-cli sch export <kind> [args...]` on `design`'s schematic, into
/// `<root>/export/kicad/sch-<kind>/` (kinds: `netlist`, `bom`, `pdf`, `svg`,
/// `dxf`, `ps`, `png`).
pub fn export_sch(design: &Design, model: &ConstraintModel, work: &Path, root: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    check_kind(kind)?;
    let cli = need_cli()?;
    let (sch, _) = export_schematic(design, model, work)?;
    let out_dir = root.join("export").join("kicad").join(format!("sch-{kind}"));
    run_export(&cli, "sch", kind, args, &sch, &out_dir, SCH_DIR_KINDS.contains(&kind), sch_ext(kind), root)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 1".into(), pos: (0, 0), id: Some("U1.1".into()), uuid: "a".into() }] },
                Violation { kind: "pin_not_connected".into(), description: "Pin not connected".into(), severity: "error".into(), items: vec![Item { description: "Pin 2".into(), pos: (0, 0), id: Some("U1.2".into()), uuid: "b".into() }] },
            ],
        };
        let j = rep.to_json(&[("pin_not_connected".into(), "U1.2".into())]);
        assert_eq!(j["violations"][0]["severity"], "error");
        assert_eq!(j["violations"][1]["severity"], "excluded");
        assert_eq!(j["counts"]["pin_not_connected"], 1);
        assert_eq!(j["violations"][0]["location"], "U1.1");
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
}
