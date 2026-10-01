//! Load a real KiCad library symbol (a `.kicad_sym` file) into
//! `eda_model::LibSymbol`, so a part's `lib_id` -- explicit (`symbol:` in
//! the intent) or defaulted by kind (`eda_model::resolve_lib_id`) -- comes
//! with its real graphics and real per-pin electrical types instead of a
//! synthesized generic box. Mirrors `footprint_lib.rs` closely: same
//! `Library:Name` id shape, same "found -> parse it, not found -> caller
//! falls back" contract, same sexpr reader.
//!
//! A `.kicad_sym` file is a flat list of `(symbol "Name" ...)` entries (no
//! nesting between *different* parts -- the nesting a single part's own
//! `(symbol "Name_0_1" ...)`/`(symbol "Name_1_1" ...)` sub-blocks show is
//! its own unit/body-style graphics, not another part). One further
//! wrinkle: a symbol may say `(extends "OtherName")`, meaning "same
//! graphics and pins as OtherName, in the same file, only my own
//! properties differ" (this is how KiCad's own library expresses a family
//! of fixed-voltage regulators, `AMS1117-3.3`/`AMS1117-5.0`/... all
//! extending `AP1117-15`) -- resolved here by re-parsing the base symbol
//! from the same file and borrowing its graphics/pins/power flag.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{resolve_lib_id, ConstraintModel};

use crate::sexpr::{self, Sexpr};

/// Environment variable overriding where KiCad's own symbol libraries (a
/// directory of `<Library>.kicad_sym` files) live.
pub const SYMBOL_LIBRARY_ROOT_ENV: &str = "EDA_KICAD_SYMBOLS";

/// Where KiCad's standard symbol libraries live: `SYMBOL_LIBRARY_ROOT_ENV`
/// if set, else KiCad's own default install location on macOS.
pub fn default_symbol_library_root() -> PathBuf {
    if let Ok(p) = std::env::var(SYMBOL_LIBRARY_ROOT_ENV) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols")
}

/// The `.kicad_sym` file a library name (`"Device"`, `"Regulator_Linear"`)
/// names, under `lib_root`. `None` when it does not exist.
pub fn find_symbol_library_file(lib_root: &Path, lib_name: &str) -> Option<PathBuf> {
    let path = lib_root.join(format!("{lib_name}.kicad_sym"));
    path.is_file().then_some(path)
}

/// A parsed `.kicad_sym` file's whole symbol table, keyed by the bare
/// symbol name (no library prefix) -- cached per absolute path, since a
/// library like `Device.kicad_sym` is large (hundreds of symbols) and
/// resolving several parts from the same library must not re-parse it
/// from scratch each time.
type LibraryTable = HashMap<String, LibSymbol>;

fn cache() -> &'static Mutex<HashMap<PathBuf, std::sync::Arc<LibraryTable>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, std::sync::Arc<LibraryTable>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve `lib_id` (`"Device:R"`) against the real installed libraries
/// under `lib_root`. `None` when the library file does not exist, the
/// symbol is not in it, or the id is not `"Library:Name"` shaped -- any of
/// which just means "no real symbol"; the caller falls back to
/// `eda_model::symbol::builtin` and finally to a synthesized generic box.
pub fn resolve_symbol(lib_root: &Path, lib_id: &str) -> Option<LibSymbol> {
    let (lib_name, symbol_name) = lib_id.split_once(':')?;
    if lib_name.is_empty() || symbol_name.is_empty() {
        return None;
    }
    let path = find_symbol_library_file(lib_root, lib_name)?;
    let table = load_library_table(&path).ok()?;
    // The table is keyed (and each entry's own `lib_id` initially set) by
    // the *bare* name a standalone `.kicad_sym` file uses for its `(symbol
    // "Name" ...)` entries -- unlike an embedded `lib_symbols` block, which
    // already spells each entry `"Library:Name"` (see `sch_import`'s own
    // use of `build_symbol_table`, which needs no such fixup). Restore the
    // fully-qualified id the caller asked for, so a resolved symbol's own
    // `lib_id` always matches how it was looked up.
    let mut sym = table.get(symbol_name).cloned()?;
    sym.lib_id = lib_id.to_string();
    Some(sym)
}

