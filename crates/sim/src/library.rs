//! A SPICE model library as the Simulation Model dialog reads it (`DIALOG_SIM_MODEL::loadLibrary` -> `SIM_LIB_MGR::SetLibrary` ->
//! `SIM_LIBRARY_SPICE::ReadFile`): the models it defines at its top level, `.subckt`s with their port names and `.model`s with the KiCad model
//! type their SPICE type gives them, and the `.include`d files read the same way.
//!
//! Ported from `eeschema/sim/spice_library_parser.cpp` (`SPICE_LIBRARY_PARSER::parseFile`, `ReadFile`), the head-line rules of
//! `eeschema/sim/spice_grammar.h` (`dotSubckt`, `dotSubcktPinSequence`, `dotModel`, `dotModelAko`, `dotInclude`, comments and `+` continuations),
//! `sim_model_subckt.cpp` (`SPICE_MODEL_PARSER_SUBCKT::ReadModel`: the ports), `spice_model_parser.cpp` (`ReadType`,
//! `ReadTypeFromSpiceStrings`) and the type tables of `sim_model.cpp` (`DeviceInfo`, `TypeInfo`, `SpiceInfo`) with the pin names of
//! `sim_model_ngspice_data_*.cpp`, `sim_model_switch.h` and `sim_model_tline.h`. `SIM_LIB_MGR::ResolveLibraryPath` and
//! `ResolveEmbeddedLibraryPath` find the files.
//!
//! Where this differs from KiCad, which parses with a PEG grammar (PEGTL): the file is cut into logical lines first (a `+` line and a line
//! ending in `\\` continue the one before, `*` lines are comments, `;`, and `$` or `//` after a space, start an end-of-line comment), and each
//! line is read by its first word. That reads the same models from the libraries vendors ship; a line the grammar would refuse with a parse error
//! is skipped here instead. An `.include` that includes itself is read once (KiCad recurses until the stack runs out). IBIS files (`.ibs`) are not
//! read (`SIM_LIBRARY_IBIS`, GAPS 34).

use std::path::{Path, PathBuf};

