//! The library tables of a board, as the studio reads them: which libraries exist, and where.
//!
//! A `Lib:Name` the studio is asked for -- in a tree, a chooser, a placement, a lookup -- is found the way KiCad finds it: the library nicknamed `Lib` in
//! the library tables, and the item `Name` in that library's file or folder. The tables are the global one in KiCad's own configuration folder
//! (`~/Library/Preferences/kicad/<version>/sym-lib-table`, which chains the 222 symbol and 155 footprint libraries of KiCad's install) and the board's own
//! (`sym-lib-table`, `fp-lib-table` in the board's folder, the studio's project folder). The pure half -- the file format, the path variables, how the
//! tables are put together -- is `eda_kicad::library_table`; this module is what needs the machine: where KiCad's configuration is, which variables it
//! defines, the project's files.
//!
//! The global tables are KiCad's and are only ever read. Nothing here writes into KiCad's configuration folder.
//!
//! [`libs`] is the one entry point the rest of the server uses: the libraries of one kind for a board, as a [`LibSet`] (a [`LibSource`], so the index, the
//! choosers and the lookups work over it exactly as they do over a plain folder of library files).

use crate::library_index::{count_footprints, modified, Kind, LibSource, LibraryInfo};
use eda_kicad::library_table::{self as lt, LoadedRow, LoadedTable, Scope, Table, TableKind, Vars};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// Where KiCad's configuration is, when it is not where this machine's KiCad keeps it (`~/Library/Preferences/kicad/<version>` on a Mac): the folder that
/// holds `sym-lib-table`, `fp-lib-table` and `kicad_common.json`. Tests point it at a folder of their own, so they never depend on the user's KiCad.
pub const CONFIG_DIR_ENV: &str = "EDA_KICAD_CONFIG_DIR";

// ------------------------------------------------------------------------------------------------ KiCad's environment

/// What this machine says about KiCad: its configuration folder, its version, the folders it was installed to, the variables the user defined in Configure
/// Paths there, and the real environment.
#[derive(Clone, Debug)]
pub(crate) struct Env {
    /// KiCad's major version (the `10` of `KICAD10_SYMBOL_DIR`), from the configuration folder's name.
    pub(crate) major: u32,
    /// `10.99`: the configuration folder's own name, when it has one.
    pub(crate) version: Option<String>,
    pub(crate) config_dir: Option<PathBuf>,
    pub(crate) stock: lt::Stock,
    /// `environment.vars` of KiCad's `kicad_common.json`.
    pub(crate) configured: BTreeMap<String, String>,
    pub(crate) real: BTreeMap<String, String>,
}

fn home(real: &BTreeMap<String, String>) -> Option<PathBuf> {
    real.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// The configuration folder: `EDA_KICAD_CONFIG_DIR` as it is, else the newest version folder of KiCad's configuration root (`KICAD_CONFIG_HOME`, else the
/// platform's: `~/Library/Preferences/kicad`, `~/.config/kicad`).
fn config_dir(real: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(dir) = real.get(CONFIG_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let root = match real.get("KICAD_CONFIG_HOME").filter(|d| !d.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => {
            let home = home(real)?;
            if cfg!(target_os = "macos") {
                home.join("Library").join("Preferences").join("kicad")
            } else {
                real.get("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config")).join("kicad")
            }
        }
    };
    lt::discover_config_dir(&root)
}

impl Env {
    pub(crate) fn discover() -> Env {
        Env::from_environment(std::env::vars().collect())
    }

    /// [`Env::discover`] over a given environment (the tests').
    pub(crate) fn from_environment(real: BTreeMap<String, String>) -> Env {
        let config_dir = config_dir(&real);
        let version = config_dir.as_ref().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).filter(|n| lt::major_of(n).is_some());
        let major = version.as_deref().and_then(lt::major_of).unwrap_or(10);
        let symbols = eda_kicad::default_symbol_library_root();
        let models = real.get("EDA_KICAD_3DMODELS_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| symbols.parent().map_or_else(|| PathBuf::from("3dmodels"), |p| p.join("3dmodels")));
        let documents = home(&real).unwrap_or_default().join("Documents").join("KiCad").join(version.clone().unwrap_or_else(|| format!("{major}.0")));
        let stock = lt::Stock::new(symbols, eda_kicad::default_footprint_library_root(), models, &documents);
        let configured = config_dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("kicad_common.json")).ok()).map(|t| lt::configured_env_vars(&t)).unwrap_or_default();
        Env { major, version, config_dir, stock, configured, real }
    }

    /// KiCad's own variables, as Configure Paths lists them.
    pub(crate) fn global_vars(&self) -> Vec<lt::EnvVar> {
        lt::global_env_vars(self.major, &self.stock, &self.configured, &self.real)
    }

    /// What `${NAME}` means for the project in `project_dir`: its own variables first, then `KIPRJMOD` (its folder), KiCad's variables and the environment.
    pub(crate) fn vars(&self, project_dir: &Path, project_vars: &BTreeMap<String, String>) -> Vars {
        let mut env = self.real.clone();
        for v in self.global_vars() {
            env.insert(v.name, v.value);
        }
        env.insert("KIPRJMOD".into(), project_dir.display().to_string());
        Vars { text: project_vars.clone(), env, predefined: lt::predefined_names(self.major) }
    }
}

// ------------------------------------------------------------------------------------------------ the tables of a board

/// Where the table the studio uses as the global one came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GlobalOrigin {
    /// KiCad's own global table, in its configuration folder.
    Config(PathBuf),
    /// KiCad has written none (never run, or another configuration folder): the table its install ships, which is what KiCad would create.
    Stock(PathBuf),
    /// Neither exists: the library files in KiCad's library folder, each its own row.
    Folder(PathBuf),
    /// There is nothing to read.
    Missing,
}

/// The two tables of one kind for a board, read. Nothing is cached: a table is a few kilobytes and a stat per library.
pub(crate) struct Loaded {
    pub(crate) env: Env,
    pub(crate) project_vars: BTreeMap<String, String>,
    pub(crate) vars: Vars,
    pub(crate) kind: TableKind,
    /// The table used as the global one, with what it chains.
    pub(crate) global: Option<LoadedTable>,
    pub(crate) global_origin: GlobalOrigin,
    /// Why KiCad's own global table could not be used, when it exists and could not be read.
    pub(crate) global_error: Option<String>,
    pub(crate) project_path: PathBuf,
    /// The project's table: none when the board has not made one.
    pub(crate) project: Option<LoadedTable>,
    pub(crate) project_error: Option<String>,
}

/// The file the board's table of this kind is kept in.
pub(crate) fn project_table_path(dir: &Path, kind: TableKind) -> PathBuf {
    dir.join(kind.file_name())
}

/// The libraries of KiCad's library folder as a table of their own, for a machine whose KiCad has no table to read: one row per `.kicad_sym` file or
/// `.pretty` folder, named by its file name.
fn folder_table(kind: TableKind, root: &Path) -> LoadedTable {
    let mut rows: Vec<lt::Row> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
                    let name = path.file_stem().and_then(|s| s.to_str())?.to_string();
                    let wanted = match kind {
                        TableKind::Symbol => ext == "kicad_sym" && path.is_file(),
                        TableKind::Footprint => ext == "pretty" && path.is_dir(),
                        TableKind::DesignBlock => ext == eda_kicad::design_block::LIBRARY_EXT && path.is_dir(),
                    };
                    wanted.then(|| lt::Row { nickname: name, uri: path.display().to_string(), lib_type: lt::KICAD_TYPE.into(), ..Default::default() })
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| a.nickname.cmp(&b.nickname));
    let loaded = rows.iter().map(|_| LoadedRow { ok: true, ..Default::default() }).collect();
    LoadedTable { path: root.join(kind.file_name()), scope: Scope::Global, table: Table { kind, version: Some(lt::TABLE_VERSION), rows }, rows: loaded }
}

/// Read the tables of `kind` for the board in `dir`.
pub(crate) fn load(dir: &Path, kind: TableKind) -> Loaded {
    load_with(Env::discover(), dir, kind)
}

pub(crate) fn load_with(env: Env, dir: &Path, kind: TableKind) -> Loaded {
    let project_vars = crate::text_variables::project_variables(dir);
    let vars = env.vars(dir, &project_vars);
    let mut global_error = None;
    let mut global: Option<LoadedTable> = None;
    let mut origin = GlobalOrigin::Missing;
    if let Some(cfg) = &env.config_dir {
        let path = cfg.join(kind.file_name());
        if path.is_file() {
            match LoadedTable::load(&path, kind, Scope::Global, &vars) {
                Ok(t) => {
                    global = Some(t);
                    origin = GlobalOrigin::Config(path);
                }
                Err(e) => global_error = Some(e),
            }
        }
    }
    if global.is_none() {
        let stock = env.stock.templates.join(kind.file_name());
        if let Ok(t) = LoadedTable::load(&stock, kind, Scope::Global, &vars) {
            global = Some(t);
            origin = GlobalOrigin::Stock(stock);
        }
    }
    if global.is_none() {
        let root = match kind {
            TableKind::Symbol => env.stock.symbols.clone(),
            TableKind::Footprint => env.stock.footprints.clone(),
            TableKind::DesignBlock => env.stock.design_blocks.clone(),
        };
        if root.is_dir() {
            global = Some(folder_table(kind, &root));
            origin = GlobalOrigin::Folder(root);
        }
    }
    let project_path = project_table_path(dir, kind);
    let (mut project, mut project_error) = (None, None);
    if project_path.is_file() {
        match LoadedTable::load(&project_path, kind, Scope::Project, &vars) {
            Ok(t) => project = Some(t),
            Err(e) => project_error = Some(e),
        }
    }
    Loaded { env, project_vars, vars, kind, global, global_origin: origin, global_error, project_path, project, project_error }
}

impl Loaded {
    /// Every library the tables define, in the order KiCad reads them (`LIBRARY_MANAGER::Rows`), found or not.
    pub(crate) fn libs(&self) -> Vec<lt::Lib> {
        let tables: Vec<&LoadedTable> = self.global.iter().chain(self.project.iter()).collect();
        lt::effective_libs(self.kind, &tables, &self.vars)
    }
}

// ------------------------------------------------------------------------------------------------ the libraries of a board

/// One library that can be read: where it is, and what its row says of it.
#[derive(Clone, Debug)]
pub(crate) struct LibEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) description: String,
    pub(crate) scope: Scope,
    pub(crate) hidden: bool,
}

/// The libraries of one kind of a board that can be read and are switched on, by nickname: what the trees, the choosers and the lookups search.
/// A library whose "Show" box is off is found (a symbol placed from it still resolves) but not listed.
#[derive(Clone, Debug)]
pub(crate) struct LibSet {
    entries: Vec<LibEntry>,
    by_name: HashMap<String, usize>,
    fingerprint: String,
}

impl LibSet {
    pub(crate) fn from_libs(kind: Kind, libs: Vec<lt::Lib>) -> LibSet {
        let mut entries: Vec<LibEntry> = libs
            .into_iter()
            .filter(|l| l.is_available())
            .map(|l| LibEntry { name: l.nickname, path: l.path, description: l.description, scope: l.scope, hidden: l.hidden })
            .collect();
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
        let by_name = entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
        // The set is another set when a library came or went, moved, or changed on disk.
        let mut text = format!("{kind:?}");
        for e in &entries {
            let stamp = modified(&e.path).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            text.push_str(&format!("|{}={}@{stamp}{}", e.name, e.path.display(), if e.hidden { "h" } else { "" }));
        }
        let fingerprint = blake3::hash(text.as_bytes()).to_hex()[..16].to_string();
        LibSet { entries, by_name, fingerprint }
    }