/// Every symbol in one real installed library file (`"Device"` ->
/// `Device:R`, `Device:C`, ...), each with its `lib_id` fixed up to the
/// fully-qualified form (see `resolve_symbol`'s own doc on why the raw
/// table doesn't already have it), sorted by bare name. Empty, not an
/// error, when the library file doesn't exist -- same "no real symbol,
/// caller falls back" contract every other lookup in this module uses.
/// `A`'s symbol chooser is this function's only caller: it needs every
/// symbol a library defines, not just the ones some part's `symbol:`
/// field already resolved one of.
pub fn list_symbols_in_library(lib_root: &Path, lib_name: &str) -> Vec<LibSymbol> {
    let Some(path) = find_symbol_library_file(lib_root, lib_name) else { return Vec::new() };
    let Ok(table) = load_library_table(&path) else { return Vec::new() };
    let mut out: Vec<LibSymbol> = table
        .iter()
        .map(|(name, sym)| {
            let mut sym = sym.clone();
            sym.lib_id = format!("{lib_name}:{name}");
            sym
        })
        .collect();
    out.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
    out
}

/// Every `.kicad_sym` file directly under `lib_root`, as bare library
/// names (`"Device"`, not `"Device.kicad_sym"`) -- what `A`'s symbol
/// chooser treats as "the libraries we already load" when it also wants
/// real installed-library content, not just the hand-ported `builtin`
/// catalog. Empty, not an error, when `lib_root` doesn't exist (the
/// common case in an environment with no real KiCad install -- every
/// caller already treats an empty result the same as "nothing more to
/// offer beyond `builtin`").
pub fn list_symbol_libraries(lib_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(lib_root) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(String::from).filter(|_| e.path().extension().and_then(|x| x.to_str()) == Some("kicad_sym")))
        .collect();
    names.sort();
    names
}

