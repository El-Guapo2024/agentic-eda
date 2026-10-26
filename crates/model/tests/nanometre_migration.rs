//! A design file and an intent written by the micrometre IR (`schema` 1, or none) are read with every length x1000, once; one written now (`schema` 2) is
//! read as it is. See `docs/NANOMETRES.md`.
//!
//! The fixtures in `tests/data/v1` were written by the binary of main before the change (`be562f2`): KiCad QA boards and schematics brought in with
//! `eda import-kicad`, and boards made with `eda board new`. Each is read with the current code and compared, number by number, with the same document
//! with its lengths multiplied by 1000 -- what is a length is said here, by key, independently of the `crate::units::de` annotations the reader uses, so
//! a length field the reader missed (or a count, an angle or a ratio it scaled) fails the comparison.

use eda_model::units::Nm;
use eda_model::ir::{Design, SCHEMA_VERSION};
use eda_model::ConstraintModel;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Integer fields that are not lengths: angles (millidegrees), counts, indices, priorities, ratios, colours, scales, attoseconds -- and the two
/// positions KiCad's own reports give, which were nanometres already.
const NOT_LENGTHS: &[&str] = &[
    "angle", "rot", "halign", "valign", "precision", "symbol_column", "shape_index", "scale_ppm", "nth", "thermal_spoke_angle_mdeg", "angle_file_mdeg",
    "text_angle", "hatch_orientation_mdeg", "hatch_smoothing_level", "hatch_border_algorithm", "priority", "corner_radius_percent",
    "corner_radius_percentage", "last_tuning_length", "last_tuning_delay", "r", "g", "b", "a", "unit", "body_style", "unit_count", "seed",
    "min_resolved_spokes", "optimizer_passes", "preflight_reach_cells", "delay_as_per_mm", "delay_as", "via_propagation_delay_as", "place_attempts",
    "place_moves_per_part", "edge_connector", "line_spacing_permille", "copper_layers", "tick_interval", "seed", "virtual_page", "schema",
];

/// Containers whose every number is not a length.
const NOT_LENGTH_CONTAINERS: &[&str] = &["at_nm", "positions_nm", "target_delay", "target_skew_delay", "row_shapes", "excluded_cells", "body_styles", "color", "fill_color"];

/// Areas: x1e6.
const AREAS: &[&str] = &["min_island_area"];

/// The document as the current code should read it: `_um` keys become `_nm`, lengths x1000, areas x1e6, `schema` 2.
fn expected(v: &Value, path: &[String]) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                // a per-placement reference written before the whole sheet path was kept: its one id is the path (`SymbolPathOverride`)
                if k == "parent_sheet_instance_id" {
                    out.insert("sheet_path".into(), json!([x]));
                    continue;
                }
                let key = k.strip_suffix("_um").map(|b| format!("{b}_nm")).unwrap_or_else(|| k.clone());
                let mut p = path.to_vec();
                p.push(k.clone());
                out.insert(key, expected(x, &p));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| expected(x, path)).collect()),
        Value::Number(n) if n.is_i64() || n.is_u64() => {
            let leaf = path.last().map(String::as_str).unwrap_or("");
            let i = n.as_i64().expect("an i64");
            if leaf == "schema" {
                json!(SCHEMA_VERSION)
            } else if path.iter().any(|k| NOT_LENGTH_CONTAINERS.contains(&k.as_str())) || NOT_LENGTHS.contains(&leaf) {
                v.clone()
            } else if AREAS.contains(&leaf) {
                json!(i * 1_000_000)
            } else if (leaf == "width_um" || leaf == "width_nm") && i < 0 && path.iter().any(|k| k == "stroke") {
                v.clone() // a table's stroke of -1 is "none", not a length
            } else {
                json!(i * 1000)
            }
        }
        _ => v.clone(),
    }
}

/// The first place two documents differ, for a failure's message.
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = first_difference(p, q, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                    (p, q) => return Some(format!("{path}.{k}: {p:?} against {q:?}")),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_difference(p, q, &format!("{path}[{i}]"))),
        _ => (a != b).then(|| format!("{path}: {a} against {b}")),
    }
}

