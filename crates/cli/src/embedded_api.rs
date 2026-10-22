//! The server's half of the embedded files (GAPS.md items 24 and 30): what the Embedded Files panel reads, the file it extracts, the plan of Collect and Embed 3D Models, and the
//! `kicad-embed://` names of the 3D models.
//!
//!   - **`GET /api/embedded?owner=<board|schematic|footprint|library_footprint|library_symbol>[&id=<reference|name|lib_id>]`**: the owner's list, without the data --
//!     `{"owner", "fonts", "files": [{"name", "type", "link", "size", "stored", "checksum", "status", "message", "used_by"}], "inherited": [..]}`. `status` says whether the file
//!     reads (`ok`; `legacy`: its checksum is of an older kind, which KiCad takes and replaces when it saves; `reference`: no data, the board holds it; `error`: it does not decode,
//!     `message` says why). For the board, `used_by` names the footprints whose own list has the file ("Deleted embedded files are also referenced in some footprints ..."),
//!     and `inherited` lists the files only footprints hold, which Board Setup shows and does not let go (`PANEL_EMBEDDED_FILES::RemoveEmbeddedFile`).
//!   - **`GET /api/embedded/file?owner=..&id=..&name=..`**: the file's content, extracted (`PANEL_EMBEDDED_FILES::onExportFiles`); `422` with the reason when it does not decode.
//!   - **`GET /api/embedded/collect`**: what Collect and Embed 3D Models would embed (`PCB_CONTROL::CollectAndEmbed3DModels`) -- `{"count", "models": [{"path", "files", "footprints"}],
//!     "skipped": [{"path", "reason"}]}`. The same plan is made when the `collect_models` verb arrives with none ([`with_collected_models`]), so that the command carries the files and
//!     replays the same elsewhere.
//!   - A footprint's model named `kicad-embed://name` is found by [`crate::model3d_api::resolve`] through [`embedded_model_path`]: the file is extracted under the board's
//!     `.kicad/embed/` (`EMBEDDED_FILES::GetTemporaryFileName`) and served from there.

use crate::model3d_api::{percent_decode, Reply};
use eda_kicad::embedded::{self, EmbedError};
use eda_model::embedded_files::{self, EmbeddedFile, EmbeddedFiles, EmbeddedOwner};
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use eda_ops::{CollectedModel, Cmd, EmbeddedCmd};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

fn reply(status: &'static str, v: Value) -> Reply {
    Reply { status, kind: "application/json", body: v.to_string().into_bytes() }
}

fn query(target: &str, key: &str) -> String {
    let q = target.split_once('?').map_or("", |(_, q)| q);
    q.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('='))).and_then(percent_decode).unwrap_or_default()
}

/// The owner a request names: `owner=board`, `owner=footprint&id=U1`, ...
fn owner_of(target: &str) -> Result<EmbeddedOwner, String> {
    let id = query(target, "id");
    match query(target, "owner").as_str() {
        "board" => Ok(EmbeddedOwner::Board),
        "schematic" => Ok(EmbeddedOwner::Schematic),
        "footprint" if !id.is_empty() => Ok(EmbeddedOwner::Footprint { reference: id }),
        "library_footprint" if !id.is_empty() => Ok(EmbeddedOwner::LibraryFootprint { name: id }),
        "library_symbol" if !id.is_empty() => Ok(EmbeddedOwner::LibrarySymbol { lib_id: id }),
        other => Err(format!("owner must be board, schematic, footprint, library_footprint or library_symbol (with id), not {other:?}")),
    }
}

