//! Rescue Symbols (`eeschema.EditorControl.rescueSymbols`, UI-LEFT A2): `SCH_EDITOR_CONTROL::RescueSymbols` -> `RescueSymbolLibTableProject` ->
//! `RESCUER::RescueProject` with a `SYMBOL_LIB_TABLE_RESCUER` (eeschema/project_rescue.cpp, eeschema/tools/sch_editor_control.cpp at KiCad 8303b2ad).
//!
//! * `GET /api/sch/rescue` is `FindCandidates` + `RemoveDuplicates` (`RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::FindRescues`): the symbols of every screen grouped by
//!   library identifier, each group's cached symbol and the library symbol the board's tables give compared, and a candidate made where the cached one
//!   is the only one, or where its pins conflict with the library's (`PinsConflictWith( .., true, true, true, true, false )`: number, name, type and
//!   orientation of a pin at each place, not its length), or where the item name has a character a library identifier may not hold
//!   (`LIB_ID::HasIllegalChars`). Each candidate carries its action as `GetActionDescription` words it, both symbols for the dialog's two previews
//!   (`DIALOG_RESCUE_EACH::displayItemsInConflict`) and its instances (`PopulateInstanceList`: reference and value).
//! * `POST /api/sch/rescue` `{requested: [lib_id]}` is the rest for the candidates the dialog's Accept column kept (`TransferDataFromWindow`):
//!   `OpenRescueLibrary` (what a rescue library already holds is kept), each `PerformAction` (the cached symbol, else the library's, flattened and named
//!   by the new identifier, `<project>-rescue:<name>-<nickname>`) and `WriteRescueLibrary`: `<project>-rescue.kicad_sym` beside the board and, when no
//!   table has a row of that nickname (or the row is a legacy one), the row `${KIPRJMOD}/<project>-rescue.kicad_sym` of type KiCad in the project's table.
//!   The answer holds the schematic's half as one command (`Cmd::RescueSymbols`), which the studio sends as one undoable `/api/cmd` step.
//!
//! The project is the board: its name is the intent's file name (what the studio's state calls the board and the derived project's root sheet is
//! named after), its folder `${KIPRJMOD}`.
//!
//! **Differs**: KiCad's cached symbol is the legacy cache library's only (`PROJECT_SCH::LegacySchLibs`, the project's `<project>-cache.lib`, looked up
//! by `Lib:Name`, then `Lib-Name`); a KiCad 6 or later schematic's own `lib_symbols` are not consulted, so a project with no cache library has nothing
//! to rescue there. The studio reads the board folder's cache library the same way (through kicad-cli's conversion of the legacy file,
//! `crate::library_convert`) and, for a symbol it does not hold, the design's own copy of the symbol (`design.symbol_library`, what a `.kicad_sch`'s
//! `lib_symbols` are): a symbol whose library is gone, or whose library symbol's pins changed, can be rescued from the copy the schematic keeps. The
//! studio's own project library (`eda:`) and generated symbols (`gen:`) have no library file and are never candidates.
//!
//! Not ported: the legacy flavour (`LEGACY_RESCUER`: `RESCUE_CASE_CANDIDATE` and `RESCUE_CACHE_CANDIDATE`, for a schematic none of whose symbols names
//! a library, `SCH_SCREENS::HasNoFullyDefinedLibIds`), which writes a legacy `.lib` rescue library and the project's legacy library list; the studio has
//! no legacy library writer. The answer says so (`legacy`). (Such a schematic -- a KiCad 5 `.sch` -- is brought in with its cache library and project file
//! since `task-v10-legacy`: `crate::legacy_import`, PARITY-sch.md section 90.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eda_kicad::library_table::TableKind;
use eda_model::ir::{Design, LibrarySymbol};
use serde_json::{json, Value};

use crate::board;

/// One placed symbol, as the rescuer lists it (`getSymbols`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    pub lib_id: String,
    pub unit: u32,
    pub body_style: u32,
    pub reference: String,
    pub value: String,
}

/// `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    /// `m_requested_id`: the library identifier the symbols name.
    pub requested: String,
    /// `m_new_id`: its identifier in the rescue library.
    pub new_id: String,
    /// `m_cache_candidate`.
    pub cache: Option<LibrarySymbol>,
    /// `m_lib_candidate`: the library's symbol (a derived one's root).
    pub lib: Option<LibrarySymbol>,
    pub unit: u32,
    pub body_style: u32,
}

impl Candidate {
    /// `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::GetActionDescription`.
    pub(crate) fn action(&self) -> String {
        let item = self.requested.split_once(':').map_or(self.requested.as_str(), |(_, n)| n);
        match (&self.cache, &self.lib) {
            (None, None) => format!("Cannot rescue symbol {} which is not available in any library or the cache.", unescape_lib_id(item)),
            (Some(_), None) => format!("Rescue symbol {} found only in cache library to {}.", unescape_lib_id(&self.requested), unescape_lib_id(&self.new_id)),
            _ => format!("Rescue modified symbol {} to {}", unescape_lib_id(&self.requested), unescape_lib_id(&self.new_id)),
        }
    }
}

/// `LIB_ID::HasIllegalChars`: the byte offset of the first character `LIB_ID::isLegalChar` refuses (`:`, a tab, a line break, `\`, `<`, `>`, `"`), or `None`.
pub(crate) fn has_illegal_chars(name: &str) -> Option<usize> {
    name.char_indices().find(|(_, c)| matches!(c, ':' | '\t' | '\n' | '\r' | '\\' | '<' | '>' | '"')).map(|(i, _)| i)
}

/// `EscapeString( aSource, CTX_LIBID )` (common/string_utils.cpp).
pub(crate) fn escape_lib_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("{backslash}"),
            '<' => out.push_str("{lt}"),
            '>' => out.push_str("{gt}"),
            ':' => out.push_str("{colon}"),
            '"' => out.push_str("{dblquote}"),
            '\n' | '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// `UnescapeString` of what [`escape_lib_id`] makes.
pub(crate) fn unescape_lib_id(s: &str) -> String {
    s.replace("{backslash}", "\\").replace("{lt}", "<").replace("{gt}", ">").replace("{colon}", ":").replace("{dblquote}", "\"")
}