fn fixtures(ext: &str) -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/v1");
    let mut out: Vec<_> = std::fs::read_dir(&dir).expect("tests/data/v1").map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(ext)).collect();
    out.sort();
    out
}

/// Every key path of `v` whose number is not zero, indices dropped (`routing.zones[].outline[].x`).
fn number_paths(v: &Value, path: String, out: &mut std::collections::BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                // a map's own key (a reference, a layer, an item id such as `jct_c319b29b1558`) is `*`
                let id_like = k.len() > 13 && k.as_bytes()[k.len() - 13] == b'_' && k[k.len() - 12..].bytes().all(|c| c.is_ascii_hexdigit());
                let field_like = k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !k.chars().next().is_some_and(|c| c.is_ascii_digit());
                let k = if field_like && !id_like { k.clone() } else { "*".into() };
                number_paths(x, if path.is_empty() { k } else { format!("{path}.{k}") }, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| number_paths(x, format!("{path}[]"), out)),
        Value::Number(n) if n.as_f64() != Some(0.0) => {
            out.insert(path);
        }
        _ => {}
    }
}

/// Each `sheet_path` cut to its last id, the one a file written before the whole path was kept says (`parent_sheet_instance_id`).
fn tail_of_sheet_paths(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "sheet_path" {
                    if let Value::Array(a) = x {
                        a.drain(..a.len().saturating_sub(1));
                    }
                } else {
                    tail_of_sheet_paths(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(tail_of_sheet_paths),
        _ => {}
    }
}

#[test]
fn every_design_written_in_micrometres_is_read_in_nanometres() {
    let files = fixtures(".json");
    assert!(files.len() >= 15, "{files:?}");
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let old: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(old["schema"], json!(1), "{}: a fixture is a version 1 file", f.display());
        let design: Design = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert_eq!(design.schema, SCHEMA_VERSION);
        // every sheet path gives each symbol the reference the old key gave it: the override filed under the path's last placement, else its own
        for e in eda_model::sheet_list::walk(&design).into_iter().skip(1) {
            let screen = eda_model::sheet_list::screen_of(&design, &e.path).unwrap();
            let raw = old["sheet_contents"][&e.file]["instance_overrides"].as_array().cloned().unwrap_or_default();
            for sym in &screen.symbols {
                let was = raw
                    .iter()
                    .find(|o| o["parent_sheet_instance_id"].as_str() == e.path.last().map(String::as_str) && o["at"]["x"].as_i64().unwrap() * 1000 == sym.at.x.get() && o["at"]["y"].as_i64().unwrap() * 1000 == sym.at.y.get() && o["unit"].as_u64().unwrap_or(1) == u64::from(sym.unit))
                    .and_then(|o| o["reference"].as_str())
                    .unwrap_or(&sym.id);
                assert_eq!(eda_model::sheet_list::reference_under(screen, sym, &e.path), was, "{}: {} under {:?}", f.display(), sym.id, e.path);
            }
        }
        let mut read = serde_json::to_value(&design).unwrap();
        // the old key is read as the whole sheet path (checked above, and the whole path below); the lengths are what is compared here
        tail_of_sheet_paths(&mut read);
        let want = expected(&old, &[]);
        if read != want {
            // name the first number that differs
            let (mut a, mut b) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
            number_paths(&read, String::new(), &mut a);
            number_paths(&want, String::new(), &mut b);
            let r = serde_json::to_string_pretty(&read).unwrap();
            let w = serde_json::to_string_pretty(&want).unwrap();
            let line = r.lines().zip(w.lines()).enumerate().find(|(_, (x, y))| x != y).map(|(i, (x, y))| format!("line {i}: read {x:?}, expected {y:?}"));
            panic!("{}: not every length came out x1000: {line:?}; paths only read: {:?}; only expected: {:?}", f.display(), a.difference(&b).collect::<Vec<_>>(), b.difference(&a).collect::<Vec<_>>());
        }
    }
}

