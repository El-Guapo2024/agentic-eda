//! The server's half of the 3D Models tab of the Footprint Properties dialogs (`PANEL_FP_PROPERTIES_3D_MODEL`, `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, and
//! `DIALOG_SELECT_3DMODEL`, `3d-viewer/dialogs/dialog_select_3d_model.cpp`). The models themselves go out through `GET /api/3dmodel` ([`crate::model3d_api`]); this is what
//! the tab asks besides:
//!
//!   - **`POST /api/3dmodel/check {"names": [..]}`**: the status cell of each row (`updateValidateStatus`, `validateModelExists`): "No filename entered", "Illegal filename"
//!     (`FILENAME_RESOLVER::ValidateFileName`, [`eda_model::fp_models::validate_path`]) or "File not found" -- the name is searched for as KiCad's resolver searches
//!     ([`crate::model3d_api::resolve`]). A file this route is not allowed to read (outside the model library, the board's folder and its path variables' folders) is its
//!     own status, `forbidden`: KiCad has no such rule, the studio answers a browser.
//!   - **`GET /api/3dmodel/browse?dir=..&filter=..`**: the model chooser's folders and files. `dir` is a path in the forms a footprint writes (`${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes`,
//!     `${KIPRJMOD}/models`, a folder one of the board's path variables names); without it, only the roots are answered. Every entry carries the path to write in the footprint:
//!     the file shortened to the first root that holds it (`FILENAME_RESOLVER::ShortenPath`), so a model chosen in KiCad's library is `${KICAD10_3DMODEL_DIR}/...` and one in
//!     the board's folder `${KIPRJMOD}/...`. Only the folders `resolve` may read are listed, and only model files (`.wrl`, `.step`, `.stp`) in them.
//!   - **`GET /api/3dmodel/migrate?dirs=..`**: what Migrate 3D Models (`DIALOG_MIGRATE_3D_MODELS`) shows -- the `.wrl` and `.wrz` models of the footprints on the board that are not there
//!     (`collectMissingModels`), each with the footprints that use it, the first one's placement of it, and the STEP-family files of the folders the resolver searches that could stand
//!     for it, ranked (`rankCandidatesFor`, [`eda_model::model_subst`]), as the paths to write in the footprint. `dirs` are the folders the person added (`m_Extra3DSearchDirs`), `|`
//!     between them, each held to what `GET /api/3dmodel` may read. The verb that replaces is `Cmd::MigrateBoardModels`.
//!   - **`GET /api/3dmodel/preview?part=U1`** (a footprint on the board) **or `?footprint=Lib:Name`** (a library footprint): the footprint as the preview panel shows it
//!     (`PANEL_PREVIEW_3D_MODEL`: "the footprint is shown like in Fp editor: rot 0, not flipped") -- its pads, graphics and texts in its own frame on the top side, in the
//!     shape `/api/state` sends a placed part, so the page draws it with the 3D view's own board builder.

use crate::library_tables::{model_names, ModelNames};
use crate::model3d_api::{board_roots, expand, finds_file, format_of, models_dir, percent_decode, resolve, Format, Reject, Reply, Roots};
use eda_model::footprint::placed_pads;
use eda_model::fp_models::{self, PathProblem};
use eda_model::model_subst::{self, CatalogEntry};
use eda_model::ir::{FootprintInstance, LabelSide, Point, Side};
use eda_model::{ConstraintModel, Part};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The most names one `check` request may carry, and the most entries one folder is listed with.
const MAX_CHECK: usize = 256;
const MAX_LISTED: usize = 5000;

fn reply(status: &'static str, v: Value) -> Reply {
    Reply { status, kind: "application/json", body: v.to_string().into_bytes() }
}

// ------------------------------------------------------------------------------------------------------------ the status cell

