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

use eda_model::units::Nm;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use eda_model::ir::{Design, LabelKind, LabelShape, Millideg, NoConnect, Point, PowerSymbol, Provenance, SchematicSection, SchematicText, SheetInstance, SymbolInstance, TitleBlock, Wire};
use eda_model::sch_extras::{JunctionLook, SchStroke};
use eda_model::symbol::{LibPin, LibSymbol, SPoint};
use eda_model::{CheckResult, ConstraintModel, Net, Part, Pin};

use crate::sch_extras_io::{parse_bool, read_effects, uuid_of, EffectsLook};
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
    /// Each item's uuid in the file -> the id the import gave it (a symbol's and a power symbol's reference, a pin's `REF.PIN`, a wire's, a
    /// label's, ...), the first one where a uuid stands for several: what a project's ERC exclusions name their items by
    /// (`crate::erc_exclusions::from_project`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub item_ids: BTreeMap<String, String>,
}

/// How a file is read, beyond what a file says.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImportOptions {
    /// A symbol with no number (`R?`), and one that repeats the reference and unit another has, is given a placeholder reference of its own
    /// (`R?1`, `R?2`, ...; the units of one unnumbered part share one), so that a design -- which names a part by its reference -- can hold it and
    /// Annotate can number it. Off, every symbol of one reference is one part, as a file's own `Reference` says (the project importer's reading).
    pub unique_references: bool,
}

pub fn import_kicad_sch(text: &str) -> Result<(Design, ConstraintModel, SchImportNotes), Vec<CheckResult>> {
    import_kicad_sch_with(text, &ImportOptions::default())
}

