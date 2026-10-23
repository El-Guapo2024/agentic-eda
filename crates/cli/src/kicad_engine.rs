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
        // The derived design is a schematic -- the root and the module sheets' screens -- and nothing else: the board's own sections (the placement and
        // routing a parity DRC compares, the waivers in `drawings`) stay as they are.
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        if design.sheet_contents.is_none() {
            design.sheet_contents = derived.sheet_contents;
        }
        if design.bus_aliases.is_empty() {
            design.bus_aliases = derived.bus_aliases;
        }
    }
    Ok((design, model))
}

/// `kicad-cli pcb drc` on the current design, without the parity test: [`drc_with`] with both options off. (The tests call this one.)
#[allow(dead_code)]
pub fn drc(dir: &Path, refill_zones: bool) -> Result<Value, Vec<CheckResult>> {
    drc_with(dir, refill_zones, false)
}

/// `kicad-cli pcb drc` on the current design: `{ engine, violations, unconnected_items, schematic_parity, ignored_checks, counts }`.
/// `refill_zones`: see [`eda_kicad_engine::drc`] (off by default: kicad-cli's filler runs first, on a copy, and the fills it makes are kept like Fill All Zones').
///
/// With the dialog's "Test for parity between PCB and schematic" (`schematic_parity`, `--schematic-parity`): the design's schematic -- the one
/// the engine would derive from the intent when none is stored yet, as for ERC -- goes beside the board for kicad-cli to compare with,
/// and the report's `schematic_parity` holds what differs.
///
/// The violations the design waived (`design.drawings.drc_exclusions`) are in the derived project, so kicad-cli matches the ones it can;
/// the report marks them `excluded` here either way (`DrcReport::to_json_with`).
pub fn drc_with(dir: &Path, refill_zones: bool, schematic_parity: bool) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = if schematic_parity { load_with_schematic(dir)? } else { board::load(dir).map(|(_, d, m)| (d, m))? };
    let waived = design.drawings.as_ref().map(|d| d.drc_exclusions.clone()).unwrap_or_default();
    let revision = crate::fill_api::design_stamp(dir);
    let mut report = eda_kicad_engine::drc_with(&design, &model, &work(dir), eda_kicad_engine::DrcOptions { refill_zones, schematic_parity })?;
    // "Refill all zones before performing DRC" made fills: they are the board's now, kept beside the design like the ones Fill All Zones makes.
    if let Some(mut fills) = report.zone_fills.take() {
        fills.revision = revision;
        let _ = eda_kicad::zone_fills::store(dir, &fills);
    }
    Ok(report.to_json_with(&waived))
}

/// `kicad-cli sch erc` on the current schematic, in the studio's ERC shape
/// (`check`, `severity`, `location` = the first item's id, `hint`) plus the
/// full mapped `items` list. A finding the schematic's own exclusion list
/// accepts reports as `excluded`, with its comment; the ones whose marker text
/// is known are in the derived project (`erc.erc_exclusions`), so kicad-cli
/// matches them itself (`ErcViolation::kicad_matched`).
pub fn erc(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let exclusions = design.schematic.as_ref().map(|s| s.erc_exclusions.clone()).unwrap_or_default();
    Ok(eda_kicad_engine::erc(&design, &model, &work(dir))?.to_json(&exclusions))
}

/// What `POST /api/erc/report` and `/api/drc/report` ask for: `{ format: "report" | "json", errors, warnings, exclusions, units: "mm" | "mil" | "in" }` -- the
/// dialog's file type and Show boxes -- and, for the DRC, `refill_zones` and `schematic_parity`, the dialog's own checkboxes.
fn report_options(body: &Value) -> eda_kicad_engine::ReportOptions {
    let flag = |k: &str| body[k].as_bool().unwrap_or(false);
    eda_kicad_engine::ReportOptions {
        json: body["format"].as_str() == Some("json"),
        errors: flag("errors"),
        warnings: flag("warnings"),
        exclusions: flag("exclusions"),
        units: match body["units"].as_str() {
            Some("mil" | "mils") => eda_kicad_engine::ReportUnits::Mils,
            Some("in") => eda_kicad_engine::ReportUnits::In,
            _ => eda_kicad_engine::ReportUnits::Mm,
        },
    }
}

