//! The schematic clipboard in KiCad's own format, both ways.
//!
//! **Writing** ([`write_clipboard`]) ports `SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION*, ..., aForClipboard = true )` as
//! `SCH_EDITOR_CONTROL::doCopy` calls it: `(lib_symbols ...)` with the library symbol of every selected symbol, then the selected items one
//! after the other, with no `(kicad_sch ...)` around them. Real KiCad reads exactly that on Paste (`SCH_IO_KICAD_SEXPR::LoadContent` ->
//! `ParseSchematic( aIsCopyableOnly = true )`), and a fragment written by real KiCad is what [`parse_clipboard`] reads.
//!
//! **Reading** ([`parse_clipboard`]) takes the same top-level forms `ParseSchematic( aIsCopyableOnly )` takes and refuses any other (KiCad then
//! pastes the text itself as a text item -- the caller does that). The items are read by the `.kicad_sch` importer
//! ([`crate::import_kicad_sch`]) on the fragment wrapped in a file header, so a symbol, a wire or a label means here on paste exactly what it
//! means in a file; the library symbols come through the Symbol Editor's reader, which keeps hidden pins and fills.
//!
//! What the schematic IR does not hold is not written and not read: a field's position and size, a wire's stroke. The
//! fields of a copied symbol are put beside it and marked `(fields_autoplaced yes)`, so KiCad lays them out when the pasted symbol is moved.

use crate::sexpr;
use crate::{duid_plain, fmt_mm_f, mm, note_item_uuid, sexpr_str};
use eda_model::ir::{Design, LabelKind, LibrarySymbol, Point, SchematicSection, SymbolInstance};
use eda_model::sch_clipboard::{place_offset_mm, place_offset_nm, SchFragment};
use eda_model::symbol::LibSymbol;
use eda_model::{is_synthetic_lib_id, CheckResult, ConstraintModel};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The top-level forms `SCH_IO_KICAD_SEXPR_PARSER::ParseSchematic` takes with `aIsCopyableOnly` (its `switch`, less the three that are
/// `Unexpected` for a copy: `paper`, `page`, `title_block`, and `bus_alias`).
const ACCEPTED: &[&str] = &[
    "group",
    "generator",
    "host",
    "generator_version",
    "uuid",
    "lib_symbols",
    "symbol",
    "image",
    "sheet",
    "junction",
    "no_connect",
    "bus_entry",
    "polyline",
    "bus",
    "wire",
    "arc",
    "circle",
    "rectangle",
    "bezier",
    "rule_area",
    "netclass_flag",
    "text",
    "label",
    "global_label",
    "hierarchical_label",
    "directive_label",
    "text_box",
    "table",
    "sheet_instances",
    "symbol_instances",
    "embedded_fonts",
    "embedded_files",
];

/// The file format version the header of a wrapped fragment claims: the one the clipboard is always parsed at (`LoadContent`'s default).
const FRAGMENT_VERSION: u32 = 20260326;

/// The properties every symbol has, which are fields of the IR itself; the others are user fields.
const MAIN_FIELDS: &[&str] = &["Reference", "Value", "Footprint", "Datasheet", "Description"];

// ---------------------------------------------------------------- reading

/// Read a clipboard text as a schematic fragment. `Err` when it is not one: not an s-expression, or one with a form that is not a schematic
/// item (KiCad pastes such a text as a text item).
pub fn parse_clipboard(text: &str) -> Result<SchFragment, String> {
    if !text.trim_start().starts_with('(') {
        return Err("the text is not an s-expression".into());
    }
    let tree = sexpr::parse(&format!("(clipboard {text}\n)")).map_err(|e| format!("not a valid s-expression: {e}"))?;
    let root = tree.as_list().ok_or("not a list")?;
    let mut notes: Vec<String> = Vec::new();
    for node in &root[1..] {
        match node.as_list().and_then(sexpr::tag) {
            Some(tag) if ACCEPTED.contains(&tag) => {}
            Some(tag) => return Err(format!("not a schematic fragment: ({tag} ...) is not an item KiCad pastes")),
            None => return Err("not a schematic fragment: it holds more than s-expression forms".into()),
        }
    }

    // The items, read as a file's are.
    let file = format!("(kicad_sch (version {FRAGMENT_VERSION}) (generator \"eda-clipboard\")\n{text}\n)");
    let (design, _model, imported) = crate::import_kicad_sch(&file).map_err(|e| e.first().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).unwrap_or_default())?;
    let mut section = design.schematic.unwrap_or_default();

    // The library symbols the fragment embeds, richer than the engine's reading of them.
    let mut lib_symbols: Vec<LibrarySymbol> = Vec::new();
    for cache in sexpr::find_all(root, "lib_symbols") {
        let parsed = crate::symbol_import::symbols_of(cache);
        notes.extend(parsed.warnings);
        lib_symbols.extend(parsed.symbols);
    }

    // The properties beyond the four main ones: user fields, by reference.
    for sym in sexpr::find_all(root, "symbol") {
        let props: Vec<(&str, &str)> = sexpr::find_all(sym, "property").filter_map(|p| Some((sexpr::txt(p, 1)?, sexpr::txt(p, 2)?))).collect();
        let Some(reference) = props.iter().find(|(k, _)| *k == "Reference").map(|(_, v)| v.to_string()) else { continue };
        for (key, value) in props {
            if MAIN_FIELDS.contains(&key) || key.starts_with("ki_") || key.starts_with("Sim.") || value.is_empty() {
                continue;
            }
            section.user_fields.entry(reference.clone()).or_default().insert(key.to_string(), value.to_string());
        }
    }

    if imported.unresolved_symbols > 0 {
        notes.push(format!("{} symbol(s) had no library symbol in the clipboard and were left out", imported.unresolved_symbols));
    }
    for (tag, what) in [("table", "tables")] {
        if sexpr::find_all(root, tag).next().is_some() {
            notes.push(format!("{what} are not pasted"));
        }
    }
    // A clipboard fragment has no hierarchy of its own.
    section.instance_overrides.clear();
    section.extras.locked.clear();
    section.imported_from_kicad = true;
    join_bends(&mut section);
    notes.sort();
    notes.dedup();
    Ok(SchFragment { section, lib_symbols, notes, sheet_screens: Default::default() })
}