/// `LIB_SYMBOL::PinsConflictWith( aOtherPart, aTestNums, aTestNames, aTestType, aTestOrientation, aTestLength )`: some pin of `a` (every unit and body
/// style, `GetGraphicalPins()`) has no pin of `b` of its unit and body style at its place that agrees in what is tested.
pub(crate) fn pins_conflict_with(a: &LibrarySymbol, b: &LibrarySymbol, nums: bool, names: bool, types: bool, orientation: bool, length: bool) -> bool {
    // positions are read from text in millimetres: two that print alike are alike (KiCad compares its integer nanometres)
    let same = |x: f64, y: f64| (x - y).abs() < 5e-7;
    a.pins.iter().any(|pin| {
        !b.pins.iter().any(|other| {
            pin.unit == other.unit
                && pin.body_style == other.body_style
                && same(pin.at.x, other.at.x)
                && same(pin.at.y, other.at.y)
                && (!nums || pin.number == other.number)
                && (!names || pin.name == other.name)
                && (!types || pin.electrical_type == other.electrical_type)
                && (!orientation || same(pin.angle_deg.rem_euclid(360.0), other.angle_deg.rem_euclid(360.0)))
                && (!length || same(pin.length_mm, other.length_mm))
        })
    })
}

/// `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::FindRescues`, then `RESCUER::RemoveDuplicates`. `placed` in any order (it is sorted by library identifier here, as
/// `getSymbols` sorts it); `cache_of` and `lib_of` answer a library identifier with the cached and the library's symbol. `rescue_nickname` is the rescue
/// library's (`GetRescueLibraryFileName( schematic ).GetName()`). The candidates come out in library identifier order (`std::map<LIB_ID, ..>`).
pub(crate) fn find_rescues(placed: &[Placed], rescue_nickname: &str, cache_of: &dyn Fn(&str) -> Option<LibrarySymbol>, lib_of: &dyn Fn(&str) -> Option<LibrarySymbol>) -> Vec<Candidate> {
    let mut sorted: Vec<&Placed> = placed.iter().collect();
    let key = |id: &str| -> (String, String) {
        let (lib, item) = id.split_once(':').unwrap_or(("", id));
        (lib.to_string(), item.to_string())
    };
    sorted.sort_by_key(|p| key(&p.lib_id));
    let mut map: BTreeMap<(String, String), Candidate> = BTreeMap::new();
    let mut last: Option<&str> = None;
    for p in sorted {
        if last == Some(p.lib_id.as_str()) {
            continue;
        }
        last = Some(p.lib_id.as_str());
        let (nickname, item) = key(&p.lib_id);
        let cache = cache_of(&p.lib_id);
        let lib = lib_of(&p.lib_id);
        if cache.is_none() && lib.is_none() {
            continue;
        }
        if has_illegal_chars(&item).is_none() {
            if let (Some(c), Some(l)) = (&cache, &lib) {
                if !pins_conflict_with(c, l, true, true, true, true, false) {
                    continue;
                }
            }
            if cache.is_none() && lib.is_some() {
                continue;
            }
        }
        let new_id = format!("{rescue_nickname}:{}-{nickname}", escape_lib_id(&item));
        map.insert((nickname, item), Candidate { requested: p.lib_id.clone(), new_id, cache, lib, unit: p.unit, body_style: p.body_style });
    }
    map.into_values().collect()
}

/// `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::PerformAction`'s symbol: the cached one, else the library's, flattened (both are flat here), named by the new identifier.
pub(crate) fn rescued_symbol(c: &Candidate) -> Option<LibrarySymbol> {
    let mut s = c.cache.clone().or_else(|| c.lib.clone())?;
    s.lib_id = c.new_id.clone();
    s.extends = None;
    Some(s)
}

/// `SCH_SCREENS::HasNoFullyDefinedLibIds`: there are symbols, and none names a library nickname.
pub(crate) fn has_no_fully_defined_lib_ids(design: &Design) -> bool {
    let ids: Vec<&str> = screens(design).flat_map(|s| s.symbols.iter().map(|x| x.lib_id.as_str()).chain(s.power_symbols.iter().map(|p| p.lib_id.as_str()))).collect();
    !ids.is_empty() && !ids.iter().any(|id| id.split_once(':').is_some_and(|(l, _)| !l.is_empty()))
}

/// `SCH_SCREENS( Root )`: the root and every screen some sheet shows.
fn screens(design: &Design) -> impl Iterator<Item = &eda_model::ir::SchematicSection> {
    let reachable = eda_model::sheet_list::reachable_files(design);
    design.schematic.iter().chain(design.sheet_contents.iter().flat_map(move |c| c.iter().filter(|(f, _)| reachable.contains(*f)).map(|(_, s)| s).collect::<Vec<_>>()))
}

/// `getSymbols`: every symbol and power symbol of the schematic, the generated and project-library ones left out (they have no library to compare with).
pub(crate) fn placed_symbols(design: &Design, model: &eda_model::ConstraintModel) -> Vec<Placed> {
    let mut out = Vec::new();
    for sch in screens(design) {
        for s in &sch.symbols {
            if eda_model::symbol::is_synthetic_lib_id(&s.lib_id) || s.lib_id.is_empty() {
                continue;
            }
            let value = if s.value.is_empty() { model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() };
            out.push(Placed { lib_id: s.lib_id.clone(), unit: s.unit, body_style: sch.body_style_of(s), reference: s.id.clone(), value });
        }
        for p in &sch.power_symbols {
            if eda_model::symbol::is_synthetic_lib_id(&p.lib_id) || p.lib_id.is_empty() {
                continue;
            }
            out.push(Placed { lib_id: p.lib_id.clone(), unit: 1, body_style: 1, reference: p.id.clone(), value: p.net.clone() });
        }
    }
    out
}

/// The board's project name: the intent's file name (`board` when it has none).
pub(crate) fn project_name(meta: &board::Meta) -> String {
    Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string()
}