/// `issue17771`'s sub3 is placed by `38a5c293..` on sub2, which the root places once by `4fea18b4..`: its old key, the immediate placement alone, is read as
/// the whole path.
#[test]
fn an_old_key_inside_a_sheet_is_read_as_the_whole_path() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/v1/eeschema-issue17771-sub.sch.json")).unwrap();
    let design: Design = serde_json::from_str(&text).unwrap();
    let screens = design.sheet_contents.as_ref().unwrap();
    let path = |file: &str| -> Vec<Vec<String>> { screens[file].instance_overrides.iter().map(|o| o.sheet_path.clone()).collect() };
    let whole = vec!["4fea18b4-6b3d-4acc-a71b-7ee31c8961ac".to_string(), "38a5c293-46d2-4c74-939b-43575208aa56".to_string()];
    assert_eq!(path("sub3.kicad_sch"), vec![whole.clone(), whole]);
    assert_eq!(path("sub2.kicad_sch"), vec![vec!["4fea18b4-6b3d-4acc-a71b-7ee31c8961ac".to_string()]; 2], "a placement on the root keeps its one id");
}

/// The kinds of length the fixtures hold, each checked by the test above: if a fixture is dropped, this says which kind lost its test.
#[test]
fn the_fixtures_hold_every_kind_of_length() {
    let mut seen = std::collections::BTreeSet::new();
    for f in fixtures(".json") {
        number_paths(&serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap(), String::new(), &mut seen);
    }
    for kind in [
        // points and outlines
        "placement.outline[].x",
        "placement.footprints[].at.y",
        "routing.zones[].outline[].x",
        // widths
        "routing.tracks[].width",
        "drawings.shapes[].stroke_width",
        "schematic.lines[].width_um",
        "schematic.extras.graphics[].width_um",
        "drawings.tables[].border.stroke.width_um",
        // tracks, arcs, vias, drills
        "routing.tracks[].pts[].y",
        "routing.tracks[].arc_mid_offset.x",
        "routing.vias[].diameter",
        "routing.vias[].drill",
        "drawings.footprint_extras[].holes[].pad.drill",
        "drawings.footprint_extras[].holes[].pad.drill_slot[]",
        // sizes
        "drawings.texts[].size_um",
        "drawings.silk_texts[].size[]",
        "drawings.barcodes[].width_um",
        "drawings.board_items.text_boxes[].margin_left_um",
        "drawings.tables[].col_widths[]",
        "drawings.tables[].cells[].margins[]",
        "schematic.sheets[].size[]",
        "schematic.extras.junction_looks.*.diameter_um",
        // zones and their settings (an area, too)
        "routing.zones[].clearance",
        "routing.zones[].thermal_gap",
        "routing.zones[].hatch_gap",
        "routing.zones[].min_island_area",
        "routing.teardrop_settings.max_len_um",
        // the overlays
        "drawings.dimensions[].kind.height",
        "drawings.dimensions[].kind.leader_length",
        "drawings.dimension_settings.arrow_length",
        "drawings.footprint_edits[].pads[].offset.x",
        "drawings.footprint_edits[].reference.at.y",
        "drawings.footprint_extras[].pads[].thermal_spoke_width",
        "drawings.footprint_extras[].graphics[].shape.end.x",
        "drawings.tuning_patterns[].target_length.opt",
        "drawings.tuning_patterns[].base_line[].x",
        "drawings.board_items.via_stitches[].pitch_um",
        "drawings.board_items.via_stitches[].via.drill_um",
        "drawings.board_items.drill_maps[].size_um",
        "drawings.board_items.drill_charts[].columns[].width_um",
        "drawings.board_items.shape_styles[].endings.end.length_um",
        "drawings.board_items.images[].at.x",
        "drawings.via_tenting[].at.x",
        "drawings.aux_origin.x",
        "drawings.outline_width_um",
        "schematic.wires[].pts[].x",
        "schematic.bus_entries[].size.x",
        "schematic.field_layout.*[].dx",
        "schematic.extras.graphics[].shape.radius_um",
        "schematic.extras.graphics[].shape.pin_length_um",
        "sheet_contents.*.symbols[].at.x",
        // and what is not a length, beside them
        "drawings.board_items.images[].scale_ppm",
        "drawings.tuning_patterns[].last_tuning_length",
        "routing.zones[].priority",
        "placement.footprints[].rot",
    ] {
        assert!(seen.contains(kind), "no fixture has {kind}");
    }
}