/// [`import_kicad_sch`] with the choices of [`ImportOptions`].
pub fn import_kicad_sch_with(text: &str, opts: &ImportOptions) -> Result<(Design, ConstraintModel, SchImportNotes), Vec<CheckResult>> {
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
    // `(locked yes)` of each item, parallel to the vector it was read into -- resolved to ids once ids exist (see below).
    let is_locked = |node: &[Sexpr]| parse_bool(node, "locked").unwrap_or(false);
    let mut nc_locked: Vec<bool> = Vec::new();
    // the `(uuid ..)` of each item, parallel to the vector it was read into: a `(group ..)` names its members by them (see below)
    let mut nc_uuid: Vec<Option<String>> = Vec::new();
    for nc in sexpr::find_all(root, "no_connect") {
        if let Some(at) = sexpr::find(nc, "at").and_then(point_mm) {
            no_connects.push(NoConnect { id: String::new(), at: mm_point_to_um(at), pin: String::new() });
            nc_locked.push(is_locked(nc));
            nc_uuid.push(uuid_of(nc));
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
        at_nm: Point,
        rot: Millideg,
        angle_deg: f64,
        mirrored: bool,
        mirror_y: bool,
        unit: u32,
        /// `(body_style N)` (`(convert N)` in a file before KiCad 10): 1, the normal body style, unless the symbol is placed in its alternate one.
        body_style: u32,
        reference: String,
        value: String,
        footprint: String,
        datasheet: String,
        dnp: bool,
        exclude_from_bom: bool,
        exclude_from_board: bool,
        exclude_from_sim: bool,
        uuid: Option<String>,
        /// The properties beyond the standard fields: user-defined fields (`FIELD_T::USER`), with where and how each is drawn.
        user_fields: Vec<RawUserField>,
        /// The places Swap Pins traded, read from the symbol's own copy of its library symbol (`(lib_name ..)`): pin number -> the pin whose place it has (`SchExtras::pin_swaps`).
        swaps: BTreeMap<String, String>,
    }
    let mut raw: Vec<RawInstance> = Vec::new();
    // each power symbol's `(uuid ..)`, parallel to `power_symbols`
    let mut power_uuid: Vec<Option<String>> = Vec::new();
    let mut locked_refs: BTreeSet<String> = BTreeSet::new();
    // `SCH_SYMBOL_INSTANCE`-style per-sheet-instance overrides -- only ever
    // non-empty when this screen is placed by more than one `SheetInstance`
    // (see `SymbolPathOverride`'s own doc). Parsed unconditionally here
    // (not just when this read is part of a multi-file tree walk) because
    // the data lives in *this* file's own `(symbol ... (instances ...))`
    // blocks regardless of whether the caller goes on to assemble a tree
    // around it -- a bare single-file `import_kicad_sch` on a screen that
    // happens to be shared just carries overrides nothing yet queries.
    let mut overrides: Vec<eda_model::ir::SymbolPathOverride> = Vec::new();
    // (place, own unit, sheet path, the unit on that path) of a symbol whose `(path .. (unit N))` is not its own `(unit ..)` (r2 B-M4)
    let mut path_units: Vec<(Point, u32, Vec<String>, u32)> = Vec::new();

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
        // `SCH_IO_KICAD_SEXPR_PARSER::parseSchematicSymbol`: `case T_convert` (legacy) and `case T_body_style` both read `SetBodyStyle`
        let body_style = sexpr::find(item, "body_style").or_else(|| sexpr::find(item, "convert")).and_then(|b| sexpr::num(b, 1)).unwrap_or(1.0).max(1.0) as u32;
        let at_nm = Point { x: crate::import::sch_mm_to_nm(x_mm), y: crate::import::sch_mm_to_nm(y_mm) };
        let rot = import_rot_millideg_sch(angle_deg);
        // `(lib_name "<cache>")`: the symbol's own copy of its library symbol in the sheet's cache, which Swap Pins gave other places for some of its pins
        let swaps = sexpr::find(item, "lib_name").and_then(|l| sexpr::txt(l, 1)).filter(|name| *name != lib_id).and_then(|name| lib_table.get(name)).map(|cached| pin_swaps_between(resolved, cached, unit, body_style)).unwrap_or_default();

        let reference = property_text(item, "Reference").unwrap_or_default();
        let value = property_text(item, "Value").unwrap_or_default();
        let footprint = property_text(item, "Footprint").unwrap_or_default();
        let datasheet = property_text(item, "Datasheet").unwrap_or_default();
        // `(dnp yes) (in_bom no) (on_board no) (exclude_from_sim yes)`: the attributes `SetAttribute` toggles. A file without the token (KiCad 6)
        // has the default: populated, in the BOM, on the board, simulated.
        let flag = |name: &str| sexpr::find(item, name).and_then(|n| sexpr::txt(n, 1));
        let (dnp, exclude_from_bom, exclude_from_board, exclude_from_sim) = (flag("dnp") == Some("yes"), flag("in_bom") == Some("no"), flag("on_board") == Some("no"), flag("exclude_from_sim") == Some("yes"));

        if is_locked(item) {
            locked_refs.insert(reference.clone());
        }
        if resolved.power {
            let pin_local = resolved.pins.first().map(|p| p.at).unwrap_or(SPoint::new(0.0, 0.0));
            let world = transform_local_point(pin_local, angle_deg, mirrored, mirror_y);
            let at_pin = Point { x: at_nm.x + crate::import::sch_mm_to_nm(world.x), y: at_nm.y + crate::import::sch_mm_to_nm(world.y) };
            power_symbols.push(PowerSymbol { id: reference, lib_id: lib_id.to_string(), at: at_pin, rot, net: value, pin: String::new() });
            power_uuid.push(uuid_of(item));
            continue;
        }

        for (o, path_unit) in parse_instance_overrides(item, at_nm, unit) {
            if path_unit != unit {
                path_units.push((at_nm, unit, o.sheet_path.clone(), path_unit));
            }
            overrides.push(o);
        }
        raw.push(RawInstance { lib_id: lib_id.to_string(), at_nm, rot, angle_deg, mirrored, mirror_y, unit, body_style, reference, value, footprint, datasheet, dnp, exclude_from_bom, exclude_from_board, exclude_from_sim, uuid: uuid_of(item), user_fields: user_fields_of(item), swaps });
    }

    if opts.unique_references {
        // Symbols that cannot be told apart by their reference get a placeholder of their own (`R?1`, `R?2`): one with no number, and one that
        // repeats the reference and unit another has. The units of an unnumbered part share a placeholder (a unit joins the first of its
        // prefix, library symbol and value that does not have it yet); Annotate numbers them afterwards.
        let mut seen: BTreeSet<(String, u32)> = BTreeSet::new();
        let mut next: BTreeMap<String, u32> = BTreeMap::new();
        let mut holders: Vec<(String, String, String, BTreeSet<u32>)> = Vec::new(); // (placeholder, lib_id, value, units)
        for r in raw.iter_mut() {
            let numbered = r.reference.ends_with(|c: char| c.is_ascii_digit()) && !r.reference.contains('?');
            let repeated = numbered && seen.contains(&(r.reference.clone(), r.unit));
            if numbered && !repeated {
                seen.insert((r.reference.clone(), r.unit));
                continue;
            }
            let prefix: String = r.reference.trim_end_matches(|c: char| c.is_ascii_digit() || c == '?').to_string();
            let prefix = if prefix.is_empty() { "U".to_string() } else { prefix };
            let joined = (!repeated).then(|| holders.iter_mut().find(|(name, lib, value, units)| name.starts_with(&format!("{prefix}?")) && *lib == r.lib_id && *value == r.value && !units.contains(&r.unit))).flatten();
            match joined {
                Some((name, _, _, units)) => {
                    units.insert(r.unit);
                    r.reference = name.clone();
                }
                None => {
                    let n = next.entry(prefix.clone()).or_default();
                    *n += 1;
                    let name = format!("{prefix}?{n}");
                    holders.push((name.clone(), r.lib_id.clone(), r.value.clone(), BTreeSet::from([r.unit])));
                    r.reference = name;
                }
            }
        }
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

    // `(body_style 2)` of the symbols placed in the alternate body style of a library symbol that has one, by `field_key` (`SchExtras::body_styles`)
    let mut body_style_entries: Vec<(String, u32)> = Vec::new();
    // the places Swap Pins traded, by `field_key` (`SchExtras::pin_swaps`)
    let mut pin_swap_entries: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    // the user-defined fields of the symbols (`SchematicSection::user_fields`, by reference) and where each unit draws them (`field_layout`, by `field_key`)
    let mut user_fields_read: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut field_layout_read: BTreeMap<String, Vec<eda_model::ir::FieldPlacement>> = BTreeMap::new();
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
                        // the pin where the owner's body style puts it (`SCH_SYMBOL::UpdatePins`: "the body style may have a different pin layout")
                        let styled = (owner.body_style > 1 && resolved.has_alternate_body()).then(|| resolved.in_style(owner.body_style));
                        // and the place it has: its own, or the one of the pin it traded with (`SwapPinGeometry`, read from the symbol's own copy)
                        let source = owner.swaps.get(&p.number).map(String::as_str).unwrap_or(p.number.as_str());
                        let place_pins: &[LibPin] = styled.as_ref().map_or(&resolved.pins, |s| &s.pins);
                        let local = if source == p.number.as_str() {
                            styled.as_ref().and_then(|s| s.pins.iter().find(|q| q.number == p.number && (q.unit == 0 || q.unit == owner.unit))).map_or(p.at, |q| q.at)
                        } else {
                            place_pins.iter().find(|q| q.number == source && (q.unit == 0 || q.unit == owner.unit)).map_or(p.at, |q| q.at)
                        };
                        let world = transform_local_point(local, owner.angle_deg, owner.mirrored, owner.mirror_y);
                        let at_pin = Point { x: owner.at_nm.x + crate::import::sch_mm_to_nm(world.x), y: owner.at_nm.y + crate::import::sch_mm_to_nm(world.y) };
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
            body_nm: None,
            edge: None,
        });
        for &i in idxs {
            let r = &raw[i];
            if r.body_style > 1 && lib_table.get(r.lib_id.as_str()).is_some_and(|l| l.has_alternate_body()) {
                body_style_entries.push((eda_model::ir::field_key(&reference, r.unit), r.body_style.min(2)));
            }
            if !r.swaps.is_empty() {
                pin_swap_entries.push((eda_model::ir::field_key(&reference, r.unit), r.swaps.clone()));
            }
            symbols.push(SymbolInstance {
                id: reference.clone(),
                at: r.at_nm,
                rot: r.rot,
                mirrored: r.mirrored,
                mirror_y: r.mirror_y,
                lib_id: r.lib_id.clone(),
                unit: r.unit,
                value: r.value.clone(),
                footprint: r.footprint.clone(),
                datasheet: r.datasheet.clone(),
                dnp: r.dnp,
                exclude_from_bom: r.exclude_from_bom,
                exclude_from_board: r.exclude_from_board,
                exclude_from_sim: r.exclude_from_sim,
            });
            if !r.user_fields.is_empty() {
                // the text of a user field is the symbol's, whatever the unit; the place is each unit's own, in the frame the writer measures it in
                // (`eda_engine::fields::page_field`: from the symbol's origin, through its turn and mirror and the width of the box a mirror is about)
                let part = parts.last().expect("the part of this reference");
                let sym = symbols.last().expect("the symbol just pushed");
                let lib_for_geometry = resolved.map(|l| if r.body_style > 1 && l.has_alternate_body() { l.in_style(r.body_style).into_owned() } else { l.clone() });
                let width = eda_engine::symgeom::SymbolGeom::of(sym, part, lib_for_geometry.as_ref()).width;
                for u in &r.user_fields {
                    user_fields_read.entry(reference.clone()).or_default().entry(u.name.clone()).or_insert_with(|| u.text.clone());
                }
                field_layout_read.insert(eda_model::ir::field_key(&reference, r.unit), r.user_fields.iter().map(|u| u.placement(sym, width)).collect());
            }
        }
    }

    let mut wires: Vec<Wire> = Vec::new();
    let mut wire_locked: Vec<bool> = Vec::new();
    let mut wire_uuid: Vec<Option<String>> = Vec::new();
    // each wire's, bus's, bus entry's and polyline's `(stroke ...)` and each junction's `(diameter ...) (color ...)`, kept beside its lock flag
    let mut wire_stroke: Vec<SchStroke> = Vec::new();
    for w in sexpr::find_all(root, "wire") {
        if let Some(pts) = import_pts(w) {
            wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus: false });
            wire_locked.push(is_locked(w));
            wire_uuid.push(uuid_of(w));
            wire_stroke.push(stroke_of(w));
        }
    }
    // `(bus ...)` (GAPS.md #20): the exact same `SCH_LINE`/`(pts ...)` shape
    // as `(wire ...)`, just `LAYER_BUS` instead of `LAYER_WIRE` -- see
    // `Wire::bus`'s own doc.
    for b in sexpr::find_all(root, "bus") {
        if let Some(pts) = import_pts(b) {
            wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus: true });
            wire_locked.push(is_locked(b));
            wire_uuid.push(uuid_of(b));
            wire_stroke.push(stroke_of(b));
        }
    }

    let mut bus_entries: Vec<eda_model::ir::BusEntry> = Vec::new();
    let mut be_locked: Vec<bool> = Vec::new();
    let mut be_stroke: Vec<SchStroke> = Vec::new();
    for be in sexpr::find_all(root, "bus_entry") {
        let Some(at) = sexpr::find(be, "at").and_then(point_mm) else { continue };
        let Some(size) = sexpr::find(be, "size").and_then(|sz| Some((sexpr::num(sz, 1)?, sexpr::num(sz, 2)?))) else { continue };
        let id = sexpr::find(be, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
        bus_entries.push(eda_model::ir::BusEntry { id, at: mm_point_to_um(at), size: Point { x: crate::import::sch_mm_to_nm(size.0), y: crate::import::sch_mm_to_nm(size.1) } });
        be_locked.push(is_locked(be));
        be_stroke.push(stroke_of(be));
    }

    // `(junction (at x y) ...)`: an explicit junction (`SCH_JUNCTION`) -- the only thing that joins two wires that merely cross
    // (see `Junction`; `reconcile` seeds it). Junctions the writer derives from geometry (`wire_junction_points`) come back as
    // explicit ones too: harmless, they sit where the wires already join.
    let mut junctions: Vec<eda_model::ir::Junction> = Vec::new();
    let mut junction_locked: Vec<bool> = Vec::new();
    let mut junction_look: Vec<JunctionLook> = Vec::new();
    // the uuids of the junctions read into each place: two dots on one point are one junction here
    let mut junction_uuid: Vec<Vec<String>> = Vec::new();
    for j in sexpr::find_all(root, "junction") {
        let Some(at) = sexpr::find(j, "at").and_then(point_mm) else { continue };
        let at = mm_point_to_um(at);
        if let Some(i) = junctions.iter().position(|x| x.at == at) {
            junction_uuid[i].extend(uuid_of(j));
        } else {
            junctions.push(eda_model::ir::Junction { id: String::new(), at });
            junction_locked.push(is_locked(j));
            junction_look.push(look_of(j));
            junction_uuid.push(uuid_of(j).into_iter().collect());
        }
    }
    // `(polyline (pts ...) (stroke (width w) ...))`: a graphic line on the notes layer (`SchLine`).
    let mut lines: Vec<eda_model::ir::SchLine> = Vec::new();
    let mut line_locked: Vec<bool> = Vec::new();
    let mut line_uuid: Vec<Option<String>> = Vec::new();
    let mut line_stroke: Vec<SchStroke> = Vec::new();
    for pl in sexpr::find_all(root, "polyline") {
        // A filled polyline is a polygon shape (`SchGraphic`), read below with the other shapes.
        let filled = sexpr::find(pl, "fill").and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)).is_some_and(|t| t != "none");
        if filled {
            continue;
        }
        if let Some(pts) = import_pts(pl).filter(|p| p.len() >= 2) {
            let width_nm = sexpr::find(pl, "stroke").and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(Nm::ZERO);
            lines.push(eda_model::ir::SchLine { id: String::new(), pts, width_nm });
            line_locked.push(is_locked(pl));
            line_uuid.push(uuid_of(pl));
            // the width is the line's own (`SchLine::width_nm`); the entry carries the style and the colour
            line_stroke.push(SchStroke { width_nm: Nm::ZERO, ..stroke_of(pl) });
        }
    }

    // `(bus_alias "NAME" (members "A" "B"))` (GAPS.md #20): real modern
    // KiCad only *writes* these into the `.kicad_pro` project file (read by
    // `import_kicad_sch_tree`, `project_bus_aliases`) -- this legacy per-screen
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
    let mut label_locked: Vec<bool> = Vec::new();
    let mut label_uuid: Vec<Option<String>> = Vec::new();
    // each label's spin and look, parallel to `labels`: the file's own, not an inference from the wires
    let mut label_geometry: Vec<(eda_model::sch_extras::LabelSpin, crate::sch_extras_io::EffectsLook)> = Vec::new();
    // each label's own fields (`parseSchText`'s properties), parallel to `labels`
    let mut label_fields_read: Vec<Vec<eda_model::sch_extras::LabelField>> = Vec::new();
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
            // `SCH_IO_KICAD_SEXPR_PARSER::parseSchText`: the angle is kept upright (0 or 90) and sets the spin, the justification then says whether
            // the text ends at the anchor
            let look = crate::sch_extras_io::read_effects(l);
            let angle = sexpr::find(l, "at").and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0);
            let vertical = eda_model::kicad_geom::upright_quarter_turns(angle) == 1;
            let ends_at_anchor = look.justify.iter().any(|j| j == "right");
            let spin = match (vertical, ends_at_anchor) {
                (false, false) => eda_model::sch_extras::LabelSpin::Right,
                (false, true) => eda_model::sch_extras::LabelSpin::Left,
                (true, false) => eda_model::sch_extras::LabelSpin::Up,
                (true, true) => eda_model::sch_extras::LabelSpin::Bottom,
            };
            labels.push(eda_model::ir::NetLabel { id: String::new(), net, at: mm_point_to_um(at), kind });
            label_locked.push(is_locked(l));
            label_uuid.push(uuid_of(l));
            label_geometry.push((spin, look));
            label_fields_read.push(crate::label_fields::read_label_fields(l, tag == "global_label"));
        }
    }

    // `T`: standalone text, no net -- see `SchematicText`'s own doc. Unlike
    // a label, its content is purely cosmetic, so it never feeds `reconcile`
    // below.
    let mut texts: Vec<SchematicText> = Vec::new();
    let mut text_locked: Vec<bool> = Vec::new();
    let mut text_uuid: Vec<Option<String>> = Vec::new();
    // each text's justification, parallel to `texts` (`SchExtras::text_justify`)
    let mut text_lie: Vec<eda_model::sch_extras::SchTextJustify> = Vec::new();
    for t in sexpr::find_all(root, "text") {
        let Some(content) = sexpr::txt(t, 1).map(String::from) else { continue };
        let Some(at) = sexpr::find(t, "at") else { continue };
        let (Some(x_mm), Some(y_mm)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let size_mm = sexpr::find(t, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).unwrap_or(1.27);
        // `parseSchText`: the angle is kept upright (`KeepUpright`); `parseEDA_TEXT`: the justification is centre, centre unless the file says otherwise
        let (angle, justify) = crate::sch_extras_io::read_text_lie(t, &crate::sch_extras_io::read_effects(t));
        texts.push(SchematicText { id: String::new(), content, at: mm_point_to_um(SPoint::new(x_mm, y_mm)), angle, size_nm: crate::import::sch_mm_to_nm(size_mm) });
        text_locked.push(is_locked(t));
        text_uuid.push(uuid_of(t));
        text_lie.push(justify);
    }

    let mut sheets: Vec<SheetInstance> = Vec::new();
    let mut sheet_locked: Vec<bool> = Vec::new();
    // how each sheet pin's text is set, by the pin's id (`SchExtras::sheet_pin_looks`)
    let mut sheet_pin_looks: Vec<(String, eda_model::sch_extras::LabelLook)> = Vec::new();
    // the pages of a sheet on the paths after its first (`SchExtras::sheet_pages`)
    let mut sheet_pages: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> = std::collections::BTreeMap::new();
    let mut sheet_user_fields: Vec<(String, Vec<eda_model::sch_extras::LabelField>)> = Vec::new();
    for s in sexpr::find_all(root, "sheet") {
        let id = sexpr::find(s, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
        let name = sheet_property_text(s, "Sheetname").unwrap_or_default();
        let file = sheet_property_text(s, "Sheetfile").unwrap_or_default();
        // the sheet's user fields (r2 B-L1): every property but the two mandatory ones
        let user: Vec<eda_model::sch_extras::LabelField> = crate::label_fields::read_label_fields(s, false).into_iter().filter(|f| !is_sheet_field(&f.name, "Sheetname") && !is_sheet_field(&f.name, "Sheetfile")).collect();
        if !user.is_empty() && !id.is_empty() {
            sheet_user_fields.push((id.clone(), user));
        }
        let at = sexpr::find(s, "at").and_then(point_mm).unwrap_or(SPoint::new(0.0, 0.0));
        let size = sexpr::find(s, "size").and_then(|sz| Some((sexpr::num(sz, 1)?, sexpr::num(sz, 2)?))).unwrap_or((0.0, 0.0));
        let pins = sexpr::find_all(s, "pin")
            .filter_map(|p| {
                let name = sexpr::txt(p, 1)?.to_string();
                let shape = sexpr::txt(p, 2).map(label_shape_from_token).unwrap_or_default();
                let at = sexpr::find(p, "at").and_then(point_mm)?;
                let id = sexpr::find(p, "uuid").and_then(|u| sexpr::txt(u, 1)).unwrap_or_default().to_string();
                // `(effects (font (size h w) bold italic) ..)`: how the pin's text is set (`SCH_SHEET_PIN` is an `EDA_TEXT`); the default one is no entry
                let effects = read_effects(p);
                let look = eda_model::sch_extras::LabelLook { size_nm: if effects.size_nm == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { effects.size_nm }, bold: effects.bold, italic: effects.italic, auto_rotate: false };
                if !id.is_empty() && !look.is_default() {
                    sheet_pin_looks.push((id.clone(), look));
                }
                Some(eda_model::ir::SheetPin { id, name, shape, at: mm_point_to_um(at) })
            })
            .collect();
        // `(instances (project "p" (path "/<parent>" (page "2"))))`: this placement's page number. Page "1" is the root's, and what this
        // project's own writer puts on a sheet whose page was never set -- both read back as "no explicit page" (`SheetInstance::page`).
        // A sheet on a screen shown on several paths has one `(path ..)` per path, each with its own page (`SCH_SHEET::m_instances`): the first is
        // the placement's page and a later one whose page differs is kept for its path (`SchExtras::sheet_pages`, by the model's path of the
        // screen: the path's sheet uuids, the root's own left out).
        // The page as `parseSchSheetInstances` reads it: "Empty page numbers are not permitted" ("#" instead), nor white space. A sheet path the file
        // gives no page gets one from `sheet_list::repair_page_numbers` once the tree is read (r2 B-M2).
        let instance_pages: Vec<(&str, String)> = sexpr::find(s, "instances")
            .into_iter()
            .flat_map(|i| sexpr::find_all(i, "project"))
            .flat_map(|p| sexpr::find_all(p, "path"))
            .filter_map(|p| Some((sexpr::txt(p, 1)?, kicad_page_number(sexpr::find(p, "page").and_then(|pg| sexpr::txt(pg, 1))?))))
            .collect();
        let page = instance_pages.first().map(|(_, pg)| pg.as_str()).filter(|pg| *pg != "1").unwrap_or_default().to_string();
        if let Some((_, first)) = instance_pages.first() {
            for (path, pg) in instance_pages.iter().skip(1).filter(|(_, pg)| pg != first) {
                let parent: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).skip(1).collect();
                if !id.is_empty() {
                    sheet_pages.entry(id.clone()).or_default().insert(parent.join("/"), pg.clone());
                }
            }
        }
        sheets.push(SheetInstance { id, name, file, at: mm_point_to_um(at), size: (crate::import::sch_mm_to_nm(size.0), crate::import::sch_mm_to_nm(size.1)), pins, page });
        sheet_locked.push(is_locked(s));
        notes.sheets_not_descended += 1;
    }

    // (uuid, id) of the symbols and the power symbols, whose ids are their references (final now: `unique_references` has numbered them)
    let mut uuid_ids: HashMap<String, Vec<String>> = HashMap::new();
    for r in &raw {
        if let Some(u) = &r.uuid {
            let ids = uuid_ids.entry(u.clone()).or_default();
            if !ids.contains(&r.reference) {
                ids.push(r.reference.clone());
            }
        }
    }
    for (p, u) in power_symbols.iter().zip(&power_uuid) {
        if let Some(u) = u {
            uuid_ids.entry(u.clone()).or_default().push(p.id.clone());
        }
    }
    let junction_points: Vec<Point> = junctions.iter().map(|j| j.at).collect();
    let nets = reconcile(&pin_world, &mut wires, &labels, &mut power_symbols, &mut no_connects, &junction_points);

    let sch = SchematicSection { symbols, wires, labels, texts, power_symbols, no_connects, bus_entries, junctions, lines, erc_exclusions: Vec::new(), erc_pin_map: None, user_fields: user_fields_read, imported_from_kicad: true, title_block, sheets, instance_overrides: overrides, extras: eda_model::sch_extras::SchExtras { page: crate::page::import_page(root).and_then(|p| p.sheet_parts().1), embedded: crate::embedded::read_owner(root, &mut Vec::new()), ..Default::default() }, field_layout: field_layout_read };
    let mut design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: blake3::hash(text.as_bytes()).to_hex().to_string(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None, sheet_contents: None, bus_aliases, symbol_library: None,
    };
    design.assign_missing_ids();
    // Drawn graphics (shapes, text boxes, rule areas, directive labels) and every item's `(locked yes)`, now that ids exist.
    if let Some(sch) = design.schematic.as_mut() {
        let graphics = crate::sch_extras_io::read_graphics(root);
        let graphic_locked: Vec<bool> = graphics.iter().map(|g| g.locked).collect();
        let graphic_uuid: Vec<Option<String>> = graphics.iter().map(|g| g.uuid.clone()).collect();
        sch.extras.graphics = graphics.into_iter().map(|g| g.graphic).collect();
        // Its tables (`SCH_TABLE`), with what a lock and a group need of each.
        let tables = crate::sch_table_io::read_all(root);
        let table_locked: Vec<bool> = tables.iter().map(|t| t.locked).collect();
        let table_uuid: Vec<Option<String>> = tables.iter().map(|t| t.uuid.clone()).collect();
        sch.extras.tables = tables.into_iter().map(|t| t.table).collect();
        sch.assign_missing_ids();
        let mut locked: Vec<String> = Vec::new();
        let mut take = |ids: Vec<&str>, flags: &[bool]| locked.extend(ids.into_iter().zip(flags).filter(|(_, l)| **l).map(|(id, _)| id.to_string()));
        take(sch.wires.iter().map(|w| w.id.as_str()).collect(), &wire_locked);
        take(sch.labels.iter().map(|l| l.id.as_str()).collect(), &label_locked);
        take(sch.texts.iter().map(|t| t.id.as_str()).collect(), &text_locked);
        take(sch.no_connects.iter().map(|n| n.id.as_str()).collect(), &nc_locked);
        take(sch.bus_entries.iter().map(|b| b.id.as_str()).collect(), &be_locked);
        take(sch.junctions.iter().map(|j| j.id.as_str()).collect(), &junction_locked);
        take(sch.lines.iter().map(|l| l.id.as_str()).collect(), &line_locked);
        take(sch.sheets.iter().map(|s| s.id.as_str()).collect(), &sheet_locked);
        take(sch.extras.graphics.iter().map(|g| g.id.as_str()).collect(), &graphic_locked);
        take(sch.extras.tables.iter().map(|t| t.id.as_str()).collect(), &table_locked);
        locked.extend(locked_refs);
        for id in locked.into_iter().filter(|id| !id.is_empty()) {
            sch.extras.set_locked(&id, true);
        }
        sch.extras.body_styles.extend(body_style_entries);
        sch.extras.pin_swaps.extend(pin_swap_entries);
        sch.extras.sheet_pin_looks.extend(sheet_pin_looks);
        sch.extras.sheet_pages.extend(sheet_pages);
        for (at, own, path, unit) in path_units {
            if let Some(sym) = sch.symbols.iter().find(|s| s.at == at && s.unit == own) {
                sch.extras.path_units.entry(eda_model::ir::field_key(&sym.id, sym.unit)).or_default().insert(path.join("/"), unit);
            }
        }
        sch.extras.sheet_fields.extend(sheet_user_fields);
        // A label's spin, size, bold and italic, as the file has them (`SchExtras::label_spins`, `label_looks`); a look equal to the default keeps no entry.
        for (id, (spin, look)) in sch.labels.iter().map(|l| l.id.clone()).zip(label_geometry) {
            if id.is_empty() {
                continue;
            }
            sch.extras.label_spins.insert(id.clone(), spin);
            let size_nm = if look.size_nm == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { look.size_nm };
            let label_look = eda_model::sch_extras::LabelLook { size_nm, bold: look.bold, italic: look.italic, auto_rotate: false };
            if !label_look.is_default() {
                sch.extras.label_looks.insert(id, label_look);
            }
        }
        // A label's own fields (`SchExtras::label_fields`): its net class and the others, not a global label's Sheet References.
        for (id, fields) in sch.labels.iter().map(|l| l.id.clone()).zip(label_fields_read) {
            if !id.is_empty() && !fields.is_empty() {
                sch.extras.label_fields.insert(id, fields);
            }
        }
        // A text's justification, as the file has it (`SchExtras::text_justify`); the default keeps no entry.
        for (id, justify) in sch.texts.iter().map(|t| t.id.clone()).zip(text_lie) {
            if !id.is_empty() {
                sch.extras.set_text_justify(&id, justify);
            }
        }
        // Strokes and junction looks that are not KiCad's defaults (`SchExtras::strokes`, `junction_looks`).
        let strokes: Vec<(String, SchStroke)> = sch.wires.iter().map(|w| w.id.clone()).zip(wire_stroke).chain(sch.bus_entries.iter().map(|b| b.id.clone()).zip(be_stroke)).chain(sch.lines.iter().map(|l| l.id.clone()).zip(line_stroke)).collect();
        for (id, stroke) in strokes.into_iter().filter(|(id, st)| !id.is_empty() && !st.is_default()) {
            sch.extras.strokes.insert(id, stroke);
        }
        for (id, look) in sch.junctions.iter().map(|j| j.id.clone()).zip(junction_look).filter(|(id, l)| !id.is_empty() && !l.is_default()) {
            sch.extras.junction_looks.insert(id, look);
        }

        // Groups (`SCH_IO_KICAD_SEXPR_PARSER::resolveGroups`): `(group "name" (uuid ..) (members "uuid" ..))` names its members by the uuids of the file, which are
        // ours now. A segment of a wire is a wire here, so a member that is a segment is that wire; the one item a group of one holds is not in a group.
        let mut by_uuid = uuid_ids;
        let mut note = |uuid: Option<String>, id: &str| {
            if let (Some(u), false) = (uuid, id.is_empty()) {
                by_uuid.entry(u).or_default().push(id.to_string());
            }
        };
        for (u, w) in wire_uuid.into_iter().zip(&sch.wires) {
            note(u, &w.id);
        }
        for (u, l) in label_uuid.into_iter().zip(&sch.labels) {
            note(u, &l.id);
        }
        for (u, t) in text_uuid.into_iter().zip(&sch.texts) {
            note(u, &t.id);
        }
        for (u, n) in nc_uuid.into_iter().zip(&sch.no_connects) {
            note(u, &n.id);
        }
        for (u, l) in line_uuid.into_iter().zip(&sch.lines) {
            note(u, &l.id);
        }
        for (us, j) in junction_uuid.into_iter().zip(&sch.junctions) {
            for u in us {
                note(Some(u), &j.id);
            }
        }
        for (u, g) in graphic_uuid.into_iter().zip(&sch.extras.graphics) {
            note(u, &g.id);
        }
        for (u, t) in table_uuid.into_iter().zip(&sch.extras.tables) {
            note(u, &t.id);
        }
        // a bus entry and a sheet are known by the uuid of the file already
        for id in sch.bus_entries.iter().map(|b| &b.id).chain(sch.sheets.iter().map(|s| &s.id)) {
            note(Some(id.clone()), id);
        }
        // the pins of the placed symbols (`(pin "1" (uuid ..))`) as `REF.PIN`, and the sheets' pins, for the items an ERC marker names
        for item in sexpr::find_all(root, "symbol") {
            let Some(reference) = uuid_of(item).and_then(|u| by_uuid.get(&u)).and_then(|ids| ids.first()).cloned() else { continue };
            for pin in sexpr::find_all(item, "pin") {
                if let (Some(number), Some(u)) = (sexpr::txt(pin, 1), uuid_of(pin)) {
                    notes.item_ids.entry(u).or_insert_with(|| format!("{reference}.{number}"));
                }
            }
        }
        for pin in sch.sheets.iter().flat_map(|s| s.pins.iter()).filter(|p| !p.id.is_empty()) {
            notes.item_ids.entry(pin.id.clone()).or_insert_with(|| pin.id.clone());
        }
        for (u, ids) in &by_uuid {
            if let Some(id) = ids.first() {
                notes.item_ids.entry(u.clone()).or_insert_with(|| id.clone());
            }
        }
        let (groups, locked_groups) = crate::import_items::build_groups(&by_uuid, &crate::import_items::import_groups(root), std::collections::BTreeSet::new());
        sch.extras.groups = groups;
        for id in locked_groups {
            sch.extras.set_locked(&id, true);
        }
    }

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
    import_kicad_sch_tree_with(path, &ImportOptions::default())
}