/// `LEGACY_SYMBOL_LIBS::CacheName`: `<project>-cache.lib` in the project's folder, else the old (2007) `<project>.cache.lib`; none when neither is there.
pub(crate) fn cache_library_path(dir: &Path, project: &str) -> Option<PathBuf> {
    [format!("{project}-cache.lib"), format!("{project}.cache.lib")].into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// The symbols of the legacy cache library at `path`, by name, read through kicad-cli's conversion (`crate::library_convert`); none when it cannot be read.
fn cache_library_symbols(path: &Path) -> BTreeMap<String, LibrarySymbol> {
    let Ok(converted) = crate::library_convert::readable(TableKind::Symbol, path) else { return BTreeMap::new() };
    let Ok(text) = eda_kicad::read_symbol_library_text(&converted) else { return BTreeMap::new() };
    let Ok(parsed) = eda_kicad::parse_library_symbols(&text) else { return BTreeMap::new() };
    let section = eda_model::ir::SymbolLibrarySection { symbols: parsed.symbols, ..Default::default() };
    section.symbols.iter().filter_map(|s| section.flatten(&s.lib_id).ok().map(|f| (s.lib_id.clone(), f))).collect()
}

/// The cached symbol of `lib_id`: the cache library's (`findSymbol( symbolName, LegacySchLibs, true )` by `Lib:Name`, then `Lib-Name`), else the design's own
/// copy (the module doc's **Differs**).
fn cached(cache: &BTreeMap<String, LibrarySymbol>, design: &Design, lib_id: &str) -> Option<LibrarySymbol> {
    let (lib, item) = lib_id.split_once(':').unwrap_or(("", lib_id));
    if let Some(s) = cache.get(lib_id).or_else(|| cache.get(&format!("{lib}-{item}"))) {
        return Some(s.clone());
    }
    let section = design.symbol_library.as_ref()?;
    section.by_lib_id(lib_id)?;
    let mut s = section.flatten(lib_id).ok()?;
    s.lib_id = lib_id.to_string();
    Some(s)
}

/// The library symbol of `lib_id` the board's tables give (`SchGetLibSymbol`; the library reader flattens a derived one, which is its root's pins).
fn library_symbol(src: &crate::library_tables::LibSet, lib_id: &str) -> Option<LibrarySymbol> {
    crate::library_search::read_installed_symbol(src, lib_id).ok().flatten().map(|(s, _)| s)
}

/// The candidates of the board in `dir`, and the rescue library's nickname.
fn candidates_of(dir: &Path) -> Result<(Vec<Candidate>, Vec<Placed>, String, bool), String> {
    let (meta, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let project = project_name(&meta);
    let legacy = has_no_fully_defined_lib_ids(&design);
    let placed = placed_symbols(&design, &model);
    if legacy {
        return Ok((Vec::new(), placed, format!("{project}-rescue"), true));
    }
    let cache = cache_library_path(dir, &project).map(|p| cache_library_symbols(&p)).unwrap_or_default();
    let src = crate::library_tables::libs(dir, crate::library_index::Kind::Symbol);
    let nickname = format!("{project}-rescue");
    let found = find_rescues(&placed, &nickname, &|id| cached(&cache, &design, id), &|id| library_symbol(&src, id));
    Ok((found, placed, nickname, false))
}

fn candidate_json(c: &Candidate, placed: &[Placed]) -> Value {
    let instances: Vec<Value> = placed.iter().filter(|p| p.lib_id == c.requested).map(|p| json!({ "reference": p.reference, "value": p.value })).collect();
    json!({
        "requested": c.requested,
        "new_id": c.new_id,
        "action": c.action(),
        "cache": c.cache,
        "lib": c.lib,
        "unit": c.unit,
        "body_style": c.body_style,
        "instances": instances,
    })
}

/// `GET /api/sch/rescue`: `{legacy, library, candidates: [{requested, new_id, action, cache, lib, unit, body_style, instances: [{reference, value}]}]}` or `{error}`.
pub(crate) fn candidates(dir: &Path) -> Value {
    match candidates_of(dir) {
        Ok((found, placed, nickname, legacy)) => json!({
            "legacy": legacy,
            "library": nickname,
            "candidates": found.iter().map(|c| candidate_json(c, &placed)).collect::<Vec<_>>(),
        }),
        Err(e) => json!({ "error": e }),
    }
}

/// `POST /api/sch/rescue` `{requested: [lib_id]}`: the rescue library written and put in the project's table, and the command that relinks the symbols
/// (`{ok, library: path, cmd}`), or `{error}`. Nothing is written when no requested identifier is a candidate.
pub(crate) fn rescue(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("bad request: {e}") }),
    };
    let wanted: Vec<String> = req.get("requested").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let (found, _, nickname, legacy) = match candidates_of(dir) {
        Ok(x) => x,
        Err(e) => return json!({ "error": e }),
    };
    if legacy {
        return json!({ "error": "The legacy symbol rescue (a schematic whose symbols name no library) is not available here: it writes a legacy symbol library." });
    }
    let chosen: Vec<&Candidate> = found.iter().filter(|c| wanted.contains(&c.requested)).collect();
    // `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::PerformAction`: a candidate with neither symbol has nothing to put in the library (KiCad's `wxCHECK_MSG( tmp, false, ..)`)
    let rescued: Vec<(String, String, LibrarySymbol)> = chosen.iter().filter_map(|c| rescued_symbol(c).map(|s| (c.requested.clone(), c.new_id.clone(), s))).collect();
    if rescued.is_empty() {
        return json!({ "error": "No symbols were rescued." });
    }
    match write_rescue_library(dir, &nickname, &rescued.iter().map(|(_, _, s)| s.clone()).collect::<Vec<_>>()) {
        Ok(path) => {
            let changes: Vec<(String, String)> = rescued.iter().map(|(from, to, _)| (from.clone(), to.clone())).collect();
            let symbols: Vec<&LibrarySymbol> = rescued.iter().map(|(_, _, s)| s).collect();
            json!({ "ok": true, "library": path.display().to_string(), "cmd": { "op": "rescue_symbols", "changes": changes, "symbols": symbols } })
        }
        Err(e) => json!({ "error": e }),
    }
}

/// `SYMBOL_LIB_TABLE_RESCUER::OpenRescueLibrary` + `WriteRescueLibrary`: `symbols` saved into `<nickname>.kicad_sym` in `dir` (the symbols the file
/// already holds kept, one of the same name replaced), and the project table's row `nickname` -> `${KIPRJMOD}/<nickname>.kicad_sym` of type KiCad when no
/// table has a row of that nickname, or the row there is a legacy one. Returns the library's path.
pub(crate) fn write_rescue_library(dir: &Path, nickname: &str, symbols: &[LibrarySymbol]) -> Result<PathBuf, String> {
    let file = format!("{nickname}.kicad_sym");
    let path = dir.join(&file);
    let saves: Vec<eda_kicad::library_save::SymbolSave> = symbols.iter().map(|s| eda_kicad::library_save::SymbolSave { symbol: s, embedded: None }).collect();
    eda_kicad::library_save::save_symbols(&path, &saves).map_err(|e| format!("Failed to save rescue library {}.\n{e}", path.display()))?;
    let row = crate::library_tables::load(dir, TableKind::Symbol).libs().into_iter().find(|l| l.nickname == nickname);
    if row.as_ref().is_none_or(|r| r.lib_type == "Legacy") {
        crate::library_tables::set_project_row(dir, TableKind::Symbol, nickname, &format!("${{KIPRJMOD}}/{file}"), eda_kicad::library_table::KICAD_TYPE)
            .map_err(|e| format!("Error saving library table:\n\n{e}"))?;
    }
    Ok(path)
}