/// What the status cell says of `name` (`PANEL_FP_PROPERTIES_3D_MODEL::validateModelExists`): `(status, message)`, status one of `ok`, `empty`, `illegal`, `missing`,
/// `forbidden`.
fn status_of(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> (&'static str, String) {
    let name = fp_models::cleanup_path(name);
    match fp_models::validate_path(&name) {
        Err(PathProblem::NoFilename) => return ("empty", PathProblem::NoFilename.message().to_string()),
        Err(PathProblem::Illegal) => return ("illegal", PathProblem::Illegal.message().to_string()),
        Ok(()) => {}
    }
    match resolve(&name, roots, env) {
        Ok(_) => ("ok", String::new()),
        Err(Reject::BadName) => ("illegal", PathProblem::Illegal.message().to_string()),
        Err(Reject::NotFound) => ("missing", "File not found".to_string()),
        Err(Reject::Forbidden(why)) => ("forbidden", why),
    }
}

/// `POST /api/3dmodel/check` for the board in `dir`: `{"results": [{"name", "status", "message"}]}`, one per name asked, in order.
pub fn check(dir: &Path, body: &[u8]) -> Reply {
    let names = model_names(dir);
    check_in(body, &board_roots(dir, &names), &|v| names.get(v))
}

/// [`check`] with the roots and the variables given (the tests').
pub fn check_in(body: &[u8], roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Reply {
    let asked: Vec<String> = serde_json::from_slice::<Value>(body).ok().and_then(|v| v["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect())).unwrap_or_default();
    let results: Vec<Value> = asked
        .iter()
        .take(MAX_CHECK)
        .map(|n| {
            let (status, message) = status_of(n, roots, env);
            json!({ "name": n, "status": status, "message": message })
        })
        .collect();
    reply("200 OK", json!({ "results": results }))
}

// ------------------------------------------------------------------------------------------------------------ the chooser

/// A folder the chooser starts from: the variable that names it (`${NAME}`) and where it is.
pub(crate) struct Root {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

/// The folders `DIALOG_SELECT_3DMODEL` offers (its "directory" choice lists the resolver's search paths: the project's folder, then the paths the person configured, then
/// the model library's): the board's folder as `KIPRJMOD`, the folders the board's own path variables name, and KiCad's variables for 3D models.
fn listing_roots(dir: &Path, names: &ModelNames) -> Vec<Root> {
    let mut out: Vec<Root> = Vec::new();
    let mut add = |name: &str, path: &Path| {
        if let Ok(c) = path.canonicalize() {
            if c.is_dir() && !out.iter().any(|r| r.path == c) {
                out.push(Root { name: name.to_string(), path: c });
            }
        }
    };
    add("KIPRJMOD", dir);
    for (name, path, from_board) in &names.named {
        if *from_board || name.to_ascii_uppercase().contains("3D") {
            add(name, path);
        }
    }
    // The library the installed KiCad ships, under the name its own footprints write, when no variable named it.
    add("KICAD10_3DMODEL_DIR", &models_dir());
    out
}

/// `path` the way a footprint writes it: relative to the first root that holds it, as `${NAME}/folder/file` (`FILENAME_RESOLVER::ShortenPath`); the path as it is when no root does.
fn shorten(path: &Path, roots: &[Root]) -> String {
    for root in roots {
        if let Ok(rel) = path.strip_prefix(&root.path) {
            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            return if rel.is_empty() { format!("${{{}}}", root.name) } else { format!("${{{}}}/{}", root.name, rel.join("/")) };
        }
    }
    path.display().to_string()
}

fn query_value(target: &str, key: &str) -> Option<String> {
    target.split_once('?').map(|(_, q)| q)?.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('='))).and_then(percent_decode)
}

/// What the chooser's filter shows (`m_FileTree->SetFilter`): every model file the studio can show, or only the STEP or only the VRML ones.
fn wanted(path: &Path, filter: &str) -> bool {
    match (format_of(path), filter) {
        (None, _) => false,
        (Some(_), "all" | "") => true,
        (Some(Format::Step), "step") => true,
        (Some(Format::Vrml), "wrl" | "vrml") => true,
        _ => false,
    }
}

/// `GET /api/3dmodel/browse` for the board in `dir`: `{"roots": [{"name", "path"}], "dir", "parent", "entries": [{"name", "kind", "path"}], "truncated"}`; `dir` and `parent`
/// are in the form a footprint writes (`null` for the list of roots). A folder outside what `GET /api/3dmodel` may read is `403`, one that is not there `404`.
pub fn browse(dir: &Path, target: &str) -> Reply {
    let names = model_names(dir);
    browse_in(&query_value(target, "dir").unwrap_or_default(), &query_value(target, "filter").unwrap_or_default(), &board_roots(dir, &names), &listing_roots(dir, &names), &|v| names.get(v))
}

fn browse_in(asked: &str, filter: &str, allowed: &Roots, listing: &[Root], env: &dyn Fn(&str) -> Option<String>) -> Reply {
    let roots_json: Vec<Value> = listing.iter().map(|r| json!({ "name": r.name, "form": format!("${{{}}}", r.name), "path": r.path.display().to_string() })).collect();
    if asked.is_empty() {
        return reply("200 OK", json!({ "roots": roots_json, "dir": null, "parent": null, "entries": [], "truncated": false }));
    }
    if asked.len() > 4096 || asked.contains('\0') {
        return reply("400 Bad Request", json!({ "error": "invalid folder name" }));
    }
    let expanded = expand(&asked.replace('\\', "/"), allowed, env);
    if expanded.contains("${") || expanded.contains("$(") {
        return reply("404 Not Found", json!({ "error": format!("no folder is named {asked}") }));
    }
    let given = PathBuf::from(&expanded);
    let found = if given.is_absolute() { given } else { allowed.project.join(&given) };
    let Ok(canonical) = found.canonicalize() else {
        return reply("404 Not Found", json!({ "error": format!("no folder is named {asked}") }));
    };
    let inside = |p: &Path| [&allowed.models, &allowed.project].into_iter().chain(&allowed.extra).filter_map(|r| r.canonicalize().ok()).any(|r| p.starts_with(&r));
    if !canonical.is_dir() || !inside(&canonical) {
        return reply("403 Forbidden", json!({ "error": "the folder is outside KiCad's 3D model directory, the board's directory and the folders its path variables name" }));
    }
    let mut entries: Vec<(String, bool, PathBuf)> = Vec::new();
    if let Ok(read) = std::fs::read_dir(&canonical) {
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            let is_dir = path.is_dir();
            if is_dir || wanted(&path, filter) {
                entries.push((name, is_dir, path));
            }
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let truncated = entries.len() > MAX_LISTED;
    entries.truncate(MAX_LISTED);
    // a folder may be left upward only to one the route may read
    let parent = canonical.parent().filter(|p| inside(p)).map(|p| shorten(p, listing));
    reply(
        "200 OK",
        json!({
            "roots": roots_json,
            "dir": shorten(&canonical, listing),
            "parent": parent,
            "entries": entries.iter().map(|(n, d, p)| json!({ "name": n, "kind": if *d { "dir" } else { "file" }, "path": shorten(p, listing) })).collect::<Vec<_>>(),
            "truncated": truncated,
        }),
    )
}

// ------------------------------------------------------------------------------------------------------------ Migrate 3D Models

/// The most files the catalog holds and how deep it walks: the model library is 7000 STEP files in a hundred folders; a folder with more than this is not a folder of models.
const MAX_CATALOG: usize = 200_000;
const MAX_DEPTH: usize = 12;

/// Every STEP-family file under `root` (`wxDir::GetAllFiles( .., wxDIR_FILES | wxDIR_DIRS )` with `hasStepExtension`), each once whatever the folder it was reached by.
fn walk_step_files(root: &Path, depth: usize, seen: &mut std::collections::HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_CATALOG {
        return;
    }
    let Ok(read) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.is_dir() {
            if !name.starts_with('.') {
                walk_step_files(&path, depth + 1, seen, out);
            }
        } else if model_subst::has_step_extension(&name) {
            if let Ok(canonical) = path.canonicalize() {
                if seen.insert(canonical.clone()) {
                    out.push(canonical);
                }
            }
        }
    }
}

/// `buildCatalog`: the files of the folders the resolver searches (the board's, the ones its path variables name, KiCad's model library), the board's own `3dshapes` folder, and the
/// folders added by hand -- each of those inside what this route may read.
fn catalog_of(dir: &Path, listing: &[Root], extra: &[PathBuf]) -> Vec<CatalogEntry> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let own = dir.join("3dshapes");
    for root in listing.iter().map(|r| r.path.as_path()).chain(std::iter::once(own.as_path())).chain(extra.iter().map(PathBuf::as_path)) {
        if root.is_dir() {
            walk_step_files(root, 0, &mut seen, &mut files);
        }
    }
    files.iter().map(|f| CatalogEntry::of(&shorten(f, listing))).collect()
}

/// `GET /api/3dmodel/migrate` for the board in `dir`: `{"missing": [{"name", "footprints", "rep", "model", "candidates": [{"path", "display", "score"}]}], "scanned", "dirs"}`.
pub fn migrate(dir: &Path, target: &str) -> Reply {
    let names = model_names(dir);
    migrate_in(dir, target, &board_roots(dir, &names), &listing_roots(dir, &names), &|v| names.get(v))
}

/// [`migrate`] with the folders it may read, the ones it lists and the variables given (the tests').
pub(crate) fn migrate_in(dir: &Path, target: &str, allowed: &Roots, listing: &[Root], env: &dyn Fn(&str) -> Option<String>) -> Reply {
    let (_, design, model) = match crate::board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return reply("404 Not Found", json!({ "error": crate::board::reasons(&e) })),
    };
    // the folders added by hand: only ones this route may read
    let mut extra: Vec<PathBuf> = Vec::new();
    for asked in query_value(target, "dirs").unwrap_or_default().split('|').filter(|d| !d.is_empty()) {
        let expanded = expand(&asked.replace('\\', "/"), allowed, env);
        let given = PathBuf::from(&expanded);
        let found = if given.is_absolute() { given } else { allowed.project.join(given) };
        let Ok(canonical) = found.canonicalize() else {
            return reply("404 Not Found", json!({ "error": format!("no folder is named {asked}") }));
        };
        let inside = [&allowed.models, &allowed.project].into_iter().chain(&allowed.extra).filter_map(|r| r.canonicalize().ok()).any(|r| canonical.starts_with(&r));
        if !canonical.is_dir() || !inside {
            return reply("403 Forbidden", json!({ "error": format!("{asked} is outside KiCad's 3D model directory, the board's directory and the folders its path variables name") }));
        }
        extra.push(canonical);
    }

    // the missing references: a .wrl or .wrz the footprints on the board name that no search finds, each with the first footprint's placement of it
    struct Missing {
        footprints: Vec<String>,
        model: eda_model::footprint::Model3d,
    }
    let mut missing: std::collections::BTreeMap<String, Missing> = std::collections::BTreeMap::new();
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        for m in eda_kicad::part_models(&model, part, &footprint).map(|p| p.models).unwrap_or_default() {
            if m.path.is_empty() || !model_subst::is_wrl(&m.path) || finds_file(&m.path, allowed, env) {
                continue;
            }
            let entry = missing.entry(m.path.clone()).or_insert_with(|| Missing { footprints: Vec::new(), model: m.clone() });
            if !entry.footprints.contains(&fp.id) {
                entry.footprints.push(fp.id.clone());
            }
        }
    }
    let catalog = if missing.is_empty() { Vec::new() } else { catalog_of(dir, listing, &extra) };
    let missing_json: Vec<Value> = missing
        .iter()
        .map(|(name, m)| {
            json!({
                "name": name,
                "footprints": m.footprints,
                "rep": m.footprints.first(),
                "model": { "name": m.model.path, "offset": m.model.offset, "scale": m.model.scale, "rotate": m.model.rotate, "opacity": m.model.opacity, "show": m.model.show },
                "candidates": model_subst::rank_candidates(name, &catalog).iter().map(|c| json!({ "path": c.path, "display": c.display, "score": c.score })).collect::<Vec<_>>(),
            })
        })
        .collect();
    reply("200 OK", json!({ "missing": missing_json, "scanned": catalog.len(), "dirs": extra.iter().map(|d| shorten(d, listing)).collect::<Vec<_>>() }))
}