/// One model of a library, in the order of the file (`SIM_LIBRARY::GetModels`).
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryModel {
    /// `modelName`, as written.
    pub name: String,
    pub kind: ModelKind,
    /// The model's SPICE code as the file has it (`SIM_MODEL_SUBCKT::GetSpiceCode`, the dialog's Subcircuit box): its lines from the head line
    /// to `.ends` for a subcircuit, the `.model` line with its continuations otherwise.
    pub code: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelKind {
    /// `.subckt name port ... [params: ...]`: `SIM_MODEL::TYPE::SUBCKT`, `Sim.Device` "SUBCKT", the ports in order (`model.AddPin( { name, n } )`).
    Subckt { ports: Vec<String> },
    /// `.model name type (...)`: the KiCad model type its SPICE type, `level` and `version` give (`ReadTypeFromSpiceStrings`), or none when KiCad
    /// does not know it (`TYPE::RAWSPICE`, "a raw Spice element").
    Model { spice_type: String, model_type: Option<&'static ModelType> },
}

/// One row of KiCad's model type table, for the types a `.model` line can name.
#[derive(Debug, PartialEq)]
pub struct ModelType {
    /// `SIM_MODEL::TYPE`'s name.
    pub name: &'static str,
    /// `SpiceInfo( type ).modelType`, `level`, `isDefaultLevel`, `version`.
    pub spice_model_type: &'static str,
    pub level: &'static str,
    pub default_level: bool,
    pub version: &'static str,
    /// `DeviceInfo( TypeInfo( type ).deviceType ).fieldValue` (`Sim.Device`) and `TypeInfo( type ).fieldValue` (`Sim.Type`, none when empty).
    pub device: &'static str,
    pub type_field: &'static str,
    /// `GetPinNames()`: the model's pins in order; a name in angle brackets is optional (`createPins` leaves it out when the symbol has too few pins).
    pub pins: &'static [&'static str],
}

const BJT: &[&str] = &["C", "B", "E", "<S>"];
const VBIC: &[&str] = &["C", "B", "E", "<S>", "<TJ>"];
const HICUM2: &[&str] = &["C", "B", "E", "S", "TJ"];
const DGS: &[&str] = &["D", "G", "S"];
const DGSB: &[&str] = &["D", "G", "S", "B"];
const VDMOS: &[&str] = &["D", "G", "S", "<TJ>", "<TCASE>"];
const SWITCH: &[&str] = &["ctrl+", "ctrl-", "no+", "no-"];
const TLINE: &[&str] = &["1+", "1-", "2+", "2-"];

macro_rules! mt {
    ($name:literal, $st:literal, $lvl:literal, $def:literal, $ver:literal, $dev:literal, $ty:literal, $pins:expr) => {
        ModelType { name: $name, spice_model_type: $st, level: $lvl, default_level: $def, version: $ver, device: $dev, type_field: $ty, pins: $pins }
    };
}

/// `SIM_MODEL::TYPE_ITERATOR()` order, the types whose `SpiceInfo( type ).modelType` is not empty (the others are skipped by
/// `ReadTypeFromSpiceStrings`).
pub static MODEL_TYPES: &[ModelType] = &[
    mt!("TLINE_Z0", "LTRA", "", false, "", "TLINE", "", TLINE),
    mt!("TLINE_RLGC", "LTRA", "", false, "", "TLINE", "RLGC", TLINE),
    mt!("SW_V", "SW", "", false, "", "SW", "V", SWITCH),
    mt!("SW_I", "CSW", "", false, "", "SW", "I", SWITCH),
    mt!("D", "D", "", false, "", "D", "", &["A", "K"]),
    mt!("NPN_VBIC", "NPN", "4", false, "", "NPN", "VBIC", VBIC),
    mt!("PNP_VBIC", "PNP", "4", false, "", "PNP", "VBIC", VBIC),
    mt!("NPN_GUMMELPOON", "NPN", "1", true, "", "NPN", "GUMMELPOON", BJT),
    mt!("PNP_GUMMELPOON", "PNP", "1", true, "", "PNP", "GUMMELPOON", BJT),
    mt!("NPN_HICUM2", "NPN", "8", false, "", "NPN", "HICUML2", HICUM2),
    mt!("PNP_HICUM2", "PNP", "8", false, "", "PNP", "HICUML2", HICUM2),
    mt!("NJFET_SHICHMANHODGES", "NJF", "1", true, "", "NJFET", "SHICHMANHODGES", DGS),
    mt!("PJFET_SHICHMANHODGES", "PJF", "1", true, "", "PJFET", "SHICHMANHODGES", DGS),
    mt!("NJFET_PARKERSKELLERN", "NJF", "2", false, "", "NJFET", "PARKERSKELLERN", DGS),
    mt!("PJFET_PARKERSKELLERN", "PJF", "2", false, "", "PJFET", "PARKERSKELLERN", DGS),
    mt!("NMES_STATZ", "NMF", "1", true, "", "NMES", "STATZ", DGS),
    mt!("PMES_STATZ", "PMF", "1", true, "", "PMES", "STATZ", DGS),
    mt!("NMES_YTTERDAL", "NMF", "2", false, "", "NMES", "YTTERDAL", DGS),
    mt!("PMES_YTTERDAL", "PMF", "2", false, "", "PMES", "YTTERDAL", DGS),
    mt!("NMES_HFET1", "NMF", "5", false, "", "NMES", "HFET1", DGS),
    mt!("PMES_HFET1", "PMF", "5", false, "", "PMES", "HFET1", DGS),
    mt!("NMES_HFET2", "NMF", "6", false, "", "NMES", "HFET2", DGS),
    mt!("PMES_HFET2", "PMF", "6", false, "", "PMES", "HFET2", DGS),
    mt!("NMOS_VDMOS", "VDMOS NCHAN", "", false, "", "NMOS", "VDMOS", VDMOS),
    mt!("PMOS_VDMOS", "VDMOS PCHAN", "", false, "", "PMOS", "VDMOS", VDMOS),
    mt!("NMOS_MOS1", "NMOS", "1", true, "", "NMOS", "MOS1", DGSB),
    mt!("PMOS_MOS1", "PMOS", "1", true, "", "PMOS", "MOS1", DGSB),
    mt!("NMOS_MOS2", "NMOS", "2", false, "", "NMOS", "MOS2", DGSB),
    mt!("PMOS_MOS2", "PMOS", "2", false, "", "PMOS", "MOS2", DGSB),
    mt!("NMOS_MOS3", "NMOS", "3", false, "", "NMOS", "MOS3", DGSB),
    mt!("PMOS_MOS3", "PMOS", "3", false, "", "PMOS", "MOS3", DGSB),
    mt!("NMOS_BSIM1", "NMOS", "4", false, "", "NMOS", "BSIM1", DGSB),
    mt!("PMOS_BSIM1", "PMOS", "4", false, "", "PMOS", "BSIM1", DGSB),
    mt!("NMOS_BSIM2", "NMOS", "5", false, "", "NMOS", "BSIM2", DGSB),
    mt!("PMOS_BSIM2", "PMOS", "5", false, "", "PMOS", "BSIM2", DGSB),
    mt!("NMOS_MOS6", "NMOS", "6", false, "", "NMOS", "MOS6", DGSB),
    mt!("PMOS_MOS6", "PMOS", "6", false, "", "PMOS", "MOS6", DGSB),
    mt!("NMOS_MOS9", "NMOS", "9", false, "", "NMOS", "MOS9", DGSB),
    mt!("PMOS_MOS9", "PMOS", "9", false, "", "PMOS", "MOS9", DGSB),
    mt!("NMOS_BSIM3", "NMOS", "8", false, "", "NMOS", "BSIM3", DGSB),
    mt!("PMOS_BSIM3", "PMOS", "8", false, "", "PMOS", "BSIM3", DGSB),
    mt!("NMOS_B4SOI", "NMOS", "10", false, "", "NMOS", "B4SOI", DGSB),
    mt!("PMOS_B4SOI", "PMOS", "10", false, "", "PMOS", "B4SOI", DGSB),
    mt!("NMOS_BSIM4", "NMOS", "14", false, "", "NMOS", "BSIM4", DGSB),
    mt!("PMOS_BSIM4", "PMOS", "14", false, "", "PMOS", "BSIM4", DGSB),
    mt!("NMOS_B3SOIFD", "NMOS", "55", false, "", "NMOS", "B3SOIFD", DGSB),
    mt!("PMOS_B3SOIFD", "PMOS", "55", false, "", "PMOS", "B3SOIFD", DGSB),
    mt!("NMOS_B3SOIDD", "NMOS", "56", false, "", "NMOS", "B3SOIDD", DGSB),
    mt!("PMOS_B3SOIDD", "PMOS", "56", false, "", "PMOS", "B3SOIDD", DGSB),
    mt!("NMOS_B3SOIPD", "NMOS", "57", false, "", "NMOS", "B3SOIPD", DGSB),
    mt!("PMOS_B3SOIPD", "PMOS", "57", false, "", "PMOS", "B3SOIPD", DGSB),
    mt!("NMOS_HISIM2", "NMOS", "68", false, "", "NMOS", "HISIM2", DGSB),
    mt!("PMOS_HISIM2", "PMOS", "68", false, "", "PMOS", "HISIM2", DGSB),
    mt!("NMOS_HISIMHV1", "NMOS", "73", false, "1.2.4", "NMOS", "HISIMHV1", DGSB),
    mt!("PMOS_HISIMHV1", "PMOS", "73", false, "1.2.4", "PMOS", "HISIMHV1", DGSB),
    mt!("NMOS_HISIMHV2", "NMOS", "73", false, "2.2.0", "NMOS", "HISIMHV2", DGSB),
    mt!("PMOS_HISIMHV2", "PMOS", "73", false, "2.2.0", "PMOS", "HISIMHV2", DGSB),
];

/// `SPICE_MODEL_PARSER::ReadTypeFromSpiceStrings`: the KiCad model type of a `.model` line's SPICE type, `level` and `version`; `None` is
/// `TYPE::RAWSPICE` ("If the type string is not recognized, demote to a raw Spice element").
pub fn read_type_from_spice_strings(type_string: &str, level: &str, version: &str, skip_default_level: bool) -> Option<&'static ModelType> {
    let input_level = level.split('.').next().unwrap_or("");
    let input_type = type_string.to_uppercase();
    let vdmos = input_type.starts_with("VDMOS");
    let pchan = vdmos && input_type.contains("PCHAN");
    for candidate in MODEL_TYPES {
        let ct = candidate.spice_model_type;
        if ct.starts_with("VDMOS") && vdmos {
            if (pchan && ct.ends_with("PCHAN")) || (!pchan && ct.ends_with("NCHAN")) {
                return Some(candidate);
            }
        } else if input_type.starts_with(ct) {
            if candidate.version != version {
                continue;
            }
            if candidate.level == input_level {
                return Some(candidate);
            }
            if skip_default_level {
                continue;
            }
            if candidate.default_level && level.is_empty() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The characters `SPICE_GRAMMAR::garbage` skips between tokens.
fn is_garbage(c: char) -> bool {
    matches!(c, ' ' | '\t' | '=' | '(' | ')' | ',')
}

/// One logical line: its text with the continuations joined, and the physical lines it took (for the model's code).
struct Logical {
    text: String,
    raw: String,
}

/// An end-of-line comment's start: `;` anywhere, `$` or `//` at the start of the line or after a blank (`eolfCommentStart`; ngspice wants the
/// blank before `$`).
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, c) in line.char_indices() {
        let after_blank = i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b',');
        if c == ';' || (after_blank && (c == '$' || line[i..].starts_with("//"))) {
            return &line[..i];
        }
    }
    line
}

