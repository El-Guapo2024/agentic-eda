//! Hierarchical sheet flattening (GAPS.md #6/#20): walks a `Design`'s sheet
//! tree (the root `schematic` plus every other sheet's own content in
//! `Design::sheet_contents`, joined by each sheet's own `SchematicSection::sheets`
//! placements) into the *one* netlist every other part of this project
//! already assumes `Design::nets`/`ConstraintModel::nets` to be, and into
//! one flattened `SchematicSection` for ERC and any other consumer that
//! only understands a single flat sheet.
//!
//! Ported from `eeschema/connection_graph.cpp` (`CONNECTION_GRAPH::
//! buildConnectionGraph`/`propagateToNeighbors`), scoped to what this
//! project's own `reconcile` union-find already does well (one sheet's own
//! point-geometry) plus the two things real hierarchy actually adds:
//!
//! - A LOCAL label's net is scoped to its own sheet *instance*, not just
//!   its own file: two placements of the exact same child screen, each
//!   with a same-named local label, must NOT merge (`PathHumanReadable`'s
//!   own role in `SCH_CONNECTION::recacheName`). Implemented by running
//!   `reconcile` once *per visited sheet instance*, with that instance's
//!   own local *and* hierarchical labels' net names prefixed by its own
//!   path -- hierarchical labels start out scoped exactly like locals
//!   because within one sheet there is nothing yet to tell them apart
//!   electrically; the cross-sheet join below is what gives a
//!   hierarchical label its real, wider meaning.
//! - A GLOBAL label (and a power symbol's own asserted net -- `GND`
//!   anywhere means the same `GND` everywhere, with no label needed at
//!   all, exactly as real KiCad's power pins behave) merges across the
//!   *whole* hierarchy: left unprefixed, so the same name-based merge
//!   `reconcile` already does within one sheet now also fires across
//!   sheets for anything carrying a bare, unprefixed name.
//! - A sheet pin <-> child hierarchical label pair, matched *by name only*
//!   (confirmed against `connection_graph.cpp::ercCheckHierSheets`, which
//!   never compares shape -- see `crate::erc::check_hier_label_mismatch`'s
//!   own doc), ties exactly that one parent-instance's local net to that
//!   one child-instance's local net: a second pass unions the two
//!   (path-qualified) net names together wherever a sheet pin's own
//!   position and a child's own hierarchical label agree on a name.
//! - A *bus*-shaped sheet pin (GAPS.md #20 -- `crate::bus::expand_bus_members`)
//!   joins member-by-member instead: each of its expanded member names
//!   (e.g. `"DATA3"` out of a `"DATA[0..7]"` pin) gets its own
//!   parent-scoped name unioned directly with the same member's
//!   child-scoped name, with no requirement that the two sides' bus *names*
//!   match textually (a `"DATA[0..7]"` parent pin and a `"DATA[0..3]"`
//!   child hierarchical label is a valid partial join -- real KiCad's own
//!   "any shared member is fine" rule, reported when it *isn't* met by
//!   `erc::check_hierarchy`'s own `bus_to_bus_conflict`, not here).

use std::collections::BTreeMap;

use eda_model::ir::{Design, LabelKind, NetLabel, Point, SchematicSection, SymbolInstance};
use eda_model::{ConstraintModel, Net};

use crate::sch_import::{reconcile as reconcile_sheet, transform_local_point};

/// One visited sheet instance's own reconciled state, kept around just
/// long enough for the cross-sheet join pass below to consult.
struct Visited {
    /// Sheet-instance ids from the root down to (and including) this
    /// sheet's own placement; empty for the root itself.
    path: Vec<String>,
    /// This instance's own content, with every symbol's `id`/`unit`
    /// already resolved through `SchematicSection::instance_overrides`
    /// for this exact `path` (see `resolve_overrides`).
    sch: SchematicSection,
    /// This instance's own nets, from reconciling *only* its own
    /// geometry (local/hierarchical label net names path-prefixed, as
    /// described in this module's own doc) -- a `Net::name` here is
    /// either a bare global name or `"@<path>@<local-net-text>"`.
    nets: Vec<Net>,
    /// Which of `nets` (by name) a given child `SheetInstance::id` +
    /// `SheetPin::name` pair resolved to on *this* (the parent) side --
    /// populated by feeding each child sheet's own pins into this
    /// instance's own `reconcile` call as synthetic anchor labels (a
    /// sheet pin is a point in the *parent's* coordinate space, exactly
    /// like a label or a power symbol).
    sheet_pin_net: BTreeMap<(String, String), String>,
}

