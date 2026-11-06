//! The 3D models of the board's parts, for the browser's 3D view (`web/studio/src/components/viewer3d`).
//!
//! The view used to wait for `kicad-cli pcb export glb` -- the whole board, every model through OpenCascade, seconds to minutes -- before it showed one real
//! model. It now loads each footprint's KiCad 3D model by itself, with three.js's own `VRMLLoader`, and places it the way KiCad does. This module is the
//! server's half of that:
//!
//!   - **`GET /api/3dmodel?name=<model path>`** serves one model, read-only. `name` is the path a footprint's `(model "...")` line writes
//!     (`${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`, a legacy `${KISYS3DMOD}/...wrl`, a project-relative one), percent-encoded. It is
//!     resolved the way KiCad's `FILENAME_RESOLVER` does (see [`resolve`]) and then held to an allow-list: the file must lie, once symlinks are resolved,
//!     under KiCad's 3D model directory or the board's own directory, and be a model file KiCad's 3D plugins read ([`model_family`]: VRML, X3D, STEP, IGES).
//!     Nothing else is readable through this route.
//!   - **STEP models** are what KiCad's installed library holds (7238 `.step`, no `.wrl`), and the browser has no STEP reader. The first request for one
//!     converts it with `kicad-cli pcb export vrml --models-dir` (`eda_kicad_engine::convert_models_to_vrml`: KiCad's own 3D cache reads the STEP and
//!     writes the VRML KiCad's libraries are written in) and keeps the result in a cache directory, keyed by the file's path, size and modification time,
//!     so a model is converted once, not once per board or per session. The route answers `202 {"status":"pending"}` while it runs and the browser asks
//!     again; the request loop never waits for kicad-cli. Requests that arrive together (one per distinct model of the board) are converted in one
//!     kicad-cli run, up to 16: a run costs 2 to 3.7 s of CPU whether it converts 2, 5 or 15 models (measured), so splitting a board's models into runs
//!     (tried: two models first, the rest after) puts the first model no sooner and the last one later. IGES, X3D and compressed VRML (`.wrz`) go the same way
//!     ([`Format::Converted`]): the box's kicad-cli (10.99.0-4fe1ca2060) converts a hand-written `.x3d`, a hand-written `.igs` / `.iges`, a `.wrz`, a
//!     `.stpz` and a `.step.gz` with `--models-dir` as it converts a STEP (2026-11-06, scratch files). `kicad-cli pcb export glb` (the Render section's exact
//!     export) takes IGES but not X3D: its STEP exporter's `getModelLabel` has no X3D reader ("Cannot identify actual file type for '%s'.").
//!   - **The models of each part** go out in `/api/state` (`parts[].models`, `parts[].kind3d`), from [`part_json`].

use crate::kicad_lane::Lane;
use eda_kicad::part_models;
use eda_model::embedded_files;
use eda_model::model_subst;
use eda_model::{ConstraintModel, Footprint, Part};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

// ----------------------------------------------------------------------- where the models are

/// Where KiCad's installed 3D model library lives, in precedence order: `EDA_KICAD_3DMODELS_DIR`, then the installed KiCad's (`eda_kicad::kicad_shared_dir`) -- the directory
/// `${KICAD10_3DMODEL_DIR}` stands for.
pub fn models_dir() -> PathBuf {
    if let Ok(p) = std::env::var("EDA_KICAD_3DMODELS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    eda_kicad::kicad_shared_dir("3dmodels")
}

/// The directories a model may be read from: KiCad's 3D model library, the board's own directory (`${KIPRJMOD}`, and what a relative path is relative to) and
/// the folders the board's path variables name.
#[derive(Debug, Clone)]
pub struct Roots {
    pub models: PathBuf,
    pub project: PathBuf,
    /// The folders the person named in Configure Paths (the board's variables, or KiCad's own): `library_tables::ModelNames::folders`. Never the environment's.
    pub extra: Vec<PathBuf>,
    /// The resolver's search paths by their bare names (`FILENAME_RESOLVER::createPathList` with `GetKicadPaths`: each of KiCad's path variables, "the path without the
    /// ${} to act as legacy alias support for older files"), each with the folder it stands for: what a KiCad 5 alias -- `NAME:file`, `:NAME:file`, or a `${NAME}/file`
    /// no variable expands -- is looked up in ([`search`]). `library_tables::ModelNames::aliases`.
    pub aliases: Vec<(String, PathBuf)>,
}

/// Why a model name does not name a readable model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// No name, a NUL in it, or a name too long to be a path.
    BadName,
    /// No such file (after KiCad's own search: the name as given, relative to the project, relative to the model library, a STEP in place of a missing VRML).
    NotFound,
    /// A file, but not one this route may serve: outside the two directories, or not a model file.
    Forbidden(String),
}

/// How the route serves a model file: a VRML 2.0 file as it is (three.js's `VRMLLoader` reads it), every other format KiCad's 3D plugins read through
/// kicad-cli's conversion to VRML ([`Store`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Vrml,
    /// STEP, IGES, X3D and a compressed VRML (`.wrz`): converted once by `kicad-cli pcb export vrml --models-dir`, whose 3D cache reads it with the plugin KiCad's 3D
    /// viewer reads it with (`libs3d_plugin_oce` for STEP and IGES, `libs3d_plugin_vrml` for X3D; a `.wrz` is gunzipped).
    Converted,
}

/// The families of model file KiCad's 3D plugins read (`S3D_PLUGIN_MANAGER`'s extension map), each a filter of the model chooser of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    /// `libs3d_plugin_oce`'s "STEP (*.stp;*.STP;*.stpZ;*.stpz;*.STPZ;*.step;*.STEP;*.stp.gz;*.STP.GZ;*.step.gz;*.STEP.GZ)".
    Step,
    /// `libs3d_plugin_oce`'s "IGES (*.igs;*.IGS;*.iges;*.IGES)".
    Iges,
    /// `libs3d_plugin_vrml`'s "VRML 1.0/2.0 (*.wrl;*.WRL;*.wrz;*.WRZ)".
    Vrml,
    /// `libs3d_plugin_vrml`'s "X3D (*.x3d;*.X3D)".
    X3d,
}

/// The family of a model file by its extension, as `S3D_PLUGIN_MANAGER::Load3DModel` finds the plugin for it: the part after the last dot, or, for a `.gz`, the one
/// before it and `.gz` ("`.gz` files are compressed versions that may have additional information in the previous extension"), looked up in the extensions the plugins
/// list (the `FILE_DATA` of KiCad 25ec2335's `plugins/3d/oce/oce.cpp` and `plugins/3d/vrml/plugin.cpp`; 8303b2ad's tree has no `plugins/`). `None` for a file no plugin
/// here reads. IDF component outlines (`libs3d_plugin_idf`'s `.idf` and `.emn`) are not served: a conversion of one was not verified.
///
/// Differs from KiCad in one way: on Linux and macOS the plugins list each spelling they take (`stp`, `STP`, `stpZ`, ...) and the map is matched as the file has it, so
/// a `.Step` loads nowhere in KiCad; here the extension is matched without regard to case.
pub fn model_family(path: &Path) -> Option<ModelFamily> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = if ext == "gz" { format!("{}.gz", stem.rsplit_once('.')?.1) } else { ext.to_string() };
    match ext.as_str() {
        "stp" | "step" | "stpz" | "stp.gz" | "step.gz" => Some(ModelFamily::Step),
        "igs" | "iges" => Some(ModelFamily::Iges),
        "wrl" | "wrz" => Some(ModelFamily::Vrml),
        "x3d" => Some(ModelFamily::X3d),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Canonical: symlinks resolved, inside one of the [`Roots`].
    pub path: PathBuf,
    pub format: Format,
}

/// KiCad's variable expansion for a model path (`KIwxExpandEnvVars`, `common/common.cpp`): `${NAME}`, `$(NAME)` and a bare `$NAME`, the name being the run of
/// letters, digits, `_` and `:` after the bracket (`while( wxIsalnum( str_m ) || str_m == '_' || str_m == ':' )`); `\$` is a plain `$`. A name resolves, in
/// order, to the board's directory (`KIPRJMOD`), to the process environment, and -- for a name that contains `KISYS3DMOD` or is any `KICAD*_3DMODEL_DIR`, however
/// old -- to the model library of the KiCad that is installed ("Replace unmatched older variables with current locations"). Anything else is left as written,
/// and then names no file; so is a bracket left open (KiCad logs "Environment variables expansion failed: missing '%c' ..." and goes on with the text after
/// the name). An unresolved `${NAME}` is tried against the resolver's aliases afterwards, by [`search`].
pub(crate) fn expand(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> String {
    let s: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());
    let mut n = 0;
    while n < s.len() {
        match s[n] {
            '\\' if matches!(s.get(n + 1), Some('$') | Some('%')) => {
                out.push(s[n + 1]);
                n += 2;
            }
            '$' => {
                let close = match s.get(n + 1) {
                    Some('(') => Some(')'),
                    Some('{') => Some('}'),
                    _ => None,
                };
                let start = if close.is_some() { n + 2 } else { n + 1 };
                if start >= s.len() {
                    // `if( m >= strlen ) break;`: the `$` (and its bracket) end the text, and nothing is appended for them (the `break` leaves the
                    // `switch` before any `strResult +=`), so `a$` is `a` and `a${` is `a`.
                    break;
                }
                let mut m = start;
                while m < s.len() && (s[m].is_alphanumeric() || s[m] == '_' || s[m] == ':') {
                    m += 1;
                }
                let var: String = s[start..m].iter().collect();
                let value = if var == "KIPRJMOD" {
                    Some(roots.project.to_string_lossy().into_owned())
                } else if let Some(v) = env(&var).filter(|v| !v.is_empty()) {
                    Some(v)
                } else if var.contains("KISYS3DMOD") || (var.starts_with("KICAD") && var.ends_with("_3DMODEL_DIR")) {
                    Some(roots.models.to_string_lossy().into_owned())
                } else {
                    None
                };
                let expanded = value.is_some();
                match value {
                    Some(v) => out.push_str(&v),
                    None => out.extend(&s[n..m]),
                }
                if let Some(c) = close {
                    if s.get(m) == Some(&c) {
                        if !expanded {
                            out.push(c);
                        }
                        m += 1;
                    }
                }
                n = m;
            }
            c => {
                out.push(c);
                n += 1;
            }
        }
    }
    out
}