/// KiCad has one segment to a wire, and a polyline this studio drew is written as one wire per segment. Read back, the segments that meet at a plain
/// bend -- exactly two wire ends there, of the same kind (wire or bus), and nothing else at that point: no junction, label, no-connect, bus entry or
/// power symbol -- are one polyline again, in the order they were written, so a copy of this studio's own wires pastes as the wires it copied.
fn join_bends(section: &mut SchematicSection) {
    use eda_model::ir::Wire;
    // the segments each joined wire was made of: a group names its members by the wires of the file, which the join replaces
    let mut made_of: Vec<Vec<String>> = Vec::new();
    use std::collections::{BTreeMap, BTreeSet};
    let mut busy: BTreeSet<Point> = BTreeSet::new();
    busy.extend(section.junctions.iter().map(|j| j.at));
    busy.extend(section.no_connects.iter().map(|n| n.at));
    busy.extend(section.labels.iter().map(|l| l.at));
    busy.extend(section.bus_entries.iter().flat_map(|b| [b.at, Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y }]));
    busy.extend(section.power_symbols.iter().map(|p| p.at));
    let wires = std::mem::take(&mut section.wires);
    let old_ids: std::collections::BTreeSet<String> = wires.iter().map(|w| w.id.clone()).collect();
    let ends = |w: &Wire| (*w.pts.first().unwrap(), *w.pts.last().unwrap());
    let mut count: BTreeMap<(bool, Point), usize> = BTreeMap::new();
    for w in wires.iter().filter(|w| w.pts.len() >= 2) {
        let (a, b) = ends(w);
        *count.entry((w.bus, a)).or_default() += 1;
        *count.entry((w.bus, b)).or_default() += 1;
    }
    let plain_bend = |bus: bool, p: Point| !busy.contains(&p) && count.get(&(bus, p)) == Some(&2);
    let mut used = vec![false; wires.len()];
    for i in 0..wires.len() {
        if used[i] || wires[i].pts.len() < 2 {
            continue;
        }
        used[i] = true;
        let bus = wires[i].bus;
        let mut pts = wires[i].pts.clone();
        let mut parts: Vec<String> = vec![wires[i].id.clone()];
        // On from the last point ...
        loop {
            let end = *pts.last().unwrap();
            if !plain_bend(bus, end) {
                break;
            }
            let Some(j) = (0..wires.len()).find(|&j| !used[j] && wires[j].bus == bus && wires[j].pts.len() >= 2 && (ends(&wires[j]).0 == end || ends(&wires[j]).1 == end)) else { break };
            used[j] = true;
            parts.push(wires[j].id.clone());
            if ends(&wires[j]).0 == end {
                pts.extend(wires[j].pts[1..].iter().copied());
            } else {
                pts.extend(wires[j].pts.iter().rev().skip(1).copied());
            }
        }
        // ... and back from the first one (the segment before it may come later in the text).
        loop {
            let start = pts[0];
            if !plain_bend(bus, start) {
                break;
            }
            let Some(j) = (0..wires.len()).find(|&j| !used[j] && wires[j].bus == bus && wires[j].pts.len() >= 2 && (ends(&wires[j]).0 == start || ends(&wires[j]).1 == start)) else { break };
            used[j] = true;
            parts.push(wires[j].id.clone());
            let mut before: Vec<Point> = if ends(&wires[j]).1 == start { wires[j].pts[..wires[j].pts.len() - 1].to_vec() } else { wires[j].pts.iter().rev().copied().take(wires[j].pts.len() - 1).collect() };
            before.extend(pts);
            pts = before;
        }
        section.wires.push(Wire { id: String::new(), net: String::new(), pins: Vec::new(), pts, bus });
        made_of.push(parts);
    }
    section.assign_missing_ids();
    // the groups that listed a segment list the wire it is part of now
    if !section.extras.groups.is_empty() {
        let mut now: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        for (w, parts) in section.wires.iter().zip(&made_of) {
            for old in parts {
                now.insert(old.clone(), w.id.clone());
            }
        }
        for g in section.extras.groups.iter_mut() {
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            // a segment that took no part in a wire (a point) is no member any more; the other items are what they were
            g.member_ids = g.member_ids.iter().filter(|m| !old_ids.contains(*m) || now.contains_key(*m)).map(|m| now.get(m).unwrap_or(m).clone()).filter(|m| seen.insert(m.clone())).collect();
        }
        eda_model::groups::prune_groups(&mut section.extras.groups);
    }
}

// ---------------------------------------------------------------- writing

/// What a copy needs to know about the design.
pub struct CopyInput<'a> {
    pub design: &'a Design,
    pub model: &'a ConstraintModel,
    /// The sheet in view: the selection is of its items.
    pub section: &'a SchematicSection,
    /// The project name the symbols' `(instances (project ...))` carry.
    pub project: &'a str,
}

/// What [`write_clipboard`] made.
#[derive(Debug, Clone, Default)]
pub struct CopyOutput {
    /// The clipboard text.
    pub text: String,
    /// How many items it holds.
    pub items: usize,
    /// What the selection had that a copy leaves out ("2 symbol(s) without a part to copy"), one line each.
    pub skipped: Vec<String>,
    /// The files of the hierarchical sheets the text holds: what each one shows is not in the text (KiCad keeps it beside the clipboard,
    /// `m_supplementaryClipboard`), so the caller keeps their screens.
    pub sheet_files: Vec<String>,
}

/// The library symbol with the places Swap Pins traded for the pins of `unit`: pin `n` has the position, orientation and length of pin `swaps[n]` (`SwapPinGeometry`),
/// in each body style the symbol has.
fn swapped_copy(base: &LibrarySymbol, unit: u32, swaps: &BTreeMap<String, String>) -> LibrarySymbol {
    let mut copy = base.clone();
    let here = |p: &eda_model::ir::LibrarySymbolPin, style: u32| p.body_style == style && (p.unit == 0 || p.unit == unit);
    for style in [1, 2] {
        let items: Vec<(String, (eda_model::symbol::SPoint, f64, f64))> = copy.pins.iter().filter(|p| here(p, style)).map(|p| (p.number.clone(), (p.at, p.angle_deg, p.length_mm))).collect();
        for (number, (at, angle, length)) in eda_model::sch_extras::permute_pins(items, swaps) {
            if let Some(p) = copy.pins.iter_mut().find(|p| here(p, style) && p.number == number) {
                p.at = at;
                p.angle_deg = angle;
                p.length_mm = length;
            }
        }
    }
    copy
}

