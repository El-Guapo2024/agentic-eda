//! eda-engine — E1 v1: bridges `ConstraintModel` -> `eda-layout` -> `design.json`.
//!
//! `derive_schematic` maps parts to layout nodes, pins to ports (by
//! `PinKind`), and nets to layout edges (2-pin nets direct, >2-pin nets as a
//! star, dense power/ground nets as labels instead of wires), then hands the
//! graph to `eda_layout::layout` and assembles the result into a
//! `Design::schematic` section.

use std::collections::{BTreeSet, HashMap};

use eda_layout::{graph, layout, EdgeEndpoint, LayoutGraph, LayoutOptions, Node};
use eda_model::ir::{Design, NetLabel, Point, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part, Pin, PinKind};

pub mod geometry;

/// Grid used by the layout engine; ports and node sizes are chosen as
/// multiples of this so everything lands on-grid. (Used by tests below;
/// production code now goes through `geometry::GRID`.)
#[cfg(test)]
const GRID: i64 = eda_layout::DEFAULT_GRID; // 1270 um

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub seed: u64,
    pub engine_version: String,
    /// Caller-provided hash of the canonical intent slice consumed.
    pub intent_hash: String,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self { seed: 0, engine_version: env!("CARGO_PKG_VERSION").to_string(), intent_hash: String::new() }
    }
}

impl EngineOptions {
    pub fn new(seed: u64, intent_hash: impl Into<String>) -> Self {
        Self { seed, intent_hash: intent_hash.into(), ..Default::default() }
    }
}

