//! `import_kicad_sch` -- the inverse of [`crate::export_kicad_sch`]: reads a
//! real `.kicad_sch` file (ours, or a human-authored KiCad one) into
//! `Design`+`ConstraintModel`.
//!
//! Ported fields: the embedded `lib_symbols` (via `symbol_lib::build_symbol_table`
//! -- the same reader the real-library loader uses), placed symbol
//! instances (lib_id, position, rotation, mirror, unit, Reference/Value/
//! Footprint/Datasheet), power symbols (any instance whose resolved
//! lib_symbol carries the library's `(power)` marker), wires, no-connect
//! flags, and local/global/hierarchical labels. A `(sheet ...)` is recorded
//! (name/file/position/size) but not descended into -- hierarchy is
//! deferred, see the task's own report.
//!
//! `.kicad_sch` carries no explicit net list (KiCad derives nets from drawn
//! connectivity, the same way this reader has to): [`reconcile`] rebuilds
//! one by treating every point two items share -- a wire's own points
//! chained together, a pin's position, a label's anchor, a power symbol's
//! own pin, a no-connect flag's position, and a wire endpoint landing on
//! another same-net wire's segment interior (a T-junction with no shared
//! vertex) -- as the same electrical node, via a small union-find. A
//! resulting group's net name prefers a label's text, then a power
//! symbol's asserted net, else a synthesized `NET_<n>`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use eda_model::ir::{Design, LabelKind, LabelShape, Millideg, NoConnect, Point, PowerSymbol, Provenance, SchematicSection, SchematicText, SheetInstance, SymbolInstance, TitleBlock, Wire};
use eda_model::symbol::{LibSymbol, SPoint};
use eda_model::{CheckResult, ConstraintModel, Net, Part, Pin};

use crate::sexpr::{self, Sexpr};
use crate::symbol_lib::build_symbol_table;

/// Counts of things this reader saw but could not carry into our model
/// exactly, or at all.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchImportNotes {
    /// Sheets present in the file, recorded (name/file/position) but not
    /// descended into -- hierarchy is deferred.
    pub sheets_not_descended: usize,
    /// A symbol instance whose `lib_id` resolved no embedded `lib_symbols`
    /// entry -- its pins/electrical types could not be recovered, so it was
    /// dropped.
    pub unresolved_symbols: usize,
}