/// One symbol of the selection, as the clipboard will have it.
struct CopiedSymbol<'a> {
    sym: &'a SymbolInstance,
    lib_id: String,
    /// KiCad's own origin of the symbol, sheet coordinates.
    origin: Point,
    /// The library symbol with both its body styles (the clipboard's `(lib_symbols ...)` names both).
    engine: LibSymbol,
    /// The body style the symbol is placed in (`SCH_SYMBOL::GetBodyStyle`): 1, or 2 for the alternate "De Morgan" one of a symbol that has it.
    style: u32,
}

impl CopiedSymbol<'_> {
    /// The library symbol as the placed symbol draws it (its box, its pins).
    fn drawn(&self) -> LibSymbol {
        if self.style > 1 && self.engine.has_alternate_body() {
            self.engine.in_style(self.style).into_owned()
        } else {
            self.engine.clone()
        }
    }
}

/// The library symbol a placed symbol draws from, its name in the clipboard and where KiCad's origin of it is.
///
/// A sheet this project drew places a symbol by the corner of its box, and draws the library symbol shifted so that corner is its origin
/// (`eda_engine::placed::corner_symbol`); KiCad's own origin is the library symbol's. The two differ by the corner of the symbol's box, turned
/// the way the symbol is. A sheet read from a `.kicad_sch` already has KiCad's origins.
fn copied_symbol<'a>(input: &CopyInput<'_>, sym: &'a SymbolInstance) -> Option<CopiedSymbol<'a>> {
    let lib_id = if sym.lib_id.is_empty() { format!("eda:{}", sym.id) } else { sym.lib_id.clone() };
    let part = input.model.part(&sym.id);
    // A generated symbol (`gen:`) is a real one made from the part (`ConstraintModel::real_symbol_of`, `gensym`): the same one the sheet's own file draws
    // and its wires reach, so the copy has its pins where the copied wires end. Only the older synthetic form (`eda:`) is the generic box below.
    let real: Option<LibSymbol> = if sym.lib_id.is_empty() {
        None
    } else {
        match part {
            Some(p) => input.model.real_symbol_of(&sym.lib_id, p),
            None if is_synthetic_lib_id(&sym.lib_id) => None,
            None => input.model.symbol_of(&sym.lib_id),
        }
    };
    match real {
        Some(engine) => {
            let style = if engine.has_alternate_body() { input.section.body_style_of(sym).min(2) } else { 1 };
            let origin = if input.section.imported_from_kicad {
                sym.at
            } else {
                let drawn = if style > 1 { engine.in_style(style).into_owned() } else { engine.clone() };
                let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&drawn, sym.unit);
                let (dx, dy) = place_offset_nm(sym.rot as f64 / 1000.0, sym.mirrored, sym.mirror_y, (-x0, -y1));
                Point { x: sym.at.x + dx, y: sym.at.y + dy }
            };
            Some(CopiedSymbol { sym, lib_id, origin, engine, style })
        }
        None => {
            // No library symbol: the generic box the sheet draws, whose origin is its corner.
            let engine = eda_engine::placed::corner_symbol(&lib_id, part?, None, sym.unit);
            Some(CopiedSymbol { sym, lib_id, origin: sym.at, engine, style: 1 })
        }
    }
}

/// A KiCad quoted, tab-indented property line.
fn property(out: &mut String, name: &str, value: &str, at: (f64, f64), hide: bool) {
    let effects = if hide { "(effects (font (size 1.27 1.27)) (hide yes))" } else { "(effects (font (size 1.27 1.27)))" };
    let _ = writeln!(out, "\t\t(property {} {} (at {} {} 0) {effects})", sexpr_str(name), sexpr_str(value), fmt_mm_f(at.0), fmt_mm_f(at.1));
}

fn label_shape_token(shape: eda_model::ir::LabelShape) -> &'static str {
    crate::label_shape_token(shape)
}

/// The box a symbol covers on the sheet (mm): its library box turned and mirrored the way it is placed, around its origin.
fn sheet_box_mm(c: &CopiedSymbol<'_>) -> (f64, f64, f64, f64) {
    let (x0, y0, x1, y1) = eda_engine::geometry::real_symbol_bbox(&c.drawn(), c.sym.unit);
    let angle = c.sym.rot as f64 / 1000.0;
    let (ox, oy) = (c.origin.x as f64 / 1000.0, c.origin.y as f64 / 1000.0);
    let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|p| {
        let (dx, dy) = place_offset_mm(angle, c.sym.mirrored, c.sym.mirror_y, p);
        (ox + dx, oy + dy)
    });
    let min = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::INFINITY, f64::min);
    let max = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
    (min(|p| p.0), min(|p| p.1), max(|p| p.0), max(|p| p.1))
}