/// What the QA files did not have: the overlays only the studio makes, written as the code writes them now and taken back to micrometres (every length
/// /1000, `_nm` keys `_um`) by the same rule the fixtures are checked with.
#[test]
fn the_studios_own_overlays_come_back_from_micrometres() {
    let v2 = json!({
        "schema": 2,
        "provenance": { "engine_version": "test", "intent_hash": "", "seed": 7 },
        "placement": { "outline": [{ "x": 0, "y": 0 }, { "x": 50_000_000, "y": 0 }, { "x": 50_000_000, "y": 40_000_000 }], "footprints": [],
            "modules": [{ "name": "power", "refs": ["U1"], "rect": [1_000_000, 2_000_000, 21_000_000, 12_000_000] }] },
        "routing": {
            "tracks": [], "vias": [], "zones": [],
            "track_width_presets": [150_000, 250_000],
            "via_presets": [{ "diameter": 600_000, "drill": 300_000 }],
        },
        "drawings": {
            "shapes": [], "texts": [],
            "page": { "paper": "User", "user_size_nm": [300_000_000, 200_000_000] },
            "drc_exclusions": [{ "check": "clearance", "items": ["a", "b"], "positions_nm": [[1_234_567, -7_654_321]], "comment": "" }],
            "board_items": {
                "points": [{ "id": "pt_1", "layer": "F.SilkS", "at": { "x": 5_000_000, "y": 6_000_000 }, "size_nm": 1_500_000 }],
                "grid_items": [{ "id": "grd_1", "at": { "x": 1_000_000, "y": 2_000_000 }, "extent": { "x": 8_000_000, "y": 9_000_000 }, "spacing": { "x": 500_000, "y": 250_000 }, "angle": 30.0, "priority": 3 }],
            },
            "tables": [{
                "id": "tbl_1", "layer": "F.SilkS", "at": { "x": 10_000_000, "y": 20_000_000 }, "angle": 90_000,
                "col_widths": [16_000_000], "row_heights": [3_000_000],
                "cells": [{ "text": "x", "margins": [1_002_000, 1_000_000, 1_000_000, 1_000_000],
                    "style": { "height_nm": 1_270_000, "width_nm": 1_270_000, "thickness_nm": 158_000, "line_spacing_permille": 1200 } }],
                "border": { "external": false, "header": false, "stroke": { "width_nm": -1 } },
                "separators": { "rows": true, "cols": true, "stroke": { "width_nm": 150_000 } },
            }],
            "tuning_patterns": [{
                "mode": "single", "layer": "F.Cu", "origin": { "x": 0, "y": 0 }, "end": { "x": 9_000_000, "y": 0 },
                "min_amplitude": 200_000, "max_amplitude": 1_000_000, "spacing": 600_000,
                "target_length": { "min": 49_900_000, "opt": 50_000_000, "max": 50_100_000 },
                "is_time_domain": true,
                "target_delay": { "min": 119_900_000, "opt": 120_000_000, "max": 120_100_000 },
                "target_skew_delay": { "min": -2_000_000, "opt": 0, "max": 2_000_000 },
                "last_tuning_length": 49_999_999, "last_tuning_delay": 300_050_000,
            }],
            "rules": {
                "constraints": {
                    "min_clearance_nm": 200_000, "min_connection_nm": 0, "min_track_width_nm": 150_000, "min_annular_width_nm": 100_000,
                    "min_via_diameter_nm": 450_000, "min_through_hole_nm": 300_000, "min_microvia_diameter_nm": 200_000, "min_microvia_drill_nm": 100_000,
                    "min_hole_to_hole_nm": 250_000, "min_hole_clearance_nm": 250_000, "min_copper_edge_clearance_nm": 500_000, "min_silk_clearance_nm": -10_000,
                    "min_groove_width_nm": 0, "min_silk_text_height_nm": 800_000, "min_silk_text_thickness_nm": 80_000, "max_error_nm": 5_000,
                    "min_resolved_spokes": 2, "use_height_for_length_calcs": true, "zones_allow_external_fillets": false,
                },
                "zone_hatch_offsets": { "F.Cu": { "x_nm": 1_500_000, "y_nm": -2_000_000 } },
            },
        },
        "schematic": {
            "symbols": [], "wires": [], "labels": [], "texts": [], "power_symbols": [], "no_connects": [],
            "erc_exclusions": [{ "check": "pin_not_connected", "location": "", "comment": "", "marker": { "error_type": "pin_not_connected", "at_nm": [12_345_600, 7_000_100], "items": [], "sheet": [], "main_sheet": [], "aux_sheet": [] } }],
            "extras": { "page": { "paper": "User", "user_size_nm": [431_800_000, 279_400_000] }, "label_looks": { "lbl_1": { "size_nm": 1_651_000 } } },
        },
    });
    let mut design: Design = serde_json::from_value(v2.clone()).expect("the version 2 overlays read");
    // the one default here that micrometres cannot hold (KiCad's 1.27 mm x sin 27.5 degrees, 0.586421 mm)
    design.drawings.as_mut().unwrap().dimension_settings.extension_height = Nm::new(586_000);
    let written = serde_json::to_value(&design).unwrap();
    // back to micrometres, by the rule `expected` multiplies by
    fn to_um(v: &Value, path: &[String]) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.iter()
                    .map(|(k, x)| {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        (k.strip_suffix("_nm").filter(|_| !NOT_LENGTH_CONTAINERS.contains(&k.as_str())).map(|b| format!("{b}_um")).unwrap_or_else(|| k.clone()), to_um(x, &p))
                    })
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(|x| to_um(x, path)).collect()),
            Value::Number(n) if n.is_i64() || n.is_u64() => {
                let leaf = path.last().map(String::as_str).unwrap_or("");
                let i = n.as_i64().unwrap();
                if leaf == "schema" {
                    json!(1)
                } else if path.iter().any(|k| NOT_LENGTH_CONTAINERS.contains(&k.as_str())) || NOT_LENGTHS.contains(&leaf) || (leaf == "width_nm" && i < 0) {
                    v.clone()
                } else if AREAS.contains(&leaf) {
                    json!(i / 1_000_000)
                } else {
                    assert_eq!(i % 1000, 0, "{path:?}: a test length that micrometres can hold");
                    json!(i / 1000)
                }
            }
            _ => v.clone(),
        }
    }
    let v1 = to_um(&written, &[]);
    assert!(v1.to_string().contains("\"size_um\"") && v1.to_string().contains("\"at_nm\""), "the old keys, and the one that was nanometres already");
    let migrated: Design = serde_json::from_value(v1.clone()).expect("the version 1 overlays read");
    let read = serde_json::to_value(&migrated).unwrap();
    assert!(read == written, "every overlay comes back to the nanometre: {:?}", first_difference(&read, &written, ""));
    let want = expected(&v1, &[]);
    assert!(want == written, "and is what the fixtures' rule says: {:?}", first_difference(&want, &written, ""));
    // spot checks of what is not a length
    let pattern = &migrated.drawings.as_ref().unwrap().tuning_patterns[0];
    assert_eq!((pattern.target_length.opt, pattern.target_delay.opt, pattern.last_tuning_length), (50_000_000, 120_000_000, Nm::new(49_999_999)));
    let table = &migrated.drawings.as_ref().unwrap().tables[0];
    assert_eq!((table.border.stroke.width_nm, table.cells[0].style.line_spacing_permille, table.angle), (Nm::new(-1), 1200, 90_000));
}