pub fn import_kicad_sch(text: &str) -> Result<(Design, ConstraintModel, SchImportNotes), Vec<CheckResult>> {
    let tree = sexpr::parse(text).map_err(|e| vec![CheckResult::fail("kicad_import.parse", "file", format!("not a valid s-expression file: {e}"))])?;
    let root = tree
        .as_list()
        .filter(|l| sexpr::tag(l) == Some("kicad_sch"))
        .ok_or_else(|| vec![CheckResult::fail("kicad_import.not_a_schematic", "file", "top-level form is not (kicad_sch ...); not a KiCad schematic file")])?;

    let mut notes = SchImportNotes::default();
    let lib_table: HashMap<String, LibSymbol> = sexpr::find(root, "lib_symbols").map(build_symbol_table).unwrap_or_default();

    let title_block = import_title_block(root);

    let mut symbols: Vec<SymbolInstance> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut power_symbols: Vec<PowerSymbol> = Vec::new();
    // Every real pin's own world position, keyed "REF.NUM" -- the ground
    // truth [`reconcile`] ties everything else back to.
    let mut pin_world: BTreeMap<String, Point> = BTreeMap::new();

    // Parsed ahead of the symbol loop below (rather than in its usual
    // position alongside wires/labels) so a pin's own reconstructed
    // `PinKind` can consult it: a `no_connect` flag sitting exactly on a
    // pin's computed world point is the *only* place this project's own
    // `nc` semantic survives the round trip through a real library symbol,
    // since a generic part's (e.g. a connector's) real electrical type is
    // just "passive" whether or not a given pin is actually used -- see
    // `pin_kind_from_electrical_type`'s own doc.
    let mut no_connects: Vec<NoConnect> = Vec::new();
    for nc in sexpr::find_all(root, "no_connect") {
        if let Some(at) = sexpr::find(nc, "at").and_then(point_mm) {
            no_connects.push(NoConnect { id: String::new(), at: mm_point_to_um(at), pin: String::new() });
        }
    }
    let nc_points: std::collections::BTreeSet<Point> = no_connects.iter().map(|nc| nc.at).collect();

    // One raw, not-yet-grouped record per placed `(symbol ...)` item --
    // multi-unit means several of these can share one `reference` (e.g. a
    // quad op-amp's gates A/B/C/D, each its own placed instance at its own
    // position, see `SymbolInstance::unit`'s own doc). Collected first, then
    // grouped by reference below, because a reference's `Part` needs to see
    // *every* one of its placed units before its own pin list (every pin
    // the real library declares, not just whichever unit happened to be
    // read first) and per-pin world position can be resolved.
    struct RawInstance {
        lib_id: String,
        at_um: Point,
        rot: Millideg,
        angle_deg: f64,
        mirrored: bool,
        mirror_y: bool,
        unit: u32,
        reference: String,
        value: String,
        footprint: String,
        datasheet: String,
    }
    let mut raw: Vec<RawInstance> = Vec::new();
    // `SCH_SYMBOL_INSTANCE`-style per-sheet-instance overrides -- only ever
    // non-empty when this screen is placed by more than one `SheetInstance`
    // (see `SymbolPathOverride`'s own doc). Parsed unconditionally here
    // (not just when this read is part of a multi-file tree walk) because
    // the data lives in *this* file's own `(symbol ... (instances ...))`
    // blocks regardless of whether the caller goes on to assemble a tree
    // around it -- a bare single-file `import_kicad_sch` on a screen that
    // happens to be shared just carries overrides nothing yet queries.
    let mut overrides: Vec<eda_model::ir::SymbolPathOverride> = Vec::new();

    for item in sexpr::find_all(root, "symbol") {
        let Some(lib_id) = sexpr::find(item, "lib_id").and_then(|l| sexpr::txt(l, 1)) else { continue };
        let Some(resolved) = lib_table.get(lib_id) else {
            notes.unresolved_symbols += 1;
            continue;
        };
        let Some(at) = sexpr::find(item, "at") else { continue };
        let (Some(x_mm), Some(y_mm)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle_deg = sexpr::num(at, 3).unwrap_or(0.0);
        // `(mirror x)` -> `SYM_MIRROR_X` -> this app's `mirror_y` (negate
        // local Y); `(mirror y)` -> `SYM_MIRROR_Y` -> this app's `mirrored`
        // (negate local X) -- confirmed directly against
        // sch_io_kicad_sexpr_parser.cpp's own `T_mirror` handling, not
        // guessed (the file token matches the internal enum's own suffix
        // verbatim, so it would have been an easy, wrong-rendering guess
        // to get backwards). `.is_some()` alone (no x/y distinction) was
        // this project's pre-`mirror_y` placeholder; see `Cmd::
        // MirrorSymbolVertical`'s own doc for why both fields are needed.
        let mirror_tag = sexpr::find(item, "mirror").and_then(|m| sexpr::txt(m, 1));
        let mirrored = mirror_tag == Some("y");
        let mirror_y = mirror_tag == Some("x");
        let unit = sexpr::find(item, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0).max(1.0) as u32;
        let at_um = Point { x: crate::import::mm_to_um(x_mm), y: crate::import::mm_to_um(y_mm) };
        let rot = import_rot_millideg_sch(angle_deg);

        let reference = property_text(item, "Reference").unwrap_or_default();
        let value = property_text(item, "Value").unwrap_or_default();
        let footprint = property_text(item, "Footprint").unwrap_or_default();
        let datasheet = property_text(item, "Datasheet").unwrap_or_default();

        if resolved.power {
            let pin_local = resolved.pins.first().map(|p| p.at).unwrap_or(SPoint::new(0.0, 0.0));
            let world = transform_local_point(pin_local, angle_deg, mirrored, mirror_y);
            let at_pin = Point { x: at_um.x + crate::import::mm_to_um(world.x), y: at_um.y + crate::import::mm_to_um(world.y) };
            power_symbols.push(PowerSymbol { id: reference, lib_id: lib_id.to_string(), at: at_pin, rot, net: value, pin: String::new() });
            continue;
        }

        overrides.extend(parse_instance_overrides(item, at_um));
        raw.push(RawInstance { lib_id: lib_id.to_string(), at_um, rot, angle_deg, mirrored, mirror_y, unit, reference, value, footprint, datasheet });
    }

    // Group by reference, preserving first-sighting order (file order) so
    // `parts`/`symbols` come out in a deterministic, re-reading-stable
    // sequence -- `assign_missing_ids`/the exporter's own `symbols.sort_by`
    // re-sort later, but a stable starting order still matters for any
    // same-hash tie-break along the way.
    let mut order: Vec<String> = Vec::new();
    let mut by_ref: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, r) in raw.iter().enumerate() {
        by_ref.entry(r.reference.clone()).or_default().push(i);
        if !order.contains(&r.reference) {
            order.push(r.reference.clone());
        }
    }

    for reference in order {
        let idxs = &by_ref[&reference];
        // Every instance of one reference is the same real part (one
        // footprint, several placed units -- this task's own framing), so
        // they all resolve the same `lib_id`/library symbol; the first
        // instance found stands in for the whole group as the "primary"
        // (used for the `Part`-level Value/Footprint/Datasheet denormalized
        // copies below, same precedent `derive_schematic`'s single-instance
        // case already set).
        let primary = &raw[idxs[0]];
        let lib_id = primary.lib_id.clone();
        let resolved = lib_table.get(lib_id.as_str());

        let mut pins: Vec<Pin> = Vec::new();
        if let Some(resolved) = resolved {
            pins.reserve(resolved.pins.len());
            for p in &resolved.pins {
                // The specific placed instance that actually carries this
                // pin's unit (or any instance at all, for a `unit == 0`
                // pin common to every unit) -- `None` means this pin's own
                // unit was never placed anywhere on the sheet at all (ERC's
                // `missing_unit`/`missing_power_pin` is what flags that;
                // the pin still belongs to the physical part, so it still
                // gets a `Pin` entry here, just with no resolvable world
                // position to seed `pin_world` with).
                let owner = idxs.iter().map(|&i| &raw[i]).find(|r| p.unit == 0 || p.unit == r.unit);
                let kind = match owner {
                    Some(owner) => {
                        let world = transform_local_point(p.at, owner.angle_deg, owner.mirrored, owner.mirror_y);
                        let at_pin = Point { x: owner.at_um.x + crate::import::mm_to_um(world.x), y: owner.at_um.y + crate::import::mm_to_um(world.y) };
                        pin_world.insert(format!("{reference}.{}", p.number), at_pin);
                        if nc_points.contains(&at_pin) {
                            eda_model::PinKind::Nc
                        } else {
                            pin_kind_from_electrical_type(&p.electrical_type, &p.name)
                        }
                    }
                    None => pin_kind_from_electrical_type(&p.electrical_type, &p.name),
                };
                pins.push(Pin { number: p.number.clone(), name: (!p.name.is_empty()).then(|| p.name.clone()), kind });
            }
        }
        parts.push(Part {
            reference: reference.clone(),
            mpn: None,
            lcsc: None,
            value: (!primary.value.is_empty()).then(|| primary.value.clone()),
            package: None,
            footprint: (!primary.footprint.is_empty()).then(|| primary.footprint.clone()),
            symbol: Some(lib_id.clone()),
            datasheet: (!primary.datasheet.is_empty()).then(|| primary.datasheet.clone()),
            pins,
            body_um: None,
            edge: None,
        });
        for &i in idxs {
            let r = &raw[i];
            symbols.push(SymbolInstance {
                id: reference.clone(),
                at: r.at_um,
                rot: r.rot,
                mirrored: r.mirrored,
                mirror_y: r.mirror_y,
                lib_id: r.lib_id.clone(),
                unit: r.unit,
                value: r.value.clone(),
                footprint: r.footprint.clone(),
                datasheet: r.datasheet.clone(),
            });
        }
    }

    let mut wires: Vec<Wire> = Vec::new();
    for w in sexpr::find_all(root, "wire") {
        if let Some(pts) = import_pts(w) {
            wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus: false });
        }
    }
    // `(bus ...)` (GAPS.md #20): the exact same `SCH_LINE`/`(pts ...)` shape
    // as `(wire ...)`, just `LAYER_BUS` instead of `LAYER_WIRE` -- see
    // `Wire::bus`'s own doc.
    for b in sexpr::find_all(root, "bus") {
        if let Some(pts) = import_pts(b) {
            wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus: true });
        }
    }

    let mut bus_entries: Vec<eda_model::ir::BusEntry> = Vec::new();
    for be in sexpr::find_all(root, "bus_entry") {
        let Some(at) = sexpr::find(be, "at").and_then(point_mm) else { continue };
        let Some(size) = sexpr::find(be, "size").and_then(|sz| Some((sexpr::num(sz, 1)?, sexpr::num(sz, 2)?))) else { continue };
        let id = sexpr::find(be, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
        bus_entries.push(eda_model::ir::BusEntry { id, at: mm_point_to_um(at), size: Point { x: crate::import::mm_to_um(size.0), y: crate::import::mm_to_um(size.1) } });
    }

    // `(junction (at x y) ...)`: an explicit junction (`SCH_JUNCTION`) -- the only thing that joins two wires that merely cross
    // (see `Junction`; `reconcile` seeds it). Junctions the writer derives from geometry (`wire_junction_points`) come back as
    // explicit ones too: harmless, they sit where the wires already join.
    let mut junctions: Vec<eda_model::ir::Junction> = Vec::new();
    for j in sexpr::find_all(root, "junction") {
        let Some(at) = sexpr::find(j, "at").and_then(point_mm) else { continue };
        let at = mm_point_to_um(at);
        if !junctions.iter().any(|x| x.at == at) {
            junctions.push(eda_model::ir::Junction { id: String::new(), at });
        }
    }
    // `(polyline (pts ...) (stroke (width w) ...))`: a graphic line on the notes layer (`SchLine`).
    let mut lines: Vec<eda_model::ir::SchLine> = Vec::new();
    for pl in sexpr::find_all(root, "polyline") {
        if let Some(pts) = import_pts(pl).filter(|p| p.len() >= 2) {
            let width_um = sexpr::find(pl, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(crate::import::mm_to_um).unwrap_or(0);
            lines.push(eda_model::ir::SchLine { id: String::new(), pts, width_um });
        }
    }

    // `(bus_alias "NAME" (members "A" "B"))` (GAPS.md #20): real modern
    // KiCad only *writes* these into the `.kicad_pro` project file this
    // project has no reader/writer for at all -- this legacy per-screen
    // `.kicad_sch` block is kept only for backward compatibility with
    // older files (`SCH_IO_KICAD_SEXPR_PARSER::parseBusAlias`, itself still
    // forwarding into the same project-wide list on import) -- see
    // `BusAlias`'s own doc for why this project round-trips aliases through
    // it instead. Collected here per-screen; `import_kicad_sch_tree`
    // merges every screen's own list into one project-wide
    // `Design::bus_aliases` the same way real KiCad's project-wide
    // visibility already treats them.
    let mut bus_aliases: Vec<eda_model::ir::BusAlias> = Vec::new();
    for a in sexpr::find_all(root, "bus_alias") {
        let Some(name) = sexpr::txt(a, 1).map(String::from) else { continue };
        // `(members "A" "B" "C")` -- bare string atoms, not sub-lists, so
        // this walks past the "members" tag itself (index 0) rather than
        // using `find`/`find_all` (which only ever match nested lists).
        let members = sexpr::find(a, "members").map(|m| m.iter().skip(1).filter_map(|s| s.text().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
        bus_aliases.push(eda_model::ir::BusAlias { name, members });
    }

    let mut labels: Vec<eda_model::ir::NetLabel> = Vec::new();
    for (tag, global) in [("label", None), ("global_label", Some(true)), ("hierarchical_label", Some(false))] {
        for l in sexpr::find_all(root, tag) {
            let Some(net) = sexpr::txt(l, 1).map(String::from) else { continue };
            let Some(at) = sexpr::find(l, "at").and_then(point_mm) else { continue };
            let shape = sexpr::find(l, "shape").and_then(|s| sexpr::txt(s, 1)).map(label_shape_from_token).unwrap_or_default();
            let kind = match global {
                None => LabelKind::Local,
                Some(true) => LabelKind::Global { shape },
                Some(false) => LabelKind::Hierarchical { shape },
            };
            labels.push(eda_model::ir::NetLabel { id: String::new(), net, at: mm_point_to_um(at), kind });
        }
    }

    // `T`: standalone text, no net -- see `SchematicText`'s own doc. Unlike
    // a label, its content is purely cosmetic, so it never feeds `reconcile`
    // below.
    let mut texts: Vec<SchematicText> = Vec::new();
    for t in sexpr::find_all(root, "text") {
        let Some(content) = sexpr::txt(t, 1).map(String::from) else { continue };
        let Some(at) = sexpr::find(t, "at") else { continue };
        let (Some(x_mm), Some(y_mm)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle_deg = sexpr::num(at, 3).unwrap_or(0.0);
        let size_mm = sexpr::find(t, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).unwrap_or(1.27);
        texts.push(SchematicText {
            id: String::new(),
            content,
            at: mm_point_to_um(SPoint::new(x_mm, y_mm)),
            angle: import_rot_millideg_sch(angle_deg),
            size_um: crate::import::mm_to_um(size_mm),
        });
    }

    let mut sheets: Vec<SheetInstance> = Vec::new();
    for s in sexpr::find_all(root, "sheet") {
        let id = sexpr::find(s, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
        let name = sheet_property_text(s, "Sheetname").unwrap_or_default();
        let file = sheet_property_text(s, "Sheetfile").unwrap_or_default();
        let at = sexpr::find(s, "at").and_then(point_mm).unwrap_or(SPoint::new(0.0, 0.0));
        let size = sexpr::find(s, "size").and_then(|sz| Some((sexpr::num(sz, 1)?, sexpr::num(sz, 2)?))).unwrap_or((0.0, 0.0));
        let pins = sexpr::find_all(s, "pin")
            .filter_map(|p| {
                let name = sexpr::txt(p, 1)?.to_string();
                let shape = sexpr::txt(p, 2).map(label_shape_from_token).unwrap_or_default();
                let at = sexpr::find(p, "at").and_then(point_mm)?;
                let id = sexpr::find(p, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
                Some(eda_model::ir::SheetPin { id, name, shape, at: mm_point_to_um(at) })
            })
            .collect();
        sheets.push(SheetInstance { id, name, file, at: mm_point_to_um(at), size: (crate::import::mm_to_um(size.0), crate::import::mm_to_um(size.1)), pins });
        notes.sheets_not_descended += 1;
    }

    let junction_points: Vec<Point> = junctions.iter().map(|j| j.at).collect();
    let nets = reconcile(&pin_world, &mut wires, &labels, &mut power_symbols, &mut no_connects, &junction_points);

    let sch = SchematicSection { symbols, wires, labels, texts, power_symbols, no_connects, bus_entries, junctions, lines, erc_exclusions: Vec::new(), erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: true, title_block, sheets, instance_overrides: overrides, extras: Default::default() };
    let mut design = Design {
        schema: 1,
        provenance: Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: blake3::hash(text.as_bytes()).to_hex().to_string(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None, sheet_contents: None, bus_aliases, symbol_library: None,
    };
    design.assign_missing_ids();

    let model = ConstraintModel { parts, nets, symbols: lib_table.into_values().collect(), ..Default::default() };
    Ok((design, model, notes))
}

/// `import_kicad_sch`'s multi-file counterpart (GAPS.md #6): reads `path`
/// as the root sheet, then recursively follows every `(sheet ...)` it (and
/// each sheet it finds) names, resolving each `Sheetfile` against *its own
/// parent's* directory -- the convention every sheet-bearing fixture in
/// KiCad's own QA corpus actually uses (confirmed directly: every
/// `Sheetfile` found there is a bare filename sitting next to the sheet
/// that names it, never a subpath) -- and folds the result into
/// `Design::sheet_contents`. The *same* file named by more than one
/// `SheetInstance` (KiCad's own shared-`SCH_SCREEN` case -- see that
/// field's own doc) is read exactly once, no matter how many places name
/// it or how deep the recursion that reaches it goes. A cycle (a sheet
/// that, through some chain, names itself) is broken by the same
/// "already read this file" set, matching `SCH_SHEET_PATH::TestForRecursion`'s
/// own intent without reproducing its full path-based algorithm.
///
/// A sheet naming a file that does not exist, or does not parse, is left
/// exactly as `import_kicad_sch` alone would leave it -- recorded (name/
/// file/position/pins) but with no entry in `sheet_contents`, so a
/// consumer that cares (`hierarchy::flatten`, `check_hierarchy`) can tell
/// "placed but unreadable" apart from "placed and empty". `notes.
/// sheets_not_descended` is recomputed at the end to count only sheets
/// that are *still* un-descended after this whole walk, not the
/// per-file-read tally `import_kicad_sch` itself would have left it at.
pub fn import_kicad_sch_tree(path: &Path) -> Result<(Design, ConstraintModel, SchImportNotes), Vec<CheckResult>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![CheckResult::fail("kicad_import.read", path.display().to_string(), format!("{e}"))])?;
    let (mut design, mut model, mut notes) = import_kicad_sch(&text)?;
    let Some(dir) = path.parent() else { return Ok((design, model, notes)) };

    let mut sheet_contents: BTreeMap<String, SchematicSection> = BTreeMap::new();
    let mut read_files: BTreeSet<String> = BTreeSet::new();
    let root_sheets: Vec<(String, std::path::PathBuf)> = design.schematic.as_ref().map(|s| s.sheets.iter().map(|sh| (sh.file.clone(), dir.to_path_buf())).collect()).unwrap_or_default();
    let mut queue: Vec<(String, std::path::PathBuf)> = root_sheets;

    while let Some((file, resolve_dir)) = queue.pop() {
        if !read_files.insert(file.clone()) {
            continue; // same screen reached again through another placement/path -- already have its content
        }
        let child_path = resolve_dir.join(&file);
        let Ok(child_text) = std::fs::read_to_string(&child_path) else { continue };
        let Ok((child_design, child_model, child_notes)) = import_kicad_sch(&child_text) else { continue };
        notes.unresolved_symbols += child_notes.unresolved_symbols;
        model.parts.extend(child_model.parts);
        model.symbols.extend(child_model.symbols);
        // Bus aliases (GAPS.md #20) are project-wide, not per-screen (see
        // `BusAlias`'s own doc) -- every sheet's own legacy `(bus_alias ...)`
        // block feeds the *one* design-level list, by name, first
        // definition wins (matching `SCHEMATIC::AddBusAlias`'s own
        // same-name-same-members dedup intent closely enough: a real
        // project only ever declares one alias per name in practice).
        for alias in child_design.bus_aliases {
            if !design.bus_aliases.iter().any(|a| a.name == alias.name) {
                design.bus_aliases.push(alias);
            }
        }
        if let Some(child_sch) = child_design.schematic {
            let child_dir = child_path.parent().unwrap_or(&resolve_dir).to_path_buf();
            for s in &child_sch.sheets {
                queue.push((s.file.clone(), child_dir.clone()));
            }
            sheet_contents.insert(file, child_sch);
        }
    }

    // Recount, over the whole final tree, rather than trust the
    // per-file incremental tally `import_kicad_sch` left on each piece.
    let mut still_not_descended = 0usize;
    let mut all_screens: Vec<&SchematicSection> = Vec::new();
    if let Some(root) = &design.schematic {
        all_screens.push(root);
    }
    all_screens.extend(sheet_contents.values());
    for screen in &all_screens {
        for s in &screen.sheets {
            if !sheet_contents.contains_key(&s.file) {
                still_not_descended += 1;
            }
        }
    }
    notes.sheets_not_descended = still_not_descended;
    design.sheet_contents = (!sheet_contents.is_empty()).then_some(sheet_contents);

    Ok((design, model, notes))
}

fn import_title_block(root: &[Sexpr]) -> Option<TitleBlock> {
    let tb = sexpr::find(root, "title_block")?;
    let text = |tag: &str| sexpr::find(tb, tag).and_then(|f| sexpr::txt(f, 1)).unwrap_or("").to_string();
    let mut comments = Vec::new();
    for c in sexpr::find_all(tb, "comment") {
        let n: usize = sexpr::txt(c, 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let val = sexpr::txt(c, 2).unwrap_or("").to_string();
        while comments.len() < n {
            comments.push(String::new());
        }
        if n > 0 {
            comments[n - 1] = val;
        }
    }
    while comments.last().is_some_and(|s: &String| s.is_empty()) {
        comments.pop();
    }
    Some(TitleBlock { title: text("title"), date: text("date"), rev: text("rev"), company: text("company"), comments })
}

/// A symbol instance's own `(property "Name" "Value" ...)` text, by name.
fn property_text(item: &[Sexpr], name: &str) -> Option<String> {
    sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1) == Some(name)).and_then(|p| sexpr::txt(p, 2)).map(String::from)
}

/// Every `(path "..." (reference "X") (unit N))` entry inside a placed
/// symbol's own `(instances (project "..." (path ...) ...) ...)` block
/// (real KiCad nests these under one or more `(project ...)` wrappers --
/// walked here regardless of project name, matching real QA data's own
/// inconsistent `""` vs the real project name across sibling symbols in
/// the same file) -- the per-sheet-instance Reference/unit overrides
/// `hierarchy::flatten` applies when a screen is placed more than once.
/// `parent_sheet_instance_id` is the *last* component of the recorded path
/// (`/<root-uuid>/<this-placement's-own-sheet-uuid>` -> the sheet uuid) --
/// see `SymbolPathOverride`'s own doc for why only that one component is
/// needed. A path with no components at all (shouldn't happen in a real
/// file) is skipped, not guessed at.
fn parse_instance_overrides(item: &[Sexpr], at: Point) -> Vec<eda_model::ir::SymbolPathOverride> {
    let Some(instances) = sexpr::find(item, "instances") else { return Vec::new() };
    let mut out = Vec::new();
    for project in sexpr::find_all(instances, "project") {
        for path in sexpr::find_all(project, "path") {
            let Some(path_str) = sexpr::txt(path, 1) else { continue };
            let Some(parent_sheet_instance_id) = path_str.rsplit('/').find(|s| !s.is_empty()) else { continue };
            let Some(reference) = sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1)) else { continue };
            let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0).max(1.0) as u32;
            out.push(eda_model::ir::SymbolPathOverride { at, parent_sheet_instance_id: parent_sheet_instance_id.to_string(), reference: reference.to_string(), unit });
        }
    }
    out
}

fn sheet_property_text(item: &[Sexpr], name: &str) -> Option<String> {
    sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1) == Some(name)).and_then(|p| sexpr::txt(p, 2)).map(String::from)
}

