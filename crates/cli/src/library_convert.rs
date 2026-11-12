//! Libraries of other formats, read through kicad-cli: a row of a library table whose type is Legacy, Eagle, Altium, CADSTAR, EasyEDA, PADS Logic (symbols) or
//! Legacy, Eagle, Altium Designer, CADSTAR, EasyEDA, gEDA, Sprint Layout (footprints) -- `eda_kicad::library_table::convertible` -- and Migrate Libraries.
//!
//! KiCad reads such a library with the plugin its row names (`SYMBOL_LIBRARY_ADAPTER::createPlugin`, `SCH_IO_MGR::FindPlugin`; the footprint side alike): the Eagle
//! plugin enumerates an `.lbr` and loads each symbol from it, and the trees, the choosers and placing see what it loaded. Those plugins are KiCad's and are not ported
//! (the port-vs-reuse rule): kicad-cli runs them. `kicad-cli sym upgrade IN -o OUT` / `fp upgrade IN -o OUT` is `SCH_IO_MGR::ConvertLibrary` /
//! `PCB_IO_MGR::ConvertLibrary` -- every item the plugin enumerates, loaded and written by KiCad's own writer -- so the library is read as the KiCad-format copy it
//! makes, and everything that reads a library of the tables (the index, the trees, the choosers, placing, the lookups) reads that copy as it reads any other.
//!
//! **The copy is a cache, not a library of the person's.** It is kept under the studio's cache folder (`$XDG_CACHE_HOME` or `~/.cache`, `~/Library/Caches` on a
//! Mac: `eda-studio/libraries/`, or `EDA_LIBRARY_CACHE`), named after the source's path and stamped with its size and time, so a source that changes is converted
//! again and the copy of its earlier state is removed; it is made read only, so no save writes into it (KiCad's foreign plugins are read only too:
//! `IsLibraryWritable` is false, "Library '%s' is read only."; a `.pretty` copy's folder is read only too, so removing the cache by hand takes a `chmod -R u+w`
//! first). A conversion that fails is remembered for that state of the source (the row's status says why), and
//! tried again when the source changes or the server starts again. Nothing is written beside the source.
//!
//! **Migrate Libraries** (`PANEL_SYM_LIB_TABLE::onConvertLegacyLibraries`, `PANEL_FP_LIB_TABLE::onMigrateLibraries`) is the same conversion with a destination the
//! person keeps: the `.kicad_sym` beside the old file (`newLib.SetExt( "kicad_sym" )`), or the `<name>.pretty` folder beside it (`newLib.AppendDir( newLib.GetName()
//! + ".pretty" )`) -- the folder the table already names, with a new extension; see [`migrate`].

use eda_kicad::library_table::{self as lt, TableKind};
use eda_kicad_engine::foreign::{convert_library, LibraryKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// Moves the cache of converted libraries (a test's own folder).
pub(crate) const CACHE_ENV: &str = "EDA_LIBRARY_CACHE";

/// What converts a library: kicad-cli, or a test's stand-in. `Err` is the reason in words.
pub(crate) type Convert<'a> = &'a dyn Fn(LibraryKind, &Path, &Path) -> Result<(), String>;

fn kicad_cli(kind: LibraryKind, from: &Path, to: &Path) -> Result<(), String> {
    convert_library(kind, from, to).map_err(|e| e.first().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).unwrap_or_default())
}

fn library_kind(kind: TableKind) -> Option<LibraryKind> {
    match kind {
        TableKind::Symbol => Some(LibraryKind::Symbol),
        TableKind::Footprint => Some(LibraryKind::Footprint),
        TableKind::DesignBlock => None,
    }
}

/// The extension of the KiCad library a conversion makes.
fn kicad_ext(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Symbol => eda_kicad::SYMBOL_LIB_FILE_EXT,
        LibraryKind::Footprint => "pretty",
    }
}