#[test]
fn a_design_read_twice_is_not_scaled_twice() {
    let text = std::fs::read_to_string(&fixtures(".json")[0]).unwrap();
    let once: Design = serde_json::from_str(&text).unwrap();
    let written = serde_json::to_string(&once).unwrap();
    assert!(written.starts_with("{\"schema\":2,"), "the version is written first: {}", &written[..40]);
    let twice: Design = serde_json::from_str(&written).unwrap();
    assert_eq!(serde_json::to_value(&twice).unwrap(), serde_json::to_value(&once).unwrap());
    // and the canonical bytes (what is hashed, and what `eda import-kicad` writes) say 2 too
    assert!(String::from_utf8(once.canonical_bytes().unwrap()).unwrap().starts_with("{\"schema\":2,"));
}

#[test]
fn a_design_without_a_version_is_a_micrometre_one() {
    let d: Design = serde_json::from_str(r#"{"provenance":{"engine_version":"x","intent_hash":"","seed":0},"placement":{"outline":[{"x":1,"y":-2}],"footprints":[]}}"#).unwrap();
    assert_eq!(d.placement.unwrap().outline[0], eda_model::ir::Point { x: Nm::new(1000), y: Nm::new(-2000) });
    assert_eq!(d.schema, SCHEMA_VERSION);
}

#[test]
fn a_new_design_is_read_as_it_is() {
    let d: Design = serde_json::from_str(r#"{"schema":2,"provenance":{"engine_version":"x","intent_hash":"","seed":0},"placement":{"outline":[{"x":1,"y":-2}],"footprints":[]}}"#).unwrap();
    assert_eq!(d.placement.unwrap().outline[0], eda_model::ir::Point { x: Nm::new(1), y: Nm::new(-2) });
    // a version 2 document whose version is not its first key (one built through a `serde_json::Value`, whose keys are sorted): what comes before it
    // is held until the version is known
    let late: Design = serde_json::from_str(r#"{"placement":{"outline":[{"x":1,"y":-2}],"footprints":[]},"provenance":{"engine_version":"x","intent_hash":"","seed":0},"schema":2}"#).unwrap();
    assert_eq!(late.placement.unwrap().outline[0], eda_model::ir::Point { x: Nm::new(1), y: Nm::new(-2) });
    let late: Design = serde_json::from_str(r#"{"placement":{"outline":[{"x":1,"y":-2}],"footprints":[]},"provenance":{"engine_version":"x","intent_hash":"","seed":0},"schema":1}"#).unwrap();
    assert_eq!(late.placement.as_ref().unwrap().outline[0], eda_model::ir::Point { x: Nm::new(1000), y: Nm::new(-2000) });
    let round: Design = serde_json::from_value(serde_json::to_value(&late).unwrap()).unwrap();
    assert_eq!(round.placement.unwrap().outline[0], eda_model::ir::Point { x: Nm::new(1000), y: Nm::new(-2000) }, "through a sorted `Value` and back, not scaled again");
}

#[test]
fn an_intent_without_a_version_is_micrometres_every_time_it_is_read() {
    let model: ConstraintModel = serde_yaml::from_str(include_str!("../../../examples/mcu_board_30plus.yaml")).unwrap();
    assert_eq!((model.board.grid, model.board.track_width, model.board.clearance), (Nm::new(254_000), Nm::new(200_000), Nm::new(200_000)), "the defaults, already nanometres");
    let model: ConstraintModel = serde_yaml::from_str(include_str!("../../../examples/dense_small_outline.yaml")).unwrap();
    assert_eq!((model.board.track_width, model.board.clearance), (Nm::new(200_000), Nm::new(200_000)), "written 200 (um) in the file");
    assert!(model.board.outline.as_ref().is_some_and(|o| o.iter().all(|p| p.x % Nm::new(1000) == Nm::ZERO && p.y % Nm::new(1000) == Nm::ZERO) && o.iter().any(|p| p.x >= Nm::new(1_000_000))));
    // an intent is not buffered (a YAML number has to stay text for a field that is text): its version has to come first, as the code writes it
    let late = serde_yaml::from_str::<ConstraintModel>("board:\n  track_width: 200000\nschema: 2\n").unwrap_err().to_string();
    assert!(late.contains("first key"), "{late}");
    // what the code writes says it is nanometres, and reads back as it is
    let written = serde_yaml::to_string(&model).unwrap();
    assert!(written.starts_with("schema: 2\n"), "{}", &written[..40]);
    let back: ConstraintModel = serde_yaml::from_str(&written).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&model).unwrap());
}

#[test]
fn an_imported_intent_written_in_micrometres_is_read_in_nanometres() {
    for f in fixtures(".intent.yaml") {
        let text = std::fs::read_to_string(&f).unwrap();
        let old: Value = serde_yaml::from_str(&text).unwrap();
        assert!(old.get("schema").is_none(), "{}: a version 1 intent", f.display());
        let model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let mut want = expected(&old, &[]);
        want.as_object_mut().unwrap().insert("schema".into(), json!(2));
        // the stackup's thicknesses and the placement rules' distances were millimetres and stay so (floats, untouched by `expected`)
        assert_eq!(serde_json::to_value(&model).unwrap(), want, "{}", f.display());
    }
}

/// Every length field of a type the design file or the intent holds reads through `crate::units::de` (or `de_width`, `de_area`): a source check of
/// `crates/model/src`, so a length added later without it is caught here rather than by an old board coming back 1000x too small.
#[test]
fn every_length_field_of_a_serialized_type_reads_through_the_unit_scale() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (mut missing, mut checked) = (Vec::new(), 0);
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        // `units.rs` itself only has its own tests' probe
        if path.extension().is_none_or(|e| e != "rs") || path.ends_with("units.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let (mut derive_de, mut depth, mut item_depth): (bool, i32, Option<i32>) = (false, 0, None);
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with("#[derive(") {
                derive_de = t.contains("Deserialize");
            }
            let opens = (t.starts_with("pub struct ") || t.starts_with("struct ") || t.starts_with("pub enum ") || t.starts_with("enum ")) && t.ends_with('{');
            if opens && derive_de && item_depth.is_none() {
                item_depth = Some(depth);
            }
            if opens {
                derive_de = false;
            }
            if let Some(_) = item_depth {
                let is_field = !t.starts_with("//") && !t.starts_with('#') && t.contains(": ") && t.ends_with(',') && !t.contains(" fn ") && !t.contains("=>");
                let ty = t.split_once(": ").map(|(_, ty)| ty.trim_end_matches(',')).unwrap_or("");
                let is_length = ty.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "Nm");
                if is_field && is_length && !t.contains('{') {
                    let attrs: String = lines[..i].iter().rev().take_while(|l| l.trim().starts_with('#') || l.trim().starts_with("//")).map(|l| l.trim()).collect();
                    checked += 1;
                    // a field that held nanometres even in the micrometre IR says so, and is not scaled
                    if !attrs.contains("crate::units::de") && !attrs.contains("nanometres since schema 1") {
                        missing.push(format!("{}:{}: {t}", path.file_name().unwrap().to_string_lossy(), i + 1));
                    }
                } else if is_length && t.contains('{') && t.contains('}') {
                    missing.push(format!("{}:{}: a one-line variant with a length (give each field its own line and attribute): {t}", path.file_name().unwrap().to_string_lossy(), i + 1));
                }
            }
            depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if item_depth.is_some_and(|d| depth <= d) {
                item_depth = None;
            }
        }
    }
    assert!(missing.is_empty(), "length fields that an old document would not scale:\n{}", missing.join("\n"));
    assert!(checked > 200, "the check found only {checked} length fields: it no longer reads the source right");
}