fn point_mm(at: &[Sexpr]) -> Option<SPoint> {
    Some(SPoint::new(sexpr::num(at, 1)?, sexpr::num(at, 2)?))
}

fn mm_point_to_um(p: SPoint) -> Point {
    Point { x: crate::import::mm_to_um(p.x), y: crate::import::mm_to_um(p.y) }
}

fn import_pts(item: &[Sexpr]) -> Option<Vec<Point>> {
    let pts = sexpr::find(item, "pts")?;
    let out: Vec<Point> = sexpr::find_all(pts, "xy").filter_map(|xy| Some(mm_point_to_um(SPoint::new(sexpr::num(xy, 1)?, sexpr::num(xy, 2)?)))).collect();
    (!out.is_empty()).then_some(out)
}

fn label_shape_from_token(t: &str) -> LabelShape {
    match t {
        "input" => LabelShape::Input,
        "output" => LabelShape::Output,
        "bidirectional" => LabelShape::Bidirectional,
        "tri_state" => LabelShape::TriState,
        _ => LabelShape::Passive,
    }
}

/// Inverse of `eda_kicad`'s coarse `PinKind` -> KiCad electrical-type
/// mapping (see `electrical_type` in `lib.rs`): lossy in the same
/// direction that mapping is (KiCad's 12 types collapse onto our 5-value
/// `PinKind`), so a round trip of *our own* export recovers the exact same
/// `PinKind` it started from, while a real KiCad file's richer pin typing
/// (input/output/tri_state/open_collector/...) folds down to `Signal`.
pub fn pin_kind_from_electrical_type(t: &str, name: &str) -> eda_model::PinKind {
    use eda_model::PinKind;
    match t {
        // `power_in` covers both a rail input and a ground pin in KiCad's
        // own type system (confirmed directly from the real `power:GND`
        // library entry); disambiguate the same way `eda_engine::geometry`'s
        // own `is_ground_name` does, so a real AMS1117 GND pin (say) reads
        // back as `Ground`, not `Power`.
        "power_in" if is_ground_name(name) => PinKind::Ground,
        "power_in" | "power_out" => PinKind::Power,
        "passive" => PinKind::Passive,
        "no_connect" => PinKind::Nc,
        _ => PinKind::Signal,
    }
}