/// The design of the board in `dir`, read as it is -- no model built: the lists need nothing else.
pub(crate) fn read_design(dir: &Path) -> Result<Design, String> {
    let text = std::fs::read_to_string(dir.join("design.json")).map_err(|e| format!("the board has no design: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("the design is unreadable: {e}"))
}

// ------------------------------------------------------------------------------------------------------------------ what a file is

/// What looking at a file found, remembered by the file's data so that a list read again after every edit does not decompress every file again.
#[derive(Clone)]
struct Seen {
    status: &'static str,
    message: String,
    content: Option<u64>,
}

fn seen_cache() -> &'static Mutex<HashMap<(String, u64, usize), Seen>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, u64, usize), Seen>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn data_key(f: &EmbeddedFile) -> (String, u64, usize) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.data.hash(&mut h);
    (f.checksum.clone(), h.finish(), f.data.len())
}

fn look_at(f: &EmbeddedFile) -> Seen {
    if f.data.is_empty() {
        return Seen { status: "reference", message: "no data of its own: the board's list holds it".into(), content: None };
    }
    let key = data_key(f);
    if let Some(hit) = seen_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return hit.clone();
    }
    let sizes = embedded::sizes(f);
    let seen = match embedded::decode(f) {
        Ok(d) if d.legacy_checksum => Seen { status: "legacy", message: "its checksum is of an older kind; KiCad writes the current one when it saves".into(), content: Some(d.bytes.len() as u64) },
        Ok(d) => Seen { status: "ok", message: String::new(), content: Some(d.bytes.len() as u64) },
        Err(e) => Seen { status: "error", message: e.to_string(), content: sizes.content },
    };
    let mut cache = seen_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() > 512 {
        cache.clear();
    }
    cache.insert(key, seen.clone());
    seen
}

fn file_json(f: &EmbeddedFile, used_by: &[String]) -> Value {
    let seen = look_at(f);
    json!({
        "name": f.name,
        "type": f.kind.word(),
        "type_label": f.kind.label(),
        "link": f.link(),
        "size": seen.content,
        "stored": embedded::sizes(f).stored,
        "checksum": f.checksum,
        "status": seen.status,
        "message": seen.message,
        "used_by": used_by,
    })
}

// ------------------------------------------------------------------------------------------------------------------ the routes

/// `GET /api/embedded`.
pub fn list(dir: &Path, target: &str) -> Reply {
    let owner = match owner_of(target) {
        Ok(o) => o,
        Err(e) => return reply("400 Bad Request", json!({ "error": e })),
    };
    let design = match read_design(dir) {
        Ok(d) => d,
        Err(e) => return reply("404 Not Found", json!({ "error": e })),
    };
    let list = match embedded_files::list_of(&design, &owner) {
        Ok(l) => l,
        Err(e) => return reply("404 Not Found", json!({ "error": e.reason, "check": e.check })),
    };
    list_json(&design, &owner, &list)
}

fn list_json(design: &Design, owner: &EmbeddedOwner, list: &EmbeddedFiles) -> Reply {
    // Which footprints list a file of the board's, and which files only footprints hold.
    let footprint_lists: Vec<(&str, &EmbeddedFiles)> = design.placement.iter().flat_map(|p| p.footprints.iter()).filter_map(|f| embedded_files::of_footprint(design, &f.id).map(|l| (f.id.as_str(), l))).collect();
    let used_by = |name: &str| -> Vec<String> { footprint_lists.iter().filter(|(_, l)| l.get(name).is_some()).map(|(r, _)| r.to_string()).collect() };
    let files: Vec<Value> = list.files.iter().map(|f| file_json(f, &if matches!(owner, EmbeddedOwner::Board) { used_by(&f.name) } else { Vec::new() })).collect();
    let mut inherited: Vec<Value> = Vec::new();
    if matches!(owner, EmbeddedOwner::Board) {
        let mut seen: BTreeMap<&str, (&EmbeddedFile, Vec<String>)> = BTreeMap::new();
        for (r, l) in &footprint_lists {
            for f in l.files.iter().filter(|f| list.get(&f.name).is_none()) {
                seen.entry(f.name.as_str()).or_insert_with(|| (f, Vec::new())).1.push(r.to_string());
            }
        }
        for (f, from) in seen.values() {
            // a reference with no data of its own is looked up in the board's list, which has none of it here: nothing to show but the name
            inherited.push(file_json(f, from));
        }
    }
    reply("200 OK", json!({ "owner": owner, "fonts": list.fonts, "files": files, "inherited": inherited }))
}