/// The STEP-family file that stands in for a VRML model that is not there, in the same directory: `S3D_CACHE::load`'s substitution for a missing reference in
/// kicad-cli and scripting ("In CLI / scripting contexts, transparently substitute a matching STEP model for a missing WRL reference", `!Pgm().IsGUI() &&
/// MODEL_SUBSTITUTION::IsWrlExtension( aModelFile )`, then `STEP_CATALOG::FindMatchFor`): only for a `.wrl` or `.wrz` name, and the candidates are KiCad's
/// `stepExtensions` -- STEP, STPZ, the gzipped STEPs and IGES -- with the same normalised stem (`normalizeStem`).
///
/// Differs from KiCad: KiCad's GUI does no such substitution when it draws (it migrates the references when the board opens, `AutoMigrateByFilename`, which
/// the studio now does too: `model3d_tab_api::auto_migrate`); kicad-cli's catalog walks every search path (`gatherScanDirs`), this looks only beside the missing
/// file, the same name with each of `stepExtensions` in KiCad's order of preference first, then any file of the same normalised stem there.
fn step_for_missing_wrl(wrl: &Path) -> Option<PathBuf> {
    let name = wrl.file_name()?.to_str()?;
    if !model_subst::is_wrl(name) {
        return None;
    }
    let stem = &name[..name.rfind('.')?];
    for ext in model_subst::STEP_EXTENSIONS {
        let sibling = wrl.with_file_name(format!("{stem}.{ext}"));
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let want = model_subst::normalize_stem(name);
    let mut found: Vec<PathBuf> = std::fs::read_dir(wrl.parent()?).ok()?.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| model_subst::has_step_extension(n) && model_subst::normalize_stem(n) == want)).collect();
    found.sort();
    found.into_iter().next()
}

/// How the route serves the file at `path` (see [`Format`]), by its extension ([`model_family`]): a `.wrl` as it is, every other model file converted.
///
/// A VRML 1.0 `.wrl` (and a `.wrz` holding one) is not drawn: KiCad's VRML plugin reads it (`WRL1BASE`), three.js's `VRMLLoader` reads VRML 2.0 only, and kicad-cli's
/// export is no way round that -- `EXPORTER_PCB_VRML::ExportVrmlFootprint` copies a `.wrl` into `--models-dir` as it is and only gunzips a `.wrz` ("copy VRML models
/// and use the scenegraph library to translate other model types"; verified: a VRML 1.0 file came back unchanged).
pub(crate) fn format_of(path: &Path) -> Option<Format> {
    let family = model_family(path)?;
    let plain_vrml = family == ModelFamily::Vrml && path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wrl"));
    Some(if plain_vrml { Format::Vrml } else { Format::Converted })
}

/// `FILENAME_RESOLVER::SplitAlias`: `ALIAS:relative/path` or `:ALIAS:relative/path` into the alias and the path; `None` when there is no colon after the alias's
/// first character, or nothing after the colon.
pub(crate) fn split_alias(name: &str) -> Option<(&str, &str)> {
    let search_start = usize::from(name.starts_with(':'));
    let tagpos = search_start + name[search_start..].find(':')?;
    if tagpos == search_start || tagpos + 1 >= name.len() {
        return None;
    }
    Some((&name[search_start..tagpos], &name[tagpos + 1..]))
}

