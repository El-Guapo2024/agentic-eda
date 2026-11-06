//! The library editors' watch on the library their item came from (UI-LEFT-2 LL3): a library file changed on disk by something else than the studio is
//! offered for reloading, as KiCad's editors offer it.
//!
//! KiCad watches the library of the open item: the Symbol Editor its symbol's library file or folder (`SCH_BASE_FRAME::setSymWatcher`, set when a symbol is
//! loaded, `SYMBOL_EDIT_FRAME::LoadSymbol` / `SetCurSymbol`), the Footprint Editor its footprint's `.kicad_mod` (`PCB_BASE_FRAME::setFPWatcher`, set when a
//! footprint is loaded and again after a save, `AddFootprintToBoard`, `SaveFootprintInLibrary`). A `wxFileSystemWatcher` event starts a one-second debounce
//! timer (`OnSymChange`, `OnFPChange`); when it fires, the file's modification time -- a folder library's latest, `KIPLATFORM::IO::TimestampDir` -- is
//! compared with the one kept, and when it differs (and is valid: a file that is gone is not reloaded) the editor reloads at once if its content is not
//! modified, else asks: "The library containing the current symbol has changed.\nDo you want to reload the library?" (`OnSymChangeDebounceTimer` ->
//! `MAIL_REFRESH_SYMBOL` -> `UpdateLibraryBuffer`) or "The library containing the current footprint has changed.\nDo you want to reload the footprint?"
//! (`OnFpChangeDebounceTimer` -> `LoadFootprint` -> `ReloadFootprint`). No keeps the editor as it is, and the new time is kept, so the same change is not
//! asked about again. While a dialog is open the reload waits (`!IsEnabled()`: "restarting debounce timer").
//!
//! The studio's server watches by polling the modification times, the way `wxPollingFileSystemWatcher` and the debounce timer's own `GetModificationTime`
//! do (no file-notification crate is a dependency): the page asks [`watch`] every second for the item it shows, keeps the first answer as the watched time and
//! compares (`web/studio/src/state/libraryWatch.ts`). A file the studio wrote itself (Save, Save As, Save Library As) is no change of anyone else's:
//! [`note_written`] keeps the time it left, and the answer says `own` for it, where KiCad resets the watch after its own save (`setFPWatcher` after
//! `SaveFootprintInLibrary`; a saved symbol library reloads unasked, its screen being saved).

use crate::library_index::Kind;
use crate::library_save_api as lsa;
use crate::library_tables;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

fn split(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

/// A modification time as the page compares it (nanoseconds since 1970), or none for a path that is not there.
fn as_stamp(t: Option<SystemTime>) -> Option<String> {
    t.map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_string())
}

/// What the watch compares for `path`: a symbol library's modification time (`GetModificationTime`, a folder's latest of itself and its `*.kicad_sym` files,
/// `TimestampDir`), a footprint file's.
pub(crate) fn stamp(kind: Kind, path: &Path) -> Option<String> {
    match kind {
        Kind::Symbol => as_stamp(eda_kicad::symbol_library_modified(path)),
        Kind::Footprint => as_stamp(std::fs::metadata(path).ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok())),
    }
}

fn own_writes() -> &'static Mutex<HashMap<PathBuf, String>> {
    static OWN: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    OWN.get_or_init(Default::default)
}

/// The studio wrote the library at `path` (a symbol library's file or folder, a footprint library's `.pretty` folder): the times its files have now are its
/// own, not a change to offer for reloading.
pub(crate) fn note_written(kind: Kind, path: &Path) {
    let mut own = own_writes().lock().unwrap_or_else(PoisonError::into_inner);
    match kind {
        Kind::Symbol => {
            if let Some(s) = stamp(kind, path) {
                own.insert(path.to_path_buf(), s);
            }
        }
        Kind::Footprint => {
            for file in std::fs::read_dir(path).into_iter().flatten().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_mod")) {
                if let Some(s) = stamp(kind, &file) {
                    own.insert(file, s);
                }
            }
        }
    }
}

