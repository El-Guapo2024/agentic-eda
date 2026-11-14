//! `/api/sim/model/*`: what the studio's Simulation Model dialog (`web/studio/src/components/simulator/SimModelDialog.tsx`, the library part of
//! `DIALOG_SIM_MODEL`) reads from the server:
//! - `POST /api/sim/model/symbol` `{ref}`: the symbol's simulation fields as the dialog starts from them (`m_fields`: the instance's fields, over
//!   the library symbol's on a sheet the studio drew, which writes those beside the instance's) and its pins sorted by number
//!   (`m_sortedPartPins`: every unit's, a De Morgan alternate's pins left out, `StrNumCmp` order), and which of the fields are the instance's own;
//! - `POST /api/sim/model/library` `{path}`: the models of a SPICE library (`loadLibrary`, `eda_sim::library`), each with its pins and the
//!   `Sim.Device` / `Sim.Type` its model type writes, and the path as `Sim.Library` stores it (`TransferDataFromWindow`: made relative to the project
//!   when it is inside it); or what the info bar says when the library cannot be read;
//! - `GET /api/sim/model/files`: the files under the board folder a library can be, for the dialog's Browse (`wxFileDialog` "Browse Models");
//! - `GET /api/sim/model/inductors`: the circuit's inductors, the K device's l1 / l2 choices, read from the design (UI-LEFT-3 M3-1).
//!
//! The project folder a relative library path is read from is the board folder (the simulator's netlist step reads the libraries from there,
//! `crate::sim_api`'s resolver).

use crate::board;
use eda_sim::library::{read_library, ModelKind};
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// `StrNumCmp( a, b, true )`: numbers in the strings compared by value, the rest without case.
pub(crate) fn str_num_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n: u128 = 0;
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n = n.saturating_mul(10).saturating_add(c as u128 - '0' as u128);
                        it.next();
                    }
                    n
                };
                let (m, n) = (take(&mut x), take(&mut y));
                if m != n {
                    return m.cmp(&n);
                }
            }
            (Some(c), Some(d)) => {
                let (c, d) = (c.to_ascii_uppercase(), d.to_ascii_uppercase());
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// `POST /api/sim/model/symbol` `{ref}`.
pub fn symbol_route(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let reference = req.get("ref").and_then(Value::as_str).unwrap_or("");
    let (meta, mut design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    // a board whose schematic was never edited keeps none: the derived one is what the studio shows (`sch_api::loaded`)
    if design.schematic.is_none() {
        match board::derived_schematic(&design, &model) {
            Ok(d) => {
                design.schematic = d.schematic;
                design.sheet_contents = d.sheet_contents;
            }
            Err(e) => return err(board::reasons(&e)),
        }
    }
    let sheets = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    let Some((sch, sym)) = sheets.flat_map(|s| s.symbols.iter().map(move |x| (s, x))).find(|(_, x)| x.id == reference) else {
        return err(format!("no symbol {reference} on the schematic"));
    };
    let mut fields: serde_json::Map<String, Value> = serde_json::Map::new();
    let part = model.part(&sym.id);
    // a sheet the studio drew writes the library symbol's simulation fields beside the instance's (`eda_kicad`'s `sim_fields_written`)
    if !sch.imported_from_kicad {
        let lib_id = part.map(eda_model::symbol::resolve_lib_id).unwrap_or_else(|| sym.lib_id.clone());
        for (n, t) in model.symbol_sim_fields.get(&lib_id).into_iter().flatten() {
            fields.insert(n.clone(), json!(t));
        }
    }
    let mut own = Vec::new();
    for (n, t) in sch.user_fields.get(&sym.id).into_iter().flatten() {
        fields.insert(n.clone(), json!(t));
        own.push(n.clone());
    }
    let value = if !sym.value.is_empty() { sym.value.clone() } else { part.and_then(|p| p.value.clone()).unwrap_or_default() };
    let mut pins: Vec<(String, String)> = part.map(|p| p.pins.iter().map(|pin| (pin.number.clone(), pin.name.clone().unwrap_or_default())).collect()).unwrap_or_default();
    pins.sort_by(|a, b| str_num_cmp(&a.0, &b.0));
    pins.dedup_by(|a, b| a.0 == b.0);
    // `GetFieldValue( &m_fields, .., aResolve = true, .. )`: a text of the dialog's fields as `SCH_FIELD::GetShownText` shows it, the field's
    // parent being the symbol (`SCH_SYMBOL::ResolveTextVar`, then its sheet's): the texts the request names, the symbol's own, and
    // "${SIM.PARAMS}" (the inferred Value)
    let mut texts: Vec<String> = req.get("texts").and_then(Value::as_array).into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect();
    texts.extend(fields.values().filter_map(|v| v.as_str().map(str::to_string)));
    texts.push(value.clone());
    texts.push("${SIM.PARAMS}".into());
    let mut shown = serde_json::Map::new();
    if texts.iter().any(|t| eda_model::text_vars::has_text_vars(t) || t.contains('{')) {
        let ids = sheet_ids_of(&design, &sym.id);
        let entries = eda_model::sheet_list::walk(&design);
        let breadcrumb: Vec<(String, String)> =
            (1..=ids.len()).map(|n| (ids[n - 1].clone(), entries.iter().find(|e| e.path[..] == ids[..n]).map(|e| e.name.clone()).unwrap_or_default())).collect();
        let project = crate::text_vars_json::project(dir, &meta.intent, eda_model::text_vars::Clock::utc_now());
        let resolver = crate::text_vars_json::SchShown::new(dir, &design, &model, &breadcrumb, project);
        let key = eda_model::ir::field_key(&sym.id, sym.unit);
        for t in &texts {
            if eda_model::text_vars::has_text_vars(t) || t.contains('{') {
                shown.insert(t.clone(), json!(resolver.symbol_text_shown(&key, t)));
            }
        }
    }
    json!({
        "ok": true,
        "ref": sym.id,
        "value": value,
        "shown": shown,
        "imported": sch.imported_from_kicad,
        "fields": fields,
        // the fields that are the instance's own (`SchematicSection::user_fields`), which a verb can remove; the others are the library symbol's
        "own": own,
        "pins": pins.iter().map(|(n, name)| json!({ "number": n, "name": name })).collect::<Vec<_>>(),
    })
}

/// `SCH_SYMBOL::UpdatePrefix` of a reference: trailing digits, "?" and "*" off, "~" a space, trimmed.
fn reference_prefix(reference: &str) -> String {
    reference.replace('~', " ").trim_end_matches(|c: char| c.is_ascii_digit() || c == '?' || c == '*').trim().to_string()
}

/// `convertSeparators` (sim_model.cpp, `InferSimModel`'s): a "," or "." is a decimal point or a thousands separator by where it is; `None` when it can be
/// neither.
fn convert_separators(mantissa: &str) -> Option<String> {
    let m: Vec<char> = mantissa.chars().filter(|c| *c != ' ').collect();
    let (mut ambiguous, mut thousands, mut decimal) = ('?', '?', '?');
    let (mut thousands_found, mut decimal_found, mut digits) = (false, false, 0);
    for ii in (0..m.len()).rev() {
        let c = m[ii];
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == '.' || c == ',' {
            if decimal != '?' || thousands != '?' {
                if c == decimal {
                    if thousands_found || decimal_found {
                        return None;
                    }
                    decimal_found = true;
                } else if c == thousands {
                    if digits != 3 {
                        return None;
                    }
                    thousands_found = true;
                }
            } else if ambiguous != '?' {
                if c == ambiguous {
                    thousands = ambiguous;
                    thousands_found = true;
                    decimal = if c == '.' { ',' } else { '.' };
                } else {
                    decimal = ambiguous;
                    decimal_found = true;
                    thousands = c;
                    thousands_found = true;
                }
            } else if (ii == 1 && m[0] == '0') || digits != 3 {
                decimal = c;
                decimal_found = true;
                thousands = if c == '.' { ',' } else { '.' };
            } else {
                ambiguous = c;
            }
            digits = 0;
        } else {
            digits = 0;
        }
    }
    if decimal == '?' && thousands == '?' {
        decimal = '.';
        thousands = ',';
    }
    Some(m.iter().filter(|c| **c != thousands).map(|c| if *c == decimal { '.' } else { *c }).collect())
}

/// `SIM_MODEL::InferSimModel`'s value pattern for a passive (`idealVal`, sim_model.cpp; the micro and ohm signs as the source has them).
fn ideal_value_regex() -> &'static eda_model::wx_regex::Regex {
    static RE: std::sync::OnceLock<eda_model::wx_regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        let pattern = "^([0-9,. ]+)([fFpPnNuUmMkKgGtT\u{3bc}\u{b5}\u{1d6cd}\u{1d707}\u{1d741} ]|M(e|E)(g|G))?([fFhH\u{3a9}\u{2126}\u{1d6c0}\u{1d6fa}\u{1d76e}rR]|ohm)?([-1-9 ]*)([fFhH\u{3a9}\u{2126}\u{1d6c0}\u{1d6fa}\u{1d76e}rR]|ohm)?$";
        eda_model::wx_regex::Regex::compile(pattern, false, eda_model::wx_regex::Flavor::Find).expect("the pattern compiles")
    })
}