/// The symbol form (`SCH_IO_KICAD_SEXPR::saveSymbol` with `aForClipboard`).
fn write_symbol(out: &mut String, input: &CopyInput<'_>, c: &CopiedSymbol<'_>, lib_name: Option<&str>) {
    let sym = c.sym;
    let part = input.model.part(&sym.id);
    let yes = |b: bool| if b { "yes" } else { "no" };
    let (ox, oy) = (c.origin.x as f64 / 1000.0, c.origin.y as f64 / 1000.0);
    let _ = writeln!(out, "\t(symbol");
    // a symbol whose pins Swap Pins moved names its own copy of the library symbol (`SCH_SCREEN::Append`)
    if let Some(name) = lib_name {
        let _ = writeln!(out, "\t\t(lib_name {})", sexpr_str(name));
    }
    let _ = writeln!(out, "\t\t(lib_id {})", sexpr_str(&c.lib_id));
    let _ = writeln!(out, "\t\t(at {} {} {})", fmt_mm_f(ox), fmt_mm_f(oy), fmt_mm_f(sym.rot as f64 / 1000.0));
    if sym.mirrored || sym.mirror_y {
        // `(mirror y)` negates x (the IR's `mirrored`), `(mirror x)` negates y (`mirror_y`).
        let _ = writeln!(out, "\t\t(mirror {})", if sym.mirrored { "y" } else { "x" });
    }
    let _ = writeln!(out, "\t\t(unit {})", sym.unit);
    let _ = writeln!(out, "\t\t(body_style {})", c.style);
    let _ = writeln!(out, "\t\t(exclude_from_sim {})", yes(sym.exclude_from_sim));
    let _ = writeln!(out, "\t\t(in_bom {})", yes(!sym.exclude_from_bom));
    let _ = writeln!(out, "\t\t(on_board {})", yes(!sym.exclude_from_board));
    let _ = writeln!(out, "\t\t(in_pos_files yes)");
    let _ = writeln!(out, "\t\t(dnp {})", yes(sym.dnp));
    let _ = writeln!(out, "\t\t(fields_autoplaced yes)");
    let uuid = duid_plain(&format!("clipboard:symbol:{}:{}", sym.id, sym.unit));
    note_item_uuid(&sym.id, &uuid);
    let _ = writeln!(out, "\t\t(uuid \"{uuid}\")");

    let (min_x, min_y, max_x, max_y) = sheet_box_mm(c);
    let cx = (min_x + max_x) / 2.0;
    let value = if !sym.value.is_empty() { sym.value.clone() } else { part.and_then(|p| p.value.clone()).unwrap_or_else(|| sym.id.clone()) };
    let footprint = if !sym.footprint.is_empty() { sym.footprint.clone() } else { part.and_then(|p| p.footprint.clone()).unwrap_or_default() };
    let datasheet = if !sym.datasheet.is_empty() { sym.datasheet.clone() } else { c.engine.datasheet.clone() };
    property(out, "Reference", &sym.id, (cx, min_y - 1.27), false);
    property(out, "Value", &value, (cx, max_y + 1.27), false);
    property(out, "Footprint", &footprint, (ox, oy), true);
    property(out, "Datasheet", &datasheet, (ox, oy), true);
    if !c.engine.description.is_empty() {
        property(out, "Description", &c.engine.description, (ox, oy), true);
    }
    if let Some(fields) = input.section.user_fields.get(&sym.id) {
        for (name, text) in fields {
            property(out, name, text, (ox, oy), true);
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for pin in c.engine.pins.iter().filter(|p| p.unit == 0 || p.unit == sym.unit) {
        if seen.contains(&pin.number.as_str()) {
            continue;
        }
        seen.push(&pin.number);
        let _ = writeln!(out, "\t\t(pin {} (uuid \"{}\"))", sexpr_str(&pin.number), duid_plain(&format!("clipboard:pin:{}:{}:{}", sym.id, sym.unit, pin.number)));
    }
    let _ = writeln!(out, "\t\t(instances");
    let _ = writeln!(out, "\t\t\t(project {}", sexpr_str(input.project));
    let _ = writeln!(out, "\t\t\t\t(path \"\"");
    let _ = writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&sym.id));
    let _ = writeln!(out, "\t\t\t\t\t(unit {})", sym.unit);
    let _ = writeln!(out, "\t\t\t\t)");
    let _ = writeln!(out, "\t\t\t)");
    let _ = writeln!(out, "\t\t)");
    let _ = writeln!(out, "\t)");
}

/// A power symbol is a symbol of a `(power)` library symbol whose Value is the net it asserts. The IR keeps where its pin is, KiCad where its
/// origin is: the pin of a power symbol is at the origin in every library, but not in a custom one.
fn write_power_symbol(out: &mut String, input: &CopyInput<'_>, p: &eda_model::ir::PowerSymbol, engine: &LibSymbol) {
    let angle = p.rot as f64 / 1000.0;
    let pin_local = engine.pins.first().map(|pin| (pin.at.x, pin.at.y)).unwrap_or((0.0, 0.0));
    let (dx, dy) = place_offset_mm(angle, false, false, pin_local);
    let (ox, oy) = (p.at.x as f64 / 1000.0 - dx, p.at.y as f64 / 1000.0 - dy);
    let _ = writeln!(out, "\t(symbol");
    let _ = writeln!(out, "\t\t(lib_id {})", sexpr_str(&p.lib_id));
    let _ = writeln!(out, "\t\t(at {} {} {})", fmt_mm_f(ox), fmt_mm_f(oy), fmt_mm_f(angle));
    let _ = writeln!(out, "\t\t(unit 1)");
    let _ = writeln!(out, "\t\t(body_style 1)");
    let _ = writeln!(out, "\t\t(exclude_from_sim no)");
    let _ = writeln!(out, "\t\t(in_bom no)");
    let _ = writeln!(out, "\t\t(on_board yes)");
    let _ = writeln!(out, "\t\t(in_pos_files yes)");
    let _ = writeln!(out, "\t\t(dnp no)");
    let _ = writeln!(out, "\t\t(fields_autoplaced yes)");
    let uuid = duid_plain(&format!("clipboard:power:{}", p.id));
    note_item_uuid(&p.id, &uuid);
    let _ = writeln!(out, "\t\t(uuid \"{uuid}\")");
    property(out, "Reference", &p.id, (ox, oy), true);
    property(out, "Value", &p.net, (ox, oy - 2.54), false);
    property(out, "Footprint", "", (ox, oy), true);
    property(out, "Datasheet", "", (ox, oy), true);
    let _ = writeln!(out, "\t\t(pin \"1\" (uuid \"{}\"))", duid_plain(&format!("clipboard:power-pin:{}", p.id)));
    let _ = writeln!(out, "\t\t(instances");
    let _ = writeln!(out, "\t\t\t(project {}", sexpr_str(input.project));
    let _ = writeln!(out, "\t\t\t\t(path \"\"");
    let _ = writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&p.id));
    let _ = writeln!(out, "\t\t\t\t\t(unit 1)");
    let _ = writeln!(out, "\t\t\t\t)");
    let _ = writeln!(out, "\t\t\t)");
    let _ = writeln!(out, "\t\t)");
    let _ = writeln!(out, "\t)");
}