fn is_ground_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("GND") || n.contains("VSS") || n.contains("AGND")
}

/// KiCad's schematic `(at x y angle)` rotates a symbol counter-clockwise
/// for positive `angle`, in this crate's own (and this whole codebase's)
/// convention -- the same sense `eda_kicad::lib::baked_local` applies when
/// pre-baking a rotation before writing. Only ever actually exercised at
/// 0 by this project's own generator; kept general for a real file that
/// rotates a symbol.
fn import_rot_millideg_sch(file_deg: f64) -> eda_model::ir::Millideg {
    (file_deg * 1000.0).round().rem_euclid(360_000.0) as eda_model::ir::Millideg
}

/// A library-local point (mm, KiCad's own +y-up symbol frame) to its
/// placed, sheet-space offset from the instance's own origin (mm, +y down)
/// -- everything `eda_kicad::lib::baked_local` does, run forward instead of
/// pre-baked: negate y (library +y-up -> sheet +y-down, see
/// `baked_local`'s own doc comment), rotate, then mirror.
///
/// `mirror_y` (KiCad's `SYM_MIRROR_X`, "Mirror Vertically") does *not*
/// negate `local.y` a second time on top of the always-applied library
/// flip above -- it *cancels* that flip instead, leaving the raw file Y
/// unchanged. Ported from, and cross-checked numerically against,
/// `transform.ts`'s own matrix table (`symbolTransformMatrix`/
/// `resolveLibPoint`, ported line-for-line from `sch_symbol.cpp::
/// SetOrientation` in the same session this function was last touched):
/// at `angle_deg == 0`, `mirror_y` alone must reproduce the point
/// completely unflipped, which only holds if the two negations cancel
/// rather than compound. `mirrored` and `mirror_y` are never both true at
/// once in a real symbol (same "one axis or none" rule `transform.ts`'s
/// own header comment states) -- the composition below is only defined
/// for that case, like source's own.
///
/// Order and sense, verified against source: the parser builds the
/// rotation as `TRANSFORM( 0, 1, -1, 0 )` for a file angle of 90 (`x' = y,
/// y' = -x` on the Y-flipped library point -- a rotation by *minus* the
/// file angle in this Y-down frame), then `SetOrientation( SYM_MIRROR_X /
/// _Y )` composes the mirror *after* it (`newTransform = temp * old`), so
/// the mirror flips the already-rotated point in sheet space.
pub fn transform_local_point(local: SPoint, angle_deg: f64, mirrored: bool, mirror_y: bool) -> SPoint {
    let (x, y) = (local.x, -local.y);
    let theta = (-angle_deg).to_radians();
    let (c, s) = (theta.cos().round_to_unit(), theta.sin().round_to_unit());
    let (mut rx, mut ry) = (x * c - y * s, x * s + y * c);
    if mirrored {
        rx = -rx;
    }
    if mirror_y {
        ry = -ry;
    }
    SPoint::new(rx, ry)
}