/// [`import_kicad_sch_tree`] with the choices of [`ImportOptions`], applied to every file of the tree.
pub fn import_kicad_sch_tree_with(path: &Path, opts: &ImportOptions) -> Result<(Design, ConstraintModel, SchImportNotes), Vec<CheckResult>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![CheckResult::fail("kicad_import.read", path.display().to_string(), format!("{e}"))])?;
    let (mut design, mut model, mut notes) = import_kicad_sch_with(&text, opts)?;
    let Some(dir) = path.parent() else { return Ok((design, model, notes)) };

    let mut sheet_contents: BTreeMap<String, SchematicSection> = BTreeMap::new();
    let mut read_files: BTreeSet<String> = BTreeSet::new();
    // each file's symbols by uuid: where they are and their unit, for the root's `(symbol_instances ..)`
    let mut symbol_places: BTreeMap<String, BTreeMap<String, (Point, Option<u32>)>> = BTreeMap::new();
    // A sheet's file is resolved against the folder of the file that places it (`SCH_IO_KICAD_SEXPR::loadHierarchy`: `fileName.MakeAbsolute(
    // m_currentPath.top() )`, then `m_currentPath.push( fileName.GetPath() )`), and a screen is the file it resolves to (`SCH_SCREENS` by file name,
    // `SearchHierarchy( fn.GetFullPath() )`): each screen is keyed by its path from the root's folder (`ampli_ht/filter.kicad_sch`), and each sheet
    // names its screen by that key. Two `x.kicad_sch` of two folders are two screens, and one file spelt two ways is one.
    let key_of = |resolve_dir: &Path, text: &str| -> String { sheet_file_key(dir, resolve_dir, text) };
    if let Some(root_sch) = design.schematic.as_mut() {
        for sh in root_sch.sheets.iter_mut() {
            sh.file = key_of(dir, &sh.file);
        }
    }
    let root_sheets: Vec<String> = design.schematic.as_ref().map(|s| s.sheets.iter().map(|sh| sh.file.clone()).collect()).unwrap_or_default();
    let mut queue: Vec<String> = root_sheets;

    while let Some(file) = queue.pop() {
        if !read_files.insert(file.clone()) {
            continue; // same screen reached again through another placement/path -- already have its content
        }
        let child_path = dir.join(&file);
        let resolve_dir = child_path.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
        let Ok(child_text) = std::fs::read_to_string(&child_path) else { continue };
        let Ok((child_design, child_model, child_notes)) = import_kicad_sch_with(&child_text, opts) else { continue };
        symbol_places.insert(file.clone(), symbol_places_of(&child_text));
        notes.unresolved_symbols += child_notes.unresolved_symbols;
        for (u, id) in child_notes.item_ids {
            notes.item_ids.entry(u).or_insert(id);
        }
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
        if let Some(mut child_sch) = child_design.schematic {
            // The embedded files belong to the schematic, whichever sheet's file holds them (`SCH_IO_KICAD_SEXPR_PARSER` files them under the `SCHEMATIC`): the root's list gets them,
            // the first of a name staying, and the sheet is left with none (the writer files them in the root sheet's file only).
            let from_child = std::mem::take(&mut child_sch.extras.embedded);
            if let Some(root_sch) = design.schematic.as_mut() {
                for f in from_child.files {
                    root_sch.extras.embedded.insert(f);
                }
            }
            for s in child_sch.sheets.iter_mut() {
                s.file = key_of(&resolve_dir, &s.file);
                queue.push(s.file.clone());
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
    // The project's aliases (`SCHEMATIC::loadBusAliasesFromProject`, read when the project is set, before the files): a file's own legacy alias of the
    // same name is added after them and wins: `buildConnectionGraph` fills `m_bus_alias_cache[name]` in the list's order, a later one over an earlier, and
    // the connection graph's `GetBusAlias` reads that cache.
    if let Ok(pro) = std::fs::read_to_string(path.with_extension("kicad_pro")) {
        for alias in project_bus_aliases(&pro) {
            if !design.bus_aliases.iter().any(|a| a.name == alias.name) {
                design.bus_aliases.push(alias);
            }
        }
    }
    legacy_symbol_instances(&text, &design, &mut sheet_contents, &symbol_places);
    design.sheet_contents = (!sheet_contents.is_empty()).then_some(sheet_contents);
    legacy_sheet_instances(&text, &mut design);
    // `SCH_SHEET_LIST::RepairPageNumbers`, which eeschema and kicad-cli run on a schematic just loaded with some page set (r2 B-M2)
    eda_model::sheet_list::repair_page_numbers(&mut design);

    Ok((design, model, notes))
}

/// A sheet instance's page as `SCH_IO_KICAD_SEXPR_PARSER` reads it (`parseSchSheetInstances` and a sheet's `(instances ..)`): "Empty page numbers are not
/// permitted" (a hash character instead), and "Whitespaces are not permitted" (`\r`, `\n`, `\t` and spaces removed).
pub(crate) fn kicad_page_number(page: &str) -> String {
    if page.is_empty() {
        "#".to_string()
    } else {
        page.chars().filter(|c| !matches!(c, '\r' | '\n' | '\t' | ' ')).collect()
    }
}

/// The key of the screen a sheet in a file of the folder `resolve_dir` names as `text`: the file's path from the root's folder `root_dir`, `/`-separated,
/// with `.` and `..` worked out (`ampli_ht/filter.kicad_sch`). A file outside the root's folder keeps its path from it with the `..` in front.
pub fn sheet_file_key(root_dir: &Path, resolve_dir: &Path, text: &str) -> String {
    use std::path::Component;
    let normal = |p: &Path| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    if out.last().is_some_and(|l| l != "..") {
                        out.pop();
                    } else {
                        out.push("..".into());
                    }
                }
                other => out.push(other.as_os_str().to_string_lossy().into_owned()),
            }
        }
        out
    };
    let target = normal(&resolve_dir.join(text));
    let root = normal(root_dir);
    let common = root.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), root.len() - common).collect();
    parts.extend(target[common..].iter().cloned());
    parts.join("/")
}