/// Whether the model `SIM_LIB_MGR::CreateModel` makes of a symbol's fields is of device L (`SIM_MODEL::GetDeviceType() == DEVICE_T::L`): `Sim.Device` "L"
/// (`ReadTypeFromFields`), a library model of an inductor's type (`device`: the library model's, read by the caller), or, with no model fields at all, a
/// two-pin symbol whose reference starts with "L" and has a Value (`InferSimModel`; not when its number's separators cannot be read).
fn is_inductor(reference: &str, field: &dyn Fn(&str) -> String, value: &str, pins: usize, library_device: &mut dyn FnMut(&str, &str) -> Option<String>) -> bool {
    let (device, kind, library, name) = (field("Sim.Device"), field("Sim.Type"), field("Sim.Library"), field("Sim.Name"));
    if !device.is_empty() {
        return device == "L";
    }
    if !library.is_empty() {
        return library_device(&library, &name).as_deref() == Some("L");
    }
    if pins != 2 || !kind.is_empty() || !name.is_empty() || value.is_empty() || !reference_prefix(reference).starts_with('L') {
        return false;
    }
    let chars: Vec<char> = value.chars().collect();
    match ideal_value_regex().find_at(&chars, 0).and_then(|c| c.group(1)) {
        Some((a, b)) => convert_separators(&chars[a..b].iter().collect::<String>()).is_some(),
        None => true,
    }
}