/// Snap `cos`/`sin` of a multiple of 90 degrees to exactly -1/0/1, so a
/// quarter-turn never leaves `6.1e-17`-style residue in a pin position.
trait RoundToUnit {
    fn round_to_unit(self) -> f64;
}

impl RoundToUnit for f64 {
    fn round_to_unit(self) -> f64 {
        if (self - self.round()).abs() < 1e-12 {
            self.round()
        } else {
            self
        }
    }
}

// ---------------------------------------------------------------- net reconciliation

/// Tiny union-find over point identities (see the module doc). Returns the
/// rebuilt net list and, in the same pass, backfills every `Wire::net`/
/// `Wire::pins`, `PowerSymbol::pin` and `NoConnect::pin` this reader could
/// not know until connectivity was resolved.
pub fn reconcile(pin_world: &BTreeMap<String, Point>, wires: &mut [Wire], labels: &[eda_model::ir::NetLabel], power_symbols: &mut [PowerSymbol], no_connects: &mut [NoConnect], junctions: &[Point]) -> Vec<Net> {
    let mut point_id: BTreeMap<Point, usize> = BTreeMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let id_of = |p: Point, point_id: &mut BTreeMap<Point, usize>, parent: &mut Vec<usize>| -> usize {
        *point_id.entry(p).or_insert_with(|| {
            parent.push(parent.len());
            parent.len() - 1
        })
    };
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Seed every point this file draws.
    for w in wires.iter() {
        for &p in &w.pts {
            id_of(p, &mut point_id, &mut parent);
        }
    }
    for (_, &p) in pin_world.iter() {
        id_of(p, &mut point_id, &mut parent);
    }
    for l in labels {
        id_of(l.at, &mut point_id, &mut parent);
    }
    for ps in power_symbols.iter() {
        id_of(ps.at, &mut point_id, &mut parent);
    }
    for nc in no_connects.iter() {
        id_of(nc.at, &mut point_id, &mut parent);
    }
    // An explicit junction (`SCH_JUNCTION`) is a seeded point like any other: the T-junction step below then
    // joins it to every wire passing through it, which is what makes two wires that merely cross one net.
    for &j in junctions {
        id_of(j, &mut point_id, &mut parent);
    }

    // Chain each wire's own polyline together.
    for w in wires.iter() {
        for pair in w.pts.windows(2) {
            let a = point_id[&pair[0]];
            let b = point_id[&pair[1]];
            union(&mut parent, a, b);
        }
    }
    // T-junctions: any seeded point landing on the interior of a wire's
    // segment joins that wire's group too.
    let all_points: Vec<Point> = point_id.keys().copied().collect();
    for w in wires.iter() {
        if w.pts.len() < 2 {
            continue;
        }
        let rep = point_id[&w.pts[0]];
        for &p in &all_points {
            if w.pts.contains(&p) {
                continue;
            }
            if w.pts.windows(2).any(|seg| point_on_segment_interior(p, seg[0], seg[1])) {
                union(&mut parent, point_id[&p], rep);
            }
        }
    }

    // Group every point by its root -- purely geometric so far (wire
    // chains, coincident points, T-junctions).
    let geo_groups: Vec<Vec<Point>> = {
        let mut by_root: BTreeMap<usize, Vec<Point>> = BTreeMap::new();
        for (&p, &id) in &point_id {
            by_root.entry(find(&mut parent, id)).or_default().push(p);
        }
        by_root.into_values().collect()
    };

    // A point an explicit no-connect flag sits on never contributes a pin
    // to any net, no matter what's geometrically there -- that is the
    // entire point of the flag (and matches `derive_schematic`, which
    // never puts an `nc`-kind pin on a `Net` either).
    let nc_points: std::collections::BTreeSet<Point> = no_connects.iter().map(|nc| nc.at).collect();

    let mut pins_of_point: BTreeMap<Point, Vec<String>> = BTreeMap::new();
    for (pin_ref, &p) in pin_world {
        if nc_points.contains(&p) {
            continue;
        }
        pins_of_point.entry(p).or_default().push(pin_ref.clone());
    }

    // A label's or power symbol's net name is a net-*wide* identity, not a
    // point-local one: two power symbols both named "GND" are the same net
    // even with no wire between them (exactly how a real schematic's
    // ground net is almost always drawn -- one power symbol per pin, tied
    // together only by sharing that name), the same way two same-named
    // local labels anywhere on the sheet are. So: first find each
    // *geometric* group's own name (if any) and own pins, then merge
    // groups that resolved to the same explicit name; a group with no
    // label/power symbol at all gets its own private synthesized name and
    // never merges with another such group.
    let mut own_pins: Vec<Vec<String>> = Vec::with_capacity(geo_groups.len());
    let mut own_name: Vec<Option<String>> = Vec::with_capacity(geo_groups.len());
    for points in &geo_groups {
        let points_set: std::collections::BTreeSet<Point> = points.iter().copied().collect();
        let label_name = labels.iter().find(|l| points_set.contains(&l.at)).map(|l| l.net.clone());
        let power_name = power_symbols.iter().find(|p| points_set.contains(&p.at)).map(|p| p.net.clone());
        own_name.push(label_name.or(power_name));
        own_pins.push(points.iter().flat_map(|p| pins_of_point.get(p).cloned().unwrap_or_default()).collect());
    }

    let mut anon = 0usize;
    let mut final_name: Vec<String> = Vec::with_capacity(geo_groups.len());
    for name in &own_name {
        final_name.push(match name {
            Some(n) => n.clone(),
            None => {
                anon += 1;
                format!("NET_{anon}")
            }
        });
    }

    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (pins, name) in own_pins.iter().zip(&final_name) {
        by_name.entry(name.clone()).or_default().extend(pins.iter().cloned());
    }
    let mut nets: Vec<Net> = Vec::new();
    for (name, mut pins) in by_name {
        pins.sort();
        pins.dedup();
        if !pins.is_empty() {
            nets.push(Net { name, pins });
        }
    }
    nets.sort_by(|a, b| a.name.cmp(&b.name));

    let mut net_name_of_point: BTreeMap<Point, String> = BTreeMap::new();
    for (points, name) in geo_groups.iter().zip(&final_name) {
        for &p in points {
            net_name_of_point.insert(p, name.clone());
        }
    }

    for w in wires.iter_mut() {
        if let Some(name) = w.pts.first().and_then(|p| net_name_of_point.get(p)) {
            w.net = name.clone();
        }
        w.pins = w.pts.iter().filter_map(|p| pins_of_point.get(p)).flatten().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    }
    for ps in power_symbols.iter_mut() {
        if let Some(refs) = pins_of_point.get(&ps.at) {
            if let Some(r) = refs.first() {
                ps.pin = r.clone();
            }
        }
    }
    for nc in no_connects.iter_mut() {
        if let Some(refs) = pins_of_point.get(&nc.at) {
            if let Some(r) = refs.first() {
                nc.pin = r.clone();
            }
        }
    }

    nets
}