pub fn derive_schematic(model: &ConstraintModel, opts: &EngineOptions) -> Result<Design, Vec<CheckResult>> {
    let mut errors = Vec::new();

    // Validate reference uniqueness.
    let mut seen_refs = BTreeSet::new();
    for p in &model.parts {
        if !seen_refs.insert(p.reference.clone()) {
            errors.push(CheckResult::fail("engine.duplicate_reference", p.reference.clone(), "duplicate part reference"));
        }
    }

    let parts_by_ref: HashMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();

    // Validate net pin refs resolve.
    for net in &model.nets {
        for pin_ref in &net.pins {
            match resolve_pin_ref(pin_ref, &parts_by_ref) {
                Some(_) => {}
                None => errors.push(CheckResult::fail(
                    "engine.unresolved_pin",
                    format!("{}:{pin_ref}", net.name),
                    "pin reference does not resolve to a known part/pin",
                )),
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // ---- node ordering (clusters kept adjacent, seed perturbs group order) ----
    let ref_order = ordered_references(model, opts.seed);

    // ---- build the layout graph ----
    let mut g = LayoutGraph::new();
    let mut node_id_of: HashMap<String, graph::NodeId> = HashMap::new();
    let mut pin_port_of: HashMap<String, Vec<Option<usize>>> = HashMap::new();

    for (i, reference) in ref_order.iter().enumerate() {
        let part = parts_by_ref[reference.as_str()];
        let (width, height) = geometry::node_size(part.pins.len());
        let (ports, pin_ports) = geometry::build_ports(part, width, height);
        let node = Node { id: i, width, height, ports };
        g.add_node(node);
        node_id_of.insert(reference.clone(), i);
        pin_port_of.insert(reference.clone(), pin_ports);
    }

    // ---- nets -> edges (2-pin direct, >2-pin star) or labels (dense power/gnd) ----
    let mut edge_meta: Vec<(String, Vec<String>)> = Vec::new(); // parallel to g.edges: (net, [ref.pin, ref.pin])
    let mut label_nets: Vec<(String, Vec<String>)> = Vec::new(); // (net, pin refs) to emit as labels

    let mut nets = model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));

    for (net_group, net) in nets.iter().enumerate() {
        if net.pins.len() < 2 {
            continue;
        }
        let kinds: Vec<PinKind> = net
            .pins
            .iter()
            .map(|r| resolve_pin_ref(r, &parts_by_ref).map(|(_, _, pin)| pin.kind).unwrap_or_default())
            .collect();
        let all_power_or_ground = kinds.iter().all(|k| matches!(k, PinKind::Power | PinKind::Ground));

        if net.pins.len() >= 3 && all_power_or_ground {
            label_nets.push((net.name.clone(), net.pins.clone()));
            continue;
        }

        if net.pins.len() == 2 {
            let (_, a_part, a_pin) = resolve_pin_ref(&net.pins[0], &parts_by_ref).unwrap();
            let (_, b_part, b_pin) = resolve_pin_ref(&net.pins[1], &parts_by_ref).unwrap();
            let a_score = drive_score(a_pin.kind, &a_pin.name);
            let b_score = drive_score(b_pin.kind, &b_pin.name);
            let a_drives = if a_score != b_score { a_score > b_score } else { net.pins[0] <= net.pins[1] };
            let (drv_pin_ref, drv_part, drv_pin, load_pin_ref, load_part, load_pin) = if a_drives {
                (&net.pins[0], a_part, a_pin, &net.pins[1], b_part, b_pin)
            } else {
                (&net.pins[1], b_part, b_pin, &net.pins[0], a_part, a_pin)
            };
            let edge =
                build_edge(&node_id_of, &pin_port_of, drv_part, &drv_pin.number, load_part, &load_pin.number);
            g.add_edge_in_group(edge.0, edge.1, net_group);
            edge_meta.push((net.name.clone(), vec![drv_pin_ref.clone(), load_pin_ref.clone()]));
        } else {
            // star: pick hub (prefer power/ground pin, else lexically-lowest "REF.PIN").
            let hub_idx = pick_hub(&net.pins, &kinds);
            let hub_pin_ref = net.pins[hub_idx].clone();
            let (hub_ref_str, hub_pin_num) = split_pin_ref(&hub_pin_ref);
            let hub_part = parts_by_ref[hub_ref_str];
            for (i, other_pin_ref) in net.pins.iter().enumerate() {
                if i == hub_idx {
                    continue;
                }
                let (other_ref_str, other_pin_num) = split_pin_ref(other_pin_ref);
                let other_part = parts_by_ref[other_ref_str];
                let edge = build_edge(&node_id_of, &pin_port_of, hub_part, hub_pin_num, other_part, other_pin_num);
                g.add_edge_in_group(edge.0, edge.1, net_group);
                edge_meta.push((net.name.clone(), vec![hub_pin_ref.clone(), other_pin_ref.clone()]));
            }
        }
    }

    let layout_opts = LayoutOptions::default();
    let result = layout(&g, &layout_opts);

    let mut symbols = Vec::with_capacity(ref_order.len());
    for (i, reference) in ref_order.iter().enumerate() {
        let at = result.positions.get(&i).copied().unwrap_or(graph::Point { x: 0, y: 0 });
        symbols.push(SymbolInstance { id: reference.clone(), at: Point { x: at.x, y: at.y }, rot: 0, mirrored: false });
    }

    let mut wires = Vec::with_capacity(edge_meta.len());
    for (i, (net, pins)) in edge_meta.into_iter().enumerate() {
        let poly = result.edge_polylines.get(i).cloned().unwrap_or_default();
        wires.push(Wire { net, pins, pts: poly.into_iter().map(|p| Point { x: p.x, y: p.y }).collect() });
    }

    let mut labels = Vec::new();
    for (net, pin_refs) in label_nets {
        for pin_ref in pin_refs {
            let (part_ref, pin_num) = split_pin_ref(&pin_ref);
            let node_id = node_id_of[part_ref];
            let port_idx = pin_port_of[part_ref][pin_index(parts_by_ref[part_ref], pin_num)];
            if let Some(port_idx) = port_idx {
                let top_left = result.positions.get(&node_id).copied().unwrap_or(graph::Point { x: 0, y: 0 });
                let at = g.nodes[node_id].port_point(top_left, port_idx);
                labels.push(NetLabel { net: net.clone(), at: Point { x: at.x, y: at.y } });
            }
        }
    }

    Ok(Design {
        schema: 1,
        provenance: Provenance {
            engine_version: opts.engine_version.clone(),
            intent_hash: opts.intent_hash.clone(),
            seed: opts.seed,
            stage_hashes: Vec::new(),
        },
        schematic: Some(SchematicSection { symbols, wires, labels }),
        placement: None,
        routing: None,
    })
}

// ---------------------------------------------------------------- ordering (clusters + seed)

fn ordered_references(model: &ConstraintModel, seed: u64) -> Vec<String> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let mut clusters = model.clusters.clone();
    clusters.sort_by(|a, b| a.anchor.cmp(&b.anchor));
    for c in &clusters {
        let mut g = Vec::new();
        if seen.insert(c.anchor.clone()) {
            g.push(c.anchor.clone());
        }
        for m in &c.members {
            if seen.insert(m.clone()) {
                g.push(m.clone());
            }
        }
        if !g.is_empty() {
            groups.push(g);
        }
    }

    let mut remaining: Vec<String> =
        model.parts.iter().map(|p| p.reference.clone()).filter(|r| !seen.contains(r)).collect();
    remaining.sort();
    for r in remaining {
        groups.push(vec![r]);
    }

    let mut idx: Vec<usize> = (0..groups.len()).collect();
    seeded_shuffle(&mut idx, seed);

    let mut out = Vec::new();
    for i in idx {
        out.extend(groups[i].clone());
    }
    out
}

