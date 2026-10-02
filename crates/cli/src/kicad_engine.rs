//! kicad-cli as the batch engine (docs/ARCHITECTURE.md): `design.json` is
//! exported to a derived `.kicad/` folder beside it, kicad-cli runs on that
//! export, and its JSON report is pointed back at our own item ids through
//! the exporter's uuid map (`eda_kicad::export_kicad_pcb_mapped`).
//!
//! The `.kicad/` files are scratch output, rewritten on every run -- never
//! edited, never read back as a design.

use crate::board;
use eda_model::CheckResult;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fail(check: &str, location: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, msg.into())]
}

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

/// `kicad-cli version`, e.g. "9.0.9".
pub fn cli_version(cli: &Path) -> String {
    Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn work_dir(dir: &Path) -> Result<PathBuf, Vec<CheckResult>> {
    let w = dir.join(".kicad");
    std::fs::create_dir_all(&w).map_err(|e| fail("kicad_engine_dir", ".kicad", format!("cannot create {}: {e}", w.display())))?;
    Ok(w)
}

/// Export the current design as `board.kicad_pcb` + `board.kicad_pro`;
/// returns the pcb path and the uuid -> our id map.
fn export_board(dir: &Path) -> Result<(PathBuf, std::collections::HashMap<String, String>), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let date = chrono_like_today();
    let (pcb, map) = eda_kicad::export_kicad_pcb_mapped(&design, &model, &eda_kicad::ExportMeta { date: &date, title: "board" })?;
    let w = work_dir(dir)?;
    let pcb_path = w.join("board.kicad_pcb");
    std::fs::write(&pcb_path, pcb).map_err(|e| fail("kicad_engine_write", "board.kicad_pcb", e.to_string()))?;
    std::fs::write(w.join("board.kicad_pro"), eda_kicad::export_kicad_pro(&model)).map_err(|e| fail("kicad_engine_write", "board.kicad_pro", e.to_string()))?;
    Ok((pcb_path, map))
}

fn chrono_like_today() -> String {
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

/// One kicad-cli report entry, in the studio's `DrcReport` shape, with each
/// item's KiCad uuid resolved to our id (`null` when it isn't one of ours,
/// e.g. a zone fill KiCad computed).
fn map_violation(v: &Value, map: &std::collections::HashMap<String, String>, units_mm: bool) -> Value {
    let to_um = |x: f64| if units_mm { (x * 1000.0).round() as i64 } else { (x * 25_400.0).round() as i64 };
    let items: Vec<Value> = v["items"]
        .as_array()
        .map(|is| {
            is.iter()
                .map(|it| {
                    let uuid = it["uuid"].as_str().unwrap_or("");
                    json!({
                        "description": it["description"],
                        "pos": [to_um(it["pos"]["x"].as_f64().unwrap_or(0.0)), to_um(it["pos"]["y"].as_f64().unwrap_or(0.0))],
                        "id": map.get(uuid),
                        "uuid": uuid,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "type": v["type"], "description": v["description"], "severity": v["severity"], "items": items })
}

/// `kicad-cli pcb drc` on the current design: `{ engine, violations,
/// unconnected_items, counts }`. Zones are refilled first, as KiCad's own
/// DRC dialog does.
pub fn drc(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let cli = find_cli().ok_or_else(|| fail("kicad_cli_missing", "kicad-cli", "kicad-cli not found (set EDA_KICAD_CLI or install KiCad 9)"))?;
    let (pcb, map) = export_board(dir)?;
    let report = pcb.with_file_name("drc.json");
    // `--refill-zones` exists only in newer kicad-cli (not 9.0); without it
    // KiCad checks the fills our exporter wrote (`eda_zone_filler`).
    let refill = Command::new(&cli).args(["pcb", "drc", "--help"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("--refill-zones") || String::from_utf8_lossy(&o.stderr).contains("--refill-zones")).unwrap_or(false);
    let mut cmd = Command::new(&cli);
    cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm"]);
    if refill {
        cmd.arg("--refill-zones");
    }
    let out = cmd
        .arg("-o")
        .arg(&report)
        .arg(&pcb)
        .output()
        .map_err(|e| fail("kicad_cli_run", "kicad-cli", e.to_string()))?;
    let text = std::fs::read_to_string(&report).map_err(|_| fail("kicad_cli_drc", "kicad-cli", format!("no report: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| fail("kicad_cli_drc", "drc.json", e.to_string()))?;
    let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
    let violations: Vec<Value> = raw["violations"].as_array().map(|vs| vs.iter().map(|v| map_violation(v, &map, units_mm)).collect()).unwrap_or_default();
    let unconnected: Vec<Value> = raw["unconnected_items"].as_array().map(|vs| vs.iter().map(|v| map_violation(v, &map, units_mm)).collect()).unwrap_or_default();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for v in violations.iter().chain(unconnected.iter()) {
        *counts.entry(v["type"].as_str().unwrap_or("?").to_string()).or_default() += 1;
    }
    Ok(json!({
        "engine": format!("kicad-cli {}", cli_version(&cli)),
        "zones_refilled_by_kicad": refill,
        "violations": violations,
        "unconnected_items": unconnected,
        "counts": counts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_report_items_back_to_our_ids() {
        let mut map = std::collections::HashMap::new();
        map.insert("u-1".to_string(), "trk_1#2".to_string());
        let v = json!({ "type": "clearance", "description": "x", "severity": "error",
            "items": [{ "description": "Track", "pos": { "x": 1.5, "y": -2.0 }, "uuid": "u-1" },
                      { "description": "Zone", "pos": { "x": 0.0, "y": 0.0 }, "uuid": "u-9" }] });
        let m = map_violation(&v, &map, true);
        assert_eq!(m["items"][0]["id"], "trk_1#2");
        assert_eq!(m["items"][0]["pos"], json!([1500, -2000]));
        assert!(m["items"][1]["id"].is_null());
    }

    #[test]
    fn today_is_a_date() {
        let d = chrono_like_today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
    }
}
