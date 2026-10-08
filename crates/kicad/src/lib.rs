//! eda-kicad — exports `Design::schematic` to KiCad 9 `.kicad_sch` text.
//!
//! Hand-rolled s-expression emitter (no `pcb-sexpr`/`pcb-kicad-sch`: the
//! format is small enough, and this way there's no external dependency on
//! an unreleased crate or a pinned git rev whose API might not fit our
//! integer-um `Design` model). Every symbol instance gets its own
//! `lib_symbol` (one instance == one reference designator == one part, in
//! v1), whose geometry is *pre-baked*: rotation/mirroring is applied once
//! by us (matching `eda_engine::geometry`/`eda_render`'s transform exactly)
//! to produce local, unrotated-in-KiCad-space points, and the KiCad
//! `(symbol ... (at x y 0))` instance itself is placed with rotation 0 and
//! no mirror. This sidesteps needing to replicate KiCad's own
//! rotate/mirror semantics bit-for-bit while still landing pins at exactly
//! our port points.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_layout::{Port, Side};
use eda_model::ir::{Design, NetLabel, NoConnect, PowerSymbol, SchematicText, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part, PinKind};

mod pcb;
pub use pcb::{custom_erc_pin_map, export_kicad_pcb, export_kicad_pcb_mapped, export_kicad_pro, export_kicad_pro_for};

mod sexpr;
mod page;
mod import;
pub use import::{import_kicad_pcb, merge_project_design_rules, merge_project_net_classes, merge_project_rule_severities, mm_to_um, parse_project_net_classes, parse_rule_severities, ImportNotes};

mod custom_rules;
pub use custom_rules::{merge_custom_rules, parse_custom_rules};

mod footprint_import;
pub use footprint_import::{parse_library_footprint, ParsedFootprint};

mod footprint_lib;
pub use footprint_lib::{default_footprint_library_root, export_kicad_mod, find_footprint_file, parse_footprint_file, resolve_library_footprints, LIBRARY_ROOT_ENV};

mod symbol_import;
pub use symbol_import::{parse_library_symbols, ParsedSymbols};

mod symbol_lib;
pub use symbol_lib::{default_symbol_library_root, export_kicad_sym, export_kicad_sym_library, find_symbol_library_file, list_symbol_libraries, list_symbols_in_library, resolve_library_symbols, resolve_symbol, SYMBOL_LIBRARY_ROOT_ENV};

mod bus;
pub use bus::expand_bus_members;

mod sch_extras_io;
mod sch_import;
pub use sch_import::{import_kicad_sch, import_kicad_sch_tree, pin_kind_from_electrical_type, reconcile, transform_local_point};

const STUB_MM: f64 = 1.27;

/// Fixed provenance for the title block. Passed explicitly (never system
/// time) so exports are byte-deterministic given the same inputs.
#[derive(Debug, Clone)]
pub struct ExportMeta<'a> {
    pub date: &'a str,
    pub title: &'a str,
}

/// [`export_kicad_sch`] plus every exported item's KiCad uuid -> our id
/// (symbol ref, `REF.PIN`, power symbol, wire, label, no-connect, text) --
/// how a kicad-cli ERC report is pointed back at `design.json`.
pub fn export_kicad_sch_mapped(design: &Design, model: &ConstraintModel, meta: &ExportMeta) -> Result<(String, std::collections::HashMap<String, String>), Vec<CheckResult>> {
    start_uuid_map();
    let r = export_kicad_sch(design, model, meta);
    let map = take_uuid_map();
    r.map(|s| (s, map))
}