/// The bus aliases of a `.kicad_pro`'s text: `schematic.bus_aliases`, `{ "<name>": ["<member>", ..] }`, where KiCad 10 keeps them (`PROJECT_FILE`'s
/// `"schematic.bus_aliases"` parameter, whose reader takes an object of arrays of strings, each name and member stripped and an empty one dropped), sorted by
/// name as its map holds them. Nothing for a project without the section (one older than KiCad 10, whose aliases are in the `.kicad_sch`).
pub fn project_bus_aliases(pro_text: &str) -> Vec<eda_model::ir::BusAlias> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(pro_text) else { return Vec::new() };
    let Some(map) = json.get("schematic").and_then(|s| s.get("bus_aliases")).and_then(|a| a.as_object()) else { return Vec::new() };
    // nlohmann's object is sorted by its keys; the name is stripped and an empty one dropped, and `m_BusAliases.emplace` keeps the first of a name
    let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut out: Vec<eda_model::ir::BusAlias> = Vec::new();
    for (key, members) in entries {
        let Some(members) = members.as_array() else { continue };
        let name = key.trim();
        if name.is_empty() || out.iter().any(|a| a.name == name) {
            continue;
        }
        let members = members.iter().filter_map(|m| m.as_str()).map(str::trim).filter(|m| !m.is_empty()).map(String::from).collect();
        out.push(eda_model::ir::BusAlias { name: name.to_string(), members });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A file's symbols by their `(uuid ..)`: where each is (its library origin, as the reader keeps it) and its `(unit ..)`, `None` when the file gives none (KiCad 6
/// keeps a shared screen's units in the root's `symbol_instances` only). Power symbols are not among them.
fn symbol_places_of(text: &str) -> BTreeMap<String, (Point, Option<u32>)> {
    let mut out = BTreeMap::new();
    let Ok(tree) = sexpr::parse(text) else { return out };
    let Some(root) = tree.as_list() else { return out };
    for item in sexpr::find_all(root, "symbol") {
        let (Some(uuid), Some(at)) = (uuid_of(item), sexpr::find(item, "at")) else { continue };
        let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let unit = sexpr::find(item, "unit").and_then(|u| sexpr::num(u, 1)).map(|u| u.max(1.0) as u32);
        out.insert(uuid, (Point { x: crate::import::sch_mm_to_nm(x), y: crate::import::sch_mm_to_nm(y) }, unit));
    }
    out
}

/// The root's `(symbol_instances (path "/<sheet>/../<symbol>" (reference ..) (unit ..)) ..)` of a file older than the per-symbol `(instances ..)` (KiCad 6 and
/// early 7: `SCH_IO_KICAD_SEXPR_PARSER::parseSchSymbolInstances`, which `SCH_SHEET_LIST::UpdateSymbolInstanceData` hands to each symbol): the reference each
/// sheet path of a screen gives its symbols, as `SchematicSection::instance_overrides` under that path (the sheets of the path, which in these files does
/// not start with the root's uuid: `parseSchSymbolInstances` puts `m_rootUuid` in front). A symbol whose own `(instances ..)` already says it for that path
/// keeps that.
fn legacy_symbol_instances(root_text: &str, design: &Design, sheet_contents: &mut BTreeMap<String, SchematicSection>, symbol_places: &BTreeMap<String, BTreeMap<String, (Point, Option<u32>)>>) {
    let Ok(tree) = sexpr::parse(root_text) else { return };
    let Some(root) = tree.as_list() else { return };
    let Some(instances) = sexpr::find(root, "symbol_instances") else { return };
    // sheet uuid -> the file it shows
    let mut file_of: BTreeMap<String, String> = BTreeMap::new();
    for sch in design.schematic.iter().chain(sheet_contents.values()) {
        for sh in &sch.sheets {
            file_of.insert(sh.id.clone(), sh.file.clone());
        }
    }
    for path in sexpr::find_all(instances, "path") {
        let Some(path_str) = sexpr::txt(path, 1) else { continue };
        let parts: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
        let [sheets @ .., sheet, symbol] = parts.as_slice() else { continue };
        let sheet_path: Vec<String> = sheets.iter().chain(std::iter::once(sheet)).map(|s| s.to_string()).collect();
        let Some(reference) = sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1)) else { continue };
        let Some(file) = file_of.get(*sheet) else { continue };
        let Some(&(at, own_unit)) = symbol_places.get(file).and_then(|m| m.get(*symbol)) else { continue };
        let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).map(|u| u.max(1.0) as u32).or(own_unit).unwrap_or(1);
        let Some(sec) = sheet_contents.get_mut(file) else { continue };
        // the symbol: by its place and own unit; one the file gives no unit by its place alone
        let Some(i) = sec.symbols.iter().position(|s| s.at == at && s.unit == own_unit.unwrap_or(1)).or_else(|| own_unit.is_none().then(|| sec.symbols.iter().position(|s| s.at == at)).flatten()) else { continue };
        // a symbol the file gives no unit is drawn as the unit its (first) instance says (`UpdateSymbolInstanceData` -> `SetUnit`)
        if own_unit.is_none() && unit != 1 && sec.symbols[i].unit == 1 && !sec.instance_overrides.iter().any(|o| o.at == at && o.unit == 1) {
            sec.symbols[i].unit = unit;
        }
        let (sym_id, own) = (sec.symbols[i].id.clone(), sec.symbols[i].unit);
        if sec.instance_overrides.iter().any(|o| o.sheet_path == sheet_path && o.at == at && o.unit == own) {
            continue;
        }
        // a path that draws the symbol as another unit (r2 B-M4)
        if unit != own {
            sec.extras.path_units.entry(eda_model::ir::field_key(&sym_id, own)).or_default().insert(sheet_path.join("/"), unit);
        }
        sec.instance_overrides.push(eda_model::ir::SymbolPathOverride { at, sheet_path, reference: reference.to_string(), unit: own });
    }
}

/// The root's `(sheet_instances (path "/<sheet>/../" (page ..)) ..)` of a KiCad 6 / early 7 file (r2 B-M3), which holds the page of every sheet path: KiCad reads
/// them with `SCH_IO_KICAD_SEXPR_PARSER::parseSchSheetInstances` (an empty page is "#": "Empty page numbers are not permitted"; a file older than 20221002 has
/// no root uuid in front) and hands them to each sheet (`SCH_SHEET_LIST::UpdateSheetInstanceData`, `eeschema_helpers.cpp` for kicad-cli). A sheet on a screen
/// shown once takes the page as its own (`SheetInstance::page`), one on a screen shown on several paths for that path (`SchExtras::sheet_pages`). The root's
/// own `/` is page 1, and a page "1" is no page here, as for a sheet's own instances.
fn legacy_sheet_instances(root_text: &str, design: &mut Design) {
    let Ok(tree) = sexpr::parse(root_text) else { return };
    let Some(root) = tree.as_list() else { return };
    let Some(instances) = sexpr::find(root, "sheet_instances") else { return };
    let root_uuid = uuid_of(root).unwrap_or_default();
    let counts = eda_model::sheet_list::placement_counts(design);
    for path in sexpr::find_all(instances, "path") {
        let Some(path_str) = sexpr::txt(path, 1) else { continue };
        let mut ids: Vec<String> = path_str.split('/').filter(|c| !c.is_empty()).map(String::from).collect();
        if ids.first() == Some(&root_uuid) {
            ids.remove(0);
        }
        let Some((sheet_id, parent)) = ids.split_last() else { continue };
        let page = sexpr::find(path, "page").and_then(|p| sexpr::txt(p, 1)).map_or_else(|| "#".to_string(), kicad_page_number);
        if page == "1" {
            continue;
        }
        // the screen the sheet is drawn on: the root, else the screen of the placement `parent` ends at
        let mut file: Option<String> = None;
        let mut here = design.schematic.as_ref();
        for id in parent {
            let Some(placement) = here.and_then(|s| s.sheets.iter().find(|sh| &sh.id == id)) else {
                here = None;
                break;
            };
            file = Some(placement.file.clone());
            here = design.sheet_contents.as_ref().and_then(|c| c.get(&placement.file));
        }
        if here.is_none() {
            continue;
        }
        let shown_once = file.as_ref().is_none_or(|f| counts.get(f).copied().unwrap_or(1) <= 1);
        let screen = match &file {
            None => design.schematic.as_mut(),
            Some(f) => design.sheet_contents.as_mut().and_then(|c| c.get_mut(f)),
        };
        let Some(screen) = screen else { continue };
        let Some(sheet) = screen.sheets.iter_mut().find(|sh| &sh.id == sheet_id) else { continue };
        if shown_once {
            sheet.page = page;
        } else {
            screen.extras.sheet_pages.entry(sheet_id.clone()).or_default().insert(parent.join("/"), page);
        }
    }
}