/// `FILENAME_RESOLVER::ResolvePath` (`common/filename_resolver.cpp`) without its embedded files (each caller asks those first): the file `name` finds, or `None`.
///
///   1. The name with its variables expanded ([`expand`]), when that is a file: "full paths, leading expanded vars".
///   2. A name that begins with `${` or `$(` and is not a file after that is a file nowhere -- unless the expansion left it as it was (no variable of that name), when
///      the variable's name is tried as one of the resolver's aliases ("a user-defined alias that was incorrectly stored in ${} format by older versions of KiCad"):
///      the folder of the search path whose bare name it is, with the rest of the name after its separator.
///   3. The name relative to the project's folder (`m_paths.begin()`: `${KIPRJMOD}`), then relative to the 3D model library (`${KICAD10_3DMODEL_DIR}/name`, "legacy
///      behavior"); neither for a name that begins with `:`.
///   4. A KiCad 5 aliased name (`ALIAS:path`, `:ALIAS:path`: [`split_alias`]): the folder of the search path whose bare name is the alias ([`Roots::aliases`]), with the
///      path after it.
///
/// Differs from KiCad, all because a server answers here and not a program run in the board's folder: a relative name is not tried against the process's working
/// directory (step 1's "paths relative to the current working directory"), nor against the footprint's library folder (`aWorkingPath`, which
/// `RENDER_3D_OPENGL::load3dModels` passes as the footprint library row's path: this route is asked a model name alone); `checkEnvVarPath`'s adding of a variable
/// to the search paths is not needed, the paths being read on every request.
pub(crate) fn search(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let tname = expand(&name, roots, env);
    let given = PathBuf::from(&tname);
    if given.is_absolute() && given.is_file() {
        return Some(given);
    }
    if name.starts_with("${") || name.starts_with("$(") {
        if tname != name {
            return None;
        }
        let close = if name.starts_with("${") { '}' } else { ')' };
        let alias_end = name.find(close).filter(|&e| e > 2)?;
        let alias = &name[2..alias_end];
        let rel = name[alias_end + 1..].strip_prefix('/').unwrap_or(&name[alias_end + 1..]);
        return in_alias(alias, rel, roots, env);
    }
    if !tname.starts_with(':') && !given.is_absolute() {
        for base in [&roots.project, &roots.models] {
            let full = base.join(&given);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    let (alias, rel) = split_alias(&tname)?;
    in_alias(alias, rel, roots, env)
}

/// The file `rel` in the folder of the search path whose bare name is `alias` (`path.m_Alias == alias && !path.m_Pathexp.empty()`, then
/// `ExpandEnvVarSubstitutions( fpath.GetPathWithSep() + relpath )` and `wxFileName::FileExists`).
fn in_alias(alias: &str, rel: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    roots.aliases.iter().filter(|(a, folder)| a == alias && !folder.as_os_str().is_empty()).find_map(|(_, folder)| {
        let full = PathBuf::from(expand(&format!("{}/{rel}", folder.display()), roots, env));
        full.is_file().then_some(full)
    })
}

/// Whether `name` finds a file the way `FILENAME_RESOLVER::ResolvePath` searches for one ([`search`]) and nothing else: a `.wrl` that is not there is *not* found
/// because a `.step` of the same name is (that stand-in is [`resolve`]'s, for the 3D view). The Migrate 3D Models dialog's "missing" is this. The allow-list is
/// not asked: a file that is there is there, whether this route may serve it or not.
pub(crate) fn finds_file(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> bool {
    if name.is_empty() || name.len() > 4096 || name.contains('\0') {
        return false;
    }
    // `kicad-embed://name` finds the file the board embeds under that name (`FILENAME_RESOLVER::ResolvePath` with the board's embedded files).
    if embedded_files::name_of_link(&name.replace('\\', "/")).is_some() {
        return crate::embedded_api::embeds(&roots.project, &name.replace('\\', "/"));
    }
    search(name, roots, env).is_some()
}

/// Whether `path` (canonical) lies inside one of the folders the route may read.
fn allowed(canonical: &Path, roots: &Roots) -> bool {
    [&roots.models, &roots.project].into_iter().chain(&roots.extra).filter_map(|r| r.canonicalize().ok()).any(|r| canonical.starts_with(&r))
}

/// The file a model name finds and this route may read, as [`finds_file`] searches for it -- no `.step` stands in for a `.wrl` that is not there. What Collect and Embed 3D Models
/// embeds (`PCB_CONTROL::CollectAndEmbed3DModels`: `ResolvePath`, then `fname.Exists()`, whatever the file's extension: a `.stpz`, a `.wrz` too); `None` for a name that finds nothing
/// or a file outside the allow-list.
pub(crate) fn resolve_exact(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if name.is_empty() || name.len() > 4096 || name.contains('\0') {
        return None;
    }
    let canonical = search(name, roots, env)?.canonicalize().ok()?;
    allowed(&canonical, roots).then_some(canonical)
}

/// The words of a refusal for a file that is not a model file.
const NOT_A_MODEL: &str = "only the 3D model files KiCad reads are served (VRML .wrl and .wrz, X3D .x3d, STEP .step, .stp, .stpz, .step.gz and .stp.gz, IGES .iges and .igs)";

/// The file a footprint's `(model "name")` names, found the way KiCad's `FILENAME_RESOLVER::ResolvePath` finds it ([`search`]) and then held to the allow-list.
///
/// A `.wrl` or `.wrz` that is not there is the STEP-family file of the same name beside it, as kicad-cli finds one ([`step_for_missing_wrl`]).
///
/// The allow-list is the part KiCad does not have, because KiCad reads files for the person who owns them and this route answers a browser: after the
/// search, the file is made canonical (every symlink and `..` resolved) and must lie under the model library, the board's directory or a folder one of the
/// board's path variables names ([`Roots::extra`]), and be a model file ([`format_of`]). A name that finds a file anywhere else is [`Reject::Forbidden`], whatever
/// it holds.
pub fn resolve(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Result<Resolved, Reject> {
    if name.is_empty() || name.len() > 4096 || name.contains('\0') {
        return Err(Reject::BadName);
    }
    let name = name.replace('\\', "/");
    // `kicad-embed://name`: the file the board embeds under that name, extracted beside the derived KiCad files (`EMBEDDED_FILES::GetTemporaryFileName`), which lies inside the board's folder.
    if let Some(extracted) = crate::embedded_api::embedded_model_path(&roots.project, &name) {
        let found = extracted.map_err(|_| Reject::NotFound)?;
        let canonical = found.canonicalize().map_err(|_| Reject::NotFound)?;
        return match format_of(&canonical) {
            Some(format) => Ok(Resolved { path: canonical, format }),
            None => Err(Reject::Forbidden(NOT_A_MODEL.into())),
        };
    }
    let found = match search(&name, roots, env) {
        Some(found) => found,
        None => {
            // the stand-in for a missing VRML is looked for where the name would have been: as it expands, or relative to the project or the library
            let expanded = PathBuf::from(expand(&name, roots, env));
            let places: Vec<PathBuf> = if expanded.is_absolute() { vec![expanded] } else { vec![roots.project.join(&expanded), roots.models.join(&expanded)] };
            places.iter().find_map(|p| step_for_missing_wrl(p)).ok_or(Reject::NotFound)?
        }
    };
    let canonical = found.canonicalize().map_err(|_| Reject::NotFound)?;
    if !allowed(&canonical, roots) {
        return Err(Reject::Forbidden("the file is outside KiCad's 3D model directory, the board's directory and the folders its path variables name".into()));
    }
    match format_of(&canonical) {
        Some(format) => Ok(Resolved { path: canonical, format }),
        None => Err(Reject::Forbidden(NOT_A_MODEL.into())),
    }
}

/// The text of a query value with its `%XX` escapes decoded (`encodeURIComponent`'s: `+` is a plus, not a space); `None` for a `%` not followed by two hex
/// digits or for bytes that are not UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ------------------------------------------------------------------- converting STEP to VRML

/// What asking for a STEP model's VRML found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The converted file, in the cache.
    Ready(PathBuf),
    /// Queued or being converted: ask again shortly.
    Pending,
    /// kicad-cli could not convert it (or could not run); kept, so a model that reliably fails does not start kicad-cli on every poll, until a retry is asked for.
    Failed(String),
}

#[derive(Debug, Clone)]
enum Entry {
    Queued,
    Running,
    Done(PathBuf),
    Failed(String),
}

#[derive(Default)]
struct Inner {
    state: HashMap<PathBuf, Entry>,
    queue: VecDeque<PathBuf>,
    worker: bool,
}

/// How a batch of STEP files becomes VRML files: one result per model, the path of its `.wrl` (written where the converter likes: the store moves it into
/// its cache) or why not.
pub type Converter = Arc<dyn Fn(&[PathBuf]) -> Vec<Result<PathBuf, String>> + Send + Sync>;

/// The converted models, and the one background thread that converts more.
pub struct Store {
    cache_dir: PathBuf,
    convert: Converter,
    /// How long the worker waits for more requests before it converts what it has: the page asks for every model of the board at once.
    gather: Duration,
    inner: Mutex<Inner>,
    /// Signalled when a batch has been converted (or has failed): what a request that waits for its model sleeps on.
    settled: Condvar,
}

/// The most models one kicad-cli run converts: it loads them all at once (and a run is dear: see the module doc).
const MAX_BATCH: usize = 16;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Store {
    pub fn new(cache_dir: PathBuf, convert: Converter, gather: Duration) -> Arc<Store> {
        Arc::new(Store { cache_dir, convert, gather, inner: Mutex::new(Inner::default()), settled: Condvar::new() })
    }

    /// Where the converted file of `step` is kept: named after the model, and after the file's path, size and modification time, so a model that changes on
    /// disk is converted again and one that does not is converted once.
    pub fn cache_file(&self, step: &Path) -> PathBuf {
        let meta = std::fs::metadata(step).ok();
        let stamp = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        let size = meta.map_or(0, |m| m.len());
        let digest = blake3::hash(format!("{}\0{size}\0{stamp}", step.display()).as_bytes()).to_hex();
        let stem: String = step.file_stem().map(|s| s.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()).unwrap_or_else(|| "model".into());
        self.cache_dir.join(format!("{stem}-{}.wrl", &digest.as_str()[..16]))
    }

    /// The VRML of the STEP file `step` (a canonical path): what is in the cache, or a conversion started (or joined) in the background.
    pub fn ask(self: &Arc<Self>, step: &Path, retry: bool) -> Ask {
        let cached = self.cache_file(step);
        if cached.is_file() {
            return Ask::Ready(cached);
        }
        let mut inner = lock(&self.inner);
        match inner.state.get(step).cloned() {
            Some(Entry::Done(p)) if p.is_file() => return Ask::Ready(p),
            Some(Entry::Queued | Entry::Running) => return Ask::Pending,
            Some(Entry::Failed(e)) if !retry => return Ask::Failed(e),
            _ => {}
        }
        inner.state.insert(step.to_path_buf(), Entry::Queued);
        inner.queue.push_back(step.to_path_buf());
        if !inner.worker {
            inner.worker = true;
            let store = self.clone();
            if std::thread::Builder::new().name("3dmodels".into()).spawn(move || store.work()).is_err() {
                inner.worker = false;
                inner.state.insert(step.to_path_buf(), Entry::Failed("could not start the conversion thread".into()));
                return Ask::Failed("could not start the conversion thread".into());
            }
        }
        Ask::Pending
    }

    /// Sleeps until the conversion of `step` is over -- it is in the cache or has failed -- or `timeout` has passed. A model that nobody asked for is not waited for.
    /// What a request for a model that is not there yet does on its own thread, so the page needs no timer to learn that the model is in.
    pub fn wait(&self, step: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut inner = lock(&self.inner);
        loop {
            if !matches!(inner.state.get(step), Some(Entry::Queued | Entry::Running)) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            inner = self.settled.wait_timeout(inner, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Converts what is queued, a batch at a time, until nothing is.
    fn work(self: Arc<Self>) {
        std::thread::sleep(self.gather);
        loop {
            let batch: Vec<PathBuf> = {
                let mut inner = lock(&self.inner);
                let mut batch: Vec<PathBuf> = Vec::new();
                let mut later = VecDeque::new();
                while let Some(p) = inner.queue.pop_front() {
                    // Two models of one file name would be one `.wrl` in one run: the second waits for the next.
                    let clash = batch.iter().any(|b| b.file_stem() == p.file_stem());
                    if batch.len() < MAX_BATCH && !clash {
                        batch.push(p);
                    } else {
                        later.push_back(p);
                    }
                }
                inner.queue = later;
                if batch.is_empty() {
                    inner.worker = false;
                    return;
                }
                for p in &batch {
                    inner.state.insert(p.clone(), Entry::Running);
                }
                batch
            };
            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.convert)(&batch))).unwrap_or_else(|_| batch.iter().map(|_| Err("the model conversion crashed".to_string())).collect());
            let mut inner = lock(&self.inner);
            for (i, step) in batch.iter().enumerate() {
                let outcome = match results.get(i) {
                    Some(Ok(produced)) => self.keep(step, produced),
                    Some(Err(e)) => Err(e.clone()),
                    None => Err("the conversion returned nothing for this model".to_string()),
                };
                inner.state.insert(step.clone(), match outcome {
                    Ok(path) => Entry::Done(path),
                    Err(e) => Entry::Failed(e),
                });
            }
            drop(inner);
            self.settled.notify_all();
        }
    }

    /// Moves a converted file into the cache under its final name.
    fn keep(&self, step: &Path, produced: &Path) -> Result<PathBuf, String> {
        let target = self.cache_file(step);
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| format!("cannot create the 3D model cache {}: {e}", self.cache_dir.display()))?;
        if std::fs::rename(produced, &target).is_err() {
            std::fs::copy(produced, &target).map_err(|e| format!("cannot keep the converted model: {e}"))?;
            let _ = std::fs::remove_file(produced);
        }
        Ok(target)
    }
}

/// Where converted models are kept: `EDA_3DMODEL_CACHE`, else the user's cache directory (`~/Library/Caches/eda-studio/3dmodels` on macOS,
/// `$XDG_CACHE_HOME` or `~/.cache` elsewhere), else the temp directory.
pub fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("EDA_3DMODEL_CACHE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("eda-studio").join("3dmodels")
}

/// The converter the studio runs: kicad-cli, in the studio's one kicad-cli lane (a DRC, an ERC and an export never run beside it). Each run works in a
/// directory of its own under the cache; what it produced is moved out of it into the cache's top level before the directory goes.
fn kicad_cli_converter(lane: Arc<Lane>, cache: PathBuf) -> Converter {
    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    Arc::new(move |batch: &[PathBuf]| {
        let run = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let work = cache.join(format!("run-{}-{run}", std::process::id()));
        let reply = lane.run("3d models", String::new, || match eda_kicad_engine::convert_models_to_vrml(&work, batch) {
            Ok(files) => json!({ "files": files.iter().map(|f| f.as_ref().map(|p| p.display().to_string())).collect::<Vec<_>>() }),
            Err(e) => json!({ "error": crate::board::reasons(&e) }),
        });
        let produced: Vec<Result<PathBuf, String>> = match (reply["files"].as_array(), reply["error"].as_str()) {
            (Some(files), _) => files.iter().map(|f| f.as_str().map(PathBuf::from).ok_or_else(|| "kicad-cli could not read this model".to_string())).collect(),
            (None, error) => batch.iter().map(|_| Err(error.unwrap_or("the conversion failed").to_string())).collect(),
        };
        let kept = produced
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.and_then(|p| {
                    let out = cache.join(format!("converted-{}-{run}-{i}.wrl", std::process::id()));
                    std::fs::rename(&p, &out).map(|_| out).map_err(|e| format!("cannot keep the converted model: {e}"))
                })
            })
            .collect();
        let _ = std::fs::remove_dir_all(&work);
        kept
    })
}