/// The file as logical lines (`newline`, `plusContinuation`, `backslashContinuation`, `commentLine`).
fn logical_lines(text: &str) -> Vec<Logical> {
    let mut out: Vec<Logical> = Vec::new();
    let mut joining_backslash = false;
    for physical in text.lines() {
        let line = physical.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with('*') {
            // a comment line, which may sit between a line and its `+` continuation
            if let Some(last) = out.last_mut() {
                last.raw.push('\n');
                last.raw.push_str(line);
            }
            continue;
        }
        let body = strip_comment(line);
        let (body, ends_backslash) = match body.trim_end().strip_suffix("\\\\") {
            Some(b) => (b.to_string(), true),
            None => (body.to_string(), false),
        };
        let continues = joining_backslash || trimmed.starts_with('+');
        if continues && !out.is_empty() {
            let last = out.last_mut().expect("checked");
            let rest = if joining_backslash { body.as_str() } else { body.trim_start().strip_prefix('+').unwrap_or("") };
            last.text.push(' ');
            last.text.push_str(rest);
            last.raw.push('\n');
            last.raw.push_str(line);
        } else if !body.trim().is_empty() || ends_backslash {
            out.push(Logical { text: body, raw: line.to_string() });
        }
        joining_backslash = ends_backslash;
    }
    out
}