pub(crate) fn import_title_block(root: &[Sexpr]) -> Option<TitleBlock> {
    // `(paper "A3")` sits next to the title block in the file and travels with it here; plain A4 is the default and is not stored.
    // (A portrait or user paper is also read in full, into `SchExtras::page`: `PageSettings::of_sheet`.)
    let paper = sexpr::find(root, "paper").and_then(|p| sexpr::txt(p, 1)).filter(|p| !p.eq_ignore_ascii_case("A4")).unwrap_or("").to_string();
    let Some(tb) = sexpr::find(root, "title_block") else {
        return (!paper.is_empty()).then(|| TitleBlock { paper, ..Default::default() });
    };
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
    Some(TitleBlock { title: text("title"), date: text("date"), rev: text("rev"), company: text("company"), comments, paper })
}

/// The places Swap Pins traded, read from the symbol's own copy of its library symbol: `SwapPinGeometry` swaps the position, orientation and length of two `SCH_PIN`s in the
/// copy (`SCH_SCREEN::Append` keeps such a symbol under a name of its own, `(lib_name ...)`), so a pin of `cached` that is not where the library's pin of that number is sits where
/// another pin of the library symbol is. The result is `pin number -> the number of the pin whose place it has` for the pins of `unit` (`SchExtras::pin_swaps`), and is empty
/// when the copy is no copy of `lib` (other pin numbers), when a pin sits where no pin of the library symbol does (a frame of its own) or where several do, or when what moved
/// is no exchange of places -- a copy this reader cannot read as swaps is read as the library symbol, as it always was.
fn pin_swaps_between(lib: &LibSymbol, cached: &LibSymbol, unit: u32, body_style: u32) -> BTreeMap<String, String> {
    let styled = |s: &LibSymbol| if body_style > 1 && s.has_alternate_body() { s.in_style(body_style).into_owned() } else { s.clone() };
    let (lib, cached) = (styled(lib), styled(cached));
    let on_unit = |p: &&LibPin| p.unit == 0 || p.unit == unit;
    let place = |p: &LibPin| (crate::import::sch_mm_to_nm(p.at.x), crate::import::sch_mm_to_nm(p.at.y), p.angle_deg.rem_euclid(360.0).round() as i64, crate::import::sch_mm_to_nm(p.length_mm));
    let (lib_pins, cached_pins): (Vec<&LibPin>, Vec<&LibPin>) = (lib.pins.iter().filter(on_unit).collect(), cached.pins.iter().filter(on_unit).collect());
    let mut swaps: BTreeMap<String, String> = BTreeMap::new();
    for cp in &cached_pins {
        let Some(lp) = lib_pins.iter().find(|q| q.number == cp.number) else { return BTreeMap::new() };
        if place(cp) == place(lp) {
            continue;
        }
        let sources: Vec<&&LibPin> = lib_pins.iter().filter(|q| place(q) == place(cp)).collect();
        if sources.len() != 1 {
            return BTreeMap::new();
        }
        swaps.insert(cp.number.clone(), sources[0].number.clone());
    }
    // an exchange of places: what a pin took is what another pin that moved left
    let sources: std::collections::BTreeSet<&String> = swaps.values().collect();
    if sources.len() != swaps.len() || !swaps.keys().all(|k| sources.contains(k)) {
        return BTreeMap::new();
    }
    swaps
}

/// A user-defined field of a placed symbol as the file has it (`SCH_IO_KICAD_SEXPR_PARSER::parseSchField`): the text, where its anchor is on the sheet and
/// at what angle, how it is set, and whether it is drawn.
struct RawUserField {
    name: String,
    text: String,
    at_nm: Point,
    angle_deg: f64,
    look: EffectsLook,
    hidden: bool,
    show_name: bool,
    do_not_autoplace: bool,
}

impl RawUserField {
    /// The field as a placement in the frame of `sym` (`SCH_FIELD::SetPosition`: the page position taken back through the symbol's transform; `eda_engine::fields::
    /// local_placement_on_page`), `width` being the box a mirror is about.
    fn placement(&self, sym: &SymbolInstance, width: f64) -> eda_model::ir::FieldPlacement {
        use eda_model::ir::{TextJustify, TextVAlign};
        let has = |w: &str| self.look.justify.iter().any(|j| j == w);
        let h = if has("left") {
            TextJustify::Left
        } else if has("right") {
            TextJustify::Right
        } else {
            TextJustify::Center
        };
        let v = if has("top") {
            TextVAlign::Top
        } else if has("bottom") {
            TextVAlign::Bottom
        } else {
            TextVAlign::Center
        };
        // a field is along the sheet or up it (`keepUpright`)
        let vertical = eda_model::kicad_geom::keep_upright_is_vertical(self.angle_deg);
        let mut p = eda_engine::fields::local_placement_on_page(sym.at, sym.rot, sym.mirrored, width, &self.name, (self.at_nm.x.as_f64(), self.at_nm.y.as_f64()), vertical, h, v, !self.hidden);
        p.size_nm = if self.look.size_nm == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { self.look.size_nm };
        p.bold = self.look.bold;
        p.italic = self.look.italic;
        p.name_shown = self.show_name;
        p.no_autoplace = self.do_not_autoplace;
        p
    }
}

/// The properties of a placed symbol that are no standard field (`FIELD_T::USER`): every `(property ...)` but Reference, Value, Footprint, Datasheet and
/// Description (matched without regard to case, as `parseSchField` does), in file order, the first of a name. `(property private "Name" "text" ...)` is read too
/// (the flag is not kept).
fn user_fields_of(item: &[Sexpr]) -> Vec<RawUserField> {
    let mut out: Vec<RawUserField> = Vec::new();
    for p in sexpr::find_all(item, "property") {
        // `private` is a word of its own before the name; a field called "private" has a list (its position) where the text would end
        let shift = usize::from(sexpr::txt(p, 1) == Some("private") && p.get(3).is_some_and(|n| n.text().is_some()));
        let (Some(name), Some(text)) = (sexpr::txt(p, 1 + shift), sexpr::txt(p, 2 + shift)) else { continue };
        if name.is_empty() || ["Reference", "Value", "Footprint", "Datasheet", "Description"].iter().any(|m| m.eq_ignore_ascii_case(name)) || out.iter().any(|f| f.name == name) {
            continue;
        }
        let at = sexpr::find(p, "at");
        let at_nm = at.and_then(|a| Some(Point { x: crate::import::sch_mm_to_nm(sexpr::num(a, 1)?), y: crate::import::sch_mm_to_nm(sexpr::num(a, 2)?) })).unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
        let effects = sexpr::find(p, "effects");
        // `(hide yes)`: KiCad 9 writes it in the property, before `(effects ...)`, and in it for a file we wrote; a bare `hide` is older still
        let hidden = parse_bool(p, "hide").unwrap_or(false) || effects.is_some_and(|e| parse_bool(e, "hide").unwrap_or(false) || e.iter().skip(1).any(|c| c.text() == Some("hide")));
        out.push(RawUserField {
            name: name.to_string(),
            text: text.to_string(),
            at_nm,
            angle_deg: at.and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0),
            look: read_effects(p),
            hidden,
            show_name: parse_bool(p, "show_name").unwrap_or(false),
            do_not_autoplace: parse_bool(p, "do_not_autoplace").unwrap_or(false),
        });
    }
    out
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
/// the studio applies when a screen is shown on more than one sheet path.
/// The key is the recorded path (`SCH_IO_KICAD_SEXPR_PARSER::parseSchematicSymbol`'s
/// `T_instances` -> `KIID_PATH`, `/<root-uuid>/<sheet-uuid>/...`) without its
/// first component, the root sheet's uuid, which the model does not keep
/// (`SymbolPathOverride::sheet_path`): `/<root>/<a>/<b>` -> `[a, b]`. A path
/// of the root alone (a symbol on the root sheet, whose reference is its
/// own) or with no components at all is skipped.
fn parse_instance_overrides(item: &[Sexpr], at: Point, own_unit: u32) -> Vec<(eda_model::ir::SymbolPathOverride, u32)> {
    let Some(instances) = sexpr::find(item, "instances") else { return Vec::new() };
    let mut out = Vec::new();
    for project in sexpr::find_all(instances, "project") {
        for path in sexpr::find_all(project, "path") {
            let Some(path_str) = sexpr::txt(path, 1) else { continue };
            let sheet_path: Vec<String> = path_str.split('/').filter(|s| !s.is_empty()).skip(1).map(String::from).collect();
            if sheet_path.is_empty() {
                continue;
            }
            let Some(reference) = sexpr::find(path, "reference").and_then(|r| sexpr::txt(r, 1)) else { continue };
            // `SCH_SYMBOL_INSTANCE::m_Unit`: the unit this path draws the symbol as. The override is filed under the symbol's own unit, which tells
            // the symbol apart (`sheet_list::override_under`), and a path unit that differs goes to `SchExtras::path_units` (r2 B-M4)
            let unit = sexpr::find(path, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0).max(1.0) as u32;
            out.push((eda_model::ir::SymbolPathOverride { at, sheet_path, reference: reference.to_string(), unit: own_unit }, unit));
        }
    }
    out
}

fn sheet_property_text(item: &[Sexpr], name: &str) -> Option<String> {
    sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1).is_some_and(|n| is_sheet_field(n, name))).and_then(|p| sexpr::txt(p, 2)).map(String::from)
}

/// Whether a sheet's `(property "<n>" ..)` is its mandatory field `canonical` ("Sheetname" or "Sheetfile"): `SCH_IO_KICAD_SEXPR_PARSER::parseSchField` compares
/// the canonical name without regard to case, and takes the names older files wrote ("Sheet name", "Sheet file", KiCad 6) for the same fields.
pub(crate) fn is_sheet_field(n: &str, canonical: &str) -> bool {
    let legacy = match canonical {
        "Sheetname" => "Sheet name",
        "Sheetfile" => "Sheet file",
        _ => canonical,
    };
    n.eq_ignore_ascii_case(canonical) || n.eq_ignore_ascii_case(legacy)
}

/// A wire's, bus's, bus entry's or polyline's `(stroke (width w) (type t) (color ...))` (`SCH_IO_KICAD_SEXPR_PARSER::parseStroke`).
fn stroke_of(node: &[Sexpr]) -> SchStroke {
    let (width_nm, style, color) = crate::sch_extras_io::parse_stroke(node);
    SchStroke { width_nm, style, color }
}

