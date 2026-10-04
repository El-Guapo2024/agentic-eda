//! A board directory's face of kicad-cli (docs/ARCHITECTURE.md, "Engines"):
//! load the board, hand its current `design.json` revision to
//! `eda_kicad_engine`, return the report as the JSON the studio and the CLI
//! print. Derived files go to `.kicad/` beside `design.json` (scratch,
//! rewritten on every run); exports go to `export/kicad/<kind>/`.
//!
//! DRC, ERC and every output (plots, Gerbers, drill, position, STEP,
//! netlist, BOM, ...) are kicad-cli's. Our own checks, the ones KiCad does
//! not have, are `eda-lint`'s (see [`lint`]).

use crate::board;
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where this board's derived KiCad files go.
fn work(dir: &Path) -> PathBuf {
    dir.join(".kicad")
}

/// The board's design and model, with the schematic the engine would derive
/// from the intent when none is stored yet -- so ERC, plots and netlists
/// work on a fresh board, same as `GET /api/schematic.svg`.
pub fn load_with_schematic(dir: &Path) -> Result<(Design, ConstraintModel), Vec<CheckResult>> {
    let (_, mut design, model) = board::load(dir)?;
    if design.schematic.is_none() {
        design = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?;
    }
    Ok((design, model))
}

/// `kicad-cli pcb drc` on the current design: `{ engine, violations,
/// unconnected_items, counts }`. `refill_zones`: see
/// [`eda_kicad_engine::drc`] (off by default; kicad-cli drops its courtyard
/// checks when it refills).
pub fn drc(dir: &Path, refill_zones: bool) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    Ok(eda_kicad_engine::drc(&design, &model, &work(dir), refill_zones)?.to_json())
}

/// `kicad-cli sch erc` on the current schematic, in the studio's ERC shape
/// (`check`, `severity`, `location` = the first item's id, `hint`) plus the
/// full mapped `items` list. A finding the schematic's own exclusion list
/// accepts reports as `excluded`.
pub fn erc(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let exclusions: Vec<(String, String)> = design.schematic.as_ref().map(|s| s.erc_exclusions.iter().map(|e| (e.check.clone(), e.location.clone())).collect()).unwrap_or_default();
    Ok(eda_kicad_engine::erc(&design, &model, &work(dir))?.to_json(&exclusions))
}

/// `kicad-cli pcb export <kind> [args...]`: `{ ok, engine, files }`.
pub fn export(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    eda_kicad_engine::export_pcb(&design, &model, &work(dir), dir, kind, args)
}

/// `kicad-cli sch export <kind> [args...]` (`netlist`, `bom`, `pdf`, `svg`,
/// `dxf`, `ps`, `png`): `{ ok, engine, files }`.
pub fn export_sch(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    eda_kicad_engine::export_sch(&design, &model, &work(dir), dir, kind, args)
}

/// Our own checks, the ones KiCad does not have: `{ pcb: { violations,
/// counts }, schematic: { violations, counts } }`. `pcb` is shaped like a
/// DRC report (`type`, `description`, `severity`, `items`, `fix`);
/// `schematic` like an ERC one (`check`, `severity`, `location`, `hint`).
/// Cheap and in-process, so the studio runs it on every refresh.
pub fn lint(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let pcb = eda_lint::check_pcb(&design, &model);
    let pcb_counts = eda_lint::counts(&pcb);
    let mut sch_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let sch_checks = eda_lint::check_schematic(&design, &model);
    let schematic: Vec<Value> = sch_checks
        .iter()
        .filter(|c| !matches!(c.status, eda_model::CheckStatus::Pass))
        .map(|c| {
            *sch_counts.entry(c.check.as_str()).or_default() += 1;
            json!({
                "check": c.check,
                "severity": match c.status { eda_model::CheckStatus::Fail => "error", _ => "warning" },
                "location": c.location,
                "hint": c.hint,
            })
        })
        .collect();
    Ok(json!({
        "pcb": { "violations": pcb, "counts": pcb_counts },
        "schematic": { "violations": schematic, "counts": sch_counts },
    }))
}

// ------------------------------------------------------------- board statistics

/// kicad-cli prints a length as `"26.3850 mm"` and an area as `"664.401 mm²"`
/// (or `in`/`in²`): the number and its unit, in micrometres / square
/// micrometres, which is what the studio keeps everything in.
fn quantity_um(s: &str, area: bool) -> f64 {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')).unwrap_or(s.len());
    let n: f64 = s[..split].parse().unwrap_or(0.0);
    let per_unit = match s[split..].trim().trim_end_matches(['²', '2']) {
        "in" => 25_400.0,
        "mil" => 25.4,
        _ => 1_000.0,
    };
    if area { n * per_unit * per_unit } else { n * per_unit }
}

