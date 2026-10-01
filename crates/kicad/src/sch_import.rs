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

use std::collections::{BTreeMap, HashMap};

use eda_model::ir::{Design, LabelKind, LabelShape, NoConnect, Point, PowerSymbol, Provenance, SchematicSection, SheetInstance, SymbolInstance, TitleBlock, Wire};
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

    for item in sexpr::find_all(root, "symbol") {
        let Some(lib_id) = sexpr::find(item, "lib_id").and_then(|l| sexpr::txt(l, 1)) else { continue };
        let Some(resolved) = lib_table.get(lib_id) else {
            notes.unresolved_symbols += 1;
            continue;
        };
        let Some(at) = sexpr::find(item, "at") else { continue };
        let (Some(x_mm), Some(y_mm)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle_deg = sexpr::num(at, 3).unwrap_or(0.0);
        let mirrored = sexpr::find(item, "mirror").is_some();
        let unit = sexpr::find(item, "unit").and_then(|u| sexpr::num(u, 1)).unwrap_or(1.0) as u32;
        let at_um = Point { x: crate::import::mm_to_um(x_mm), y: crate::import::mm_to_um(y_mm) };
        let rot = import_rot_millideg_sch(angle_deg);

        let reference = property_text(item, "Reference").unwrap_or_default();
        let value = property_text(item, "Value").unwrap_or_default();
        let footprint = property_text(item, "Footprint").unwrap_or_default();
        let datasheet = property_text(item, "Datasheet").unwrap_or_default();

        if resolved.power {
            let pin_local = resolved.pins.first().map(|p| p.at).unwrap_or(SPoint::new(0.0, 0.0));
            let world = transform_local_point(pin_local, angle_deg, mirrored);
            let at_pin = Point { x: at_um.x + crate::import::mm_to_um(world.x), y: at_um.y + crate::import::mm_to_um(world.y) };
            power_symbols.push(PowerSymbol { id: reference, lib_id: lib_id.to_string(), at: at_pin, rot, net: value, pin: String::new() });
            continue;
        }

        let mut pins: Vec<Pin> = Vec::with_capacity(resolved.pins.len());
        for p in &resolved.pins {
            let world = transform_local_point(p.at, angle_deg, mirrored);
            let at_pin = Point { x: at_um.x + crate::import::mm_to_um(world.x), y: at_um.y + crate::import::mm_to_um(world.y) };
            pin_world.insert(format!("{reference}.{}", p.number), at_pin);
            let kind = if nc_points.contains(&at_pin) { eda_model::PinKind::Nc } else { pin_kind_from_electrical_type(&p.electrical_type, &p.name) };
            pins.push(Pin { number: p.number.clone(), name: (!p.name.is_empty()).then(|| p.name.clone()), kind });
        }
        parts.push(Part {
            reference: reference.clone(),
            mpn: None,
            lcsc: None,
            value: (!value.is_empty()).then(|| value.clone()),
            package: None,
            footprint: (!footprint.is_empty()).then(|| footprint.clone()),
            symbol: Some(lib_id.to_string()),
            datasheet: (!datasheet.is_empty()).then_some(datasheet.clone()),
            pins,
            body_um: None,
            edge: None,
        });
        symbols.push(SymbolInstance { id: reference, at: at_um, rot, mirrored, lib_id: lib_id.to_string(), unit, value, footprint, datasheet });
    }

    let mut wires: Vec<Wire> = Vec::new();
    for w in sexpr::find_all(root, "wire") {
        if let Some(pts) = import_pts(w) {
            wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts });
        }
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

    let mut sheets: Vec<SheetInstance> = Vec::new();
    for s in sexpr::find_all(root, "sheet") {
        let name = sheet_property_text(s, "Sheetname").unwrap_or_default();
        let file = sheet_property_text(s, "Sheetfile").unwrap_or_default();
        let at = sexpr::find(s, "at").and_then(point_mm).unwrap_or(SPoint::new(0.0, 0.0));
        let size = sexpr::find(s, "size").and_then(|sz| Some((sexpr::num(sz, 1)?, sexpr::num(sz, 2)?))).unwrap_or((0.0, 0.0));
        sheets.push(SheetInstance { name, file, at: mm_point_to_um(at), size: (crate::import::mm_to_um(size.0), crate::import::mm_to_um(size.1)) });
        notes.sheets_not_descended += 1;
    }

    let nets = reconcile(&pin_world, &mut wires, &labels, &mut power_symbols, &mut no_connects);

    let sch = SchematicSection { symbols, wires, labels, power_symbols, no_connects, title_block, sheets };
    let mut design = Design {
        schema: 1,
        provenance: Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: blake3::hash(text.as_bytes()).to_hex().to_string(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
    };
    design.assign_missing_ids();

    let model = ConstraintModel { parts, nets, symbols: lib_table.into_values().collect(), ..Default::default() };
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
/// `baked_local`'s own doc comment), mirror, then rotate.
pub fn transform_local_point(local: SPoint, angle_deg: f64, mirrored: bool) -> SPoint {
    let ly = -local.y;
    let lx = if mirrored { -local.x } else { local.x };
    let theta = angle_deg.to_radians();
    let rx = lx * theta.cos() - ly * theta.sin();
    let ry = lx * theta.sin() + ly * theta.cos();
    SPoint::new(rx, ry)
}

// ---------------------------------------------------------------- net reconciliation

/// Tiny union-find over point identities (see the module doc). Returns the
/// rebuilt net list and, in the same pass, backfills every `Wire::net`/
/// `Wire::pins`, `PowerSymbol::pin` and `NoConnect::pin` this reader could
/// not know until connectivity was resolved.
pub fn reconcile(pin_world: &BTreeMap<String, Point>, wires: &mut [Wire], labels: &[eda_model::ir::NetLabel], power_symbols: &mut [PowerSymbol], no_connects: &mut [NoConnect]) -> Vec<Net> {
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
}
