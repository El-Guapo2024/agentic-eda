//! Opening the parts cache and reading it back.
//!
//! Every process opens its own connection: a writer ([`Store::open`]) makes the tables and takes the lease before it builds; a reader
//! ([`Store::open_read_only`], `SQLITE_OPEN_READ_ONLY`) only reads, and is what `eda parts sql` and `GET /api/parts/*` use. WAL lets readers read while a
//! library is being written (each library is one transaction, so a reader sees a library whole or not at all), and a 5 s busy timeout waits out another
//! writer's transaction.

use super::schema;
use crate::library_index::Kind;
use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) struct Store {
    pub(crate) conn: Connection,
    pub(crate) path: PathBuf,
}

/// How long a lease lives without a heartbeat before another process may take it (a builder that died).
pub(crate) const LEASE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The SQL function `kicad_wildcard(pattern, text)`: KiCad's anchored `*` / `?` match (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`, the footprint filters' rule,
/// `library_search::wildcard_full`), case-insensitive as the choosers apply it.
fn add_functions(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function("kicad_wildcard", 2, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let pattern: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
        let text: String = ctx.get::<Option<String>>(1)?.unwrap_or_default();
        Ok(crate::library_search::wildcard_full(&pattern.to_lowercase(), &text.to_lowercase()))
    })
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    add_functions(conn)
}

/// One library's row.
#[derive(Debug, Clone, Default)]
pub(crate) struct LibRow {
    pub(crate) id: i64,
    pub(crate) kind: String,
    pub(crate) path: String,
    pub(crate) nickname: String,
    pub(crate) format: String,
    pub(crate) stamp: Option<String>,
    pub(crate) files_stamp: Option<String>,
    pub(crate) indexed_at: Option<i64>,
    pub(crate) item_count: i64,
    pub(crate) load_error: Option<String>,
    pub(crate) verdict_stamp: Option<String>,
    pub(crate) verdict_state: String,
    pub(crate) verdict_message: Option<String>,
}

const LIB_COLUMNS: &str = "id, kind, path, nickname, format, stamp, files_stamp, indexed_at, item_count, load_error, verdict_stamp, verdict_state, verdict_message";

fn lib_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibRow> {
    Ok(LibRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        path: r.get(2)?,
        nickname: r.get(3)?,
        format: r.get(4)?,
        stamp: r.get(5)?,
        files_stamp: r.get(6)?,
        indexed_at: r.get(7)?,
        item_count: r.get(8)?,
        load_error: r.get(9)?,
        verdict_stamp: r.get(10)?,
        verdict_state: r.get(11)?,
        verdict_message: r.get(12)?,
    })
}

impl Store {
    /// The cache at `path` for reading and writing, made if it is not there (its folder too), its tables prepared.
    pub(crate) fn open(path: &Path) -> Result<Store, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let err = |e: rusqlite::Error| format!("{}: {e}", path.display());
        configure(&conn).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(err)?;
        // the schema is checked and made under the write lock, so two processes opening a new file do not both make it
        conn.execute_batch("BEGIN IMMEDIATE").map_err(err)?;
        let prepared = schema::prepare(&conn);
        conn.execute_batch(if prepared.is_ok() { "COMMIT" } else { "ROLLBACK" }).map_err(err)?;
        prepared.map_err(err)?;
        Ok(Store { conn, path: path.to_path_buf() })
    }

    /// The cache at `path`, read only (`SQLITE_OPEN_READ_ONLY`): an error when there is none yet or it is of another schema.
    pub(crate) fn open_read_only(path: &Path) -> Result<Store, String> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        configure(&conn).map_err(|e| format!("{}: {e}", path.display()))?;
        let version = schema::meta(&conn, "schema_version").and_then(|v| v.parse::<i64>().ok());
        if version != Some(schema::SCHEMA_VERSION) {
            return Err(format!("{}: no parts cache of schema {} here (run `eda parts refresh`)", path.display(), schema::SCHEMA_VERSION));
        }
        Ok(Store { conn, path: path.to_path_buf() })
    }

    pub(crate) fn library(&self, kind: Kind, path: &Path) -> Option<LibRow> {
        self.conn
            .query_row(&format!("SELECT {LIB_COLUMNS} FROM libraries WHERE kind = ?1 AND path = ?2"), params![super::kind_name(kind), path.to_string_lossy()], lib_row)
            .optional()
            .ok()
            .flatten()
    }

    /// Every library row, by kind then path.
    pub(crate) fn libraries(&self) -> Vec<LibRow> {
        let Ok(mut st) = self.conn.prepare(&format!("SELECT {LIB_COLUMNS} FROM libraries ORDER BY kind, path")) else { return Vec::new() };
        st.query_map([], lib_row).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    pub(crate) fn generation(&self) -> i64 {
        schema::meta(&self.conn, "generation").and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    /// Take the lease that lets one process build at a time: free, held by `owner` already, or left by a builder that stopped beating (older than
    /// [`LEASE_TIMEOUT`]). False when another builder holds it.
    pub(crate) fn take_lease(&self, owner: &str) -> Result<bool, String> {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let held: Option<(String, i64)> = tx.query_row("SELECT owner, heartbeat FROM lease WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
        let free = match &held {
            None => true,
            Some((o, beat)) => o == owner || now_secs() - beat > LEASE_TIMEOUT.as_secs() as i64,
        };
        if free {
            tx.execute(
                "INSERT INTO lease (id, owner, pid, heartbeat) VALUES (1, ?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET owner = excluded.owner, pid = excluded.pid, heartbeat = excluded.heartbeat",
                params![owner, std::process::id(), now_secs()],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(free)
    }

    pub(crate) fn beat(&self, owner: &str) {
        let _ = self.conn.execute("UPDATE lease SET heartbeat = ?2 WHERE id = 1 AND owner = ?1", params![owner, now_secs()]);
    }

    pub(crate) fn release_lease(&self, owner: &str) {
        let _ = self.conn.execute("DELETE FROM lease WHERE id = 1 AND owner = ?1", [owner]);
    }

    /// Who builds now, if anyone: `(owner, pid, seconds since its last heartbeat)`.
    pub(crate) fn lease_holder(&self) -> Option<(String, i64, i64)> {
        self.conn.query_row("SELECT owner, pid, heartbeat FROM lease WHERE id = 1", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, now_secs() - r.get::<_, i64>(2)?))).optional().ok().flatten()
    }
}

/// A process-wide reader for the studio's own lookups (`library_search`'s phase 1b, the `/api/parts` routes), opened on first use; `None` while there is no
/// cache to read. One connection behind a mutex: a lookup is a few milliseconds.
pub(crate) fn with_reader<T>(f: impl FnOnce(&Store) -> T) -> Option<T> {
    use std::sync::{Mutex, OnceLock};
    static READER: OnceLock<Mutex<Option<(PathBuf, Store)>>> = OnceLock::new();
    let cell = READER.get_or_init(Default::default);
    let mut guard = cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = super::db_path();
    if guard.as_ref().is_none_or(|(p, _)| *p != path) {
        if !path.is_file() {
            return None;
        }
        *guard = Some((path.clone(), Store::open_read_only(&path).ok()?));
    }
    guard.as_ref().map(|(_, s)| f(s))
}