// ------------------------------------------------------------------------------------------------------------ the preview

/// The part of a footprint at the origin, top side, not turned, in the shape `/api/state` sends a placed part: its pads, its graphics and texts, its courtyard and body.
fn preview_part(model: &ConstraintModel, part: &Part) -> Option<Value> {
    let footprint = model.footprint_of(part)?;
    let pose = FootprintInstance { id: part.reference.clone(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above };
    let pads: Vec<Value> = placed_pads(model, part, &pose)
        .unwrap_or_default()
        .iter()
        .map(|q| json!({ "num": q.number, "net": Value::Null, "x": q.center.x, "y": q.center.y, "w": q.size.0, "h": q.size.1, "round": q.is_round(), "th": q.through_hole }))
        .collect();
    let courtyard = eda_model::footprint::placed_courtyard(model, part, &pose).map(|c| [c.0, c.1, c.2, c.3]);
    let mut p = json!({
        "ref": part.reference,
        "value": part.value,
        "package": part.package,
        "mpn": part.mpn,
        "block": Value::Null,
        "placed": true,
        "size": courtyard.map(|c| [c[2] - c[0], c[3] - c[1]]),
        "footprint": part.footprint.clone().or_else(|| part.package.clone()),
        "at": [0, 0],
        "rot": 0.0,
        "side": "top",
        "label": "above",
        "courtyard": courtyard,
        "body": crate::fp_art_json::placed_body(&pose, &footprint).map(|c| [c.0, c.1, c.2, c.3]),
        "pads": pads,
    });
    crate::fp_art_json::add_art(&mut p, part, &pose, &footprint);
    Some(p)
}

/// `GET /api/3dmodel/preview?part=U1` or `?footprint=Lib:Name` for the board in `dir`: `{"part": <the part JSON>}`, or `{"error"}` with `404` for a name that is nothing.
pub fn preview(dir: &Path, target: &str) -> Reply {
    let not_found = |why: String| reply("404 Not Found", json!({ "error": why }));
    if let Some(reference) = query_value(target, "part") {
        let (_, _, model) = match crate::board::load(dir) {
            Ok(loaded) => loaded,
            Err(e) => return not_found(crate::board::reasons(&e)),
        };
        return match model.part(&reference).and_then(|p| preview_part(&model, p)) {
            Some(part) => reply("200 OK", json!({ "part": part })),
            None => not_found(format!("{reference} is not a footprint of this board with a footprint to show")),
        };
    }
    let Some(name) = query_value(target, "footprint") else {
        return reply("400 Bad Request", json!({ "error": "give part=<reference> or footprint=<Lib:Name>" }));
    };
    let library = match crate::library_api::footprint_any(dir, &name) {
        Ok((fp, _)) => fp,
        Err(e) => return not_found(crate::board::reasons(&e)),
    };
    let engine = library.to_engine_footprint();
    let probe = Part { reference: "REF".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(engine.name.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
    let model = ConstraintModel { parts: vec![probe.clone()], footprints: vec![engine], ..Default::default() };
    match preview_part(&model, &probe) {
        Some(part) => reply("200 OK", json!({ "part": part })),
        None => not_found(format!("{name} has no geometry to show")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_model3d_tab_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#VRML V2.0 utf8\n").unwrap();
    }

    /// A model library and a board folder with a few models, a path variable that names a third folder.
    struct Fixture {
        models: PathBuf,
        project: PathBuf,
        extra: PathBuf,
        outside: PathBuf,
    }

    fn fixture(tag: &str) -> Fixture {
        let root = tree(tag);
        let (models, project, extra, outside) = (root.join("3dmodels"), root.join("board"), root.join("mine"), root.join("elsewhere"));
        touch(&models.join("Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"));
        touch(&models.join("Resistor_SMD.3dshapes/R_0805_2012Metric.step"));
        touch(&models.join("Resistor_SMD.3dshapes/notes.txt"));
        touch(&models.join("Capacitor_SMD.3dshapes/C_0603_1608Metric.stp"));
        touch(&project.join("parts/chip.wrl"));
        touch(&extra.join("custom.step"));
        touch(&outside.join("secret.wrl"));
        Fixture { models, project, extra, outside }
    }

    impl Fixture {
        fn roots(&self) -> Roots {
            Roots { models: self.models.clone(), project: self.project.clone(), extra: vec![self.extra.clone()] }
        }
        fn listing(&self) -> Vec<Root> {
            vec![
                Root { name: "KIPRJMOD".into(), path: self.project.clone() },
                Root { name: "MYMODELS".into(), path: self.extra.clone() },
                Root { name: "KICAD10_3DMODEL_DIR".into(), path: self.models.clone() },
            ]
        }
    }

    fn body(r: &Reply) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    fn env(name: &str) -> Option<String> {
        (name == "MYMODELS").then(|| "/nonexistent".to_string())
    }

    #[test]
    fn the_status_cell_says_what_the_tab_says() {
        let f = fixture("status");
        let ask = |names: &[&str]| -> Vec<(String, String)> {
            let r = check_in(json!({ "names": names }).to_string().as_bytes(), &f.roots(), &env);
            body(&r)["results"].as_array().unwrap().iter().map(|v| (v["status"].as_str().unwrap().to_string(), v["message"].as_str().unwrap().to_string())).collect()
        };
        let got = ask(&[
            "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl",
            "Resistor_SMD.3dshapes/R_0805_2012Metric.step",
            "${KIPRJMOD}/parts/chip.wrl",
            "",
            "bad?.step",
            "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/nothing.step",
            &f.outside.join("secret.wrl").display().to_string(),
            "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/notes.txt",
            "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_\n0603_1608Metric.wrl",
        ]);
        let status: Vec<&str> = got.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(status, ["ok", "ok", "ok", "empty", "illegal", "missing", "forbidden", "forbidden", "ok"]);
        assert_eq!((got[3].1.as_str(), got[4].1.as_str(), got[5].1.as_str()), ("No filename entered", "Illegal filename", "File not found"));
        assert!(got[6].1.contains("outside"), "{:?}", got[6]);
        // a name asked for twice is answered twice, and the answer carries the name back
        let r = check_in(br#"{"names": ["a.step", "a.step"]}"#, &f.roots(), &env);
        assert_eq!(body(&r)["results"].as_array().unwrap().len(), 2);
        assert_eq!(body(&check_in(b"not json", &f.roots(), &env))["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn the_chooser_lists_the_roots_then_a_folder_with_the_paths_a_footprint_writes() {
        let f = fixture("browse");
        let none = browse_in("", "", &f.roots(), &f.listing(), &env);
        let v = body(&none);
        assert_eq!(v["roots"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect::<Vec<_>>(), ["KIPRJMOD", "MYMODELS", "KICAD10_3DMODEL_DIR"]);
        assert_eq!(v["dir"], Value::Null);

        let lib = browse_in("${KICAD10_3DMODEL_DIR}", "", &f.roots(), &f.listing(), &env);
        let v = body(&lib);
        assert_eq!(lib.status, "200 OK");
        let names: Vec<(&str, &str)> = v["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap(), e["kind"].as_str().unwrap())).collect();
        assert_eq!(names, [("Capacitor_SMD.3dshapes", "dir"), ("Resistor_SMD.3dshapes", "dir")]);
        assert_eq!(v["dir"], "${KICAD10_3DMODEL_DIR}");

        let sub = body(&browse_in("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes", "", &f.roots(), &f.listing(), &env));
        let files: Vec<(&str, &str)> = sub["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap(), e["path"].as_str().unwrap())).collect();
        assert_eq!(files, [("R_0603_1608Metric.wrl", "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"), ("R_0805_2012Metric.step", "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0805_2012Metric.step")], "model files only, the path the footprint writes");
        assert_eq!((sub["dir"].as_str(), sub["parent"].as_str()), (Some("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes"), Some("${KICAD10_3DMODEL_DIR}")));
        // the filter
        let step = body(&browse_in("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes", "step", &f.roots(), &f.listing(), &env));
        assert_eq!(step["entries"].as_array().unwrap().len(), 1);
        let wrl = body(&browse_in("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes", "wrl", &f.roots(), &f.listing(), &env));
        assert_eq!(wrl["entries"][0]["name"], "R_0603_1608Metric.wrl");
        // the board's folder, and a path variable's
        let board = body(&browse_in("${KIPRJMOD}/parts", "", &f.roots(), &f.listing(), &env));
        assert_eq!(board["entries"][0]["path"], "${KIPRJMOD}/parts/chip.wrl");
        let mine = body(&browse_in(&f.extra.display().to_string(), "", &f.roots(), &f.listing(), &env));
        assert_eq!(mine["entries"][0]["path"], "${MYMODELS}/custom.step");
        // a relative folder is the board's
        assert_eq!(body(&browse_in("parts", "", &f.roots(), &f.listing(), &env))["entries"][0]["name"], "chip.wrl");
    }

    #[test]
    fn the_chooser_does_not_leave_the_folders_the_model_route_may_read() {
        let f = fixture("forbidden");
        for asked in [f.outside.display().to_string(), format!("{}/..", f.models.display()), "${KICAD10_3DMODEL_DIR}/../elsewhere".to_string(), "/".to_string()] {
            let r = browse_in(&asked, "", &f.roots(), &f.listing(), &env);
            assert_eq!(r.status, "403 Forbidden", "{asked}: {}", String::from_utf8_lossy(&r.body));
        }
        assert_eq!(browse_in("${KICAD10_3DMODEL_DIR}/Nothing.3dshapes", "", &f.roots(), &f.listing(), &env).status, "404 Not Found");
        assert_eq!(browse_in("${NO_SUCH_VARIABLE}/x", "", &f.roots(), &f.listing(), &env).status, "404 Not Found");
        // the top of a root has no parent to climb to
        let top = body(&browse_in("${KICAD10_3DMODEL_DIR}", "", &f.roots(), &f.listing(), &env));
        assert_eq!(top["parent"], Value::Null);
    }

    #[test]
    fn a_path_is_shortened_to_the_first_root_that_holds_it() {
        let f = fixture("shorten");
        let listing = f.listing();
        assert_eq!(shorten(&f.models.join("Resistor_SMD.3dshapes/x.step"), &listing), "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/x.step");
        assert_eq!(shorten(&f.project.join("parts"), &listing), "${KIPRJMOD}/parts");
        assert_eq!(shorten(&f.models, &listing), "${KICAD10_3DMODEL_DIR}");
        assert_eq!(shorten(&f.outside.join("secret.wrl"), &listing), f.outside.join("secret.wrl").display().to_string(), "no root: as it is");
    }

    #[test]
    fn the_catalog_holds_the_step_files_of_the_folders_once_each_with_the_paths_a_footprint_writes() {
        let f = fixture("catalog");
        touch(&f.models.join("Resistor_SMD.3dshapes/R_0603_1608Metric.stp"));
        touch(&f.models.join("Resistor_SMD.3dshapes/deeper/R_1206.step.gz"));
        touch(&f.models.join(".hidden/secret.step"));
        touch(&f.project.join("3dshapes/own.step"));
        let catalog = catalog_of(&f.project, &f.listing(), &[f.extra.clone()]);
        let mut paths: Vec<&str> = catalog.iter().map(|c| c.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "${KICAD10_3DMODEL_DIR}/Capacitor_SMD.3dshapes/C_0603_1608Metric.stp",
                "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.stp",
                "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0805_2012Metric.step",
                "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/deeper/R_1206.step.gz",
                "${KIPRJMOD}/3dshapes/own.step",
                "${MYMODELS}/custom.step",
            ],
            "STEP-family files only, no hidden folder, a root named by the first that holds it, nothing twice"
        );
        assert_eq!(catalog.iter().find(|c| c.path.ends_with("R_1206.step.gz")).unwrap().stem, "r_1206");
        assert_eq!(catalog.iter().find(|c| c.path.ends_with("R_1206.step.gz")).unwrap().parent, "deeper");
    }

    #[test]
    fn a_name_is_found_when_a_file_by_that_name_is_there_and_a_wrl_is_not_found_by_its_step() {
        let f = fixture("finds");
        touch(&f.models.join("Resistor_SMD.3dshapes/R_0402.step"));
        let ask = |name: &str| finds_file(name, &f.roots(), &env);
        assert!(ask("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"));
        assert!(ask("Resistor_SMD.3dshapes/R_0805_2012Metric.step"), "relative to the model library");
        assert!(ask("${KIPRJMOD}/parts/chip.wrl"));
        assert!(!ask("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0402.wrl"), "its STEP is there; the file it names is not (the view's stand-in is not KiCad's resolver)");
        assert!(!ask("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/nothing.wrl") && !ask("") && !ask("${NO_SUCH}/x.wrl"));
        assert!(ask(&f.outside.join("secret.wrl").display().to_string()), "a file is there whether the route may serve it or not");
    }

    #[test]
    fn the_preview_is_the_footprint_at_the_origin_with_its_pads_and_drawings() {
        use eda_model::footprint::{Pad, PadKind, PadShape};
        let pad = |n: &str, x: i64, y: i64| Pad { number: n.into(), at: (x, y), size: (800_000, 600_000), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() };
        let mut art = eda_model::fp_art::FpArt::default();
        art.shapes.push(eda_model::fp_art::FpShape::new(eda_model::ir::Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: Point { x: -1_000_000, y: -900_000 }, end: Point { x: 1_000_000, y: -900_000 } }));
        let footprint = eda_model::Footprint { name: "T:Two".into(), pads: vec![pad("1", -1_000_000, 0), pad("2", 1_000_000, 0)], courtyard: Some((2_400_000, 1_000_000)), art, ..Default::default() };
        let probe = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("v".into()), package: None, footprint: Some("T:Two".into()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
        let model = ConstraintModel { parts: vec![probe.clone()], footprints: vec![footprint], ..Default::default() };
        let p = preview_part(&model, &probe).unwrap();
        assert_eq!((p["side"].as_str(), p["at"].clone(), p["rot"].as_f64()), (Some("top"), json!([0, 0]), Some(0.0)));
        let pads: Vec<(i64, i64, i64, i64)> = p["pads"].as_array().unwrap().iter().map(|q| (q["x"].as_i64().unwrap(), q["y"].as_i64().unwrap(), q["w"].as_i64().unwrap(), q["h"].as_i64().unwrap())).collect();
        assert_eq!(pads, [(-1_000_000, 0, 800_000, 600_000), (1_000_000, 0, 800_000, 600_000)], "in the footprint's own frame");
        assert_eq!(p["graphics"].as_array().unwrap().len(), 1);
        assert_eq!(p["courtyard"], json!([-2_400_000, -1_000_000, 2_400_000, 1_000_000]));
    }
}