/// While a copy is written, the uuid every item is written as is noted (`crate::note_item_uuid`), so the groups can name their members by them.
struct UuidScope(Option<BTreeMap<String, Vec<String>>>);

impl UuidScope {
    fn start() -> UuidScope {
        UuidScope(crate::ITEM_UUIDS.with(|m| m.borrow_mut().replace(BTreeMap::new())))
    }

    /// What has been noted so far.
    fn snapshot() -> BTreeMap<String, Vec<String>> {
        crate::ITEM_UUIDS.with(|m| m.borrow().clone().unwrap_or_default())
    }
}

impl Drop for UuidScope {
    fn drop(&mut self) {
        crate::ITEM_UUIDS.with(|m| *m.borrow_mut() = self.0.take());
    }
}

/// The text of `ids`' items as KiCad's clipboard has them. `Err` when the selection holds nothing a copy takes.
///
/// `ids` are what the studio selects by: a symbol's reference (all its placed units), the ids of wires, labels, texts, power symbols,
/// no-connects, bus entries, junctions, lines and drawn graphics, and the ids of hierarchical sheets: a `(sheet ...)` goes in the text with its pins, and the file it
/// shows is in `CopyOutput::sheet_files` (what a paste needs the screens of; `SCH_EDITOR_CONTROL::m_supplementaryClipboard`).
pub fn write_clipboard(input: &CopyInput<'_>, ids: &[String]) -> Result<CopyOutput, Vec<CheckResult>> {
    let sch = input.section;
    let _uuids = UuidScope::start();
    // "Groups need to have all their items selected" (`SCH_EDITOR_CONTROL::doCopy`): a selected group is copied with everything below it, the groups in it too
    let tree = sch.extras.group_tree();
    let mut copied_groups: Vec<String> = Vec::new();
    for id in ids.iter().filter(|id| tree.is_group(id)) {
        for g in tree.group_and_descendants(id) {
            if !copied_groups.contains(&g) {
                copied_groups.push(g);
            }
        }
    }
    let mut ids: Vec<String> = ids.to_vec();
    for g in &copied_groups {
        for leaf in tree.group_leaves(g) {
            if !ids.contains(&leaf) {
                ids.push(leaf);
            }
        }
    }
    let wanted = |id: &str| ids.iter().any(|i| i == id);
    let mut skipped: Vec<String> = Vec::new();

    let symbols: Vec<CopiedSymbol<'_>> = sch.symbols.iter().filter(|s| wanted(&s.id)).filter_map(|s| copied_symbol(input, s)).collect();
    let powers: Vec<&eda_model::ir::PowerSymbol> = sch.power_symbols.iter().filter(|p| wanted(&p.id)).collect();
    let wires: Vec<&eda_model::ir::Wire> = sch.wires.iter().filter(|w| wanted(&w.id) && w.pts.len() >= 2).collect();
    let labels: Vec<&eda_model::ir::NetLabel> = sch.labels.iter().filter(|l| wanted(&l.id)).collect();
    let texts: Vec<&eda_model::ir::SchematicText> = sch.texts.iter().filter(|t| wanted(&t.id)).collect();
    let no_connects: Vec<&eda_model::ir::NoConnect> = sch.no_connects.iter().filter(|n| wanted(&n.id)).collect();
    let bus_entries: Vec<&eda_model::ir::BusEntry> = sch.bus_entries.iter().filter(|b| wanted(&b.id)).collect();
    let junctions: Vec<&eda_model::ir::Junction> = sch.junctions.iter().filter(|j| wanted(&j.id)).collect();
    let lines: Vec<&eda_model::ir::SchLine> = sch.lines.iter().filter(|l| wanted(&l.id) && l.pts.len() >= 2).collect();
    let graphics: Vec<&eda_model::sch_extras::SchGraphic> = sch.extras.graphics.iter().filter(|g| wanted(&g.id)).collect();
    let sheets: Vec<&eda_model::ir::SheetInstance> = sch.sheets.iter().filter(|s| wanted(&s.id)).collect();
    let dropped_symbols = sch.symbols.iter().filter(|s| wanted(&s.id)).count() - symbols.len();
    if dropped_symbols > 0 {
        skipped.push(format!("{dropped_symbols} symbol(s) without a part to copy"));
    }

    let items = symbols.len() + powers.len() + wires.len() + labels.len() + texts.len() + no_connects.len() + bus_entries.len() + junctions.len() + lines.len() + graphics.len() + sheets.len();
    if items == 0 {
        return Err(vec![CheckResult::fail("clipboard_empty", "selection", "nothing in the selection can be copied")]);
    }

    let mut out = String::new();

    // `(lib_symbols ...)`: one entry per library symbol the selection names, by name.
    let mut cache: BTreeMap<String, LibrarySymbol> = BTreeMap::new();
    for c in &symbols {
        cache.entry(c.lib_id.clone()).or_insert_with(|| match input.design.symbol_library.as_ref().and_then(|l| l.flatten(&c.lib_id).ok().filter(|_| l.by_lib_id(&c.lib_id).is_some_and(|s| s.published))) {
            Some(rich) => rich, // the schematic caches the flat symbol (`LIB_SYMBOL::Flatten`)
            None => LibrarySymbol::from_engine_symbol(&c.engine),
        });
    }
    let mut power_engine: BTreeMap<String, LibSymbol> = BTreeMap::new();
    for p in &powers {
        let engine = input.model.symbol_of(&p.lib_id).or_else(|| eda_model::symbol::builtin(&p.lib_id));
        if let Some(engine) = engine {
            cache.entry(p.lib_id.clone()).or_insert_with(|| match input.design.symbol_library.as_ref().and_then(|l| l.flatten(&p.lib_id).ok().filter(|_| l.by_lib_id(&p.lib_id).is_some_and(|s| s.published))) {
                Some(rich) => rich,
                None => LibrarySymbol::from_engine_symbol(&engine),
            });
            power_engine.insert(p.lib_id.clone(), engine);
        }
    }
    // The symbols whose pins Swap Pins moved keep a copy of their own in the cache, one per placed symbol, named `<symbol>_<n>`, with the places traded (`SwapPinGeometry`): the
    // pasted symbol, here and in KiCad, has its pins where the copied one has them.
    let mut cache_names: BTreeMap<(String, u32), String> = BTreeMap::new();
    let mut cache_counts: BTreeMap<String, u32> = BTreeMap::new();
    for c in &symbols {
        let Some(swaps) = sch.extras.pin_swaps_of(&eda_model::ir::field_key(&c.sym.id, c.sym.unit)) else { continue };
        let Some(base) = cache.get(&c.lib_id).cloned() else { continue };
        let bare = c.lib_id.rsplit(':').next().unwrap_or(&c.lib_id).to_string();
        let n = cache_counts.entry(bare.clone()).or_insert(0);
        *n += 1;
        let name = format!("{bare}_{n}");
        cache.insert(name.clone(), swapped_copy(&base, c.sym.unit, swaps));
        cache_names.insert((c.sym.id.clone(), c.sym.unit), name);
    }
    if !cache.is_empty() {
        let _ = writeln!(out, "(lib_symbols");
        for (name, sym) in &cache {
            let mut block = String::new();
            crate::symbol_lib::write_symbol_named(&mut block, sym, name);
            // `write_symbol_named` writes at library-file depth; the cache is one level deeper in a file, but the indentation is cosmetic.
            out.push_str(&block);
        }
        let _ = writeln!(out, ")");
    }

    for c in &symbols {
        write_symbol(&mut out, input, c, cache_names.get(&(c.sym.id.clone(), c.sym.unit)).map(String::as_str));
    }
    for p in &powers {
        match power_engine.get(&p.lib_id) {
            Some(engine) => write_power_symbol(&mut out, input, p, engine),
            None => skipped.push(format!("power symbol {} (no library symbol {})", p.id, p.lib_id)),
        }
    }

    // Wires and buses: a polyline is one `(wire ...)` per segment, KiCad has no other.
    for w in &wires {
        let tag = if w.bus { "bus" } else { "wire" };
        for (j, pair) in w.pts.windows(2).enumerate() {
            let uuid = duid_plain(&format!("clipboard:{tag}:{}:{j}:{},{}:{},{}", w.id, pair[0].x, pair[0].y, pair[1].x, pair[1].y));
            note_item_uuid(&w.id, &uuid);
            let _ = writeln!(out, "\t({tag}\n\t\t(pts (xy {} {}) (xy {} {}))\n\t\t(stroke (width 0) (type default))\n\t\t(uuid \"{uuid}\")\n\t)", mm(pair[0].x), mm(pair[0].y), mm(pair[1].x), mm(pair[1].y));
        }
    }
    for b in &bus_entries {
        let uuid = duid_plain(&format!("clipboard:bus_entry:{}:{}", b.at.x, b.at.y));
        note_item_uuid(&b.id, &uuid);
        let _ = writeln!(out, "\t(bus_entry\n\t\t(at {} {})\n\t\t(size {} {})\n\t\t(stroke (width 0) (type default))\n\t\t(uuid \"{uuid}\")\n\t)", mm(b.at.x), mm(b.at.y), mm(b.size.x), mm(b.size.y));
    }

    // Junctions: the selected ones, and the dots the selected wires imply (KiCad has them as items of their own).
    let mut dots: Vec<Point> = junctions.iter().map(|j| j.at).collect();
    let owned: Vec<eda_model::ir::Wire> = wires.iter().map(|w| (*w).clone()).collect();
    dots.extend(eda_engine::geometry::wire_junction_points(&owned).into_iter().map(|(_, p)| p));
    dots.sort();
    dots.dedup();
    for at in &dots {
        let uuid = duid_plain(&format!("clipboard:junction:{}:{}", at.x, at.y));
        for j in junctions.iter().filter(|j| j.at == *at) {
            note_item_uuid(&j.id, &uuid);
        }
        let _ = writeln!(out, "\t(junction\n\t\t(at {} {})\n\t\t(diameter 0)\n\t\t(color 0 0 0 0)\n\t\t(uuid \"{uuid}\")\n\t)", mm(at.x), mm(at.y));
    }

    for n in &no_connects {
        let uuid = duid_plain(&format!("clipboard:no_connect:{}:{}", n.at.x, n.at.y));
        note_item_uuid(&n.id, &uuid);
        let _ = writeln!(out, "\t(no_connect\n\t\t(at {} {})\n\t\t(uuid \"{uuid}\")\n\t)", mm(n.at.x), mm(n.at.y));
    }

    // Graphic lines on the notes layer.
    for l in &lines {
        let pts: String = l.pts.iter().map(|p| format!(" (xy {} {})", mm(p.x), mm(p.y))).collect();
        let uuid = duid_plain(&format!("clipboard:line:{pts}"));
        note_item_uuid(&l.id, &uuid);
        let _ = writeln!(out, "\t(polyline\n\t\t(pts{pts})\n\t\t(stroke (width {}) (type default))\n\t\t(uuid \"{uuid}\")\n\t)", mm(l.width_nm));
    }

    // Shapes, text boxes, rule areas, directive labels: written by the same code that writes a file's.
    if !graphics.is_empty() {
        let mut mini = SchematicSection::default();
        mini.extras.graphics = graphics.iter().map(|g| (*g).clone()).collect();
        crate::sch_extras_io::write_graphics(&mut out, &mini);
    }

    for l in &labels {
        let (tag, shape) = match &l.kind {
            LabelKind::Local => ("label", None),
            LabelKind::Global { shape } => ("global_label", Some(*shape)),
            LabelKind::Hierarchical { shape } => ("hierarchical_label", Some(*shape)),
        };
        let (x, y) = (mm(l.at.x), mm(l.at.y));
        let uuid = duid_plain(&format!("clipboard:label:{}:{}:{}", l.net, l.at.x, l.at.y));
        note_item_uuid(&l.id, &uuid);
        let _ = write!(out, "\t({tag} {}", sexpr_str(&l.net));
        if let Some(shape) = shape {
            let _ = write!(out, "\n\t\t(shape {})", label_shape_token(shape));
        }
        // the label's spin, size, bold and italic go on the clipboard (`SCH_IO_KICAD_SEXPR::saveText`)
        let (angle, effects) = crate::label_effects(sch, l, tag == "label");
        let _ = writeln!(out, "\n\t\t(at {x} {y} {angle})");
        if tag == "global_label" || sch.extras.label_fields.get(&l.id).is_some_and(|f| !f.is_empty()) {
            let _ = writeln!(out, "\t\t(fields_autoplaced yes)");
        }
        let _ = writeln!(out, "\t\t{effects}");
        let _ = writeln!(out, "\t\t(uuid \"{uuid}\")");
        // its fields: a global label's Sheet References and the label's own (`saveField`)
        crate::label_fields::write_label_fields(&mut out, sch, l, false);
        let _ = writeln!(out, "\t)");
    }
    for t in &texts {
        let (x, y) = (mm(t.at.x), mm(t.at.y));
        let uuid = duid_plain(&format!("clipboard:text:{}:{}:{}", t.content, t.at.x, t.at.y));
        note_item_uuid(&t.id, &uuid);
        // upright angle and the justification, as the file writes them (`KeepUpright`, `EDA_TEXT::Format`)
        let angle = if eda_model::kicad_geom::keep_upright_is_vertical(t.angle as f64 / 1000.0) { 90.0 } else { 0.0 };
        let effects = crate::sch_extras_io::text_effects_str(t.size_nm, sch.extras.text_justify_of(&t.id));
        let _ = writeln!(out, "\t(text {}\n\t\t(exclude_from_sim no)\n\t\t(at {x} {y} {})\n\t\t{effects}\n\t\t(uuid \"{uuid}\")\n\t)", sexpr_str(&t.content), fmt_mm_f(angle));
    }

    // Hierarchical sheets: the sheet symbols, with their pins; their screens are not in the text.
    for s in &sheets {
        crate::write_sheet_item(&mut out, sch, s, if s.page.is_empty() { "1" } else { &s.page }, "/");
    }
    let mut sheet_files: Vec<String> = sheets.iter().map(|s| s.file.clone()).collect();
    sheet_files.sort();
    sheet_files.dedup();

    // The groups: each selected group, and the groups below it, once, naming the uuids the items were written as (`saveGroup`).
    if !copied_groups.is_empty() {
        let written = UuidScope::snapshot();
        let groups: Vec<&eda_model::ir::Group> = sch.extras.groups.iter().filter(|g| copied_groups.contains(&g.id)).collect();
        let uuid_of = |g: &eda_model::ir::Group| duid_plain(&format!("clipboard:group:{}", g.id));
        crate::pcb_items::write_groups(&mut out, &groups, &written, &uuid_of, &|_| false);
    }

    Ok(CopyOutput { text: out, items, skipped, sheet_files })
}