    pub(crate) fn find(&self, name: &str) -> Option<&LibEntry> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[LibEntry] {
        &self.entries
    }
}

/// How many footprints a library folder holds, read again only when the folder changes.
fn footprint_count(dir: &Path) -> usize {
    type Counts = Mutex<HashMap<PathBuf, (Option<SystemTime>, usize)>>;
    static COUNTS: OnceLock<Counts> = OnceLock::new();
    let stamp = modified(dir);
    let counts = COUNTS.get_or_init(Default::default);
    if let Some((s, n)) = counts.lock().unwrap_or_else(PoisonError::into_inner).get(dir) {
        if *s == stamp {
            return *n;
        }
    }
    let n = count_footprints(dir);
    counts.lock().unwrap_or_else(PoisonError::into_inner).insert(dir.to_path_buf(), (stamp, n));
    n
}

impl LibSource for LibSet {
    fn lib_path(&self, _kind: Kind, lib: &str) -> Result<PathBuf, String> {
        self.find(lib).map(|e| e.path.clone()).ok_or_else(|| format!("no library {lib:?} is installed"))
    }

    fn listed(&self, kind: Kind) -> Arc<Vec<LibraryInfo>> {
        Arc::new(
            self.entries
                .iter()
                .filter(|e| !e.hidden)
                .map(|e| LibraryInfo { name: e.name.clone(), count: (kind == Kind::Footprint).then(|| footprint_count(&e.path)) })
                .collect(),
        )
    }

    fn describe(&self, _kind: Kind, lib: &str) -> Option<String> {
        self.find(lib).map(|e| e.description.clone()).filter(|d| !d.is_empty())
    }

    fn scope_of(&self, _kind: Kind, lib: &str) -> Option<&'static str> {
        self.find(lib).map(|e| e.scope.name())
    }

    fn fingerprint(&self, kind: Kind) -> String {
        format!("tables:{kind:?}:{}", self.fingerprint)
    }

    fn boxed(&self) -> Arc<dyn LibSource> {
        Arc::new(self.clone())
    }
}

/// The libraries of `kind` of the board in `dir`: KiCad's own through the global table, the board's through its project table, a nickname defined by both
/// being the project's.
pub(crate) fn libs(dir: &Path, kind: Kind) -> Arc<LibSet> {
    Arc::new(LibSet::from_libs(kind, load(dir, kind.table_kind()).libs()))
}

// ------------------------------------------------------------------------------------------------ the dialogs' data

fn bad(msg: impl Into<String>) -> Value {
    json!({ "error": msg.into() })
}

fn parse_body(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice::<Value>(body).map_err(|e| bad(format!("bad request: {e}")))
}

fn kind_in(req: &Value) -> Result<TableKind, Value> {
    req.get("kind").and_then(Value::as_str).and_then(TableKind::parse).ok_or_else(|| bad("kind is symbol, footprint or design_block"))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The rows of a request, field by field (the wire shape is the dialog's grid: `enabled` and `visible` are its two check boxes).
fn rows_in(req: &Value) -> Result<Vec<lt::Row>, Value> {
    let Some(rows) = req.get("rows").and_then(Value::as_array) else { return Err(bad("rows is a list")) };
    Ok(rows
        .iter()
        .map(|r| lt::Row {
            nickname: text(r, "nickname"),
            uri: text(r, "uri"),
            lib_type: text(r, "type"),
            options: text(r, "options"),
            description: text(r, "descr"),
            disabled: r.get("enabled").and_then(Value::as_bool) == Some(false),
            hidden: r.get("visible").and_then(Value::as_bool) == Some(false),
        })
        .collect())
}

/// `GuessPluginTypeFromLibPath`: a table file is a nested table; KiCad's own formats are told by their extension.
fn guess_type(kind: TableKind, path: &Path) -> Option<&'static str> {
    if path.is_file() {
        if let Ok(t) = std::fs::read_to_string(path).map_err(|_| ()).and_then(|t| Table::parse(&t).map_err(|_| ())) {
            if t.kind == kind {
                return Some(lt::NESTED_TYPE);
            }
        }
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match kind {
        TableKind::Symbol => (ext == "kicad_sym").then_some(lt::KICAD_TYPE),
        TableKind::Footprint => (ext == "pretty").then_some(lt::KICAD_TYPE),
        // `DESIGN_BLOCK_IO_MGR::GuessPluginTypeFromLibPath` asks `CanReadLibrary`, which looks for a file with the extension *inside* the folder and so never finds
        // one in a real library (a library holds only block folders); the folder's own extension is what tells it here.
        TableKind::DesignBlock => (ext == eda_kicad::design_block::LIBRARY_EXT && path.is_dir()).then_some(lt::KICAD_TYPE),
    }
}

fn row_json(kind: TableKind, row: &lt::Row, loaded: Option<&LoadedRow>, vars: &Vars, base: &Path) -> Value {
    let path = vars.expand_uri(&row.uri, base);
    let error = match loaded {
        Some(l) if !l.ok => Some(l.error.clone()),
        _ => lt::check_row(kind, row, &path),
    };
    let status = if error.is_some() { "error" } else if row.is_table() { "table" } else { "ok" };
    let mut v = json!({
        "nickname": row.nickname, "uri": row.uri, "type": row.lib_type, "options": row.options, "descr": row.description,
        "enabled": !row.disabled, "visible": !row.hidden,
        "status": status, "error": error.unwrap_or_default(), "path": path.display().to_string(),
    });
    if let Some(child) = loaded.and_then(|l| l.child.as_deref()) {
        v["table"] = table_json(kind, child, vars);
    }
    v
}

/// A table with the rows it holds, a chained table's own rows below its row.
fn table_json(kind: TableKind, t: &LoadedTable, vars: &Vars) -> Value {
    let base = t.path.parent().map(Path::to_path_buf).unwrap_or_default();
    json!({
        "path": t.path.display().to_string(),
        "rows": t.table.rows.iter().zip(&t.rows).map(|(r, l)| row_json(kind, r, Some(l), vars, &base)).collect::<Vec<_>>(),
    })
}

fn global_note(kind: TableKind, origin: &GlobalOrigin) -> String {
    let name = kind.file_name();
    match origin {
        GlobalOrigin::Config(_) => format!("This is KiCad's own global {name}. The studio reads it and never writes it: change it in KiCad (Preferences, Manage Libraries)."),
        GlobalOrigin::Stock(p) => format!("KiCad has written no global {name} in its configuration folder, so the studio reads the table KiCad installs ({}) -- the one KiCad creates on its first run. The studio never writes a global table.", p.display()),
        GlobalOrigin::Folder(p) => format!("No library table was found: the library files in {} are listed, each as its own row. The studio never writes a global table.", p.display()),
        GlobalOrigin::Missing => "No KiCad library table or library folder was found on this machine.".to_string(),
    }
}

/// The two tables of one kind: the global one (read only) and the board's.
fn kind_json(l: &Loaded) -> Value {
    let kind = l.kind;
    let global_path = match &l.global_origin {
        GlobalOrigin::Config(p) | GlobalOrigin::Stock(p) | GlobalOrigin::Folder(p) => p.clone(),
        GlobalOrigin::Missing => l.env.config_dir.clone().unwrap_or_default().join(kind.file_name()),
    };
    let origin = match &l.global_origin {
        GlobalOrigin::Config(_) => "config",
        GlobalOrigin::Stock(_) => "stock",
        GlobalOrigin::Folder(_) => "folder",
        GlobalOrigin::Missing => "missing",
    };
    let global_rows = l.global.as_ref().map(|t| table_json(kind, t, &l.vars)["rows"].clone()).unwrap_or_else(|| json!([]));
    let project_rows = l.project.as_ref().map(|t| table_json(kind, t, &l.vars)["rows"].clone()).unwrap_or_else(|| json!([]));
    let taken: Vec<String> = l.libs().into_iter().map(|lib| lib.nickname).collect();
    json!({
        "kind": kind.name(),
        "file": kind.file_name(),
        "supports_visibility": kind.supports_visibility(),
        "types": lt::known_types(kind),
        "new_library_name": unique_name(&taken),
        "global": { "scope": "global", "path": global_path.display().to_string(), "exists": l.global.is_some(), "writable": false, "origin": origin,
                    "note": global_note(kind, &l.global_origin), "error": l.global_error, "rows": global_rows },
        "project": { "scope": "project", "path": l.project_path.display().to_string(), "exists": l.project_path.is_file(), "writable": true,
                     "error": l.project_error, "rows": project_rows },
    })
}

/// `SYMBOL_LIBRARY_MANAGER::GetUniqueLibraryName`: `New_Library`, else `New_Library_0`, `New_Library_1`, ...
fn unique_name(taken: &[String]) -> String {
    let base = "New_Library".to_string();
    if !taken.contains(&base) {
        return base;
    }
    (0u32..).map(|i| format!("{base}_{i}")).find(|n| !taken.contains(n)).unwrap_or(base)
}

/// The variables used in the tables and defined nowhere: KiCad lists them too (an old `KICAD8_FOOTPRINT_DIR` in a table), with the value the real
/// environment gives them, so the person sees what a path needs.
fn undefined_variables(loaded: &[Loaded]) -> Vec<Value> {
    let mut defined: Vec<String> = vec!["KIPRJMOD".to_string()];
    let mut used: Vec<String> = Vec::new();
    for l in loaded {
        defined.extend(l.env.global_vars().into_iter().map(|v| v.name));
        defined.extend(l.project_vars.keys().cloned());
        for lib in l.libs() {
            for name in lt::variables_used(&lib.uri) {
                if !used.contains(&name) {
                    used.push(name);
                }
            }
        }
        for table in l.global.iter().chain(l.project.iter()) {
            for row in &table.table.rows {
                for name in lt::variables_used(&row.uri) {
                    if !used.contains(&name) {
                        used.push(name);
                    }
                }
            }
        }
    }
    let real = loaded.first().map(|l| l.env.real.clone()).unwrap_or_default();
    used.into_iter().filter(|n| !defined.contains(n)).map(|n| json!({ "name": n, "value": real.get(&n).cloned().unwrap_or_default(), "external": real.contains_key(&n) })).collect()
}

/// `GET /api/library_tables`: everything the Manage Symbol Libraries and Manage Footprint Libraries dialogs and Configure Paths show.
pub(crate) fn view(dir: &Path) -> Value {
    view_with(&Env::discover(), dir)
}

pub(crate) fn view_with(env: &Env, dir: &Path) -> Value {
    let loaded: Vec<Loaded> = TableKind::ALL.iter().map(|&k| load_with(env.clone(), dir, k)).collect();
    let mut tables = serde_json::Map::new();
    for l in &loaded {
        tables.insert(l.kind.name().to_string(), kind_json(l));
    }
    // Configure Paths lists the project's variables that are named like a path variable; the Text Variables page of Board Setup edits all of them.
    let project_vars: BTreeMap<String, String> = crate::text_variables::project_variables(dir).into_iter().filter(|(name, _)| lt::is_variable_name(name)).collect();
    json!({
        "project_dir": dir.display().to_string(),
        "config_dir": env.config_dir.as_ref().map(|d| d.display().to_string()),
        "version": env.version,
        "major": env.major,
        "tables": tables,
        "variables": {
            "global": env.global_vars().into_iter().map(|v| {
                let predefined = lt::is_predefined(&v.name, env.major);
                json!({ "name": v.name, "value": v.value, "external": v.external, "default": v.default, "predefined": predefined })
            }).collect::<Vec<_>>(),
            "project": project_vars.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>(),
            "undefined": undefined_variables(&loaded),
            "reserved": ["KIPRJMOD"],
            "file": lt::PATH_VARIABLES_FILE,
        },
    })
}

/// Why a table cannot be written the way the person typed it (the format has no escapes, so a quote or a line break in a field has no spelling).
fn unwritable(rows: &[lt::Row]) -> Option<(usize, &'static str, String)> {
    for (i, r) in rows.iter().enumerate() {
        for (column, value, label) in [("nickname", &r.nickname, "A nickname"), ("uri", &r.uri, "A library path"), ("type", &r.lib_type, "A library format"), ("options", &r.options, "Options"), ("descr", &r.description, "A description")] {
            if value.contains(['"', '\n', '\r']) {
                return Some((i, column, format!("{label} cannot hold a double quote or a line break: KiCad's library table has no way to write one.")));
            }
        }
    }
    None
}

/// The nickname of the studio's own project library (`eda:Name`, the ids `derive_schematic` and the editors hand out): a table library of that name would be shadowed by it.
const PROJECT_LIBRARY: &str = "eda";

fn reserved_nickname(nickname: &str) -> Option<String> {
    (nickname == PROJECT_LIBRARY).then(|| format!("The nickname '{PROJECT_LIBRARY}' is the studio's own project library (the ids eda:Name): choose another."))
}

/// The studio never writes KiCad's own configuration: a board folder that is KiCad's configuration folder (or lies inside it) has no project tables of its own to edit,
/// and the file such an edit would write is KiCad's global table.
fn refuse_kicads_configuration(env: &Env, dir: &Path) -> Result<(), String> {
    let Some(config) = &env.config_dir else { return Ok(()) };
    let (board, config) = (std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()), std::fs::canonicalize(config).unwrap_or_else(|_| config.clone()));
    if board.starts_with(&config) {
        return Err(format!("{} is KiCad's own configuration folder: the studio reads KiCad's global library tables and never writes them. Open a board folder of your own to edit a project's tables.", dir.display()));
    }
    Ok(())
}

/// The project's table of `kind` as the file holds it, or an empty one when the board has none. A table that does not parse is an error, never overwritten blindly.
fn project_table(dir: &Path, kind: TableKind) -> Result<Table, String> {
    let path = project_table_path(dir, kind);
    if !path.is_file() {
        return Ok(Table::new(kind));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Table::parse(&text).map_err(|e| format!("The project's {} cannot be read ({}): {}", kind.file_name(), path.display(), e.message))
}

/// `POST /api/library_tables` `{kind, rows}`: the Project tab's OK. The rows go through KiCad's `VerifyTable` (a row with no path is dropped, a nickname must be
/// there and legal, no two alike) and the project's table is written; the answer is the view again, or `{error, row, column}` for the row that was refused.
pub(crate) fn set_table(dir: &Path, body: &[u8]) -> Value {
    set_table_with(&Env::discover(), dir, body)
}

pub(crate) fn set_table_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    if let Err(e) = refuse_kicads_configuration(env, dir) {
        return bad(e);
    }
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let kind = match kind_in(&req) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let rows = match rows_in(&req) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if let Some((row, column, message)) = unwritable(&rows) {
        return json!({ "error": message, "row": row, "column": column });
    }
    let kept = match lt::verify_rows(&rows, kind.supports_visibility()) {
        Ok(k) => k,
        Err(p) => return json!({ "error": p.message, "row": p.row, "column": p.column }),
    };
    if let Some((row, message)) = kept.iter().enumerate().find_map(|(i, r)| reserved_nickname(&r.nickname).map(|m| (i, m))) {
        let shown = rows.iter().enumerate().filter(|(_, r)| !r.uri.trim().is_empty()).map(|(k, _)| k).nth(row).unwrap_or(row);
        return json!({ "error": message, "row": shown, "column": "nickname" });
    }
    // a row whose format was left blank takes the one its path says (`GuessPluginTypeFromLibPath`)
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let kept: Vec<lt::Row> = kept
        .into_iter()
        .map(|mut r| {
            if r.lib_type.trim().is_empty() {
                r.lib_type = guess_type(kind, &vars.expand_uri(&r.uri, dir)).unwrap_or(lt::KICAD_TYPE).to_string();
            }
            r
        })
        .collect();
    let path = project_table_path(dir, kind);
    if kept.is_empty() && !path.is_file() {
        let mut v = view_with(env, dir);
        v["ok"] = json!(true);
        return v;
    }
    let mut table = Table::new(kind);
    table.rows = kept;
    if let Err(e) = lt::write_table(&path, &table) {
        return bad(e);
    }
    let mut v = view_with(env, dir);
    v["ok"] = json!(true);
    v
}

/// `POST /api/library_tables/check` `{kind, rows}`: `CheckTableRow` for each row as it stands in the grid -- where its path leads and why it cannot be read.
pub(crate) fn check_rows(dir: &Path, body: &[u8]) -> Value {
    check_rows_with(&Env::discover(), dir, body)
}

pub(crate) fn check_rows_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let kind = match kind_in(&req) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let rows = match rows_in(&req) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let results: Vec<Value> = rows
        .iter()
        .map(|r| {
            let path = vars.expand_uri(&r.uri, dir);
            let mut error = if r.uri.trim().is_empty() { None } else { lt::check_row(kind, r, &path) };
            if error.is_none() && r.is_table() && !r.uri.trim().is_empty() {
                error = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Table::parse(&t).map_err(|e| e.message)).err();
            }
            json!({ "error": error.unwrap_or_default(), "path": path.display().to_string(), "guess": guess_type(kind, &path) })
        })
        .collect();
    json!({ "results": results })
}