fn saved(report: Result<eda_kicad_engine::SavedReport, Vec<CheckResult>>) -> Value {
    match report {
        Ok(r) => json!({ "ok": true, "file_name": r.file_name, "report": r.text }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

/// `POST /api/erc/report`: the ERC dialog's "Save..." -- `kicad-cli sch erc` writes the report in the asked format (`ERC.rpt` text, `ERC.json`) for the
/// severities the dialog shows, and the text comes back for the page to save: `{ ok, file_name, report }`.
pub fn erc_report(dir: &Path, body: &[u8]) -> Value {
    let body: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (design, model) = match load_with_schematic(dir) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    saved(eda_kicad_engine::erc_report(&design, &model, &work(dir), &report_options(&body)))
}

/// `POST /api/drc/report`: the DRC dialog's "Save..." (`DRC.rpt`, `DRC.json`), with the dialog's refill and parity options.
pub fn drc_report(dir: &Path, body: &[u8]) -> Value {
    let body: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let parity = body["schematic_parity"].as_bool().unwrap_or(false);
    let loaded = if parity { load_with_schematic(dir) } else { board::load(dir).map(|(_, d, m)| (d, m)) };
    let (design, model) = match loaded {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let drc = eda_kicad_engine::DrcOptions { refill_zones: body["refill_zones"].as_bool().unwrap_or(false), schematic_parity: parity };
    saved(eda_kicad_engine::drc_report(&design, &model, &work(dir), drc, &report_options(&body)))
}

/// Today's date (`YYYY-MM-DD`), for the derived-file header the studio's
/// "Save As" routes write; the one clock helper lives in the engine crate.
pub(crate) fn chrono_like_today() -> String {
    eda_kicad_engine::today()
}

/// The board's project name: its intent's file stem, as every file written for
/// it has always been named.
pub(crate) fn project_name(dir: &Path) -> String {
    board::load(dir).ok().and_then(|(meta, _, _)| Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).map(str::to_string)).unwrap_or_else(|| "board".to_string())
}

/// The board's project name (the derived files' stem), for the simulator's default workbook name (`SIMULATOR_CONTROL::getDefaultFilename`).
pub fn project_name_of(dir: &Path) -> String {
    project_name(dir)
}

/// kicad-cli's SPICE and XML netlists of the board's schematic, for the simulator (`crate::sim_api`), with the design they were made from.
pub fn sim_netlists(dir: &Path) -> Result<(Design, eda_kicad_engine::SimNetlists), Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let n = eda_kicad_engine::sim_netlists(&design, &model, &work(dir), &project_name(dir))?;
    Ok((design, n))
}

/// The exporters the studio offers only where the kicad-cli in use has them (File > Export > IDFv3... and GenCAD...), by their
/// `pcb export` command, with the name a message calls each one by.
pub const OPTIONAL_PCB_EXPORTS: [(&str, &str); 2] = [("idf", "IDF"), ("gencad", "GenCAD")];

/// Why `kicad-cli pcb export <kind>` cannot run here, or `None` when it can: "This kicad-cli (10.99.0) has no IDF exporter; KiCad 10.99
/// nightly or later has one." Only the kinds of [`OPTIONAL_PCB_EXPORTS`] are asked about (`eda_kicad_engine::cli_has_export`, one
/// `kicad-cli pcb export --help` per binary per process); with no kicad-cli at all nothing is refused here, and the export says it is missing.
pub fn missing_pcb_export(kind: &str) -> Option<String> {
    let (_, name) = OPTIONAL_PCB_EXPORTS.iter().find(|(k, _)| *k == kind)?;
    let cli = eda_kicad_engine::find_cli()?;
    if eda_kicad_engine::cli_has_export(&cli, "pcb", kind) {
        return None;
    }
    let version = eda_kicad_engine::cli_version(&cli);
    Some(format!("This kicad-cli{} has no {name} exporter; KiCad 10.99 nightly or later has one.", if version.is_empty() { String::new() } else { format!(" ({version})") }))
}

/// `kicad-cli pcb export <kind> [args...]`: `{ ok, engine, files }`. IDF or GenCAD on a kicad-cli that does not have it is refused with
/// [`missing_pcb_export`]'s reason before anything is written.
pub fn export(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    if let Some(why) = missing_pcb_export(kind) {
        return Err(vec![CheckResult::fail("kicad_cli_export", kind, why)]);
    }
    let (_, design, model) = board::load(dir)?;
    eda_kicad_engine::export_pcb(&design, &model, &work(dir), dir, &project_name(dir), kind, args)
}

/// `kicad-cli sch export <kind> [args...]` (`netlist`, `bom`, `pdf`, `svg`,
/// `dxf`, `ps`, `png`): `{ ok, engine, files }`.
pub fn export_sch(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    eda_kicad_engine::export_sch(&design, &model, &work(dir), dir, &project_name(dir), kind, args)
}

/// `kicad-cli sch export bom` with the Symbol Fields Table's settings as KiCad's `BOM_PRESET` / `BOM_FMT_PRESET` JSON, into `output` (a path relative to the
/// board directory; default `export/kicad/sch-bom/<name>.csv`): `{ ok, engine, files }`.
pub fn export_sch_bom(dir: &Path, preset: &Value, fmt: &Value, output: Option<&Path>) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    eda_kicad_engine::export_sch_bom(&design, &model, &work(dir), dir, &project_name(dir), preset, fmt, output.map(|o| dir.join(o)).as_deref())
}

/// `kicad-cli sym export svg [args...]` on `kicad_sym` (a library file's text): `{ ok, engine, files }`, one SVG per unit.
pub fn export_symbol_svg(dir: &Path, name: &str, kicad_sym: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    eda_kicad_engine::export_symbol_svg(&work(dir), dir, name, kicad_sym, args)
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

/// `POST /api/check_rules` (Board Setup > Custom Rules, "Check syntax"): `{ text }`, a `.kicad_dru` text, answered with
/// kicad-cli's verdict on it (`eda_kicad_engine::check_rules`): `{ ok, engine, valid, forms, bad, message }`. Nothing here
/// reads a rule; the board is not involved.
pub fn check_rules(_dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(text) = req.get("text").and_then(Value::as_str) else {
        return json!({ "ok": false, "message": "check_rules needs {\"text\": the rules text}" });
    };
    eda_kicad_engine::check_rules(text).map(|c| c.to_json()).unwrap_or_else(|e| json!({ "ok": false, "message": board::reasons(&e) }))
}

/// `POST /api/footprint/check` (the Footprint Editor's Inspect > Footprint Checker, `DIALOG_FOOTPRINT_CHECKER`): `{ name }`, the footprint open in the editor (its project
/// library entry, else what the libraries resolve the name to), checked by kicad-cli's DRC on a scratch board that holds it (`eda_kicad_engine::footprint_check`):
/// `{ ok, engine, markers: [{ type, description, severity, at, items }], counts }`. Nothing here checks a footprint; the board is not involved.
pub fn footprint_check(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(name) = req.get("name").and_then(Value::as_str) else {
        return json!({ "ok": false, "message": "footprint_check needs {\"name\": the footprint}" });
    };
    let run = || -> Result<Value, Vec<CheckResult>> {
        let (fp, _) = crate::library_api::footprint_any(dir, name)?;
        Ok(eda_kicad_engine::footprint_check::footprint_check(&fp)?.to_json())
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