fn entries(titles: &[(&str, &str)], section: &Value) -> Vec<Value> {
    titles.iter().map(|(title, key)| json!({ "title": title, "qty": section[*key].as_i64().unwrap_or(0) })).collect()
}

/// kicad-cli's `pcb export stats` JSON in the Board Statistics dialog's
/// shape (`BoardStatsReply`): lengths in um, areas in um^2.
pub(crate) fn stats_reply(j: &Value) -> Value {
    let b = &j["board"];
    let len = |k: &str| quantity_um(b[k].as_str().unwrap_or("0"), false);
    let area = |k: &str| quantity_um(b[k].as_str().unwrap_or("0"), true);
    let pct = |k: &str| b[k].as_str().unwrap_or("0").trim().trim_end_matches('%').trim().parse::<f64>().unwrap_or(0.0);
    let comp = |kind: &str| json!({ "front": j["components"][kind]["front"].as_i64().unwrap_or(0), "back": j["components"][kind]["back"].as_i64().unwrap_or(0) });
    let footprints: Vec<Value> = [("THT:", "tht"), ("SMD:", "smd"), ("Unspecified:", "unspecified")]
        .iter()
        .map(|(title, kind)| {
            let mut c = comp(kind);
            c["title"] = json!(title);
            c
        })
        .collect();
    let drills: Vec<Value> = j["drill_holes"]
        .as_array()
        .map(|ds| {
            ds.iter()
                .map(|d| {
                    let layer = |k: &str| d[k].as_str().filter(|l| !l.is_empty() && *l != "N/A").map(str::to_string);
                    json!({
                        "qty": d["count"].as_i64().unwrap_or(0),
                        "shape": if d["shape"].as_str().unwrap_or("Round").eq_ignore_ascii_case("round") { "round" } else { "slot" },
                        "x_um": quantity_um(d["x_size"].as_str().unwrap_or("0"), false),
                        "y_um": quantity_um(d["y_size"].as_str().unwrap_or("0"), false),
                        "plated": d["plated"].as_bool().unwrap_or(false),
                        "is_pad": d["source"].as_str() == Some("Pad"),
                        "start_layer": layer("start_layer"),
                        "stop_layer": layer("stop_layer"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "ok": true,
        "board": {
            "has_outline": b["has_outline"].as_bool().unwrap_or(false),
            "width_um": len("width"),
            "height_um": len("height"),
            "area_um2": area("area"),
            "front_copper_area_um2": area("front_copper_area"),
            "back_copper_area_um2": area("back_copper_area"),
            "front_courtyard_area_um2": area("front_footprint_area"),
            "back_courtyard_area_um2": area("back_footprint_area"),
            "front_density_pct": pct("front_component_density"),
            "back_density_pct": pct("back_component_density"),
            "min_track_width_um": len("min_track_width"),
            "min_clearance_um": len("min_track_clearance"),
            "min_drill_um": len("min_drill_diameter"),
            "thickness_um": len("board_thickness"),
        },
        "footprints": footprints,
        "pads": entries(&[("Through hole:", "through_hole"), ("SMD:", "smd"), ("Connector:", "connector"), ("NPTH:", "npth")], &j["pads"]),
        "pad_properties": entries(&[("Castellated:", "castellated"), ("Press-fit:", "press_fit")], &j["pads"]),
        "vias": entries(&[("Through vias:", "through"), ("Blind vias:", "blind"), ("Buried vias:", "buried"), ("Micro vias:", "micro")], &j["vias"]),
        "drills": drills,
    })
}

/// `POST /api/board_stats` (the Board Statistics dialog): `{exclude_footprints_without_pads,
/// subtract_holes_from_board_area, subtract_holes_from_copper_areas}` and, to
/// save the report, `{report: true, units: "mm"|"in"|"mils"}`. kicad-cli computes it
/// (`pcb export stats`), nothing here measures a board.
pub fn board_stats(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let want_report = flag("report");
    let opts = eda_kicad_engine::StatsOptions {
        exclude_footprints_without_pads: flag("exclude_footprints_without_pads"),
        subtract_holes_from_board: flag("subtract_holes_from_board_area"),
        subtract_holes_from_copper: flag("subtract_holes_from_copper_areas"),
        inches: matches!(req.get("units").and_then(Value::as_str), Some("in") | Some("mils")),
    };
    let run = || -> Result<Value, Vec<CheckResult>> {
        let (meta, design, model) = board::load(dir)?;
        let text = eda_kicad_engine::stats(&design, &model, &work(dir), opts, true)?;
        let j: Value = serde_json::from_str(&text).map_err(|e| vec![CheckResult::fail("kicad_cli_stats", "stats.json", e.to_string())])?;
        let mut reply = stats_reply(&j);
        if want_report {
            reply["report"] = Value::String(eda_kicad_engine::stats(&design, &model, &work(dir), opts, false)?);
            let name = std::path::PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
            reply["report_file_name"] = Value::String(format!("{name}_report.txt"));
        }
        Ok(reply)
    };
    run().unwrap_or_else(|e| json!({ "ok": false, "message": board::reasons(&e) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kicad_clis_quantities_become_micrometres() {
        assert_eq!(quantity_um("26.3850 mm", false), 26_385.0);
        assert_eq!(quantity_um("0.2000 mm", false), 200.0);
        assert_eq!(quantity_um("664.401 mm\u{b2}", true), 664_401_000.0);
        assert_eq!(quantity_um("0.5 in", false), 12_700.0);
        assert_eq!(quantity_um("1 in\u{b2}", true), 25_400.0 * 25_400.0);
    }

    #[test]
    fn the_stats_json_fills_the_dialogs_reply() {
        let j = json!({
            "board": { "has_outline": true, "width": "26.3850 mm", "height": "25.1810 mm", "area": "664.401 mm\u{b2}", "front_copper_area": "124.712 mm\u{b2}", "back_copper_area": "32.81 mm\u{b2}",
                       "front_component_density": "2.18", "back_component_density": "1.00", "min_track_clearance": "0.2030 mm", "min_track_width": "0.2000 mm", "min_drill_diameter": "0.3000 mm",
                       "board_thickness": "1.6000 mm", "front_footprint_area": "14.498 mm\u{b2}", "back_footprint_area": "7 mm\u{b2}" },
            "pads": { "through_hole": 4, "smd": 72, "connector": 1, "npth": 2, "castellated": 0, "press_fit": 3 },
            "vias": { "through": 12, "blind": 0, "buried": 0, "micro": 0 },
            "components": { "tht": { "front": 1, "back": 2 }, "smd": { "front": 3, "back": 4 }, "unspecified": { "front": 30, "back": 0 } },
            "drill_holes": [
                { "count": 12, "shape": "Round", "x_size": "0.3000 mm", "y_size": "0.3000 mm", "plated": true, "source": "Via", "start_layer": "F.Cu", "stop_layer": "B.Cu" },
                { "count": 2, "shape": "Oblong", "x_size": "1.0000 mm", "y_size": "2.0000 mm", "plated": false, "source": "Pad", "start_layer": "N/A", "stop_layer": "" },
            ],
        });
        let r = stats_reply(&j);
        assert_eq!(r["ok"], true);
        assert_eq!(r["board"]["width_um"], 26_385.0);
        assert_eq!(r["board"]["area_um2"], 664_401_000.0);
        assert_eq!(r["board"]["front_density_pct"], 2.18);
        assert_eq!(r["board"]["min_clearance_um"], 203.0);
        assert_eq!(r["board"]["thickness_um"], 1_600.0);
        assert_eq!(r["board"]["front_courtyard_area_um2"], 14_498_000.0);
        assert_eq!(r["footprints"][0], json!({ "title": "THT:", "front": 1, "back": 2 }));
        assert_eq!(r["footprints"][2]["front"], 30);
        assert_eq!(r["pads"][0], json!({ "title": "Through hole:", "qty": 4 }));
        assert_eq!(r["pads"][3], json!({ "title": "NPTH:", "qty": 2 }));
        assert_eq!(r["pad_properties"][1], json!({ "title": "Press-fit:", "qty": 3 }));
        assert_eq!(r["vias"][0], json!({ "title": "Through vias:", "qty": 12 }));
        assert_eq!(r["drills"][0]["is_pad"], false);
        assert_eq!(r["drills"][0]["start_layer"], "F.Cu");
        assert_eq!(r["drills"][1]["shape"], "slot");
        assert_eq!(r["drills"][1]["plated"], false);
        assert_eq!(r["drills"][1]["is_pad"], true);
        assert!(r["drills"][1]["start_layer"].is_null() && r["drills"][1]["stop_layer"].is_null(), "N/A and empty layers are null");
    }
}
