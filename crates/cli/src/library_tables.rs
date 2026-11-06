//! The library tables of a board, as the studio reads them: which libraries exist, and where.
//!
//! A `Lib:Name` the studio is asked for -- in a tree, a chooser, a placement, a lookup -- is found the way KiCad finds it: the library nicknamed `Lib` in
//! the library tables, and the item `Name` in that library's file or folder. The tables are the global one in KiCad's own configuration folder
//! (`~/Library/Preferences/kicad/<version>/sym-lib-table`, which chains the 222 symbol and 155 footprint libraries of KiCad's install) and the board's own
//! (`sym-lib-table`, `fp-lib-table` in the board's folder, the studio's project folder). The pure half -- the file format, the path variables, how the
//! tables are put together -- is `eda_kicad::library_table`; this module is what needs the machine: where KiCad's configuration is, which variables it
//! defines, the project's files.
//!
//! The global tables are the user's, in KiCad's configuration folder: the Global Libraries tab of Manage Libraries edits them and Reset Libraries writes them
//! anew, as KiCad's dialogs do (`PANEL_SYM_LIB_TABLE::TransferDataFromWindow`, `onReset`), and Configure Paths writes KiCad's path variables into its
//! `kicad_common.json` (`DIALOG_CONFIGURE_PATHS`). Those three files, and a table one of the tables chains, are all the studio writes there; a board folder that
//! is KiCad's configuration folder is refused, and so is a library made inside it.
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

/// The table KiCad installs (`LIBRARY_MANAGER::StockTablePath`: `PATHS::GetStockTemplatesPath()`, `template/sym-lib-table`), which a first run's global table chains
/// and Reset Libraries chains again. A checkout of KiCad's library repositories (`KICAD_PATH` with no `template/` folder) has it beside the libraries instead
/// (`symbols/sym-lib-table`, `footprints/fp-lib-table`: the files KiCad's install copies into `template/`), and that one stands in for it.
pub(crate) fn stock_table(env: &Env, kind: TableKind) -> PathBuf {
    let installed = env.stock.templates.join(kind.file_name());
    if installed.is_file() {
        return installed;
    }
    let beside = match kind {
        TableKind::Symbol => env.stock.symbols.join(kind.file_name()),
        TableKind::Footprint => env.stock.footprints.join(kind.file_name()),
        TableKind::DesignBlock => env.stock.design_blocks.join(kind.file_name()),
    };
    if beside.is_file() {
        beside
    } else {
        installed
    }
}

/// The global table of this kind in KiCad's configuration folder (`LIBRARY_MANAGER::DefaultGlobalTablePath`: `PATHS::GetUserSettingsPath()` and the table's file
/// name), whether or not KiCad has written it yet; `None` when this machine has no KiCad configuration folder.
pub(crate) fn global_table_path(env: &Env, kind: TableKind) -> Option<PathBuf> {
    env.config_dir.as_ref().map(|d| d.join(kind.file_name()))
}

/// The libraries of KiCad's library folder as a table of their own, for a machine whose KiCad has no table to read: one row per `.kicad_sym` file or
/// `.kicad_symdir` folder library (KiCad 10's kicad-symbols ships only these) or `.pretty` folder, named by its file name. A name both a file and a folder
/// give is the file's row, as `eda_kicad::find_symbol_library_file` reads the file.
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
                        TableKind::Symbol => eda_kicad::symbol_library_name(&path).is_some(),
                        TableKind::Footprint => ext == "pretty" && path.is_dir(),
                        TableKind::DesignBlock => ext == eda_kicad::design_block::LIBRARY_EXT && path.is_dir(),
                    };
                    wanted.then(|| lt::Row { nickname: name, uri: path.display().to_string(), lib_type: lt::KICAD_TYPE.into(), ..Default::default() })
                })
                .collect()
        })
        .unwrap_or_default();
    // `Device.kicad_sym` sorts before `Device.kicad_symdir`
    rows.sort_by(|a, b| a.nickname.cmp(&b.nickname).then_with(|| a.uri.cmp(&b.uri)));
    rows.dedup_by(|b, a| a.nickname == b.nickname);
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
        let stock = stock_table(&env, kind);
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
            .filter_map(|l| {
                // a library of another format is read as the KiCad copy kicad-cli makes of it (`library_convert`); one that does not convert is not a library
                let path = if lt::convertible(kind.table_kind(), &l.lib_type) { crate::library_convert::readable(kind.table_kind(), &l.path).ok()? } else { l.path };
                Some(LibEntry { name: l.nickname, path, description: l.description, scope: l.scope, hidden: l.hidden })
            })
            .collect();
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
        let by_name = entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
        // The set is another set when a library came or went, moved, or changed on disk. A folder symbol library (`.kicad_symdir`) counts by the folder's
        // own time: a file of it added, removed or renamed, and KiCad's save (which writes a sibling temp file and renames it over the old one). The time of
        // each of its files (`Kind::modified`, what the caches of one library's content check) is a look at every file -- kicad-symbols has 22,910 -- on
        // every request; a file edited in place leaves the set as it was, and so the all-names answer of the trees (`library_index::all`).
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

    /// Every library of the set, sorted as the trees sort them (hidden ones too).
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

/// `GuessPluginTypeFromLibPath`: a table file is a nested table; KiCad's own formats are told by their extension, and a symbol library also by being a
/// folder of `.kicad_sym` files (`SCH_IO_KICAD_SEXPR::CanReadLibrary`, KiCad 10's `.kicad_symdir`).
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
        TableKind::Symbol => (ext == eda_kicad::SYMBOL_LIB_FILE_EXT || (path.is_dir() && eda_kicad::can_read_symbol_library(path))).then_some(lt::KICAD_TYPE),
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
        // a library of another format is read through kicad-cli: what it said when it could not convert the library; a KiCad symbol library that loads with
        // errors (KiCad 25ec2335 `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`: a folder library's file that does not read) says so, as the row's `LIB_STATUS::error`
        _ => lt::check_row(kind, row, &path)
            .or_else(|| lt::convertible(kind, &row.lib_type).then(|| crate::library_convert::status(kind, &path)).flatten())
            .or_else(|| (kind == TableKind::Symbol && row.lib_type == lt::KICAD_TYPE).then(|| eda_kicad::symbol_library_load_errors(&path)).flatten()),
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
        "writable": eda_kicad::design_block::may_write(&t.path),
        "rows": t.table.rows.iter().zip(&t.rows).map(|(r, l)| row_json(kind, r, Some(l), vars, &base)).collect::<Vec<_>>(),
    })
}

