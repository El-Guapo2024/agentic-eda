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
    pub(crate) project_dir: PathBuf,
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
    let project_vars = lt::read_project_variables(dir);
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
    Loaded { env, project_dir: dir.to_path_buf(), project_vars, vars, kind, global, global_origin: origin, global_error, project_path, project, project_error }
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

    pub(crate) fn entries(&self) -> &[LibEntry] {
        &self.entries
    }
}

/// How many footprints a library folder holds, read again only when the folder changes.
fn footprint_count(dir: &Path) -> usize {
    static COUNTS: OnceLock<Mutex<HashMap<PathBuf, (Option<SystemTime>, usize)>>> = OnceLock::new();
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

#[cfg(test)]
mod tests {
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
}