/// A junction's `(diameter d) (color r g b a)`; a colour of all zeroes is KiCad's "unspecified".
fn look_of(node: &[Sexpr]) -> JunctionLook {
    let diameter_nm = sexpr::find(node, "diameter").and_then(|d| sexpr::num(d, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(Nm::ZERO);
    let color = sexpr::find(node, "color").and_then(crate::sch_extras_io::parse_color).filter(|c| !(c.r == 0 && c.g == 0 && c.b == 0 && c.a == 0));
    JunctionLook { diameter_nm, color }
}

fn point_mm(at: &[Sexpr]) -> Option<SPoint> {
    Some(SPoint::new(sexpr::num(at, 1)?, sexpr::num(at, 2)?))
}

fn mm_point_to_um(p: SPoint) -> Point {
    Point { x: crate::import::sch_mm_to_nm(p.x), y: crate::import::sch_mm_to_nm(p.y) }
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
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;
    use crate::{export_kicad_sch, ExportMeta};
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{ConstraintModel, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: None, footprint: Some("Foo:Bar".into()), symbol: None, datasheet: None, pins, body_nm: None, edge: None }
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

        let lib_id = design.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "U1").unwrap().lib_id.clone();
        let with_mirror_x = text.replacen(&format!("(lib_id \"{lib_id}\") (at "), &format!("(lib_id \"{lib_id}\") (mirror x) (at "), 1);
        let (design_x, _, _) = import_kicad_sch(&with_mirror_x).expect("parses with an injected (mirror x)");
        let u1_x = design_x.schematic.unwrap().symbols.into_iter().find(|s| s.id == "U1").unwrap();
        assert!(u1_x.mirror_y, "(mirror x) must set mirror_y");
        assert!(!u1_x.mirrored, "(mirror x) must not also set mirrored");

        let with_mirror_y = text.replacen(&format!("(lib_id \"{lib_id}\") (at "), &format!("(lib_id \"{lib_id}\") (mirror y) (at "), 1);
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
            hidden: false,
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
            pin_names_hidden: false,
            pin_numbers_hidden: false,
            pin_name_offset_mm: 0.508,
            alternate: None,
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

        let sym = |unit: u32, x: Nm| SymbolInstance { id: "U1".into(), at: Point { x, y: Nm::ZERO }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:DUAL".into(), unit, value: "DUAL".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        let sch = SchematicSection {
            symbols: vec![sym(1, Nm::ZERO), sym(2, mm(50.0))],
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![], bus_entries: vec![],
            erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(),
            title_block: None,
            sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(),
            imported_from_kicad: false,
        };
        let design = Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None };

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
        assert_eq!(unit1.at.x, Nm::ZERO);
        assert_eq!(unit2.at.x, mm(50.0));
    }

    /// A symbol placed in the alternate ("De Morgan") body style of its library symbol: the file carries both bodies (`Name_<unit>_1`, `Name_<unit>_2`) and
    /// `(convert 2)` on the instance, and reading it back gives the alternate body, the style, and the pins where the alternate body has them.
    #[test]
    fn alternate_body_style_round_trips_through_export_and_import() {
        use eda_model::symbol::{AlternateBody, LibPin, LibSymbol, SPoint, SymbolGraphic};
        let p = |number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64| LibPin {
            number: number.into(),
            name: name.into(),
            electrical_type: etype.into(),
            shape: "line".into(),
            at: SPoint::new(x, y),
            angle_deg: angle,
            length_mm: 2.54,
            unit: 1,
            hidden: false,
        };
        let rect = |x0: f64, x1: f64| SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(x0, 2.54), end: SPoint::new(x1, -2.54), stroke_mm: 0.254, filled: false };
        // style 1: inputs on the left, the output on the right; style 2 (the same gate drawn with its bubbles): same pins, a wider body
        let lib = LibSymbol {
            lib_id: "test:GATE".into(),
            graphics: vec![rect(-2.54, 2.54)],
            pins: vec![p("1", "A", "input", -7.62, 1.27, 0.0), p("2", "B", "input", -7.62, -1.27, 0.0), p("3", "Y", "output", 7.62, 0.0, 180.0)],
            power: false,
            in_bom: true,
            on_board: true,
            datasheet: String::new(),
            description: String::new(),
            reference_prefix: "U".into(),
            unit_count: 1,
            pin_names_hidden: false,
            pin_numbers_hidden: false,
            pin_name_offset_mm: 0.508,
            alternate: Some(Box::new(AlternateBody::new(
                vec![rect(-3.81, 3.81)],
                vec![p("1", "A", "input", -7.62, 1.27, 0.0), p("2", "B", "input", -7.62, -1.27, 0.0), p("3", "Y", "output", 7.62, 0.0, 180.0)],
            ))),
        };
        let u1 = part("U1", vec![pin("1", "A", PinKind::Signal), pin("2", "B", PinKind::Signal), pin("3", "Y", PinKind::Signal)]);
        let model = ConstraintModel { parts: vec![u1], symbols: vec![lib], ..Default::default() };
        let instance = SymbolInstance { id: "U1".into(), at: Point { x: mm(25.4), y: mm(25.4) }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:GATE".into(), unit: 1, value: "GATE".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        let mut sch = SchematicSection { symbols: vec![instance], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false };
        let design_with = |sch: SchematicSection| Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None };
        let meta = ExportMeta { date: "2026-01-01", title: "alt" };

        // in the normal body style: both bodies are in the file, the instance says nothing
        let normal = export_kicad_sch(&design_with(sch.clone()), &model, &meta).unwrap();
        assert!(normal.contains("\"GATE_1_1\"") && normal.contains("\"GATE_1_2\"") && normal.contains("\"GATE_0_1\"") && normal.contains("\"GATE_0_2\""), "{normal}");
        assert!(!normal.contains("(convert"), "the default body style is not written");
        let (back, back_model, _) = import_kicad_sch(&normal).expect("round trips");
        assert!(back.schematic.unwrap().extras.body_styles.is_empty());
        assert!(back_model.symbols[0].alternate.is_some(), "the alternate body comes back");

        // in the alternate one
        sch.extras.body_styles.insert("U1".into(), 2000);
        let alt = export_kicad_sch(&design_with(sch), &model, &meta).unwrap();
        assert!(alt.contains("(unit 1) (convert 2)"), "{alt}");
        let (back, back_model, notes) = import_kicad_sch(&alt).expect("round trips");
        assert_eq!(notes.unresolved_symbols, 0);
        let back_sch = back.schematic.unwrap();
        assert_eq!(back_sch.extras.body_styles.get("U1"), Some(&2), "the instance keeps its body style");
        assert_eq!(back_sch.body_style_of(&back_sch.symbols[0]), 2);
        let lib = &back_model.symbols[0];
        let alternate = lib.alternate.as_ref().expect("both bodies are read");
        assert_eq!((lib.pins.len(), alternate.pins.len()), (3, 3), "each body has its own pins");
        assert_eq!((lib.graphics.len(), alternate.graphics.len()), (1, 1), "and its own graphics, not both drawn at once");
        let width = |g: &SymbolGraphic| match g {
            SymbolGraphic::Rectangle { start, end, .. } => (end.x - start.x).abs(),
            _ => 0.0,
        };
        assert!((width(&lib.graphics[0]) - 5.08).abs() < 1e-6 && (width(&alternate.graphics[0]) - 7.62).abs() < 1e-6, "{:?} {:?}", lib.graphics, alternate.graphics);
    }

    /// r2 B-H2: a sheet's file is resolved against the folder of the file that places it (`SCH_IO_KICAD_SEXPR::loadHierarchy`), and a screen is keyed by
    /// the file it resolves to, from the root's folder: `a/x.kicad_sch` and `b/x.kicad_sch`, each placed as `x.kicad_sch` by the file beside it, are two
    /// screens; `./a/../a/x.kicad_sch` is `a/x.kicad_sch`. The writer names each again as its placing file reads it, and writes it in its folder.
    #[test]
    fn sheets_in_subfolders_are_keyed_by_the_file_they_resolve_to() {
        let root = Path::new("/p");
        assert_eq!(sheet_file_key(root, root, "a/x.kicad_sch"), "a/x.kicad_sch");
        assert_eq!(sheet_file_key(root, &root.join("a"), "x.kicad_sch"), "a/x.kicad_sch");
        assert_eq!(sheet_file_key(root, root, "./a/../a/x.kicad_sch"), "a/x.kicad_sch");
        assert_eq!(sheet_file_key(root, &root.join("a"), "../top.kicad_sch"), "top.kicad_sch");
        assert_eq!(sheet_file_key(root, root, "../out.kicad_sch"), "../out.kicad_sch");
        assert_eq!(crate::sheet_file_relative_to("a", "a/x.kicad_sch"), "x.kicad_sch");
        assert_eq!(crate::sheet_file_relative_to("a", "top.kicad_sch"), "../top.kicad_sch");
        assert_eq!(crate::sheet_file_relative_to("", "a/x.kicad_sch"), "a/x.kicad_sch");
        assert_eq!(crate::sheet_file_relative_to("a/b", "a/c/x.kicad_sch"), "../c/x.kicad_sch");

        let dir = std::env::temp_dir().join(format!("eda_kicad_sch_subfolders_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in ["a", "b"] {
            std::fs::create_dir_all(dir.join(f)).unwrap();
        }
        let sheet = |uuid: &str, name: &str, file: &str, y: i32| format!("(sheet (at 100 {y}) (size 20 20) (uuid \"{uuid}\") (property \"Sheetname\" \"{name}\" (at 100 99 0)) (property \"Sheetfile\" \"{file}\" (at 100 121 0)))");
        let file = |uuid: &str, body: &str| format!("(kicad_sch (version 20250114) (generator \"test\") (uuid \"{uuid}\") (paper \"A4\") {body})");
        let leaf = |uuid: &str, text: &str| file(uuid, &format!("(label \"{text}\" (at 10 10 0) (uuid \"{uuid}-l\"))"));
        std::fs::write(dir.join("root.kicad_sch"), file("11111111-1111-1111-1111-111111111111", &format!("{} {}", sheet("21111111-1111-1111-1111-111111111111", "A", "a/a.kicad_sch", 100), sheet("31111111-1111-1111-1111-111111111111", "B", "b/b.kicad_sch", 200)))).unwrap();
        std::fs::write(dir.join("a/a.kicad_sch"), file("41111111-1111-1111-1111-111111111111", &sheet("51111111-1111-1111-1111-111111111111", "X", "x.kicad_sch", 100))).unwrap();
        std::fs::write(dir.join("b/b.kicad_sch"), file("61111111-1111-1111-1111-111111111111", &sheet("71111111-1111-1111-1111-111111111111", "X", "x.kicad_sch", 100))).unwrap();
        std::fs::write(dir.join("a/x.kicad_sch"), leaf("81111111-1111-1111-1111-111111111111", "IN_A")).unwrap();
        std::fs::write(dir.join("b/x.kicad_sch"), leaf("91111111-1111-1111-1111-111111111111", "IN_B")).unwrap();
        let (design, model, notes) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).unwrap();
        assert_eq!(notes.sheets_not_descended, 0);
        let screens = design.sheet_contents.as_ref().unwrap();
        let mut keys: Vec<&String> = screens.keys().collect();
        keys.sort();
        assert_eq!(keys, ["a/a.kicad_sch", "a/x.kicad_sch", "b/b.kicad_sch", "b/x.kicad_sch"]);
        assert_eq!(screens["a/x.kicad_sch"].labels[0].net, "IN_A");
        assert_eq!(screens["b/x.kicad_sch"].labels[0].net, "IN_B", "the other folder's x.kicad_sch is a screen of its own");
        assert_eq!(screens["a/a.kicad_sch"].sheets[0].file, "a/x.kicad_sch");
        // written back: each screen in its folder, named relative to the file that places it
        let files = crate::export_kicad_sch_tree(&design, &model, &ExportMeta { date: "2026-10-27", title: "t" }, "root.kicad_sch").unwrap();
        let text = |name: &str| files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name} in {:?}", files.iter().map(|f| &f.0).collect::<Vec<_>>())).1.clone();
        assert!(text("a/a.kicad_sch").contains("(property \"Sheetfile\" \"x.kicad_sch\""), "{}", text("a/a.kicad_sch"));
        assert!(text("root.kicad_sch").contains("(property \"Sheetfile\" \"b/b.kicad_sch\""));
        assert!(text("b/x.kicad_sch").contains("IN_B"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// r2 B-L1 (`saveSheet` saves every field; `SCH_SHEET::ResolveTextVar` reads one by name): a sheet's user field is read and written back.
    #[test]
    fn a_sheets_user_field_is_read_and_written_back() {
        let text = "(kicad_sch (version 20250114) (generator \"test\") (uuid \"11111111-1111-1111-1111-111111111111\") (paper \"A4\") (sheet (at 100 100) (size 20 20) (uuid \"21111111-1111-1111-1111-111111111111\") (property \"Sheetname\" \"A\" (at 100 99 0)) (property \"Sheetfile\" \"a.kicad_sch\" (at 100 121 0)) (property \"param\" \"NODE_1\" (at 100 100 0) (effects (font (size 1.27 1.27)) hide))))";
        let (design, model, _) = import_kicad_sch(text).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        let fields = &sch.extras.sheet_fields["21111111-1111-1111-1111-111111111111"];
        assert_eq!((fields[0].name.as_str(), fields[0].text.as_str(), fields[0].visible), ("param", "NODE_1", false));
        let out = crate::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-10-27", title: "t" }).unwrap();
        assert!(out.contains("(property \"param\" \"NODE_1\""), "{out}");
    }

    /// r2 B-M3 (`parseSchSheetInstances`, `UpdateSheetInstanceData`): a KiCad 6 root's `(sheet_instances (path "/<sheet>/" (page "7")))` is the sheet's page, and an
    /// empty page is "#".
    #[test]
    fn a_kicad_6_roots_sheet_instances_give_the_sheets_their_pages() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_sheet_instances_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sheet = |uuid: &str, file: &str, y: i32| format!("(sheet (at 100 {y}) (size 20 20) (uuid \"{uuid}\") (property \"Sheet name\" \"{file}\" (at 100 99 0)) (property \"Sheet file\" \"{file}.kicad_sch\" (at 100 121 0)))");
        let root = format!("(kicad_sch (version 20211123) (generator eeschema) (uuid \"11111111-1111-1111-1111-111111111111\") (paper \"A4\") {} {} (sheet_instances (path \"/\" (page \"1\")) (path \"/21111111-1111-1111-1111-111111111111/\" (page \"7\")) (path \"/31111111-1111-1111-1111-111111111111/\" (page \"\"))))", sheet("21111111-1111-1111-1111-111111111111", "a", 100), sheet("31111111-1111-1111-1111-111111111111", "b", 200));
        std::fs::write(dir.join("root.kicad_sch"), root).unwrap();
        for f in ["a", "b"] {
            std::fs::write(dir.join(format!("{f}.kicad_sch")), "(kicad_sch (version 20211123) (generator eeschema) (uuid \"41111111-1111-1111-1111-111111111111\") (paper \"A4\"))").unwrap();
        }
        let (design, _, _) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).unwrap();
        let pages: Vec<(String, String)> = design.schematic.as_ref().unwrap().sheets.iter().map(|s| (s.name.clone(), s.page.clone())).collect();
        assert_eq!(pages, [("a".to_string(), "7".to_string()), ("b".to_string(), "#".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// r2 B-M2 (`SCH_SHEET_LIST::RepairPageNumbers`, run on load when some page is set; QA `netlists/hierarchy_aliases`): root sheets on pages "2" and "3",
    /// each showing a screen with a sheet the file gives no page: the two nested paths get 4 and 5, the lowest pages no sheet holds, and no page is shown
    /// twice. An empty `(page "")` reads as "#" and white space is removed (`parseSchSheetInstances`).
    #[test]
    fn an_imported_file_with_some_pages_set_gives_the_others_unused_pages() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_repair_pages_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root_uuid = "11111111-1111-1111-1111-111111111111";
        let sheet = |uuid: &str, name: &str, file: &str, x: i32, page: Option<&str>| {
            let inst = page.map(|p| format!("(instances (project \"t\" (path \"/{root_uuid}\" (page \"{p}\"))))")).unwrap_or_default();
            format!("(sheet (at {x} 50) (size 20 20) (uuid \"{uuid}\") (property \"Sheetname\" \"{name}\" (at {x} 49 0)) (property \"Sheetfile\" \"{file}\" (at {x} 71 0)) {inst})")
        };
        let file = |uuid: &str, body: String| format!("(kicad_sch (version 20231120) (generator eeschema) (uuid \"{uuid}\") (paper \"A4\") {body})");
        std::fs::write(dir.join("root.kicad_sch"), file(root_uuid, format!("{} {} {} (sheet_instances (path \"/\" (page \"1\")))", sheet("21111111-1111-1111-1111-111111111111", "First", "sub.kicad_sch", 10, Some("2")), sheet("31111111-1111-1111-1111-111111111111", "Second", "sub.kicad_sch", 60, Some("3")), sheet("51111111-1111-1111-1111-111111111111", "Third", "leaf.kicad_sch", 110, Some("")))) ).unwrap();
        std::fs::write(dir.join("sub.kicad_sch"), file("41111111-1111-1111-1111-111111111111", sheet("61111111-1111-1111-1111-111111111111", "Sub2", "leaf.kicad_sch", 10, None))).unwrap();
        std::fs::write(dir.join("leaf.kicad_sch"), file("71111111-1111-1111-1111-111111111111", String::new())).unwrap();
        let (design, _, _) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).unwrap();
        let shown: Vec<(String, String)> = eda_model::sheet_list::walk(&design).into_iter().map(|e| (e.name.clone(), e.effective_page())).collect();
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(shown, [s("", "1"), s("First", "2"), s("Sub2", "4"), s("Second", "3"), s("Sub2", "5"), s("Third", "#")]);
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(root_sch.sheets[0].page, "2", "the placement's own page number is read from its (instances ...) block");

        let screens = design.sheet_contents.expect("sheet_contents populated");
        let child = screens.get("child.kicad_sch").expect("child content present");
        assert_eq!(child.symbols.len(), 1);
        assert_eq!(child.symbols[0].id, "R1");
        assert!(model.parts.iter().any(|p| p.reference == "R1"), "child's own Part folded into the combined model");

        // KiCad 6's names for the two fields ("Sheet name", "Sheet file": `parseSchField`'s legacy names, the QA corpus's `complex_hierarchy`) are the same fields
        std::fs::write(dir.join("root.kicad_sch"), root_text.replace("\"Sheetname\"", "\"Sheet name\"").replace("\"Sheetfile\"", "\"Sheet file\"")).unwrap();
        let (design, _, notes) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).expect("tree import succeeds");
        assert_eq!(notes.sheets_not_descended, 0);
        let sheet = &design.schematic.as_ref().unwrap().sheets[0];
        assert_eq!((sheet.name.as_str(), sheet.file.as_str()), ("child", "child.kicad_sch"));
        assert_eq!(design.sheet_contents.as_ref().and_then(|c| c.get("child.kicad_sch")).map(|c| c.symbols.len()), Some(1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Do not Populate and the three exclusions (`SCH_EDIT_TOOL::SetAttribute`) survive `export_kicad_sch` -> `import_kicad_sch`: the writer
    /// puts them where kicad-cli's BOM, netlist and ERC read them, the reader takes them back.
    #[test]
    fn symbol_attributes_round_trip_through_the_derived_file() {
        let model = ConstraintModel { parts: vec![part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]), part("R2", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)])], ..Default::default() };
        let mut design = derive_schematic(&model, &EngineOptions::new(1, "attrs")).unwrap();
        {
            let sch = design.schematic.as_mut().unwrap();
            let r1 = sch.symbols.iter_mut().find(|s| s.id == "R1").unwrap();
            r1.dnp = true;
            r1.exclude_from_bom = true;
            r1.exclude_from_board = true;
            r1.exclude_from_sim = true;
        }
        let text = export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "attrs" }).unwrap();
        assert!(text.contains("(exclude_from_sim yes) (in_bom no) (on_board no) (dnp yes)"), "R1's flags are in the file");
        assert!(text.contains("(exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)"), "R2 keeps the defaults");
        let (back, _, _) = import_kicad_sch(&text).unwrap();
        let sch = back.schematic.unwrap();
        let r1 = sch.symbols.iter().find(|s| s.id == "R1").unwrap();
        let r2 = sch.symbols.iter().find(|s| s.id == "R2").unwrap();
        assert!(r1.dnp && r1.exclude_from_bom && r1.exclude_from_board && r1.exclude_from_sim);
        assert!(!r2.dnp && !r2.exclude_from_bom && !r2.exclude_from_board && !r2.exclude_from_sim);
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
        // `KICAD_QA_DATA` names `qa/data` of the KiCad sources (commit 8303b2ad); the default is the copy kept beside them.
        let data = std::env::var_os("KICAD_QA_DATA").map(std::path::PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| std::path::PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let root = data.join("pcbnew/issue21739/topology_mismatch.kicad_sch");
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

    /// p12 S4 (b): KiCad 10 keeps a project's bus aliases in the `.kicad_pro` (`schematic.bus_aliases`, `SCHEMATIC::loadBusAliasesFromProject`), not the
    /// `.kicad_sch`: they are read from the project beside the root, a member stripped and an empty one dropped, and a file's own legacy `(bus_alias ..)` of
    /// the same name wins (the files are read after the project, and `buildConnectionGraph`'s `m_bus_alias_cache[name]` keeps the last of a name). A name
    /// is stripped and an empty one dropped, as `PROJECT_FILE`'s reader does.
    #[test]
    fn a_projects_bus_aliases_are_read_from_the_kicad_pro() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_pro_bus_aliases_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("p.kicad_sch");
        std::fs::write(&root, "(kicad_sch (version 20250114) (generator \"eeschema\") (uuid \"00000000-0000-0000-0000-000000000001\") (paper \"A4\")\n  (bus_alias \"b_yz\" (members \"q\"))\n)\n").unwrap();
        std::fs::write(dir.join("p.kicad_pro"), r#"{ "schematic": { "bus_aliases": { "b_yz": ["y", "z"], " a_xyz ": ["x", " y ", "", "z"], "a_xyz": ["dropped"], "  ": ["e"] } } }"#).unwrap();
        let (design, _, _) = import_kicad_sch_tree(&root).unwrap();
        let aliases: Vec<(String, Vec<String>)> = design.bus_aliases.iter().map(|a| (a.name.clone(), a.members.clone())).collect();
        let v = |m: &[&str]| m.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(aliases, vec![("b_yz".to_string(), v(&["q"])), ("a_xyz".to_string(), v(&["x", "y", "z"]))]);
        // a project without the section (older than KiCad 10) gives none
        assert!(project_bus_aliases(r#"{ "schematic": {} }"#).is_empty() && project_bus_aliases("not json").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A KiCad 10 file with a symbol that declares `(body_styles demorgan)` and has `Name_1_1` and `Name_1_2` sub-blocks: the importer reads both bodies of the
    /// library symbol (the normal one is no longer drawn doubled with the alternate one) and the instances, all in the normal body style, keep no body style entry.
    #[test]
    fn real_qa_de_morgan_symbol_reads_both_bodies() {
        let data = std::env::var_os("KICAD_QA_DATA").map(std::path::PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| std::path::PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let file = data.join("eeschema/variants/pic_sockets.kicad_sch");
        let Ok(text) = std::fs::read_to_string(&file) else {
            eprintln!("QA corpus not found at {}; skipping", file.display());
            return;
        };
        let (design, model, notes) = import_kicad_sch(&text).expect("real QA file imports");
        assert_eq!(notes.unresolved_symbols, 0);
        let pic = model.symbols.iter().find(|s| s.lib_id == "pic_programmer:PIC16F54").expect("the PIC16F54 library symbol");
        let alt = pic.alternate.as_ref().expect("declared (body_styles demorgan), with PIC16F54_1_2");
        assert!(!pic.pins.is_empty() && !alt.pins.is_empty(), "each body has its pins");
        assert_eq!(pic.pins.len(), alt.pins.len(), "the same pins in both bodies");
        // the sub-blocks are split, not merged: the pins are not listed twice in the normal body
        let mut numbers: Vec<&str> = pic.pins.iter().map(|p| p.number.as_str()).collect();
        numbers.sort();
        numbers.dedup();
        assert_eq!(numbers.len(), pic.pins.len(), "a pin appears once in a body");
        assert!(design.schematic.unwrap().extras.body_styles.is_empty(), "the symbols are placed in the normal style");
    }

    /// A bus wire, a bus entry, and a bus alias (GAPS.md #20) all survive
    /// `export_kicad_sch` -> `import_kicad_sch` -- no real symbols/parts
    /// needed, this is purely about the three new sexpr shapes
    /// (`crate::lib`'s writer / this module's own `(bus ...)`/`(bus_entry
    /// ...)`/`(bus_alias ...)` readers above) agreeing with each other.
    #[test]
    fn bus_wire_entry_and_alias_round_trip_through_export_and_import() {
        let bus_pt = Point { x: mm(10.0), y: mm(10.0) };
        let net_pt = Point { x: mm(12.54), y: mm(12.54) };
        let sch = SchematicSection {
            wires: vec![Wire { id: String::new(), net: "DATA[0..3]".into(), pins: vec![], pts: vec![Point { x: Nm::ZERO, y: mm(10.0) }, bus_pt], bus: true }],
            bus_entries: vec![eda_model::ir::BusEntry { id: String::new(), at: bus_pt, size: Point { x: mm(2.54), y: mm(2.54) } }],
            labels: vec![eda_model::ir::NetLabel { id: String::new(), net: "DATA2".into(), at: net_pt, kind: LabelKind::Local }],
            ..SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
        };
        let design = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
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
        assert_eq!(sch.wires[0].pts, vec![Point { x: Nm::ZERO, y: mm(10.0) }, bus_pt]);
        assert_eq!(sch.bus_entries.len(), 1);
        assert_eq!(sch.bus_entries[0].at, bus_pt);
        assert_eq!(sch.bus_entries[0].size, Point { x: mm(2.54), y: mm(2.54) });
        assert_eq!(reimported.bus_aliases.len(), 1);
        assert_eq!(reimported.bus_aliases[0].name, "USB");
        assert_eq!(reimported.bus_aliases[0].members, vec!["D+".to_string(), "D-".to_string()]);
    }

    /// Two wires that cross (neither ends on the other) are two nets -- until an explicit junction (`J`,
    /// `SCH_JUNCTION`) sits at the crossing. The junction and a notes-layer line survive the file round trip.
    #[test]
    fn an_explicit_junction_joins_crossing_wires_and_junctions_and_graphic_lines_round_trip() {
        let cross = Point { x: mm(10.0), y: mm(10.0) };
        let build = |junctions: Vec<eda_model::ir::Junction>| {
            let sch = SchematicSection {
                wires: vec![
                    Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![Point { x: Nm::ZERO, y: mm(10.0) }, Point { x: mm(20.0), y: mm(10.0) }], bus: false },
                    Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(20.0) }], bus: false },
                ],
                labels: vec![
                    eda_model::ir::NetLabel { id: String::new(), net: "H".into(), at: Point { x: Nm::ZERO, y: mm(10.0) }, kind: LabelKind::Local },
                    eda_model::ir::NetLabel { id: String::new(), net: "V".into(), at: Point { x: mm(10.0), y: Nm::ZERO }, kind: LabelKind::Local },
                ],
                junctions,
                lines: vec![eda_model::ir::SchLine { id: String::new(), pts: vec![Point { x: Nm::ZERO, y: mm(30.0) }, Point { x: mm(5.0), y: mm(30.0) }, Point { x: mm(5.0), y: mm(35.0) }], width_nm: mm(0.254) }],
                ..SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
            };
            let design = Design {
                schema: eda_model::ir::SCHEMA_VERSION,
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
        assert_ne!(net_of(&apart, Point { x: Nm::ZERO, y: mm(10.0) }), net_of(&apart, Point { x: mm(10.0), y: Nm::ZERO }), "crossing wires with no junction stay two nets");
        assert!(apart.junctions.is_empty(), "no junction written or read back");

        let (text, joined) = build(vec![eda_model::ir::Junction { id: String::new(), at: cross }]);
        assert_eq!(net_of(&joined, Point { x: Nm::ZERO, y: mm(10.0) }), net_of(&joined, Point { x: mm(10.0), y: Nm::ZERO }), "the junction joins them");
        assert!(text.contains("(junction (at 10 10)"), "{text}");
        assert_eq!(joined.junctions.len(), 1);
        assert_eq!(joined.junctions[0].at, cross);

        // a line of three points is written as KiCad's line tool saves it, a polyline of two points per segment (KiCad reads a longer one as a polygon shape),
        // and read back as those two lines
        assert_eq!(text.matches("(polyline").count(), 2, "{text}");
        assert_eq!(joined.lines.len(), 2);
        let mut segs: Vec<Vec<Point>> = joined.lines.iter().map(|l| l.pts.clone()).collect();
        segs.sort();
        assert_eq!(segs, vec![vec![Point { x: Nm::ZERO, y: mm(30.0) }, Point { x: mm(5.0), y: mm(30.0) }], vec![Point { x: mm(5.0), y: mm(30.0) }, Point { x: mm(5.0), y: mm(35.0) }]]);
        assert!(joined.lines.iter().all(|l| l.width_nm == mm(0.254)));
        assert_eq!(joined.wires.len(), 2, "a graphic line is never read back as a wire");
    }

    /// The strokes Wire/Bus Properties sets on a wire, a bus, a bus entry and a graphic line, and the diameter and colour of a junction, are written as KiCad
    /// writes them and read back; an item left at the defaults writes the defaults and reads back with no annotation at all.
    #[test]
    fn strokes_and_junction_looks_round_trip_through_a_kicad_sch_file() {
        use eda_model::sch_extras::{JunctionLook, SchColor, SchLineStyle};
        let p = |x: Nm, y: Nm| Point { x, y };
        let red = SchColor { r: 255, g: 0, b: 0, a: 255 };
        let mut sch = SchematicSection::default();
        sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(20.0), Nm::ZERO)], bus: false });
        sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![p(Nm::ZERO, mm(10.0)), p(mm(20.0), mm(10.0))], bus: true });
        sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![p(Nm::ZERO, mm(20.0)), p(mm(20.0), mm(20.0))], bus: false });
        sch.bus_entries.push(eda_model::ir::BusEntry { id: String::new(), at: p(mm(5.0), mm(10.0)), size: p(mm(2.54), mm(2.54)) });
        sch.lines.push(eda_model::ir::SchLine { id: String::new(), pts: vec![p(Nm::ZERO, mm(30.0)), p(mm(5.0), mm(30.0))], width_nm: mm(0.254) });
        sch.junctions.push(eda_model::ir::Junction { id: String::new(), at: p(mm(10.0), Nm::ZERO) });
        sch.junctions.push(eda_model::ir::Junction { id: String::new(), at: p(mm(10.0), mm(20.0)) });
        sch.assign_missing_ids();
        let id_at = |sch: &SchematicSection, first: Point| sch.wires.iter().find(|w| w.pts[0] == first).unwrap().id.clone();
        let (plain_id, bus_id) = (id_at(&sch, p(Nm::ZERO, Nm::ZERO)), id_at(&sch, p(Nm::ZERO, mm(10.0))));
        let (bent_id, line_id) = (sch.bus_entries[0].id.clone(), sch.lines[0].id.clone());
        let jct_id = sch.junctions[0].id.clone();
        let plain = SchStroke { width_nm: mm(0.3), style: SchLineStyle::Dash, color: Some(red) };
        let bus = SchStroke { width_nm: Nm::ZERO, style: SchLineStyle::Dot, color: None };
        let bent = SchStroke { width_nm: mm(0.2), style: SchLineStyle::Default, color: Some(SchColor { r: 0, g: 0, b: 255, a: 255 }) };
        let line = SchStroke { width_nm: Nm::ZERO, style: SchLineStyle::DashDot, color: Some(SchColor { r: 0, g: 128, b: 0, a: 255 }) };
        sch.extras.strokes.extend([(plain_id, plain), (bus_id, bus), (bent_id, bent), (line_id, line)]);
        sch.extras.junction_looks.insert(jct_id, JunctionLook { diameter_nm: mm(0.9), color: Some(red) });
        let design = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
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
        let text = crate::export_kicad_sch(&design, &ConstraintModel::default(), &crate::ExportMeta { date: "2026-01-01", title: "strokes" }).expect("exports");
        for token in ["(stroke (width 0.3) (type dash) (color 255 0 0 1))", "(stroke (width 0) (type dot))", "(stroke (width 0.2) (type default) (color 0 0 255 1))", "(stroke (width 0.254) (type dash_dot) (color 0 128 0 1))", "(junction (at 10 0) (diameter 0.9) (color 255 0 0 1)", "(junction (at 10 20) (diameter 0) (color 0 0 0 0)"] {
            assert!(text.contains(token), "{token} in\n{text}");
        }
        let (back, _model, _notes) = import_kicad_sch(&text).expect("re-imports");
        let back = back.schematic.expect("schematic");
        let stroke_of_wire = |first: Point| back.extras.strokes.get(&id_at(&back, first)).copied();
        assert_eq!(stroke_of_wire(p(Nm::ZERO, Nm::ZERO)), Some(plain));
        assert_eq!(stroke_of_wire(p(Nm::ZERO, mm(10.0))), Some(bus));
        assert_eq!(stroke_of_wire(p(Nm::ZERO, mm(20.0))), None, "the default stroke is not an annotation");
        assert_eq!(back.extras.strokes.get(&back.bus_entries[0].id), Some(&bent));
        assert_eq!(back.extras.strokes.get(&back.lines[0].id), Some(&line));
        assert_eq!(back.lines[0].width_nm, mm(0.254), "a graphic line's width stays its own");
        assert_eq!(back.extras.strokes.len(), 4);
        let look_at = |at: Point| back.junctions.iter().find(|j| j.at == at).and_then(|j| back.extras.junction_looks.get(&j.id)).copied();
        assert_eq!(look_at(p(mm(10.0), Nm::ZERO)), Some(JunctionLook { diameter_nm: mm(0.9), color: Some(red) }));
        assert_eq!(look_at(p(mm(10.0), mm(20.0))), None);
    }

    /// Shapes, text boxes, rule areas, directive labels and every item's lock survive the `.kicad_sch` round trip.
    #[test]
    fn drawn_graphics_and_locks_round_trip_through_a_kicad_sch_file() {
        use eda_model::sch_extras::{DirectiveShape, SchFill, SchGraphic, SchGraphicKind, SchHAlign, SchVAlign};
        let p = |x: Nm, y: Nm| Point { x, y };
        let mut sch = SchematicSection::default();
        sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(20.0), Nm::ZERO)], bus: false });
        sch.texts.push(SchematicText { id: String::new(), content: "note".into(), at: p(mm(5.0), mm(5.0)), angle: 0, size_nm: mm(1.27) });
        let mut rect = SchGraphic::new(SchGraphicKind::Rectangle { start: p(Nm::ZERO, mm(10.0)), end: p(mm(20.0), mm(30.0)), corner_radius_nm: Nm::ZERO });
        rect.fill = SchFill::Background;
        sch.extras.graphics = vec![
            rect,
            SchGraphic::new(SchGraphicKind::TextBox { start: p(Nm::ZERO, mm(40.0)), end: p(mm(30.0), mm(50.0)), text: "boxed".into(), angle: 0, size_nm: mm(1.27), bold: false, italic: true, h_align: SchHAlign::Left, v_align: SchVAlign::Top, margin_nm: Nm::ZERO }),
            SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(Nm::ZERO, mm(60.0)), p(mm(10.0), mm(60.0)), p(mm(10.0), mm(70.0))], exclude_from_sim: false, exclude_from_bom: false, exclude_from_board: false, dnp: true }),
            SchGraphic::new(SchGraphicKind::Directive { at: p(mm(20.0), Nm::ZERO), orientation: 0, shape: DirectiveShape::Round, pin_length_nm: mm(2.54), netclass: "Power".into(), component_class: String::new() }),
        ];
        sch.assign_missing_ids();
        let wire_id = sch.wires[0].id.clone();
        let text_id = sch.texts[0].id.clone();
        let rect_id = sch.extras.graphics[0].id.clone();
        for id in [&wire_id, &text_id, &rect_id] {
            sch.extras.set_locked(id, true);
        }
        let design = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch.clone()),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        let text = crate::export_kicad_sch(&design, &ConstraintModel::default(), &crate::ExportMeta { date: "2026-01-01", title: "graphics" }).expect("exports");
        for token in ["(rectangle", "(text_box \"boxed\"", "(rule_area", "(netclass_flag", "(locked yes)"] {
            assert!(text.contains(token), "{token} in\n{text}");
        }
        let (back, _model, _notes) = import_kicad_sch(&text).expect("re-imports");
        let back = back.schematic.expect("schematic");
        let mut kinds_before: Vec<&str> = sch.extras.graphics.iter().map(|g| g.shape.id_prefix()).collect();
        let mut kinds_after: Vec<&str> = back.extras.graphics.iter().map(|g| g.shape.id_prefix()).collect();
        kinds_before.sort();
        kinds_after.sort();
        assert_eq!(kinds_before, kinds_after, "every graphic comes back");
        assert_eq!(back.extras.locked, sch.extras.locked, "the same three items are locked: {:?}", back.extras.locked);
        assert!(matches!(back.extras.graphics.iter().find(|g| g.shape.id_prefix() == "rarea").map(|g| &g.shape), Some(SchGraphicKind::RuleArea { dnp: true, .. })));
    }
}