/// Deterministic xorshift64-based Fisher-Yates: same seed -> same
/// permutation; different seeds -> (with overwhelming likelihood)
/// different orderings.
fn seeded_shuffle(idx: &mut [usize], seed: u64) {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    if state == 0 {
        state = 0xDEAD_BEEF_u64;
    }
    let mut next_u64 = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for i in (1..idx.len()).rev() {
        let j = (next_u64() % (i as u64 + 1)) as usize;
        idx.swap(i, j);
    }
}

// ---------------------------------------------------------------- pin ref helpers

fn split_pin_ref(pin_ref: &str) -> (&str, &str) {
    pin_ref.split_once('.').unwrap_or((pin_ref, ""))
}

fn resolve_pin_ref<'a>(
    pin_ref: &str,
    parts_by_ref: &HashMap<&'a str, &'a Part>,
) -> Option<(&'a str, &'a Part, &'a Pin)> {
    let (r, num) = split_pin_ref(pin_ref);
    let (&reference, &part) = parts_by_ref.get_key_value(r)?;
    let pin = part.pins.iter().find(|p| p.number == num)?;
    Some((reference, part, pin))
}

fn pin_index(part: &Part, pin_number: &str) -> usize {
    part.pins.iter().position(|p| p.number == pin_number).expect("pin ref already validated")
}

fn build_edge(
    node_id_of: &HashMap<String, graph::NodeId>,
    pin_port_of: &HashMap<String, Vec<Option<usize>>>,
    from_part: &Part,
    from_pin: &str,
    to_part: &Part,
    to_pin: &str,
) -> (EdgeEndpoint, EdgeEndpoint) {
    let from_idx = pin_index(from_part, from_pin);
    let to_idx = pin_index(to_part, to_pin);
    let from_port = pin_port_of[&from_part.reference][from_idx].expect("NC pin cannot be wired");
    let to_port = pin_port_of[&to_part.reference][to_idx].expect("NC pin cannot be wired");
    (
        EdgeEndpoint { node: node_id_of[&from_part.reference], port: from_port },
        EdgeEndpoint { node: node_id_of[&to_part.reference], port: to_port },
    )
}

fn drive_score(kind: PinKind, name: &Option<String>) -> i32 {
    if kind == PinKind::Power {
        return 3;
    }
    let upper = name.clone().unwrap_or_default().to_uppercase();
    if upper.contains("OUT") {
        return 2;
    }
    if upper.contains("IN") || upper.contains("EN") {
        return -2;
    }
    0
}