/// The circuit's inductors the K device's "l1" and "l2" choose from (UI-LEFT-3 M3-1; `DIALOG_SIM_MODEL::newParamProperty`: `SPICE_CIRCUIT_MODEL circuit(
/// &schEditFrame->Schematic() ); circuit.ReadSchematicAndLibraries( OPTION_DEFAULT_FLAGS, devNul )`, the items whose model is of device L by `refName`,
/// `StrNumCmp` order). Read from the design -- the schematic in memory, as KiCad's dialog reads it -- not from a netlist of the saved files:
/// `NETLIST_EXPORTER_SPICE::ReadSchematicAndLibraries` visits every sheet path's symbols, once per reference (`findNextSymbol`), not a power or other
/// `#` symbol, not one excluded from the simulation on itself or a sheet above it (`ResolveExcludedFromSim`); its fields are the instance's over the library
/// symbol's simulation fields (which a sheet the studio drew writes beside them).
pub(crate) fn inductors(dir: &Path, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Vec<String> {
    use eda_model::sheet_list;
    let spice_lib_dir = std::env::var_os("SPICE_LIB_DIR").map(std::path::PathBuf::from);
    let vars = crate::library_tables::Env::discover().vars(dir, &crate::text_variables::project_variables(dir));
    let mut libraries: std::collections::HashMap<String, eda_sim::library::Library> = std::collections::HashMap::new();
    let mut library_device = |path: &str, name: &str| -> Option<String> {
        let lib = libraries.entry(path.to_string()).or_insert_with(|| read_library(&vars.expand(path), dir, spice_lib_dir.as_deref()));
        lib.models.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| match &m.kind {
            ModelKind::Model { model_type: Some(t), .. } => Some(t.device.to_string()),
            _ => None,
        })
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for e in sheet_list::walk(design) {
        let Some(screen) = sheet_list::screen_of(design, &e.path) else { continue };
        let sheet_excluded = eda_model::variants::sheet_path_flags(design, &e.path, "").exclude_from_sim;
        for sym in &screen.symbols {
            let reference = sheet_list::reference_under(screen, sym, &e.path);
            if reference.starts_with('#') || !seen.insert(reference.clone()) || sym.exclude_from_sim || sheet_excluded {
                continue;
            }
            let part = model.part(&sym.id);
            let lib_id = part.map(eda_model::symbol::resolve_lib_id).unwrap_or_else(|| sym.lib_id.clone());
            let library_fields = (!screen.imported_from_kicad).then(|| model.symbol_sim_fields.get(&lib_id)).flatten();
            let own = screen.user_fields.get(&sym.id);
            let field = |n: &str| -> String {
                own.and_then(|f| f.get(n)).cloned().or_else(|| library_fields.and_then(|f| f.iter().find(|(k, _)| k == n).map(|(_, t)| t.clone()))).unwrap_or_default()
            };
            let value = if !sym.value.is_empty() { sym.value.clone() } else { part.and_then(|p| p.value.clone()).unwrap_or_default() };
            let pins: std::collections::BTreeSet<&str> = part.map(|p| p.pins.iter().map(|pin| pin.number.as_str()).collect()).unwrap_or_default();
            if is_inductor(&reference, &field, &value, pins.len(), &mut library_device) {
                out.push(reference);
            }
        }
    }
    out.sort_by(|a, b| str_num_cmp(a, b));
    out
}

/// `GET /api/sim/model/inductors`: `{ok, inductors}` ([`inductors`]).
pub fn inductors_route(dir: &Path) -> Value {
    let (_, mut design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    if design.schematic.is_none() {
        match board::derived_schematic(&design, &model) {
            Ok(d) => {
                design.schematic = d.schematic;
                design.sheet_contents = d.sheet_contents;
            }
            Err(e) => return err(board::reasons(&e)),
        }
    }
    json!({ "ok": true, "inductors": inductors(dir, &design, &model) })
}

/// The placement ids from the root down to the first sheet whose screen holds the symbol `reference` (`[]` for the root).
fn sheet_ids_of(design: &eda_model::ir::Design, reference: &str) -> Vec<String> {
    for e in eda_model::sheet_list::walk(design) {
        if eda_model::sheet_list::screen_of(design, &e.path).is_some_and(|s| s.symbols.iter().any(|x| x.id == reference)) {
            return e.path;
        }
    }
    Vec::new()
}

/// The `Sim.Library` text for a path: relative to the project when it is inside it (`fn.MakeRelativeTo( Prj().GetProjectPath() )` and not
/// starting with ".."), else as given.
fn stored_path(path: &str, dir: &Path) -> String {
    let p = Path::new(path);
    if !p.is_absolute() || path.contains('$') {
        return path.to_string();
    }
    let (Ok(abs), Ok(project)) = (std::fs::canonicalize(p), std::fs::canonicalize(dir)) else { return path.to_string() };
    abs.strip_prefix(&project).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.to_string())
}

/// `POST /api/sim/model/library` `{path}`.
pub fn library_route(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let path = req.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if path.is_empty() {
        return json!({ "ok": true, "path": "", "models": [], "messages": [] });
    }
    let spice_lib_dir = std::env::var_os("SPICE_LIB_DIR").map(std::path::PathBuf::from);
    // `ExpandEnvVarSubstitutions( aLibraryPath, m_project )`: `${KIPRJMOD}`, KiCad's own (`${KICAD9_SYMBOL_DIR}` read as this KiCad's), the project's
    // and the environment's variables
    let vars = crate::library_tables::Env::discover().vars(dir, &crate::text_variables::project_variables(dir));
    if path.to_lowercase().ends_with(".ibs") {
        return ibis_library(&path, &vars.expand(&path), dir, spice_lib_dir.as_deref());
    }
    let lib = read_library(&vars.expand(&path), dir, spice_lib_dir.as_deref());
    let models: Vec<Value> = lib
        .models
        .iter()
        .map(|m| match &m.kind {
            ModelKind::Subckt { ports } => json!({ "name": m.name, "kind": "subckt", "device": "SUBCKT", "type": "", "pins": ports, "code": m.code }),
            ModelKind::Model { spice_type, model_type } => match model_type {
                Some(t) => json!({ "name": m.name, "kind": "model", "spice_type": spice_type, "modelType": t.name, "device": t.device, "type": t.type_field, "pins": t.pins, "code": m.code }),
                // `TYPE::RAWSPICE`: KiCad's raw SPICE element, whose pins are not known here
                None => json!({ "name": m.name, "kind": "model", "spice_type": spice_type, "device": "SPICE", "type": "", "pins": [], "code": m.code }),
            },
        })
        .collect();
    json!({ "ok": lib.messages.is_empty(), "path": stored_path(&path, dir), "models": models, "messages": lib.messages })
}

/// An IBIS library (`SIM_LIBRARY_IBIS::ReadFile`): `KIBIS` of the file (`eda_sim::ibis`, KiCad's `IbisParser` and its checks), whose messages
/// go to the dialog's reporter in order, then "Invalid IBIS file '%s'" (an error, with the resolved path, `aFilePath`) when `!m_kibis.m_valid`;
/// the dialog's `loadLibrary` refuses the library when the reporter has an error (`HasMessageOfSeverity( RPT_SEVERITY_UNDEFINED |
/// RPT_SEVERITY_ERROR )`): `ok: false`, no model, and `messages` the reported lines (`WX_STRING_REPORTER::GetMessages`, one a line). Else each
/// `[Component]` is a model of the KIBIS device type, with its pins (`m_ibisPins`), each pin's models (`ChangePin`) and the differential pins
/// (`isPinDiff`); `messages` then holds what was reported all the same (warnings, the checks' "Checking ..." lines), which KiCad's dialog does
/// not show. Where KiCad's parser throws (`std::out_of_range`, which ends KiCad), the last line says so and the library is refused, without
/// "Invalid IBIS file" (`ReadFile` does not get there).
fn ibis_library(path: &str, expanded: &str, dir: &Path, spice_lib_dir: Option<&Path>) -> Value {
    let file = match eda_sim::library::resolve_library_path(expanded, dir, spice_lib_dir) {
        Ok(f) => f,
        Err(e) => return json!({ "ok": false, "path": path, "models": [], "messages": [e] }),
    };
    let text = match std::fs::read(&file) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(_) => return json!({ "ok": false, "path": path, "models": [], "messages": [format!("File '{}' does not exist.", file.display())] }),
    };
    let ibis = eda_sim::ibis::read_ibis(&text);
    let mut messages: Vec<String> = ibis.messages.iter().map(|(_, m)| m.clone()).collect();
    let mut refused = ibis.messages.iter().any(|(s, _)| *s == eda_sim::ibis::Severity::Error);
    if !ibis.valid && ibis.thrown.is_none() {
        messages.push(format!("Invalid IBIS file '{}'", file.display()));
        refused = true;
    }
    if refused || ibis.thrown.is_some() {
        return json!({ "ok": false, "path": path, "models": [], "messages": messages });
    }
    let models: Vec<Value> = ibis
        .components
        .iter()
        .map(|c| {
            json!({
                "name": c.name, "kind": "ibis", "device": "IBIS", "type": "DEVICE", "pins": [], "code": "",
                "ibisPins": c.pins.iter().map(|(n, s)| json!([n, s])).collect::<Vec<_>>(),
                "pinModels": c.pin_models,
                "diffPins": c.diff_pins,
            })
        })
        .collect();
    json!({ "ok": true, "ibis": true, "path": stored_path(path, dir), "models": models, "messages": messages })
}

