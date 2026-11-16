//! The parts cache's tables. Lengths are nanometres (KiCad's internal unit), booleans 0/1, lists JSON. Every row of a library hangs off its `libraries` row
//! (`ON DELETE CASCADE`), so a library read again is one delete and its inserts.
//!
//! A cache, not a store of record: a database whose `schema_version` is not this one is emptied and built again ([`prepare`]), and one whose
//! `scanner_version` is not this one has every library marked unread, so the next refresh reads them all.

use rusqlite::{params, Connection, OptionalExtension};

/// The layout of the tables; another one is rebuilt from nothing.
pub(crate) const SCHEMA_VERSION: i64 = 1;

/// What the scanners pick out of the files (`eda_kicad::scan_symbols`, `scan_symbol_pins`, `scan_footprint_detail`, the model lookup); another one reads
/// every library again.
pub(crate) const SCANNER_VERSION: i64 = 1;

const TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- one process builds at a time (`store::Store::take_lease`)
CREATE TABLE IF NOT EXISTS lease (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    owner TEXT NOT NULL,
    pid INTEGER NOT NULL,
    heartbeat INTEGER NOT NULL
);

-- a library is its kind and its path (a table row's resolved path; a converted or HTTP library's readable copy)
CREATE TABLE IF NOT EXISTS libraries (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,                    -- symbol | footprint
    path TEXT NOT NULL,
    nickname TEXT NOT NULL DEFAULT '',     -- the nickname the last refresh found it under
    format TEXT NOT NULL DEFAULT 'kicad',  -- kicad | converted (a copy kicad-cli made of another format's library)
    stamp TEXT,                            -- the shallow stamp: a symbol library's modification time (a folder's latest), a .pretty folder's
    files_stamp TEXT,                      -- the deep stamp: every file's name, size and time
    indexed_at INTEGER,                    -- unix seconds
    item_count INTEGER NOT NULL DEFAULT 0,
    load_error TEXT,                       -- this crate's reader's message for a library that reads with errors
    reader_refused_json TEXT NOT NULL DEFAULT '[]', -- footprints this crate's reader refuses
    verdict_stamp TEXT,                    -- the files_stamp KiCad's parser's verdict is for
    verdict_state TEXT NOT NULL DEFAULT 'unknown', -- unknown | ok | some_refused | failed
    verdict_message TEXT,                  -- the library's load message once KiCad's refusals are in it
    verdict_at INTEGER,
    UNIQUE (kind, path)
);

CREATE TABLE IF NOT EXISTS files (
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    rel TEXT NOT NULL,
    mtime INTEGER NOT NULL,                -- nanoseconds
    size INTEGER NOT NULL,
    PRIMARY KEY (library_id, rel)
) WITHOUT ROWID;

-- the rows of the library tables a refresh read: scope `global` or `project:<board folder>`
CREATE TABLE IF NOT EXISTS table_rows (
    scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    nickname TEXT NOT NULL,
    library_id INTEGER REFERENCES libraries(id) ON DELETE SET NULL,
    path TEXT NOT NULL,
    table_path TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    hidden INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (scope, kind, nickname)
);

CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    lib_nick_hint TEXT NOT NULL,
    name TEXT NOT NULL,
    extends TEXT,
    reference TEXT NOT NULL,
    value TEXT NOT NULL,
    footprint TEXT NOT NULL,
    datasheet TEXT NOT NULL,
    description TEXT NOT NULL,
    keywords TEXT NOT NULL,
    fp_filters TEXT NOT NULL,
    power INTEGER NOT NULL,
    units INTEGER NOT NULL,                -- the highest unit drawn (0: draws nothing itself); a chooser offers max(units, 1)
    pin_count INTEGER NOT NULL,            -- the distinct pin numbers of the first body style (`SymbolSummary::pin_count`)
    pin_numbers TEXT NOT NULL,             -- those numbers, sorted, one per line
    alternate_body_style INTEGER NOT NULL,
    user_fields_json TEXT NOT NULL,        -- [[key, value], ...] in file order
    span_start INTEGER NOT NULL,           -- the symbol's form in the library's text (a folder library's combined text)
    span_end INTEGER NOT NULL,
    file TEXT NOT NULL,                    -- the file it is in
    kicad_refused INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS symbols_library ON symbols(library_id);
CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);