/// The studio's store: one per process, started the first time a model is asked for.
pub fn store(lane: &Arc<Lane>) -> Arc<Store> {
    static STORE: OnceLock<Arc<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        let cache = cache_dir();
        Store::new(cache.clone(), kicad_cli_converter(lane.clone(), cache), Duration::from_millis(60))
    })
    .clone()
}

// ------------------------------------------------------------------------------- the route

/// An answer for `studio::respond`.
#[derive(Debug)]
pub struct Reply {
    pub status: &'static str,
    pub kind: &'static str,
    pub body: Vec<u8>,
}

fn json_reply(status: &'static str, v: Value) -> Reply {
    Reply { status, kind: "application/json", body: v.to_string().into_bytes() }
}

/// The largest model file this route sends: a real model is a few hundred KB, the biggest of KiCad's libraries about 10 MB.
const MAX_MODEL_BYTES: u64 = 64 * 1024 * 1024;

fn file_reply(path: &Path) -> Reply {
    match std::fs::metadata(path) {
        Ok(m) if m.len() <= MAX_MODEL_BYTES => match std::fs::read(path) {
            Ok(body) => Reply { status: "200 OK", kind: "model/vrml", body },
            Err(e) => json_reply("500 Internal Server Error", json!({ "status": "failed", "error": e.to_string() })),
        },
        Ok(_) => json_reply("413 Payload Too Large", json!({ "status": "failed", "error": "the model file is too large" })),
        Err(e) => json_reply("404 Not Found", json!({ "status": "missing", "error": e.to_string() })),
    }
}

/// `GET /api/3dmodel?name=<percent-encoded model path>[&retry=1]` (`target` is the request target, query included) for the board in `dir`.
///
///   - `200 model/vrml`: the model, ready to parse;
///   - `202 {"status":"pending"}`: a STEP model being converted, ask again;
///   - `200 {"status":"failed","error":..}`: its conversion failed (kept until `retry=1`);
///   - `404 {"status":"missing"}`: the name finds no model; `403 {"status":"forbidden"}`: it finds a file this route may not serve (nothing is read);
///     `400`: no usable name.
pub fn reply(dir: &Path, target: &str, store: &Arc<Store>) -> Reply {
    let names = crate::library_tables::model_names(dir);
    reply_in(target, store, &board_roots(dir, &names), &|v| names.get(v), None)
}

/// Where the board in `dir` may read models from: KiCad's library, its own folder and the folders its path variables name. A name's `${VARIABLE}` is asked of
/// `names` (the board's variables, KiCad's, the environment), so a variable that gives a library its folder gives the 3D models theirs.
pub(crate) fn board_roots(dir: &Path, names: &crate::library_tables::ModelNames) -> Roots {
    Roots { models: models_dir(), project: dir.to_path_buf(), extra: names.folders.clone(), aliases: names.aliases.clone() }
}

/// How long a request with `wait=1` is held for its model: the page asks again after this (a conversion that long is a conversion that is stuck, or a queue of
/// big models). Long enough that the page almost never asks twice, short enough that a closed tab does not leave a thread sleeping for a minute.
pub const WAIT_FOR_MODEL: Duration = Duration::from_secs(25);

/// Whether `target` asks to be held until its model is ready (`&wait=1`): the studio then answers on a thread of its own ([`reply_waiting`]), not on the request loop.
pub fn wants_wait(target: &str) -> bool {
    target.split_once('?').is_some_and(|(_, q)| q.split('&').any(|kv| kv == "wait=1"))
}

/// [`reply`] for a request that waits: a STEP model still being converted is waited for (up to [`WAIT_FOR_MODEL`]) and then answered, so one request per model is
/// all the page sends and the answer comes the moment the model is in. Blocks its thread: not for the request loop.
pub fn reply_waiting(dir: &Path, target: &str, store: &Arc<Store>) -> Reply {
    let names = crate::library_tables::model_names(dir);
    reply_in(target, store, &board_roots(dir, &names), &|v| names.get(v), Some(WAIT_FOR_MODEL))
}

/// [`reply`] with the model library and the environment given (the tests'), and how long to wait for a model that is being converted (`None`: not at all).
#[cfg(test)]
pub fn reply_with(dir: &Path, target: &str, store: &Arc<Store>, models: &Path, env: &dyn Fn(&str) -> Option<String>, wait: Option<Duration>) -> Reply {
    reply_in(target, store, &Roots { models: models.to_path_buf(), project: dir.to_path_buf(), extra: Vec::new(), aliases: Vec::new() }, env, wait)
}

/// [`reply_with`] with the roots given.
pub fn reply_in(target: &str, store: &Arc<Store>, roots: &Roots, env: &dyn Fn(&str) -> Option<String>, wait: Option<Duration>) -> Reply {
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let value = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('=')));
    let Some(name) = value("name").and_then(percent_decode) else {
        return Reply { status: "400 Bad Request", kind: "text/plain", body: b"invalid model name".to_vec() };
    };
    let retry = value("retry") == Some("1");
    match resolve(&name, roots, env) {
        Err(Reject::BadName) => Reply { status: "400 Bad Request", kind: "text/plain", body: b"invalid model name".to_vec() },
        Err(Reject::NotFound) => json_reply("404 Not Found", json!({ "status": "missing", "error": format!("no 3D model file for {name}") })),
        Err(Reject::Forbidden(why)) => json_reply("403 Forbidden", json!({ "status": "forbidden", "error": why })),
        Ok(Resolved { path, format: Format::Vrml }) => file_reply(&path),
        Ok(Resolved { path, format: Format::Converted }) => {
            let mut asked = store.ask(&path, retry);
            if let (Ask::Pending, Some(limit)) = (&asked, wait) {
                store.wait(&path, limit);
                asked = store.ask(&path, false);
            }
            match asked {
                Ask::Ready(vrml) => file_reply(&vrml),
                Ask::Pending => json_reply("202 Accepted", json!({ "status": "pending" })),
                Ask::Failed(error) => json_reply("200 OK", json!({ "status": "failed", "error": error })),
            }
        }
    }
}

/// The most model names one `prepare` request may carry: a board has a few dozen distinct packages, a big one a few hundred.
const MAX_PREPARE: usize = 1024;

/// `POST /api/3dmodel/prepare {"names": [<model path>, ..]}`: queues the conversion of every STEP model among them that is not in the cache yet, all at once, and answers
/// at once with how many of the names are in each state. The page sends it with every model of the board before it asks for the first, so they are converted in one
/// kicad-cli run and not in the runs the browser's own pace of asking would make. Names that resolve to nothing, or to a file this route may not serve, are counted
/// as missing and not read.
pub fn prepare(dir: &Path, body: &[u8], store: &Arc<Store>) -> Value {
    let names = crate::library_tables::model_names(dir);
    prepare_in(body, store, &board_roots(dir, &names), &|v| names.get(v))
}

#[cfg(test)]
pub fn prepare_with(dir: &Path, body: &[u8], store: &Arc<Store>, models: &Path, env: &dyn Fn(&str) -> Option<String>) -> Value {
    prepare_in(body, store, &Roots { models: models.to_path_buf(), project: dir.to_path_buf(), extra: Vec::new(), aliases: Vec::new() }, env)
}