/// A clipboard fragment ([`write_clipboard`]'s text) as the schematic file a design block keeps: `SCH_EDIT_FRAME::SaveSelectionAsDesignBlock` puts the selection on a
/// screen of its own and saves that (`saveSchematicFile`), so the file is a whole schematic -- the header a file has, the fragment's `(lib_symbols ...)` and items, the page
/// the screen is on and the fonts line. The symbols of a fragment name their sheet path as `""` (a paste puts them on the sheet it lands on); in a file of their own they are on the
/// root, `/` and the file's uuid.
pub fn fragment_as_sheet_file(fragment: &str) -> String {
    let uuid = duid_plain("design-block:selection-file");
    let body = fragment.replace("(path \"\"", &format!("(path \"/{uuid}\""));
    format!("(kicad_sch\n\t(version 20250114)\n\t(generator \"eda-kicad\")\n\t(generator_version \"9.0\")\n\t(uuid \"{uuid}\")\n\t(paper \"A4\")\n{body}\n\t(sheet_instances\n\t\t(path \"/\" (page \"1\"))\n\t)\n\t(embedded_fonts no)\n)\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fragment the way real KiCad writes one for a resistor, a wire and a label (`SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION* )`).
    const KICAD_FRAGMENT: &str = r#"(lib_symbols
	(symbol "Device:R"
		(pin_numbers
			(hide yes)
		)
		(pin_names
			(offset 0)
		)
		(exclude_from_sim no)
		(in_bom yes)
		(on_board yes)
		(property "Reference" "R"
			(at 2.032 0 90)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(property "Value" "R"
			(at 0 0 90)
			(effects
				(font
					(size 1.27 1.27)
				)
			)
		)
		(symbol "R_0_1"
			(rectangle
				(start -1.016 -2.54)
				(end 1.016 2.54)
				(stroke
					(width 0.254)
					(type default)
				)
				(fill
					(type none)
				)
			)
		)
		(symbol "R_1_1"
			(pin passive line
				(at 0 3.81 270)
				(length 1.27)
				(name "~"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "1"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
			(pin passive line
				(at 0 -3.81 90)
				(length 1.27)
				(name "~"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
				(number "2"
					(effects
						(font
							(size 1.27 1.27)
						)
					)
				)
			)
		)
		(embedded_fonts no)
	)
)
(symbol
	(lib_id "Device:R")
	(at 100.33 50.8 90)
	(unit 1)
	(body_style 1)
	(exclude_from_sim no)
	(in_bom yes)
	(on_board yes)
	(in_pos_files yes)
	(dnp no)
	(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0001")
	(property "Reference" "R1"
		(at 100.33 47.2 90)
		(effects
			(font
				(size 1.27 1.27)
			)
		)
	)
	(property "Value" "10k"
		(at 100.33 54.4 90)
		(effects
			(font
				(size 1.27 1.27)
			)
		)
	)
	(property "Footprint" "Resistor_SMD:R_0603_1608Metric"
		(at 98.552 50.8 90)
		(effects
			(font
				(size 1.27 1.27)
			)
			(hide yes)
		)
	)
	(property "Datasheet" "~"
		(at 100.33 50.8 0)
		(effects
			(font
				(size 1.27 1.27)
			)
			(hide yes)
		)
	)
	(property "MPN" "RC0603FR-0710KL"
		(at 100.33 50.8 0)
		(effects
			(font
				(size 1.27 1.27)
			)
			(hide yes)
		)
	)
	(pin "1"
		(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0002")
	)
	(pin "2"
		(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0003")
	)
	(instances
		(project "demo"
			(path ""
				(reference "R1")
				(unit 1)
			)
		)
	)
)
(wire
	(pts
		(xy 100.33 54.61) (xy 120.65 54.61)
	)
	(stroke
		(width 0)
		(type default)
	)
	(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0004")
)
(junction
	(at 120.65 54.61)
	(diameter 0)
	(color 0 0 0 0)
	(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0005")
)
(label "SDA"
	(at 120.65 54.61 0)
	(effects
		(font
			(size 1.27 1.27)
		)
		(justify left bottom)
	)
	(uuid "6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0006")
)
"#;

    #[test]
    fn a_fragment_copied_by_kicad_reads() {
        let f = parse_clipboard(KICAD_FRAGMENT).expect("a KiCad fragment parses");
        assert_eq!(f.section.symbols.len(), 1);
        let r = &f.section.symbols[0];
        assert_eq!((r.id.as_str(), r.lib_id.as_str(), r.value.as_str(), r.unit), ("R1", "Device:R", "10k", 1));
        assert_eq!(r.footprint, "Resistor_SMD:R_0603_1608Metric");
        assert_eq!((r.at.x, r.at.y, r.rot), (100_330, 50_800, 90_000), "KiCad's own origin and angle");
        assert_eq!(f.section.wires.len(), 1);
        assert_eq!((f.section.wires[0].pts[0].x, f.section.wires[0].pts[1].x), (100_330, 120_650));
        assert_eq!(f.section.junctions.len(), 1);
        assert_eq!(f.section.labels.len(), 1);
        assert_eq!(f.section.labels[0].net, "SDA");
        assert_eq!(f.lib_symbols.len(), 1);
        assert_eq!(f.lib_symbols[0].lib_id, "Device:R", "the cache names the symbol by its full lib_id");
        assert_eq!(f.lib_symbols[0].pins.len(), 2);
        assert!(f.lib_symbols[0].pin_numbers_hidden && !f.lib_symbols[0].pin_names_hidden);
        assert_eq!(f.section.user_fields.get("R1").and_then(|m| m.get("MPN")).map(String::as_str), Some("RC0603FR-0710KL"), "custom fields come across");
        assert!(f.section.imported_from_kicad && f.notes.is_empty(), "{:?}", f.notes);
    }

    #[test]
    fn segments_that_meet_at_a_plain_bend_are_one_polyline_again_and_anything_else_stays_apart() {
        let wire = |a: (f64, f64), b: (f64, f64)| format!("(wire (pts (xy {} {}) (xy {} {})) (stroke (width 0) (type default)) (uuid \"6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0004\"))\n", a.0, a.1, b.0, b.1);
        // A three-point polyline written as two wires in order; a lone wire; two wires that meet at a point with a junction on it.
        let text = [wire((0.0, 0.0), (10.0, 0.0)), wire((10.0, 0.0), (10.0, 5.0)), wire((30.0, 0.0), (40.0, 0.0)), wire((40.0, 0.0), (40.0, 9.0)), "(junction (at 40 0) (diameter 0) (color 0 0 0 0) (uuid \"6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0005\"))\n".to_string()].concat();
        let f = parse_clipboard(&text).unwrap();
        let mut polylines: Vec<Vec<(i64, i64)>> = f.section.wires.iter().map(|w| w.pts.iter().map(|p| (p.x, p.y)).collect()).collect();
        polylines.sort();
        assert_eq!(polylines, vec![vec![(0, 0), (10_000, 0), (10_000, 5_000)], vec![(30_000, 0), (40_000, 0)], vec![(40_000, 0), (40_000, 9_000)]]);
        // Written in the other order the polyline is the same, from whichever end its first written segment starts.
        let backwards = [wire((10.0, 0.0), (10.0, 5.0)), wire((0.0, 0.0), (10.0, 0.0))].concat();
        let f = parse_clipboard(&backwards).unwrap();
        assert_eq!(f.section.wires.len(), 1);
        assert_eq!(f.section.wires[0].pts.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(), vec![(0, 0), (10_000, 0), (10_000, 5_000)]);
    }

    /// An image copied in KiCad is `(image ..)` in the fragment, and pastes as an image (it used to be left out with "images are not pasted").
    #[test]
    fn an_image_in_a_fragment_is_pasted_as_an_image() {
        let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(30, 20, [5, 6, 7], 300));
        let text = format!("(image (at 50.8 38.1) (scale 2) (uuid \"6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0009\") (data \"{}\" \"{}\"))\n", &data[..40], &data[40..]);
        let f = parse_clipboard(&text).expect("a fragment with an image parses");
        assert!(f.notes.iter().all(|n| !n.contains("images")), "{:?}", f.notes);
        let images: Vec<_> = f.section.extras.graphics.iter().filter_map(|g| if let eda_model::sch_extras::SchGraphicKind::Image { at, scale_ppm, data } = &g.shape { Some((*at, *scale_ppm, data.clone())) } else { None }).collect();
        assert_eq!(images, vec![(Point { x: 50_800, y: 38_100 }, 2_000_000, data)]);
    }

    #[test]
    fn text_that_is_not_a_fragment_is_refused() {
        assert!(parse_clipboard("just some words").is_err());
        assert!(parse_clipboard("(kicad_sch (version 20250114))").is_err(), "a whole file is not a copyable fragment");
        assert!(parse_clipboard("(footprint \"R_0603\" (layer \"F.Cu\"))").is_err(), "a board fragment is not a schematic one");
        assert!(parse_clipboard("(wire (pts (xy 0 0)").is_err(), "unbalanced");
        assert!(parse_clipboard("(wire (pts (xy 0 0) (xy 1 0)) (stroke (width 0) (type default)) (uuid \"6a2e1b0c-5a3a-4a41-9d7e-0d5e5c2e0004\"))").is_ok());
    }
}
