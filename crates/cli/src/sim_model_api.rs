//! `/api/sim/model/*`: what the studio's Simulation Model dialog (`web/studio/src/components/simulator/SimModelDialog.tsx`, the library part of
//! `DIALOG_SIM_MODEL`) reads from the server:
//! - `POST /api/sim/model/symbol` `{ref}`: the symbol's simulation fields as the dialog starts from them (`m_fields`: the instance's fields, over
//!   the library symbol's on a sheet the studio drew, which writes those beside the instance's) and its pins sorted by number
//!   (`m_sortedPartPins`: every unit's, a De Morgan alternate's pins left out, `StrNumCmp` order);
//! - `POST /api/sim/model/library` `{path}`: the models of a SPICE library (`loadLibrary`, `eda_sim::library`), each with its pins and the
//!   `Sim.Device` / `Sim.Type` its model type writes, and the path as `Sim.Library` stores it (`TransferDataFromWindow`: made relative to the project
//!   when it is inside it); or what the info bar says when the library cannot be read;
//! - `GET /api/sim/model/files`: the files under the board folder a library can be, for the dialog's Browse (`wxFileDialog` "Browse Models").
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
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
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
    for (n, t) in sch.user_fields.get(&sym.id).into_iter().flatten() {
        fields.insert(n.clone(), json!(t));
    }
    let value = if !sym.value.is_empty() { sym.value.clone() } else { part.and_then(|p| p.value.clone()).unwrap_or_default() };
    let mut pins: Vec<(String, String)> = part.map(|p| p.pins.iter().map(|pin| (pin.number.clone(), pin.name.clone().unwrap_or_default())).collect()).unwrap_or_default();
    pins.sort_by(|a, b| str_num_cmp(&a.0, &b.0));
    pins.dedup_by(|a, b| a.0 == b.0);
    json!({
        "ok": true,
        "ref": sym.id,
        "value": value,
        "imported": sch.imported_from_kicad,
        "fields": fields,
        "pins": pins.iter().map(|(n, name)| json!({ "number": n, "name": name })).collect::<Vec<_>>(),
    })
}

/// The `Sim.Library` text for a path: relative to the project when it is inside it (`fn.MakeRelativeTo( Prj().GetProjectPath() )` and not
/// starting with ".."), else as given.
fn stored_path(path: &str, dir: &Path) -> String {
    let p = Path::new(path);
    if !p.is_absolute() {
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
    if path.to_lowercase().ends_with(".ibs") {
        return json!({ "ok": false, "path": path, "models": [], "messages": ["IBIS models are not read by the studio yet (GAPS 34): edit the Sim.* fields by hand."] });
    }
    let spice_lib_dir = std::env::var_os("SPICE_LIB_DIR").map(std::path::PathBuf::from);
    let lib = read_library(&path, dir, spice_lib_dir.as_deref());
    let models: Vec<Value> = lib
        .models
        .iter()
        .map(|m| match &m.kind {
            ModelKind::Subckt { ports } => json!({ "name": m.name, "kind": "subckt", "device": "SUBCKT", "type": "", "pins": ports, "code": m.code }),
            ModelKind::Model { spice_type, model_type } => match model_type {
                Some(t) => json!({ "name": m.name, "kind": "model", "spice_type": spice_type, "device": t.device, "type": t.type_field, "pins": t.pins, "code": m.code }),
                // `TYPE::RAWSPICE`: KiCad's raw SPICE element, whose pins are not known here
                None => json!({ "name": m.name, "kind": "model", "spice_type": spice_type, "device": "SPICE", "type": "", "pins": [], "code": m.code }),
            },
        })
        .collect();
    json!({ "ok": lib.messages.is_empty(), "path": stored_path(&path, dir), "models": models, "messages": lib.messages })
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
        let f = files_route(&dir);
        assert_eq!(f["files"], json!(["spice/amp.lib"]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