/// The path watched for the editor's item `id` (`Lib:Name`): its library's file or folder for a symbol (`setSymWatcher` of the symbol's `LIB_ID`), its
/// `.kicad_mod` for a footprint (`m_watcherFileName.Assign( libfullname, name, KiCadFootprintFileExtension )`). None for the project library's items and a
/// footprint loaded from the board (`IsCurrentFPFromBoard()`: `setFPWatcher( nullptr )`), which have no file, and for a library the tables do not have.
pub(crate) fn watched_path(dir: &Path, kind: Kind, id: &str) -> Option<PathBuf> {
    if !eda_ops::library_editors::is_table_library_item(id) {
        return None;
    }
    let (lib, item) = split(id);
    let entry = library_tables::libs(dir, kind).find(lib).cloned()?;
    Some(match kind {
        Kind::Symbol => entry.path,
        Kind::Footprint => entry.path.join(eda_kicad::library_save::footprint_file_name(item)),
    })
}

/// `GET /api/library/watch?kind=&id=` -- the watched library of the editor's item `id` as it is now: `{"path", "stamp", "own", "steady", "modified", "dirty"}`.
/// `stamp` is null when there is nothing to compare (no file); `own` when the studio wrote it last ([`note_written`]); `steady` when the time did not move
/// while the answer was made (so `modified`, which reads the file, was read from the file `stamp` stands for); `modified` is the item's modified flag or text
/// difference (`crate::library_save_api::item_modified`), `dirty` the flag alone (`IsContentModified()`). `{"path": null}` for an item with no file.
pub(crate) fn watch(dir: &Path, kind: &str, id: &str) -> Value {
    let Some(kind) = Kind::parse(kind) else { return json!({ "error": "kind is footprint or symbol" }) };
    let Some(path) = watched_path(dir, kind, id) else { return json!({ "path": null }) };
    let before = stamp(kind, &path);
    let sections = crate::library_buffer::read_sections(dir).unwrap_or_default();
    let modified = lsa::item_modified(dir, kind, id, sections.symbol_library.as_ref(), sections.footprint_library.as_ref());
    let after = stamp(kind, &path);
    let own = after.as_ref().is_some_and(|s| own_writes().lock().unwrap_or_else(PoisonError::into_inner).get(&path) == Some(s));
    json!({
        "path": path.display().to_string(),
        "stamp": after,
        "own": own,
        "steady": before == after,
        "modified": modified,
        "dirty": crate::library_buffer::is_dirty(dir, kind, id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::{scratch, setup};

    #[test]
    fn a_library_written_outside_the_studio_moves_the_watched_time_and_one_the_studio_wrote_is_its_own() {
        let dir = scratch("library_watch");
        setup(&dir);
        let added = library_tables::add_library(&dir, br#"{"kind":"symbol","path":"${KIPRJMOD}/libs/Mine","create":true}"#);
        assert_eq!(added["ok"], true, "{added}");
        let lib = dir.join("libs").join("Mine.kicad_sym");
        std::fs::write(&lib, "(kicad_symbol_lib (version 20251024)\n\t(symbol \"R\" (property \"Reference\" \"R\") (symbol \"R_1_1\"))\n)\n").unwrap();
        // the project library and a footprint from the board have nothing to watch
        assert_eq!(watch(&dir, "symbol", "eda:R")["path"], Value::Null);
        assert_eq!(watch(&dir, "footprint", "<board>/R1")["path"], Value::Null);
        let first = watch(&dir, "symbol", "Mine:R");
        assert_eq!(first["path"], lib.display().to_string());
        assert_eq!((first["own"].as_bool(), first["steady"].as_bool(), first["modified"].as_bool()), (Some(false), Some(true), Some(false)));
        // a write from outside: a new time, not the studio's own
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&lib, "(kicad_symbol_lib (version 20251024)\n\t(symbol \"R\" (property \"Reference\" \"Q\") (symbol \"R_1_1\"))\n)\n").unwrap();
        let changed = watch(&dir, "symbol", "Mine:R");
        assert_ne!(changed["stamp"], first["stamp"]);
        assert_eq!(changed["own"], false);
        // the studio's own write is said so
        note_written(Kind::Symbol, &lib);
        assert_eq!(watch(&dir, "symbol", "Mine:R")["own"], true);
        // a footprint's file: gone is no stamp (KiCad: `!lastModified.IsValid()`, nothing reloaded)
        let pretty = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Fp.pretty","create":true}"#);
        assert_eq!(pretty["ok"], true, "{pretty}");
        assert_eq!(watch(&dir, "footprint", "Fp:X")["stamp"], Value::Null);
        std::fs::write(dir.join("libs").join("Fp.pretty").join("X.kicad_mod"), "(footprint \"X\" (layer \"F.Cu\"))\n").unwrap();
        assert!(watch(&dir, "footprint", "Fp:X")["stamp"].is_string());
    }
}