fn load_library_table(path: &Path) -> Result<std::sync::Arc<LibraryTable>, String> {
    {
        let guard = cache().lock().unwrap();
        if let Some(t) = guard.get(path) {
            return Ok(t.clone());
        }
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let table = parse_symbol_library(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let table = std::sync::Arc::new(table);
    cache().lock().unwrap().insert(path.to_path_buf(), table.clone());
    Ok(table)
}

/// Parse a whole `.kicad_sym` file's text into a name -> `LibSymbol` table,
/// resolving every `(extends ...)` relationship internally so callers
/// never see one.
pub fn parse_symbol_library(text: &str) -> Result<LibraryTable, String> {
    let tree = sexpr::parse(text).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    let root = tree.as_list().filter(|l| sexpr::tag(l) == Some("kicad_symbol_lib")).ok_or("top-level form is not (kicad_symbol_lib ...); not a KiCad symbol library file")?;
    Ok(build_symbol_table(root))
}

/// Same table-building pass as [`parse_symbol_library`], shared with the
/// `.kicad_sch` reader (`sch_import.rs`) for a schematic's own embedded
/// `(lib_symbols ...)` block -- there each `(symbol ...)`'s name is already
/// a full `"Library:Name"` lib id (that is what a schematic's `lib_symbols`
/// entries are keyed by), rather than a bare name, but the grammar and the
/// `extends` resolution are otherwise identical.
pub(crate) fn build_symbol_table(list: &[Sexpr]) -> LibraryTable {
    // Two passes: first collect every symbol's own raw node (so `extends`
    // can look sideways at a base defined later in the file/block -- KiCad
    // does not require base-before-derived ordering), then build each into
    // a `LibSymbol`, following one level of `extends`.
    let mut raw: HashMap<&str, &[Sexpr]> = HashMap::new();
    for item in sexpr::find_all(list, "symbol") {
        if let Some(name) = sexpr::txt(item, 1) {
            raw.insert(name, item);
        }
    }

    let mut out = HashMap::new();
    for (&name, &item) in &raw {
        out.insert(name.to_string(), build_symbol(name, item, &raw));
    }
    out
}

/// Build one `LibSymbol` from its raw `(symbol "Name" ...)` node. `extends`
/// borrows the base's graphics/pins/power flag (one level only -- no
/// library in practice chains more than one deep); the derived symbol's
/// own properties always win.
fn build_symbol(name: &str, item: &[Sexpr], raw: &HashMap<&str, &[Sexpr]>) -> LibSymbol {
    let extends = sexpr::find(item, "extends").and_then(|e| sexpr::txt(e, 1)).map(String::from);
    let base = extends.as_deref().and_then(|b| raw.get(b));

    let own = graphics_and_pins(item);
    let (graphics, pins, unit_count) = match base {
        // `extends` with no graphics/pins of its own (the common case --
        // AMS1117-3.3 only overrides properties): borrow the base's
        // wholesale. A derived symbol that *does* draw its own graphics
        // (rare, but legal -- an alternate body style) keeps them.
        Some(base_item) if own.0.is_empty() && own.1.is_empty() => graphics_and_pins(base_item),
        _ => own,
    };
    let power = item.iter().any(|it| it.as_list().is_some_and(|l| sexpr::tag(l) == Some("power"))) || base.is_some_and(|b| b.iter().any(|it| it.as_list().is_some_and(|l| sexpr::tag(l) == Some("power"))));

    let prop = |key: &str| -> String {
        sexpr::find_all(item, "property")
            .find(|p| sexpr::txt(p, 1) == Some(key))
            .or_else(|| base.map(|b| sexpr::find_all(b, "property")).into_iter().flatten().find(|p| sexpr::txt(p, 1) == Some(key)))
            .and_then(|p| sexpr::txt(p, 2))
            .unwrap_or("")
            .to_string()
    };
    let bool_field = |key: &str, default: bool| -> bool {
        sexpr::find(item, key).and_then(|f| sexpr::txt(f, 1)).map(|v| v == "yes").unwrap_or(default)
    };

    LibSymbol {
        lib_id: name.to_string(),
        graphics,
        pins,
        power,
        in_bom: bool_field("in_bom", true),
        on_board: bool_field("on_board", true),
        datasheet: prop("Datasheet"),
        description: prop("Description"),
        reference_prefix: prop("Reference"),
        unit_count: unit_count.max(1),
    }
}

/// Every graphic/pin across every unit sub-block of one symbol's own raw
/// node (`(symbol "Name_0_1" ...)`, `(symbol "Name_1_1" ...)`, ...), plus
/// the highest unit index seen.
fn graphics_and_pins(item: &[Sexpr]) -> (Vec<SymbolGraphic>, Vec<LibPin>, u32) {
    let mut graphics = Vec::new();
    let mut pins = Vec::new();
    let mut max_unit = 1u32;
    for sub in sexpr::find_all(item, "symbol") {
        let Some(sub_name) = sexpr::txt(sub, 1) else { continue };
        let unit = unit_of_subname(sub_name);
        max_unit = max_unit.max(unit);
        for r in sexpr::find_all(sub, "rectangle") {
            if let (Some(s), Some(e)) = (sexpr::find(r, "start"), sexpr::find(r, "end")) {
                if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (sexpr::num(s, 1), sexpr::num(s, 2), sexpr::num(e, 1), sexpr::num(e, 2)) {
                    graphics.push(SymbolGraphic::Rectangle { unit, start: SPoint::new(sx, sy), end: SPoint::new(ex, ey), stroke_mm: stroke_width(r), filled: is_filled(r) });
                }
            }
        }
        for p in sexpr::find_all(sub, "polyline") {
            if let Some(pts_node) = sexpr::find(p, "pts") {
                let pts: Vec<SPoint> = sexpr::find_all(pts_node, "xy").filter_map(|xy| Some(SPoint::new(sexpr::num(xy, 1)?, sexpr::num(xy, 2)?))).collect();
                if pts.len() >= 2 {
                    graphics.push(SymbolGraphic::Polyline { unit, pts, stroke_mm: stroke_width(p), filled: is_filled(p) });
                }
            }
        }
        for c in sexpr::find_all(sub, "circle") {
            if let (Some(center), Some(radius)) = (sexpr::find(c, "center"), sexpr::find(c, "radius").and_then(|r| sexpr::num(r, 1))) {
                if let (Some(cx), Some(cy)) = (sexpr::num(center, 1), sexpr::num(center, 2)) {
                    graphics.push(SymbolGraphic::Circle { unit, center: SPoint::new(cx, cy), radius_mm: radius, stroke_mm: stroke_width(c), filled: is_filled(c) });
                }
            }
        }
        for a in sexpr::find_all(sub, "arc") {
            if let (Some(s), Some(m), Some(e)) = (sexpr::find(a, "start"), sexpr::find(a, "mid"), sexpr::find(a, "end")) {
                if let (Some(sx), Some(sy), Some(mx), Some(my), Some(ex), Some(ey)) = (sexpr::num(s, 1), sexpr::num(s, 2), sexpr::num(m, 1), sexpr::num(m, 2), sexpr::num(e, 1), sexpr::num(e, 2)) {
                    graphics.push(SymbolGraphic::Arc { unit, start: SPoint::new(sx, sy), mid: SPoint::new(mx, my), end: SPoint::new(ex, ey), stroke_mm: stroke_width(a), filled: is_filled(a) });
                }
            }
        }
        for t in sexpr::find_all(sub, "text") {
            if let (Some(content), Some(at)) = (sexpr::txt(t, 1), sexpr::find(t, "at")) {
                if let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) {
                    let angle = sexpr::num(at, 3).unwrap_or(0.0);
                    let size = sexpr::find(t, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).unwrap_or(1.27);
                    graphics.push(SymbolGraphic::Text { unit, text: content.to_string(), at: SPoint::new(x, y), angle_deg: angle, size_mm: size });
                }
            }
        }
        for p in sexpr::find_all(sub, "pin") {
            if let Some(lib_pin) = parse_pin(p, unit) {
                pins.push(lib_pin);
            }
        }
    }
    (graphics, pins, max_unit)
}

/// `"R_0_1"` -> unit 0; `"R_1_1"` -> unit 1; a name with no trailing
/// `_<unit>_<style>` pair (should not happen in a real library, but this
/// reader is tolerant, not a validator) defaults to unit 1.
fn unit_of_subname(sub_name: &str) -> u32 {
    let mut parts = sub_name.rsplitn(3, '_');
    let _style = parts.next();
    parts.next().and_then(|u| u.parse().ok()).unwrap_or(1)
}

fn stroke_width(item: &[Sexpr]) -> f64 {
    sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).unwrap_or(0.254)
}