/// The variables a path may be written with, as `NormalizePath` takes them: KiCad's, then the project's (their values with their own variables replaced).
fn normalize_vars(env: &Env, vars: &Vars, project_dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = env.global_vars().into_iter().map(|v| (v.name, v.value)).collect();
    for (k, v) in &vars.text {
        // a variable's folder as a path has it: its own variables replaced and `..` resolved
        out.push((k.clone(), lt::absolute(Path::new(&vars.expand(v)), project_dir).display().to_string()));
    }
    out.sort();
    out
}

/// `GET /api/library_tables/normalize?path=`: a path the way a table holds it (`NormalizePath`) -- what the browse buttons of the grid put in a new row.
pub(crate) fn normalize(dir: &Path, typed: &str) -> Value {
    normalize_with(&Env::discover(), dir, typed)
}

pub(crate) fn normalize_with(env: &Env, dir: &Path, typed: &str) -> Value {
    if typed.trim().is_empty() {
        return bad("Choose a library to add.");
    }
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let path = vars.expand_uri(typed.trim(), dir);
    json!({ "uri": lt::normalize_path(&path, &normalize_vars(env, &vars, dir), Some(dir)), "path": path.display().to_string() })
}

/// `POST /api/library_tables/add` `{kind, path, create?, nickname?, descr?, table?}`: Add Existing Library, and New Library when `create` is set. The path may
/// hold variables and is relative to the board's folder. A new library is an empty `.kicad_sym` file or `.pretty` folder (`SCH_IO::CreateLibrary`, `PCB_IO::CreateLibrary`);
/// either way a row is added to the board's table (never the global one) with the path written as KiCad writes it (`NormalizePath`) and the nickname the file's name makes
/// (`LIB_ID::FixIllegalChars`).
pub(crate) fn add_library(dir: &Path, body: &[u8]) -> Value {
    add_library_with(&Env::discover(), dir, body)
}

pub(crate) fn add_library_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    if let Err(e) = refuse_kicads_configuration(env, dir) {
        return bad(e);
    }
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let kind = match kind_in(&req) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let (create, nested) = (req.get("create").and_then(Value::as_bool) == Some(true), req.get("table").and_then(Value::as_bool) == Some(true));
    let typed = text(&req, "path");
    if typed.trim().is_empty() {
        return bad(if create { "Give the new library a place and a name." } else { "Choose a library to add." });
    }
    let project_vars = crate::text_variables::project_variables(dir);
    let vars = env.vars(dir, &project_vars);
    let ext = match kind {
        TableKind::Symbol => "kicad_sym",
        TableKind::Footprint => "pretty",
        TableKind::DesignBlock => eda_kicad::design_block::LIBRARY_EXT,
    };
    let mut path = vars.expand_uri(typed.trim(), dir);
    if create && path.extension().and_then(|e| e.to_str()) != Some(ext) {
        path = PathBuf::from(format!("{}.{ext}", path.display()));
    }
    let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return bad(format!("'{}' is not a library path.", path.display())) };
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let has_ext = path.extension().and_then(|e| e.to_str()) == Some(ext);
    let lib_type = if nested { lt::NESTED_TYPE } else { lt::KICAD_TYPE };
    // a footprint library that is not a `.pretty` folder has its whole name for a nickname (`PCB_BASE_EDIT_FRAME::AddLibrary`), and a design block library that is not
    // a `.kicad_blocks` folder likewise (`PANEL_DESIGN_BLOCK_LIB_TABLE::browseLibrariesHandler`, `DESIGN_BLOCK_PANE::AddDesignBlockLibrary`)
    let nickname = match text(&req, "nickname").trim() {
        "" => lt::fix_nickname(if kind != TableKind::Symbol && !has_ext && !nested { &file_name } else { &stem }),
        given => given.to_string(),
    };
    if nickname.is_empty() {
        return bad("Library must have a nickname.");
    }
    if let Some(ch) = lt::illegal_nickname_char(&nickname) {
        return bad(format!("Illegal character '{ch}' in nickname '{nickname}'."));
    }
    if let Some(message) = reserved_nickname(&nickname) {
        return bad(message);
    }
    let mut table = match project_table(dir, kind) {
        Ok(t) => t,
        Err(e) => return bad(e),
    };
    let loaded = load_with(env.clone(), dir, kind);
    if !nested && (table.has_row(&nickname) || loaded.libs().iter().any(|l| l.nickname == nickname && l.is_available())) {
        return bad(format!("Library '{nickname}' already exists."));
    }
    if table.has_row(&nickname) {
        return bad(format!("Library '{nickname}' already exists."));
    }
    let mut created = false;
    if create {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if !parent.is_dir() {
            if parent.starts_with(dir) {
                if let Err(e) = std::fs::create_dir_all(&parent) {
                    return bad(format!("Could not create the folder '{}': {e}", parent.display()));
                }
            } else {
                return bad(format!("The folder '{}' does not exist.", parent.display()));
            }
        }
        if path.exists() {
            return bad(format!("Library {} already exists.", path.display()));
        }
        let made = match kind {
            TableKind::Symbol => std::fs::write(&path, eda_kicad::export_kicad_sym_library(&[])),
            TableKind::Footprint => std::fs::create_dir(&path),
            TableKind::DesignBlock => eda_kicad::design_block::create_library(&path).map_err(std::io::Error::other),
        };
        if let Err(e) = made {
            return bad(format!("Could not create the library '{}'.\nMake sure you have write permissions and try again. ({e})", path.display()));
        }
        created = true;
    } else if nested {
        match std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|t| Table::parse(&t).map_err(|e| e.message)) {
            Ok(t) if t.kind == kind => {}
            Ok(t) => return bad(format!("Library table '{}' has type {} but expected {}.", path.display(), t.kind.keyword(), kind.keyword())),
            Err(e) => return bad(e),
        }
    } else {
        let row = lt::Row { nickname: nickname.clone(), uri: path.display().to_string(), lib_type: lib_type.to_string(), ..Default::default() };
        if let Some(why) = lt::check_row(kind, &row, &path) {
            return bad(why);
        }
        if kind == TableKind::Symbol && !has_ext {
            return bad("Choose a .kicad_sym file.");
        }
    }
    let uri = lt::normalize_path(&path, &normalize_vars(env, &vars, dir), Some(dir));
    table.rows.push(lt::Row { nickname: nickname.clone(), uri: uri.clone(), lib_type: lib_type.to_string(), description: text(&req, "descr"), ..Default::default() });
    if let Err(e) = lt::write_table(&project_table_path(dir, kind), &table) {
        return bad(e);
    }
    let mut v = view_with(env, dir);
    v["ok"] = json!(true);
    v["added"] = json!({ "kind": kind.name(), "nickname": nickname, "uri": uri, "path": path.display().to_string(), "created": created });
    v
}