/// `"shpin:<child sheet-instance id>:<pin name>"` -- a synthetic, globally
/// unique local-label net name standing in for one child sheet's own pin
/// while it participates in the parent's own point-geometry reconcile
/// pass. Never shown to a user; stripped back out (via `sheet_pin_net`)
/// once that pass is done.
fn sheet_pin_anchor_name(child_sheet_id: &str, pin_name: &str) -> String {
    format!("shpin:{child_sheet_id}:{pin_name}")
}

fn path_prefix(path: &[String]) -> String {
    path.join("/")
}

/// A label's net name once this sheet instance's own scoping is applied:
/// bare (global/power -- merges across the whole hierarchy, by the exact
/// same "same name = same net" mechanism `reconcile` already uses) or
/// always wrapped `@<path>@<raw name>` (local/hierarchical -- scoped to
/// this one instance, until the cross-sheet join pass explicitly unions a
/// hierarchical one) -- unconditionally wrapped even at the root (`path`
/// empty, giving `"@@VCC"`), so a root-level *local* "VCC" can never
/// accidentally collide with an unrelated *global* "VCC" label elsewhere
/// just because both happened to produce the same bare string; only a
/// label actually marked global ever uses the bare form.
fn scoped_name(prefix: &str, raw_net: &str, global: bool) -> String {
    if global {
        raw_net.to_string()
    } else {
        format!("@{prefix}@{raw_net}")
    }
}

/// Inverse of `scoped_name`'s non-global branch: strips the `@<path>@`
/// wrapper back off, for a final net's own display name (see
/// `flatten`'s own doc on why -- grouping correctness and display-name
/// cleanliness are two separate concerns). A bare (global) name has no
/// wrapper to strip and passes through unchanged.
fn unwrap_scoped(name: &str) -> String {
    if let Some(rest) = name.strip_prefix('@') {
        if let Some(idx) = rest.find('@') {
            return rest[idx + 1..].to_string();
        }
    }
    name.to_string()
}

/// Apply this screen's own `SymbolPathOverride`s (only ever non-empty when
/// `sch.instance_overrides` -- i.e. this exact *content* -- is placed by
/// more than one `SheetInstance`) for the one specific placement named by
/// `parent_sheet_instance_id` (the last component of this instance's own
/// path; `None` for the root, which is never placed by anyone). A symbol
/// with no matching override (the overwhelming common case -- a sheet
/// placed exactly once) keeps its own `id`/`unit` unchanged.
fn resolve_overrides(sch: &SchematicSection, parent_sheet_instance_id: Option<&str>) -> Vec<SymbolInstance> {
    let Some(parent_id) = parent_sheet_instance_id else { return sch.symbols.clone() };
    if sch.instance_overrides.is_empty() {
        return sch.symbols.clone();
    }
    sch.symbols
        .iter()
        .map(|s| {
            let Some(ov) = sch.instance_overrides.iter().find(|o| o.at == s.at && o.parent_sheet_instance_id == parent_id) else { return s.clone() };
            SymbolInstance { id: ov.reference.clone(), unit: ov.unit, ..s.clone() }
        })
        .collect()
}

/// Every real pin's own world position for `symbols` (same "resolve a
/// part's real/synthetic library symbol, transform each pin by this
/// instance's rotation/mirror" logic `reconcile_schematic`/`import_kicad_sch`
/// both already have their own copy of -- a third copy here, same
/// precedent those two already set, rather than a cross-module refactor
/// this task's own "keep shared-file edits small" rule argues against).
/// Multi-unit-aware: a pin not on this specific instance's own unit (and
/// not a `unit == 0` common one) is skipped, exactly like those two.
fn build_pin_world(symbols: &[SymbolInstance], model: &ConstraintModel) -> BTreeMap<String, Point> {
    let mut pin_world = BTreeMap::new();
    for sym in symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let resolved = if sym.lib_id.is_empty() || eda_model::is_synthetic_lib_id(&sym.lib_id) { None } else { model.symbol_of(&sym.lib_id) };
        let angle_deg = sym.rot as f64 / 1000.0;
        for p in &part.pins {
            let real_pin = resolved.as_ref().and_then(|s| s.pin_by_number(&p.number));
            if let Some(rp) = real_pin {
                if !(rp.unit == 0 || rp.unit == sym.unit) {
                    continue;
                }
            }
            let Some(lib_pin_at) = real_pin.map(|rp| rp.at) else { continue };
            let world = transform_local_point(lib_pin_at, angle_deg, sym.mirrored, sym.mirror_y);
            pin_world.insert(format!("{}.{}", sym.id, p.number), Point { x: sym.at.x + crate::import::mm_to_um(world.x), y: sym.at.y + crate::import::mm_to_um(world.y) });
        }
    }
    pin_world
}