fn is_filled(item: &[Sexpr]) -> bool {
    sexpr::find(item, "fill").and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)).map(|t| t != "none").unwrap_or(false)
}

/// `(pin <etype> <shape> (at x y angle) (length L) (name "N" ...) (number "N" ...))`.
fn parse_pin(item: &[Sexpr], unit: u32) -> Option<LibPin> {
    let electrical_type = sexpr::txt(item, 1)?.to_string();
    let shape = sexpr::txt(item, 2).unwrap_or("line").to_string();
    let at = sexpr::find(item, "at")?;
    let x = sexpr::num(at, 1)?;
    let y = sexpr::num(at, 2)?;
    let angle_deg = sexpr::num(at, 3).unwrap_or(0.0);
    let length_mm = sexpr::find(item, "length").and_then(|l| sexpr::num(l, 1)).unwrap_or(0.0);
    let name = sexpr::find(item, "name").and_then(|n| sexpr::txt(n, 1)).unwrap_or("~").to_string();
    let number = sexpr::find(item, "number").and_then(|n| sexpr::txt(n, 1)).unwrap_or("").to_string();
    let name = if name == "~" { String::new() } else { name };
    Some(LibPin { number, name, electrical_type, shape, at: SPoint::new(x, y), angle_deg, length_mm, unit: unit.max(1) })
}