/// `POST /api/path_variables` `{variables: [{name, value}]}`: Configure Paths' OK for the board's own variables (KiCad's are read only here). The name rules are the
/// dialog's: capitals, digits and `_`, `KIPRJMOD` reserved, a name and a path both there.
pub(crate) fn set_variables(dir: &Path, body: &[u8]) -> Value {
    set_variables_with(&Env::discover(), dir, body)
}

pub(crate) fn set_variables_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    if let Err(e) = refuse_kicads_configuration(env, dir) {
        return bad(e);
    }
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let Some(list) = req.get("variables").and_then(Value::as_array) else { return bad("variables is a list") };
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (row, item) in list.iter().enumerate() {
        let (name, value) = (text(item, "name").trim().to_string(), text(item, "value").trim().to_string());
        let fail = |column: &str, message: String| json!({ "error": message, "row": row, "column": column });
        if name.is_empty() && value.is_empty() {
            continue;
        }
        if name.is_empty() {
            return fail("name", "Environment variable name cannot be empty.".into());
        }
        if value.is_empty() {
            return fail("value", "Environment variable path cannot be empty.".into());
        }
        if name == "KIPRJMOD" {
            return fail("name", "The name KIPRJMOD is reserved, and cannot be used.".into());
        }
        if !lt::is_variable_name(&name) {
            return fail("name", format!("'{name}' is not a variable name: capital letters, digits (not first) and underscores only."));
        }
        if value.contains(['"', '\n', '\r']) {
            return fail("value", "A path cannot hold a double quote or a line break.".into());
        }
        if vars.insert(name.clone(), value).is_some() {
            return fail("name", format!("Two variables are named {name}."));
        }
    }
    // The board's variables are the project's text variables (Board Setup > Text Variables is the other view of the list): the ones this dialog does not list,
    // those not named like a path variable, stay as they are.
    let mut all: BTreeMap<String, String> = crate::text_variables::project_variables(dir).into_iter().filter(|(name, _)| !lt::is_variable_name(name)).collect();
    all.extend(vars);
    if let Err(e) = crate::text_variables::set_project_variables(dir, &all) {
        return bad(e);
    }
    let mut v = view_with(env, dir);
    v["ok"] = json!(true);
    v
}

/// `GET /api/fs/list?path=&what=`: the stand-in for the native file dialog the browse buttons open in KiCad -- a page cannot see the machine's folders, the
/// server can. The folders of `path` (variables allowed, relative to the board's folder; a file stands for its folder), and the files `what` asks for:
/// `symbol` (.kicad_sym), `table` (a library table), nothing but folders for `footprint` and `design_block` (a `.pretty` or `.kicad_blocks` folder is flagged: it is the
/// library) and `folder`.
pub(crate) fn browse(dir: &Path, typed: &str, what: &str) -> Value {
    browse_with(&Env::discover(), dir, typed, what)
}