/// `GET /api/embedded/file`: the content of one file.
pub fn file(dir: &Path, target: &str) -> Reply {
    let owner = match owner_of(target) {
        Ok(o) => o,
        Err(e) => return reply("400 Bad Request", json!({ "error": e })),
    };
    let name = query(target, "name");
    let design = match read_design(dir) {
        Ok(d) => d,
        Err(e) => return reply("404 Not Found", json!({ "error": e })),
    };
    let list = match embedded_files::list_of(&design, &owner) {
        Ok(l) => l,
        Err(e) => return reply("404 Not Found", json!({ "error": e.reason, "check": e.check })),
    };
    let name = embedded_files::name_of_link(&name).unwrap_or(&name).to_string();
    let Some(found) = list.get(&name) else { return reply("404 Not Found", json!({ "error": format!("no file named '{name}' is embedded") })) };
    // A footprint's file that is only a reference has its data in the board's list (`BOARD::FixupEmbeddedData`).
    let found = if found.data.is_empty() { design.drawings.as_ref().and_then(|d| d.embedded.get(&name)).unwrap_or(found) } else { found };
    match embedded::decode(found) {
        Ok(d) => Reply { status: "200 OK", kind: "application/octet-stream", body: d.bytes },
        Err(EmbedError::NoData) => reply("422 Unprocessable Entity", json!({ "error": format!("'{name}' has no data in this file") })),
        Err(e) => reply("422 Unprocessable Entity", json!({ "error": format!("{name}: {e}") })),
    }
}

// ------------------------------------------------------------------------------------------------------------------ the 3D models

/// The files named `kicad-embed://` that a 3D model can be, by the order a lookup tries them: the board's own list, then the footprints'. The design is read when it has changed.
fn lookup_cache() -> &'static Mutex<HashMap<PathBuf, ((u128, u64), Arc<Vec<EmbeddedFile>>)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, ((u128, u64), Arc<Vec<EmbeddedFile>>)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn lookup_files(dir: &Path) -> Arc<Vec<EmbeddedFile>> {
    let path = dir.join("design.json");
    let stamp = std::fs::metadata(&path).ok().map(|m| (m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos()), m.len())).unwrap_or((0, 0));
    if let Some((was, files)) = lookup_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&path) {
        if *was == stamp {
            return files.clone();
        }
    }
    let files: Arc<Vec<EmbeddedFile>> = Arc::new(read_design(dir).map(|d| embedded_files::lookup_files(&d).into_iter().cloned().collect()).unwrap_or_default());
    lookup_cache().lock().unwrap_or_else(PoisonError::into_inner).insert(path, (stamp, files.clone()));
    files
}

/// The file `kicad-embed://name` names, extracted under the board's `.kicad/embed/` (`FILENAME_RESOLVER::ResolvePath` with the embedded files of the board and its footprints):
/// its path, or why not. `None` for a name that is no such link.
pub(crate) fn embedded_model_path(dir: &Path, link: &str) -> Option<Result<PathBuf, String>> {
    let name = embedded_files::name_of_link(link)?;
    let files = lookup_files(dir);
    let Some(f) = files.iter().find(|f| f.name == name) else { return Some(Err(format!("no file named '{name}' is embedded in the board"))) };
    Some(embedded::materialize(&dir.join(".kicad").join("embed"), f))
}

/// Whether `kicad-embed://name` names a file the board embeds.
pub(crate) fn embeds(dir: &Path, link: &str) -> bool {
    embedded_files::name_of_link(link).is_some_and(|n| lookup_files(dir).iter().any(|f| f.name == n))
}

/// The STEP files that can stand beside a VRML model (`PCB_CONTROL::CollectAndEmbed3DModels`: "Store STEP along with WRL for the OCCT(STEP) exporter").
const STEP_EXTENSIONS: [&str; 9] = ["stp", "step", "STP", "STEP", "Stp", "Step", "stpz", "stpZ", "STPZ"];

/// What Collect and Embed 3D Models would do to the board.
#[derive(Debug, Default)]
pub struct Plan {
    /// The models to embed, by the path the footprints name them with.
    pub models: Vec<CollectedModel>,
    /// How many models of footprints it renames: the number KiCad's message counts ("%d 3D model(s) successfully embedded").
    pub count: usize,
    /// The footprints that use each path.
    pub used_by: BTreeMap<String, Vec<String>>,
    /// The paths that are not embedded, and why.
    pub skipped: Vec<(String, String)>,
}