CREATE TABLE IF NOT EXISTS symbol_pins (
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    unit INTEGER NOT NULL,                 -- 0: every unit
    body_style INTEGER NOT NULL,           -- 1 normal, 2 DeMorgan, 0 both
    number TEXT NOT NULL,
    name TEXT NOT NULL,
    electrical_type TEXT NOT NULL,
    shape TEXT NOT NULL,
    hidden INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS symbol_pins_symbol ON symbol_pins(symbol_id);

CREATE TABLE IF NOT EXISTS footprints (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    lib_nick_hint TEXT NOT NULL,
    name TEXT NOT NULL,
    file_mtime INTEGER NOT NULL,
    file_size INTEGER NOT NULL,
    description TEXT NOT NULL,
    tags TEXT NOT NULL,
    attr TEXT NOT NULL,                    -- smd | through_hole | unspecified
    pads INTEGER NOT NULL,                 -- every (pad ..)
    numbered_pads INTEGER NOT NULL,        -- FOOTPRINT::GetNumberedPadCount
    smd_pads INTEGER NOT NULL,
    tht_pads INTEGER NOT NULL,
    npth_pads INTEGER NOT NULL,
    pitch_nm INTEGER NOT NULL,             -- nearest two numbered copper pads of different numbers, to the micrometre; 0 for fewer than two
    courtyard_w_nm INTEGER NOT NULL,       -- the F.CrtYd box, else the B.CrtYd one
    courtyard_h_nm INTEGER NOT NULL,
    has_courtyard INTEGER NOT NULL,
    model_paths_json TEXT NOT NULL,
    model_found INTEGER NOT NULL,          -- it names a model and every model it names is found
    exclude_from_bom INTEGER NOT NULL,
    exclude_from_pos INTEGER NOT NULL,
    kicad_refused INTEGER NOT NULL DEFAULT 0,
    reader_error TEXT                      -- the file did not read or is no footprint (kept by its name, as the choosers keep it)
);
CREATE INDEX IF NOT EXISTS footprints_library ON footprints(library_id);
CREATE INDEX IF NOT EXISTS footprints_name ON footprints(name);

CREATE TABLE IF NOT EXISTS footprint_pads (
    footprint_id INTEGER NOT NULL REFERENCES footprints(id) ON DELETE CASCADE,
    number TEXT NOT NULL,
    type TEXT NOT NULL,                    -- smd | thru_hole | np_thru_hole | connect
    shape TEXT NOT NULL,
    x INTEGER NOT NULL,
    y INTEGER NOT NULL,
    rot REAL NOT NULL,
    w INTEGER NOT NULL,
    h INTEGER NOT NULL,
    drill INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS footprint_pads_footprint ON footprint_pads(footprint_id);

-- substring search (a prefilter of the chooser scoring), rowid = symbols.id / footprints.id
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(name, keywords, description, footprint, tokenize = 'trigram');
CREATE VIRTUAL TABLE IF NOT EXISTS footprints_fts USING fts5(name, tags, description, tokenize = 'trigram');

-- phase 2 (supplier data): the tables, empty until the fetchers come
CREATE TABLE IF NOT EXISTS parts (
    id INTEGER PRIMARY KEY,
    mpn TEXT NOT NULL,
    manufacturer TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    package TEXT NOT NULL DEFAULT '',
    lifecycle TEXT NOT NULL DEFAULT '',
    datasheet TEXT NOT NULL DEFAULT '',
    params_json TEXT NOT NULL DEFAULT '{}',
    updated_at INTEGER,
    UNIQUE (manufacturer, mpn)
);
CREATE TABLE IF NOT EXISTS part_params (
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value_text TEXT NOT NULL DEFAULT '',
    value_num REAL,
    unit TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS part_links (
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,                    -- symbol | footprint
    lib_id TEXT NOT NULL,                  -- 'Lib:Name', never a row id
    source TEXT NOT NULL,                  -- symbol_field | user | jlc_map | footprint_match
    confidence REAL NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS offers (
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    supplier TEXT NOT NULL,
    sku TEXT NOT NULL,
    stock INTEGER,
    moq INTEGER,
    price_breaks_json TEXT NOT NULL DEFAULT '[]',
    currency TEXT NOT NULL DEFAULT '',
    fetched_at INTEGER,
    url TEXT NOT NULL DEFAULT '',
    UNIQUE (supplier, sku)
);
"#;

/// Every table the cache drops when it is rebuilt (the schema's, in an order the foreign keys allow).
const ALL: [&str; 15] = [
    "offers", "part_links", "part_params", "parts", "symbols_fts", "footprints_fts", "symbol_pins", "symbols", "footprint_pads", "footprints", "table_rows", "files", "libraries",
    "lease", "meta",
];

pub(crate) fn meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value]).map(|_| ())
}

/// Make the tables, rebuild a database of another schema, and mark every library unread for another scanner. Run by a writer only.
pub(crate) fn prepare(conn: &Connection) -> rusqlite::Result<()> {
    let has_meta: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'", [], |r| r.get::<_, i64>(0))? > 0;
    let version = if has_meta { meta(conn, "schema_version").and_then(|v| v.parse::<i64>().ok()) } else { None };
    if has_meta && version != Some(SCHEMA_VERSION) {
        for t in ALL {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS {t};"))?;
        }
    }
    conn.execute_batch(TABLES)?;
    if meta(conn, "schema_version").is_none() {
        set_meta(conn, "schema_version", &SCHEMA_VERSION.to_string())?;
        set_meta(conn, "generation", "0")?;
    }
    if meta(conn, "scanner_version").and_then(|v| v.parse::<i64>().ok()) != Some(SCANNER_VERSION) {
        conn.execute("UPDATE libraries SET stamp = NULL, files_stamp = NULL", [])?;
        set_meta(conn, "scanner_version", &SCANNER_VERSION.to_string())?;
    }
    Ok(())
}