/// `GET /api/sim/model/files`: the board folder's files with a SPICE library's usual extensions, relative, sorted; the studio's own folders
/// (`.kicad`, `.history`, `export`) left out.
pub fn files_route(dir: &Path) -> Value {
    const EXT: [&str; 11] = ["lib", "sub", "mod", "cir", "inc", "sp", "spi", "spice", "ckt", "txt", "ibs"];
    let mut out = Vec::new();
    fn walk(base: &Path, at: &Path, depth: usize, out: &mut Vec<String>) {
        if depth > 5 || out.len() > 2000 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(at) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !name.starts_with('.') && name != "export" && name != "node_modules" && name != "target" {
                    walk(base, &p, depth + 1, out);
                }
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| EXT.contains(&x.to_lowercase().as_str())) {
                if let Ok(r) = p.strip_prefix(base) {
                    out.push(r.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    walk(dir, dir, 0, &mut out);
    out.sort();
    json!({ "ok": true, "files": out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_num_cmp_orders_numbers_by_value() {
        let mut v = vec!["10", "2", "1", "A1", "a2", "B"];
        v.sort_by(|a, b| str_num_cmp(a, b));
        assert_eq!(v, ["1", "2", "10", "A1", "a2", "B"]);
    }

    #[test]
    fn a_library_route_lists_models_with_pins_and_a_project_relative_path() {
        let dir = std::env::temp_dir().join(format!("eda-sim-model-api-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("spice")).unwrap();
        std::fs::write(dir.join("spice/amp.lib"), ".subckt AMP inp inn vcc vee out\n.ends\n.model DX D\n.model QX NPN\n").unwrap();
        let abs = dir.join("spice/amp.lib").to_string_lossy().into_owned();
        let r = library_route(&dir, json!({ "path": abs }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["path"], "spice/amp.lib", "inside the project: stored relative");
        assert_eq!(r["models"][0]["pins"], json!(["inp", "inn", "vcc", "vee", "out"]));
        assert_eq!(r["models"][0]["device"], "SUBCKT");
        assert_eq!((r["models"][1]["device"].as_str(), r["models"][1]["pins"].clone()), (Some("D"), json!(["A", "K"])));
        assert_eq!((r["models"][2]["device"].as_str(), r["models"][2]["type"].as_str()), (Some("NPN"), Some("GUMMELPOON")));
        let r = library_route(&dir, json!({ "path": "spice/none.lib" }).to_string().as_bytes());
        assert_eq!(r["ok"], false);
        assert!(r["messages"][0].as_str().unwrap().starts_with("Simulation model library not found at '"));
        let r = library_route(&dir, json!({ "path": "${KIPRJMOD}/spice/amp.lib" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["path"].as_str()), (Some(true), Some("${KIPRJMOD}/spice/amp.lib")), "a path with a variable is stored as written");
        let f = files_route(&dir);
        assert_eq!(f["files"], json!(["spice/amp.lib"]));
        // an IBIS file: its components with their pins, each pin's models and the differential pins
        // (a file KiCad's IbisParser takes: a [File Name] and [File Rev], the component's [Manufacturer] and [Package], the model's type,
        // C_comp and voltage range)
        std::fs::write(
            dir.join("spice/io.ibs"),
            "[IBIS Ver] 5.1\n[File Name] io.ibs\n[File Rev] 1.0\n[Component] CHIP\n[Manufacturer] ACME\n[Package]\nR_pkg 0.1 NA NA\nL_pkg 1n NA NA\nC_pkg 1p NA NA\n[Pin] signal_name model_name R_pin L_pin C_pin\n1 OUT drv NA NA NA\n2 GND GND NA NA NA\n[Model] drv\nModel_type Output\nC_comp 1p NA NA\n[Voltage Range] 3.3 3.0 3.6\n[End]\n",
        )
        .unwrap();
        let r = library_route(&dir, json!({ "path": "spice/io.ibs" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["ibis"].as_bool(), r["models"][0]["name"].as_str()), (Some(true), Some(true), Some("CHIP")), "{r}");
        assert_eq!(r["models"][0]["ibisPins"], json!([["1", "OUT"], ["2", "GND"]]));
        assert_eq!(r["models"][0]["pinModels"], json!([["drv"], []]));
        assert_eq!(r["messages"], json!([]));
        // a header alone: a valid file with no component (KIBIS is valid), so a library with no model
        std::fs::write(dir.join("spice/empty.ibs"), "[IBIS Ver] 5.1\n").unwrap();
        let r = library_route(&dir, json!({ "path": "spice/empty.ibs" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["models"].clone(), r["messages"].clone()), (Some(true), json!([]), json!([])), "{r}");
        // a file KIBIS refuses: what it reported, then "Invalid IBIS file" with the resolved path
        std::fs::write(dir.join("spice/bad.ibs"), "[IBIS Ver] 5.1\n[File Name] bad.ibs\n[Component] X\n").unwrap();
        let r = library_route(&dir, json!({ "path": "spice/bad.ibs" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["models"].clone()), (Some(false), json!([])), "{r}");
        let invalid = format!("Invalid IBIS file '{}'", dir.join("spice/bad.ibs").display());
        assert_eq!(r["messages"], json!(["Checking Header...", "Missing [File Rev]", "Error on line 3", invalid]));
        // an error that does not invalidate the file still refuses the library (`loadLibrary`), and a warning does not
        let io = std::fs::read_to_string(dir.join("spice/io.ibs")).unwrap();
        std::fs::write(dir.join("spice/ramp.ibs"), io.replace("[End]\n", "[Ramp]\ndV/dt_r NA NA NA\n[End]\n")).unwrap();
        let r = library_route(&dir, json!({ "path": "spice/ramp.ibs" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["messages"].clone()), (Some(false), json!(["Invalid rising dV/dt.", "Invalid Ramp specification."])), "{r}");
        std::fs::write(dir.join("spice/warn.ibs"), io.replace("[Voltage Range] 3.3 3.0 3.6", "[Voltage Range] 3.3")).unwrap();
        let r = library_route(&dir, json!({ "path": "spice/warn.ibs" }).to_string().as_bytes());
        assert_eq!((r["ok"].as_bool(), r["messages"].clone()), (Some(true), json!(["Failed to read a word.", "Failed to read a word."])), "{r}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A board made from `yaml` the way `eda board new` makes one.
    fn board(name: &str, yaml: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-sim-model-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let intent = dir.join(format!("{name}.yaml"));
        std::fs::write(&intent, yaml).unwrap();
        let out = dir.join("board");
        let args: Vec<String> = ["new", intent.to_str().unwrap(), "-o", out.to_str().unwrap()].iter().map(|s| s.to_string()).collect();
        crate::board::run(&args, |model| {
            let empty = eda_model::ir::Design {
                schema: eda_model::ir::SCHEMA_VERSION,
                provenance: eda_model::ir::Provenance { engine_version: "t".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
                schematic: None,
                nets: None,
                placement: None,
                routing: None,
                drawings: None,
                footprint_library: None,
                sheet_contents: None,
                bus_aliases: vec![],
                symbol_library: None,
            };
            crate::seed_outline(&empty, model)
        })
        .unwrap();
        out
    }

    /// An RC with a diode across C1, driven by a DC source made a sine: R1 and C1 with no model (KiCad infers them from their values), V1
    /// `Simulation_SPICE:VDC` (its library symbol's Sim.Device V, Sim.Type DC), D1 `Device:D` (Sim.Device D, Sim.Pins "1=K 2=A").
    const BUILTIN: &str = "parts:\n  - { reference: V1, value: \"5\", symbol: \"Simulation_SPICE:VDC\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R1, value: 1k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: C1, value: 1u, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: D1, value: 1N4148, symbol: \"Device:D\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\nnets:\n  - { name: IN, pins: [V1.1, R1.1] }\n  - { name: MID, pins: [R1.2, C1.1, D1.2] }\n  - { name: GND, pins: [V1.2, C1.2, D1.1] }\n";

    /// web/studio's TypeScript run by node (tools/sim-model-proof.mjs): the dialog scripted, or KiCad's generator on given fields.
    fn studio(node: &std::path::Path, req: &Value) -> Value {
        let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio");
        let file = std::env::temp_dir().join(format!("eda-sim-model-proof-{}.json", std::process::id()));
        std::fs::write(&file, req.to_string()).unwrap();
        let out = std::process::Command::new(node)
            .current_dir(&web)
            .args(["--experimental-transform-types", "--no-warnings", "--import", "./tools/lib/ts-register.mjs", "tools/sim-model-proof.mjs"])
            .arg(&file)
            .output()
            .unwrap();
        assert!(out.status.success(), "node: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// A symbol's lines in a SPICE netlist: its `.model` card (with its `+` lines) and its item line.
    fn lines_of(spice: &str, reference: &str) -> (String, String) {
        let lines: Vec<&str> = spice.lines().collect();
        let mut model = String::new();
        if let Some(k) = lines.iter().position(|l| l.starts_with(&format!(".model __{reference} "))) {
            model.push_str(lines[k]);
            model.push('\n');
            for l in &lines[k + 1..] {
                if !l.starts_with('+') {
                    break;
                }
                model.push_str(l);
                model.push('\n');
            }
        }
        let item = lines.iter().find(|l| l.starts_with(&format!("{reference} "))).map(|l| format!("{l}\n")).unwrap_or_default();
        (model, item)
    }

    /// Slow tier (kicad-cli, and node for the studio's TypeScript): the Simulation Model dialog's built-in models are what KiCad writes. The
    /// dialog, scripted as a user drives it, sets R1 (inferred, a new value saved in the Value), C1 (inferred, "Save in Value" off), V1 (DC made
    /// a sine, with its AC magnitude) and D1 (two model parameters and an instance one); its verbs are applied as OK sends them. Then:
    /// - the fields are KiCad's (`WriteFields`), typed here by hand -- the sine's td, theta and phase filled with "0" when ac was typed
    ///   (`SIM_MODEL_SOURCE::doSetParamValue`), the diode's instance parameter first;
    /// - kicad-cli's netlist (`kicad-cli sch export netlist --format spice`) holds the lines KiCad's generator gives for those fields
    ///   (`SPICE_GENERATOR` and its classes, ported in simModelBuiltin.ts, run on the items kicad-cli builds), typed here by hand too;
    /// - the same fields written by hand on a second board give the same netlist lines;
    /// - the Code tab shows the same model card and, after its pins ("NC--n": the preview's item has none), the same item line, with the
    ///   reference as the model's name where the netlist has `__REF`.
    #[test]
    fn the_dialogs_built_in_models_are_what_kicad_cli_writes() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: no kicad-cli");
            return;
        }
        let Some(node) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("node")).find(|n| n.exists())) else {
            eprintln!("skipped: no node for the studio's TypeScript");
            return;
        };
        let refs = ["R1", "C1", "V1", "D1"];
        let b = board("builtin", BUILTIN);
        let syms: Vec<Value> = refs.iter().map(|r| symbol_route(&b, json!({ "ref": r }).to_string().as_bytes())).collect();
        for s in &syms {
            assert_eq!(s["ok"], true, "{s}");
        }
        assert_eq!((syms[2]["fields"]["Sim.Type"].as_str(), syms[3]["fields"]["Sim.Pins"].as_str()), (Some("DC"), Some("1=K 2=A")), "the library symbols' fields: {syms:?}");
        let dialog = studio(
            &node,
            &json!({ "dialog": [
                { "symbol": syms[0], "params": [["r", "2.2k"]], "saveInValue": true },
                { "symbol": syms[1], "params": [["c", "470n"]], "saveInValue": false },
                { "symbol": syms[2], "type": "Sine", "params": [["dc", "0.5"], ["ampl", "2"], ["f", "10k"], ["ac", "1"]] },
                { "symbol": syms[3], "params": [["is", "1e-14"], ["n", "1.8"], ["m", "2"]] },
            ] }),
        );
        let d = dialog["dialog"].as_array().unwrap();
        let sim = |k: usize| -> Value {
            let f = d[k]["fields"].as_object().unwrap();
            Value::Object(f.iter().filter(|(n, _)| n.starts_with("Sim.") || *n == "Value").map(|(n, v)| (n.clone(), v.clone())).collect())
        };
        // the fields KiCad's dialog writes, by hand
        let hand = [
            json!({ "Value": "2.2k", "Sim.Device": "R", "Sim.Pins": "1=+ 2=-" }),
            json!({ "Value": "${SIM.PARAMS}", "Sim.Device": "C", "Sim.Params": "c=470n", "Sim.Pins": "1=+ 2=-" }),
            json!({ "Value": "${SIM.PARAMS}", "Sim.Device": "V", "Sim.Type": "SIN", "Sim.Params": "dc=0.5 ampl=2 f=10k td=0 theta=0 phase=0 ac=1", "Sim.Pins": "1=+ 2=-" }),
            json!({ "Value": "1N4148", "Sim.Device": "D", "Sim.Params": "m=2 is=1e-14 n=1.8", "Sim.Pins": "1=K 2=A" }),
        ];
        for k in 0..4 {
            assert_eq!(sim(k), hand[k], "{}: what OK writes", refs[k]);
        }
        assert_eq!(d[0]["saveInValueLabel"], "Save parameter 'Resistance (r)' in Value field");
        assert_eq!(d[2]["saveInValueLabel"], "Save primary parameter in Value field");
        // OK's verbs, one batch per symbol (one undo step each)
        for x in d {
            let cmd: eda_ops::Cmd = serde_json::from_value(json!({ "op": "batch", "cmds": x["cmds"] })).unwrap();
            crate::board::step(&b, cmd, false, "test").unwrap();
        }
        // the same fields by hand on a second board
        let h = board("builtin-hand", BUILTIN);
        let mut edits = Vec::new();
        for (k, r) in refs.iter().enumerate() {
            for (field, value) in hand[k].as_object().unwrap() {
                edits.push(eda_ops::fields_table::FieldEdit { id: r.to_string(), field: field.clone(), value: value.as_str().unwrap().to_string() });
            }
        }
        crate::board::step(&h, eda_ops::Cmd::SetSymbolFields { edits, add_fields: vec![], rename_fields: vec![], remove_fields: vec![] }, false, "test").unwrap();

        let (_, nl) = crate::kicad_engine::sim_netlists(&b).unwrap();
        let (_, nh) = crate::kicad_engine::sim_netlists(&h).unwrap();
        assert!(nl.messages.is_empty(), "kicad-cli: {:?}", nl.messages);
        // KiCad's generator on those fields, for the items kicad-cli builds: the symbol's pins by number (`CreatePinList`'s StrNumCmp), their nets
        let xml = eda_sim::xml::parse_xml_netlist(&nl.xml);
        let net_of = |r: &str, p: &str| xml.nets.iter().find(|n| n.nodes.iter().any(|(a, b)| a == r && b == p)).map(|n| eda_sim::netlist::spice_net_name(&n.name)).unwrap_or_default();
        let items: Vec<Value> = refs
            .iter()
            .enumerate()
            .map(|(k, r)| {
                let mut fields = d[k]["fields"].clone();
                // kicad-cli reads the fields resolved (`GetShownText`): `${SIM.PARAMS}` is the Sim.Params text
                if fields["Value"] == "${SIM.PARAMS}" {
                    fields["Value"] = fields["Sim.Params"].clone();
                }
                json!({ "fields": fields, "pins": [{ "number": "1" }, { "number": "2" }], "item": { "refName": r, "pinNumbers": ["1", "2"], "pinNetNames": [net_of(r, "1"), net_of(r, "2")] } })
            })
            .collect();
        let generated = studio(&node, &json!({ "netlist": items }));
        let expected = [
            ("", "R1 /IN /MID 2.2k\n"),
            ("", "C1 /MID GND 470n\n"),
            ("", "V1 /IN GND DC 0.5 SIN( 0.5 2 10k 0 0 0 ) AC 1  \n"),
            (".model __D1 D\n+           is=10f\n+           n=1.8\n", "D1 /MID GND __D1 m=2\n"),
        ];
        for (k, r) in refs.iter().enumerate() {
            let got = lines_of(&nl.spice, r);
            assert_eq!((got.0.as_str(), got.1.as_str()), expected[k], "{r}: kicad-cli's lines, against KiCad's generator by hand; netlist:\n{}", nl.spice);
            let g = &generated["netlist"][k];
            assert_eq!((g["modelLine"].as_str().unwrap(), g["itemLine"].as_str().unwrap()), expected[k], "{r}: the studio's port of the generator");
            assert_eq!(lines_of(&nh.spice, r), got, "{r}: the same fields written by hand");
        }
        // the Code tab: the model card under the reference's name, the item line after its pins
        let squeeze = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let tabs = ["R NC--0 NC--1 2.2k", "C NC--0 NC--1 470n", "V NC--0 NC--1 DC 0.5 SIN( 0.5 2 10k 0 0 0 ) AC 1", ".model D1 D\n+         is=10f\n+         n=1.8\n\n\nD NC--0 NC--1 D1 m=2"];
        for (k, r) in refs.iter().enumerate() {
            let code = d[k]["code"].as_str().unwrap();
            assert_eq!(code, tabs[k], "{r}: the Code tab");
            let (model, item) = lines_of(&nl.spice, r);
            let code_item = code.lines().last().unwrap();
            let after_pins = |l: &str| l.split_whitespace().skip(3).collect::<Vec<_>>().join(" ");
            assert_eq!(after_pins(code_item).replace(&format!(" {r} "), &format!(" __{r} ")).replace(&format!("{r} "), &format!("__{r} ")), after_pins(&item), "{r}: the item line after its pins");
            let code_model: Vec<&str> = code.lines().filter(|l| l.starts_with(".model") || l.starts_with('+')).collect();
            assert_eq!(squeeze(&code_model.join("\n")).replace(&format!(".model {r} "), &format!(".model __{r} ")), squeeze(&model), "{r}: the model card");
        }
        for dir in [&b, &h] {
            std::fs::remove_dir_all(dir.parent().unwrap()).ok();
        }
    }

    /// Parts of 2, 3 and 4 pins on four nets, for [`every_generator_class_writes_what_kicad_cli_writes`].
    fn classes_yaml() -> String {
        let part = |r: &str, n: usize| {
            let pins: Vec<String> = (1..=n).map(|k| format!("{{ number: \"{k}\", kind: passive }}")).collect();
            // a two-pin part has a package, so the board has an outline to size
            let package = if n == 2 { ", package: \"0603\"" } else { "" };
            format!("  - {{ reference: {r}, value: X{package}, pins: [{}] }}\n", pins.join(", "))
        };
        let parts: [(&str, usize); 12] = [("V2", 2), ("I1", 2), ("B1", 2), ("L1", 2), ("V3", 2), ("X1", 2), ("S1", 4), ("T1", 4), ("E1", 4), ("Q1", 3), ("M1", 4), ("R2", 3)];
        let mut y = String::from("parts:\n");
        for (r, n) in parts {
            y.push_str(&part(r, n));
        }
        y.push_str("nets:\n");
        for net in 1..=4 {
            let pins: Vec<String> = parts.iter().filter(|(_, n)| *n >= net).map(|(r, _)| format!("{r}.{net}")).collect();
            y.push_str(&format!("  - {{ name: N{net}, pins: [{}] }}\n", pins.join(", ")));
        }
        y
    }

    /// Slow tier (kicad-cli, node): the fields of each SPICE generator class, written by hand, give in kicad-cli's netlist the lines the studio's
    /// port of the generator gives for kicad-cli's items -- a pulse (PULSE's "0 1" for ngspice 46), a PWL current (its points in SPICE notation),
    /// a behavioural resistor, an ideal inductor, a white-noise source, a raw SPICE element, a voltage-controlled switch (its `.model` with
    /// `vt`, the output pins first), a lossless line (LTRA from Z0 and the delay), a VCVS, a Gummel-Poon NPN and a level-1 MOSFET (their model
    /// cards, the MOSFET's L and W on its item line), a potentiometer.
    #[test]
    fn every_generator_class_writes_what_kicad_cli_writes() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: no kicad-cli");
            return;
        }
        let Some(node) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("node")).find(|n| n.exists())) else {
            eprintln!("skipped: no node for the studio's TypeScript");
            return;
        };
        let fields: [(&str, &[(&str, &str)]); 12] = [
            ("V2", &[("Sim.Device", "V"), ("Sim.Type", "PULSE"), ("Sim.Params", "y1=0 y2=5 td=1u tr=10n tf=10n tw=1m"), ("Sim.Pins", "1=+ 2=-")]),
            ("I1", &[("Sim.Device", "I"), ("Sim.Type", "PWL"), ("Sim.Params", "pwl=\"0 0 1m 2M {t0 + 1M}\""), ("Sim.Pins", "1=+ 2=-")]),
            ("B1", &[("Sim.Device", "R"), ("Sim.Type", "="), ("Sim.Params", "r=\"V(N1)*2k\""), ("Sim.Pins", "1=+ 2=-")]),
            ("L1", &[("Sim.Device", "L"), ("Sim.Params", "l=10u"), ("Sim.Pins", "1=+ 2=-")]),
            ("V3", &[("Sim.Device", "V"), ("Sim.Type", "WHITENOISE"), ("Sim.Params", "rms=1m dt=1u"), ("Sim.Pins", "1=+ 2=-")]),
            ("X1", &[("Sim.Device", "SPICE"), ("Sim.Params", "type=\"R\" model=\"4.7k\""), ("Sim.Pins", "1=1 2=2")]),
            ("S1", &[("Sim.Device", "SW"), ("Sim.Type", "V"), ("Sim.Params", "thr=2.5 ron=1 roff=1M"), ("Sim.Pins", "1=ctrl+ 2=ctrl- 3=no+ 4=no-")]),
            ("T1", &[("Sim.Device", "TLINE"), ("Sim.Params", "z0=50 td=1n"), ("Sim.Pins", "1=1+ 2=1- 3=2+ 4=2-")]),
            ("E1", &[("Sim.Device", "E"), ("Sim.Params", "gain=10"), ("Sim.Pins", "1=+ 2=- 3=C+ 4=C-")]),
            ("Q1", &[("Sim.Device", "NPN"), ("Sim.Type", "GUMMELPOON"), ("Sim.Params", "bf=200 is=1e-15"), ("Sim.Pins", "1=C 2=B 3=E")]),
            ("M1", &[("Sim.Device", "NMOS"), ("Sim.Type", "MOS1"), ("Sim.Params", "l=1u w=10u vto=0.7 kp=20u"), ("Sim.Pins", "1=D 2=G 3=S 4=B")]),
            ("R2", &[("Sim.Device", "R"), ("Sim.Type", "POT"), ("Sim.Params", "r=10k pos=0.3"), ("Sim.Pins", "1=r0 2=wiper 3=r1")]),
        ];
        let b = board("classes", &classes_yaml());
        let mut edits = Vec::new();
        for (r, fs) in &fields {
            for (f, v) in fs.iter() {
                edits.push(eda_ops::fields_table::FieldEdit { id: r.to_string(), field: f.to_string(), value: v.to_string() });
            }
        }
        crate::board::step(&b, eda_ops::Cmd::SetSymbolFields { edits, add_fields: vec![], rename_fields: vec![], remove_fields: vec![] }, false, "test").unwrap();
        let (_, nl) = crate::kicad_engine::sim_netlists(&b).unwrap();
        let xml = eda_sim::xml::parse_xml_netlist(&nl.xml);
        let items: Vec<Value> = fields
            .iter()
            .map(|(r, fs)| {
                // `CreatePinList`: the symbol's pins by number, each one's net
                let mut pins: Vec<String> = xml.nets.iter().flat_map(|n| n.nodes.iter()).filter(|(a, _)| a == r).map(|(_, p)| p.clone()).collect();
                pins.sort_by(|a, b| str_num_cmp(a, b));
                pins.dedup();
                let nets: Vec<String> = pins.iter().map(|p| xml.nets.iter().find(|n| n.nodes.iter().any(|(a, b)| a == r && b == p)).map(|n| eda_sim::netlist::spice_net_name(&n.name)).unwrap_or_default()).collect();
                let mut f = serde_json::Map::new();
                f.insert("Reference".into(), json!(r));
                f.insert("Value".into(), json!("X"));
                for (k, v) in fs.iter() {
                    f.insert(k.to_string(), json!(v));
                }
                json!({ "fields": f, "pins": pins.iter().map(|p| json!({ "number": p })).collect::<Vec<_>>(), "item": { "refName": r, "pinNumbers": pins, "pinNetNames": nets } })
            })
            .collect();
        let generated = studio(&node, &json!({ "netlist": items }));
        let mut failures = Vec::new();
        for (k, (r, _)) in fields.iter().enumerate() {
            let g = &generated["netlist"][k];
            let (model, item) = (g["modelLine"].as_str().unwrap(), g["itemLine"].as_str().unwrap());
            // the model card is the `.model` line of the item's model name with its "+" lines
            let model_ok = model.is_empty() || nl.spice.contains(model);
            let item_ok = item.lines().all(|l| nl.spice.lines().any(|n| n == l));
            if !model_ok || !item_ok {
                failures.push(format!("{r}: the port gives {model:?} {item:?}"));
            }
        }
        assert!(failures.is_empty(), "{}\nkicad-cli's netlist:\n{}", failures.join("\n"), nl.spice);
        // the classes' own forms, typed by hand from the C++
        // (the nets are labelled on the root sheet: "/N1"; an item's name is its reference after its letter unless it starts with it: "RB1",
        // the raw element's "RX1")
        for line in [
            "V2 /N1 /N2 PULSE( 0 5 1u 10n 10n 1m 0 1 ) ",
            "I1 /N1 /N2 PWL( 0 0 1m 2Meg {t0 + 1Meg} ) ",
            "RB1 /N1 /N2 V(N1)*2k",
            "L1 /N1 /N2 10u",
            "V3 /N1 /N2 TRNOISE( 1m 1u 0 0 0 0 0 ) ",
            "RX1 /N1 /N2 4.7k",
            "E1 /N1 /N2 /N3 /N4 10 ",
            "Q1 /N1 /N2 /N3 __Q1",
            "M1 /N1 /N2 /N3 /N4 __M1 l=1u w=10u",
            "AR2 /N1 /N2 /N3 __R2",
            "OT1 /N1 /N2 /N3 /N4 __T1",
        ] {
            assert!(nl.spice.lines().any(|l| l == line), "{line:?} in\n{}", nl.spice);
        }
        assert!(nl.spice.contains(".model __Q1 NPN\n+           is=1f\n+           bf=200\n"), "the card in the model's parameter order: {}", nl.spice);
        assert!(nl.spice.contains(".model __S1 SW\n+           vt=2.5\n+           ron=1\n+           roff=1Meg\n") && nl.spice.lines().any(|l| l == "S1 /N3 /N4 /N1 /N2 __S1"), "{}", nl.spice);
        assert!(nl.spice.contains(".model __T1 LTRA( r=0 l=5e-08 g=0 c=2e-11 len=1 )\n"), "{}", nl.spice);
        assert!(nl.spice.contains(".model __R2 potentiometer( r=10k position=0.3 )\n"), "{}", nl.spice);
        std::fs::remove_dir_all(b.parent().unwrap()).ok();
    }
}