pub fn export_kicad_sch(
    design: &Design,
    model: &ConstraintModel,
    meta: &ExportMeta,
) -> Result<String, Vec<CheckResult>> {
    let Some(sch) = &design.schematic else {
        return Err(vec![CheckResult::fail("kicad.no_schematic", "design", "design has no schematic section")]);
    };

    let mut errors = Vec::new();
    let parts_by_ref: BTreeMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();

    let mut symbols: Vec<&SymbolInstance> = sch.symbols.iter().collect();
    // (id, unit): several instances now legitimately share one id (a
    // multi-unit part's placed units), so a secondary key is needed for a
    // deterministic order between them.
    symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
    for sym in &symbols {
        if !parts_by_ref.contains_key(sym.id.as_str()) {
            errors.push(CheckResult::fail("kicad.unknown_part", sym.id.clone(), "symbol id has no matching part in the constraint model"));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut wires: Vec<&Wire> = sch.wires.iter().collect();
    wires.sort_by(|a, b| (&a.net, &a.pts).cmp(&(&b.net, &b.pts)));

    let mut labels: Vec<&NetLabel> = sch.labels.iter().collect();
    labels.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));

    let mut power_symbols: Vec<&PowerSymbol> = sch.power_symbols.iter().collect();
    power_symbols.sort_by(|a, b| a.id.cmp(&b.id));

    let mut no_connects: Vec<&NoConnect> = sch.no_connects.iter().collect();
    no_connects.sort_by(|a, b| a.at.cmp(&b.at));

    let mut texts: Vec<&SchematicText> = sch.texts.iter().collect();
    texts.sort_by(|a, b| (&a.content, a.at).cmp(&(&b.content, b.at)));

    // ---- resolve every distinct lib_id used, once, into a `lib_symbols`-block entry ----
    let mut lib_ids: Vec<String> = symbols.iter().map(|s| sym_lib_id(s)).collect();
    lib_ids.extend(power_symbols.iter().map(|p| p.lib_id.clone()));
    lib_ids.sort();
    lib_ids.dedup();

    let mut out = String::new();

    // ---- header ----
    let sheet_uuid = duid("sheet:/");
    writeln!(out, "(kicad_sch").unwrap();
    writeln!(out, "\t(version 20250114)").unwrap();
    writeln!(out, "\t(generator \"eda-kicad\")").unwrap();
    writeln!(out, "\t(generator_version \"9.0\")").unwrap();
    writeln!(out, "\t(uuid \"{sheet_uuid}\")").unwrap();
    // The sheet's paper (Page Settings); A4 landscape when it never set one.
    writeln!(out, "\t{}", sch.extras.page.as_ref().map(|p| p.to_sexpr()).unwrap_or_else(|| "(paper \"A4\")".to_string())).unwrap();
    writeln!(out, "\t(title_block").unwrap();
    let tb = sch.title_block.as_ref();
    let title = tb.map(|t| t.title.as_str()).filter(|s| !s.is_empty()).unwrap_or(meta.title);
    let date = tb.map(|t| t.date.as_str()).filter(|s| !s.is_empty()).unwrap_or(meta.date);
    writeln!(out, "\t\t(title {})", sexpr_str(title)).unwrap();
    writeln!(out, "\t\t(date {})", sexpr_str(date)).unwrap();
    if let Some(t) = tb.filter(|t| !t.rev.is_empty()) {
        writeln!(out, "\t\t(rev {})", sexpr_str(&t.rev)).unwrap();
    }
    if let Some(t) = tb.filter(|t| !t.company.is_empty()) {
        writeln!(out, "\t\t(company {})", sexpr_str(&t.company)).unwrap();
    }
    if let Some(t) = tb {
        // A title block the user (or an imported file) has: its own comments, and none when it has none (Page Settings can clear them).
        for (i, c) in t.comments.iter().enumerate() {
            writeln!(out, "\t\t(comment {} {})", i + 1, sexpr_str(c)).unwrap();
        }
    } else {
        writeln!(out, "\t\t(comment 1 {})", sexpr_str(&format!("engine_version: {}", design.provenance.engine_version))).unwrap();
        writeln!(out, "\t\t(comment 2 {})", sexpr_str(&format!("intent_hash: {}", design.provenance.intent_hash))).unwrap();
        writeln!(out, "\t\t(comment 3 {})", sexpr_str(&format!("seed: {}", design.provenance.seed))).unwrap();
    }
    writeln!(out, "\t)").unwrap();

    // ---- lib_symbols ----
    writeln!(out, "\t(lib_symbols").unwrap();
    for lib_id in &lib_ids {
        if let Some((part, representative)) = symbols.iter().find_map(|s| (sym_lib_id(s) == *lib_id).then(|| (parts_by_ref[s.id.as_str()], *s))) {
            // Every placed unit of *this one reference* (not just the first
            // instance this lib_id happened to match) -- a multi-unit part
            // embeds all of them as sibling `_<unit>_1` sub-blocks of the
            // same `lib_symbols` entry, see `write_regular_lib_symbol`'s own
            // doc. Two different references sharing one real `lib_id`
            // (`CIN`/`COUT` both `Device:C`) still collapse onto this same
            // representative's own transform, the pre-existing simplification
            // this exporter has always made for that case (see `bare_name`'s
            // own doc below) -- multi-unit does not make that any worse.
            let instances: Vec<&SymbolInstance> = symbols.iter().filter(|s| s.id == representative.id).copied().collect();
            write_regular_lib_symbol(&mut out, lib_id, &instances, part, model);
        } else {
            // A power symbol (never a bare part): `model.symbol_of` always
            // resolves it, real library first, then `symbol::builtin`'s
            // generic rail fallback for any net name.
            let resolved = model.symbol_of(lib_id).unwrap_or_else(|| eda_model::symbol::builtin(lib_id).expect("power lib_id always resolves"));
            write_power_lib_symbol(&mut out, lib_id, &resolved);
        }
    }
    writeln!(out, "\t)").unwrap();

    // ---- symbol instances ----
    // KiCad 9 nests the sheet-path -> reference mapping *inside* each
    // symbol (an `instances` block), not in a separate top-level
    // `symbol_instances` list — that older shape parses as an unknown
    // token here and kicad-cli refuses to load the file.
    for sym in &symbols {
        let part = parts_by_ref[sym.id.as_str()];
        let lib_id = sym_lib_id(sym);
        let resolved = if eda_model::is_synthetic_lib_id(&lib_id) { None } else { model.symbol_of(&lib_id) };
        let x = mm(sym.at.x);
        let y = mm(sym.at.y);
        let uuid = duid_for(&format!("sym:{}", sym.id), &sym.id);
        writeln!(out, "\t(symbol (lib_id {}) (at {x} {y} 0) (unit {})", sexpr_str(&lib_id), sym.unit).unwrap();
        // The attributes `SCH_EDIT_TOOL::SetAttribute` toggles (Do not Populate, Exclude from BOM / Board / Simulation): kicad-cli's BOM, netlist and ERC read them from here.
        let yes_no = |b: bool| if b { "yes" } else { "no" };
        writeln!(
            out,
            "\t\t(exclude_from_sim {}) (in_bom {}) (on_board {}) (dnp {})",
            yes_no(sym.exclude_from_sim),
            yes_no(!sym.exclude_from_bom),
            yes_no(!sym.exclude_from_board),
            yes_no(sym.dnp)
        )
        .unwrap();
        if sch.extras.is_locked(&sym.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        let value = if !sym.value.is_empty() { sym.value.as_str() } else { part.value.as_deref().unwrap_or(&sym.id) };
        let footprint = if !sym.footprint.is_empty() { sym.footprint.as_str() } else { part.footprint.as_deref().unwrap_or("") };
        let datasheet = if !sym.datasheet.is_empty() {
            sym.datasheet.as_str()
        } else {
            resolved.as_ref().map(|s| s.datasheet.as_str()).filter(|s| !s.is_empty()).unwrap_or("")
        };
        write_property(&mut out, "Reference", &sym.id, 0.0, -2.0, false);
        write_property(&mut out, "Value", value, 0.0, 2.0, false);
        write_property(&mut out, "Footprint", footprint, 0.0, 4.0, true);
        write_property(&mut out, "Datasheet", datasheet, 0.0, 6.0, true);
        // Only this instance's own unit's pins (plus any `unit == 0`
        // common-to-every-unit pin) -- a multi-unit instance's pin-uuid
        // list must not claim pins that are actually drawn on a sibling
        // placed unit elsewhere on the sheet. A single-unit part (no real
        // symbol resolved, or one with exactly one unit) has every pin
        // match trivially, same list as before this field existed.
        for pin in part.pins.iter().filter(|pin| resolved.as_ref().and_then(|s| s.pin_by_number(&pin.number)).is_none_or(|p| p.unit == 0 || p.unit == sym.unit)) {
            let pin_uuid = duid_for(&format!("pin:{}:{}", sym.id, pin.number), &format!("{}.{}", sym.id, pin.number));
            writeln!(out, "\t\t(pin {} (uuid \"{pin_uuid}\"))", sexpr_str(&pin.number)).unwrap();
        }
        writeln!(out, "\t\t(instances").unwrap();
        writeln!(out, "\t\t\t(project \"eda-kicad\"").unwrap();
        writeln!(out, "\t\t\t\t(path \"/{sheet_uuid}\"").unwrap();
        writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&sym.id)).unwrap();
        writeln!(out, "\t\t\t\t\t(unit {})", sym.unit).unwrap();
        writeln!(out, "\t\t\t\t)").unwrap();
        writeln!(out, "\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- power symbol instances ----
    for ps in &power_symbols {
        let x = mm(ps.at.x);
        let y = mm(ps.at.y);
        let uuid = duid_for(&format!("pwr:{}", ps.id), &ps.id);
        writeln!(out, "\t(symbol (lib_id {}) (at {x} {y} 0) (unit 1)", sexpr_str(&ps.lib_id)).unwrap();
        writeln!(out, "\t\t(exclude_from_sim no) (in_bom no) (on_board no) (dnp no)").unwrap();
        if sch.extras.is_locked(&ps.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        write_property(&mut out, "Reference", &ps.id, 0.0, -2.0, true);
        write_property(&mut out, "Value", &ps.net, 0.0, 2.0, false);
        write_property(&mut out, "Footprint", "", 0.0, 0.0, true);
        write_property(&mut out, "Datasheet", "", 0.0, 0.0, true);
        let pin_uuid = duid_for(&format!("pwrpin:{}", ps.id), &ps.id);
        writeln!(out, "\t\t(pin \"1\" (uuid \"{pin_uuid}\"))").unwrap();
        writeln!(out, "\t\t(instances").unwrap();
        writeln!(out, "\t\t\t(project \"eda-kicad\"").unwrap();
        writeln!(out, "\t\t\t\t(path \"/{sheet_uuid}\"").unwrap();
        writeln!(out, "\t\t\t\t\t(reference {})", sexpr_str(&ps.id)).unwrap();
        writeln!(out, "\t\t\t\t\t(unit 1)").unwrap();
        writeln!(out, "\t\t\t\t)").unwrap();
        writeln!(out, "\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- wires and bus wires (GAPS.md #20 -- `SCH_LINE`'s own `LAYER_WIRE`
    // vs `LAYER_BUS`, the same shape either way) -- each polyline segment
    // as one KiCad wire/bus ----
    for (i, w) in wires.iter().enumerate() {
        let tag = if w.bus { "bus" } else { "wire" };
        for (j, pair) in w.pts.windows(2).enumerate() {
            let x1 = mm(pair[0].x);
            let y1 = mm(pair[0].y);
            let x2 = mm(pair[1].x);
            let y2 = mm(pair[1].y);
            let uuid = duid_for(&format!("{tag}:{}:{}:{}", w.net, i, j), &w.id);
            writeln!(out, "\t({tag}").unwrap();
            writeln!(out, "\t\t(pts (xy {x1} {y1}) (xy {x2} {y2}))").unwrap();
            writeln!(out, "\t\t(stroke (width 0) (type default))").unwrap();
            writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
            if sch.extras.is_locked(&w.id) {
                writeln!(out, "\t\t(locked yes)").unwrap();
            }
            writeln!(out, "\t)").unwrap();
        }
    }

    // ---- bus entries (GAPS.md #20) ----
    let mut bus_entries: Vec<&eda_model::ir::BusEntry> = sch.bus_entries.iter().collect();
    bus_entries.sort_by(|a, b| a.at.cmp(&b.at));
    for be in &bus_entries {
        let x = mm(be.at.x);
        let y = mm(be.at.y);
        let dx = mm(be.size.x);
        let dy = mm(be.size.y);
        let uuid = if be.id.is_empty() { duid(&format!("bent:{}:{}", be.at.x, be.at.y)) } else { be.id.clone() };
        writeln!(out, "\t(bus_entry (at {x} {y}) (size {dx} {dy})").unwrap();
        writeln!(out, "\t\t(stroke (width 0) (type default))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&be.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }

    // ---- bus aliases (GAPS.md #20) -- see `BusAlias`'s own doc for why
    // this legacy per-screen block (not a `.kicad_pro` project file this
    // project has no writer for) is where this project round-trips them.
    // `export_kicad_sch` has no way to tell "am I the root of a tree
    // export" from its own arguments alone (`export_kicad_sch_tree` clones
    // the *whole* design, `bus_aliases` included, into every child screen's
    // own call) -- rather than invent one, every screen's own file just
    // carries the full project-wide list. Harmless duplication: real
    // project-wide *visibility* never depended on which screen's text
    // "owns" an alias, and `sch_import::import_kicad_sch_tree`'s own
    // by-name merge on the way back in already de-duplicates it.
    for alias in &design.bus_aliases {
        writeln!(out, "\t(bus_alias {}", sexpr_str(&alias.name)).unwrap();
        write!(out, "\t\t(members").unwrap();
        for m in &alias.members {
            write!(out, " {}", sexpr_str(m)).unwrap();
        }
        writeln!(out, ")").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- junctions: every point where 3+ same-net wire-segment endpoints
    // meet, *including* a T-junction with no shared vertex (one wire's
    // endpoint landing mid-span on another same-net wire's segment) --
    // `geometry::wire_junction_points` is the single definition
    // `eda_render`/`eda_gates` already hold every dot to, so drawing
    // anything narrower here (the old plain vertex-coincidence count) is
    // exactly what `schematic_missing_junction` exists to catch: a real
    // connection real KiCad accepts silently, left with no dot marking it.
    // Net-scoped (not just by point), so two different nets whose wires
    // happen to cross at the same coordinate never draw a false short.
    let derived_junctions = eda_engine::geometry::wire_junction_points(&sch.wires);
    for (net, pt) in &derived_junctions {
        let x = mm(pt.x);
        let y = mm(pt.y);
        let uuid = duid(&format!("junction:{net}:{}:{}", pt.x, pt.y));
        writeln!(out, "\t(junction (at {x} {y}) (diameter 0) (color 0 0 0 0)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        writeln!(out, "\t)").unwrap();
    }
    // Explicit junctions (`J`, `Junction`): a crossing joined on purpose. Skipped where a derived one above already marks the point.
    let mut explicit: Vec<&eda_model::ir::Junction> = sch.junctions.iter().filter(|j| !derived_junctions.iter().any(|(_, pt)| *pt == j.at)).collect();
    explicit.sort_by_key(|j| j.at);
    for j in explicit {
        let x = mm(j.at.x);
        let y = mm(j.at.y);
        let uuid = duid_for(&format!("junction:{}:{}", j.at.x, j.at.y), &j.id);
        writeln!(out, "\t(junction (at {x} {y}) (diameter 0) (color 0 0 0 0)").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&j.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }
    // Graphic lines on the notes layer (`I`, `SchLine`): `(polyline ...)`, decoration with no net.
    let mut lines: Vec<&eda_model::ir::SchLine> = sch.lines.iter().filter(|l| l.pts.len() >= 2).collect();
    lines.sort_by(|a, b| a.pts.cmp(&b.pts));
    for l in lines {
        let pts: String = l.pts.iter().map(|p| format!(" (xy {} {})", mm(p.x), mm(p.y))).collect();
        let width = mm(l.width_um);
        let uuid = duid_for(&format!("sch_line:{pts}"), &l.id);
        writeln!(out, "\t(polyline").unwrap();
        writeln!(out, "\t\t(pts{pts})").unwrap();
        writeln!(out, "\t\t(stroke (width {width}) (type default))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&l.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }

    // Shapes, text boxes, rule areas and directive labels (`SchGraphic`).
    sch_extras_io::write_graphics(&mut out, sch);

    // ---- no-connect flags ----
    for nc in &no_connects {
        let x = mm(nc.at.x);
        let y = mm(nc.at.y);
        let uuid = duid_for(&format!("nc:{}:{}", nc.at.x, nc.at.y), &nc.id);
        let locked = if sch.extras.is_locked(&nc.id) { " (locked yes)" } else { "" };
        writeln!(out, "\t(no_connect (at {x} {y}) (uuid \"{uuid}\"){locked})").unwrap();
    }

    // ---- labels: local, global or hierarchical, per `NetLabel::kind` ----
    for l in &labels {
        let x = mm(l.at.x);
        let y = mm(l.at.y);
        let uuid = duid_for(&format!("label:{}:{}:{}", l.net, l.at.x, l.at.y), &l.id);
        let (tag, shape) = match &l.kind {
            eda_model::ir::LabelKind::Local => ("label", None),
            eda_model::ir::LabelKind::Global { shape } => ("global_label", Some(*shape)),
            eda_model::ir::LabelKind::Hierarchical { shape } => ("hierarchical_label", Some(*shape)),
        };
        write!(out, "\t({tag} {}", sexpr_str(&l.net)).unwrap();
        if let Some(shape) = shape {
            write!(out, " (shape {})", label_shape_token(shape)).unwrap();
        }
        writeln!(out, " (at {x} {y} 0)").unwrap();
        writeln!(out, "\t\t(effects (font (size 1.27 1.27)) (justify left))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&l.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }

    // ---- free text (`T`) -- same shape as a label's own s-expr, minus the
    // net/shape fields a plain KiCad `(text ...)` has neither of ----
    for t in &texts {
        let x = mm(t.at.x);
        let y = mm(t.at.y);
        let angle_deg = t.angle as f64 / 1000.0;
        let size_mm = t.size_um as f64 / 1000.0;
        let uuid = duid_for(&format!("text:{}:{}:{}", t.content, t.at.x, t.at.y), &t.id);
        writeln!(out, "\t(text {}", sexpr_str(&t.content)).unwrap();
        writeln!(out, "\t\t(at {x} {y} {angle_deg})").unwrap();
        writeln!(out, "\t\t(effects (font (size {size_mm} {size_mm})))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&t.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        writeln!(out, "\t)").unwrap();
    }

    // ---- child hierarchical sheets (GAPS.md #6), with their own pins ----
    let mut sheets: Vec<&eda_model::ir::SheetInstance> = sch.sheets.iter().collect();
    sheets.sort_by(|a, b| (&a.file, a.at).cmp(&(&b.file, b.at)));
    for s in &sheets {
        let x = mm(s.at.x);
        let y = mm(s.at.y);
        let w = mm(s.size.0);
        let h = mm(s.size.1);
        let uuid = if s.id.is_empty() { duid(&format!("sheet:{}:{}", s.file, s.at.x)) } else { s.id.clone() };
        writeln!(out, "\t(sheet").unwrap();
        writeln!(out, "\t\t(at {x} {y}) (size {w} {h})").unwrap();
        writeln!(out, "\t\t(stroke (width 0.1524) (type solid))").unwrap();
        writeln!(out, "\t\t(fill (color 255 255 194 1.0000))").unwrap();
        writeln!(out, "\t\t(uuid \"{uuid}\")").unwrap();
        if sch.extras.is_locked(&s.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        let name_y = mm(s.at.y - 600);
        let file_y = mm(s.at.y + s.size.1 + 600);
        writeln!(out, "\t\t(property \"Sheetname\" {} (at {x} {name_y} 0) (effects (font (size 1.27 1.27))))", sexpr_str(&s.name)).unwrap();
        writeln!(out, "\t\t(property \"Sheetfile\" {} (at {x} {file_y} 0) (effects (font (size 1.27 1.27))))", sexpr_str(&s.file)).unwrap();
        for p in &s.pins {
            let px = mm(p.at.x);
            let py = mm(p.at.y);
            let pin_uuid = if p.id.is_empty() { duid(&format!("sheetpin:{uuid}:{}", p.name)) } else { p.id.clone() };
            writeln!(out, "\t\t(pin {} {}", sexpr_str(&p.name), label_shape_token(p.shape)).unwrap();
            writeln!(out, "\t\t\t(at {px} {py} 0)").unwrap();
            writeln!(out, "\t\t\t(uuid \"{pin_uuid}\")").unwrap();
            writeln!(out, "\t\t\t(effects (font (size 1.27 1.27)) (justify left)))").unwrap();
        }
        writeln!(out, "\t\t(instances").unwrap();
        writeln!(out, "\t\t\t(project \"eda-kicad\"").unwrap();
        let page = if s.page.is_empty() { "1" } else { s.page.as_str() };
        writeln!(out, "\t\t\t\t(path \"/{sheet_uuid}\" (page {}))", sexpr_str(page)).unwrap();
        writeln!(out, "\t\t\t)").unwrap();
        writeln!(out, "\t\t)").unwrap();
        writeln!(out, "\t)").unwrap();
    }

    // ---- sheet instances (required by KiCad 9 for a valid project-less sheet) ----
    writeln!(out, "\t(sheet_instances").unwrap();
    writeln!(out, "\t\t(path \"/\" (page \"1\"))").unwrap();
    writeln!(out, "\t)").unwrap();
    writeln!(out, "\t(embedded_fonts no)").unwrap();

    writeln!(out, ")").unwrap();
    Ok(out)
}

/// `export_kicad_sch`'s multi-file counterpart (GAPS.md #6): the root
/// (`root_filename`, `design.schematic`) plus one file per
/// `design.sheet_contents` entry, each written through the exact same
/// per-screen `export_kicad_sch` -- a screen's own `(sheet ...)` placements
/// (if it has grandchildren) are written by that same call, since
/// `sch.sheets` is read identically regardless of whether the caller is
/// the root or some other screen. Returns `(filename, text)` pairs rather
/// than doing any filesystem I/O itself, same convention `export_kicad_sch`
/// already set (the caller decides where/whether to write them -- see
/// `crate::sch_import::import_kicad_sch_tree`'s own doc for the inverse
/// direction's equivalent "caller resolves paths" choice).
pub fn export_kicad_sch_tree(design: &Design, model: &ConstraintModel, meta: &ExportMeta, root_filename: &str) -> Result<Vec<(String, String)>, Vec<CheckResult>> {
    let mut out = vec![(root_filename.to_string(), export_kicad_sch(design, model, meta)?)];
    if let Some(screens) = &design.sheet_contents {
        for (file, sch) in screens {
            let child_design = Design { schematic: Some(sch.clone()), sheet_contents: None, nets: None, ..design.clone() };
            let title = file.strip_suffix(".kicad_sch").unwrap_or(file);
            let child_meta = ExportMeta { date: meta.date, title };
            out.push((file.clone(), export_kicad_sch(&child_design, model, &child_meta)?));
        }
    }
    Ok(out)
}

/// A `SymbolInstance`'s lib_id, with the pre-`lib_id`-field
/// (`design.json` written before this port) fallback to the old synthetic
/// `"eda:<id>"` form.
fn sym_lib_id(sym: &SymbolInstance) -> String {
    if !sym.lib_id.is_empty() {
        sym.lib_id.clone()
    } else {
        format!("eda:{}", sym.id)
    }
}

fn label_shape_token(shape: eda_model::ir::LabelShape) -> &'static str {
    use eda_model::ir::LabelShape;
    match shape {
        LabelShape::Input => "input",
        LabelShape::Output => "output",
        LabelShape::Bidirectional => "bidirectional",
        LabelShape::TriState => "tri_state",
        LabelShape::Passive => "passive",
    }
}

fn write_property(out: &mut String, key: &str, value: &str, x: f64, y: f64, hide: bool) {
    if hide {
        writeln!(
            out,
            "\t\t(property {} {} (at {x} {y} 0)\n\t\t\t(effects (font (size 1.27 1.27)) (hide yes))\n\t\t)",
            sexpr_str(key),
            sexpr_str(value)
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "\t\t(property {} {} (at {x} {y} 0)\n\t\t\t(effects (font (size 1.27 1.27)))\n\t\t)",
            sexpr_str(key),
            sexpr_str(value)
        )
        .unwrap();
    }
}

/// Feeds a real library symbol's own local point (library frame, mm, +y
/// **up** -- [`eda_model::symbol::SPoint`]'s own convention) through
/// [`baked_local`]: first shifted into the same sheet-frame,
/// box-corner-relative local convention [`local_port_point`]/
/// [`local_stub_tip`] already use for the synthetic path, then given the
/// same rotation/mirror treatment, so a real pin/graphic composes correctly
/// if a rotated/mirrored instance is ever emitted (`derive_schematic` never
/// emits one today -- see this module's own doc comment).
///
/// `(x0, y1)` is the real symbol's own box corner in *library* frame --
/// [`eda_engine::geometry::real_symbol_bbox`]'s own `(x0, _, _, y1)`, the
/// same corner `node_size`/`build_ports` already measured this part's box
/// and every pin's `Port::offset` from. This instance's own `sym.at` *is*
/// that corner (not the library's `(0,0)` origin -- a real symbol's origin
/// is almost never its own box's top-left), so a real point has to be
/// shifted by it before it means anything relative to `sym.at`: `p.x - x0`
/// along library +x (untouched by the sheet's y-flip), `y1 - p.y` along
/// library +y (flipped, since library "up" is sheet "into the box" from the
/// top) -- the exact shift [`eda_engine::geometry::build_ports_from_real_symbol`]'s
/// own `body_x - x0` / `y1 - body_y` offsets already use, and
/// [`eda_engine::geometry::nc_pin_local_points`]'s real-symbol branch
/// already applies for a `nc`-kind pin. Skipping this shift (as an earlier
/// version of this function did, treating `p` as already box-corner
/// relative) draws a real pin exactly `(x0, y1)` away from wherever its own
/// wire/power-symbol stub actually terminates -- confirmed empirically:
/// `Device:C`'s box corner sits 2.54mm from its library origin, and
/// `CIN`/`COUT`'s power-flagged pin landed 2.54mm/3.81mm off its own
/// GND power symbol until this shift was added.
fn baked_real_point(sym: &SymbolInstance, width_um: f64, x0: f64, y1: f64, p: eda_model::symbol::SPoint) -> eda_model::symbol::SPoint {
    let (x, y) = baked_local(sym, width_um, (p.x - x0) * 1000.0, (y1 - p.y) * 1000.0);
    eda_model::symbol::SPoint::new(x, y)
}

/// A real pin's own `angle_deg`, composed with the instance's mirror the
/// same way [`baked_real_point`] composes position (rotation, never
/// non-zero today, is left for whoever adds rotated-instance support).
/// Mirroring is a flip across a vertical axis, so it swaps East/West
/// (0<->180) and leaves North/South (90/270) alone -- the standard
/// `angle -> 180 - angle` reflection.
fn baked_real_angle(sym: &SymbolInstance, angle_deg: f64) -> f64 {
    if sym.mirrored {
        (180.0 - angle_deg).rem_euclid(360.0)
    } else {
        angle_deg
    }
}

/// [`write_symbol_graphic`]'s input, but with every point run through
/// [`baked_real_point`]/[`baked_real_angle`] first -- i.e. the same
/// graphic, placed the way *this* instance's own rotation/mirror would
/// draw it, in the on-disk library-frame convention [`write_symbol_graphic`]
/// itself expects. `unit`/`stroke_mm`/`filled`/`size_mm`/`text` carry no
/// position and pass through untouched.
fn baked_graphic(g: &eda_model::SymbolGraphic, sym: &SymbolInstance, width_um: f64, x0: f64, y1: f64) -> eda_model::SymbolGraphic {
    use eda_model::SymbolGraphic::*;
    let p = |pt| baked_real_point(sym, width_um, x0, y1, pt);
    match *g {
        Rectangle { unit, start, end, stroke_mm, filled } => Rectangle { unit, start: p(start), end: p(end), stroke_mm, filled },
        Polyline { unit, ref pts, stroke_mm, filled } => Polyline { unit, pts: pts.iter().map(|&pt| p(pt)).collect(), stroke_mm, filled },
        Circle { unit, center, radius_mm, stroke_mm, filled } => Circle { unit, center: p(center), radius_mm, stroke_mm, filled },
        Arc { unit, start, mid, end, stroke_mm, filled } => Arc { unit, start: p(start), mid: p(mid), end: p(end), stroke_mm, filled },
        Text { unit, ref text, at, angle_deg, size_mm } => Text { unit, text: text.clone(), at: p(at), angle_deg: baked_real_angle(sym, angle_deg), size_mm },
    }
}

/// Embeds one part-backed symbol's `lib_symbol` definition: when a real
/// library symbol resolved for `lib_id`, its own graphics and pins, drawn
/// verbatim (baked through this instance's own rotation/mirror -- see
/// [`baked_graphic`]/[`baked_real_point`]) so an R/C/LED/connector/IC looks
/// exactly as real KiCad draws it; otherwise the synthetic generic box this
/// exporter has always drawn. Either way, `eda_engine::geometry`'s own
/// `node_size`/`build_ports` already derived this part's box/port geometry
/// from the *same* resolved symbol (see `write_schematic`'s own call),
/// so a wire's stub tip and this function's own drawn pin position can
/// never disagree about where a pin actually is.
/// `instances`: every placed `SymbolInstance` on the sheet that shares the
/// representative reference this `lib_id` was first seen on (always exactly
/// one for a single-unit part; several, one per placed unit, for a
/// multi-unit one -- see this function's own per-unit loop below).
fn write_regular_lib_symbol(out: &mut String, lib_id: &str, instances: &[&SymbolInstance], part: &Part, model: &ConstraintModel) {
    let resolved = model.real_symbol_of(lib_id, part);
    // The representative instance whose own rotation/mirror every graphic
    // and pin gets baked through (see `baked_graphic`/`baked_real_point`'s
    // own docs on why this exporter bakes at all): unit 1's own placed
    // instance when there is one (the common case -- unit 1 is almost
    // always placed), else simply the first instance this reference has.
    // Every *other* unit's own sub-block below prefers its own placed
    // instance's transform when one exists, falling back to this same
    // default only for a unit that is not placed anywhere (nothing to bake
    // through at all, so the representative's transform is as good a guess
    // as any -- that unit's embedded graphics are never actually shown
    // anywhere on this sheet regardless).
    let default_baker = instances.iter().find(|s| s.unit == 1).copied().unwrap_or(instances[0]);
    let unit_count = resolved.as_ref().map(|s| s.unit_count).unwrap_or(1);

    // KiCad requires a unit/body-style sub-symbol's own name to be
    // `<bare-name>_<unit>_<style>`, where `<bare-name>` is the *library*
    // symbol's own name (the part of `lib_id` after its `Library:` prefix)
    // -- never an instance's reference designator. This matters once a
    // real `lib_id` like `"Device:C"` is shared by more than one instance
    // (`CIN` and `COUT` both resolve to it): naming the sub-units after
    // whichever instance happened to trigger this block (`"CIN_0_1"`)
    // parses as a name/prefix mismatch and `kicad-cli` refuses to load the
    // file at all. Confirmed empirically against real `kicad-cli sch erc`.
    let bare_name = lib_id.rsplit(':').next().unwrap_or(lib_id);
    writeln!(out, "\t\t(symbol {}", sexpr_str(lib_id)).unwrap();
    writeln!(out, "\t\t\t(exclude_from_sim no) (in_bom yes) (on_board yes)").unwrap();
    write_property(out, "Reference", "U", 0.0, 0.0, false);
    write_property(out, "Value", bare_name, 0.0, 0.0, false);

    // ---- unit _0_1: the body common to every unit ----
    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{bare_name}_0_1"))).unwrap();
    match &resolved {
        Some(sym_data) => {
            // Strictly unit 0 ("every unit") here -- a graphic scoped to a
            // *specific* unit belongs in that unit's own `_<u>_1` sub-block
            // below instead, not doubled up here too (KiCad shows `_0_1`'s
            // contents underneath *every* unit's own body, so leaking a
            // unit-1-specific graphic in here would draw it under units
            // 2..N as well).
            let (width, _height) = eda_engine::geometry::node_size(part, resolved.as_ref(), 1);
            let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(sym_data, 1);
            for g in sym_data.graphics.iter().filter(|g| g.unit() == 0) {
                write_symbol_graphic(out, &baked_graphic(g, default_baker, width as f64, x0, y1));
            }
        }
        None => {
            let (width, height) = eda_engine::geometry::node_size(part, None, 1);
            let corners = [(0.0, 0.0), (width as f64, 0.0), (width as f64, height as f64), (0.0, height as f64), (0.0, 0.0)];
            write!(out, "\t\t\t\t(polyline\n\t\t\t\t\t(pts").unwrap();
            for (lx, ly) in corners {
                let (bx, by) = baked_local(default_baker, width as f64, lx, ly);
                write!(out, " (xy {} {})", fmt_mm_f(bx), fmt_mm_f(by)).unwrap();
            }
            writeln!(out, ")\n\t\t\t\t\t(stroke (width 0.254) (type default))\n\t\t\t\t\t(fill (type none))\n\t\t\t\t)").unwrap();
        }
    }
    writeln!(out, "\t\t\t)").unwrap();

    // ---- unit _<u>_1, u = 1..=unit_count: each unit's own body + pins ----
    // A single-unit part (`unit_count == 1`, the overwhelming common case)
    // runs this loop exactly once, over exactly the same pins/graphics the
    // old unconditional "_1_1" block always wrote -- no behavior change.
    for u in 1..=unit_count {
        let baker = instances.iter().find(|s| s.unit == u).copied().unwrap_or(default_baker);
        let (width, height) = eda_engine::geometry::node_size(part, resolved.as_ref(), u);
        let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height, resolved.as_ref(), u);
        let (x0, _, _, y1) = resolved.as_ref().map(|s| eda_engine::geometry::real_symbol_bbox(s, u)).unwrap_or((0.0, 0.0, 0.0, 0.0));
        let mut pin_of_port: Vec<Option<usize>> = vec![None; ports.len()];
        for (pin_idx, port_idx) in pin_port.iter().enumerate() {
            if let Some(pi) = port_idx {
                pin_of_port[*pi] = Some(pin_idx);
            }
        }

        writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{bare_name}_{u}_1"))).unwrap();
        // This unit's own graphics (a multi-unit symbol very often draws a
        // different body outline per unit -- e.g. each gate of a logic
        // array) -- unit 1 of a single-unit part has none of these beyond
        // what `_0_1` already drew, same as before this loop existed.
        if let Some(sym_data) = &resolved {
            for g in sym_data.graphics.iter().filter(|g| g.unit() == u) {
                write_symbol_graphic(out, &baked_graphic(g, baker, width as f64, x0, y1));
            }
        }
        for (port_idx, port) in ports.iter().enumerate() {
            let Some(pin_idx) = pin_of_port[port_idx] else { continue };
            let pin = &part.pins[pin_idx];
            let real_pin = resolved.as_ref().and_then(|s| s.pin_by_number(&pin.number));
            let (bx, by, angle, length_mm) = match real_pin {
                // The real pin's own outer point/angle/length -- exactly
                // where its own drawn line (just written above, as part of
                // the body's graphics when it's a separate primitive, or
                // implicit when the pin line itself *is* the graphic) and a
                // wire's own stub tip (computed from the very same point by
                // `build_ports`'s real-symbol path) both place it.
                Some(rp) => {
                    let p = baked_real_point(baker, width as f64, x0, y1, rp.at);
                    (p.x, p.y, baked_real_angle(baker, rp.angle_deg), rp.length_mm)
                }
                // No matching real pin (shouldn't happen for a resolved
                // symbol whose numbers agree with ours -- defensive only):
                // fall back to the synthetic stub tip so the instance's own
                // pin-uuid list still has a drawn pin for every part.pins
                // entry.
                None => {
                    let (plx, ply) = local_port_point(port, width, height);
                    let (slx, sly) = local_stub_tip(port, plx, ply);
                    let (bx, by) = baked_local(baker, width as f64, slx, sly);
                    (bx, by, 0.0, STUB_MM)
                }
            };
            let etype = resolve_pin_electrical_type(pin, resolved.as_ref());
            let name = pin.name.clone().unwrap_or_else(|| "~".to_string());
            writeln!(
                out,
                "\t\t\t\t(pin {etype} line (at {} {} {}) (length {})\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
                fmt_mm_f(bx),
                fmt_mm_f(by),
                fmt_mm_f(angle),
                fmt_mm_f(length_mm),
                sexpr_str(&name),
                sexpr_str(&pin.number),
            )
            .unwrap();
        }
        // `nc`-kind pins carry no `Port` (see `geometry::nc_pin_local_points`);
        // draw them here too, at the same points `derive_schematic` placed a
        // `no_connects` marker at, so this unit's own pin-uuid list always
        // has a matching drawn pin for every one of its `part.pins` entries.
        for (i, local) in eda_engine::geometry::nc_pin_local_points(part, width, height, resolved.as_ref(), u) {
            let pin = &part.pins[i];
            // `baked_local` takes um (like every other call site in this
            // function — `local_stub_tip`'s output, `width`/`height`
            // themselves) and does its own um->mm division at the end; do
            // not pre-convert `local` here too, or every nc pin lands 1000x
            // closer to the origin than intended.
            let (bx, by) = baked_local(baker, width as f64, local.x as f64, local.y as f64);
            let etype = resolve_pin_electrical_type(pin, resolved.as_ref());
            let name = pin.name.clone().unwrap_or_else(|| "~".to_string());
            writeln!(
                out,
                "\t\t\t\t(pin {etype} line (at {} {} 0) (length 0)\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
                fmt_mm_f(bx),
                fmt_mm_f(by),
                sexpr_str(&name),
                sexpr_str(&pin.number),
            )
            .unwrap();
        }
        writeln!(out, "\t\t\t)").unwrap();
    }

    writeln!(out, "\t\t)").unwrap();
}

/// A power symbol's `lib_symbol`: the resolved definition's real graphics
/// and pin(s), embedded verbatim (already in the correct on-disk
/// convention — see [`baked_local`]'s doc comment). One shared definition
/// per `lib_id`, regardless of how many instances place it.
fn write_power_lib_symbol(out: &mut String, lib_id: &str, resolved: &eda_model::LibSymbol) {
    writeln!(out, "\t\t(symbol {}", sexpr_str(lib_id)).unwrap();
    writeln!(out, "\t\t\t(power)").unwrap();
    writeln!(out, "\t\t\t(pin_names (offset 0) (hide yes))").unwrap();
    writeln!(out, "\t\t\t(exclude_from_sim no) (in_bom yes) (on_board yes)").unwrap();
    let value = lib_id.strip_prefix("power:").unwrap_or(lib_id);
    write_property(out, "Reference", "#PWR", 0.0, -2.0, true);
    write_property(out, "Value", value, 0.0, 2.0, false);
    write_property(out, "Footprint", "", 0.0, 0.0, true);
    write_property(out, "Datasheet", &resolved.datasheet, 0.0, 0.0, true);

    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{value}_0_1"))).unwrap();
    for g in &resolved.graphics {
        write_symbol_graphic(out, g);
    }
    writeln!(out, "\t\t\t)").unwrap();

    writeln!(out, "\t\t\t(symbol {}", sexpr_str(&format!("{value}_1_1"))).unwrap();
    for pin in &resolved.pins {
        writeln!(
            out,
            "\t\t\t\t(pin {} line (at {} {} {}) (length {})\n\t\t\t\t\t(name {} (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number {} (effects (font (size 1.27 1.27))))\n\t\t\t\t)",
            pin.electrical_type,
            fmt_mm_f(pin.at.x),
            fmt_mm_f(pin.at.y),
            fmt_mm_f(pin.angle_deg),
            fmt_mm_f(pin.length_mm),
            sexpr_str(&pin.name),
            sexpr_str(&pin.number),
        )
        .unwrap();
    }
    writeln!(out, "\t\t\t)").unwrap();
    writeln!(out, "\t\t)").unwrap();
}

fn write_symbol_graphic(out: &mut String, g: &eda_model::SymbolGraphic) {
    use eda_model::SymbolGraphic::*;
    match g {
        Rectangle { start, end, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(rectangle\n\t\t\t\t\t(start {} {}) (end {} {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(start.x),
                fmt_mm_f(start.y),
                fmt_mm_f(end.x),
                fmt_mm_f(end.y),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Polyline { pts, stroke_mm, filled, .. } => {
            write!(out, "\t\t\t\t(polyline\n\t\t\t\t\t(pts").unwrap();
            for p in pts {
                write!(out, " (xy {} {})", fmt_mm_f(p.x), fmt_mm_f(p.y)).unwrap();
            }
            writeln!(
                out,
                ")\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Circle { center, radius_mm, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(circle\n\t\t\t\t\t(center {} {}) (radius {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(center.x),
                fmt_mm_f(center.y),
                fmt_mm_f(*radius_mm),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Arc { start, mid, end, stroke_mm, filled, .. } => {
            writeln!(
                out,
                "\t\t\t\t(arc\n\t\t\t\t\t(start {} {}) (mid {} {}) (end {} {})\n\t\t\t\t\t(stroke (width {}) (type default))\n\t\t\t\t\t(fill (type {}))\n\t\t\t\t)",
                fmt_mm_f(start.x),
                fmt_mm_f(start.y),
                fmt_mm_f(mid.x),
                fmt_mm_f(mid.y),
                fmt_mm_f(end.x),
                fmt_mm_f(end.y),
                fmt_mm_f(*stroke_mm),
                if *filled { "background" } else { "none" },
            )
            .unwrap();
        }
        Text { text, at, angle_deg, size_mm, .. } => {
            writeln!(
                out,
                "\t\t\t\t(text {} (at {} {} {})\n\t\t\t\t\t(effects (font (size {} {})))\n\t\t\t\t)",
                sexpr_str(text),
                fmt_mm_f(at.x),
                fmt_mm_f(at.y),
                fmt_mm_f(*angle_deg),
                fmt_mm_f(*size_mm),
                fmt_mm_f(*size_mm),
            )
            .unwrap();
        }
    }
}

/// A pin's electrical type for the file: the real resolved library
/// symbol's own pin, matched by number (the "map pins by number" this
/// port's loader exists for), when one resolved; else the same coarse
/// `PinKind` mapping this exporter has always used.
fn resolve_pin_electrical_type(pin: &eda_model::Pin, resolved: Option<&eda_model::LibSymbol>) -> String {
    if let Some(p) = resolved.and_then(|s| s.pin_by_number(&pin.number)) {
        return p.electrical_type.clone();
    }
    electrical_type(pin.kind, pin.name.as_deref()).to_string()
}

/// The coarse `PinKind` -> KiCad electrical-type fallback this exporter
/// uses whenever no real library symbol resolved a pin's real type. A
/// `Power`-kind pin whose name reads as an output (a regulator's own
/// VOUT) maps to `power_out`, not `power_in` — the one place this coarse
/// mapping needs to distinguish a rail's source from its sinks, since
/// `power_pin_not_driven` (kicad-cli's ERC) requires *some*
/// `power_out` pin on every power net, and nothing else in this project's
/// model says which `Power`-kind pin, if any, plays that role. Kept in
/// lockstep with `eda_engine::derive_schematic`'s own `PWR_FLAG` decision,
/// which uses the exact same name convention.
fn electrical_type(kind: PinKind, name: Option<&str>) -> &'static str {
    match kind {
        PinKind::Power if name.unwrap_or("").to_ascii_uppercase().contains("OUT") => "power_out",
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
}

/// Local (box-space, um) point for `port`, matching `eda_render::local_port_point`.
fn local_port_point(port: &Port, width: i64, height: i64) -> (f64, f64) {
    match port.side {
        Side::Top => (port.offset as f64, 0.0),
        Side::Bottom => (port.offset as f64, height as f64),
        Side::Left => (0.0, port.offset as f64),
        Side::Right => (width as f64, port.offset as f64),
    }
}

/// Local (box-space, um) pin-stub tip, matching `eda_render::stub_tip`.
fn local_stub_tip(port: &Port, lx: f64, ly: f64) -> (f64, f64) {
    let stub = eda_engine::geometry::STUB as f64;
    match port.side {
        Side::Top => (lx, ly - stub),
        Side::Bottom => (lx, ly + stub),
        Side::Left => (lx - stub, ly),
        Side::Right => (lx + stub, ly),
    }
}

/// Applies the symbol's mirror+rotation (but NOT translation) to a local
/// (um) point, exactly matching `eda_render::SymbolBox::to_abs` minus the
/// final `+ sym.at`. Returns mm.
/// KiCad's `lib_symbols` graphics/pins live in the *library's own* frame,
/// which is +y **up**; everything else in a `.kicad_sch` (the sheet itself,
/// symbol instance `(at ...)`, wires, labels) is +y **down**, matching this
/// crate's own `ir::Point`. KiCad negates a library symbol's local y when
/// it composites that symbol onto the sheet (instance position + library
/// point, with the library point's y flipped) -- so a lib_symbol's own
/// graphics/pins must be written with y already negated, or the two
/// negations fail to cancel and KiCad places the *instance's* pins
/// somewhere our own wires never reach. Confirmed against real
/// `kicad-cli sch erc`: without this negation, every pin whose local y is
/// non-zero comes back `pin_not_connected` (and, for a power-style pin,
/// `power_pin_not_driven` too) even though the wire in the file terminates
/// at exactly the un-negated point -- KiCad is looking for it at `-y`.
fn baked_local(sym: &SymbolInstance, width: f64, lx: f64, ly: f64) -> (f64, f64) {
    let lx = if sym.mirrored { width - lx } else { lx };
    let theta = (sym.rot as f64 / 1000.0) * std::f64::consts::PI / 180.0;
    let rx = lx * theta.cos() - ly * theta.sin();
    let ry = lx * theta.sin() + ly * theta.cos();
    (rx / 1000.0, -ry / 1000.0)
}

pub(crate) fn mm(um: i64) -> String {
    fmt_mm_f(um as f64 / 1000.0)
}

pub(crate) fn fmt_mm_f(v: f64) -> String {
    // Round to 0.1 um to kill float noise from trig, keep well under the
    // 1 um round-trip tolerance.
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let s = format!("{rounded:.4}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

pub(crate) fn sexpr_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// KiCad stores a pad's drawn rotation as its footprint's own file angle
/// plus the pad's own -- but mirroring (a bottom-side part) flips the
/// *sense* of a rotation that is defined, as `Pad::rot` is, on the
/// footprint's un-mirrored local shape: mirroring a shape that carries its
/// own rotation of `theta` is the same as mirroring the unrotated shape and
/// then rotating it by `-theta`. So a pad's own rotation contributes with
/// the opposite sign on the bottom side; the footprint's own `fp_rot` needs
/// no such flip (it is applied *after* the mirror, in `to_board`, so its
/// sign is the same either side).
///
/// Returns the pad's absolute file angle, degrees, ready for `(at x y
/// angle)` -- the same single negation `write_footprint` already applies
/// to a footprint with no rotated pads.
pub(crate) fn pad_file_angle(fp_side: eda_model::ir::Side, fp_rot: eda_model::ir::Millideg, pad_rot: eda_model::ir::Millideg) -> String {
    let effective = if fp_side == eda_model::ir::Side::Bottom { -(pad_rot as i64) } else { pad_rot as i64 };
    fmt_mm_f(-((fp_rot as i64 + effective) as f64) / 1000.0)
}

/// Inverse of `pad_file_angle`: recover a pad's own `rot` (relative to its
/// footprint, our internal convention) from its file angle and its
/// footprint's own rotation (already imported, `ir::Millideg`).
pub(crate) fn pad_rot_from_file(fp_side: eda_model::ir::Side, fp_rot: eda_model::ir::Millideg, pad_file_deg: f64) -> eda_model::ir::Millideg {
    let combined = import::import_rot_millideg(pad_file_deg) as i64;
    let delta = (combined - fp_rot as i64).rem_euclid(360_000);
    (if fp_side == eda_model::ir::Side::Bottom { -delta } else { delta }).rem_euclid(360_000) as eda_model::ir::Millideg
}

/// Deterministic UUID-shaped id derived from a stable string (blake3, not a
/// true RFC 4122 v5, but stable/collision-resistant and structurally valid
/// so KiCad accepts it as a UUID field).
pub(crate) fn duid(seed: &str) -> String {
    duid_raw(seed)
}

thread_local! {
    /// Active while [`pcb::export_kicad_pcb_mapped`] runs: every exported
    /// item's KiCad uuid -> our own item id.
    static UUID_MAP: std::cell::RefCell<Option<std::collections::HashMap<String, String>>> = const { std::cell::RefCell::new(None) };
}

/// [`duid`], also recording `uuid -> our_id` when a mapped export is running.
pub(crate) fn duid_for(seed: &str, our_id: &str) -> String {
    let u = duid_raw(seed);
    UUID_MAP.with(|m| {
        if let Some(map) = m.borrow_mut().as_mut() {
            map.insert(u.clone(), our_id.to_string());
        }
    });
    u
}

pub(crate) fn start_uuid_map() {
    UUID_MAP.with(|m| *m.borrow_mut() = Some(Default::default()));
}

pub(crate) fn take_uuid_map() -> std::collections::HashMap<String, String> {
    UUID_MAP.with(|m| m.borrow_mut().take().unwrap_or_default())
}

fn duid_raw(seed: &str) -> String {
    let hash = blake3::hash(seed.as_bytes());
    let b = hash.as_bytes();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&b[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Pin};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: None, footprint: Some("Foo:Bar".into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![
                pin("1", "VIN", PinKind::Power),
                pin("2", "GND", PinKind::Ground),
                pin("3", "EN", PinKind::Signal),
                pin("4", "VOUT", PinKind::Power),
                pin("5", "NC", PinKind::Nc),
            ],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        ConstraintModel {
            parts: vec![u1, cin, cout],
            nets: vec![
                net("VIN", &["U1.1", "CIN.1", "U1.3"]),
                net("VOUT", &["U1.4", "COUT.1"]),
                net("GND", &["U1.2", "CIN.2", "COUT.2"]),
            ],
            ..Default::default()
        }
    }

    fn export(model: &ConstraintModel) -> String {
        let design = derive_schematic(model, &EngineOptions::new(1, "hash")).unwrap();
        let meta = ExportMeta { date: "2026-01-01", title: "LDO test" };
        export_kicad_sch(&design, model, &meta).unwrap()
    }

    #[test]
    fn the_sheets_paper_and_title_block_are_written_and_read_back_by_the_importer() {
        let model = ldo_model();
        let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let meta = ExportMeta { date: "2026-01-01", title: "LDO test" };
        let plain = export_kicad_sch(&design, &model, &meta).unwrap();
        assert!(plain.contains("(paper \"A4\")"), "never set: A4 landscape");
        assert!(plain.contains("(comment 1 \"engine_version:"), "no title block of its own: the provenance comments, as before");

        let sch = design.schematic.as_mut().unwrap();
        sch.extras.page = Some(eda_model::page::PageSettings { paper: "USLetter".into(), portrait: true, user_size_um: None });
        sch.title_block = Some(eda_model::ir::TitleBlock { title: "Mine".into(), comments: vec!["only".into()], ..Default::default() });
        let out = export_kicad_sch(&design, &model, &meta).unwrap();
        assert!(out.contains("(paper \"USLetter\" portrait)"), "{out}");
        assert!(out.contains("(comment 1 \"only\")") && !out.contains("engine_version:"), "a title block of its own has its own comments only");

        let (back, _, _) = import_kicad_sch(&out).unwrap();
        let sch = back.schematic.unwrap();
        assert_eq!(sch.extras.page, Some(eda_model::page::PageSettings { paper: "USLetter".into(), portrait: true, user_size_um: None }));
        assert_eq!(sch.title_block.map(|t| t.title), Some("Mine".to_string()));
        let (default_back, _, _) = import_kicad_sch(&plain).unwrap();
        assert!(default_back.schematic.unwrap().extras.page.is_none(), "A4 landscape is the default and is not kept");
    }

    fn balanced_parens(s: &str) -> bool {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escaped = false;
        for c in s.chars() {
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return false;
            }
        }
        depth == 0 && !in_str
    }

    #[test]
    fn balanced_parens_output() {
        let out = export(&ldo_model());
        assert!(balanced_parens(&out), "unbalanced parens:\n{out}");
    }

    #[test]
    fn deterministic_export() {
        let model = ldo_model();
        let a = export(&model);
        let b = export(&model);
        assert_eq!(a, b);
    }

    #[test]
    fn symbol_and_wire_counts_match() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let out = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        // Every part instance *and* every power symbol (a `PWR_FLAG`
        // included) is written as its own `(symbol (lib_id ...) ...)`
        // sheet block, so the pattern's count is the sum of both, not
        // just the part instances.
        assert_eq!(out.matches("(symbol (lib_id").count(), sch.symbols.len() + sch.power_symbols.len());
        let expected_wire_segments: usize = sch.wires.iter().map(|w| w.pts.len().saturating_sub(1)).sum();
        assert_eq!(out.matches("\t(wire\n").count(), expected_wire_segments);
    }

    #[test]
    fn pin_coordinates_round_trip_within_1um() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        let u1 = sch.symbols.iter().find(|s| s.id == "U1").unwrap();
        let part = model.part("U1").unwrap();
        let (width, height) = eda_engine::geometry::node_size(part, None, 1);
        let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height, None, 1);
        // VIN is pin "1" -> some port; compute expected world stub tip.
        let port_idx = pin_port[0].unwrap();
        let port = ports[port_idx];
        let (plx, ply) = local_port_point(&port, width, height);
        let (slx, sly) = local_stub_tip(&port, plx, ply);
        let (bx_mm, by_mm) = baked_local(u1, width as f64, slx, sly);
        let expected_x_um = u1.at.x + (bx_mm * 1000.0).round() as i64;
        let expected_y_um = u1.at.y + (by_mm * 1000.0).round() as i64;

        let meta = ExportMeta { date: "2026-01-01", title: "t" };
        let out = export_kicad_sch(&design, &model, &meta).unwrap();
        // Find the emitted pin line for number "1" within U1's own
        // "_1_1" pin sub-symbol block and parse its (at x y 0).
        let scope_start = out.find("\"U1_1_1\"").expect("U1 pin block emitted");
        let scope = &out[scope_start..];
        let marker = "(number \"1\"";
        let idx = scope.find(marker).expect("pin 1 emitted");
        let before = &scope[..idx];
        let at_idx = before.rfind("(at ").expect("preceding (at ...)");
        let at_str = &before[at_idx + 4..];
        let end = at_str.find(" 0)").unwrap();
        let coords: Vec<f64> = at_str[..end].split(' ').map(|t| t.parse().unwrap()).collect();
        let got_x_um = (coords[0] * 1000.0).round() as i64 + u1.at.x;
        let got_y_um = (coords[1] * 1000.0).round() as i64 + u1.at.y;
        assert!((got_x_um - expected_x_um).abs() <= 1, "x mismatch: {got_x_um} vs {expected_x_um}");
        assert!((got_y_um - expected_y_um).abs() <= 1, "y mismatch: {got_y_um} vs {expected_y_um}");
    }

    #[test]
    fn power_nets_use_power_symbols_not_wires() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        let out = export(&model);
        // GND is a name-recognized ground net, so the engine drops a real
        // `power:GND` symbol at each of its pins instead of wiring them or
        // labeling them — no `global_label`/`label` for GND at all.
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.power_symbols.iter().any(|p| p.net == "GND" && p.lib_id == "power:GND"), "{:#?}", sch.power_symbols);
        assert!(out.contains("(lib_id \"power:GND\")"), "GND should be a real power symbol:\n{out}");
        assert!(!out.contains("(global_label \"GND\""));
        assert!(!out.contains("(label \"GND\""));
    }

    #[test]
    fn missing_schematic_errors() {
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: None,
            routing: None,
            drawings: None,
        };
        let model = ConstraintModel::default();
        let err = export_kicad_sch(&design, &model, &ExportMeta { date: "d", title: "t" }).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn unknown_part_errors() {
        let mut model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
        model.parts.clear(); // now no part resolves any symbol id
        let err = export_kicad_sch(&design, &model, &ExportMeta { date: "d", title: "t" }).unwrap_err();
        assert!(err.iter().any(|e| e.check == "kicad.unknown_part"));
    }

    #[test]
    fn uuids_are_stable_and_look_like_uuids() {
        let a = duid("sym:U1");
        let b = duid("sym:U1");
        let c = duid("sym:U2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 36);
        assert_eq!(a.chars().filter(|&c| c == '-').count(), 4);
    }

    /// GAPS.md #6: `export_kicad_sch_tree` writes a real `(sheet ...)`
    /// placement (with its own pin) on the root, and the child screen it
    /// names as its own standalone file -- round-tripped end to end
    /// through `import_kicad_sch_tree` (actual disk files, not just
    /// in-memory structs), confirming the two directions agree on the
    /// file format this task's own research pinned down.
    #[test]
    fn multi_sheet_design_round_trips_through_export_tree_and_import_tree() {
        use eda_model::ir::{LabelKind, LabelShape, Point, SchematicSection, SheetInstance, SheetPin};

        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let model = ConstraintModel { parts: vec![r1], ..Default::default() };
        let mut child_design = derive_schematic(&model, &EngineOptions::new(1, "tree_export_child")).unwrap();
        let child_sch = child_design.schematic.as_mut().unwrap();
        // A hierarchical label landing exactly on R1's own pin 1 stub tip
        // -- reuse the already-reconciled wire-free single-pin convention
        // this crate's other tests already rely on, rather than
        // re-deriving the exact stub-tip offset by hand.
        let r1_at = child_sch.symbols[0].at;
        let pin1_tip = Point { x: r1_at.x, y: r1_at.y - 3810 };
        child_sch.labels.push(eda_model::ir::NetLabel { id: String::new(), net: "AD0".into(), at: pin1_tip, kind: LabelKind::Hierarchical { shape: LabelShape::Passive } });

        let root_sch = SchematicSection {
            symbols: vec![],
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![], bus_entries: vec![],
            erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(),
            title_block: None,
            sheets: vec![SheetInstance {
                id: String::new(),
                name: "child".into(),
                file: "child.kicad_sch".into(),
                at: Point { x: 10_000, y: 10_000 },
                size: (20_000, 20_000),
                pins: vec![SheetPin { id: String::new(), name: "AD0".into(), shape: LabelShape::Passive, at: Point { x: 15_000, y: 30_000 } }],
                page: String::new(),
            }],
            instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(),
            imported_from_kicad: false,
        };
        let mut screens = std::collections::BTreeMap::new();
        screens.insert("child.kicad_sch".to_string(), child_sch.clone());
        let design = Design { sheet_contents: Some(screens), schematic: Some(root_sch), ..child_design };

        let files = export_kicad_sch_tree(&design, &model, &ExportMeta { date: "2026-01-01", title: "root" }, "root.kicad_sch").unwrap();
        assert_eq!(files.len(), 2, "root + one child");

        let dir = std::env::temp_dir().join(format!("eda_kicad_export_tree_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in &files {
            std::fs::write(dir.join(name), text).unwrap();
        }

        let (back_design, _model, notes) = crate::import_kicad_sch_tree(&dir.join("root.kicad_sch")).expect("round trips");
        assert_eq!(notes.sheets_not_descended, 0);
        let back_root = back_design.schematic.unwrap();
        assert_eq!(back_root.sheets.len(), 1);
        assert_eq!(back_root.sheets[0].file, "child.kicad_sch");
        assert_eq!(back_root.sheets[0].pins.len(), 1, "the sheet pin survives the round trip");
        assert_eq!(back_root.sheets[0].pins[0].name, "AD0");
        let back_child = back_design.sheet_contents.unwrap().remove("child.kicad_sch").unwrap();
        assert_eq!(back_child.symbols.len(), 1);
        assert!(back_child.labels.iter().any(|l| l.net == "AD0" && matches!(l.kind, LabelKind::Hierarchical { .. })));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