fn global_note(kind: TableKind, origin: &GlobalOrigin, target: Option<&Path>) -> String {
    let name = kind.file_name();
    let then = match target {
        Some(t) => format!(" OK writes these rows as your global table, {}.", t.display()),
        None => " It cannot be written: this machine has no KiCad configuration folder (run KiCad once, or set KICAD_CONFIG_HOME).".to_string(),
    };
    match origin {
        GlobalOrigin::Config(p) => format!("Your global {name}, in KiCad's configuration folder ({}): OK writes your changes to it, as KiCad's dialog does.", p.display()),
        GlobalOrigin::Stock(p) => format!("KiCad has written no global {name} in its configuration folder yet, so the table KiCad installs ({}) is shown -- the one KiCad's first run offers.{then}", p.display()),
        GlobalOrigin::Folder(p) => format!("No library table was found: the library files in {} are listed, each as its own row.{then}", p.display()),
        GlobalOrigin::Missing => format!("No KiCad library table or library folder was found on this machine.{then}"),
    }
}

/// The two tables of one kind: the global one (KiCad's configuration folder's, or the one KiCad installs until that is written) and the board's.
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
    let target = global_table_path(&l.env, kind);
    json!({
        "kind": kind.name(),
        "file": kind.file_name(),
        "supports_visibility": kind.supports_visibility(),
        "types": lt::offered_types(kind),
        "new_library_name": unique_name(&taken),
        "global": { "scope": "global", "path": global_path.display().to_string(), "exists": l.global.is_some(), "writable": target.is_some(), "origin": origin,
                    "target": target.as_ref().map(|t| t.display().to_string()), "stock": stock_table(&l.env, kind).display().to_string(),
                    "note": global_note(kind, &l.global_origin, target.as_deref()), "error": l.global_error, "rows": global_rows },
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
            "global_file": env.config_dir.as_ref().map(|d| d.join("kicad_common.json").display().to_string()),
            "search_paths": search_paths(env, dir),
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

/// A board folder that is KiCad's configuration folder (or lies inside it) has no project tables of its own to edit: the file such an edit would write is KiCad's
/// global table, which only the Global Libraries tab writes.
pub(crate) fn refuse_kicads_configuration(env: &Env, dir: &Path) -> Result<(), String> {
    let Some(config) = &env.config_dir else { return Ok(()) };
    let (board, config) = (std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()), std::fs::canonicalize(config).unwrap_or_else(|_| config.clone()));
    if board.starts_with(&config) {
        return Err(format!("{} is KiCad's own configuration folder, not a board's: open a board folder of your own to edit a project's tables (the Global Libraries tab edits KiCad's global ones).", dir.display()));
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
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let kind = match kind_in(&req) {
        Ok(k) => k,
        Err(e) => return e,
    };
    // Which table: the board's (the Project tab), KiCad's global one (`PANEL_SYM_LIB_TABLE::TransferDataFromWindow`: `globalTable->Save()`), or a table one of them
    // chains (`LIB_TABLE_NOTEBOOK_PANEL::SaveTable` of a tab opened on it). Nothing else is written: a chained table must be one the tables name.
    let scope = req.get("scope").and_then(Value::as_str).unwrap_or("project");
    let path = match scope {
        "project" => {
            if let Err(e) = refuse_kicads_configuration(env, dir) {
                return bad(e);
            }
            project_table_path(dir, kind)
        }
        "global" => match global_table_path(env, kind) {
            Some(p) => p,
            None => return bad("There is no KiCad configuration folder on this machine, so there is no global library table to write (run KiCad once, or set KICAD_CONFIG_HOME)."),
        },
        "nested" => {
            let asked = std::fs::canonicalize(text(&req, "path")).unwrap_or_else(|_| PathBuf::from(text(&req, "path")));
            if !chained_tables(&load_with(env.clone(), dir, kind)).contains(&asked) {
                return bad(format!("{} is not a table the library tables chain: the studio writes only the tables they name.", asked.display()));
            }
            asked
        }
        other => return bad(format!("scope is project, global or nested, not '{other}'")),
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
    // a row whose format was left blank takes the one its path says (`GuessPluginTypeFromLibPath`); a relative path is relative to the table's folder
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let base = path.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
    let kept: Vec<lt::Row> = kept
        .into_iter()
        .map(|mut r| {
            if r.lib_type.trim().is_empty() {
                r.lib_type = guess_type(kind, &vars.expand_uri(&r.uri, &base)).unwrap_or(lt::KICAD_TYPE).to_string();
            }
            r
        })
        .collect();
    if kept.is_empty() && !path.is_file() && scope == "project" {
        let mut v = view_with(env, dir);
        v["ok"] = json!(true);
        return v;
    }
    let mut table = Table::new(kind);
    table.rows = kept;
    if let Err(e) = lt::write_table(&path, &table) {
        let what = if scope == "global" { "global" } else { "project" };
        return bad(format!("Error saving {what} library table:\n\n{e}"));
    }
    let mut v = view_with(env, dir);
    v["ok"] = json!(true);
    v
}

/// Every table the global and project tables chain, as read (resolved paths): the only tables besides those two that a write may name.
fn chained_tables(l: &Loaded) -> Vec<PathBuf> {
    fn walk(t: &LoadedTable, out: &mut Vec<PathBuf>) {
        for loaded in &t.rows {
            if let Some(child) = &loaded.child {
                out.push(child.path.clone());
                walk(child, out);
            }
        }
    }
    let mut out = Vec::new();
    for t in l.global.iter().chain(l.project.iter()) {
        walk(t, &mut out);
    }
    out
}

/// `POST /api/library_tables/reset` `{kind}`: Reset Libraries (`PANEL_SYM_LIB_TABLE::onReset`, `PANEL_FP_LIB_TABLE::onReset`) -- `LIBRARY_MANAGER::CreateGlobalTable(
/// type, true )`: the global table in KiCad's configuration folder written anew with one row, `(lib (name "KiCad") (type "Table") (uri "<the table KiCad installs>")
/// (options "") (descr "KiCad Default Libraries"))`. It takes place at once, whatever the dialog's OK or Cancel (KiCad: "this action takes place even if the dialog is
/// canceled"); the dialog asks first ("This action will reset your global library table on disk and cannot be undone.") when the table has rows. The answer is the view.
pub(crate) fn reset_global(dir: &Path, body: &[u8]) -> Value {
    reset_global_with(&Env::discover(), dir, body)
}

pub(crate) fn reset_global_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let kind = match kind_in(&req) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let Some(path) = global_table_path(env, kind) else {
        return bad("There is no KiCad configuration folder on this machine, so there is no global library table to reset (run KiCad once, or set KICAD_CONFIG_HOME).");
    };
    let mut table = Table::new(kind);
    table.rows = vec![lt::Row {
        nickname: "KiCad".into(),
        uri: stock_table(env, kind).display().to_string(),
        lib_type: lt::NESTED_TYPE.into(),
        description: "KiCad Default Libraries".into(),
        ..Default::default()
    }];
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = lt::write_table(&path, &table) {
        return bad(format!("Error saving global library table:\n\n{e}"));
    }
    let mut v = view_with(env, dir);
    v["ok"] = json!(true);
    v
}

/// `POST /api/library_tables/migrate` `{kind, rows: [{nickname, uri, type, options}], overwrite: [index...], plan?: bool}`: Migrate Libraries
/// (`PANEL_SYM_LIB_TABLE::onConvertLegacyLibraries`, `PANEL_FP_LIB_TABLE::onMigrateLibraries`) for the rows the person selected, as the grid holds them. The rows of
/// KiCad's own format (and, for symbols, Database ones) are not migrated; none left is "Select one or more rows containing libraries to save as current KiCad
/// format.". Each row's library is converted by kicad-cli ([`crate::library_convert::migrate`]) into the KiCad library beside it, and the answer gives the row's new
/// path (written as a table writes it, `NormalizePath`), type `KiCad` and options (a symbol row's are cleared, a footprint row's kept); the grid takes them and OK
/// writes the table. A destination that exists is not touched unless its row's index is in `overwrite`: with `plan` the answer only says, per row, where it would
/// go and whether that exists, so the dialog can ask KiCad's question first ("File '%s' already exists. Do you want overwrite this file?" / "Folder '%s' already
/// exists. Do you want overwrite any existing footprints?", Yes / No / Cancel). `{results: [{row, error?, uri?, type?, options?, dest, exists}]}`.
pub(crate) fn migrate_rows(dir: &Path, body: &[u8]) -> Value {
    migrate_rows_with(&Env::discover(), dir, body, &|kind, from, to| crate::library_convert::migrate(kind, from, to))
}

pub(crate) fn migrate_rows_with(env: &Env, dir: &Path, body: &[u8], migrate: &dyn Fn(TableKind, &Path, &Path) -> Result<(), String>) -> Value {
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
    let plan = req.get("plan").and_then(Value::as_bool).unwrap_or(false);
    let overwrite: Vec<usize> = req.get("overwrite").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as usize).collect()).unwrap_or_default();
    let base = match req.get("table").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        Some(t) => Path::new(t).parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf()),
        None => dir.to_path_buf(),
    };
    let migratable = |r: &lt::Row| r.lib_type != lt::KICAD_TYPE && !(kind == TableKind::Symbol && r.lib_type == "Database");
    if !rows.iter().any(migratable) {
        return bad("Select one or more rows containing libraries to save as current KiCad format.");
    }
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let norm_vars = normalize_vars(env, &vars, dir);
    let results: Vec<Value> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| migratable(r))
        .map(|(i, r)| {
            let source = vars.expand_uri(&r.uri, &base);
            if !source.exists() {
                return json!({ "row": i, "error": format!("Library '{}' not found.", r.uri) });
            }
            let Some(dest) = crate::library_convert::migrated_path(kind, &source) else {
                return json!({ "row": i, "error": "Design block libraries have no other format to migrate from." });
            };
            let exists = dest.exists();
            if plan || (exists && !overwrite.contains(&i)) {
                return json!({ "row": i, "dest": dest.display().to_string(), "exists": exists, "skipped": !plan });
            }
            if let Err(e) = refuse_inside_kicads_configuration(env, &dest) {
                return json!({ "row": i, "error": e });
            }
            match migrate(kind, &source, &dest) {
                Ok(()) => json!({
                    "row": i, "dest": dest.display().to_string(), "exists": exists,
                    "uri": lt::normalize_path(&dest, &norm_vars, Some(dir)), "type": lt::KICAD_TYPE,
                    "options": if kind == TableKind::Symbol { String::new() } else { r.options.clone() },
                }),
                Err(e) => json!({ "row": i, "error": e, "dest": dest.display().to_string() }),
            }
        })
        .collect();
    json!({ "results": results })
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
            // a library of another format: KiCad's check loads it with its plugin (`CheckTableRow`), so here kicad-cli converts it, and a failure is its status
            if error.is_none() && lt::convertible(kind, &r.lib_type) && !r.uri.trim().is_empty() {
                error = crate::library_convert::readable(kind, &path).err();
            }
            json!({ "error": error.unwrap_or_default(), "path": path.display().to_string(), "guess": guess_type(kind, &path) })
        })
        .collect();
    json!({ "results": results })
}