/// The first token of a line and the rest (`modelName : plus<not_at<garbageOrEolf>, any>`).
fn split_token(s: &str) -> (&str, &str) {
    let s = s.trim_start_matches(is_garbage);
    let end = s.find(is_garbage).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// `dotSubckt`'s ports: blank-separated words up to the parameters (`params:`, or a `name = value` pair, `dotSubcktParams`).
fn subckt_ports(rest: &str) -> Vec<String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut ports = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let is_ident = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if w.eq_ignore_ascii_case("params:") {
            break;
        }
        if let Some((name, _)) = w.split_once('=') {
            if is_ident(name.trim()) {
                break;
            }
        }
        if is_ident(w) && words.get(i + 1).is_some_and(|n| n.starts_with('=')) {
            break;
        }
        ports.push(w.to_string());
    }
    ports
}

/// `level` and `version` of a `.model` line's parameters (`ReadType`: `paramName == "level"`, compared as written).
fn level_and_version(params: &str) -> (String, String) {
    let tokens: Vec<&str> = params.split(is_garbage).filter(|t| !t.is_empty()).collect();
    let (mut level, mut version) = (String::new(), String::new());
    for pair in tokens.windows(2) {
        match pair[0] {
            "level" => level = pair[1].to_string(),
            "version" => version = pair[1].to_string(),
            _ => {}
        }
    }
    (level, version)
}