/// [`prepare_with`] with the roots given.
pub fn prepare_in(body: &[u8], store: &Arc<Store>, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Value {
    let names: Vec<String> = serde_json::from_slice::<Value>(body).ok().and_then(|v| v["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect())).unwrap_or_default();
    let (mut ready, mut pending, mut failed, mut missing) = (0, 0, 0, 0);
    for name in names.iter().take(MAX_PREPARE) {
        match resolve(name, roots, env) {
            Ok(Resolved { format: Format::Vrml, .. }) => ready += 1,
            Ok(Resolved { path, format: Format::Converted }) => match store.ask(&path, false) {
                Ask::Ready(_) => ready += 1,
                Ask::Pending => pending += 1,
                Ask::Failed(_) => failed += 1,
            },
            Err(_) => missing += 1,
        }
    }
    json!({ "ready": ready, "pending": pending, "failed": failed, "missing": missing })
}

// ------------------------------------------------------------------------ the parts' models

fn model_json(m: &eda_model::footprint::Model3d) -> Value {
    json!({ "name": m.path, "offset": m.offset, "scale": m.scale, "rotate": m.rotate, "opacity": m.opacity, "show": m.show })
}

/// The 3D models of `part` for `/api/state`: `{ "models": [{ name, offset, scale, rotate, opacity, show }], "kind3d": "smd" | "tht" | "virtual", "dnp", "not_in_pos",
/// "extruded_body"? }`, `None` when it has neither a model nor an Extruded 3D Body. `footprint` is what the model resolved the part's footprint to. The placement is the
/// footprint file's, in the studio's frame for a top-side footprint (`Model3d::flipped_frame`); the 3D view turns it with a bottom-side part. `extruded_body` is
/// [`crate::extruded_body_api::body_json`], present only for a part that has one (a part with a body and no model has `models: []`).
///
/// The kind and the two flags are the board footprint's own attributes (`Design::effective_attrs`: Footprint Properties' edit, else what the board file is written
/// with), which `BOARD_ADAPTER::IsFootprintShown` reads: `FP_SMD` an SMD model, `FP_THROUGH_HOLE` a through-hole one, neither a virtual one; `FP_DNP` and
/// `FP_EXCLUDE_FROM_POS_FILES` hide it unless the viewer's "Models marked DNP" / "Models not in POS File" rows are on. The extruded body is shown by the same rule
/// (`renderExtrudedBodies`: `if( !body->m_show || !m_boardAdapter.IsFootprintShown( fp ) ) continue;`).
pub fn part_json(design: &eda_model::ir::Design, model: &ConstraintModel, part: &Part, footprint: &Footprint) -> Option<Value> {
    use eda_model::fp_edit::FootprintKind;
    let found = part_models(model, part, footprint);
    let body = crate::extruded_body_api::body_json(design, model, part, footprint);
    if found.is_none() && body.is_none() {
        return None;
    }
    let attrs = design.effective_attrs(&part.reference, footprint);
    let kind = match attrs.kind {
        FootprintKind::Smd => "smd",
        FootprintKind::ThroughHole => "tht",
        FootprintKind::Unspecified => "virtual",
    };
    let models: Vec<Value> = found.map(|f| f.models.iter().map(model_json).collect()).unwrap_or_default();
    let mut v = json!({ "models": models, "kind3d": kind, "dnp": attrs.dnp, "not_in_pos": attrs.exclude_from_pos_files });
    if let Some(body) = body {
        v["extruded_body"] = body;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    /// A scratch directory tree: `<tmp>/models` (KiCad's 3D library), `<tmp>/project`, `<tmp>/outside`.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("eda-3dmodel-{name}-{}-{}", std::process::id(), N.fetch_add(1, SeqCst)));
            let _ = std::fs::remove_dir_all(&root);
            for d in ["models/Lib.3dshapes", "project/prj.3dshapes", "outside"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            std::fs::write(root.join("models/Lib.3dshapes/a.wrl"), "#VRML V2.0 utf8\n# a").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/b.step"), "ISO-10303-21;").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/C-d e.step"), "ISO-10303-21;").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/notes.txt"), "not a model").unwrap();
            std::fs::write(root.join("project/prj.3dshapes/p.wrl"), "#VRML V2.0 utf8\n# p").unwrap();
            std::fs::write(root.join("project/design.json"), "{\"secret\":\"design\"}").unwrap();
            std::fs::write(root.join("outside/secret.wrl"), "#VRML V2.0 utf8\n# SECRET").unwrap();
            std::fs::write(root.join("outside/secret.step"), "SECRET").unwrap();
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(root.join("outside/secret.wrl"), root.join("models/Lib.3dshapes/link.wrl")).unwrap();
                std::os::unix::fs::symlink(root.join("outside"), root.join("models/escape")).unwrap();
            }
            Tree { root }
        }
        fn roots(&self) -> Roots {
            Roots { models: self.root.join("models"), project: self.root.join("project"), extra: Vec::new(), aliases: Vec::new() }
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel).canonicalize().unwrap()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn names_resolve_the_way_kicad_resolves_them() {
        let t = Tree::new("resolve");
        let r = t.roots();
        let ok = |name: &str| resolve(name, &r, &no_env);
        let a = Resolved { path: t.path("models/Lib.3dshapes/a.wrl"), format: Format::Vrml };
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl"), Ok(a.clone()));
        assert_eq!(ok("${KICAD6_3DMODEL_DIR}/Lib.3dshapes/a.wrl"), Ok(a.clone()), "an older version's variable is the current library");
        assert_eq!(ok("$(KISYS3DMOD)/Lib.3dshapes/a.wrl"), Ok(a.clone()), "so is the deprecated one, in either bracket");
        assert_eq!(ok("Lib.3dshapes/a.wrl"), Ok(a.clone()), "a partial path is relative to the library");
        assert_eq!(ok("${KIPRJMOD}/prj.3dshapes/p.wrl"), Ok(Resolved { path: t.path("project/prj.3dshapes/p.wrl"), format: Format::Vrml }));
        assert_eq!(ok("prj.3dshapes/p.wrl"), Ok(Resolved { path: t.path("project/prj.3dshapes/p.wrl"), format: Format::Vrml }), "and relative to the project first");
        assert_eq!(ok("Lib.3dshapes\\a.wrl"), Ok(a), "a Windows separator is a separator");
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step"), Ok(Resolved { path: t.path("models/Lib.3dshapes/b.step"), format: Format::Converted }));
        // A VRML that is not there is the STEP of the same part (kicad-cli's --subst-models).
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.wrl"), Ok(Resolved { path: t.path("models/Lib.3dshapes/b.step"), format: Format::Converted }));
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/c_d_e.wrl"), Ok(Resolved { path: t.path("models/Lib.3dshapes/C-d e.step"), format: Format::Converted }), "a name that differs in case, '-' and ' ' too");
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/nothing.step"), Err(Reject::NotFound));
        assert_eq!(ok("${NOT_A_VARIABLE}/Lib.3dshapes/a.wrl"), Err(Reject::NotFound), "a variable nobody defines is left as written");
        // The process environment is read like KiCad's own.
        let env = |v: &str| (v == "MY_3D").then(|| t.root.join("models").to_string_lossy().into_owned());
        assert!(resolve("${MY_3D}/Lib.3dshapes/a.wrl", &r, &env).is_ok());
    }

    #[test]
    fn every_format_kicads_3d_plugins_read_is_served_vrml_2_as_it_is_and_the_rest_converted() {
        use ModelFamily::*;
        for (file, family) in [("a.step", Step), ("A.STP", Step), ("a.stpZ", Step), ("a.step.gz", Step), ("a.STP.GZ", Step), ("a.igs", Iges), ("a.IGES", Iges), ("a.wrl", Vrml), ("a.WRZ", Vrml), ("a.x3d", X3d), ("a.X3D", X3d)] {
            assert_eq!(model_family(Path::new(file)), Some(family), "{file}");
        }
        for no in ["a.idf", "a.emn", "a.txt", "a.gz", "a.wrl.gz", "step", "a.wrl.txt"] {
            assert_eq!(model_family(Path::new(no)), None, "{no}: no plugin reads it here");
        }
        let t = Tree::new("formats");
        let lib = t.root.join("models/Lib.3dshapes");
        for (f, text) in [("x.x3d", "<X3D/>"), ("i.igs", "S      1"), ("j.iges", "S      1"), ("z.wrz", "gz"), ("s.stpz", "gz"), ("g.igs", "S      1"), ("h.igs", "S      1"), ("h.step", "ISO-10303-21;")] {
            std::fs::write(lib.join(f), text).unwrap();
        }
        let r = t.roots();
        let format = |name: &str| resolve(name, &r, &no_env).map(|x| (x.path.file_name().unwrap().to_string_lossy().into_owned(), x.format));
        let conv = |f: &str| Ok((f.to_string(), Format::Converted));
        assert_eq!(format("Lib.3dshapes/a.wrl"), Ok(("a.wrl".to_string(), Format::Vrml)), "a VRML 2.0 file goes as it is");
        for f in ["x.x3d", "i.igs", "j.iges", "z.wrz", "s.stpz"] {
            assert_eq!(format(&format!("Lib.3dshapes/{f}")), conv(f), "{f}");
        }
        // a missing VRML: the STEP-family file of the same stem beside it, KiCad's `stepExtensions` in order (STEP before IGES), IGES when it is the only one
        assert_eq!(format("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/g.wrl"), conv("g.igs"));
        assert_eq!(format("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/h.wrz"), conv("h.step"));
        assert_eq!(format("Lib.3dshapes/missing.igs"), Err(Reject::NotFound), "only a missing .wrl or .wrz has a stand-in");
        assert!(!finds_file("Lib.3dshapes/g.wrl", &r, &no_env), "a stand-in is no file KiCad's resolver finds (Migrate 3D Models' missing)");
    }

    #[test]
    fn a_kicad5_alias_is_looked_up_in_the_resolvers_search_paths() {
        let t = Tree::new("aliases");
        let team = t.root.join("models/Team.3dshapes");
        std::fs::create_dir_all(&team).unwrap();
        std::fs::write(team.join("q.wrl"), "#VRML V2.0 utf8\n# q").unwrap();
        let mut r = t.roots();
        r.aliases = vec![("TEAM".into(), team.clone()), ("EMPTY".into(), PathBuf::new()), ("KICAD10_3DMODEL_DIR".into(), t.root.join("models")), ("OUT".into(), t.root.join("outside"))];
        let found = |name: &str| resolve(name, &r, &no_env).map(|x| x.path);
        let q = Ok(t.path("models/Team.3dshapes/q.wrl"));
        assert_eq!(found("TEAM:q.wrl"), q, "ALIAS:path");
        assert_eq!(found(":TEAM:q.wrl"), q, ":ALIAS:path");
        assert_eq!(found("${TEAM}/q.wrl"), q, "an alias stored as ${{ALIAS}} by an older KiCad, where no variable has that name");
        assert_eq!(found("$(TEAM)q.wrl"), q, "with no separator after the bracket");
        assert_eq!(found("KICAD10_3DMODEL_DIR:Lib.3dshapes/a.wrl"), Ok(t.path("models/Lib.3dshapes/a.wrl")), "a path variable is an alias under its bare name");
        assert_eq!(found("team:q.wrl"), Err(Reject::NotFound), "an alias is matched as it is spelled");
        assert_eq!(found("EMPTY:q.wrl"), Err(Reject::NotFound), "a search path with no folder is skipped");
        assert_eq!(found("${}/q.wrl"), Err(Reject::NotFound), "`${{}}` names no alias");
        assert_eq!(found(":prj.3dshapes/p.wrl"), Err(Reject::NotFound), "a name that begins with a colon is not tried against the project");
        assert!(matches!(found("OUT:secret.wrl"), Err(Reject::Forbidden(_))), "an alias leads only where the route may read");
        // a variable that does expand is no alias, even where the file is not there
        let env = |v: &str| (v == "TEAM").then(|| t.root.join("outside").to_string_lossy().into_owned());
        assert_eq!(resolve("${TEAM}/q.wrl", &r, &env), Err(Reject::NotFound));
        for (name, want) in [("a:b", Some(("a", "b"))), (":a:b/c.wrl", Some(("a", "b/c.wrl"))), ("C:/x/y.wrl", Some(("C", "/x/y.wrl"))), ("a:", None), (":a", None), ("::b", None), ("ab", None)] {
            assert_eq!(split_alias(name), want, "{name}");
        }
    }

    #[test]
    fn nothing_outside_the_model_library_and_the_project_is_readable() {
        let t = Tree::new("escapes");
        let r = t.roots();
        let forbidden = |name: &str| matches!(resolve(name, &r, &no_env), Err(Reject::Forbidden(_)));
        let nope = |name: &str| matches!(resolve(name, &r, &no_env), Err(Reject::NotFound | Reject::Forbidden(_) | Reject::BadName));
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/../outside/secret.wrl"), "a .. out of the library");
        assert!(forbidden("../outside/secret.wrl"), "a .. out of the project");
        assert!(forbidden("${KIPRJMOD}/../outside/secret.step"));
        assert!(forbidden(&t.root.join("outside/secret.wrl").to_string_lossy()), "an absolute path elsewhere");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/link.wrl"), "a symlink in the library that leads out of it");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/escape/secret.wrl"), "a directory symlink that leads out of it");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/notes.txt"), "a file in the library that is no model");
        assert!(forbidden("${KIPRJMOD}/design.json"), "the board's own file: only model files are served");
        assert!(nope("/etc/passwd"), "{:?}", resolve("/etc/passwd", &r, &no_env));
        assert!(nope(&format!("{}/../../../../../etc/passwd", r.models.display())));
        assert!(nope("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl\0.txt"), "a NUL ends a C string");
        assert!(nope(""));
        // The environment cannot lead anywhere the roots do not.
        let env = |v: &str| (v == "ELSEWHERE").then(|| t.root.join("outside").to_string_lossy().into_owned());
        assert!(matches!(resolve("${ELSEWHERE}/secret.wrl", &r, &env), Err(Reject::Forbidden(_))));
    }

    #[test]
    fn the_route_answers_each_case_and_never_reads_what_it_refuses() {
        let t = Tree::new("route");
        let store = Store::new(t.root.join("cache"), Arc::new(|b: &[PathBuf]| b.iter().map(|_| Err("no converter".to_string())).collect()), Duration::from_millis(1));
        let (models, project) = (t.root.join("models"), t.root.join("project"));
        let get = |target: &str| reply_with(&project, target, &store, &models, &no_env, None);
        let enc = |s: &str| s.replace('%', "%25").replace('{', "%7B").replace('}', "%7D").replace('/', "%2F").replace(' ', "%20");

        let ok = get(&format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl")));
        assert_eq!((ok.status, ok.kind), ("200 OK", "model/vrml"));
        assert_eq!(ok.body, b"#VRML V2.0 utf8\n# a");
        for (target, status) in [
            (format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/../outside/secret.wrl")), "403 Forbidden"),
            ("/api/3dmodel?name=%2e%2e%2foutside%2fsecret.wrl".to_string(), "403 Forbidden"),
            // 404 where the temp directory is deep (macOS), 403 where the climb reaches the real /etc/passwd (Linux's /tmp): refused either way.
            ("/api/3dmodel?name=..%2F..%2F..%2Fetc%2Fpasswd".to_string(), "403 Forbidden|404 Not Found"),
            (format!("/api/3dmodel?name={}", enc(&t.root.join("outside/secret.wrl").to_string_lossy())), "403 Forbidden"),
            (format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/link.wrl")), "403 Forbidden"),
            (format!("/api/3dmodel?name={}", enc("${KIPRJMOD}/design.json")), "403 Forbidden"),
            ("/api/3dmodel?name=%zz".to_string(), "400 Bad Request"),
            ("/api/3dmodel?name=".to_string(), "400 Bad Request"),
            ("/api/3dmodel".to_string(), "400 Bad Request"),
            (format!("/api/3dmodel?name={}", enc("Lib.3dshapes/missing.wrl")), "404 Not Found"),
        ] {
            let r = get(&target);
            assert!(status.split('|').any(|s| s == r.status), "{target}: {} {}", r.status, String::from_utf8_lossy(&r.body));
            let body = String::from_utf8_lossy(&r.body);
            assert!(!body.contains("SECRET") && !body.contains("secret\":\"design") && !body.contains("root:"), "{target} leaked a file: {body}");
        }
    }

    /// A footprint of a team library writes its model as `${TEAM}/...`: the board's path variable (Configure Paths) gives the route the folder, as it gives
    /// a library its folder; a folder only the process environment names stays closed, and so does everything beside the folder the variable names.
    #[test]
    fn a_board_variable_reaches_a_model_and_the_environment_alone_does_not() {
        let m = crate::library_tables::tests::machine("model-variables", true);
        let (team, shell) = (m.root.join("team"), m.root.join("shell"));
        for d in [team.join("Lib.3dshapes"), shell.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(team.join("Lib.3dshapes/t.wrl"), "#VRML V2.0 utf8\n# t").unwrap();
        std::fs::write(shell.join("e.wrl"), "#VRML V2.0 utf8\n# e").unwrap();
        std::fs::write(m.root.join("secret.wrl"), "#VRML V2.0 utf8\n# SECRET").unwrap();
        eda_kicad::library_table::write_project_variables(&m.board, &[("TEAM".to_string(), team.display().to_string())].into()).unwrap();
        let mut env = m.env();
        env.real.insert("FROM_THE_SHELL".into(), shell.display().to_string());
        let names = crate::library_tables::model_names_with(&env, &m.board);
        let roots = Roots { models: m.share.join("3dmodels"), project: m.board.clone(), extra: names.folders.clone(), aliases: names.aliases.clone() };
        let store = Store::new(m.root.join("cache"), Arc::new(|b: &[PathBuf]| b.iter().map(|_| Err("no converter".to_string())).collect()), Duration::from_millis(1));
        let enc = |s: &str| s.replace('%', "%25").replace('{', "%7B").replace('}', "%7D").replace('/', "%2F");
        let get = |name: &str| reply_in(&format!("/api/3dmodel?name={}", enc(name)), &store, &roots, &|v| names.get(v), None);
        let ok = get("${TEAM}/Lib.3dshapes/t.wrl");
        assert_eq!((ok.status, ok.body.as_slice()), ("200 OK", b"#VRML V2.0 utf8\n# t".as_slice()), "a board variable names the folder");
        assert_eq!(get("${KIPRJMOD}/../shell/e.wrl").status, "403 Forbidden", "a .. out of the board's folder into one nobody named");
        assert_eq!(get("${FROM_THE_SHELL}/e.wrl").status, "403 Forbidden", "the environment defines it, but nobody named that folder for this board");
        assert_eq!(get("${TEAM}/../secret.wrl").status, "403 Forbidden", "only the folder the variable names, not what lies beside it");
        assert_eq!(get("${NOBODY_DEFINES_THIS}/x.wrl").status, "404 Not Found");
        // and `prepare` asks the same question
        let body = format!("{{\"names\":[\"${{TEAM}}/Lib.3dshapes/t.wrl\",\"${{FROM_THE_SHELL}}/e.wrl\",\"${{NOBODY_DEFINES_THIS}}/x.wrl\"]}}");
        assert_eq!(prepare_in(body.as_bytes(), &store, &roots, &|v| names.get(v)), json!({ "ready": 1, "pending": 0, "failed": 0, "missing": 2 }));
        let _ = std::fs::remove_dir_all(&m.root);
    }

    #[test]
    fn percent_decoding_is_encode_uri_components() {
        assert_eq!(percent_decode("${KICAD10_3DMODEL_DIR}%2FLib.3dshapes%2Fa%20b%2Bc%2C1.step").as_deref(), Some("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a b+c,1.step"));
        assert_eq!(percent_decode("a+b").as_deref(), Some("a+b"), "a plus is a plus");
        assert_eq!(percent_decode("%e2%82%ac").as_deref(), Some("\u{20ac}"), "UTF-8");
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%4"), None);
        assert_eq!(percent_decode("%gg"), None);
        assert_eq!(percent_decode("%ff"), None, "not UTF-8");
    }

    /// A converter that writes a VRML file per model and counts its runs and the size of each.
    fn counting_converter(dir: PathBuf, runs: Arc<Mutex<Vec<usize>>>) -> Converter {
        Arc::new(move |batch: &[PathBuf]| {
            runs.lock().unwrap().push(batch.len());
            std::fs::create_dir_all(&dir).unwrap();
            batch
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    if step.to_string_lossy().contains("bad") {
                        return Err("kicad-cli could not read this model".to_string());
                    }
                    let out = dir.join(format!("run{}-{i}.wrl", runs.lock().unwrap().len()));
                    std::fs::write(&out, format!("#VRML V2.0 utf8\n# {}", step.file_name().unwrap().to_string_lossy())).unwrap();
                    Ok(out)
                })
                .collect()
        })
    }

    fn wait_for(store: &Arc<Store>, step: &Path) -> Ask {
        for _ in 0..400 {
            match store.ask(step, false) {
                Ask::Pending => std::thread::sleep(Duration::from_millis(10)),
                other => return other,
            }
        }
        Ask::Pending
    }

    #[test]
    fn models_asked_for_together_are_converted_in_one_run_and_kept() {
        let t = Tree::new("store");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(120));
        let steps: Vec<PathBuf> = ["b.step", "C-d e.step"].iter().map(|n| t.path(&format!("models/Lib.3dshapes/{n}"))).collect();
        assert_eq!(store.ask(&steps[0], false), Ask::Pending, "the first ask starts the work and answers at once");
        assert_eq!(store.ask(&steps[1], false), Ask::Pending);
        let ready: Vec<Ask> = steps.iter().map(|s| wait_for(&store, s)).collect();
        for (step, got) in steps.iter().zip(&ready) {
            let Ask::Ready(path) = got else { panic!("{step:?} -> {got:?}") };
            assert!(path.starts_with(t.root.join("cache")), "kept in the cache: {path:?}");
            assert!(std::fs::read_to_string(path).unwrap().contains(step.file_name().unwrap().to_str().unwrap()), "each model has its own file");
        }
        assert_eq!(*runs.lock().unwrap(), vec![2], "two models, one run");
        // Asked again, a model is in the cache: no run. A new store over the same directory finds it too (a model is converted once, not once per session).
        assert!(matches!(store.ask(&steps[0], false), Ask::Ready(_)));
        let second = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(1));
        assert!(matches!(second.ask(&steps[1], false), Ask::Ready(_)), "found on disk");
        assert_eq!(runs.lock().unwrap().len(), 1, "and neither asked kicad-cli again");
        // The cached file is named after the source file's size and time: a model that changes is another file.
        let before = store.cache_file(&steps[0]);
        std::fs::write(&steps[0], "ISO-10303-21; changed").unwrap();
        assert_ne!(store.cache_file(&steps[0]), before);
    }

    #[test]
    fn a_failed_conversion_is_kept_until_a_retry_and_models_of_one_name_run_apart() {
        let t = Tree::new("failed");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(30));
        std::fs::write(t.root.join("models/Lib.3dshapes/bad.step"), "x").unwrap();
        std::fs::create_dir_all(t.root.join("models/Other.3dshapes")).unwrap();
        std::fs::write(t.root.join("models/Other.3dshapes/b.step"), "other b").unwrap();
        let bad = t.path("models/Lib.3dshapes/bad.step");
        assert_eq!(store.ask(&bad, false), Ask::Pending);
        let Ask::Failed(why) = wait_for(&store, &bad) else { panic!("a model kicad-cli cannot read fails") };
        assert!(why.contains("could not read"), "{why}");
        assert!(matches!(store.ask(&bad, false), Ask::Failed(_)), "kept: not run again on every poll");
        assert_eq!(runs.lock().unwrap().len(), 1);
        assert_eq!(store.ask(&bad, true), Ask::Pending, "a retry runs it again");
        assert!(matches!(wait_for(&store, &bad), Ask::Failed(_)));
        assert_eq!(runs.lock().unwrap().len(), 2);

        // Two models with one file name (one library's b.step and another's) would write one b.wrl: they are converted in runs of their own.
        let (b1, b2) = (t.path("models/Lib.3dshapes/b.step"), t.path("models/Other.3dshapes/b.step"));
        runs.lock().unwrap().clear();
        assert_eq!(store.ask(&b1, false), Ask::Pending);
        assert_eq!(store.ask(&b2, false), Ask::Pending);
        assert!(matches!(wait_for(&store, &b1), Ask::Ready(_)) && matches!(wait_for(&store, &b2), Ask::Ready(_)));
        assert_eq!(*runs.lock().unwrap(), vec![1, 1]);
    }

    /// A kicad-cli run costs 2 to 3.7 s of CPU whether it converts 2, 5 or 15 models (measured), so a burst is one run -- not a small first run and then the rest, which
    /// was tried and put the first model no sooner and the last one later -- up to [`MAX_BATCH`], and what does not fit follows in the next.
    #[test]
    fn a_burst_of_models_is_one_run_up_to_the_batch_limit() {
        let t = Tree::new("burst");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(120));
        let steps: Vec<PathBuf> = (0..MAX_BATCH + 3)
            .map(|i| {
                std::fs::write(t.root.join(format!("models/Lib.3dshapes/m{i}.step")), format!("ISO-10303-21; {i}")).unwrap();
                t.path(&format!("models/Lib.3dshapes/m{i}.step"))
            })
            .collect();
        for s in &steps {
            assert_eq!(store.ask(s, false), Ask::Pending);
        }
        for s in &steps {
            assert!(matches!(wait_for(&store, s), Ask::Ready(_)), "{s:?}");
        }
        assert_eq!(*runs.lock().unwrap(), vec![MAX_BATCH, 3], "a full run, then what did not fit");
    }

    fn delayed(inner: Converter, delay: Duration) -> Converter {
        Arc::new(move |batch: &[PathBuf]| {
            std::thread::sleep(delay);
            inner(batch)
        })
    }

    /// The page sends every model of the board in one `prepare` (one kicad-cli run converts them), then one request per model that waits for it on a thread of its
    /// own: the answer comes when the model is in, with no polling and so no timer in the page.
    #[test]
    fn prepare_queues_a_boards_models_together_and_a_waiting_request_is_answered_when_its_model_is_in() {
        let t = Tree::new("wait");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), delayed(counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(250)), Duration::from_millis(40));
        let (models, project) = (t.root.join("models"), t.root.join("project"));
        let body = json!({ "names": [
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step",
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/C-d e.step",
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl",
            "${KICAD10_3DMODEL_DIR}/../outside/secret.wrl",
            "nothing.step"
        ] })
        .to_string();
        let got = prepare_with(&project, body.as_bytes(), &store, &models, &no_env);
        assert_eq!(got, json!({ "ready": 1, "pending": 2, "failed": 0, "missing": 2 }), "a VRML is ready; two STEPs are queued; an escape and an unknown name are missing and not read");
        assert_eq!(prepare_with(&project, b"not json", &store, &models, &no_env), json!({ "ready": 0, "pending": 0, "failed": 0, "missing": 0 }));

        let enc = |s: &str| s.replace('{', "%7B").replace('}', "%7D").replace('/', "%2F").replace(' ', "%20");
        let started = Instant::now();
        let target = format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step"));
        let r = reply_with(&project, &target, &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!((r.status, r.kind), ("200 OK", "model/vrml"), "{}", String::from_utf8_lossy(&r.body));
        assert!(r.body.starts_with(b"#VRML V2.0 utf8") && started.elapsed() >= Duration::from_millis(200), "held until the conversion was over: {:?}", started.elapsed());
        let r2 = reply_with(&project, &format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/C-d e.step")), &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!(r2.status, "200 OK");
        assert_eq!(*runs.lock().unwrap(), vec![2], "the two prepared models were one run");

        // A wait that runs out answers pending: the page asks again.
        std::fs::write(t.root.join("models/Lib.3dshapes/slow.step"), "x").unwrap();
        let slow = format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/slow.step"));
        let short = reply_with(&project, &slow, &store, &models, &no_env, Some(Duration::from_millis(20)));
        assert_eq!(short.status, "202 Accepted", "{}", String::from_utf8_lossy(&short.body));
        let long = reply_with(&project, &slow, &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!(long.status, "200 OK");
    }

    #[test]
    fn only_wait_equals_one_holds_a_request() {
        assert!(wants_wait("/api/3dmodel?name=a&wait=1"));
        assert!(wants_wait("/api/3dmodel?wait=1&name=a"));
        assert!(!wants_wait("/api/3dmodel?name=a"));
        assert!(!wants_wait("/api/3dmodel?name=a&wait=0"));
        assert!(!wants_wait("/api/3dmodel?name=wait%3D1"));
        assert!(!wants_wait("/api/3dmodel"));
    }

    /// R1 (r2-editors-exports): `KIwxExpandEnvVars`' scanner -- a name is letters, digits, `_` and `:` only, a name that only contains `KISYS3DMOD` is the
    /// model library too, and a bare `$NAME` is a variable as well.
    #[test]
    fn variables_are_scanned_as_kiwxexpandenvvars_scans_them() {
        let roots = Roots { models: PathBuf::from("/m"), project: PathBuf::from("/p"), extra: Vec::new(), aliases: Vec::new() };
        let env = |n: &str| (n == "MY").then(|| "/my".to_string());
        assert_eq!(expand("${NO LIB}/x.wrl", &roots, &env), "${NO LIB}/x.wrl", "the name `NO` ends at the space and is no variable");
        assert_eq!(expand("${MY LIB}/x.wrl", &roots, &env), "/my LIB}/x.wrl", "`MY` is one, and the bracket left open is text (KiCad's warning, no skip)");
        assert_eq!(expand("${MY}/x.wrl", &roots, &env), "/my/x.wrl");
        assert_eq!(expand("$MY/x.wrl", &roots, &env), "/my/x.wrl", "Bracket_None");
        assert_eq!(expand("$(MY)/x.wrl", &roots, &env), "/my/x.wrl");
        assert_eq!(expand("${OLD_KISYS3DMOD}/a.wrl", &roots, &env), "/m/a.wrl", "`strVarName.Contains( \"KISYS3DMOD\" )`");
        assert_eq!(expand("${KICAD6_3DMODEL_DIR}/a.wrl", &roots, &env), "/m/a.wrl");
        assert_eq!(expand("${KIPRJMOD}/b.step", &roots, &env), "/p/b.step");
        assert_eq!(expand("${NOPE}/b.step", &roots, &env), "${NOPE}/b.step");
        assert_eq!(expand("a$", &roots, &env), "a", "r3 R3-1: a trailing `$` is dropped");
        assert_eq!(expand("a${", &roots, &env), "a");
        assert_eq!(expand("a$(", &roots, &env), "a");
    }

    #[test]
    fn the_parts_models_go_out_with_their_placement_and_kind() {
        let part = Part { reference: "R1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![], body_nm: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part.clone()], ..Default::default() };
        let fp = model.footprint_of(&part).unwrap();
        let mut design: eda_model::ir::Design = serde_json::from_value(json!({ "provenance": { "engine_version": "t", "intent_hash": "x", "seed": 0, "stage_hashes": [] } })).unwrap();
        let v = part_json(&design, &model, &part, &fp).expect("a 0603 has a model");
        assert_eq!((v["kind3d"].clone(), v["dnp"].clone(), v["not_in_pos"].clone()), (json!("smd"), json!(false), json!(false)));
        // V2 (r2-editors-exports): Footprint Properties' Component type and flags decide, as `IsFootprintShown` reads the board footprint's.
        let attrs = eda_model::fp_edit::FootprintAttrs { kind: eda_model::fp_edit::FootprintKind::Unspecified, dnp: true, exclude_from_pos_files: true, ..Default::default() };
        design.drawings.get_or_insert_with(Default::default).footprint_edits.push(eda_model::fp_edit::FootprintEdit { id: "R1".into(), attrs: Some(attrs), ..Default::default() });
        let v = part_json(&design, &model, &part, &fp).unwrap();
        assert_eq!((v["kind3d"].clone(), v["dnp"].clone(), v["not_in_pos"].clone()), (json!("virtual"), json!(true), json!(true)));
        let m = &v["models"][0];
        assert!(m["name"].as_str().unwrap().ends_with("Resistor_SMD.3dshapes/R_0603_1608Metric.step"), "{m}");
        assert_eq!((m["scale"].clone(), m["opacity"].clone(), m["show"].clone()), (json!([1.0, 1.0, 1.0]), json!(1.0), json!(true)));
        let none = Part { package: Some("NO-SUCH-PACKAGE".into()), footprint: Some("NO-SUCH-PACKAGE".into()), ..part.clone() };
        assert!(model.footprint_of(&none).is_none(), "no footprint, no models");
    }

    /// The real thing, with KiCad.app installed (a kicad-cli round trip: the slow tier): a STEP of the installed library becomes VRML in KiCad's units.
    #[test]
    fn a_real_step_model_is_converted_by_kicad_cli() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            return;
        }
        let step = models_dir().join("Resistor_SMD.3dshapes/R_0603_1608Metric.step");
        if !step.is_file() {
            return;
        }
        let t = Tree::new("real");
        let store = Store::new(t.root.join("cache"), kicad_cli_converter(Arc::new(Lane::default()), t.root.join("cache")), Duration::from_millis(1));
        let Ask::Ready(vrml) = wait_for_long(&store, &step.canonicalize().unwrap()) else { panic!("converted") };
        let text = std::fs::read_to_string(vrml).unwrap();
        assert!(text.starts_with("#VRML V2.0 utf8"), "{}", &text[..text.len().min(80)]);
        assert!(text.contains("IndexedFaceSet"), "geometry");
    }

    /// An X3D cube of side 1 (KiCad reads an X3D, like a VRML, in units of 0.1 inch) in one red `Shape`.
    pub(crate) const X3D_CUBE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<X3D profile="Immersive" version="3.2"><Scene><Transform><Shape>
<Appearance><Material diffuseColor="0.8 0.1 0.1"/></Appearance>
<IndexedFaceSet coordIndex="0 1 2 3 -1 7 6 5 4 -1 0 4 5 1 -1 1 5 6 2 -1 2 6 7 3 -1 3 7 4 0 -1"><Coordinate point="0 0 0 1 0 0 1 1 0 0 1 0 0 0 1 1 0 1 1 1 1 0 1 1"/></IndexedFaceSet>
</Shape></Transform></Scene></X3D>
"#;

    /// A minimal IGES 5.3 file, millimetres: a box of `size` made of six bilinear B-spline surfaces (entity 128), the fixed 80-column records KiCad's
    /// `fileType` recognises (`iline[72] == 'S'`) and OpenCascade's IGES reader takes.
    pub(crate) fn iges_box(size: [f64; 3]) -> String {
        let line = |text: &str, sec: char, n: usize| format!("{text:<72}{sec}{n:>7}\n");
        let out = line("eda hand-written IGES: a box of six B-spline faces", 'S', 1);
        let global = "1H,,1H;,3Hbox,7Hbox.igs,3Heda,3H1.0,32,38,6,308,15,3Hbox,1.,2,2HMM,1,1.,15H20261106.000000,0.001,10.,3Heda,3Heda,11,0,15H20261106.000000;";
        let g: Vec<String> = global.as_bytes().chunks(72).enumerate().map(|(i, c)| line(std::str::from_utf8(c).unwrap(), 'G', i + 1)).collect();
        let [x, y, z] = size;
        let faces = [
            [[0.0, 0.0, 0.0], [0.0, y, 0.0], [x, 0.0, 0.0], [x, y, 0.0]],
            [[0.0, 0.0, z], [x, 0.0, z], [0.0, y, z], [x, y, z]],
            [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [0.0, 0.0, z], [x, 0.0, z]],
            [[0.0, y, 0.0], [0.0, y, z], [x, y, 0.0], [x, y, z]],
            [[0.0, 0.0, 0.0], [0.0, 0.0, z], [0.0, y, 0.0], [0.0, y, z]],
            [[x, 0.0, 0.0], [x, y, 0.0], [x, 0.0, z], [x, y, z]],
        ];
        let (mut d, mut p): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for (i, face) in faces.iter().enumerate() {
            let de = 2 * i + 1;
            let mut params: Vec<String> = "128,1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.".split(',').map(String::from).collect();
            params.extend(face.iter().flatten().map(|c| format!("{c:?}")));
            params.extend(["0.", "1.", "0.", "1."].map(String::from));
            let mut text = params.join(",") + ";";
            let first = p.len() + 1;
            while !text.is_empty() {
                let cut = if text.len() > 64 { text[..64].rfind(',').unwrap() + 1 } else { text.len() };
                p.push(format!("{:<64}{de:>8}P{:>7}\n", &text[..cut], p.len() + 1));
                text = text[cut..].to_string();
            }
            d.push(line(&format!("{:>8}{first:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}", 128, 0, 0, 0, 0, 0, 0, "00000000"), 'D', de));
            d.push(line(&format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}", 128, 0, 0, p.len() + 1 - first, 0, "", "", "", 0), 'D', de + 1));
        }
        let t = line(&format!("S{:>7}G{:>7}D{:>7}P{:>7}", 1, g.len(), d.len(), p.len()), 'T', 1);
        out + &g.concat() + &d.concat() + &p.concat() + &t
    }

    /// The slow tier: kicad-cli converts the formats it reads besides STEP -- a hand-written X3D and IGES (`.igs` and `.iges`) -- into VRML in KiCad's units, in one run.
    #[test]
    fn x3d_and_iges_models_are_converted_by_kicad_cli_as_a_step_is() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            return;
        }
        let t = Tree::new("formats-real");
        let dir = t.root.join("project/prj.3dshapes");
        std::fs::write(dir.join("cube.x3d"), X3D_CUBE).unwrap();
        std::fs::write(dir.join("box.igs"), iges_box([2.0, 1.0, 0.5])).unwrap();
        std::fs::write(dir.join("tall.iges"), iges_box([1.0, 1.0, 3.0])).unwrap();
        assert!(iges_box([2.0, 1.0, 0.5]).lines().all(|l| l.len() == 80), "IGES records are 80 columns");
        let store = Store::new(t.root.join("cache"), kicad_cli_converter(Arc::new(Lane::default()), t.root.join("cache")), Duration::from_millis(1));
        let files: Vec<PathBuf> = ["cube.x3d", "box.igs", "tall.iges"].iter().map(|f| dir.join(f).canonicalize().unwrap()).collect();
        for f in &files {
            assert_eq!(format_of(f), Some(Format::Converted));
            assert!(matches!(store.ask(f, false), Ask::Pending | Ask::Ready(_)));
        }
        for (f, top) in files.iter().zip([1.0, 0.5 / 2.54, 3.0 / 2.54]) {
            let Ask::Ready(vrml) = wait_for_long(&store, f) else { panic!("{} converted", f.display()) };
            let text = std::fs::read_to_string(vrml).unwrap();
            assert!(text.starts_with("#VRML V2.0 utf8") && text.contains("IndexedFaceSet"), "{}: {}", f.display(), &text[..text.len().min(80)]);
            // the highest point, in 0.1 inch: the cube's 1 as it is, the IGES boxes' millimetres divided by 2.54
            let zs: Vec<f64> = text.split("point [").skip(1).flat_map(|b| b[..b.find(']').unwrap()].split(',').filter_map(|p| p.split_whitespace().nth(2)?.parse::<f64>().ok()).collect::<Vec<_>>()).collect();
            let max = zs.iter().cloned().fold(f64::MIN, f64::max);
            assert!((max - top).abs() < 1e-4, "{}: top at {max}, want {top}", f.display());
        }
    }

    fn wait_for_long(store: &Arc<Store>, step: &Path) -> Ask {
        for _ in 0..3000 {
            match store.ask(step, false) {
                Ask::Pending => std::thread::sleep(Duration::from_millis(20)),
                other => return other,
            }
        }
        Ask::Pending
    }
}