// ------------------------------------------------------------------------------------------------ Remap Legacy Library Symbols

/// One line of the remap dialog's report (`WX_HTML_REPORT_PANEL`): `info`, `warning` or `action`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReportLine {
    pub severity: &'static str,
    pub text: String,
}

fn line(severity: &'static str, text: String) -> ReportLine {
    ReportLine { severity, text }
}

/// The project's legacy symbol library list (`PROJECT_FILE` `schematic.legacy_lib_dir` / `legacy_lib_list`, which KiCad migrates from a KiCad 5 `.pro`'s
/// `[eeschema] LibDir=` and `[eeschema/libraries] LibName<n>=`): the board folder's `<project>.kicad_pro`, else its `<project>.pro`. `(lib_dir, names)`.
pub(crate) fn legacy_library_list(dir: &Path, project: &str) -> (String, Vec<String>) {
    if let Ok(text) = std::fs::read_to_string(dir.join(format!("{project}.kicad_pro"))) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let sch = &v["schematic"];
            let names: Vec<String> = sch["legacy_lib_list"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default();
            if !names.is_empty() {
                return (sch["legacy_lib_dir"].as_str().unwrap_or("").to_string(), names);
            }
        }
    }
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{project}.pro"))) else { return (String::new(), Vec::new()) };
    let (mut section, mut lib_dir, mut names) = (String::new(), String::new(), BTreeMap::new());
    for l in text.lines().map(str::trim) {
        if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            section = name.to_string();
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        match section.as_str() {
            "eeschema" if k == "LibDir" => lib_dir = v.to_string(),
            "eeschema/libraries" => {
                if let Some(n) = k.strip_prefix("LibName").and_then(|n| n.parse::<u32>().ok()) {
                    names.insert(n, v.to_string());
                }
            }
            _ => {}
        }
    }
    (lib_dir, names.into_values().collect())
}

/// `LEGACY_SYMBOL_LIBS::LoadAllLibraries`' file of each listed library: `<name>.lib` as it is when it exists (beside the board for a relative name), else
/// found in the library folders (`SchSearchS`: the project's folder, then each of `LibDir`'s `;`-separated folders, relative to the project). The names
/// not found come back as `Err(path)`.
pub(crate) fn legacy_library_files(dir: &Path, lib_dir: &str, names: &[String]) -> Vec<(String, Result<PathBuf, PathBuf>)> {
    names
        .iter()
        .map(|n| {
            let file = PathBuf::from(format!("{n}.lib"));
            let direct = if file.is_absolute() { file.clone() } else { dir.join(&file) };
            if direct.is_file() {
                return (n.clone(), Ok(direct));
            }
            let found = lib_dir.split(';').filter(|d| !d.is_empty()).map(|d| if Path::new(d).is_absolute() { PathBuf::from(d) } else { dir.join(d) }).map(|d| d.join(&file)).find(|p| p.is_file());
            (n.clone(), found.ok_or(direct))
        })
        .collect()
}

/// The symbol names a legacy library holds, read through kicad-cli's conversion; none when it cannot be read.
fn legacy_symbol_names(path: &Path) -> Vec<String> {
    let Ok(converted) = crate::library_convert::readable(TableKind::Symbol, path) else { return Vec::new() };
    eda_kicad::read_symbol_library_text(&converted).map(|t| eda_kicad::scan_symbols(&t).into_iter().map(|s| s.name).collect()).unwrap_or_default()
}