fn point_on_segment_interior(p: Point, a: Point, b: Point) -> bool {
    if a.x == b.x {
        p.x == a.x && p.y > a.y.min(b.y) && p.y < a.y.max(b.y)
    } else if a.y == b.y {
        p.y == a.y && p.x > a.x.min(b.x) && p.x < a.x.max(b.x)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export_kicad_sch, ExportMeta};
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{ConstraintModel, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: None, footprint: Some("Foo:Bar".into()), symbol: None, datasheet: None, pins, body_um: None, edge: None }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "EN", PinKind::Signal), pin("4", "VOUT", PinKind::Power), pin("5", "NC", PinKind::Nc)],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        ConstraintModel { parts: vec![u1, cin, cout], nets: vec![net("VIN", &["U1.1", "CIN.1"]), net("VOUT", &["U1.4", "COUT.1"]), net("GND", &["U1.2", "CIN.2", "COUT.2"])], ..Default::default() }
    }

    /// `transform_local_point`'s `mirror_y` branch, cross-checked
    /// numerically against `transform.ts`'s own matrix table (ported
    /// line-for-line from `sch_symbol.cpp::SetOrientation` on the frontend)
    /// rather than re-derived from a description: at every one of the 4
    /// right-angle rotations, `mirror_y` alone must land exactly where
    /// `symbolTransformMatrix(rot, "x")` (the frontend's own name for this
    /// same axis) does, and `mirrored` alone (the pre-existing axis) must
    /// keep matching `symbolTransformMatrix(rot, "y")` the same way it
    /// already did before this field existed.
    #[test]
    fn transform_local_point_mirror_y_matches_the_frontends_matrix_table() {
        let p = SPoint::new(3.0, 5.0);
        // (angle_deg, mirrored, mirror_y, expected (x, y)) -- expected
        // values hand-computed from transform.ts's MATRICES table (see
        // transform_local_point's own doc comment for the derivation).
        let cases = [
            (0.0, false, true, (3.0, 5.0)), // mirror_y alone at rot 0: unchanged from the raw point
            (0.0, true, false, (-3.0, -5.0)), // mirrored alone at rot 0 (pre-existing, must not regress)
            (90.0, false, true, (-5.0, 3.0)),
            (90.0, true, false, (5.0, -3.0)),
            (180.0, false, true, (-3.0, -5.0)), // 180+mirror_y == 0+mirrored (transform.ts's own documented equivalence)
            (180.0, true, false, (3.0, 5.0)),
            (270.0, false, true, (5.0, -3.0)),
            (270.0, true, false, (-5.0, 3.0)),
            // Plain rotations: `TRANSFORM( 0, 1, -1, 0 )` etc. straight
            // from the parser (internal point = (3, -5)).
            (0.0, false, false, (3.0, -5.0)),
            (90.0, false, false, (-5.0, -3.0)),
            (180.0, false, false, (-3.0, 5.0)),
            (270.0, false, false, (5.0, 3.0)),
        ];
        for (angle_deg, mirrored, mirror_y, (ex, ey)) in cases {
            let got = transform_local_point(p, angle_deg, mirrored, mirror_y);
            assert!(
                (got.x - ex).abs() < 1e-9 && (got.y - ey).abs() < 1e-9,
                "angle={angle_deg} mirrored={mirrored} mirror_y={mirror_y}: got ({}, {}), want ({ex}, {ey})",
                got.x,
                got.y
            );
        }
    }

    /// `(mirror x)`/`(mirror y)` in a real `.kicad_sch` file must land on
    /// the *opposite*-named field here (`sch_io_kicad_sexpr_parser.cpp`'s
    /// own `T_mirror` handling: `(mirror x)` -> `SYM_MIRROR_X`, which this
    /// app calls `mirror_y`; `(mirror y)` -> `SYM_MIRROR_Y`, this app's
    /// pre-existing `mirrored` -- see `transform_local_point`'s own doc).
    /// This app's own exporter never emits the tag at all (it bakes
    /// mirror/rotation into bytes directly instead -- `baked_local`'s own
    /// doc), so this test hand-injects the tag into our own round-trip
    /// export rather than being able to exercise it through `parses_our_
    /// own_export` directly.
    #[test]
    fn mirror_x_and_mirror_y_tags_read_back_to_the_opposite_named_field() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "mirror_roundtrip")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "mirror_roundtrip" }).unwrap();
        assert!(!text.contains("(mirror "), "sanity: our own exporter never emits a mirror tag on the instance (it bakes the transform instead)");

        let with_mirror_x = text.replacen("(lib_id \"eda:U1\") (at ", "(lib_id \"eda:U1\") (mirror x) (at ", 1);
        let (design_x, _, _) = import_kicad_sch(&with_mirror_x).expect("parses with an injected (mirror x)");
        let u1_x = design_x.schematic.unwrap().symbols.into_iter().find(|s| s.id == "U1").unwrap();
        assert!(u1_x.mirror_y, "(mirror x) must set mirror_y");
        assert!(!u1_x.mirrored, "(mirror x) must not also set mirrored");

        let with_mirror_y = text.replacen("(lib_id \"eda:U1\") (at ", "(lib_id \"eda:U1\") (mirror y) (at ", 1);
        let (design_y, _, _) = import_kicad_sch(&with_mirror_y).expect("parses with an injected (mirror y)");
        let u1_y = design_y.schematic.unwrap().symbols.into_iter().find(|s| s.id == "U1").unwrap();
        assert!(u1_y.mirrored, "(mirror y) must set mirrored");
        assert!(!u1_y.mirror_y, "(mirror y) must not also set mirror_y");
    }

    #[test]
    fn parses_our_own_export() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "roundtrip")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "roundtrip" }).unwrap();
        let (back_design, back_model, notes) = import_kicad_sch(&text).expect("parses our own export");
        assert_eq!(notes.unresolved_symbols, 0, "every lib_id we write is embedded in lib_symbols");
        let sch = back_design.schematic.expect("schematic section");
        assert_eq!(sch.symbols.len(), 3, "U1, CIN, COUT");
        assert!(!sch.power_symbols.is_empty(), "GND pins round-trip as power symbols");
        assert_eq!(back_model.parts.len(), 3);
    }

    #[test]
    fn recovers_the_declared_nets() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "roundtrip2")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "roundtrip2" }).unwrap();
        let (_, back_model, _) = import_kicad_sch(&text).unwrap();
        let gnd = back_model.nets.iter().find(|n| n.name == "GND").expect("GND net recovered");
        let mut pins = gnd.pins.clone();
        pins.sort();
        assert_eq!(pins, vec!["CIN.2".to_string(), "COUT.2".to_string(), "U1.2".to_string()]);
    }

    /// A ground pin (`kind: ground`) and a plain power pin (`kind: power`)
    /// both map to KiCad's real `power_in` electrical type in the file
    /// (confirmed against the real `power:GND` library entry); reading
    /// that back must still recover `Ground`, not collapse it into
    /// `Power`, by using the pin's own name the same way
    /// `eda_engine::geometry::is_ground_name` does.
    #[test]
    fn ground_pin_kind_survives_round_trip_not_just_power() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "roundtrip4")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "roundtrip4" }).unwrap();
        let (_, back_model, _) = import_kicad_sch(&text).unwrap();
        let u1 = back_model.part("U1").expect("U1 recovered");
        let gnd_pin = u1.pins.iter().find(|p| p.number == "2").expect("U1 pin 2 (GND)");
        assert_eq!(gnd_pin.kind, PinKind::Ground, "GND pin must not collapse into Power on read-back");
        let vin_pin = u1.pins.iter().find(|p| p.number == "1").expect("U1 pin 1 (VIN)");
        assert_eq!(vin_pin.kind, PinKind::Power);
    }

    #[test]
    fn nc_pin_is_not_forced_onto_any_net() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "roundtrip3")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "roundtrip3" }).unwrap();
        let (_, back_model, _) = import_kicad_sch(&text).unwrap();
        assert!(back_model.nets.iter().all(|n| !n.pins.contains(&"U1.5".to_string())));
    }

    /// `U1` above is a *synthetic* box (no real library resolves for it),
    /// so the writer already spells its unused pin's electrical type
    /// `"no_connect"` and the read-back above is almost too easy. A real
    /// library symbol is not so obliging: every `Connector_Generic` pin is
    /// generically `"passive"` (see `eda_model::symbol::conn_01x`) whether
    /// or not this project marked it `nc` -- `examples/ldo.yaml`'s own `J1`
    /// pin 4 is exactly this shape, and reading it back as `Passive`
    /// (losing the `nc` semantic) left it with no drawn no-connect flag on
    /// re-export, which `kicad-cli sch erc` then reported as a genuine
    /// dangling `pin_not_connected` error after a round trip. The fix: a
    /// `no_connect` flag sitting exactly on the pin's own point overrides
    /// whatever the library's electrical type says.
    #[test]
    fn nc_pin_kind_survives_round_trip_through_a_real_library_symbol() {
        let j1 = part("J1", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "VOUT", PinKind::Power), pin("4", "NC", PinKind::Nc)]);
        let u1 = part("U1", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "VOUT", PinKind::Power)]);
        let model = ConstraintModel {
            parts: vec![u1, j1],
            nets: vec![net("VIN", &["U1.1", "J1.1"]), net("GND", &["U1.2", "J1.2"]), net("VOUT", &["U1.3", "J1.3"])],
            ..Default::default()
        };
        let design = derive_schematic(&model, &EngineOptions::new(1, "nc-real-lib")).unwrap();
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "nc-real-lib" }).unwrap();
        let (_, back_model, _) = import_kicad_sch(&text).unwrap();
        let j1_back = back_model.part("J1").expect("J1 recovered");
        let nc_pin = j1_back.pins.iter().find(|p| p.number == "4").expect("J1 pin 4 recovered");
        assert_eq!(nc_pin.kind, PinKind::Nc, "a no_connect-flagged pin on a real (generically-'passive') library symbol must read back as Nc, not Passive");
        assert!(back_model.nets.iter().all(|n| !n.pins.contains(&"J1.4".to_string())));
    }

    #[test]
    fn rejects_non_schematic_input() {
        assert!(import_kicad_sch("(kicad_pcb (version 1))").is_err());
    }

    /// GAPS.md #21: a multi-unit part -- one reference, two placed units,
    /// each at its own position -- survives `export_kicad_sch` ->
    /// `import_kicad_sch` as one `Part` with every one of the real
    /// symbol's pins, and two `SymbolInstance`s that keep their own
    /// `unit`/position apart (the exact bug `reconcile`'s own per-unit pin
    /// filter, and this module's grouped-by-reference symbol loop, exist
    /// to fix -- see both their own doc comments).
    #[test]
    fn multi_unit_symbol_round_trips_through_export_and_import() {
        use eda_model::symbol::{LibPin, LibSymbol, SPoint};
        let p = |number: &str, name: &str, etype: &str, x: f64, angle: f64, unit: u32| LibPin {
            number: number.into(),
            name: name.into(),
            electrical_type: etype.into(),
            shape: "line".into(),
            at: SPoint::new(x, 1.27),
            angle_deg: angle,
            length_mm: 1.27,
            unit,
        };
        let lib = LibSymbol {
            lib_id: "test:DUAL".into(),
            graphics: vec![],
            pins: vec![
                p("1", "A1", "input", -2.54, 0.0, 1),
                p("2", "A2", "input", -2.54, 0.0, 1),
                p("3", "Y1", "output", 2.54, 180.0, 1),
                p("4", "A3", "input", -2.54, 0.0, 2),
                p("5", "A4", "input", -2.54, 0.0, 2),
                p("6", "Y2", "output", 2.54, 180.0, 2),
            ],
            power: false,
            in_bom: true,
            on_board: true,
            datasheet: String::new(),
            description: "Dual gate".into(),
            reference_prefix: "U".into(),
            unit_count: 2,
        };
        let u1 = part(
            "U1",
            vec![
                pin("1", "A1", PinKind::Signal),
                pin("2", "A2", PinKind::Signal),
                pin("3", "Y1", PinKind::Signal),
                pin("4", "A3", PinKind::Signal),
                pin("5", "A4", PinKind::Signal),
                pin("6", "Y2", PinKind::Signal),
            ],
        );
        let model = ConstraintModel { parts: vec![u1], symbols: vec![lib], ..Default::default() };

        let sym = |unit: u32, x: i64| SymbolInstance { id: "U1".into(), at: Point { x, y: 0 }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:DUAL".into(), unit, value: "DUAL".into(), footprint: String::new(), datasheet: String::new() };
        let sch = SchematicSection {
            symbols: vec![sym(1, 0), sym(2, 50_000)],
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![], bus_entries: vec![],
            erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(),
            title_block: None,
            sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(),
            imported_from_kicad: false,
        };
        let design = Design { schema: 1, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None };

        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "multi_unit" }).unwrap();
        assert!(text.contains("(unit 1)"), "{text}");
        assert!(text.contains("(unit 2)"), "{text}");

        let (back_design, back_model, notes) = import_kicad_sch(&text).expect("round trips");
        assert_eq!(notes.unresolved_symbols, 0);
        let back_sch = back_design.schematic.expect("schematic section");
        assert_eq!(back_sch.symbols.len(), 2, "both placed units survive the round trip");
        assert!(back_sch.symbols.iter().all(|s| s.id == "U1"));
        let units: std::collections::BTreeSet<u32> = back_sch.symbols.iter().map(|s| s.unit).collect();
        assert_eq!(units, std::collections::BTreeSet::from([1, 2]));

        // One Part, every one of the real symbol's 6 pins -- not split per
        // unit, not duplicated (this task's own "one part, one footprint,
        // several placed units" framing).
        assert_eq!(back_model.parts.len(), 1);
        assert_eq!(back_model.parts[0].pins.len(), 6);

        // Unit 2's own pins must resolve at unit 2's own position, not
        // unit 1's -- the exact bug a naive "position every pin at
        // whichever instance's own id we see last" implementation would
        // get wrong.
        let unit1 = back_sch.symbols.iter().find(|s| s.unit == 1).unwrap();
        let unit2 = back_sch.symbols.iter().find(|s| s.unit == 2).unwrap();
        assert_eq!(unit1.at.x, 0);
        assert_eq!(unit2.at.x, 50_000);
    }

    /// GAPS.md #6: `import_kicad_sch_tree` actually follows a `(sheet ...)`
    /// placement onto disk and reads the file it names, unlike bare
    /// `import_kicad_sch` (which only ever records that a sheet was there).
    /// The child file here is this crate's own ordinary export (already
    /// covered by `parses_our_own_export`); this test is about the
    /// multi-file *walk* on top of it, not re-proving single-file import.
    #[test]
    fn import_kicad_sch_tree_descends_into_a_sibling_sheet_file() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_sch_tree_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let child_model = ConstraintModel { parts: vec![part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)])], ..Default::default() };
        let child_design = derive_schematic(&child_model, &EngineOptions::new(1, "tree_child")).unwrap();
        let child_text = export_kicad_sch(&child_design, &child_model, &ExportMeta { date: "2026-01-01", title: "child" }).unwrap();
        std::fs::write(dir.join("child.kicad_sch"), child_text).unwrap();

        let root_text = r#"(kicad_sch
	(version 20250114)
	(generator "test")
	(uuid "11111111-1111-1111-1111-111111111111")
	(paper "A4")
	(sheet
		(at 100 100) (size 20 20)
		(stroke (width 0.1524) (type solid))
		(fill (color 255 255 194 1.0000))
		(uuid "22222222-2222-2222-2222-222222222222")
		(property "Sheetname" "child" (at 100 99 0) (effects (font (size 1.27 1.27))))
		(property "Sheetfile" "child.kicad_sch" (at 100 121 0) (effects (font (size 1.27 1.27))))
		(pin "AD0" passive
			(at 100 110 180)
			(uuid "33333333-3333-3333-3333-333333333333")
			(effects (font (size 1.27 1.27)) (justify left)))
		(instances (project "test" (path "/11111111-1111-1111-1111-111111111111" (page "2")))))
	(sheet_instances (path "/" (page "1"))))