pub(crate) fn browse_with(env: &Env, dir: &Path, typed: &str, what: &str) -> Value {
    const MAX: usize = 4000;
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let mut at = if typed.trim().is_empty() { dir.to_path_buf() } else { vars.expand_uri(typed.trim(), dir) };
    if at.is_file() {
        at = at.parent().map(Path::to_path_buf).unwrap_or(at);
    }
    if !at.is_dir() {
        return bad(format!("'{}' is not a folder.", at.display()));
    }
    let read = match std::fs::read_dir(&at) {
        Ok(r) => r,
        Err(e) => return bad(format!("{}: {e}", at.display())),
    };
    let mut entries: Vec<(String, bool, bool)> = Vec::new();
    for e in read.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let is_dir = path.is_dir();
        let wanted = if is_dir {
            true
        } else {
            match what {
                "symbol" => name.ends_with(".kicad_sym"),
                "table" => lt::TableKind::ALL.iter().any(|k| name == k.file_name()),
                _ => false,
            }
        };
        if wanted {
            entries.push((name.clone(), is_dir, is_dir && (name.ends_with(".pretty") || name.ends_with(".kicad_blocks"))));
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let truncated = entries.len() > MAX;
    entries.truncate(MAX);
    json!({
        "path": at.display().to_string(),
        "parent": at.parent().map(|p| p.display().to_string()),
        "home": env.real.get("HOME"),
        "project": dir.display().to_string(),
        "entries": entries.iter().map(|(n, d, l)| json!({ "name": n, "dir": d, "library": l })).collect::<Vec<_>>(),
        "truncated": truncated,
    })
}

// ------------------------------------------------------------------------------------------------ 3D model paths

/// What a footprint's `(model "...")` path may name for the board in `dir`: the board's own path variables first, then KiCad's variables and the environment
/// (`PROJECT::TextVarResolver`, then `wxGetEnv`: [`Vars::expand`], the same way a library path is read), so a variable that gives a library its folder gives the
/// board's 3D models theirs too. `GET /api/3dmodel` asks it ([`ModelNames::get`]) before it reads a file, and may read from [`ModelNames::folders`] besides KiCad's
/// 3D model library and the board's folder: they are folders the person named in Configure Paths, here or in KiCad, never one the environment happens to define.
pub(crate) struct ModelNames {
    vars: Vars,
    pub(crate) folders: Vec<PathBuf>,
    /// The variables behind [`ModelNames::folders`], by name, with whether the board's own Configure Paths defines them (and not KiCad's): the roots the 3D model chooser lists
    /// (`DIALOG_SELECT_3DMODEL`'s folder list). A folder two variables name has the first.
    pub(crate) named: Vec<(String, PathBuf, bool)>,
}

pub(crate) fn model_names(dir: &Path) -> ModelNames {
    model_names_with(&Env::discover(), dir)
}

pub(crate) fn model_names_with(env: &Env, dir: &Path) -> ModelNames {
    let project = crate::text_variables::project_variables(dir);
    let vars = env.vars(dir, &project);
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut named: Vec<(String, PathBuf, bool)> = Vec::new();
    let board_vars = project.iter().map(|(k, v)| (k.clone(), v.clone(), true));
    for (name, value, from_board) in board_vars.chain(env.global_vars().into_iter().map(|v| (v.name, v.value, false))) {
        // an existing folder, but never the whole file system
        let folder = PathBuf::from(vars.expand(&value));
        if folder.is_absolute() && folder.parent().is_some() && folder.is_dir() && !folders.contains(&folder) {
            folders.push(folder.clone());
            named.push((name, folder, from_board));
        }
    }
    ModelNames { vars, folders, named }
}

impl ModelNames {
    /// What `${name}` stands for, expanded all the way (a variable whose value names another), or `None` when nothing defines it or it is not a name KiCad reads.
    pub(crate) fn get(&self, name: &str) -> Option<String> {
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':') {
            return None;
        }
        let spelled = format!("${{{name}}}");
        let value = self.vars.expand(&spelled);
        (value != spelled && !value.is_empty()).then_some(value)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-tables-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A machine of its own: a KiCad install (`share`) with two symbol libraries and two footprint libraries and the stock tables for them, a configuration
    /// folder, and a board folder. The environment is a map, so a test never reads the real one.
    pub(crate) struct Machine {
        pub(crate) root: PathBuf,
        pub(crate) share: PathBuf,
        pub(crate) config: PathBuf,
        pub(crate) board: PathBuf,
        pub(crate) env: BTreeMap<String, String>,
    }

    pub(crate) fn machine(tag: &str, with_global_table: bool) -> Machine {
        let root = temp(tag);
        let (share, config, board) = (root.join("KiCad").join("SharedSupport"), root.join("config").join("10.99"), root.join("board"));
        for d in [share.join("symbols"), share.join("footprints"), share.join("template"), config.clone(), board.clone()] {
            std::fs::create_dir_all(&d).unwrap();
        }
        for lib in ["Device", "Amp"] {
            std::fs::write(share.join("symbols").join(format!("{lib}.kicad_sym")), format!("(kicad_symbol_lib (symbol \"{lib}_Part\" (property \"Description\" \"from {lib}\")))")).unwrap();
        }
        for (lib, fp) in [("Package_SO", "SOIC-8"), ("Resistor_SMD", "R_0603")] {
            std::fs::create_dir_all(share.join("footprints").join(format!("{lib}.pretty"))).unwrap();
            std::fs::write(share.join("footprints").join(format!("{lib}.pretty")).join(format!("{fp}.kicad_mod")), format!("(footprint \"{fp}\" (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\")))")).unwrap();
        }
        std::fs::write(
            share.join("template").join("sym-lib-table"),
            "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Device\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Device.kicad_sym\") (options \"\") (descr \"Passives\"))\n\t(lib (name \"Amp\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Amp.kicad_sym\") (options \"\") (descr \"Amplifiers\"))\n)\n",
        )
        .unwrap();
        std::fs::write(
            share.join("template").join("fp-lib-table"),
            "(fp_lib_table\n\t(version 7)\n\t(lib (name \"Package_SO\") (type \"KiCad\") (uri \"${KICAD10_FOOTPRINT_DIR}/Package_SO.pretty\") (options \"\") (descr \"SO packages\"))\n\t(lib (name \"Resistor_SMD\") (type \"KiCad\") (uri \"${KICAD10_FOOTPRINT_DIR}/Resistor_SMD.pretty\") (options \"\") (descr \"SMD resistors\"))\n)\n",
        )
        .unwrap();
        if with_global_table {
            for kind in TableKind::ALL {
                std::fs::write(config.join(kind.file_name()), format!("({}\n\t(version 7)\n\t(lib (name \"KiCad\") (type \"Table\") (uri \"{}\") (options \"\") (descr \"KiCad Default Libraries\"))\n)\n", kind.keyword(), share.join("template").join(kind.file_name()).display())).unwrap();
            }
        }
        let env: BTreeMap<String, String> = [
            (CONFIG_DIR_ENV.to_string(), config.display().to_string()),
            ("HOME".to_string(), root.join("home").display().to_string()),
            ("KICAD10_SYMBOL_DIR".to_string(), share.join("symbols").display().to_string()),
            ("KICAD10_FOOTPRINT_DIR".to_string(), share.join("footprints").display().to_string()),
        ]
        .into();
        Machine { root, share, config, board, env }
    }

    impl Machine {
        pub(crate) fn env(&self) -> Env {
            let mut env = Env::from_environment(self.env.clone());
            // the install of this machine, whatever the process environment says about the real one
            env.stock = lt::Stock::new(self.share.join("symbols"), self.share.join("footprints"), self.share.join("3dmodels"), &self.root.join("home").join("Documents").join("KiCad").join("10.99"));
            env
        }

        pub(crate) fn set(&self, kind: Kind) -> LibSet {
            LibSet::from_libs(kind, load_with(self.env(), &self.board, kind.table_kind()).libs())
        }
    }

    fn names(set: &LibSet) -> Vec<&str> {
        set.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn the_global_table_chains_kicads_libraries_and_they_are_listed_with_their_descriptions() {
        let m = machine("global", true);
        let loaded = load_with(m.env(), &m.board, TableKind::Symbol);
        assert!(matches!(&loaded.global_origin, GlobalOrigin::Config(p) if p.ends_with("10.99/sym-lib-table")), "{:?}", loaded.global_origin);
        assert_eq!((loaded.env.major, loaded.env.version.as_deref()), (10, Some("10.99")));
        let set = m.set(Kind::Symbol);
        assert_eq!(names(&set), ["Amp", "Device"], "sorted by name");
        assert_eq!(set.find("Device").unwrap().path, m.share.join("symbols").join("Device.kicad_sym"), "${{KICAD10_SYMBOL_DIR}} is KiCad's symbol folder");
        assert_eq!((set.describe(Kind::Symbol, "Amp").as_deref(), set.scope_of(Kind::Symbol, "Amp")), (Some("Amplifiers"), Some("global")));
        let listed = set.listed(Kind::Symbol);
        assert_eq!(listed.iter().map(|l| (l.name.as_str(), l.count)).collect::<Vec<_>>(), [("Amp", None), ("Device", None)]);
        let fps = m.set(Kind::Footprint);
        assert_eq!(fps.listed(Kind::Footprint).iter().map(|l| (l.name.as_str(), l.count)).collect::<Vec<_>>(), [("Package_SO", Some(1)), ("Resistor_SMD", Some(1))]);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn without_a_global_table_the_table_kicads_install_ships_is_used_and_without_that_the_folder() {
        let m = machine("stock", false);
        let loaded = load_with(m.env(), &m.board, TableKind::Symbol);
        assert!(matches!(&loaded.global_origin, GlobalOrigin::Stock(p) if p.ends_with("template/sym-lib-table")), "{:?}", loaded.global_origin);
        assert_eq!(names(&m.set(Kind::Symbol)), ["Amp", "Device"]);
        std::fs::remove_file(m.share.join("template").join("sym-lib-table")).unwrap();
        let loaded = load_with(m.env(), &m.board, TableKind::Symbol);
        assert!(matches!(&loaded.global_origin, GlobalOrigin::Folder(_)), "{:?}", loaded.global_origin);
        let set = m.set(Kind::Symbol);
        assert_eq!(names(&set), ["Amp", "Device"], "a row per library file");
        assert_eq!(set.describe(Kind::Symbol, "Amp"), None);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn an_unreadable_global_table_is_reported_and_the_stock_one_is_used_instead() {
        let m = machine("badglobal", true);
        std::fs::write(m.config.join("sym-lib-table"), "(sym_lib_table (oops))").unwrap();
        let loaded = load_with(m.env(), &m.board, TableKind::Symbol);
        assert!(loaded.global_error.as_deref().is_some_and(|e| e.starts_with("Syntax error at line 1")), "{:?}", loaded.global_error);
        assert!(matches!(loaded.global_origin, GlobalOrigin::Stock(_)));
        assert_eq!(names(&m.set(Kind::Symbol)), ["Amp", "Device"]);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_projects_libraries_come_with_the_table_in_its_folder_and_win_by_nickname() {
        let m = machine("project", true);
        let libs = m.board.join("libs");
        std::fs::create_dir_all(libs.join("Mine.pretty")).unwrap();
        std::fs::write(libs.join("Mine.pretty").join("Pad.kicad_mod"), "(footprint \"Pad\")").unwrap();
        std::fs::write(libs.join("Mine.kicad_sym"), "(kicad_symbol_lib (symbol \"Chip\"))").unwrap();
        std::fs::write(libs.join("Device.kicad_sym"), "(kicad_symbol_lib (symbol \"Mine\"))").unwrap();
        let row = |nick: &str, uri: &str, descr: &str| lt::Row { nickname: nick.into(), uri: uri.into(), lib_type: "KiCad".into(), description: descr.into(), ..Default::default() };
        let mut sym = Table::new(TableKind::Symbol);
        sym.rows = vec![row("Mine", "${KIPRJMOD}/libs/Mine.kicad_sym", "My parts"), row("Device", "libs/Device.kicad_sym", "My Device"), row("Hidden", "${KIPRJMOD}/libs/Mine.kicad_sym", "")];
        sym.rows[2].hidden = true;
        lt::write_table(&project_table_path(&m.board, TableKind::Symbol), &sym).unwrap();
        let mut fp = Table::new(TableKind::Footprint);
        fp.rows = vec![row("Mine", "${KIPRJMOD}/libs/Mine.pretty", "")];
        lt::write_table(&project_table_path(&m.board, TableKind::Footprint), &fp).unwrap();

        let set = m.set(Kind::Symbol);
        assert_eq!(names(&set), ["Amp", "Device", "Hidden", "Mine"]);
        // (a relative path is relative to the table's folder as the file system names it: /private/var for a temporary folder on a Mac)
        assert_eq!(set.find("Device").unwrap().path.canonicalize().unwrap(), libs.join("Device.kicad_sym").canonicalize().unwrap(), "the project's Device replaced KiCad's");
        assert_eq!((set.scope_of(Kind::Symbol, "Device"), set.scope_of(Kind::Symbol, "Amp"), set.scope_of(Kind::Symbol, "Mine")), (Some("project"), Some("global"), Some("project")));
        assert_eq!(set.lib_path(Kind::Symbol, "Mine").unwrap(), libs.join("Mine.kicad_sym"));
        assert!(set.lib_path(Kind::Symbol, "Nope").is_err());
        assert_eq!(set.listed(Kind::Symbol).iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Amp", "Device", "Mine"], "a hidden library is found but not listed");
        assert!(set.find("Hidden").unwrap().hidden);
        let fps = m.set(Kind::Footprint);
        assert_eq!(names(&fps), ["Mine", "Package_SO", "Resistor_SMD"]);
        assert_eq!(fps.listed(Kind::Footprint)[0].count, Some(1));
        // the same fingerprint until something changes
        let again = m.set(Kind::Symbol);
        assert_eq!(set.fingerprint(Kind::Symbol), again.fingerprint(Kind::Symbol));
        sym.rows.pop();
        lt::write_table(&project_table_path(&m.board, TableKind::Symbol), &sym).unwrap();
        assert_ne!(set.fingerprint(Kind::Symbol), m.set(Kind::Symbol).fingerprint(Kind::Symbol), "a library fewer is another set");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_library_that_is_missing_or_switched_off_is_not_a_source_of_anything() {
        let m = machine("missing", true);
        let row = |nick: &str, uri: &str| lt::Row { nickname: nick.into(), uri: uri.into(), lib_type: "KiCad".into(), ..Default::default() };
        let mut sym = Table::new(TableKind::Symbol);
        sym.rows = vec![row("Gone", "${KIPRJMOD}/libs/Gone.kicad_sym"), lt::Row { disabled: true, ..row("Off", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym") }, lt::Row { lib_type: "Legacy".into(), ..row("Old", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym") }];
        lt::write_table(&project_table_path(&m.board, TableKind::Symbol), &sym).unwrap();
        assert_eq!(names(&m.set(Kind::Symbol)), ["Amp", "Device"]);
        let all = load_with(m.env(), &m.board, TableKind::Symbol).libs();
        let by = |n: &str| all.iter().find(|l| l.nickname == n).unwrap().clone();
        assert!(by("Gone").error.as_deref().is_some_and(|e| e.ends_with("Gone.kicad_sym' not found.")));
        assert!(by("Off").disabled && by("Off").error.is_none());
        assert!(by("Old").error.as_deref().is_some_and(|e| e.contains("KiCad-format libraries only")));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_projects_own_variables_resolve_in_its_table() {
        let m = machine("vars", true);
        let shared = m.root.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("Team.kicad_sym"), "(kicad_symbol_lib (symbol \"T\"))").unwrap();
        lt::write_project_variables(&m.board, &[("TEAMLIBS".to_string(), shared.display().to_string())].into()).unwrap();
        let mut sym = Table::new(TableKind::Symbol);
        sym.rows = vec![lt::Row { nickname: "Team".into(), uri: "${TEAMLIBS}/Team.kicad_sym".into(), lib_type: "KiCad".into(), ..Default::default() }];
        lt::write_table(&project_table_path(&m.board, TableKind::Symbol), &sym).unwrap();
        assert_eq!(m.set(Kind::Symbol).find("Team").unwrap().path, shared.join("Team.kicad_sym"));
        lt::write_project_variables(&m.board, &BTreeMap::new()).unwrap();
        assert!(m.set(Kind::Symbol).find("Team").is_none(), "without the variable the library is not found");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    fn req(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn row(nick: &str, uri: &str) -> Value {
        json!({ "nickname": nick, "uri": uri, "type": "KiCad", "options": "", "descr": "", "enabled": true, "visible": true })
    }

    #[test]
    fn the_view_shows_the_global_table_read_only_with_what_it_chains() {
        let m = machine("view", true);
        let v = view_with(&m.env(), &m.board);
        let sym = &v["tables"]["symbol"];
        assert_eq!((v["major"].clone(), v["version"].clone()), (json!(10), json!("10.99")));
        assert_eq!(sym["supports_visibility"], true);
        assert_eq!(v["tables"]["footprint"]["supports_visibility"], false);
        assert_eq!((sym["global"]["writable"].clone(), sym["project"]["writable"].clone()), (json!(false), json!(true)));
        assert_eq!(sym["global"]["origin"], "config");
        assert!(sym["global"]["note"].as_str().unwrap().contains("never writes it"));
        let kicad = &sym["global"]["rows"][0];
        assert_eq!((kicad["nickname"].clone(), kicad["type"].clone(), kicad["status"].clone()), (json!("KiCad"), json!("Table"), json!("table")));
        let chained: Vec<&str> = kicad["table"]["rows"].as_array().unwrap().iter().map(|r| r["nickname"].as_str().unwrap()).collect();
        assert_eq!(chained, ["Device", "Amp"], "the 222 libraries of a real install, here two");
        assert_eq!(kicad["table"]["rows"][0]["status"], "ok");
        assert_eq!(kicad["table"]["rows"][0]["uri"], "${KICAD10_SYMBOL_DIR}/Device.kicad_sym");
        assert!(kicad["table"]["rows"][0]["path"].as_str().unwrap().ends_with("symbols/Device.kicad_sym"));
        assert_eq!((sym["project"]["exists"].clone(), sym["project"]["rows"].clone()), (json!(false), json!([])));
        assert_eq!(sym["new_library_name"], "New_Library");
        assert!(sym["types"].as_array().unwrap().contains(&json!("KiCad")) && sym["types"].as_array().unwrap().contains(&json!("Table")));
        let g = v["variables"]["global"].as_array().unwrap();
        let sym_dir = g.iter().find(|x| x["name"] == "KICAD10_SYMBOL_DIR").unwrap();
        assert_eq!((sym_dir["predefined"].clone(), sym_dir["external"].clone()), (json!(true), json!(true)), "the test environment defines it, which wins over KiCad's default");
        assert_eq!(v["variables"]["reserved"], json!(["KIPRJMOD"]));
        assert_eq!(v["variables"]["project"], json!([]));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_project_tab_writes_the_boards_table_after_kicads_own_checks() {
        let m = machine("settable", true);
        let env = m.env();
        let table = |kind: &str, rows: Vec<Value>| req(json!({ "kind": kind, "rows": rows }));
        // OK: the row with no path is dropped, values are trimmed, the symbol table keeps its Show flag
        let ok = set_table_with(&env, &m.board, &table("symbol", vec![row(" Mine ", " ${KIPRJMOD}/libs/Mine.kicad_sym "), row("", ""), json!({ "nickname": "Hid", "uri": "/x/h.kicad_sym", "type": "KiCad", "options": "", "descr": "d", "enabled": false, "visible": false })]));
        assert_eq!(ok["ok"], true, "{ok}");
        let written = std::fs::read_to_string(m.board.join("sym-lib-table")).unwrap();
        assert_eq!(written, "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/libs/Mine.kicad_sym\") (options \"\") (descr \"\"))\n\t(lib (name \"Hid\") (type \"KiCad\") (uri \"/x/h.kicad_sym\") (options \"\") (descr \"d\") (disabled) (hidden))\n)\n");
        let rows = ok["tables"]["symbol"]["project"]["rows"].as_array().unwrap();
        assert_eq!((rows[0]["status"].clone(), rows[0]["error"].as_str().unwrap().starts_with("Library '")), (json!("error"), true), "Mine's file is not there yet");
        assert_eq!((rows[1]["enabled"].clone(), rows[1]["visible"].clone()), (json!(false), json!(false)));
        // a footprint table keeps no hidden flag
        let ok = set_table_with(&env, &m.board, &table("footprint", vec![json!({ "nickname": "F", "uri": "${KIPRJMOD}/f.pretty", "type": "KiCad", "options": "", "descr": "", "enabled": true, "visible": false })]));
        assert_eq!(ok["tables"]["footprint"]["project"]["rows"][0]["visible"], true);
        assert!(!std::fs::read_to_string(m.board.join("fp-lib-table")).unwrap().contains("(hidden)"));
        // refused: nothing is written
        let before = std::fs::read_to_string(m.board.join("sym-lib-table")).unwrap();
        for (rows, message, at) in [
            (vec![row("A", "/a"), row("", "/b")], "Library must have a nickname.", (1, "nickname")),
            (vec![row("A:B", "/a")], "Illegal character ':' in nickname 'A:B'.", (0, "nickname")),
            (vec![row("A", "/a"), row("B", "/b"), row("A", "/c")], "Multiple libraries cannot share the same nickname ('A').", (2, "nickname")),
            (vec![json!({ "nickname": "Q", "uri": "/a", "type": "KiCad", "options": "", "descr": "say \"hi\"", "enabled": true, "visible": true })], "A description cannot hold a double quote or a line break: KiCad's library table has no way to write one.", (0, "descr")),
        ] {
            let r = set_table_with(&env, &m.board, &table("symbol", rows));
            assert_eq!(r["error"], message, "{r}");
            assert_eq!((r["row"].clone(), r["column"].clone()), (json!(at.0), json!(at.1)));
            assert_eq!(std::fs::read_to_string(m.board.join("sym-lib-table")).unwrap(), before, "a refused table leaves the file as it was");
        }
        // an empty table with no file writes none; with a file it writes an empty one
        let none = machine("settable-empty", true);
        assert_eq!(set_table_with(&none.env(), &none.board, &table("symbol", vec![]))["ok"], true);
        assert!(!none.board.join("sym-lib-table").exists());
        assert_eq!(set_table_with(&env, &m.board, &table("symbol", vec![]))["ok"], true);
        assert_eq!(std::fs::read_to_string(m.board.join("sym-lib-table")).unwrap(), "(sym_lib_table\n\t(version 7)\n)\n");
        // a blank format takes the one the path says
        let r = set_table_with(&env, &m.board, &table("footprint", vec![json!({ "nickname": "G", "uri": "${KIPRJMOD}/g.pretty", "type": "", "options": "", "descr": "", "enabled": true, "visible": true })]));
        assert_eq!(r["tables"]["footprint"]["project"]["rows"][0]["type"], "KiCad");
        assert!(set_table_with(&env, &m.board, br#"{"kind":"nope","rows":[]}"#)["error"].is_string());
        assert!(set_table_with(&env, &m.board, b"{").get("error").is_some());
        // the global tables were never touched
        assert!(std::fs::read_to_string(m.config.join("sym-lib-table")).unwrap().contains("KiCad Default Libraries"));
        let _ = std::fs::remove_dir_all(&m.root);
        let _ = std::fs::remove_dir_all(&none.root);
    }

    #[test]
    fn new_library_makes_an_empty_file_or_folder_and_a_row_in_the_project_table() {
        let m = machine("newlib", true);
        let env = m.env();
        // a symbol library in the project: the file, the row, the nickname from the file's name, the path written with ${KIPRJMOD}
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": "libs/My Parts" })));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["added"]["nickname"], "My Parts");
        assert_eq!(r["added"]["uri"], "${KIPRJMOD}/libs/My Parts.kicad_sym");
        let file = m.board.join("libs").join("My Parts.kicad_sym");
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("(kicad_symbol_lib (version "), "an empty library file");
        assert!(eda_kicad::scan_symbols(&std::fs::read_to_string(&file).unwrap()).is_empty(), "no symbol in it yet");
        assert!(std::fs::read_to_string(m.board.join("sym-lib-table")).unwrap().contains("(name \"My Parts\") (type \"KiCad\") (uri \"${KIPRJMOD}/libs/My Parts.kicad_sym\")"));
        assert!(m.set(Kind::Symbol).find("My Parts").is_some(), "the new library is a library of the board's set at once");
        // and the same name again is refused, as is a nickname a global library has
        for (path, nickname) in [("libs/My Parts", ""), ("libs/Other", "Device")] {
            let again = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": path, "nickname": nickname })));
            assert!(again["error"].as_str().unwrap().contains("already exists"), "{again}");
        }
        // a footprint library is a .pretty folder
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "footprint", "create": true, "path": "${KIPRJMOD}/fp/Chips", "descr": "My chips" })));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["added"]["uri"], "${KIPRJMOD}/fp/Chips.pretty");
        assert!(m.board.join("fp").join("Chips.pretty").is_dir());
        assert_eq!(m.set(Kind::Footprint).describe(Kind::Footprint, "Chips").as_deref(), Some("My chips"));
        // a folder that is not there is made inside the project, not elsewhere
        let elsewhere = m.root.join("elsewhere").join("deep").join("X");
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": elsewhere.display().to_string() })));
        assert!(r["error"].as_str().unwrap().contains("does not exist"), "{r}");
        assert!(!m.root.join("elsewhere").exists());
        // a place that exists, outside the project, takes the file and the row says the full path
        let outside = m.root.join("shared");
        std::fs::create_dir_all(&outside).unwrap();
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": outside.join("Team.kicad_sym").display().to_string() })));
        assert_eq!(r["added"]["uri"], outside.join("Team.kicad_sym").display().to_string());
        // under a variable's folder the path is written with the variable
        let under = m.share.join("symbols");
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "path": under.join("Device.kicad_sym").display().to_string(), "nickname": "DeviceCopy" })));
        assert_eq!(r["added"]["uri"], "${KICAD10_SYMBOL_DIR}/Device.kicad_sym");
        // the studio's own library name is not for a table library
        let reserved = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": "libs/eda" })));
        assert!(reserved["error"].as_str().unwrap().contains("studio's own project library"), "{reserved}");
        assert!(!m.board.join("libs").join("eda.kicad_sym").exists(), "nothing was made");
        let table = set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": [row("A", "/a"), row("eda", "/b")] })));
        assert_eq!((table["row"].clone(), table["column"].clone()), (json!(1), json!("nickname")), "{table}");
        // a bad nickname, an empty path
        assert!(add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": "libs/x", "nickname": "a:b" })))["error"].as_str().unwrap().contains("Illegal character ':'"));
        assert!(add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "create": true, "path": "  " })))["error"].is_string());
        assert!(!m.root.join("config").join("10.99").join("fp-lib-table").to_string_lossy().contains("Chips"));
        assert!(!std::fs::read_to_string(m.config.join("fp-lib-table")).unwrap().contains("Chips"), "the global table is never written");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_design_block_table_is_the_third_kind_a_new_library_is_a_kicad_blocks_folder_and_the_global_table_is_kicads() {
        let m = machine("dblib", true);
        let env = m.env();
        // the view lists three tables; the design block one has no Show column and chains KiCad's (here a table KiCad's install does not ship)
        let v = view_with(&env, &m.board);
        let t = &v["tables"]["design_block"];
        assert_eq!((t["file"].clone(), t["supports_visibility"].clone(), t["types"].clone()), (json!("design-block-lib-table"), json!(false), json!(["KiCad", "Table"])));
        assert!(v["tables"]["symbol"]["supports_visibility"].as_bool().unwrap() && !v["tables"]["footprint"]["supports_visibility"].as_bool().unwrap());
        assert_eq!(t["global"]["rows"][0]["status"], "error", "the stock table of this machine's KiCad is not there: {}", t["global"]["rows"][0]);
        assert!(t["global"]["rows"][0]["error"].as_str().unwrap().contains("does not exist"));
        // New Library: the folder, the row, the path written with ${KIPRJMOD}, the nickname from the folder's name
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Power", "descr": "Regulators" })));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["added"]["nickname"].clone(), r["added"]["uri"].clone()), (json!("Power"), json!("${KIPRJMOD}/blocks/Power.kicad_blocks")));
        assert!(m.board.join("blocks").join("Power.kicad_blocks").is_dir());
        assert!(std::fs::read_to_string(m.board.join("design-block-lib-table")).unwrap().starts_with("(design_block_lib_table\n\t(version 7)\n\t(lib (name \"Power\") (type \"KiCad\") (uri \"${KIPRJMOD}/blocks/Power.kicad_blocks\")"));
        let libs = load_with(env.clone(), &m.board, TableKind::DesignBlock).libs();
        let power = libs.iter().find(|l| l.nickname == "Power").expect("the new library is in the board's set at once");
        assert_eq!((power.path.clone(), power.is_listed(), power.description.as_str()), (m.board.join("blocks").join("Power.kicad_blocks"), true, "Regulators"));
        assert_eq!(eda_kicad::design_block::enumerate(&power.path).unwrap(), Vec::<String>::new());
        // the same name again, and a library that is a folder already, are refused; so is a symbol file offered as a block library
        let again = add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Power" })));
        assert!(again["error"].as_str().unwrap().contains("already exists"), "{again}");
        std::fs::write(m.root.join("shared.kicad_sym"), "(kicad_symbol_lib)").unwrap();
        assert!(add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "path": m.root.join("shared.kicad_sym").display().to_string() })))["error"].as_str().unwrap().contains("does not exist"));
        // Add Existing: a folder, nicknamed by its name less the extension; a folder with another name keeps the whole name
        let shared = m.root.join("shared");
        std::fs::create_dir_all(shared.join("Team.kicad_blocks")).unwrap();
        std::fs::create_dir_all(shared.join("loose")).unwrap();
        assert_eq!(add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "path": shared.join("Team.kicad_blocks").display().to_string() })))["added"]["nickname"], "Team");
        assert_eq!(add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "path": shared.join("loose").display().to_string() })))["added"]["nickname"], "loose");
        // the dialog's OK: rows with no Show flag, a blank format taking the one the path says
        let rows = vec![json!({ "nickname": "Power", "uri": "${KIPRJMOD}/blocks/Power.kicad_blocks", "type": "", "options": "", "descr": "", "enabled": true, "visible": false })];
        let r = set_table_with(&env, &m.board, &req(json!({ "kind": "design_block", "rows": rows })));
        assert_eq!(r["ok"], true, "{r}");
        let saved = Table::parse(&std::fs::read_to_string(m.board.join("design-block-lib-table")).unwrap()).unwrap();
        assert_eq!((saved.kind, saved.rows.len(), saved.rows[0].lib_type.as_str(), saved.rows[0].hidden), (TableKind::DesignBlock, 1, "KiCad", false), "a design block table keeps no hidden flag");
        // the browse listing offers a library folder as the library, and the table files of all three kinds
        std::fs::write(shared.join("design-block-lib-table"), "(design_block_lib_table (version 7))").unwrap();
        let listing = browse_with(&env, &m.board, &shared.display().to_string(), "design_block");
        let team = listing["entries"].as_array().unwrap().iter().find(|e| e["name"] == "Team.kicad_blocks").unwrap().clone();
        assert_eq!((team["dir"].clone(), team["library"].clone()), (json!(true), json!(true)));
        let tables = browse_with(&env, &m.board, &shared.display().to_string(), "table");
        assert!(tables["entries"].as_array().unwrap().iter().any(|e| e["name"] == "design-block-lib-table"));
        // the global tables are never written
        assert!(std::fs::read_to_string(m.config.join("design-block-lib-table")).unwrap().contains("KiCad Default Libraries"));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn add_existing_takes_a_file_a_folder_or_a_whole_table() {
        let m = machine("addlib", true);
        let env = m.env();
        let shared = m.root.join("shared");
        std::fs::create_dir_all(shared.join("Snap.pretty")).unwrap();
        std::fs::create_dir_all(shared.join("plain_folder")).unwrap();
        std::fs::write(shared.join("Snap.pretty").join("A.kicad_mod"), "(footprint \"A\")").unwrap();
        std::fs::write(shared.join("Parts.kicad_sym"), "(kicad_symbol_lib (symbol \"P\"))").unwrap();
        std::fs::write(shared.join("notes.txt"), "x").unwrap();
        std::fs::write(shared.join("more-sym-lib-table"), "(sym_lib_table (version 7) (lib (name \"Z\") (type \"KiCad\") (uri \"/z.kicad_sym\")))").unwrap();
        let add = |v: Value| add_library_with(&env, &m.board, &req(v));
        assert_eq!(add(json!({ "kind": "symbol", "path": shared.join("Parts.kicad_sym").display().to_string() }))["added"]["nickname"], "Parts");
        assert_eq!(add(json!({ "kind": "footprint", "path": shared.join("Snap.pretty").display().to_string() }))["added"]["nickname"], "Snap");
        // a footprint folder that is not a .pretty keeps its whole name for a nickname
        assert_eq!(add(json!({ "kind": "footprint", "path": shared.join("plain_folder").display().to_string() }))["added"]["nickname"], "plain_folder");
        // not there, not the right shape
        assert!(add(json!({ "kind": "symbol", "path": shared.join("Missing.kicad_sym").display().to_string() }))["error"].as_str().unwrap().contains("not found"));
        assert!(add(json!({ "kind": "symbol", "path": shared.join("notes.txt").display().to_string() }))["error"].as_str().unwrap().contains(".kicad_sym"));
        assert!(add(json!({ "kind": "footprint", "path": shared.join("Parts.kicad_sym").display().to_string() }))["error"].is_string());
        // a table is chained in as a row of type Table
        let r = add(json!({ "kind": "symbol", "table": true, "path": shared.join("more-sym-lib-table").display().to_string(), "nickname": "More" }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["tables"]["symbol"]["project"]["rows"][1]["type"], "Table");
        assert_eq!(r["tables"]["symbol"]["project"]["rows"][1]["table"]["rows"][0]["nickname"], "Z");
        assert!(add(json!({ "kind": "footprint", "table": true, "path": shared.join("more-sym-lib-table").display().to_string(), "nickname": "Wrong" }))["error"].as_str().unwrap().contains("expected fp_lib_table"));
        let names: Vec<String> = m.set(Kind::Symbol).entries().iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"Parts".to_string()), "{names:?}");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn rows_are_checked_as_they_stand_in_the_grid() {
        let m = machine("check", true);
        let r = check_rows_with(&m.env(), &m.board, &req(json!({ "kind": "symbol", "rows": [
            row("A", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym"),
            row("B", "${KICAD10_SYMBOL_DIR}/Nope.kicad_sym"),
            { "nickname": "T", "uri": m.share.join("template").join("sym-lib-table").display().to_string(), "type": "Table" },
            { "nickname": "T2", "uri": "/no/such", "type": "Table" },
            { "nickname": "L", "uri": "/x", "type": "Legacy" },
            row("E", ""),
        ] })));
        let res = r["results"].as_array().unwrap();
        assert_eq!(res[0]["error"], "");
        assert!(res[1]["error"].as_str().unwrap().ends_with("Nope.kicad_sym' not found."));
        assert_eq!(res[2]["error"], "");
        assert!(res[3]["error"].as_str().unwrap().starts_with("Nested table '/no/such' not found"));
        assert!(res[4]["error"].as_str().unwrap().contains("KiCad-format libraries only"));
        assert_eq!(res[5]["error"], "", "a row with no path is dropped on OK, not an error now");
        assert_eq!(res[2]["guess"], "Table");
        assert_eq!(res[0]["guess"], "KiCad");
        assert!(res[0]["path"].as_str().unwrap().ends_with("symbols/Device.kicad_sym"));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn kicads_own_configuration_folder_is_never_written() {
        let m = machine("neverwrite", true);
        let env = m.env();
        let before: Vec<String> = ["sym-lib-table", "fp-lib-table"].iter().map(|f| std::fs::read_to_string(m.config.join(f)).unwrap()).collect();
        // a "board" that is KiCad's configuration folder, or a folder in it: every route that writes refuses
        for dir in [m.config.clone(), m.config.join("sub")] {
            std::fs::create_dir_all(&dir).unwrap();
            let rows = req(json!({ "kind": "symbol", "rows": [row("Mine", "/x/Mine.kicad_sym")] }));
            assert!(set_table_with(&env, &dir, &rows)["error"].as_str().unwrap().contains("KiCad's own configuration folder"));
            assert!(add_library_with(&env, &dir, &req(json!({ "kind": "symbol", "create": true, "path": "New" })))["error"].is_string());
            assert!(set_variables_with(&env, &dir, &req(json!({ "variables": [{ "name": "A", "value": "/x" }] })))["error"].is_string());
        }
        let after: Vec<String> = ["sym-lib-table", "fp-lib-table"].iter().map(|f| std::fs::read_to_string(m.config.join(f)).unwrap()).collect();
        assert_eq!(before, after, "the global tables are as KiCad left them");
        assert!(!m.config.join("sub").join("sym-lib-table").exists() && !m.config.join(lt::PATH_VARIABLES_FILE).exists());
        // a board folder of its own is fine
        assert_eq!(set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": [row("Mine", "/x/Mine.kicad_sym")] })))["ok"], true);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_board_variable_names_a_3d_models_folder_and_the_environment_does_not() {
        let mut m = machine("models", true);
        let team = m.root.join("team");
        let other = m.root.join("elsewhere");
        for d in [&team, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        // KiCad's Configure Paths holds one variable of the person's, and the process environment another
        std::fs::write(m.config.join("kicad_common.json"), format!("{{ \"environment\": {{ \"vars\": {{ \"MY_KICAD_3D\": \"{}\" }} }} }}", team.join("kicad").display())).unwrap();
        std::fs::create_dir_all(team.join("kicad")).unwrap();
        m.env.insert("FROM_THE_SHELL".into(), other.display().to_string());
        let env = m.env();
        let set = set_variables_with(&env, &m.board, &req(json!({ "variables": [{ "name": "TEAM", "value": team.display().to_string() }, { "name": "SUB", "value": "${TEAM}/parts" }, { "name": "GONE", "value": "/no/such/folder" }, { "name": "ROOT", "value": "/" }] })));
        assert_eq!(set["ok"], true, "{set}");
        std::fs::create_dir_all(team.join("parts")).unwrap();
        let names = model_names_with(&env, &m.board);
        let s = |v: Option<String>| v.map(|v| PathBuf::from(v).display().to_string());
        assert_eq!(s(names.get("TEAM")), Some(team.display().to_string()), "a board variable");
        assert_eq!(s(names.get("SUB")), Some(format!("{}/parts", team.display())), "its value names another");
        assert_eq!(s(names.get("MY_KICAD_3D")), Some(team.join("kicad").display().to_string()), "a variable of KiCad's Configure Paths");
        assert_eq!(s(names.get("KIPRJMOD")), Some(m.board.display().to_string()));
        assert_eq!(s(names.get("FROM_THE_SHELL")), Some(other.display().to_string()), "the environment is read like any variable");
        assert_eq!(names.get("NOBODY_DEFINES_THIS"), None);
        assert_eq!(names.get("not a name"), None);
        assert_eq!(names.get(""), None);
        // the folders the person named, never the environment's, a missing folder or the whole file system
        assert!(names.folders.contains(&team) && names.folders.contains(&team.join("kicad")) && names.folders.contains(&team.join("parts")), "{:?}", names.folders);
        assert!(!names.folders.contains(&other), "{:?}", names.folders);
        assert!(names.folders.iter().all(|f| f.is_dir() && f.parent().is_some()), "{:?}", names.folders);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_path_is_written_the_way_a_table_holds_it() {
        let m = machine("normalize", true);
        let env = m.env();
        let under = m.share.join("symbols").join("Device.kicad_sym");
        assert_eq!(normalize_with(&env, &m.board, &under.display().to_string())["uri"], "${KICAD10_SYMBOL_DIR}/Device.kicad_sym");
        assert_eq!(normalize_with(&env, &m.board, &m.board.join("libs").join("X.pretty").display().to_string())["uri"], "${KIPRJMOD}/libs/X.pretty");
        assert_eq!(normalize_with(&env, &m.board, "libs/Y.kicad_sym")["uri"], "${KIPRJMOD}/libs/Y.kicad_sym", "a relative path is the board's");
        assert_eq!(normalize_with(&env, &m.board, "/somewhere/else/Z.kicad_sym")["uri"], "/somewhere/else/Z.kicad_sym");
        assert!(normalize_with(&env, &m.board, "  ")["error"].is_string());
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_path_under_a_variable_with_dots_in_its_value_is_written_with_the_variable() {
        let m = machine("dotted", true);
        let env = m.env();
        let libs = m.root.join("mylibs");
        std::fs::create_dir_all(&libs).unwrap();
        lt::write_project_variables(&m.board, &[("TEAM".to_string(), "${KIPRJMOD}/../mylibs".to_string())].into()).unwrap();
        assert_eq!(normalize_with(&env, &m.board, &libs.join("X.pretty").display().to_string())["uri"], "${TEAM}/X.pretty");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_boards_own_path_variables_follow_configure_paths_rules() {
        let m = machine("pathvars", true);
        let env = m.env();
        let set = |v: Value| set_variables_with(&env, &m.board, &req(json!({ "variables": v })));
        let ok = set(json!([{ "name": "TEAM_LIBS", "value": " /srv/libs " }, { "name": "", "value": "" }, { "name": "KICAD10_TEMPLATE_DIR", "value": "/mine/t" }]));
        assert_eq!(ok["ok"], true, "{ok}");
        assert_eq!(ok["variables"]["project"], json!([{ "name": "KICAD10_TEMPLATE_DIR", "value": "/mine/t" }, { "name": "TEAM_LIBS", "value": "/srv/libs" }]));
        assert_eq!(lt::read_project_variables(&m.board).get("TEAM_LIBS").map(String::as_str), Some("/srv/libs"));
        for (v, message) in [
            (json!([{ "name": "KIPRJMOD", "value": "/x" }]), "The name KIPRJMOD is reserved, and cannot be used."),
            (json!([{ "name": "", "value": "/x" }]), "Environment variable name cannot be empty."),
            (json!([{ "name": "A", "value": "" }]), "Environment variable path cannot be empty."),
            (json!([{ "name": "a b", "value": "/x" }]), "'a b' is not a variable name: capital letters, digits (not first) and underscores only."),
            (json!([{ "name": "A", "value": "/x" }, { "name": "A", "value": "/y" }]), "Two variables are named A."),
        ] {
            assert_eq!(set(v)["error"], message);
        }
        assert_eq!(lt::read_project_variables(&m.board).len(), 2, "a refused edit changes nothing");
        // a variable the tables use and nothing defines is listed with what the environment says
        let mut t = Table::new(TableKind::Footprint);
        t.rows.push(lt::Row { nickname: "Old".into(), uri: "${KICAD6_FOOTPRINT_DIR}/Old.pretty".into(), lib_type: "KiCad".into(), ..Default::default() });
        t.rows.push(lt::Row { nickname: "New".into(), uri: "${SOMETHING_ELSE}/New.pretty".into(), lib_type: "KiCad".into(), ..Default::default() });
        lt::write_table(&project_table_path(&m.board, TableKind::Footprint), &t).unwrap();
        let v = view_with(&env, &m.board);
        let undefined: Vec<&str> = v["variables"]["undefined"].as_array().unwrap().iter().map(|u| u["name"].as_str().unwrap()).collect();
        assert_eq!(undefined, ["KICAD6_FOOTPRINT_DIR", "SOMETHING_ELSE"]);
        // none left removes the file
        assert_eq!(set(json!([]))["ok"], true);
        assert!(!m.board.join(lt::PATH_VARIABLES_FILE).exists());
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn the_browse_dialog_lists_folders_and_the_files_asked_for() {
        let m = machine("browse", true);
        let env = m.env();
        let dir = m.root.join("shared");
        std::fs::create_dir_all(dir.join("B.pretty")).unwrap();
        std::fs::create_dir_all(dir.join("a_folder")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("Z.kicad_sym"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        std::fs::write(dir.join("fp-lib-table"), "").unwrap();
        let names = |what: &str| -> Vec<(String, bool, bool)> {
            let v = browse_with(&env, &m.board, &dir.display().to_string(), what);
            v["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap().to_string(), e["dir"].as_bool().unwrap(), e["library"].as_bool().unwrap())).collect()
        };
        assert_eq!(names("symbol"), [("a_folder".into(), true, false), ("B.pretty".into(), true, true), ("Z.kicad_sym".into(), false, false)], "folders first, then files, not the hidden or the others");
        assert_eq!(names("footprint"), [("a_folder".into(), true, false), ("B.pretty".into(), true, true)]);
        assert_eq!(names("table").len(), 3);
        let v = browse_with(&env, &m.board, &dir.join("Z.kicad_sym").display().to_string(), "symbol");
        assert_eq!(v["path"], dir.display().to_string(), "a file stands for its folder");
        assert_eq!(v["parent"], m.root.display().to_string());
        let here = browse_with(&env, &m.board, "", "folder");
        assert_eq!(here["path"], m.board.display().to_string(), "the board's folder by default");
        let variable = browse_with(&env, &m.board, "${KICAD10_SYMBOL_DIR}", "symbol");
        assert_eq!(variable["path"], m.share.join("symbols").display().to_string());
        assert!(browse_with(&env, &m.board, "/no/such/folder", "folder")["error"].is_string());
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_library_of_the_boards_table_lists_searches_opens_and_places_like_an_installed_one() {
        use crate::library_index::{items_in_for_tests, libraries_json_for_tests};
        let m = machine("e2e", true);
        let env = m.env();
        // a library of the project with one symbol and one footprint, added through the dialog's route
        std::fs::create_dir_all(m.board.join("libs").join("Mine.pretty")).unwrap();
        std::fs::write(
            m.board.join("libs").join("Mine.kicad_sym"),
            r#"(kicad_symbol_lib (version 20251024) (symbol "Chip9000" (property "Reference" "U") (property "Value" "Chip9000") (property "Footprint" "Mine:Pad2") (property "Description" "A made-up chip") (property "ki_keywords" "zeta widget")
                (symbol "Chip9000_1_1" (pin input line (at 0 0 0) (length 1) (name "A") (number "1")) (pin output line (at 0 2.54 0) (length 1) (name "B") (number "2")))))"#,
        )
        .unwrap();
        std::fs::write(m.board.join("libs").join("Mine.pretty").join("Pad2.kicad_mod"), r#"(footprint "Pad2" (descr "two pads") (tags "widget") (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu")) (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu")))"#).unwrap();
        for (kind, path) in [("symbol", "${KIPRJMOD}/libs/Mine.kicad_sym"), ("footprint", "${KIPRJMOD}/libs/Mine.pretty")] {
            let r = add_library_with(&env, &m.board, &req(json!({ "kind": kind, "path": path, "descr": "My parts" })));
            assert_eq!(r["ok"], true, "{r}");
        }
        let (syms, fps) = (m.set(Kind::Symbol), m.set(Kind::Footprint));
        // the index lists it with its description and table, its items by name
        let idx = libraries_json_for_tests(Kind::Symbol, &syms);
        let mine = idx["libraries"].as_array().unwrap().iter().find(|l| l["name"] == "Mine").unwrap().clone();
        assert_eq!((mine["description"].clone(), mine["scope"].clone()), (json!("My parts"), json!("project")));
        assert_eq!(items_in_for_tests(Kind::Symbol, &syms, "Mine").unwrap(), ["Chip9000"]);
        assert_eq!(items_in_for_tests(Kind::Footprint, &fps, "Mine").unwrap(), ["Pad2"]);
        // the chooser's search finds it, by name, keyword and description
        crate::library_search::index_all(Kind::Symbol, &syms);
        let found = |q: &str| -> Vec<String> {
            let r = crate::library_search::search_in(Kind::Symbol, &syms, q, &crate::library_search::Filter::default(), 50);
            r["libraries"].as_array().unwrap().iter().flat_map(|l| l["items"].as_array().unwrap().iter().map(move |i| format!("{}:{}", l["name"].as_str().unwrap(), i["name"].as_str().unwrap()))).collect()
        };
        assert_eq!(found("chip9000"), ["Mine:Chip9000"]);
        assert_eq!(found("zeta"), ["Mine:Chip9000"], "a keyword");
        assert_eq!(found("made-up"), ["Mine:Chip9000"], "a word of the description");
        let d = crate::library_search::details_in(Kind::Symbol, &syms, "Mine:Chip9000");
        assert_eq!((d["footprint"].clone(), d["pins"].clone(), d["symbol"]["lib_id"].clone()), (json!("Mine:Pad2"), json!(2), json!("Mine:Chip9000")));
        let fp = crate::library_search::details_in(Kind::Footprint, &fps, "Mine:Pad2");
        assert_eq!((fp["numbered_pads"].clone(), fp["footprint"]["name"].clone()), (json!(2), json!("Mine:Pad2")));
        // placing it keeps its definition with the schematic, with the library's Footprint field
        let design: eda_model::ir::Design = serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
        let cmd = eda_ops::Cmd::AddSymbol { id: "U1".into(), lib_id: "Mine:Chip9000".into(), at: eda_model::ir::Point { x: 1000, y: 1000 }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 };
        let rewritten = crate::library_place::rewrite_for_tests(&cmd, &design, &eda_model::ConstraintModel::default(), &syms, &fps).expect("placed from the table library");
        let eda_ops::Cmd::Batch { cmds } = rewritten else { panic!("{rewritten:?}") };
        assert!(matches!(&cmds[0], eda_ops::Cmd::EmbedLibSymbol { symbol } if symbol.lib_id == "Mine:Chip9000" && symbol.pins.len() == 2));
        assert!(matches!(&cmds[1], eda_ops::Cmd::AddSymbol { value, footprint, .. } if value == "Chip9000" && footprint == "Mine:Pad2"));
        let place = eda_ops::Cmd::PlaceFootprint { footprint: "Mine:Pad2".into(), at: eda_model::ir::Point { x: 0, y: 0 }, reference: String::new(), value: String::new(), definition: None };
        let Some(eda_ops::Cmd::PlaceFootprint { definition: Some(def), .. }) = crate::library_place::rewrite_for_tests(&place, &design, &eda_model::ConstraintModel::default(), &syms, &fps) else { panic!("not rewritten") };
        assert_eq!(def.pads.len(), 2);
        // and an intent part naming the footprint finds its pads
        let mut model = eda_model::ConstraintModel::default();
        model.parts.push(eda_model::Part { reference: "U1".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some("Mine:Pad2".into()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None });
        let warnings = eda_kicad::resolve_library_footprints_with(&mut model, |id| {
            let (lib, item) = id.split_once(':')?;
            fps.lib_path(Kind::Footprint, lib).ok().map(|d| d.join(format!("{item}.kicad_mod")))
        });
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(model.footprints.len(), 1);
        assert_eq!(model.footprints[0].pads.len(), 2);
        let _ = std::fs::remove_dir_all(&m.root);
    }
}