/// Visits `sch` (found at `path`, having been placed -- if non-root -- by
/// `parent_sheet_instance_id`), reconciles its own geometry in isolation,
/// and recurses into every child sheet `design.sheet_contents` has an
/// entry for (a sheet naming a file this design never descended into is
/// left a leaf -- same "can't see inside it" limitation a bare single-file
/// `import_kicad_sch` always had, just now scoped to one branch instead of
/// the whole design).
#[allow(clippy::too_many_arguments)]
fn visit(design: &Design, model: &ConstraintModel, sch: &SchematicSection, path: Vec<String>, parent_sheet_instance_id: Option<&str>, out: &mut Vec<Visited>) {
    let prefix = path_prefix(&path);
    let symbols = resolve_overrides(sch, parent_sheet_instance_id);
    let pin_world = build_pin_world(&symbols, model);

    let mut wires = sch.wires.clone();
    let mut power_symbols = sch.power_symbols.clone();
    let mut no_connects = sch.no_connects.clone();

    let real_labels: Vec<NetLabel> = sch
        .labels
        .iter()
        .map(|l| {
            let global = matches!(l.kind, LabelKind::Global { .. });
            NetLabel { net: scoped_name(&prefix, &l.net, global), ..l.clone() }
        })
        .collect();
    let mut labels = real_labels.clone();
    // Every power symbol's own net name is scoped like a global label --
    // real KiCad power pins connect across the whole hierarchy with no
    // label needed at all, the same "same name = same net, anywhere"
    // mechanism this project's own single-sheet `reconcile` already uses
    // for a power symbol's `net`, now simply left unprefixed so it keeps
    // doing that across sheet boundaries too.

    // This sheet's own children's pins, as synthetic local-scope anchor
    // labels participating in *this* sheet's own point-geometry pass (a
    // sheet pin is drawn on the parent's own border, in the parent's own
    // coordinate space -- electrically no different from a label or a
    // power symbol landing there).
    for child in &sch.sheets {
        for pin in &child.pins {
            labels.push(NetLabel { id: String::new(), net: scoped_name(&prefix, &sheet_pin_anchor_name(&child.id, &pin.name), false), at: pin.at, kind: LabelKind::Local });
        }
    }

    let nets = reconcile_sheet(&pin_world, &mut wires, &labels, &mut power_symbols, &mut no_connects);

    // Which net each child sheet's own pin landed on, read straight off the
    // now-reconciled geometry (`net_at_point`, shared with
    // `erc::check_multi_unit_symbols`'s own identical need) rather than by
    // matching the synthetic anchor's own name back against `nets`: once a
    // real wire/label/power-symbol touches the exact same point (the
    // ordinary, expected case -- a wire drawn from the sheet pin's border
    // out to wherever it's actually used), `reconcile`'s own tie-break
    // prefers that real name over the anchor's placeholder, so a name-based
    // lookup would miss it. Only when *nothing* else touches the pin's own
    // point (an otherwise-isolated sheet pin) does its net keep the
    // anchor's own synthetic name -- still found correctly, by the same
    // geometry check, since `net_at_point` falls through to it below.
    // Deliberately checked against `real_labels` (no synthetic anchors),
    // not the combined `labels` reconcile just used: a sheet pin's own
    // anchor sits at this *exact* point by construction, so checking the
    // combined list would always "find" the anchor itself first and never
    // reach the real power-symbol/label underneath it.
    let mut sheet_pin_net: BTreeMap<(String, String), String> = BTreeMap::new();
    for child in &sch.sheets {
        for pin in &child.pins {
            let anchor = scoped_name(&prefix, &sheet_pin_anchor_name(&child.id, &pin.name), false);
            let net = crate::erc::net_at_point(&wires, &real_labels, &power_symbols, pin.at).unwrap_or(anchor);
            sheet_pin_net.insert((child.id.clone(), pin.name.clone()), net);
        }
    }

    out.push(Visited {
        path: path.clone(),
        sch: SchematicSection {
            symbols,
            wires,
            labels: real_labels,
            texts: sch.texts.clone(),
            power_symbols,
            no_connects,
            bus_entries: sch.bus_entries.clone(),
            erc_exclusions: sch.erc_exclusions.clone(),
            title_block: sch.title_block.clone(),
            sheets: sch.sheets.clone(),
            instance_overrides: Vec::new(),
            imported_from_kicad: sch.imported_from_kicad,
        },
        nets,
        sheet_pin_net,
    });

    for child in &sch.sheets {
        let Some(design_sheets) = &design.sheet_contents else { continue };
        let Some(child_content) = design_sheets.get(&child.file) else { continue };
        let mut child_path = path.clone();
        child_path.push(child.id.clone());
        visit(design, model, child_content, child_path, Some(child.id.as_str()), out);
    }
}