/// The cache folder: `EDA_LIBRARY_CACHE`, else `<the user's cache folder>/eda-studio/libraries` (the 3D model cache's base, `model3d_api::cache_dir`).
pub(crate) fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os(CACHE_ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    crate::model3d_api::cache_dir().parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir).join("libraries")
}

/// The state of a source library: its size and time, and for a folder (a gEDA library) those of each file in it.
fn stamp(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let time = |m: &std::fs::Metadata| m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    let mut text = format!("{}@{}", meta.len(), time(&meta));
    if meta.is_dir() {
        let mut files: Vec<(String, String)> = std::fs::read_dir(path)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), format!("{}@{}", m.len(), time(&m))))
            })
            .collect();
        files.sort();
        for (n, s) in files {
            text.push_str(&format!("|{n}={s}"));
        }
    }
    Some(text)
}

fn hash16(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// Make what a conversion wrote read only: the file, or the folder and its files.
fn make_read_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.is_dir() {
            for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
                let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o444));
            }
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o555));
        } else {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444));
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(meta) = std::fs::metadata(path) {
            let mut p = meta.permissions();
            p.set_readonly(true);
            let _ = std::fs::set_permissions(path, p);
        }
    }
}

/// Remove a file or a folder this module made read only.
fn remove(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.is_dir() {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
        }
    }
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

type Memo = Mutex<HashMap<(PathBuf, LibraryKind), (String, Result<PathBuf, String>)>>;

fn memo() -> &'static Memo {
    static MEMO: OnceLock<Memo> = OnceLock::new();
    MEMO.get_or_init(Default::default)
}

/// The KiCad-format copy of the library at `source` (a library of a [`lt::convertible`] format), converting it when the cache has no copy of its present state.
/// `Err` says why it cannot be read: not there, or what kicad-cli said.
pub(crate) fn readable(kind: TableKind, source: &Path) -> Result<PathBuf, String> {
    readable_with(kind, source, &cache_dir(), &kicad_cli)
}

/// [`readable`] of a row of the tables, with the row's format and options (UI-LEFT-2 LL5). KiCad hands a row's options to its plugin (`LIBRARY_TABLE_ROW::
/// GetProperties`); of the options a library read takes, only the CADSTAR symbol plugin's two exist (`SCH_IO_CADSTAR_ARCHIVE::GetLibraryOptions`; the footprint
/// plugins' library functions read none -- `PCB_IO::GetLibraryOptions`' six are offered and never read): `csa`, the CADSTAR schematic archive the library
/// goes with (a relative path against the library's folder), and `fplib`, the nickname of the footprint library its symbols' footprints are in
/// (`cadstarpcblib` when none). kicad-cli takes no options, so the studio gives them to it the way the plugin would find them: the library is converted from a
/// folder of its own where it sits beside the named archive under its own name (`<library>.csa`, the first place `ensureLoadedLibrary` looks), and the
/// converted symbols' footprints are named in `fplib` instead of `cadstarpcblib` (`LIB_ID( m_footprintLibName, .. )`). The options are part of the cached
/// copy's state.
pub(crate) fn readable_row(kind: TableKind, source: &Path, lib_type: &str, options: &str) -> Result<PathBuf, String> {
    let opts = lt::parse_options(options);
    if kind != TableKind::Symbol || lib_type != CADSTAR_SYMBOL_TYPE || !(opts.contains_key("csa") || opts.contains_key("fplib")) {
        return readable(kind, source);
    }
    let staged = |lk: LibraryKind, from: &Path, to: &Path| cadstar_with_options(lk, from, to, &opts, &kicad_cli);
    readable_keyed(kind, source, &cache_dir(), &staged, options)
}

/// `SCH_IO_MGR::SCH_CADSTAR_ARCHIVE`'s table type.
const CADSTAR_SYMBOL_TYPE: &str = "CADSTAR Schematic Archive";