/// `PCB_CONTROL::CollectAndEmbed3DModels`' search: every model of every footprint on the board that does not begin with `kicad-embed` and names a file that is there.
pub(crate) fn plan(dir: &Path, design: &Design, model: &ConstraintModel) -> Plan {
    let names = crate::library_tables::model_names(dir);
    let roots = crate::model3d_api::board_roots(dir, &names);
    let mut plan = Plan::default();
    let mut tried: BTreeMap<String, Option<CollectedModel>> = BTreeMap::new();
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        for m in eda_kicad::part_models(model, part, &footprint).map(|p| p.models).unwrap_or_default() {
            // `model.m_Filename.StartsWith( FILEEXT::KiCadUriPrefix )`: "kicad-embed", not only the link
            if m.path.is_empty() || m.path.starts_with(embedded_files::URI_SCHEME.trim_end_matches("://")) {
                continue;
            }
            let found = tried.entry(m.path.clone()).or_insert_with(|| match collect_one(&m.path, &roots, &|v| names.get(v)) {
                Ok(c) => Some(c),
                Err(why) => {
                    plan.skipped.push((m.path.clone(), why));
                    None
                }
            });
            if found.is_some() {
                plan.count += 1;
                plan.used_by.entry(m.path.clone()).or_default().push(fp.id.clone());
            }
        }
    }
    plan.models = tried.into_values().flatten().collect();
    plan
}

/// The files of one model path: the model, and for a VRML model the STEP beside it.
fn collect_one(path: &str, roots: &crate::model3d_api::Roots, env: &dyn Fn(&str) -> Option<String>) -> Result<CollectedModel, String> {
    let file = crate::model3d_api::resolve_exact(path, roots, env).ok_or_else(|| "no file of that name was found in KiCad's 3D model library, the board's folder or the folders of its path variables".to_string())?;
    let mut files = vec![read_embedded(&file)?];
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase();
    if ext == "WRL" || ext == "WRZ" {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if let Some(alt) = STEP_EXTENSIONS.iter().map(|e| file.with_file_name(format!("{stem}.{e}"))).find(|p| p.is_file()) {
            if let Ok(f) = read_embedded(&alt) {
                files.push(f);
            }
        }
    }
    Ok(CollectedModel { path: path.to_string(), files })
}

fn read_embedded(path: &Path) -> Result<EmbeddedFile, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len as usize > eda_ops::embedded::MAX_FILE_BYTES {
        return Err(format!("{} is {len} bytes; the largest file that can be embedded is {}", path.display(), eda_ops::embedded::MAX_FILE_BYTES));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("model");
    embedded::encode_new(name, &bytes).map_err(|e| e.to_string())
}

/// `GET /api/embedded/collect`.
pub fn collect_plan(dir: &Path) -> Reply {
    let (_, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return reply("404 Not Found", json!({ "error": crate::board::reasons(&e) })),
    };
    let plan = plan(dir, &design, &model);
    reply(
        "200 OK",
        json!({
            "count": plan.count,
            "models": plan.models.iter().map(|m| json!({ "path": m.path, "files": m.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), "footprints": plan.used_by.get(&m.path) })).collect::<Vec<_>>(),
            "skipped": plan.skipped.iter().map(|(p, why)| json!({ "path": p, "reason": why })).collect::<Vec<_>>(),
        }),
    )
}

/// `cmd` with the models of every `collect_models` in it found and read, or `None` when there is nothing to read (`crate::sheet_import::with_loaded_file` does the same for
/// Import Sheet): a command has to replay and undo the same on a machine that lacks the files.
pub(crate) fn with_collected_models(cmd: &Cmd, dir: &Path, design: &Design, model: &ConstraintModel) -> Result<Option<Cmd>, Vec<CheckResult>> {
    match cmd {
        Cmd::Batch { cmds } => {
            let mut changed = false;
            let mut out = Vec::with_capacity(cmds.len());
            for c in cmds {
                match with_collected_models(c, dir, design, model)? {
                    Some(c) => {
                        changed = true;
                        out.push(c);
                    }
                    None => out.push(c.clone()),
                }
            }
            Ok(changed.then_some(Cmd::Batch { cmds: out }))
        }
        Cmd::Embedded(EmbeddedCmd::CollectModels { models }) if models.is_empty() => Ok(Some(Cmd::Embedded(EmbeddedCmd::CollectModels { models: plan(dir, design, model).models }))),
        _ => Ok(None),
    }
}