/// `dotModelType`: the SPICE type, "VDMOS NCHAN" for the VDMOS form.
fn dot_model_type(rest: &str) -> (String, &str) {
    let (t, after) = split_token(rest);
    if t.eq_ignore_ascii_case("vdmos") {
        let (chan, after2) = split_token(after);
        if chan.eq_ignore_ascii_case("nchan") || chan.eq_ignore_ascii_case("pchan") {
            return (format!("{t} {chan}"), after2);
        }
    }
    (t.to_string(), after)
}

/// What [`read_library`] gives: the models, and what went wrong (each one a line of the dialog's info bar).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Library {
    pub models: Vec<LibraryModel>,
    pub messages: Vec<String>,
}

/// `SIM_LIB_MGR::ResolveLibraryPath`: an absolute path that exists, else the path from the project folder, else from `SPICE_LIB_DIR`; not
/// found, "Simulation model library not found at '%s'" (with "or '%s'" when `SPICE_LIB_DIR` gave another place).
pub fn resolve_library_path(path: &str, project: &Path, spice_lib_dir: Option<&Path>) -> Result<PathBuf, String> {
    let expanded = expand_project_var(path, project).replace('\\', "/");
    let p = Path::new(&expanded);
    if p.is_absolute() && p.exists() {
        return Ok(p.to_path_buf());
    }
    let project_fn = if p.is_absolute() { p.to_path_buf() } else { project.join(p) };
    if project_fn.exists() {
        return Ok(project_fn);
    }
    if let Some(dir) = spice_lib_dir.filter(|d| !d.as_os_str().is_empty()) {
        let lib_fn = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        if lib_fn.exists() {
            return Ok(lib_fn);
        }
        if lib_fn != project_fn {
            return Err(format!("Simulation model library not found at '{}' or '{}'", project_fn.display(), lib_fn.display()));
        }
    }
    Err(format!("Simulation model library not found at '{}'", project_fn.display()))
}

/// `ExpandEnvVarSubstitutions` for the one variable a project's library path names: `${KIPRJMOD}`, the project folder.
fn expand_project_var(path: &str, project: &Path) -> String {
    let dir = project.to_string_lossy();
    path.replace("${KIPRJMOD}", &dir).replace("$(KIPRJMOD)", &dir)
}

/// `SIM_LIB_MGR::SetLibrary` and `SIM_LIBRARY_SPICE::ReadFile`: the library at `path` (as the `Sim.Library` field or the dialog's File box
/// says it), read with its includes. The library is refused as a whole when anything was reported (`loadLibrary`:
/// `HasMessageOfSeverity( RPT_SEVERITY_UNDEFINED | RPT_SEVERITY_ERROR )` clears it): then `models` is empty and `messages` says why.
pub fn read_library(path: &str, project: &Path, spice_lib_dir: Option<&Path>) -> Library {
    let mut lib = Library::default();
    let resolved = match resolve_library_path(path, project, spice_lib_dir) {
        Ok(p) => p,
        Err(e) => {
            lib.messages.push(e);
            return lib;
        }
    };
    let mut seen = Vec::new();
    parse_file(&resolved, project, spice_lib_dir, &mut lib, &mut seen);
    if !lib.messages.is_empty() {
        lib.models.clear();
    }
    lib
}

/// `SPICE_LIBRARY_PARSER::parseFile`.
fn parse_file(file: &Path, project: &Path, spice_lib_dir: Option<&Path>, lib: &mut Library, seen: &mut Vec<PathBuf>) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if seen.contains(&canonical) {
        return;
    }
    seen.push(canonical);
    // `SafeReadFile`
    let text = match std::fs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) if !file.is_file() => {
            lib.messages.push(format!("File '{}' does not exist.", file.display()));
            return;
        }
        Err(_) => {
            lib.messages.push(format!("Cannot open file '{}'.", file.display()));
            return;
        }
    };
    let (models, includes) = parse_text(&text);
    lib.models.extend(models);
    for inc in includes {
        // `ResolveEmbeddedLibraryPath( path, aFilePath )`: from the including file's folder first, then as a library path
        let from_here = file.parent().map(|d| d.join(&inc)).filter(|p| Path::new(&inc).is_relative() && p.exists());
        let target = match from_here {
            Some(p) => p,
            None => match resolve_library_path(&inc, project, spice_lib_dir) {
                Ok(p) => p,
                Err(e) => {
                    lib.messages.push(e);
                    // the file is still opened (and reported missing) as `parseFile( lib, .. )` does
                    let missing = if Path::new(&inc).is_absolute() { PathBuf::from(&inc) } else { project.join(&inc) };
                    parse_file(&missing, project, spice_lib_dir, lib, seen);
                    continue;
                }
            },
        };
        parse_file(&target, project, spice_lib_dir, lib, seen);
    }
}