/// `DIALOG_SYMBOL_REMAP::backupProject`: the symbol library table, the schematic (the design, `design.json`), the project file, the cache libraries and the
/// rescue library copied into `rescue-backup/` with `-%Y-%m-%d-%H-%M-%S` after the name. A file that is not there is reported and skipped, as KiCad does.
fn backup_project(dir: &Path, project: &str, stamp: &str, report: &mut Vec<ReportLine>) -> Result<(), String> {
    let backup = dir.join("rescue-backup");
    std::fs::create_dir_all(&backup).map_err(|_| format!("Cannot create project remap back up folder '{}'.", backup.display()))?;
    let mut errors = String::new();
    let stamped = |name: &str| -> String {
        let p = Path::new(name);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match p.extension() {
            Some(ext) => format!("{stem}{stamp}.{}", ext.to_string_lossy()),
            _ => format!("{name}{stamp}"),
        }
    };
    let pro = if dir.join(format!("{project}.kicad_pro")).is_file() { format!("{project}.kicad_pro") } else { format!("{project}.pro") };
    for name in ["sym-lib-table".to_string(), "design.json".to_string(), pro, format!("{project}-cache.lib"), format!("{project}.cache.lib"), format!("{project}-rescue.lib"), format!("{project}-rescue.dcm")] {
        let (src, dst) = (dir.join(&name), backup.join(stamped(&name)));
        report.push(line("info", format!("Backing up file '{}' to '{}'.", src.display(), dst.display())));
        if src.is_file() && std::fs::copy(&src, &dst).is_err() {
            errors.push_str(&format!("Failed to back up file '{}'.\n", src.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// `DIALOG_SYMBOL_REMAP::OnRemapSymbols` for the board in `dir`: the project backed up; the project's symbol library table removed and made again from the
/// legacy libraries no table names (`createProjectSymbolLibTable`: a row of type Legacy per library found, its nickname the library's name with spaces as
/// `-` and a number added while a library of that name exists); and each symbol that names no library given the nickname of the first legacy library that
/// holds its name, found in the tables by its file (`remapSymbolToLibTable`, `FindLibraryByURI`). Returns the report and the identifier changes
/// (`bare name -> Library:Name`), which the studio applies as one `Cmd::RescueSymbols`. **Differs**: the rescue KiCad runs first (`LEGACY_RESCUER`, a
/// legacy `.lib` rescue library) is not ported, and the project file's legacy library list is left as it is.
pub(crate) fn remap_plan(dir: &Path, design: &Design, project: &str, stamp: &str) -> Result<(Vec<ReportLine>, Vec<(String, String)>), String> {
    let mut report = Vec::new();
    if let Err(e) = backup_project(dir, project, stamp, &mut report) {
        return Err(e);
    }
    // "Delete the existing project symbol library table."
    let table = crate::library_tables::project_table_path(dir, TableKind::Symbol);
    if table.is_file() {
        std::fs::remove_file(&table).map_err(|e| format!("{}: {e}", table.display()))?;
    }
    let (lib_dir, names) = legacy_library_list(dir, project);
    let files = legacy_library_files(dir, &lib_dir, &names);
    // `getLibsNotInGlobalSymbolLibTable` (`FindRowByURI`), then `createProjectSymbolLibTable`
    let rows = |dir: &Path| crate::library_tables::load(dir, TableKind::Symbol).libs();
    let known = rows(dir);
    let mut taken: Vec<String> = known.iter().map(|l| l.nickname.clone()).collect();
    let mut added = false;
    for (name, file) in &files {
        if let Ok(path) = file {
            if known.iter().any(|l| l.path == *path) {
                continue;
            }
        }
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name).replace(' ', "-");
        let mut nickname = base.clone();
        let mut inc = 1;
        while taken.contains(&nickname) {
            nickname = format!("{base}{inc}");
            inc += 1;
        }
        match file {
            Ok(path) => {
                let uri = crate::library_tables::normalized_uri(dir, path);
                report.push(line("info", format!("Adding library '{nickname}', file '{uri}' to project symbol library table.")));
                crate::library_tables::set_project_row(dir, TableKind::Symbol, &nickname, &uri, "Legacy").map_err(|e| format!("Error saving library table:\n\n{e}"))?;
                taken.push(nickname);
                added = true;
            }
            Err(path) => report.push(line("warning", format!("Library '{}' not found.", path.display()))),
        }
    }
    if added {
        report.push(line("info", "Created project symbol library table.\n".to_string()));
    }
    // `remapSymbolsToLibTable`
    let tables = rows(dir);
    let contents: Vec<(PathBuf, Vec<String>)> = files.iter().filter_map(|(_, f)| f.as_ref().ok()).map(|p| (p.clone(), legacy_symbol_names(p))).collect();
    let mut changes: BTreeMap<String, String> = BTreeMap::new();
    for sch in screens(design) {
        let ids = sch.symbols.iter().map(|s| s.lib_id.as_str()).chain(sch.power_symbols.iter().map(|p| p.lib_id.as_str()));
        for id in ids {
            if id.contains(':') || id.is_empty() {
                continue;
            }
            let nickname = contents.iter().filter(|(_, names)| names.iter().any(|n| n == id)).find_map(|(path, _)| tables.iter().find(|l| l.path == *path).map(|l| l.nickname.clone()));
            match nickname {
                Some(nick) => {
                    report.push(line("action", format!("Symbol {id} mapped to symbol library '{nick}'.")));
                    changes.insert(id.to_string(), format!("{nick}:{id}"));
                }
                None => report.push(line("warning", format!("No symbol {id} found in symbol library table."))),
            }
        }
    }
    report.push(line("info", "Symbol library table mapping complete!".to_string()));
    Ok((report, changes.into_iter().collect()))
}

/// `POST /api/sch/remap`: [`remap_plan`] for the board in `dir`, `{ok, report: [{severity, text}], cmd}` (no `cmd` when nothing was mapped) or `{error}`.
/// Refused unless no symbol of the schematic names a library (`remapSymbolsCondition`: `HasNoFullyDefinedLibIds`).
pub(crate) fn remap(dir: &Path) -> Value {
    let (meta, design, _) = match board::load(dir) {
        Ok(x) => x,
        Err(e) => return json!({ "error": board::reasons(&e) }),
    };
    if !has_no_fully_defined_lib_ids(&design) {
        return json!({ "error": "The remapping can only be performed on legacy projects." });
    }
    let stamp = chrono_stamp();
    match remap_plan(dir, &design, &project_name(&meta), &stamp) {
        Ok((report, changes)) => {
            let mut v = json!({ "ok": true, "report": report.iter().map(|l| json!({ "severity": l.severity, "text": l.text })).collect::<Vec<_>>() });
            if !changes.is_empty() {
                v["cmd"] = json!({ "op": "rescue_symbols", "changes": changes, "symbols": [] });
            }
            v
        }
        Err(e) => json!({ "error": e }),
    }
}

/// `wxDateTime::Now().Format( "-%Y-%m-%d-%H-%M-%S" )`, in UTC (the server has no local time zone of the person's).
fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // the civil date of a day count (Howard Hinnant's `civil_from_days`)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("-{year:04}-{month:02}-{day:02}-{:02}-{:02}-{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::LibrarySymbolPin;

    fn pin(number: &str, name: &str, x: f64, y: f64, angle: f64) -> LibrarySymbolPin {
        let mut p: LibrarySymbolPin = serde_json::from_value(json!({ "number": number, "name": name, "electrical_type": "passive", "shape": "line", "at": { "x": x, "y": y }, "angle_deg": angle, "length_mm": 2.54, "unit": 1, "body_style": 1 })).unwrap();
        p.at.x = x;
        p.at.y = y;
        p
    }

    fn sym(id: &str, pins: Vec<LibrarySymbolPin>) -> LibrarySymbol {
        LibrarySymbol { lib_id: id.into(), unit_count: 1, pins, ..Default::default() }
    }

    fn placed(id: &str, r: &str) -> Placed {
        Placed { lib_id: id.into(), unit: 1, body_style: 1, reference: r.into(), value: "v".into() }
    }

    #[test]
    fn pins_conflict_by_number_name_type_and_orientation_at_a_place_not_by_length() {
        let a = sym("L:R", vec![pin("1", "~", 0.0, 3.81, 270.0), pin("2", "~", 0.0, -3.81, 90.0)]);
        let mut b = a.clone();
        assert!(!pins_conflict_with(&a, &b, true, true, true, true, false));
        b.pins[0].length_mm = 5.08;
        assert!(!pins_conflict_with(&a, &b, true, true, true, true, false), "the length is not tested");
        b.pins[0].number = "3".into();
        assert!(pins_conflict_with(&a, &b, true, true, true, true, false));
        let mut c = a.clone();
        c.pins[1].at.y = -5.08;
        assert!(pins_conflict_with(&a, &c, true, true, true, true, false), "a pin moved");
        let mut d = a.clone();
        d.pins[0].electrical_type = "input".into();
        assert!(pins_conflict_with(&a, &d, true, true, true, true, false));
        // one way only: every pin of the first must be in the second
        let mut e = a.clone();
        e.pins.push(pin("3", "X", 5.0, 0.0, 180.0));
        assert!(!pins_conflict_with(&a, &e, true, true, true, true, false));
        assert!(pins_conflict_with(&e, &a, true, true, true, true, false));
    }

    #[test]
    fn a_symbol_only_in_the_cache_or_changed_in_the_library_is_a_candidate_one_that_agrees_or_has_no_cache_is_not() {
        let r = sym("Device:R", vec![pin("1", "~", 0.0, 3.81, 270.0), pin("2", "~", 0.0, -3.81, 90.0)]);
        let mut r_moved = r.clone();
        r_moved.pins[0].at.y = 5.08;
        let gone = sym("Old:Q", vec![pin("1", "B", -5.08, 0.0, 0.0)]);
        let cache = |id: &str| match id {
            "Device:R" | "Device:C" => Some(r.clone()),
            "Old:Q" => Some(gone.clone()),
            _ => None,
        };
        let lib = |id: &str| match id {
            "Device:R" => Some(r.clone()),
            "Device:C" => Some(r_moved.clone()),
            "Device:L" => Some(r.clone()),
            _ => None,
        };
        let placed = vec![placed("Old:Q", "Q1"), placed("Device:R", "R1"), placed("Device:C", "C1"), placed("Device:C", "C2"), placed("Device:L", "L1"), placed("Gone:X", "X1")];
        let found = find_rescues(&placed, "board-rescue", &cache, &lib);
        let got: Vec<(&str, &str, String)> = found.iter().map(|c| (c.requested.as_str(), c.new_id.as_str(), c.action())).collect();
        assert_eq!(
            got,
            vec![
                ("Device:C", "board-rescue:C-Device", "Rescue modified symbol Device:C to board-rescue:C-Device".to_string()),
                ("Old:Q", "board-rescue:Q-Old", "Rescue symbol Old:Q found only in cache library to board-rescue:Q-Old.".to_string()),
            ],
            "R agrees with its library, L has no cached copy, X is neither in a library nor in the cache"
        );
        let s = rescued_symbol(&found[0]).unwrap();
        assert_eq!(s.lib_id, "board-rescue:C-Device");
        assert_eq!(s.pins, r.pins, "the cached symbol is what is rescued");
    }

    #[test]
    fn a_name_with_a_character_a_library_id_cannot_hold_is_always_rescued_under_an_escaped_name() {
        assert_eq!(has_illegal_chars("R<1>"), Some(1));
        assert_eq!(has_illegal_chars("R 1"), None, "a space is allowed");
        assert_eq!(escape_lib_id("R<1>"), "R{lt}1{gt}");
        let r = sym("L:R<1>", vec![pin("1", "~", 0.0, 0.0, 0.0)]);
        let found = find_rescues(&[placed("L:R<1>", "R1")], "p-rescue", &|_| None, &|_| Some(r.clone()));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].new_id, "p-rescue:R{lt}1{gt}-L");
        assert_eq!(found[0].action(), "Rescue modified symbol L:R<1> to p-rescue:R<1>-L");
    }

    #[test]
    fn a_schematic_whose_symbols_name_no_library_is_a_legacy_one() {
        let design = |ids: &[&str]| -> Design {
            let symbols: Vec<Value> = ids.iter().enumerate().map(|(i, id)| json!({ "id": format!("R{i}"), "lib_id": id, "at": { "x": 0, "y": 0 }, "rot": 0 })).collect();
            serde_json::from_value(json!({ "schema": eda_model::ir::SCHEMA_VERSION, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0, "stage_hashes": [] }, "schematic": { "symbols": symbols, "wires": [], "labels": [], "power_symbols": [], "no_connects": [] } })).unwrap()
        };
        assert!(!has_no_fully_defined_lib_ids(&design(&[])), "no symbols: not legacy");
        assert!(has_no_fully_defined_lib_ids(&design(&["R", "C"])));
        assert!(!has_no_fully_defined_lib_ids(&design(&["R", "Device:R"])), "one symbol with a library nickname is enough");
    }

    #[test]
    fn the_rescue_library_is_written_beside_the_board_and_put_in_its_table_once() {
        let dir = tempfile_dir();
        let s = sym("board-rescue:R-Device", vec![pin("1", "~", 0.0, 3.81, 270.0)]);
        let path = write_rescue_library(&dir, "board-rescue", std::slice::from_ref(&s)).unwrap();
        assert_eq!(path, dir.join("board-rescue.kicad_sym"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("(symbol \"R-Device\""), "{text}");
        let t = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
        assert!(t.contains("(name \"board-rescue\")") && t.contains("${KIPRJMOD}/board-rescue.kicad_sym") && t.contains("(type \"KiCad\")"), "{t}");
        // a second rescue keeps what the library holds and the one row
        let s2 = sym("board-rescue:C-Device", vec![pin("1", "~", 0.0, 3.81, 270.0)]);
        write_rescue_library(&dir, "board-rescue", &[s2]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("(symbol \"R-Device\"") && text.contains("(symbol \"C-Device\""), "{text}");
        let t = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
        assert_eq!(t.matches("board-rescue\"").count(), 1, "{t}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A library of one resistor, `R`, its two pins at (0, 3.81) and (0, -3.81).
    const LIB: &str = r#"(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "R" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Footprint" "" (at -1.778 0 90) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Datasheet" "~" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Description" "Resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
    )
  )
)
"#;

    /// A board whose schematic has U1 (`MyParts:R`, a library of the board's table whose R has since moved its pin 2) and U2 (`Gone:R`, a library no
    /// table names), each with the copy the schematic kept of it.
    fn rescue_board(dir: &Path) {
        use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
        let part = |r: &str, symbol: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("10k".into()), package: None, footprint: None, symbol: Some(symbol.into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part("U1", "MyParts:R"), part("U2", "Gone:R")], nets: vec![Net { name: "N1".into(), pins: vec!["U1.1".into(), "U2.1".into()] }], ..Default::default() };
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        // the library as it is now: pin 2 at (0, -5.08)
        std::fs::create_dir_all(dir.join("libs")).unwrap();
        std::fs::write(dir.join("libs").join("MyParts.kicad_sym"), LIB.replace("(at 0 -3.81 90)", "(at 0 -5.08 90)")).unwrap();
        let added = crate::library_tables::add_library(dir, br#"{"kind":"symbol","path":"${KIPRJMOD}/libs/MyParts.kicad_sym","descr":""}"#);
        assert_eq!(added["ok"], true, "{added}");
        let kept = eda_kicad::parse_library_symbols(LIB).unwrap().symbols.remove(0);
        let copy = |id: &str| LibrarySymbol { lib_id: id.into(), published: true, ..kept.clone() };
        let sym = |id: &str, lib_id: &str, x: f64| json!({ "id": id, "lib_id": lib_id, "at": { "x": (x * 1e6) as i64, "y": 20_000_000 }, "rot": 0, "unit": 1 });
        let design: Design = serde_json::from_value(json!({
            "schema": eda_model::ir::SCHEMA_VERSION,
            "provenance": { "engine_version": "t", "intent_hash": "x", "seed": 0, "stage_hashes": [] },
            "schematic": { "symbols": [sym("U1", "MyParts:R", 20.32), sym("U2", "Gone:R", 40.64)], "wires": [], "labels": [], "power_symbols": [], "no_connects": [], "imported_from_kicad": true },
            "placement": { "outline": [], "footprints": [] },
            "symbol_library": { "symbols": [copy("Gone:R"), copy("MyParts:R")] },
        }))
        .unwrap();
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent.display().to_string(), snap_nm: eda_model::units::mm(0.1), spacing_nm: eda_model::units::mm(0.3) };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    /// The whole of Rescue Symbols on a board: the two candidates and their words, the rescue library and its table row, the command that relinks both
    /// symbols as one undo step, and nothing left to rescue after. With `EDA_SLOW_TESTS=1`, kicad-cli's ERC of the derived project judges it: U2's
    /// missing library (`lib_symbol_issues`) is found before, and after the rescue kicad-cli reads both symbols from the rescue library.
    #[test]
    fn rescue_symbols_writes_the_rescue_library_and_relinks_the_symbols_in_one_undo_step() {
        let dir = tempfile_dir();
        rescue_board(&dir);
        let found = candidates(&dir);
        assert_eq!(found["legacy"], false, "{found}");
        assert_eq!(found["library"], "intent-rescue");
        let rows: Vec<(String, String)> = found["candidates"].as_array().unwrap().iter().map(|c| (c["requested"].as_str().unwrap().to_string(), c["action"].as_str().unwrap().to_string())).collect();
        assert_eq!(
            rows,
            vec![
                ("Gone:R".to_string(), "Rescue symbol Gone:R found only in cache library to intent-rescue:R-Gone.".to_string()),
                ("MyParts:R".to_string(), "Rescue modified symbol MyParts:R to intent-rescue:R-MyParts".to_string()),
            ]
        );
        assert_eq!(found["candidates"][1]["instances"], json!([{ "reference": "U1", "value": "10k" }]));
        assert!(found["candidates"][1]["lib"].is_object() && found["candidates"][0]["lib"].is_null(), "{found}");

        let slow = std::env::var_os("EDA_SLOW_TESTS").is_some() && eda_kicad_engine::find_cli().is_some();
        let library_findings = |dir: &Path| -> Vec<String> {
            let erc = crate::kicad_engine::erc(dir).expect("kicad-cli runs ERC");
            erc["violations"].as_array().unwrap().iter().map(|v| v.to_string()).filter(|v| v.contains("lib_symbol_issues") || v.contains("lib_symbol_mismatch")).collect()
        };
        if slow {
            board::step(&dir, eda_ops::Cmd::SetErcSeverities { severities: [("lib_symbol_issues".to_string(), "warning".to_string()), ("lib_symbol_mismatch".to_string(), "warning".to_string())].into() }, false, "test").unwrap();
            let before = library_findings(&dir);
            assert!(before.iter().any(|f| f.contains("lib_symbol_issues")) && before.iter().any(|f| f.contains("lib_symbol_mismatch")), "kicad-cli finds the missing library and the changed symbol: {before:?}");
        }

        let done = rescue(&dir, br#"{"requested":["Gone:R","MyParts:R"]}"#);
        assert_eq!(done["ok"], true, "{done}");
        let lib = std::fs::read_to_string(dir.join("intent-rescue.kicad_sym")).unwrap();
        assert!(lib.contains("(symbol \"R-Gone\"") && lib.contains("(symbol \"R-MyParts\""), "{lib}");
        assert!(lib.contains("(at 0 -3.81 90)") && !lib.contains("(at 0 -5.08 90)"), "the cached symbols are what is rescued: {lib}");
        let table = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
        assert!(table.contains("${KIPRJMOD}/intent-rescue.kicad_sym"), "{table}");
        let cmd: eda_ops::Cmd = serde_json::from_value(done["cmd"].clone()).unwrap();
        board::step(&dir, cmd, false, "test").unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        let ids: Vec<(String, String)> = design.schematic.as_ref().unwrap().symbols.iter().map(|s| (s.id.clone(), s.lib_id.clone())).collect();
        assert_eq!(ids, vec![("U1".to_string(), "intent-rescue:R-MyParts".to_string()), ("U2".to_string(), "intent-rescue:R-Gone".to_string())]);
        assert!(design.symbol_library.as_ref().unwrap().by_lib_id("intent-rescue:R-MyParts").is_some(), "the schematic keeps the rescued copy");
        assert_eq!(candidates(&dir)["candidates"], json!([]), "nothing is left to rescue");
        if slow {
            // (`lib_symbol_mismatch` is not judged: the derived schematic's cache is the writer's, its symbols drawn from the corner of their box, which no
            // library symbol is byte for byte -- the derived project switches that check off for every symbol)
            let after: Vec<String> = library_findings(&dir).into_iter().filter(|f| f.contains("lib_symbol_issues")).collect();
            assert!(after.is_empty(), "kicad-cli finds the rescued symbols in the rescue library of the project's table: {after:?}");
        }
        // one undo step back to the old links (the rescue library stays, as KiCad leaves it)
        board::undo(&dir, "test", Some(eda_ops::Domain::Schematic)).unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        assert_eq!(design.schematic.as_ref().unwrap().symbols.iter().map(|s| s.lib_id.as_str()).collect::<Vec<_>>(), vec!["MyParts:R", "Gone:R"]);
        assert!(dir.join("intent-rescue.kicad_sym").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn qa_legacy_power4() -> Option<PathBuf> {
        let root = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("src/kicad-src-8303b2ad"));
        Some(root.join("qa/data/eeschema/netlists/legacy_power4")).filter(|p| p.is_dir())
    }

    #[test]
    fn the_legacy_library_list_is_read_from_a_kicad5_project_file_and_its_libraries_found() {
        let dir = tempfile_dir();
        std::fs::write(dir.join("intent.pro"), "update=x\n[eeschema]\nversion=1\nLibDir=libs;/nowhere\n[eeschema/libraries]\nLibName2=other\nLibName1=symbols\nLibName3=missing\n").unwrap();
        let (lib_dir, names) = legacy_library_list(&dir, "intent");
        assert_eq!((lib_dir.as_str(), names.clone()), ("libs;/nowhere", vec!["symbols".to_string(), "other".to_string(), "missing".to_string()]), "LibName<n> in number order");
        std::fs::write(dir.join("symbols.lib"), "EESchema-LIBRARY Version 2.4\n#End Library\n").unwrap();
        std::fs::create_dir_all(dir.join("libs")).unwrap();
        std::fs::write(dir.join("libs").join("other.lib"), "EESchema-LIBRARY Version 2.4\n#End Library\n").unwrap();
        let files = legacy_library_files(&dir, &lib_dir, &names);
        assert_eq!(files[0].1, Ok(dir.join("symbols.lib")));
        assert_eq!(files[1].1, Ok(dir.join("libs").join("other.lib")), "found in LibDir");
        assert_eq!(files[2].1, Err(dir.join("missing.lib")));
        // a KiCad 6+ project file's list wins
        std::fs::write(dir.join("intent.kicad_pro"), r#"{ "schematic": { "legacy_lib_dir": "", "legacy_lib_list": ["symbols"] } }"#).unwrap();
        assert_eq!(legacy_library_list(&dir, "intent").1, vec!["symbols".to_string()]);
        assert!(chrono_stamp().starts_with("-20") && chrono_stamp().len() == "-2026-11-06-08-00-00".len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Remap Legacy Library Symbols on KiCad's QA `legacy_power4` (a KiCad 5 project: `legacy_power4.pro` lists `symbols`, whose `symbols.lib` has `+3V3`,
    /// `GND` and `R`, `+3.3V` an alias of `+3V3`), its symbols' bare names on a board: the backup, the project's table made with a Legacy row for `symbols`,
    /// each symbol named `symbols:<name>`, and kicad-cli reads the remapped board's libraries. Slow tier: the `.lib` is read through kicad-cli's conversion.
    #[test]
    fn remap_gives_the_legacy_symbols_of_kicads_qa_legacy_power4_their_library() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 (and have kicad-cli) to run it");
            return;
        }
        let Some(qa) = qa_legacy_power4() else {
            eprintln!("no KiCad QA data; skipping");
            return;
        };
        let dir = tempfile_dir();
        rescue_board(&dir);
        std::fs::copy(qa.join("legacy_power4.pro"), dir.join("intent.pro")).unwrap();
        std::fs::copy(qa.join("symbols.lib"), dir.join("symbols.lib")).unwrap();
        std::fs::copy(qa.join("legacy_power4-cache.lib"), dir.join("intent-cache.lib")).unwrap();
        // the legacy schematic's names: R1 and R2 are `R`, the power symbols `GND` and `+3.3V`
        let (_, mut design, _) = board::load(&dir).unwrap();
        let sch = design.schematic.as_mut().unwrap();
        for s in sch.symbols.iter_mut() {
            s.lib_id = "R".into();
        }
        let power = |id: &str, lib: &str, net: &str, x: i64| -> eda_model::ir::PowerSymbol { serde_json::from_value(json!({ "id": id, "lib_id": lib, "at": { "x": x, "y": 10_000_000 }, "rot": 0, "net": net, "pin": "" })).unwrap() };
        sch.power_symbols = vec![power("#PWR01", "+3.3V", "+3.3V", 20_320_000), power("#PWR03", "GND", "GND", 40_640_000)];
        // the copies a legacy schematic's symbols draw from (its cache library's), under their bare names
        let lib = design.symbol_library.get_or_insert_with(Default::default);
        for (bare, builtin) in [("+3.3V", "power:+3V3"), ("GND", "power:GND"), ("R", "Device:R")] {
            let mut copy = LibrarySymbol::from_engine_symbol(&eda_model::symbol::builtin(builtin).unwrap());
            copy.lib_id = bare.into();
            copy.published = true;
            lib.symbols.push(copy);
        }
        board::save(&dir, &design).unwrap();
        let before = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
        assert!(before.contains("MyParts"));

        let r = remap(&dir);
        assert_eq!(r["ok"], true, "{r}");
        let texts: Vec<String> = r["report"].as_array().unwrap().iter().map(|l| format!("{}: {}", l["severity"].as_str().unwrap(), l["text"].as_str().unwrap())).collect();
        let has = |t: &str| texts.iter().any(|x| x.contains(t));
        assert!(has("Backing up file '") && has("rescue-backup"), "{texts:#?}");
        assert!(has("info: Adding library 'symbols', file '${KIPRJMOD}/symbols.lib' to project symbol library table."), "{texts:#?}");
        assert!(has("info: Created project symbol library table."), "{texts:#?}");
        assert!(has("action: Symbol R mapped to symbol library 'symbols'.") && has("action: Symbol GND mapped to symbol library 'symbols'."), "{texts:#?}");
        assert!(has("action: Symbol +3.3V mapped to symbol library 'symbols'."), "an alias of the library's `+3V3` (`ALIAS +3.3V`) is a symbol of it: {texts:#?}");
        assert!(!has("warning"), "{texts:#?}");
        assert_eq!(texts.last().unwrap(), "info: Symbol library table mapping complete!");
        let table = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
        assert!(!table.contains("MyParts") && table.contains("symbols") && table.contains("Legacy"), "the table is made again from the legacy libraries: {table}");
        assert!(std::fs::read_dir(dir.join("rescue-backup")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("sym-lib-table-20")), "the old table is backed up");
        let cmd: eda_ops::Cmd = serde_json::from_value(r["cmd"].clone()).unwrap();
        board::step(&dir, cmd, false, "test").unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.symbols.iter().all(|s| s.lib_id == "symbols:R"), "{:?}", sch.symbols.iter().map(|s| &s.lib_id).collect::<Vec<_>>());
        assert_eq!(sch.power_symbols.iter().map(|p| p.lib_id.as_str()).collect::<Vec<_>>(), vec!["symbols:+3.3V", "symbols:GND"]);
        assert_eq!(remap(&dir)["error"], "The remapping can only be performed on legacy projects.", "a schematic with a library link is not legacy any more");
        // kicad-cli finds `symbols:R` and `symbols:GND` in the Legacy row of the derived project's table
        board::step(&dir, eda_ops::Cmd::SetErcSeverities { severities: [("lib_symbol_issues".to_string(), "warning".to_string())].into() }, false, "test").unwrap();
        let erc = crate::kicad_engine::erc(&dir).expect("kicad-cli runs ERC");
        let issues: Vec<String> = erc["violations"].as_array().unwrap().iter().map(|v| v.to_string()).filter(|v| v.contains("lib_symbol_issues")).collect();
        assert!(!issues.iter().any(|f| f.contains("symbols")), "{issues:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let d = std::env::temp_dir().join(format!("eda-rescue-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