/// Tiny union-find over net-name strings -- the cross-sheet join pass's own
/// equivalent of `sch_import::reconcile`'s point union-find, operating on
/// net *names* instead of points since every name involved is already
/// globally unique across the whole flattened design (path-prefixed local/
/// hierarchical names, or bare global ones).
#[derive(Default)]
struct NameUnionFind {
    parent: BTreeMap<String, String>,
}

impl NameUnionFind {
    fn find(&mut self, x: &str) -> String {
        let Some(p) = self.parent.get(x).cloned() else {
            self.parent.insert(x.to_string(), x.to_string());
            return x.to_string();
        };
        if p == x {
            return p;
        }
        let root = self.find(&p);
        self.parent.insert(x.to_string(), root.clone());
        root
    }
    fn union(&mut self, a: &str, b: &str) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

/// Flattens `design`'s whole sheet tree into one `SchematicSection` (every
/// symbol/wire/label/power-symbol/no-connect from every visited sheet, with
/// each screen's own symbol references already resolved through
/// `instance_overrides`) and the `Vec<Net>` that implies -- this module's
/// own doc has the full algorithm. A single-sheet design (`design.
/// sheet_contents` is `None`, or empty, i.e. everything before this module
/// existed) visits only the root with an empty path prefix, which makes
/// every "scope by path" step above a no-op: the result is byte-for-byte
/// what plain `reconcile` on the root alone would already give.
pub fn flatten(design: &Design, model: &ConstraintModel) -> Option<(SchematicSection, Vec<Net>)> {
    let root = design.schematic.as_ref()?;
    let mut visited: Vec<Visited> = Vec::new();
    visit(design, model, root, Vec::new(), None, &mut visited);

    // Cross-sheet join: for every parent's own record of what net each of
    // its children's pins landed on, union that with whatever net the
    // child's own matching hierarchical label resolved to on *its* side.
    // A pin with no matching label (or vice versa) is exactly
    // `hier_label_mismatch` -- left unmerged here, not an error in this
    // function; `erc::check_hier_label_mismatch` is what reports it.
    let mut uf = NameUnionFind::default();
    for v in &visited {
        let parent_prefix = path_prefix(&v.path);
        for child in &v.sch.sheets {
            let Some(child_visited) = visited.iter().find(|c| c.path == { let mut p = v.path.clone(); p.push(child.id.clone()); p }) else { continue };
            let child_prefix = path_prefix(&child_visited.path);
            for pin in &child.pins {
                // A bus-shaped pin (GAPS.md #20) joins member-by-member
                // instead of as one single name: `sheet_pin_net`'s anchor
                // only ever captures the pin's own *one* point, but a
                // bus's individual signals are tapped in via a
                // `BusEntry` elsewhere on each sheet's own canvas (see
                // `crate::bus`'s own doc), so each member's scoped name is
                // constructed directly on both sides rather than looked
                // up. Harmless when a given member is unused on either
                // side -- a pin-less union-find group never survives the
                // final collapse below, same as always. This does not
                // require the two sides to *name* the bus identically
                // (`"DATA[0..7]"` parent, `"DATA[0..3]"` child is a valid
                // partial join) -- `erc::check_hierarchy`'s own
                // `bus_to_bus_conflict` is what reports when they share no
                // member at all; this function just joins whatever they do
                // share.
                if let Some(members) = crate::bus::expand_bus_members(&pin.name, &design.bus_aliases) {
                    for member in &members {
                        uf.union(&scoped_name(&parent_prefix, member, false), &scoped_name(&child_prefix, member, false));
                    }
                    continue;
                }
                let Some(parent_net) = v.sheet_pin_net.get(&(child.id.clone(), pin.name.clone())) else { continue };
                // The child's own hierarchical label of the same name, if
                // it declared one -- `child_visited.sch.labels` already
                // holds each label's *scoped* name (`visit` stores them
                // that way), so the comparison has to be scoped-to-scoped,
                // not against `pin.name`'s own raw text.
                let child_net = scoped_name(&child_prefix, &pin.name, false);
                let has_matching_hier_label = child_visited.sch.labels.iter().any(|l| matches!(l.kind, LabelKind::Hierarchical { .. }) && l.net == child_net);
                if !has_matching_hier_label {
                    continue;
                }
                uf.union(parent_net, &child_net);
            }
        }
    }

    // Collapse: every visited sheet's own nets, grouped by their union-find
    // root, merged pin-lists, named after the "best" member of the group
    // (a bare/global name first, else the shortest/lexicographically-first
    // scoped name -- never a synthetic `shpin:` placeholder when any real
    // name is available in the same group).
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pins_by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in &visited {
        for n in &v.nets {
            groups.entry(uf_find_const(&uf, &n.name)).or_default().push(n.name.clone());
            pins_by_name.entry(n.name.clone()).or_default().extend(n.pins.iter().cloned());
        }
    }
    // A sheet-pin's own resolved name (`sheet_pin_net`'s values -- the
    // *real* label/power-symbol/wire name `net_at_point` found, per that
    // field's own doc) must be considered as a naming candidate even when
    // it never heads its own pin-bearing `Net` in `v.nets`: a sheet pin
    // wired only to a power symbol (nothing else of this sheet's own
    // real parts touches that exact point) is real and common, but
    // `reconcile`'s own "a net needs at least one real pin" rule means
    // such a point produces no entry in `v.nets` at all on *this* sheet's
    // side -- the name still has to survive into the union-find's name
    // pool, or the join below would silently fall back to the far less
    // meaningful synthetic/other-side name once it finds nothing else to
    // prefer. Adding it with no extra pins (it contributes none of its
    // own beyond whatever its union-find group already collected) is
    // exactly what's needed for the "best name" selection to find it.
    for v in &visited {
        for name in v.sheet_pin_net.values() {
            groups.entry(uf_find_const(&uf, name)).or_default().push(name.clone());
        }
    }
    let is_global = |name: &str| !name.starts_with('@');
    // `scoped_name` always renders an anchor's own raw name as
    // `"shpin:<id>:<pin>"`, wrapped `@<prefix>@shpin:...` -- a substring
    // check is enough since nothing else this module ever names a net
    // produces that literal text.
    let is_synthetic = |name: &str| name.contains("shpin:");
    struct Pending {
        chosen: String,
        pins: Vec<String>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    for (_, members) in groups {
        let mut pins: Vec<String> = members.iter().flat_map(|m| pins_by_name.get(m).cloned().unwrap_or_default()).collect();
        pins.sort();
        pins.dedup();
        if pins.is_empty() {
            continue; // a pin-less group is either a lone unmatched sheet-pin anchor or an empty net -- nothing for ERC/export to see
        }
        let chosen = members
            .iter()
            .filter(|m| is_global(m))
            .min()
            .or_else(|| members.iter().filter(|m| !is_synthetic(m)).min())
            .or_else(|| members.iter().min())
            .cloned()
            .unwrap_or_default();
        pending.push(Pending { chosen, pins });
    }
    // Display/debug name: stripped of its own `@<path>@` wrapper (if any)
    // back to the plain text a label/power-symbol actually carries -- the
    // wrapper's whole job was keeping *grouping* correct during the
    // union-find above, a settled question by this point, so the common
    // case (single-sheet, or a hierarchy join that reached a real name)
    // should read exactly like it always did, e.g. `"VCC"` rather than
    // `"@@VCC"`. But two *different*, legitimately-unmerged nets (a local
    // label with the same text reused in two unrelated sheet instances,
    // say) can unwrap to the exact same plain text -- `Net::name` is a
    // lookup key throughout this project (`Wire::net`, `NetLabel::net`,
    // every ERC net-name lookup), so silently colliding two distinct nets
    // onto one name would be a real, if rare, correctness bug, not just an
    // ugly string. Only ever unwrap when the result is unique across this
    // whole flattened design; the rarer colliding case keeps its full,
    // unambiguous scoped name instead.
    let mut unwrapped_count: BTreeMap<String, usize> = BTreeMap::new();
    for p in &pending {
        *unwrapped_count.entry(unwrap_scoped(&p.chosen)).or_default() += 1;
    }
    let mut final_nets: Vec<Net> = pending
        .into_iter()
        .map(|p| {
            let unwrapped = unwrap_scoped(&p.chosen);
            let name = if unwrapped_count[&unwrapped] == 1 { unwrapped } else { p.chosen };
            Net { name, pins: p.pins }
        })
        .collect();
    final_nets.sort_by(|a, b| a.name.cmp(&b.name));

    let mut combined = SchematicSection {
        symbols: Vec::new(),
        wires: Vec::new(),
        labels: Vec::new(),
        texts: Vec::new(),
        power_symbols: Vec::new(),
        no_connects: Vec::new(),
        bus_entries: Vec::new(),
        erc_exclusions: root.erc_exclusions.clone(),
        title_block: root.title_block.clone(),
        sheets: root.sheets.clone(),
        instance_overrides: Vec::new(),
        imported_from_kicad: root.imported_from_kicad,
    };
    for v in visited {
        combined.symbols.extend(v.sch.symbols);
        combined.wires.extend(v.sch.wires);
        combined.labels.extend(v.sch.labels);
        combined.texts.extend(v.sch.texts);
        combined.power_symbols.extend(v.sch.power_symbols);
        combined.no_connects.extend(v.sch.no_connects);
    }
    Some((combined, final_nets))
}

/// `NameUnionFind::find`, but over a `&self` (path compression already
/// happened during the union pass above; this is just a read for the final
/// collapse, which needs no further mutation).
fn uf_find_const(uf: &NameUnionFind, x: &str) -> String {
    let mut cur = x.to_string();
    loop {
        match uf.parent.get(&cur) {
            Some(p) if p != &cur => cur = p.clone(),
            _ => return cur,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{LabelShape, PowerSymbol, Provenance, SheetInstance, SheetPin};
    use eda_model::{Part, Pin, PinKind};
    use std::collections::BTreeMap as Map;

    fn r1_part() -> Part {
        Part { reference: "R1".into(), mpn: None, lcsc: None, value: None, package: None, footprint: None, symbol: Some("Device:R".into()), datasheet: None, pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None }
    }

    fn r1_instance(at: Point) -> SymbolInstance {
        SymbolInstance { id: "R1".into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() }
    }

    fn empty_sch(symbols: Vec<SymbolInstance>) -> SchematicSection {
        SchematicSection { symbols, wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], imported_from_kicad: false }
    }

    fn design_with(root: SchematicSection, screens: Map<String, SchematicSection>) -> Design {
        Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: (!screens.is_empty()).then_some(screens),
            bus_aliases: vec![], symbol_library: None,
        }
    }

    /// Device:R's own real pin 1 world position when placed unrotated at
    /// `at` -- pin 1 sits at library `(0, 3.81)` (`eda_model::symbol::builtin`),
    /// which `transform_local_point` (unrotated, unmirrored) puts at sheet
    /// `(at.x, at.y - 3810)` um (library +y-up -> sheet +y-down negation,
    /// same convention every other pin-position call site in this crate
    /// already relies on).
    fn r1_pin1_world(at: Point) -> Point {
        Point { x: at.x, y: at.y - 3810 }
    }

    /// A design with no sheets at all flattens to exactly its own root
    /// content, net-for-net -- `flatten` must be a true no-op for every
    /// design this project had before hierarchy support existed.
    #[test]
    fn single_sheet_design_flattens_to_itself() {
        let model = ConstraintModel { parts: vec![r1_part()], ..Default::default() };
        let root = empty_sch(vec![r1_instance(Point { x: 0, y: 0 })]);
        let design = design_with(root, Map::new());
        let (flat, nets) = flatten(&design, &model).expect("root exists");
        assert_eq!(flat.symbols.len(), 1);
        assert_eq!(flat.symbols[0].id, "R1");
        // Two isolated pins, nothing wiring them together -- two separate,
        // synthesized-name nets (`reconcile`'s own documented fallback),
        // neither of them carrying a leftover "@...@" scope marker.
        assert_eq!(nets.len(), 2);
        assert!(nets.iter().all(|n| !n.name.contains('@')));
    }

    /// The whole point of hierarchy: a sheet pin on the root, tied *by
    /// name* to a hierarchical label inside the child, joins the child's
    /// own resistor onto whatever net the root's own geometry assigns that
    /// sheet pin's position to (here, a GND power symbol sitting exactly
    /// on it) -- pin 1 ends up on the joined net, pin 2 (nothing else
    /// connects it) stays on its own, separate, sheet-scoped net.
    #[test]
    fn sheet_pin_joins_the_childs_hierarchical_label_to_the_parents_net() {
        let model = ConstraintModel { parts: vec![r1_part()], ..Default::default() };

        let pin_at = Point { x: 5_000, y: 10_000 };
        let root = SchematicSection {
            sheets: vec![SheetInstance { id: "sheetA".into(), name: "child".into(), file: "child.kicad_sch".into(), at: Point { x: 0, y: 0 }, size: (10_000, 10_000), pins: vec![SheetPin { id: "p1".into(), name: "AD0".into(), shape: LabelShape::Passive, at: pin_at }] }],
            power_symbols: vec![PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: pin_at, rot: 0, net: "GND".into(), pin: String::new() }],
            ..empty_sch(vec![])
        };

        let r1_at = Point { x: 0, y: 0 };
        let child = SchematicSection {
            labels: vec![NetLabel { id: String::new(), net: "AD0".into(), at: r1_pin1_world(r1_at), kind: LabelKind::Hierarchical { shape: LabelShape::Passive } }],
            ..empty_sch(vec![r1_instance(r1_at)])
        };

        let mut screens = Map::new();
        screens.insert("child.kicad_sch".to_string(), child);
        let design = design_with(root, screens);

        let (flat, nets) = flatten(&design, &model).expect("root exists");
        assert_eq!(flat.symbols.len(), 1, "R1 from the child screen made it into the flattened view");

        let joined = nets.iter().find(|n| n.pins.contains(&"R1.1".to_string())).unwrap_or_else(|| panic!("R1.1 on no net at all: {nets:#?}"));
        assert_eq!(joined.name, "GND", "the join picked the root's own real net name, not a synthetic/scoped placeholder");
        assert!(!nets.iter().any(|n| n.pins.contains(&"R1.2".to_string()) && n.name == "GND"), "R1.2 must NOT be swept into the joined net");
    }

    /// Two *different* child sheets, each with their own local label of
    /// the exact same text and no hierarchical label/sheet-pin tying them
    /// together, must stay on separate nets -- the core "local labels
    /// don't leak across sheet instances" rule this whole module exists to
    /// enforce (`connection_graph.cpp`'s own `PathHumanReadable`-qualified
    /// net names).
    #[test]
    fn same_named_local_labels_in_different_sheets_do_not_merge() {
        let model = ConstraintModel { parts: vec![r1_part()], ..Default::default() };
        let root = SchematicSection {
            sheets: vec![
                SheetInstance { id: "sheetA".into(), name: "a".into(), file: "leaf.kicad_sch".into(), at: Point { x: 0, y: 0 }, size: (1000, 1000), pins: vec![] },
                SheetInstance { id: "sheetB".into(), name: "b".into(), file: "leaf.kicad_sch".into(), at: Point { x: 20_000, y: 0 }, size: (1000, 1000), pins: vec![] },
            ],
            ..empty_sch(vec![])
        };
        // The *same* file placed twice (a shared screen) -- each instance
        // still gets its own scope purely from its own sheet-instance path,
        // with no `instance_overrides` needed at all for this test (R1's
        // own bare id is fine to repeat across the two instances' own
        // flattened output; this test is about *net* scoping, not
        // reference uniqueness).
        let leaf = SchematicSection { labels: vec![NetLabel { id: String::new(), net: "LOCAL".into(), at: r1_pin1_world(Point { x: 0, y: 0 }), kind: LabelKind::Local }], ..empty_sch(vec![r1_instance(Point { x: 0, y: 0 })]) };
        let mut screens = Map::new();
        screens.insert("leaf.kicad_sch".to_string(), leaf);
        let design = design_with(root, screens);

        let (_flat, nets) = flatten(&design, &model).expect("root exists");
        let local_named: Vec<&Net> = nets.iter().filter(|n| n.pins.contains(&"R1.1".to_string())).collect();
        // Two R1 instances (one per sheet placement) -- the module doesn't
        // disambiguate their *reference* in this test (see the comment
        // above), so both contribute an "R1.1" pin ref; what matters here
        // is that they land on *two separate* nets, not one merged "LOCAL".
        assert_eq!(local_named.len(), 2, "each sheet instance's own R1.1 is on its own separate net: {nets:#?}");
        assert_ne!(local_named[0].name, local_named[1].name);
    }

    /// A global label, by contrast, *does* merge across sheet instances --
    /// that is its entire purpose, and the one case this module must not
    /// accidentally path-scope away.
    #[test]
    fn global_labels_merge_across_sheet_instances() {
        let model = ConstraintModel { parts: vec![r1_part()], ..Default::default() };
        let root = SchematicSection { sheets: vec![SheetInstance { id: "sheetA".into(), name: "a".into(), file: "leaf.kicad_sch".into(), at: Point { x: 0, y: 0 }, size: (1000, 1000), pins: vec![] }], ..empty_sch(vec![r1_instance(Point { x: 100_000, y: 0 })]) };
        let root = SchematicSection { labels: vec![NetLabel { id: String::new(), net: "VBUS".into(), at: r1_pin1_world(Point { x: 100_000, y: 0 }), kind: LabelKind::Global { shape: LabelShape::Passive } }], ..root };

        let leaf = SchematicSection { labels: vec![NetLabel { id: String::new(), net: "VBUS".into(), at: r1_pin1_world(Point { x: 0, y: 0 }), kind: LabelKind::Global { shape: LabelShape::Passive } }], ..empty_sch(vec![SymbolInstance { id: "R2".into(), ..r1_instance(Point { x: 0, y: 0 }) }]) };
        let mut screens = Map::new();
        screens.insert("leaf.kicad_sch".to_string(), leaf);
        let model = ConstraintModel { parts: vec![r1_part(), Part { reference: "R2".into(), ..r1_part() }], ..model };
        let design = design_with(root, screens);

        let (_flat, nets) = flatten(&design, &model).expect("root exists");
        let vbus = nets.iter().find(|n| n.name == "VBUS").unwrap_or_else(|| panic!("no single VBUS net: {nets:#?}"));
        assert!(vbus.pins.contains(&"R1.1".to_string()) && vbus.pins.contains(&"R2.1".to_string()), "a global label must merge both sheets onto one net: {vbus:?}");
    }

    /// A *bus*-shaped sheet pin (GAPS.md #20) joins member-by-member: a
    /// parent-side local label naming one specific member ("DATA0") reaches
    /// the child's own same-member local label, even though the two sides'
    /// own *bus* names differ ("DATA[0..1]" vs "DATA[0..3]") and share only
    /// that one member -- real KiCad's own "any shared member is fine"
    /// rule (`erc::check_hierarchy`'s `bus_to_bus_conflict` is what reports
    /// when it is *not* met; this is purely the connectivity side). No
    /// `BusEntry`/bus wire is needed for this test -- `crate::bus`'s own
    /// tests already cover that geometric/ERC side; this one is about the
    /// union-find join alone, same minimal style as this module's other
    /// tests above (a bare label standing in for "something names this
    /// point").
    #[test]
    fn bus_sheet_pin_joins_matching_members_even_when_the_two_bus_names_differ() {
        let model = ConstraintModel { parts: vec![r1_part(), Part { reference: "R2".into(), ..r1_part() }], ..Default::default() };

        let pin_at = Point { x: 5_000, y: 10_000 };
        let root = SchematicSection {
            sheets: vec![SheetInstance { id: "sheetA".into(), name: "child".into(), file: "child.kicad_sch".into(), at: Point { x: 0, y: 0 }, size: (10_000, 10_000), pins: vec![SheetPin { id: "p1".into(), name: "DATA[0..1]".into(), shape: LabelShape::Passive, at: pin_at }] }],
            labels: vec![NetLabel { id: String::new(), net: "DATA0".into(), at: r1_pin1_world(Point { x: 0, y: 0 }), kind: LabelKind::Local }],
            ..empty_sch(vec![r1_instance(Point { x: 0, y: 0 })])
        };

        let r2_at = Point { x: 0, y: 0 };
        let child = SchematicSection {
            labels: vec![NetLabel { id: String::new(), net: "DATA0".into(), at: r1_pin1_world(r2_at), kind: LabelKind::Local }],
            ..empty_sch(vec![SymbolInstance { id: "R2".into(), ..r1_instance(r2_at) }])
        };

        let mut screens = Map::new();
        screens.insert("child.kicad_sch".to_string(), child);
        let design = design_with(root, screens);

        let (_flat, nets) = flatten(&design, &model).expect("root exists");
        let joined = nets.iter().find(|n| n.pins.contains(&"R2.1".to_string())).unwrap_or_else(|| panic!("R2.1 on no net at all: {nets:#?}"));
        assert!(joined.pins.contains(&"R1.1".to_string()), "R1 (parent, member DATA0) and R2 (child, member DATA0) must share one net: {joined:?}");
    }
}