/// The CADSTAR symbol library at `from` converted into `to` as KiCad's plugin reads it with the row's `csa` and `fplib` options (see [`readable_row`]).
fn cadstar_with_options(lk: LibraryKind, from: &Path, to: &Path, opts: &std::collections::BTreeMap<String, String>, convert: Convert<'_>) -> Result<(), String> {
    let mut source = from.to_path_buf();
    let mut stage: Option<PathBuf> = None;
    if let Some(csa) = opts.get("csa").filter(|c| !c.is_empty()) {
        let csa = Path::new(csa);
        let csa = if csa.is_absolute() { csa.to_path_buf() } else { from.parent().unwrap_or(Path::new(".")).join(csa) };
        let dir = std::env::temp_dir().join(format!("eda-cadstar-{}-{}", std::process::id(), hash16(&format!("{}{:?}", from.display(), SystemTime::now()))));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = from.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        let stem = from.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::copy(from, dir.join(&name)).map_err(|e| e.to_string())?;
        std::fs::copy(&csa, dir.join(format!("{stem}.csa"))).map_err(|_| format!("Cannot find the .csa file corresponding to library '{}'.", from.display()))?;
        source = dir.join(&name);
        stage = Some(dir);
    }
    let converted = convert(lk, &source, to);
    if let Some(dir) = stage {
        let _ = std::fs::remove_dir_all(dir);
    }
    converted?;
    if let Some(fplib) = opts.get("fplib").filter(|f| !f.is_empty() && f.as_str() != "cadstarpcblib") {
        let text = std::fs::read_to_string(to).map_err(|e| e.to_string())?;
        std::fs::write(to, text.replace("(property \"Footprint\" \"cadstarpcblib:", &format!("(property \"Footprint\" \"{fplib}:"))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn readable_with(kind: TableKind, source: &Path, cache: &Path, convert: Convert<'_>) -> Result<PathBuf, String> {
    readable_keyed(kind, source, cache, convert, "")
}

/// [`readable_with`], the copy kept for the source's state and `options` together.
fn readable_keyed(kind: TableKind, source: &Path, cache: &Path, convert: Convert<'_>, options: &str) -> Result<PathBuf, String> {
    let Some(lk) = library_kind(kind) else { return Err("A design block library is read only in KiCad's format.".to_string()) };
    let Some(mut state) = stamp(source) else { return Err(format!("Library '{}' not found.", source.display())) };
    if !options.is_empty() {
        state.push_str(&format!("|options={options}"));
    }
    let key = (source.to_path_buf(), lk);
    if let Some((s, answer)) = memo().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        // the same state of the source: the same answer (a copy someone removed from the cache is made again)
        if *s == state && (answer.is_err() || answer.as_ref().is_ok_and(|p| p.exists())) {
            return answer.clone();
        }
    }
    let stem: String = source.file_stem().map(|s| s.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()).unwrap_or_default();
    let family = format!("{stem}-{}-", hash16(&format!("{lk:?}|{}", source.display())));
    let target = cache.join(format!("{family}{}.{}", hash16(&state), kicad_ext(lk)));
    let answer = if target.exists() {
        Ok(target.clone())
    } else {
        convert_into(lk, source, cache, &target, convert)
    };
    if answer.is_ok() {
        // the copies of the source's earlier states go
        for e in std::fs::read_dir(cache).into_iter().flatten().flatten() {
            let p = e.path();
            if p != target && e.file_name().to_string_lossy().starts_with(&family) {
                remove(&p);
            }
        }
    }
    memo().lock().unwrap_or_else(PoisonError::into_inner).insert(key, (state, answer.clone()));
    answer
}

/// Convert `source` into a sibling of `target` and move it there (kicad-cli refuses an output that exists, and a reader must never see half a library).
fn convert_into(lk: LibraryKind, source: &Path, cache: &Path, target: &Path, convert: Convert<'_>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(cache).map_err(|e| format!("Cannot create the library cache {}: {e}", cache.display()))?;
    let tmp = cache.join(format!(".converting-{}-{}.{}", std::process::id(), hash16(&format!("{}{:?}", target.display(), SystemTime::now())), kicad_ext(lk)));
    remove(&tmp);
    convert(lk, source, &tmp)?;
    make_read_only(&tmp);
    std::fs::rename(&tmp, target).map_err(|e| {
        remove(&tmp);
        format!("Cannot keep the converted library {}: {e}", target.display())
    })?;
    Ok(target.to_path_buf())
}

/// What the row of a converted library says in the table dialog: `None` once it reads, else the reason. Only what has been tried is told (the dialog does not
/// start a conversion: the libraries' first reader does).
pub(crate) fn status(kind: TableKind, source: &Path) -> Option<String> {
    let lk = library_kind(kind)?;
    let state = stamp(source)?;
    match memo().lock().unwrap_or_else(PoisonError::into_inner).get(&(source.to_path_buf(), lk)) {
        Some((s, Err(e))) if *s == state || s.starts_with(&format!("{state}|options=")) => Some(e.clone()),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------ Migrate Libraries

/// Where Migrate Libraries puts the KiCad library made of `source`: `PANEL_SYM_LIB_TABLE::onConvertLegacyLibraries`' `newLib.SetExt( "kicad_sym" )` (the file
/// beside the old one, its extension changed), `PANEL_FP_LIB_TABLE::onMigrateLibraries`' `newLib.AppendDir( newLib.GetName() + ".pretty" ); newLib.SetName( "" )`
/// (a folder beside the old file, named after it).
pub(crate) fn migrated_path(kind: TableKind, source: &Path) -> Option<PathBuf> {
    match kind {
        TableKind::Symbol => Some(source.with_extension(eda_kicad::SYMBOL_LIB_FILE_EXT)),
        TableKind::Footprint => {
            let name = source.file_stem()?.to_string_lossy().into_owned();
            Some(source.parent()?.join(format!("{name}.pretty")))
        }
        TableKind::DesignBlock => None,
    }
}

/// Migrate one library: convert `source` into `dest`, which may exist only when the person said to overwrite it. A symbol library replaces the file (KiCad's
/// `ConvertLibrary` saves the new library over it); a footprint library's footprints go into the folder, replacing the ones of the same name and leaving the
/// others (`PCB_IO_MGR::ConvertLibrary` saves each footprint into the folder: "Do you want overwrite any existing footprints?").
pub(crate) fn migrate(kind: TableKind, source: &Path, dest: &Path) -> Result<(), String> {
    migrate_with(kind, source, dest, &kicad_cli)
}

/// [`migrate`] of a row with its format and options: `SCH_IO_MGR::ConvertLibrary( &props, .. )` hands the row's options to the plugin, which for a CADSTAR
/// symbol library are read as [`readable_row`] reads them (UI-LEFT-2 LL5).
pub(crate) fn migrate_row(kind: TableKind, source: &Path, dest: &Path, lib_type: &str, options: &str) -> Result<(), String> {
    let opts = lt::parse_options(options);
    if kind == TableKind::Symbol && lib_type == CADSTAR_SYMBOL_TYPE && (opts.contains_key("csa") || opts.contains_key("fplib")) {
        return migrate_with(kind, source, dest, &|lk, from, to| cadstar_with_options(lk, from, to, &opts, &kicad_cli));
    }
    migrate(kind, source, dest)
}

pub(crate) fn migrate_with(kind: TableKind, source: &Path, dest: &Path, convert: Convert<'_>) -> Result<(), String> {
    let Some(lk) = library_kind(kind) else { return Err("Design block libraries have no other format to migrate from.".to_string()) };
    if !source.exists() {
        return Err(format!("Library '{}' not found.", source.display()));
    }
    let folder = dest.parent().ok_or_else(|| format!("{} has no folder.", dest.display()))?;
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = folder.join(format!(".{name}.migrating-{}", std::process::id()));
    if tmp.is_dir() {
        let _ = std::fs::remove_dir_all(&tmp);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
    convert(lk, source, &tmp)?;
    let moved = match lk {
        LibraryKind::Symbol => std::fs::rename(&tmp, dest).map_err(|e| e.to_string()),
        LibraryKind::Footprint if !dest.exists() => std::fs::rename(&tmp, dest).map_err(|e| e.to_string()),
        LibraryKind::Footprint => (|| {
            for e in std::fs::read_dir(&tmp)?.flatten() {
                std::fs::rename(e.path(), dest.join(e.file_name()))?;
            }
            std::fs::remove_dir_all(&tmp)
        })()
        .map_err(|e| e.to_string()),
    };
    moved.map_err(|e| {
        if tmp.is_dir() {
            let _ = std::fs::remove_dir_all(&tmp);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
        match lk {
            LibraryKind::Symbol => format!("Failed to save symbol library file '{}'. {e}", dest.display()),
            LibraryKind::Footprint => format!("Failed to save footprint library file '{}'. {e}", dest.display()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-libconv-{tag}-{}", std::process::id()));
        remove(&d);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A converter that writes a one-symbol library naming the source's text, and counts its runs.
    fn fake(runs: &Cell<u32>) -> impl Fn(LibraryKind, &Path, &Path) -> Result<(), String> + '_ {
        move |lk, from, to| {
            runs.set(runs.get() + 1);
            let text = std::fs::read_to_string(from).map_err(|e| e.to_string())?;
            if text.contains("garbage") {
                return Err("Unrecognized library type Unable to convert library".into());
            }
            match lk {
                LibraryKind::Symbol => std::fs::write(to, format!("(kicad_symbol_lib (version 20241209) (generator \"t\") (symbol \"{}\"))", text.trim())).map_err(|e| e.to_string()),
                LibraryKind::Footprint => {
                    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
                    std::fs::write(to.join(format!("{}.kicad_mod", text.trim())), "(footprint \"x\")").map_err(|e| e.to_string())
                }
            }
        }
    }

    #[test]
    fn a_foreign_library_is_read_as_its_converted_copy_once_per_state_and_the_copy_is_read_only() {
        let root = temp("read");
        let cache = root.join("cache");
        let lib = root.join("parts.lib");
        std::fs::write(&lib, "R1").unwrap();
        let runs = Cell::new(0);
        let conv = fake(&runs);
        let a = readable_with(TableKind::Symbol, &lib, &cache, &conv).unwrap();
        assert!(a.starts_with(&cache) && a.extension().unwrap() == "kicad_sym");
        assert!(std::fs::read_to_string(&a).unwrap().contains("\"R1\""));
        assert!(!eda_kicad::library_save::is_symbol_library_writable(&a), "no save writes into the cache");
        assert_eq!(readable_with(TableKind::Symbol, &lib, &cache, &conv).unwrap(), a);
        assert_eq!(runs.get(), 1, "the same state is converted once");
        // the source changes: a new copy, and the old one goes
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&lib, "R2 changed").unwrap();
        let b = readable_with(TableKind::Symbol, &lib, &cache, &conv).unwrap();
        assert_ne!(a, b);
        assert!(!a.exists() && b.exists());
        assert_eq!(runs.get(), 2);
        assert!(std::fs::read_dir(&cache).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".converting")), "no half-made copy is left");
        remove(&cache);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_conversion_is_remembered_for_that_state_and_reported_as_the_rows_status() {
        let root = temp("fail");
        let cache = root.join("cache");
        let lib = root.join("bad.lbr");
        std::fs::write(&lib, "garbage").unwrap();
        let runs = Cell::new(0);
        let conv = fake(&runs);
        let e = readable_with(TableKind::Footprint, &lib, &cache, &conv).unwrap_err();
        assert!(e.contains("Unable to convert library"));
        assert!(readable_with(TableKind::Footprint, &lib, &cache, &conv).is_err());
        assert_eq!(runs.get(), 1, "a failure is not retried until the source changes");
        assert_eq!(status(TableKind::Footprint, &lib).as_deref(), Some(e.as_str()));
        assert_eq!(readable_with(TableKind::Symbol, &root.join("gone.lib"), &cache, &conv).unwrap_err(), format!("Library '{}' not found.", root.join("gone.lib").display()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_puts_the_kicad_library_beside_the_old_one_and_merges_footprints_into_an_existing_folder() {
        let root = temp("migrate");
        let runs = Cell::new(0);
        let conv = fake(&runs);
        let lib = root.join("old.lib");
        std::fs::write(&lib, "OLD").unwrap();
        assert_eq!(migrated_path(TableKind::Symbol, &lib).unwrap(), root.join("old.kicad_sym"));
        migrate_with(TableKind::Symbol, &lib, &root.join("old.kicad_sym"), &conv).unwrap();
        assert!(std::fs::read_to_string(root.join("old.kicad_sym")).unwrap().contains("OLD"));
        // again over the file: replaced
        std::fs::write(&lib, "NEWER").unwrap();
        migrate_with(TableKind::Symbol, &lib, &root.join("old.kicad_sym"), &conv).unwrap();
        assert!(std::fs::read_to_string(root.join("old.kicad_sym")).unwrap().contains("NEWER"));
        // footprints: `<name>.pretty` beside the file; an existing folder keeps its other footprints
        let lbr = root.join("Parts.lbr");
        std::fs::write(&lbr, "FP1").unwrap();
        let pretty = migrated_path(TableKind::Footprint, &lbr).unwrap();
        assert_eq!(pretty, root.join("Parts.pretty"));
        std::fs::create_dir_all(&pretty).unwrap();
        std::fs::write(pretty.join("Mine.kicad_mod"), "(footprint \"Mine\")").unwrap();
        migrate_with(TableKind::Footprint, &lbr, &pretty, &conv).unwrap();
        assert!(pretty.join("FP1.kicad_mod").is_file() && pretty.join("Mine.kicad_mod").is_file());
        assert!(std::fs::read_dir(&root).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains(".migrating-")));
        assert_eq!(migrate_with(TableKind::Symbol, &root.join("none.lib"), &root.join("none.kicad_sym"), &conv).unwrap_err(), format!("Library '{}' not found.", root.join("none.lib").display()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The slow tier: kicad-cli converts KiCad's own QA libraries -- a legacy `.lib`, an Eagle `.lbr` (symbols and footprints) and an Altium `.PcbLib` -- and the
    /// copies read as libraries of the tables do.
    #[test]
    fn kicad_cli_converts_legacy_eagle_and_altium_libraries_that_then_read_like_kicads_own() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: set EDA_SLOW_TESTS=1 with kicad-cli installed");
            return;
        }
        let Some(qa) = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|s| PathBuf::from(s).join("qa").join("data"))) else {
            eprintln!("skipped: KICAD_QA_DATA is not set");
            return;
        };
        let root = temp("slow");
        let cache = root.join("cache");
        let legacy = qa.join("eeschema/netlists/legacy_power4/symbols.lib");
        let sym = readable_with(TableKind::Symbol, &legacy, &cache, &kicad_cli).unwrap();
        let names: Vec<String> = eda_kicad::scan_symbols(&std::fs::read_to_string(&sym).unwrap()).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["+3V3", "GND", "R", "+3.3V"], "the legacy library's four symbols, as KiCad's writer saved them");
        let lbr = qa.join("pcbnew/plugins/eagle/lbr/SparkFun-GPS.lbr");
        let fps = readable_with(TableKind::Footprint, &lbr, &cache, &kicad_cli).unwrap();
        assert!(fps.join("NEO-M8P.kicad_mod").is_file());
        let eagle_syms = readable_with(TableKind::Symbol, &lbr, &cache, &kicad_cli).unwrap();
        let eagle: Vec<String> = eda_kicad::scan_symbols(&std::fs::read_to_string(&eagle_syms).unwrap()).into_iter().map(|s| s.name).collect();
        assert_eq!(eagle.len(), 24, "the Eagle library's 24 devices: {eagle:?}");
        assert!(eagle.iter().any(|n| n == "ANTENNAGPS-2X7MM"));
        let altium = qa.join("pcbnew/plugins/altium/pcblib/Espressif ESP32-WROOM-32.PcbLib");
        let esp = readable_with(TableKind::Footprint, &altium, &cache, &kicad_cli).unwrap();
        let fp = std::fs::read_to_string(esp.join("Espressif ESP32-WROOM-32.kicad_mod")).unwrap();
        assert!(eda_kicad::parse_library_footprint(&fp).is_ok(), "the converted footprint reads");
        // Migrate Libraries: the legacy file's KiCad library beside a copy of it
        std::fs::copy(&legacy, root.join("symbols.lib")).unwrap();
        let dest = migrated_path(TableKind::Symbol, &root.join("symbols.lib")).unwrap();
        migrate(TableKind::Symbol, &root.join("symbols.lib"), &dest).unwrap();
        assert_eq!(eda_kicad::scan_symbols(&std::fs::read_to_string(&dest).unwrap()).len(), 4);
        let bad = root.join("bad.lib");
        std::fs::write(&bad, "garbage").unwrap();
        let e = readable_with(TableKind::Symbol, &bad, &cache, &kicad_cli).unwrap_err();
        assert!(e.contains("Unable to convert library") || e.contains("Unrecognized"), "{e}");
        remove(&cache);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A CADSTAR row's options reach the conversion (UI-LEFT-2 LL5): `csa` puts the named archive beside the library under the library's name, where the plugin
    /// looks first, and `fplib` names the converted symbols' footprint library.
    #[test]
    fn a_cadstar_rows_csa_and_fplib_options_reach_the_conversion() {
        let dir = temp("cadstar");
        std::fs::create_dir_all(dir.join("arch")).unwrap();
        std::fs::write(dir.join("parts.lib"), "lib").unwrap();
        std::fs::write(dir.join("arch").join("design.csa"), "the archive").unwrap();
        let seen = std::cell::RefCell::new(String::new());
        let fake = |_lk: LibraryKind, from: &Path, to: &Path| -> Result<(), String> {
            let csa = from.with_extension("csa");
            *seen.borrow_mut() = std::fs::read_to_string(&csa).unwrap_or_default();
            std::fs::write(to, "(kicad_symbol_lib (symbol \"R\" (property \"Footprint\" \"cadstarpcblib:R0603\")))").map_err(|e| e.to_string())
        };
        let opts = lt::parse_options("csa=arch/design.csa|fplib=MyPcbLib");
        let out = dir.join("out.kicad_sym");
        cadstar_with_options(LibraryKind::Symbol, &dir.join("parts.lib"), &out, &opts, &fake).unwrap();
        assert_eq!(*seen.borrow(), "the archive", "the archive sits beside the library under its name");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("\"MyPcbLib:R0603\"") && !text.contains("cadstarpcblib"), "{text}");
        // no options: as it is
        cadstar_with_options(LibraryKind::Symbol, &dir.join("parts.lib"), &out, &Default::default(), &fake).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("cadstarpcblib:R0603"));
        assert_eq!(lt::parse_options("a=1|b=x\\|y| c"), [("a".to_string(), "1".to_string()), ("b".to_string(), "x|y".to_string()), ("c".to_string(), String::new())].into());
        remove(&dir);
    }
}