/// The variables a path may be written with, as `NormalizePath` takes them: KiCad's, then the project's (their values with their own variables replaced).
pub(crate) fn normalize_vars(env: &Env, vars: &Vars, project_dir: &Path) -> Vec<(String, String)> {
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
        if path.exists() && kind == TableKind::DesignBlock {
            // `DESIGN_BLOCK_PANE::createNewDesignBlockLibrary`: an existing folder is refused when it is not writable ("Library %s is read only.") and otherwise
            // asked about (a KIDIALOG "Library %s already exists." with OK labelled "Overwrite"); Overwrite is `pi->DeleteLibrary( libPath )` then `CreateLibrary`.
            // `DeleteLibrary`'s own refusals ("Library folder '%s' has unexpected files.") stand, so a folder with anything but blocks in it is never removed.
            if !eda_kicad::design_block::is_writable(&path) {
                return bad(format!("Library {} is read only.", path.display()));
            }
            if req.get("overwrite").and_then(Value::as_bool) != Some(true) {
                let mut v = bad(format!("Library {} already exists.", path.display()));
                v["confirm"] = json!("Overwrite");
                return v;
            }
            if let Err(e) = eda_kicad::design_block::delete_library(&path) {
                return bad(e);
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
        // a folder is a folder library, KiCad 10's `.kicad_symdir` ("KiCad (folder with .kicad_sym files)" of `PANEL_SYM_LIB_TABLE`'s browse menu, any
        // folder its `wxDirDialog` lets one pick), nicknamed by `GetName()` as a file is
        if kind == TableKind::Symbol && !has_ext && !path.is_dir() {
            return bad("Choose a .kicad_sym file, or a folder of .kicad_sym files.");
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

/// A library the library editors' Save Library As wrote (`crate::library_save_api`), put in the board's table: `SYMBOL_EDIT_FRAME::addLibTableEntry( aLibFile,
/// PROJECT )` (the nickname is the file's name, and a nickname a library already has takes `%s%d` from 1 until it is free), or with `replace` the row of
/// that nickname pointed at the new path (`replaceLibTableEntry`: its type set to `KiCad`). A row of KiCad's global table is KiCad's and is not rewritten here.
/// The path is written as a table writes one (`NormalizePath`). The answer is the nickname and the path as written.
pub(crate) fn put_library_row(dir: &Path, kind: TableKind, path: &Path, replace: Option<&str>) -> Result<(String, String), String> {
    let env = Env::discover();
    refuse_kicads_configuration(&env, dir)?;
    refuse_inside_kicads_configuration(&env, path)?;
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let uri = lt::normalize_path(path, &normalize_vars(&env, &vars, dir), Some(dir));
    let mut table = project_table(dir, kind)?;
    let nickname = match replace {
        Some(nick) => {
            let Some(row) = table.rows.iter_mut().find(|r| r.nickname == nick) else {
                return Err(format!("The library '{nick}' is a row of KiCad's global table, which Save Library As does not update here: add the new library to the project's table, or change the global row in Manage Libraries."));
            };
            row.uri = uri.clone();
            row.lib_type = lt::KICAD_TYPE.to_string();
            nick.to_string()
        }
        None => {
            let stem = lt::fix_nickname(&path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let taken = |n: &str| table.has_row(n) || n == PROJECT_LIBRARY || load_with(env.clone(), dir, kind).libs().iter().any(|l| l.nickname == n && l.is_available());
            let mut nickname = stem.clone();
            let mut suffix = 1;
            while taken(&nickname) {
                nickname = format!("{stem}{suffix}");
                suffix += 1;
            }
            table.rows.push(lt::Row { nickname: nickname.clone(), uri: uri.clone(), lib_type: lt::KICAD_TYPE.to_string(), ..Default::default() });
            nickname
        }
    };
    lt::write_table(&project_table_path(dir, kind), &table)?;
    Ok((nickname, uri))
}

/// A library file the studio is asked to write that lies in KiCad's own configuration folder: refused, as every write there is.
pub(crate) fn refuse_inside_kicads_configuration(env: &Env, path: &Path) -> Result<(), String> {
    let Some(config) = &env.config_dir else { return Ok(()) };
    let config = std::fs::canonicalize(config).unwrap_or_else(|_| config.clone());
    let at = path.parent().and_then(|p| std::fs::canonicalize(p).ok()).map(|p| p.join(path.file_name().unwrap_or_default())).unwrap_or_else(|| path.to_path_buf());
    if at.starts_with(&config) {
        return Err(format!("{} is in KiCad's own configuration folder, where the studio writes only the global library tables and KiCad's path variables.", path.display()));
    }
    Ok(())
}

/// The rows of one grid of Configure Paths, checked as `DIALOG_CONFIGURE_PATHS::TransferDataFromWindow` and `OnGridCellChanging` check them: a row with neither a name
/// nor a path is dropped; else both are needed ("Environment variable name cannot be empty.", "Environment variable path cannot be empty."), the name is a variable
/// name (`ENV_VAR_NAME_VALIDATOR`: capitals, digits and `_`) and not `KIPRJMOD`. A variable the real environment defines is taken as it is ("Don't check for
/// consistency on external variables"). `Err` is `{error, row, column, scope}` for the row refused.
fn checked_variables(list: &[Value], scope: &str, external: &dyn Fn(&str) -> bool) -> Result<BTreeMap<String, String>, Value> {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (row, item) in list.iter().enumerate() {
        let (name, value) = (text(item, "name").trim().to_string(), text(item, "value").trim().to_string());
        let fail = |column: &str, message: String| json!({ "error": message, "row": row, "column": column, "scope": scope });
        if !name.is_empty() && external(&name) {
            vars.insert(name, value);
            continue;
        }
        if name.is_empty() && value.is_empty() {
            continue;
        }
        if name.is_empty() {
            return Err(fail("name", "Environment variable name cannot be empty.".into()));
        }
        if value.is_empty() {
            return Err(fail("value", "Environment variable path cannot be empty.".into()));
        }
        if name == "KIPRJMOD" {
            return Err(fail("name", "The name KIPRJMOD is reserved, and cannot be used.".into()));
        }
        if !lt::is_variable_name(&name) {
            return Err(fail("name", format!("'{name}' is not a variable name: capital letters, digits (not first) and underscores only.")));
        }
        if value.contains(['"', '\n', '\r']) {
            return Err(fail("value", "A path cannot hold a double quote or a line break.".into()));
        }
        if vars.insert(name.clone(), value).is_some() {
            return Err(fail("name", format!("Two variables are named {name}.")));
        }
    }
    Ok(vars)
}

/// What `kicad_common.json`'s `environment.vars` holds after Configure Paths' OK with KiCad's variables `listed` (`TransferDataFromWindow` then
/// `COMMON_SETTINGS`' `environment.vars` writer): a variable KiCad predefines and that keeps its default value is not kept ("Default values are never persisted"); one
/// the real environment defines keeps the value the file had for it, or none ("Vars that existed in JSON are persisted, but if they were overridden externally, we
/// persist the old value"); every other one listed is kept; one not listed is gone ("Remove deleted env vars").
pub(crate) fn persisted_global_vars(env: &Env, listed: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let defaults: BTreeMap<String, String> = lt::global_env_vars(env.major, &env.stock, &BTreeMap::new(), &BTreeMap::new()).into_iter().map(|v| (v.name, v.value)).collect();
    let mut out = BTreeMap::new();
    for (name, value) in listed {
        if env.real.get(name).is_some_and(|v| !v.is_empty()) {
            if let Some(old) = env.configured.get(name) {
                out.insert(name.clone(), old.clone());
            }
            continue;
        }
        if defaults.get(name) == Some(value) {
            continue;
        }
        out.insert(name.clone(), value.clone());
    }
    out
}

/// Write `environment.vars` of `kicad_common.json` in KiCad's configuration folder, every other setting of the file kept.
fn write_global_vars(env: &Env, vars: &BTreeMap<String, String>) -> Result<(), String> {
    let Some(config) = &env.config_dir else {
        return Err("There is no KiCad configuration folder on this machine, so KiCad's path variables cannot be written (run KiCad once, or set KICAD_CONFIG_HOME).".into());
    };
    let path = config.join("kicad_common.json");
    let mut doc: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{} cannot be read, so it is not written: {e}", path.display()))?,
        Err(_) => json!({}),
    };
    if !doc.is_object() {
        return Err(format!("{} is not a settings file, so it is not written.", path.display()));
    }
    if !doc.get("environment").is_some_and(Value::is_object) {
        doc["environment"] = json!({});
    }
    doc["environment"]["vars"] = Value::Object(vars.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect());
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    let tmp = config.join(".kicad_common.json.eda-tmp");
    std::fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `POST /api/path_variables` `{variables?: [{name, value}], global?: [{name, value}]}`: Configure Paths' OK. `global` is KiCad's own variables -- the dialog's one
/// grid in KiCad (`DIALOG_CONFIGURE_PATHS`), written where KiCad keeps them, `kicad_common.json`'s `environment.vars` ([`persisted_global_vars`]); they are also
/// the 3D model search paths (`FILENAME_RESOLVER::createPathList`). `variables` is the board's own (a studio addition: KiCad's project text variables named like a
/// path variable). The name rules are the dialog's ([`checked_variables`]).
pub(crate) fn set_variables(dir: &Path, body: &[u8]) -> Value {
    set_variables_with(&Env::discover(), dir, body)
}

pub(crate) fn set_variables_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let project = req.get("variables").and_then(Value::as_array);
    let global = req.get("global").and_then(Value::as_array);
    if project.is_none() && global.is_none() {
        return bad("variables (the board's) or global (KiCad's) is a list");
    }
    let project = match project.map(|l| checked_variables(l, "project", &|_| false)).transpose() {
        Ok(v) => v,
        Err(e) => return e,
    };
    let global = match global.map(|l| checked_variables(l, "global", &|n| env.real.get(n).is_some_and(|v| !v.is_empty()))).transpose() {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Some(vars) = project {
        if let Err(e) = refuse_kicads_configuration(env, dir) {
            return bad(e);
        }
        // The board's variables are the project's text variables (Board Setup > Text Variables is the other view of the list): the ones this dialog does not list,
        // those not named like a path variable, stay as they are.
        let mut all: BTreeMap<String, String> = crate::text_variables::project_variables(dir).into_iter().filter(|(name, _)| !lt::is_variable_name(name)).collect();
        all.extend(vars);
        if let Err(e) = crate::text_variables::set_project_variables(dir, &all) {
            return bad(e);
        }
    }
    let mut env = env.clone();
    if let Some(vars) = global {
        let kept = persisted_global_vars(&env, &vars);
        if let Err(e) = write_global_vars(&env, &kept) {
            return bad(e);
        }
        env.configured = kept;
    }
    let mut v = view_with(&env, dir);
    v["ok"] = json!(true);
    v
}

/// The 3D model search paths (`FILENAME_RESOLVER::createPathList`, `GetKicadPaths`): `${KIPRJMOD}` first, then every path variable of KiCad's but
/// `KICAD_PTEMPLATES`, the `KICAD*_FOOTPRINT_DIR` ones and a value that is a URL, and `KICADn_3DMODEL_DIR` when none of them is a 3D model folder. KiCad 8303b2ad
/// has no page that edits the list (nothing calls `FILENAME_RESOLVER::UpdatePathList`; Configure Paths' 3D grid is gone and its error strings are what is left):
/// the list is the path variables, which Configure Paths edits.
fn search_paths(env: &Env, dir: &Path) -> Vec<Value> {
    let mut out = vec![json!({ "alias": "${KIPRJMOD}", "path": dir.display().to_string() })];
    let mut has_models = false;
    for v in env.global_vars() {
        let footprints = v.name.starts_with("KICAD") && v.name.ends_with("_FOOTPRINT_DIR");
        if v.name == "KICAD_PTEMPLATES" || footprints || v.value.contains("://") {
            continue;
        }
        has_models |= v.name.starts_with("KICAD") && v.name.ends_with("_3DMODEL_DIR");
        out.push(json!({ "alias": format!("${{{}}}", v.name), "path": v.value }));
    }
    if !has_models {
        let name = format!("KICAD{}_3DMODEL_DIR", env.major);
        out.push(json!({ "alias": format!("${{{name}}}"), "path": env.real.get(&name).cloned().unwrap_or_default() }));
    }
    out
}

/// `GET /api/fs/list?path=&what=`: the stand-in for the native file dialog the browse buttons open in KiCad -- a page cannot see the machine's folders, the
/// server can. The folders of `path` (variables allowed, relative to the board's folder; a file stands for its folder), and the files `what` asks for:
/// `symbol` (.kicad_sym), `table` (a library table), `drawing_sheet` (.kicad_wks, Page Settings' drawing sheet), nothing but folders for `footprint`, `design_block`, `symbol_folder` (a folder library of symbols) and
/// `folder`. A `.pretty`, `.kicad_blocks` or `.kicad_symdir` folder is flagged: it is the library.
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
                // Page Settings' drawing sheet file (`FILEEXT::DrawingSheetFileWildcard`)
                "drawing_sheet" => name.to_ascii_lowercase().ends_with(".kicad_wks"),
                "table" => lt::TableKind::ALL.iter().any(|k| name == k.file_name()),
                _ => false,
            }
        };
        if wanted {
            entries.push((name.clone(), is_dir, is_dir && (name.ends_with(".pretty") || name.ends_with(".kicad_blocks") || name.ends_with(".kicad_symdir"))));
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
    use eda_model::units::Nm;
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

    /// KiCad 10's folder libraries (kicad-symbols ships `Device.kicad_symdir/`, a `.kicad_sym` per symbol): with no table to read, each is a row of the
    /// folder's table, named by its stem; a row naming one is a library that reads (`isLibraryPathValid`: the folder exists), of type KiCad when the
    /// type is left blank (`CanReadLibrary`); Add Existing takes one ("KiCad (folder with .kicad_sym files)"), and the browser flags it as a library.
    #[test]
    fn a_folder_of_symbol_files_is_a_library_of_the_tables() {
        let m = machine("symdir", false);
        std::fs::remove_file(m.share.join("template").join("sym-lib-table")).unwrap();
        let symbols = m.share.join("symbols");
        for lib in ["Device", "Amp"] {
            std::fs::remove_file(symbols.join(format!("{lib}.kicad_sym"))).unwrap();
        }
        let dir = symbols.join("Device.kicad_symdir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("R.kicad_sym"), "(kicad_symbol_lib (version 20251024) (symbol \"R\"))").unwrap();
        std::fs::write(symbols.join("Amp.kicad_sym"), "(kicad_symbol_lib (symbol \"LM358\"))").unwrap();
        // the same name as a file and as a folder: one row, the file's
        std::fs::create_dir_all(symbols.join("Amp.kicad_symdir")).unwrap();
        let loaded = load_with(m.env(), &m.board, TableKind::Symbol);
        assert!(matches!(&loaded.global_origin, GlobalOrigin::Folder(_)), "{:?}", loaded.global_origin);
        let set = m.set(Kind::Symbol);
        assert_eq!(names(&set), ["Amp", "Device"]);
        assert_eq!(set.find("Device").unwrap().path, dir);
        assert!(set.find("Amp").unwrap().path.ends_with("Amp.kicad_sym"));
        assert_eq!(crate::library_index::items_in_for_tests(Kind::Symbol, &set, "Device").unwrap(), ["R"]);

        let env = m.env();
        let shared = m.root.join("shared");
        let team = shared.join("Team.kicad_symdir");
        std::fs::create_dir_all(&team).unwrap();
        std::fs::write(team.join("X.kicad_sym"), "(kicad_symbol_lib (symbol \"X\"))").unwrap();
        std::fs::create_dir_all(shared.join("plain")).unwrap();
        let checked = check_rows_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": [row("Team", &team.display().to_string()), row("Plain", &shared.join("plain").display().to_string())] })));
        assert_eq!(checked["results"][0]["error"], "", "{checked}");
        assert_eq!(checked["results"][0]["guess"], "KiCad");
        assert_eq!(checked["results"][1]["error"], "", "an empty folder loads as an empty library");
        assert!(checked["results"][1]["guess"].is_null(), "but CanReadLibrary does not know it: {checked}");
        let added = add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "path": team.display().to_string() })));
        assert_eq!(added["added"]["nickname"], "Team", "{added}");
        assert_eq!(added["added"]["uri"], team.display().to_string());
        assert_eq!(crate::library_index::items_in_for_tests(Kind::Symbol, &m.set(Kind::Symbol), "Team").unwrap(), ["X"]);
        assert!(add_library_with(&env, &m.board, &req(json!({ "kind": "symbol", "path": shared.join("nothing").display().to_string() })))["error"].as_str().unwrap().contains("not found"));
        let listed = browse_with(&env, &m.board, &shared.display().to_string(), "symbol_folder");
        let entries: Vec<(String, bool)> = listed["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap().to_string(), e["library"].as_bool().unwrap())).collect();
        assert_eq!(entries, [("plain".to_string(), false), ("Team.kicad_symdir".to_string(), true)]);
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
        sym.rows = vec![row("Gone", "${KIPRJMOD}/libs/Gone.kicad_sym"), lt::Row { disabled: true, ..row("Off", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym") }, lt::Row { lib_type: "Database".into(), ..row("Old", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym") }];
        lt::write_table(&project_table_path(&m.board, TableKind::Symbol), &sym).unwrap();
        assert_eq!(names(&m.set(Kind::Symbol)), ["Amp", "Device"]);
        let all = load_with(m.env(), &m.board, TableKind::Symbol).libs();
        let by = |n: &str| all.iter().find(|l| l.nickname == n).unwrap().clone();
        assert!(by("Gone").error.as_deref().is_some_and(|e| e.ends_with("Gone.kicad_sym' not found.")));
        assert!(by("Off").disabled && by("Off").error.is_none());
        assert!(by("Old").error.as_deref().is_some_and(|e| e.contains("a 'Database' library is not read here")));
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
    fn the_view_shows_the_global_table_with_what_it_chains() {
        let m = machine("view", true);
        let v = view_with(&m.env(), &m.board);
        let sym = &v["tables"]["symbol"];
        assert_eq!((v["major"].clone(), v["version"].clone()), (json!(10), json!("10.99")));
        assert_eq!(sym["supports_visibility"], true);
        assert_eq!(v["tables"]["footprint"]["supports_visibility"], false);
        assert_eq!((sym["global"]["writable"].clone(), sym["project"]["writable"].clone()), (json!(true), json!(true)));
        assert_eq!(sym["global"]["origin"], "config");
        assert_eq!(sym["global"]["target"], json!(m.config.join("sym-lib-table").display().to_string()));
        assert!(sym["global"]["note"].as_str().unwrap().contains("OK writes your changes to it"));
        assert_eq!(sym["global"]["stock"], json!(m.share.join("template").join("sym-lib-table").display().to_string()));
        // the Library Format column offers what `PANEL_FP_LIB_TABLE::populatePluginList` offers: no Allegro, no IPC-2581
        let fp_types = v["tables"]["footprint"]["types"].as_array().unwrap();
        assert!(fp_types.contains(&json!("gEDA / Lepton EDA")) && !fp_types.contains(&json!("Allegro")) && !fp_types.contains(&json!("IPC-2581")));
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

    /// r6 B6-4 (`DESIGN_BLOCK_PANE::createNewDesignBlockLibrary`): New Library on an existing folder says "Library %s is read only." when it cannot be written,
    /// else asks "Library %s already exists." with an Overwrite button, which deletes the library and makes it again.
    #[test]
    fn new_design_block_library_on_a_read_only_existing_folder_says_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let m = machine("dbexists", true);
        let env = m.env();
        let old = m.board.join("blocks").join("Old.kicad_blocks");
        std::fs::create_dir_all(old.join("amp.kicad_block")).unwrap();
        let asked = add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Old" })));
        assert_eq!((asked["error"].clone(), asked["confirm"].clone()), (json!(format!("Library {} already exists.", old.display())), json!("Overwrite")), "{asked}");
        assert!(old.join("amp.kicad_block").is_dir(), "nothing is removed before the answer");
        let r = add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Old", "overwrite": true })));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(eda_kicad::design_block::enumerate(&old).unwrap(), Vec::<String>::new(), "Overwrite: DeleteLibrary then CreateLibrary");
        let locked = m.board.join("blocks").join("Locked.kicad_blocks");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        if !eda_kicad::design_block::is_writable(&locked) {
            let r = add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Locked", "overwrite": true })));
            assert_eq!(r["error"], json!(format!("Library {} is read only.", locked.display())), "{r}");
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
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
            { "nickname": "D", "uri": "/x", "type": "Database" },
        ] })));
        let res = r["results"].as_array().unwrap();
        assert_eq!(res[0]["error"], "");
        assert!(res[1]["error"].as_str().unwrap().ends_with("Nope.kicad_sym' not found."));
        assert_eq!(res[2]["error"], "");
        assert!(res[3]["error"].as_str().unwrap().starts_with("Nested table '/no/such' not found"));
        assert_eq!(res[4]["error"], "Library '/x' not found.", "a legacy library is read through kicad-cli: it has to be there");
        assert!(res[6]["error"].as_str().unwrap().contains("a 'Database' library is not read here"));
        assert_eq!(res[5]["error"], "", "a row with no path is dropped on OK, not an error now");
        assert_eq!(res[2]["guess"], "Table");
        assert_eq!(res[0]["guess"], "KiCad");
        assert!(res[0]["path"].as_str().unwrap().ends_with("symbols/Device.kicad_sym"));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn a_board_in_kicads_configuration_folder_has_no_project_tables_to_write() {
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

    /// `PANEL_SYM_LIB_TABLE::TransferDataFromWindow`: the Global Libraries tab's rows go through `VerifyTable` and into the global table in KiCad's configuration
    /// folder; a table it chains is written when its tab changed (`LIB_TABLE_NOTEBOOK_PANEL::SaveTable`); any other path is refused.
    #[test]
    fn the_global_tab_writes_kicads_global_table_and_a_chained_tab_its_own_table() {
        let m = machine("globalwrite", true);
        let env = m.env();
        let kicad_row = json!({ "nickname": "KiCad", "uri": m.share.join("template").join("sym-lib-table").display().to_string(), "type": "Table", "descr": "KiCad Default Libraries" });
        // VerifyTable's checks as for the project's
        let bad_rows = set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "scope": "global", "rows": [kicad_row.clone(), row("", "/x/a.kicad_sym")] })));
        assert_eq!((bad_rows["error"].clone(), bad_rows["row"].clone()), (json!("Library must have a nickname."), json!(1)));
        let ok = set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "scope": "global", "rows": [kicad_row.clone(), row("Mine", "${KICAD10_SYMBOL_DIR}/Amp.kicad_sym")] })));
        assert_eq!(ok["ok"], true, "{ok}");
        let written = std::fs::read_to_string(m.config.join("sym-lib-table")).unwrap();
        assert!(written.contains("(name \"Mine\")") && written.contains("(name \"KiCad\")"), "{written}");
        assert!(!project_table_path(&m.board, TableKind::Symbol).exists(), "the board's table is not touched");
        assert!(names(&m.set(Kind::Symbol)).contains(&"Mine"), "the libraries follow the global table");
        // the chained table: its own file, and only a table the tables name
        let chained = std::fs::canonicalize(m.share.join("template").join("sym-lib-table")).unwrap();
        let r = set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "scope": "nested", "path": chained.display().to_string(), "rows": [row("Device", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym")] })));
        assert_eq!(r["ok"], true, "{r}");
        assert!(!std::fs::read_to_string(&chained).unwrap().contains("\"Amp\""), "the chained table lost its Amp row");
        let stray = m.root.join("stray-sym-lib-table");
        std::fs::write(&stray, "(sym_lib_table (version 7))").unwrap();
        let r = set_table_with(&env, &m.board, &req(json!({ "kind": "symbol", "scope": "nested", "path": stray.display().to_string(), "rows": [] })));
        assert!(r["error"].as_str().unwrap().contains("is not a table the library tables chain"), "{r}");
        assert_eq!(std::fs::read_to_string(&stray).unwrap(), "(sym_lib_table (version 7))");
        // a machine with no KiCad configuration folder has no global table to write
        let mut none = env.clone();
        none.config_dir = None;
        assert!(set_table_with(&none, &m.board, &req(json!({ "kind": "symbol", "scope": "global", "rows": [kicad_row] })))["error"].as_str().unwrap().contains("no KiCad configuration folder"));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    /// KiCad 25ec2335's `Load` of a folder library with a file that does not read: the row's status is the load error, in KiCad's words.
    #[test]
    fn a_folder_library_with_a_broken_file_is_an_error_row() {
        let m = machine("broken", true);
        let lib = m.board.join("Broken.kicad_symdir");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Good.kicad_sym"), "(kicad_symbol_lib (version 20241209) (symbol \"Good\"))").unwrap();
        std::fs::write(lib.join("Bad.kicad_sym"), "(kicad_symbol_lib (version 20241209) (symbol \"Bad\"").unwrap();
        let r = set_table_with(&m.env(), &m.board, &req(json!({ "kind": "symbol", "rows": [row("Broken", "${KIPRJMOD}/Broken.kicad_symdir")] })));
        let rows = &r["tables"]["symbol"]["project"]["rows"];
        assert_eq!(rows[0]["status"], "error", "{rows}");
        let e = rows[0]["error"].as_str().unwrap();
        assert!(e.contains("Unable to read file") && e.contains("Bad.kicad_sym") && e.ends_with("The library cannot be saved until these errors are fixed manually."), "{e}");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    /// `LIBRARY_MANAGER::CreateGlobalTable( type, true )`: one row naming the table KiCad installs; KiCad's first run writes no global table, and Reset writes one.
    #[test]
    fn reset_libraries_writes_the_global_table_as_one_row_naming_the_installed_table() {
        let m = machine("reset", false);
        let env = m.env();
        let before = view_with(&env, &m.board);
        assert_eq!(before["tables"]["footprint"]["global"]["origin"], "stock", "no global table yet: the installed one is shown");
        let v = reset_global_with(&env, &m.board, &req(json!({ "kind": "footprint" })));
        assert_eq!(v["ok"], true, "{v}");
        let t = Table::parse(&std::fs::read_to_string(m.config.join("fp-lib-table")).unwrap()).unwrap();
        assert_eq!(t.rows.len(), 1);
        let r = &t.rows[0];
        assert_eq!((r.nickname.as_str(), r.lib_type.as_str(), r.description.as_str()), ("KiCad", "Table", "KiCad Default Libraries"));
        assert_eq!(PathBuf::from(&r.uri), m.share.join("template").join("fp-lib-table"));
        assert_eq!(v["tables"]["footprint"]["global"]["origin"], "config");
        assert_eq!(names(&m.set(Kind::Footprint)), ["Package_SO", "Resistor_SMD"], "the libraries come through the chained table");
        // a library checkout with no template/ folder: the table beside the libraries stands in for the installed one
        std::fs::remove_dir_all(m.share.join("template")).unwrap();
        std::fs::write(m.share.join("symbols").join("sym-lib-table"), "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Device\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Device.kicad_sym\") (options \"\") (descr \"\"))\n)\n").unwrap();
        assert_eq!(stock_table(&env, TableKind::Symbol), m.share.join("symbols").join("sym-lib-table"));
        reset_global_with(&env, &m.board, &req(json!({ "kind": "symbol" })));
        assert_eq!(names(&m.set(Kind::Symbol)), ["Device"]);
        let _ = std::fs::remove_dir_all(&m.root);
    }

    /// Migrate Libraries: the selected rows of another format, each converted into the KiCad library beside it, the row given its new path, type and options.
    #[test]
    fn migrate_libraries_converts_the_selected_foreign_rows_and_asks_before_overwriting() {
        let m = machine("migrate", true);
        let env = m.env();
        std::fs::write(m.board.join("old.lib"), "EESchema-LIBRARY Version 2.4").unwrap();
        std::fs::write(m.board.join("taken.lib"), "EESchema-LIBRARY Version 2.4").unwrap();
        std::fs::write(m.board.join("taken.kicad_sym"), "(kicad_symbol_lib)").unwrap();
        let converted = std::cell::RefCell::new(Vec::new());
        let fake = |kind: TableKind, from: &Path, to: &Path| -> Result<(), String> {
            converted.borrow_mut().push((kind, from.to_path_buf(), to.to_path_buf()));
            std::fs::write(to, "(kicad_symbol_lib (version 20241209))").map_err(|e| e.to_string())
        };
        let rows = json!([
            { "nickname": "Old", "uri": "${KIPRJMOD}/old.lib", "type": "Legacy", "options": "x=1" },
            { "nickname": "Dev", "uri": "${KICAD10_SYMBOL_DIR}/Device.kicad_sym", "type": "KiCad" },
            { "nickname": "Taken", "uri": "${KIPRJMOD}/taken.lib", "type": "Legacy" },
            { "nickname": "Gone", "uri": "${KIPRJMOD}/gone.lib", "type": "EAGLE" },
        ]);
        // nothing of another format among the rows chosen
        let none = migrate_rows_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": [rows[1].clone()] })), &fake);
        assert_eq!(none["error"], "Select one or more rows containing libraries to save as current KiCad format.");
        // the plan: where each would go and whether it is there already
        let plan = migrate_rows_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": rows, "plan": true })), &fake);
        let res = plan["results"].as_array().unwrap();
        assert_eq!(res.len(), 3, "the KiCad row is not migrated: {plan}");
        assert_eq!((res[0]["row"].clone(), res[0]["exists"].clone()), (json!(0), json!(false)));
        assert_eq!((res[1]["row"].clone(), res[1]["exists"].clone()), (json!(2), json!(true)));
        assert_eq!(res[2]["error"], "Library '${KIPRJMOD}/gone.lib' not found.");
        assert!(converted.borrow().is_empty(), "a plan converts nothing");
        // the run: the existing destination is skipped unless the person said to overwrite it
        let run = migrate_rows_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": rows })), &fake);
        let res = run["results"].as_array().unwrap();
        assert_eq!((res[0]["uri"].clone(), res[0]["type"].clone(), res[0]["options"].clone()), (json!("${KIPRJMOD}/old.kicad_sym"), json!("KiCad"), json!("")), "a symbol row's options are cleared: {run}");
        assert_eq!(res[1]["skipped"], true);
        assert_eq!(std::fs::read_to_string(m.board.join("taken.kicad_sym")).unwrap(), "(kicad_symbol_lib)");
        let run = migrate_rows_with(&env, &m.board, &req(json!({ "kind": "symbol", "rows": rows, "overwrite": [2] })), &fake);
        assert_eq!(run["results"][1]["uri"], "${KIPRJMOD}/taken.kicad_sym");
        assert!(std::fs::read_to_string(m.board.join("taken.kicad_sym")).unwrap().contains("20241209"));
        // footprints: `<name>.pretty` beside the file, the row's options kept
        let fp = migrate_rows_with(&env, &m.board, &req(json!({ "kind": "footprint", "rows": [{ "nickname": "E", "uri": "${KIPRJMOD}/old.lib", "type": "Eagle", "options": "k=v" }], "plan": true })), &fake);
        assert_eq!(fp["results"][0]["dest"], json!(m.board.join("old.pretty").display().to_string()));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    /// Configure Paths' OK for KiCad's own variables: `kicad_common.json`'s `environment.vars`, by `COMMON_SETTINGS`' rules, every other setting kept.
    #[test]
    fn kicads_path_variables_are_written_to_kicad_common_json_by_kicads_rules() {
        let mut m = machine("globalvars", true);
        m.env.remove("KICAD10_SYMBOL_DIR"); // not defined outside: KiCad's default holds
        std::fs::write(m.config.join("kicad_common.json"), "{\"appearance\": {\"icon_scale\": 0}, \"environment\": {\"vars\": {\"OLD\": \"/old\"}}}").unwrap();
        let env = m.env();
        let default_sym = env.global_vars().into_iter().find(|v| v.name == "KICAD10_SYMBOL_DIR").unwrap().value;
        let list = json!([
            { "name": "KICAD10_SYMBOL_DIR", "value": default_sym },
            { "name": "KICAD10_FOOTPRINT_DIR", "value": "/elsewhere" },
            { "name": "TEAM_LIBS", "value": " /team " },
        ]);
        let v = set_variables_with(&env, &m.board, &req(json!({ "global": list })));
        assert_eq!(v["ok"], true, "{v}");
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(m.config.join("kicad_common.json")).unwrap()).unwrap();
        // the default is not kept; the one the environment defines keeps the file's value (it had none); OLD was deleted from the grid
        assert_eq!(doc["environment"]["vars"], json!({ "TEAM_LIBS": "/team" }), "{doc}");
        assert_eq!(doc["appearance"]["icon_scale"], 0, "the rest of the file is kept");
        let g = v["variables"]["global"].as_array().unwrap();
        assert!(g.iter().any(|x| x["name"] == "TEAM_LIBS" && x["value"] == "/team"), "the view reads the file again");
        // KiCad's words for a row with no path
        let r = set_variables_with(&env, &m.board, &req(json!({ "global": [{ "name": "X", "value": "" }] })));
        assert_eq!((r["error"].clone(), r["scope"].clone(), r["column"].clone()), (json!("Environment variable path cannot be empty."), json!("global"), json!("value")));
        // the 3D model search paths: KIPRJMOD, then the variables but the footprint folders
        let sp: Vec<String> = v["variables"]["search_paths"].as_array().unwrap().iter().map(|x| x["alias"].as_str().unwrap().to_string()).collect();
        assert_eq!(sp[0], "${KIPRJMOD}");
        assert!(sp.contains(&"${KICAD10_3DMODEL_DIR}".to_string()) && sp.contains(&"${TEAM_LIBS}".to_string()));
        assert!(!sp.iter().any(|a| a.contains("FOOTPRINT_DIR")), "{sp:?}");
        let _ = std::fs::remove_dir_all(&m.root);
    }

    /// The slow tier: an Eagle library of the board's table is read through kicad-cli's conversion and listed, searched and opened like a KiCad library.
    #[test]
    fn an_eagle_library_of_the_tables_is_read_through_kicad_cli() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: set EDA_SLOW_TESTS=1 with kicad-cli installed");
            return;
        }
        let Some(qa) = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from) else {
            eprintln!("skipped: KICAD_QA_DATA is not set");
            return;
        };
        let m = machine("eagle", true);
        let lbr = qa.join("pcbnew/plugins/eagle/lbr/SparkFun-GPS.lbr");
        let r = set_table_with(&m.env(), &m.board, &req(json!({ "kind": "footprint", "rows": [{ "nickname": "GPS", "uri": lbr.display().to_string(), "type": "Eagle" }] })));
        assert_eq!(r["ok"], true, "{r}");
        let set = m.set(Kind::Footprint);
        let gps = set.find("GPS").expect("the Eagle library is a library of the tables");
        assert!(gps.path.join("NEO-M8P.kicad_mod").is_file(), "read as kicad-cli's KiCad copy: {}", gps.path.display());
        assert!(!eda_kicad::library_save::is_footprint_library_writable(&gps.path), "and read only, as KiCad's Eagle plugin is");
        let v = view_with(&m.env(), &m.board);
        assert_eq!(v["tables"]["footprint"]["project"]["rows"][0]["status"], "ok");
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
        let cmd = eda_ops::Cmd::AddSymbol { annotate: None, id: "U1".into(), lib_id: "Mine:Chip9000".into(), at: eda_model::ir::Point { x: Nm::new(1000), y: Nm::new(1000) }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 };
        let rewritten = crate::library_place::rewrite_for_tests(&cmd, &design, &eda_model::ConstraintModel::default(), &syms, &fps).expect("placed from the table library");
        let eda_ops::Cmd::Batch { cmds } = rewritten else { panic!("{rewritten:?}") };
        assert!(matches!(&cmds[0], eda_ops::Cmd::EmbedLibSymbol { symbol } if symbol.lib_id == "Mine:Chip9000" && symbol.pins.len() == 2));
        assert!(matches!(&cmds[1], eda_ops::Cmd::AddSymbol { value, footprint, .. } if value == "Chip9000" && footprint == "Mine:Pad2"));
        let place = eda_ops::Cmd::PlaceFootprint { footprint: "Mine:Pad2".into(), at: eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO }, reference: String::new(), value: String::new(), definition: None };
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