"#;
        std::fs::write(dir.join("root.kicad_sch"), root_text).unwrap();

        let (design, model, notes) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).expect("tree import succeeds");
        assert_eq!(notes.sheets_not_descended, 0, "the one sheet named was successfully read");
        let root_sch = design.schematic.expect("root schematic");
        assert_eq!(root_sch.sheets.len(), 1);
        assert_eq!(root_sch.sheets[0].file, "child.kicad_sch");
        assert_eq!(root_sch.sheets[0].pins.len(), 1);
        assert_eq!(root_sch.sheets[0].pins[0].name, "AD0");

        let screens = design.sheet_contents.expect("sheet_contents populated");
        let child = screens.get("child.kicad_sch").expect("child content present");
        assert_eq!(child.symbols.len(), 1);
        assert_eq!(child.symbols[0].id, "R1");
        assert!(model.parts.iter().any(|p| p.reference == "R1"), "child's own Part folded into the combined model");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real KiCad data, not a hand-built fixture: `topology_mismatch.kicad_sch`
    /// places `i2c_thingy.kicad_sch` *twice* (ch0/ch1), each instance
    /// giving its own shared symbols a different Reference via per-path
    /// `(instances (project ... (path ...)))` overrides (confirmed directly
    /// against the file this session's own research read: the same drawn
    /// `Device:R`/`Interface_Expansion:MAX7325AEG+` show up as "R1"/"U1"
    /// through one sheet path and "R2"/"U2" through the other). Skipped
    /// gracefully, like every other QA-corpus test in this crate, when the
    /// corpus isn't present in this environment.
    #[test]
    fn real_qa_shared_screen_gets_a_different_reference_per_sheet_instance() {
        let root = std::path::PathBuf::from("/private/tmp/claude-501/-Users-juanantonioluera-ws/8eb77140-1019-4605-b5f4-960e15f5bf6d/scratchpad/kicad_qa_boards/qa/data/pcbnew/issue21739/topology_mismatch.kicad_sch");
        if !root.exists() {
            eprintln!("QA corpus not found at {}; skipping", root.display());
            return;
        }
        let (design, _model, notes) = import_kicad_sch_tree(&root).expect("real QA file imports");
        assert_eq!(notes.sheets_not_descended, 0, "both ch0/ch1 placements of i2c_thingy.kicad_sch are the same file, read once");
        assert_eq!(design.schematic.as_ref().expect("root schematic").sheets.len(), 2, "ch0 and ch1");
        {
            let screens = design.sheet_contents.as_ref().expect("sheet_contents populated");
            let child = screens.get("i2c_thingy.kicad_sch").expect("child content present");
            assert!(!child.instance_overrides.is_empty(), "a twice-placed screen must carry per-instance overrides");
        }
    }

    /// A bus wire, a bus entry, and a bus alias (GAPS.md #20) all survive
    /// `export_kicad_sch` -> `import_kicad_sch` -- no real symbols/parts
    /// needed, this is purely about the three new sexpr shapes
    /// (`crate::lib`'s writer / this module's own `(bus ...)`/`(bus_entry
    /// ...)`/`(bus_alias ...)` readers above) agreeing with each other.
    #[test]
    fn bus_wire_entry_and_alias_round_trip_through_export_and_import() {
        let bus_pt = Point { x: 10_000, y: 10_000 };
        let net_pt = Point { x: 12_540, y: 12_540 };
        let sch = SchematicSection {
            wires: vec![Wire { id: String::new(), net: "DATA[0..3]".into(), pins: vec![], pts: vec![Point { x: 0, y: 10_000 }, bus_pt], bus: true }],
            bus_entries: vec![eda_model::ir::BusEntry { id: String::new(), at: bus_pt, size: Point { x: 2_540, y: 2_540 } }],
            labels: vec![eda_model::ir::NetLabel { id: String::new(), net: "DATA2".into(), at: net_pt, kind: LabelKind::Local }],
            ..SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
        };
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![eda_model::ir::BusAlias { name: "USB".into(), members: vec!["D+".into(), "D-".into()] }],
            symbol_library: None,
        };
        let model = ConstraintModel::default();
        let text = crate::export_kicad_sch(&design, &model, &crate::ExportMeta { date: "2026-01-01", title: "bus test" }).expect("exports cleanly with zero symbols");

        let (reimported, _model, _notes) = import_kicad_sch(&text).expect("re-imports cleanly");
        let sch = reimported.schematic.expect("schematic present");
        assert_eq!(sch.wires.len(), 1);
        assert!(sch.wires[0].bus, "the bus wire's own layer flag must survive the round trip");
        assert_eq!(sch.wires[0].pts, vec![Point { x: 0, y: 10_000 }, bus_pt]);
        assert_eq!(sch.bus_entries.len(), 1);
        assert_eq!(sch.bus_entries[0].at, bus_pt);
        assert_eq!(sch.bus_entries[0].size, Point { x: 2_540, y: 2_540 });
        assert_eq!(reimported.bus_aliases.len(), 1);
        assert_eq!(reimported.bus_aliases[0].name, "USB");
        assert_eq!(reimported.bus_aliases[0].members, vec!["D+".to_string(), "D-".to_string()]);
    }

    /// Two wires that cross (neither ends on the other) are two nets -- until an explicit junction (`J`,
    /// `SCH_JUNCTION`) sits at the crossing. The junction and a notes-layer line survive the file round trip.
    #[test]
    fn an_explicit_junction_joins_crossing_wires_and_junctions_and_graphic_lines_round_trip() {
        let cross = Point { x: 10_000, y: 10_000 };
        let build = |junctions: Vec<eda_model::ir::Junction>| {
            let sch = SchematicSection {
                wires: vec![
                    Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![Point { x: 0, y: 10_000 }, Point { x: 20_000, y: 10_000 }], bus: false },
                    Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 20_000 }], bus: false },
                ],
                labels: vec![
                    eda_model::ir::NetLabel { id: String::new(), net: "H".into(), at: Point { x: 0, y: 10_000 }, kind: LabelKind::Local },
                    eda_model::ir::NetLabel { id: String::new(), net: "V".into(), at: Point { x: 10_000, y: 0 }, kind: LabelKind::Local },
                ],
                junctions,
                lines: vec![eda_model::ir::SchLine { id: String::new(), pts: vec![Point { x: 0, y: 30_000 }, Point { x: 5_000, y: 30_000 }, Point { x: 5_000, y: 35_000 }], width_um: 254 }],
                ..SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
            };
            let design = Design {
                schema: 1,
                provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
                schematic: Some(sch),
                nets: None,
                placement: None,
                routing: None,
                drawings: None,
                footprint_library: None,
                sheet_contents: None,
                bus_aliases: vec![],
                symbol_library: None,
            };
            let text = crate::export_kicad_sch(&design, &ConstraintModel::default(), &crate::ExportMeta { date: "2026-01-01", title: "junction test" }).expect("exports cleanly with zero symbols");
            let (reimported, _model, _notes) = import_kicad_sch(&text).expect("re-imports cleanly");
            (text, reimported.schematic.expect("schematic present"))
        };

        let (_, apart) = build(vec![]);
        let net_of = |sch: &SchematicSection, first: Point| sch.wires.iter().find(|w| w.pts[0] == first).unwrap().net.clone();
        assert_ne!(net_of(&apart, Point { x: 0, y: 10_000 }), net_of(&apart, Point { x: 10_000, y: 0 }), "crossing wires with no junction stay two nets");
        assert!(apart.junctions.is_empty(), "no junction written or read back");

        let (text, joined) = build(vec![eda_model::ir::Junction { id: String::new(), at: cross }]);
        assert_eq!(net_of(&joined, Point { x: 0, y: 10_000 }), net_of(&joined, Point { x: 10_000, y: 0 }), "the junction joins them");
        assert!(text.contains("(junction (at 10 10)"), "{text}");
        assert_eq!(joined.junctions.len(), 1);
        assert_eq!(joined.junctions[0].at, cross);

        assert!(text.contains("(polyline"), "{text}");
        assert_eq!(joined.lines.len(), 1);
        assert_eq!(joined.lines[0].pts, vec![Point { x: 0, y: 30_000 }, Point { x: 5_000, y: 30_000 }, Point { x: 5_000, y: 35_000 }]);
        assert_eq!(joined.lines[0].width_um, 254);
        assert_eq!(joined.wires.len(), 2, "a graphic line is never read back as a wire");
    }
}