/// For every part in `model` whose resolved `lib_id`
/// (`eda_model::resolve_lib_id`) is not synthetic and not already in
/// `model.symbols`, try to load it from `lib_root`'s real installed
/// libraries and add it there -- so `model.symbol_of` (explicit list, then
/// [`eda_model::symbol::builtin`]) resolves it with real graphics and real
/// per-pin electrical types. Best-effort, exactly like
/// `resolve_library_footprints`: a lib_id that is not `"Library:Name"`
/// shaped, or whose file/symbol does not exist, is left for `symbol_of`'s
/// built-in fallback to handle, silently -- that is the expected path for
/// most parts (a plain resistor, a generic IC), not an error. Returns one
/// human-readable warning per symbol that resolved to a file that exists
/// but failed to parse.
pub fn resolve_library_symbols(model: &mut ConstraintModel, lib_root: &Path) -> Vec<String> {
    let have: std::collections::BTreeSet<String> = model.symbols.iter().map(|s| s.lib_id.clone()).collect();
    let mut wanted: Vec<String> = model.parts.iter().map(resolve_lib_id).filter(|id| !eda_model::is_synthetic_lib_id(id) && !have.contains(id)).collect();
    wanted.sort();
    wanted.dedup();

    let mut warnings = Vec::new();
    for lib_id in wanted {
        let Some((lib_name, _)) = lib_id.split_once(':') else { continue };
        let Some(path) = find_symbol_library_file(lib_root, lib_name) else { continue };
        match load_library_table(&path) {
            Ok(table) => {
                if let Some((_, symbol_name)) = lib_id.split_once(':') {
                    if let Some(sym) = table.get(symbol_name) {
                        let mut sym = sym.clone();
                        sym.lib_id = lib_id;
                        model.symbols.push(sym);
                    }
                    // A file that parses but does not contain this exact
                    // symbol name is not a warning: `symbol_of` falls back
                    // to `builtin`/synthetic, same as a missing file.
                }
            }
            Err(e) => warnings.push(e),
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_LIB: &str = r##"(kicad_symbol_lib (version 20231120) (generator "kicad_symbol_editor")
        (symbol "R"
            (pin_numbers (hide yes))
            (pin_names (offset 0))
            (exclude_from_sim no) (in_bom yes) (on_board yes)
            (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
            (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
            (property "Datasheet" "" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (property "Description" "Resistor" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "R_0_1"
                (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none)))
            )
            (symbol "R_1_1"
                (pin passive line (at 0 3.81 270) (length 1.27) (name "" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
                (pin passive line (at 0 -3.81 90) (length 1.27) (name "" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
            )
        )
        (symbol "AP1117-15"
            (property "Datasheet" "http://example.com/ap1117.pdf" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "AP1117-15_0_1"
                (rectangle (start -5.08 -5.08) (end 5.08 1.905) (stroke (width 0.254) (type default)) (fill (type background)))
            )
            (symbol "AP1117-15_1_1"
                (pin power_in line (at 0 -7.62 90) (length 2.54) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
                (pin power_out line (at 7.62 0 180) (length 2.54) (name "VO" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
                (pin power_in line (at -7.62 0 0) (length 2.54) (name "VI" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
            )
        )
        (symbol "AMS1117-3.3"
            (extends "AP1117-15")
            (property "Value" "AMS1117-3.3" (at 0 3.175 0) (effects (font (size 1.27 1.27))))
            (property "Datasheet" "http://example.com/ds1117.pdf" (at 0 0 0) (effects (font (size 1.27 1.27))))
        )
        (symbol "GND"
            (power)
            (pin_names (offset 0) (hide yes))
            (property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27))))
            (property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
            (symbol "GND_0_1"
                (polyline (pts (xy 0 0) (xy 0 -1.27) (xy 1.27 -1.27) (xy 0 -2.54) (xy -1.27 -1.27) (xy 0 -1.27)) (stroke (width 0) (type default)) (fill (type none)))
            )
            (symbol "GND_1_1"
                (pin power_in line (at 0 0 270) (length 0) (name "" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
            )
        )
    )"##;

    #[test]
    fn parses_plain_symbol_graphics_and_pins() {
        let table = parse_symbol_library(MINI_LIB).unwrap();
        let r = table.get("R").expect("R present");
        assert_eq!(r.pins.len(), 2);
        assert_eq!(r.pins[0].number, "1");
        assert_eq!(r.pins[0].electrical_type, "passive");
        assert!(matches!(r.graphics[0], SymbolGraphic::Rectangle { .. }));
        assert!(!r.power);
    }

    #[test]
    fn extends_borrows_base_graphics_and_pins() {
        let table = parse_symbol_library(MINI_LIB).unwrap();
        let ams = table.get("AMS1117-3.3").expect("AMS1117-3.3 present");
        assert_eq!(ams.pins.len(), 3, "pins borrowed from AP1117-15");
        let vin = ams.pins.iter().find(|p| p.name == "VI").unwrap();
        assert_eq!(vin.electrical_type, "power_in");
        let vout = ams.pins.iter().find(|p| p.name == "VO").unwrap();
        assert_eq!(vout.electrical_type, "power_out");
        // Own property overrides the base's.
        assert_eq!(ams.datasheet, "http://example.com/ds1117.pdf");
    }

    #[test]
    fn power_flag_detected() {
        let table = parse_symbol_library(MINI_LIB).unwrap();
        let gnd = table.get("GND").unwrap();
        assert!(gnd.power);
        assert_eq!(gnd.pins[0].electrical_type, "power_in");
    }

    #[test]
    fn resolve_symbol_from_file() {
        let dir = std::env::temp_dir().join("eda_kicad_symbol_lib_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Device.kicad_sym"), MINI_LIB).unwrap();
        let sym = resolve_symbol(&dir, "Device:R").expect("resolves");
        assert_eq!(sym.lib_id, "Device:R");
        assert_eq!(sym.pins.len(), 2);
        assert!(resolve_symbol(&dir, "Device:DoesNotExist").is_none());
        assert!(resolve_symbol(&dir, "NoSuchLib:R").is_none());
        assert!(resolve_symbol(&dir, "not_a_lib_id").is_none());
    }

    #[test]
    fn build_symbol_captures_the_reference_property_as_a_prefix() {
        let table = parse_symbol_library(MINI_LIB).unwrap();
        assert_eq!(table.get("R").unwrap().reference_prefix, "R");
        assert_eq!(table.get("GND").unwrap().reference_prefix, "#PWR", "power symbols use KiCad's own #PWR convention");
        assert_eq!(table.get("AP1117-15").unwrap().reference_prefix, "", "no Reference property in this fixture's AP1117-15 -- empty, not a guess");
    }

    #[test]
    fn list_symbols_in_library_returns_every_entry_with_a_fully_qualified_id() {
        let dir = std::env::temp_dir().join("eda_kicad_symbol_lib_list_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Device.kicad_sym"), MINI_LIB).unwrap();

        let all = list_symbols_in_library(&dir, "Device");
        let ids: Vec<&str> = all.iter().map(|s| s.lib_id.as_str()).collect();
        assert_eq!(ids, vec!["Device:AMS1117-3.3", "Device:AP1117-15", "Device:GND", "Device:R"], "every symbol in the file, sorted, not just one a part already resolved");
        assert!(all.iter().all(|s| s.lib_id.starts_with("Device:")), "every entry's lib_id is fixed up, same as resolve_symbol's own doc explains the raw table needs");

        assert!(list_symbols_in_library(&dir, "NoSuchLib").is_empty(), "a library with no file is empty, not an error -- same contract as resolve_symbol");
    }

    #[test]
    fn list_symbol_libraries_finds_kicad_sym_files_only() {
        let dir = std::env::temp_dir().join("eda_kicad_symbol_lib_names_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Device.kicad_sym"), MINI_LIB).unwrap();
        std::fs::write(dir.join("power.kicad_sym"), MINI_LIB).unwrap();
        std::fs::write(dir.join("README.txt"), "not a library").unwrap();

        assert_eq!(list_symbol_libraries(&dir), vec!["Device", "power"], "sorted bare names, non-.kicad_sym files ignored");
        assert!(list_symbol_libraries(Path::new("/no/such/directory/at/all")).is_empty(), "a missing root is empty, not a panic");
    }

    #[test]
    fn resolve_library_symbols_fills_model() {
        let dir = std::env::temp_dir().join("eda_kicad_symbol_lib_resolve_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Device.kicad_sym"), MINI_LIB).unwrap();

        let part = |r: &str, sym: Option<&str>, pins: usize| eda_model::Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: None,
            footprint: None,
            symbol: sym.map(String::from),
            datasheet: None,
            pins: (1..=pins).map(|n| eda_model::Pin { number: n.to_string(), name: None, kind: eda_model::PinKind::Passive }).collect(),
            body_um: None,
            edge: None,
        };
        let mut model = ConstraintModel { parts: vec![part("R1", None, 2), part("U9", None, 20)], ..Default::default() };
        let warnings = resolve_library_symbols(&mut model, &dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(model.symbols.len(), 1, "only R1 resolves to a real Device:R -- U9 (20 pins) is a synthetic generic box, never looked up");
        assert_eq!(model.symbols[0].lib_id, "Device:R");
    }

    /// The two real, task-named symbols, straight from a real KiCad
    /// install -- not `#[ignore]`d, but skipped gracefully when there is
    /// none, the same pattern `footprint_lib.rs` uses.
    #[test]
    fn loads_real_installed_symbols() {
        let root = default_symbol_library_root();
        let Some(r) = resolve_symbol(&root, "Device:R") else {
            eprintln!("KiCad symbol libraries not found at {}; skipping", root.display());
            return;
        };
        assert_eq!(r.pins.len(), 2);
        assert_eq!(r.pins[0].electrical_type, "passive");

        let ams = resolve_symbol(&root, "Regulator_Linear:AMS1117-3.3").expect("ships with KiCad");
        assert_eq!(ams.pins.len(), 3, "inherited via extends from AP1117-15");
        assert!(ams.pins.iter().any(|p| p.electrical_type == "power_out"), "VOUT pin");
        assert!(!ams.datasheet.is_empty());

        let gnd = resolve_symbol(&root, "power:GND").expect("ships with KiCad");
        assert!(gnd.power);

        let conn = resolve_symbol(&root, "Connector_Generic:Conn_01x04").expect("ships with KiCad");
        assert_eq!(conn.pins.len(), 4);
    }
}