/// The top-level models of one file's text and the paths it includes, in order (`libraryGrammar`'s `modelUnit` and `dotInclude` nodes; a model
/// inside a subcircuit is part of it, `until<dotSubcktEnd, spiceUnit>`).
pub fn parse_text(text: &str) -> (Vec<LibraryModel>, Vec<String>) {
    let lines = logical_lines(text);
    let mut models: Vec<LibraryModel> = Vec::new();
    let mut includes = Vec::new();
    let mut i = 0;
    let mut in_control = false;
    while i < lines.len() {
        let l = &lines[i];
        let t = l.text.trim_start_matches(is_garbage);
        let (word, rest) = split_token(t);
        let w = word.to_lowercase();
        i += 1;
        if in_control {
            in_control = !w.starts_with(".endc");
            continue;
        }
        if w == ".control" {
            in_control = true;
            continue;
        }
        if w == ".subckt" {
            let (name, after) = split_token(rest);
            if name.is_empty() {
                continue;
            }
            let ports = subckt_ports(after);
            let mut code = l.raw.clone();
            let mut depth = 1;
            while i < lines.len() && depth > 0 {
                let inner = lines[i].text.trim_start_matches(is_garbage).to_lowercase();
                code.push('\n');
                code.push_str(&lines[i].raw);
                if inner.starts_with(".subckt") {
                    depth += 1;
                } else if inner.starts_with(".ends") {
                    depth -= 1;
                }
                i += 1;
            }
            models.push(LibraryModel { name: name.to_string(), kind: ModelKind::Subckt { ports }, code });
        } else if w == ".model" {
            let (name, after) = split_token(rest);
            if name.is_empty() {
                continue;
            }
            let (spice_type, params) = dot_model_type(after);
            let model_type = if spice_type.eq_ignore_ascii_case("ako:") {
                // `dotModelAko`: the type of the model it is a kind of (`FindModel`, without case)
                let (parent, _) = split_token(params);
                models.iter().find(|m| m.name.eq_ignore_ascii_case(parent)).and_then(|m| match &m.kind {
                    ModelKind::Model { model_type, .. } => *model_type,
                    ModelKind::Subckt { .. } => None,
                })
            } else {
                // `ReadType`: the type string alone first, then with `level` and `version`
                read_type_from_spice_strings(&spice_type, "", "", true).or_else(|| {
                    let (level, version) = level_and_version(params);
                    read_type_from_spice_strings(&spice_type, &level, &version, false)
                })
            };
            models.push(LibraryModel { name: name.to_string(), kind: ModelKind::Model { spice_type, model_type }, code: l.raw.clone() });
        } else if w.starts_with(".inc") {
            // `dotInclude`: ".inc" and any word after it ("include", "incl"), then the path, quoted or to the end of the line
            let path = rest.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            if !path.is_empty() {
                includes.push(path.to_string());
            }
        }
    }
    (models, includes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPAMP: &str = "* A vendor library\n\
.SUBCKT LM358 1 2 3 4 5\n\
* pins: +in -in V+ V- out\n\
Q1 6 1 7 QX\n\
.MODEL QX PNP(BF=200)\n\
.ENDS LM358\n\
.subckt TL072 IN+ IN- VCC VEE\n\
+ OUT params: GAIN=1e5\n\
R1 IN+ IN- 1MEG\n\
.ends\n\
.model 1N4148 D(IS=2.52n RS=0.568 N=1.752)\n\
.model QNPN NPN (BF=100 level=4) ; a VBIC\n\
.model Q2 NPN\n\
.model M1 NMOS (LEVEL=3)\n\
.model M2 nmos level=8\n\
.model M3 VDMOS pchan (Rd=1)\n\
.model W1 CSW (IT=1)\n\
.model FOO WEIRD (a=1)\n\
.model Q3 ako: QNPN\n";

    fn names(text: &str) -> Vec<String> {
        parse_text(text).0.into_iter().map(|m| m.name).collect()
    }

    #[test]
    fn the_top_level_models_in_the_files_order() {
        // the .MODEL inside LM358 belongs to it
        assert_eq!(names(OPAMP), ["LM358", "TL072", "1N4148", "QNPN", "Q2", "M1", "M2", "M3", "W1", "FOO", "Q3"]);
    }

    #[test]
    fn a_subcircuits_ports_stop_at_its_parameters_and_follow_continuations() {
        let (models, _) = parse_text(OPAMP);
        assert_eq!(models[0].kind, ModelKind::Subckt { ports: vec!["1".into(), "2".into(), "3".into(), "4".into(), "5".into()] });
        assert_eq!(models[1].kind, ModelKind::Subckt { ports: ["IN+", "IN-", "VCC", "VEE", "OUT"].map(String::from).to_vec() });
        assert!(models[0].code.starts_with(".SUBCKT LM358") && models[0].code.ends_with(".ENDS LM358"), "{}", models[0].code);
        assert_eq!(subckt_ports("A B C GAIN = 3"), ["A", "B", "C"]);
    }

    #[test]
    fn a_models_kicad_type_is_read_from_its_spice_type_level_and_version() {
        let (models, _) = parse_text(OPAMP);
        let ty = |name: &str| match &models.iter().find(|m| m.name == name).unwrap().kind {
            ModelKind::Model { model_type, .. } => model_type.map(|t| t.name),
            _ => panic!(),
        };
        assert_eq!(ty("1N4148"), Some("D"));
        assert_eq!(ty("QNPN"), Some("NPN_VBIC"));
        assert_eq!(ty("Q2"), Some("NPN_GUMMELPOON"), "no level: the default level");
        // `paramName == "level"` is compared as written: LEVEL=3 is no level, so MOS1
        assert_eq!(ty("M1"), Some("NMOS_MOS1"));
        assert_eq!(ty("M2"), Some("NMOS_BSIM3"));
        assert_eq!(ty("M3"), Some("PMOS_VDMOS"));
        assert_eq!(ty("W1"), Some("SW_I"));
        assert_eq!(ty("FOO"), None, "a raw SPICE element");
        assert_eq!(ty("Q3"), Some("NPN_VBIC"), "ako: the parent's type");
        assert_eq!(read_type_from_spice_strings("NMOS", "73", "2.2.0", false).map(|t| t.name), Some("NMOS_HISIMHV2"));
        assert_eq!(read_type_from_spice_strings("NMOS", "14", "4.8", false), None, "a version KiCad does not list is a raw element");
    }

    #[test]
    fn comments_and_continuations() {
        let text = ".model A D ; comment .model B D\n* .model C D\n.model D1 D\n+ (IS=1) $ more\n.control\n.model E D\n.endc\n";
        assert_eq!(names(text), ["A", "D1"]);
    }

    #[test]
    fn a_library_reads_its_includes_and_is_refused_when_one_is_missing() {
        let dir = std::env::temp_dir().join(format!("eda-sim-lib-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("models/main.lib"), ".include \"diodes.lib\"\n.subckt X a b\n.ends\n").unwrap();
        std::fs::write(dir.join("models/diodes.lib"), ".model DX D\n").unwrap();
        let lib = read_library("models/main.lib", &dir, None);
        assert!(lib.messages.is_empty(), "{:?}", lib.messages);
        assert_eq!(lib.models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["X", "DX"]);
        let lib = read_library("${KIPRJMOD}/models/main.lib", &dir, None);
        assert_eq!(lib.models.len(), 2);

        std::fs::write(dir.join("models/broken.lib"), ".inc gone.lib\n.model DY D\n").unwrap();
        let lib = read_library("models/broken.lib", &dir, None);
        assert!(lib.models.is_empty());
        assert!(lib.messages[0].starts_with("Simulation model library not found at '"), "{:?}", lib.messages);
        assert!(lib.messages.iter().any(|m| m.starts_with("File '") && m.ends_with("' does not exist.")), "{:?}", lib.messages);

        let lib = read_library("nope.lib", &dir, None);
        assert_eq!(lib.messages, [format!("Simulation model library not found at '{}'", dir.join("nope.lib").display())]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