fn pick_hub(pins: &[String], kinds: &[PinKind]) -> usize {
    let power_ground: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| matches!(k, PinKind::Power | PinKind::Ground))
        .map(|(i, _)| i)
        .collect();
    let candidates: &[usize] = if power_ground.is_empty() { &[] } else { &power_ground };
    if !candidates.is_empty() {
        return *candidates.iter().min_by_key(|&&i| &pins[i]).unwrap();
    }
    (0..pins.len()).min_by_key(|&i| &pins[i]).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::Net;

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }

    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, value: None, package: None, footprint: None, pins }
    }

    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// U1 (LDO) + CIN + COUT: VIN, VOUT 2-pin nets, a 3-pin GND star, one NC pin.
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

    fn opts(seed: u64) -> EngineOptions {
        EngineOptions::new(seed, "test-intent-hash")
    }

    #[test]
    fn all_symbols_present() {
        let d = derive_schematic(&ldo_model(), &opts(1)).unwrap();
        let sch = d.schematic.unwrap();
        let mut ids: Vec<_> = sch.symbols.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["CIN", "COUT", "U1"]);
    }

    #[test]
    fn wires_carry_correct_pin_refs() {
        let d = derive_schematic(&ldo_model(), &opts(1)).unwrap();
        let sch = d.schematic.unwrap();
        // VIN is now a 3-pin star (VIN, CIN.1, and EN tied in) hubbed on
        // U1.1: two edges, both carrying the hub pin.
        let vin_wires: Vec<_> = sch.wires.iter().filter(|w| w.net == "VIN").collect();
        assert_eq!(vin_wires.len(), 2, "VIN should be a 2-edge star");
        let mut all_pins: Vec<String> = vin_wires.iter().flat_map(|w| w.pins.clone()).collect();
        all_pins.sort();
        assert_eq!(all_pins, vec!["CIN.1".to_string(), "U1.1".to_string(), "U1.1".to_string(), "U1.3".to_string()]);

        let vout_wire = sch.wires.iter().find(|w| w.net == "VOUT").expect("VOUT wire present");
        let mut pins = vout_wire.pins.clone();
        pins.sort();
        assert_eq!(pins, vec!["COUT.1".to_string(), "U1.4".to_string()]);
    }

    #[test]
    fn no_nc_ports_and_no_nc_wires() {
        let d = derive_schematic(&ldo_model(), &opts(1)).unwrap();
        let sch = d.schematic.unwrap();
        for w in &sch.wires {
            for p in &w.pins {
                assert!(!p.ends_with(".5"), "NC pin U1.5 must never appear on a wire: {p}");
            }
        }
        for l in &sch.labels {
            assert!(!l.net.is_empty());
        }
    }

    #[test]
    fn multi_pin_signal_net_is_a_star() {
        // 3-pin non-power/ground signal net -> star topology: 2 edges, same net.
        let u1 = part(
            "U1",
            vec![
                pin("1", "A_OUT", PinKind::Signal),
                pin("2", "B_IN", PinKind::Signal),
                pin("3", "C_IN", PinKind::Signal),
            ],
        );
        let model = ConstraintModel { parts: vec![u1], nets: vec![net("SIG", &["U1.1", "U1.2", "U1.3"])], ..Default::default() };
        let d = derive_schematic(&model, &opts(2)).unwrap();
        let sch = d.schematic.unwrap();
        let sig_wires: Vec<_> = sch.wires.iter().filter(|w| w.net == "SIG").collect();
        assert_eq!(sig_wires.len(), 2, "3-pin net should yield 2 star edges");
        // hub (driver, A_OUT) appears on both edges since it's lexically lowest.
        let hub_count = sig_wires.iter().filter(|w| w.pins.contains(&"U1.1".to_string())).count();
        assert_eq!(hub_count, 2);
    }

    #[test]
    fn six_pin_gnd_net_becomes_labels_not_wires() {
        let mut pins = Vec::new();
        let mut parts = Vec::new();
        let mut net_pins = Vec::new();
        for i in 0..6 {
            let r = format!("R{i}");
            parts.push(part(&r, vec![pin("1", "GND", PinKind::Ground)]));
            net_pins.push(format!("{r}.1"));
            pins.push(r);
        }
        let model = ConstraintModel { parts, nets: vec![net("GND", &net_pins.iter().map(|s| s.as_str()).collect::<Vec<_>>())], ..Default::default() };
        let d = derive_schematic(&model, &opts(3)).unwrap();
        let sch = d.schematic.unwrap();
        assert!(sch.wires.iter().all(|w| w.net != "GND"), "dense GND net must not be wired");
        let gnd_labels: Vec<_> = sch.labels.iter().filter(|l| l.net == "GND").collect();
        assert_eq!(gnd_labels.len(), 6, "one label per pin on the dense GND net");
    }

    #[test]
    fn determinism_same_seed_same_bytes() {
        let model = ldo_model();
        let d1 = derive_schematic(&model, &opts(42)).unwrap();
        let d2 = derive_schematic(&model, &opts(42)).unwrap();
        assert_eq!(d1.canonical_bytes().unwrap(), d2.canonical_bytes().unwrap());
    }

    #[test]
    fn seed_variation_changes_bytes() {
        // Larger model so seed-driven group reordering has room to differ.
        let mut parts = vec![part("U1", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground)])];
        for i in 0..8 {
            parts.push(part(&format!("R{i}"), vec![pin("1", "A", PinKind::Passive), pin("2", "B", PinKind::Passive)]));
        }
        let model = ConstraintModel { parts, nets: vec![], ..Default::default() };
        let mut seen = std::collections::HashSet::new();
        for seed in 0..8u64 {
            let d = derive_schematic(&model, &opts(seed)).unwrap();
            seen.insert(d.canonical_bytes().unwrap());
        }
        assert!(seen.len() > 1, "varying the seed should produce more than one distinct candidate");
    }

    #[test]
    fn roundtrip_serde() {
        let d = derive_schematic(&ldo_model(), &opts(1)).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schematic.unwrap().symbols.len(), 3);
    }

    #[test]
    fn everything_lands_on_grid() {
        let d = derive_schematic(&ldo_model(), &opts(7)).unwrap();
        let sch = d.schematic.unwrap();
        for s in &sch.symbols {
            assert_eq!(s.at.x % GRID, 0, "symbol {} x off-grid", s.id);
            assert_eq!(s.at.y % GRID, 0, "symbol {} y off-grid", s.id);
        }
        for w in &sch.wires {
            for p in &w.pts {
                assert_eq!(p.x % GRID, 0, "wire point x off-grid on net {}", w.net);
                assert_eq!(p.y % GRID, 0, "wire point y off-grid on net {}", w.net);
            }
        }
        for l in &sch.labels {
            assert_eq!(l.at.x % GRID, 0);
            assert_eq!(l.at.y % GRID, 0);
        }
    }

    #[test]
    fn unresolved_pin_ref_fails() {
        let model = ConstraintModel {
            parts: vec![part("U1", vec![pin("1", "VIN", PinKind::Power)])],
            nets: vec![net("X", &["U1.1", "U1.99"])],
            ..Default::default()
        };
        let errs = derive_schematic(&model, &opts(1)).unwrap_err();
        assert!(errs.iter().any(|e| e.check == "engine.unresolved_pin"));
    }
}
