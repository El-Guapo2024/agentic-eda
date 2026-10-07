//! eda-engine — E1 v1: bridges `ConstraintModel` -> `eda-layout` -> `design.json`.
//!
//! `derive_schematic` maps parts to layout nodes, pins to ports (by
//! `PinKind`), and nets to layout edges (2-pin nets direct, >2-pin nets as a
//! star, dense power/ground nets as labels instead of wires), then hands the
//! graph to `eda_layout::layout` and assembles the result into a
//! `Design::schematic` section.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use eda_layout::{graph, layout, EdgeEndpoint, LayoutGraph, LayoutOptions, Node};
use eda_model::ir::{Design, LabelKind, NetLabel, NoConnect, Point, PowerSymbol, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::{resolve_lib_id, CheckResult, ConstraintModel, Part, Pin, PinKind};

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

/// A wire whose routed polyline comes out longer than this is demoted to a
/// named net label instead (see the crate design note). Deliberately under
/// `eda-gates`' 80 mm `schematic_wire_length` limit, so any wire that
/// survives layout passes the gate with margin.
pub const WIRE_BUDGET_UM: i64 = 76_000;

/// A same-part run of adjacent same-net pins (see the net-label loop below)
/// is joined by one bus stub only up to this length/bend count. Mirrors
/// `eda-gates`' `MAX_POWER_WIRE_LEN_UM`/`MAX_POWER_WIRE_BENDS` with margin
/// (`eda-engine` cannot depend on `eda-gates` -- the same reason
/// `is_control_pin_name` is duplicated rather than shared -- so the two are
/// kept in lockstep by inspection/tests instead). "Adjacent" pins on a small
/// part are a few mm apart and a short stub reads as one local jumper; on a
/// wide multi-ground connector the same rule can pick out five pins spread
/// across the whole box, and stubbing those together draws exactly the
/// sheet-spanning rail the gate exists to catch. Past this size each pin in
/// the run gets its own independent flag instead, which is how a real
/// schematic draws a connector's several ground pins anyway.
const MAX_MERGED_STUB_LEN_UM: i64 = 24_000;
const MAX_MERGED_STUB_BENDS: usize = 2;

/// Gap left between packed cluster blocks (um), and the slack around a
/// block's drawn extent for its net-label glyphs/text. Two settings: blocks
/// *inside* one cluster (an IC and its decoupling caps) are packed tight,
/// because "next to its IC" is the whole point of a functional block, while
/// whole clusters are separated generously so the eye reads them as
/// distinct groups.
const CLUSTER_GAP_UM: i64 = 10 * eda_layout::DEFAULT_GRID;
const CLUSTER_MARGIN_UM: i64 = 4 * eda_layout::DEFAULT_GRID;
const TIGHT_GAP_UM: i64 = 3 * eda_layout::DEFAULT_GRID;
const TIGHT_MARGIN_UM: i64 = 2 * eda_layout::DEFAULT_GRID;
/// A part with at least this many pins is treated as an IC — a cluster
/// anchor that passives and connectors hang off.
const ANCHOR_MIN_PINS: usize = 5;

/// One cluster's finished sub-layout, in its own local coordinates.
struct Laid {
    positions: BTreeMap<String, graph::Point>,
    /// (net, [driver pin ref, load pin ref], polyline)
    edges: Vec<(String, Vec<String>, Vec<graph::Point>)>,
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

    // Per-part symbol geometry is a pure function of the part, so it is
    // built once here and shared by every cluster's sub-layout and by the
    // net-label emission at the end.
    let mut node_of: BTreeMap<String, Node> = BTreeMap::new();
    let mut pin_port_of: BTreeMap<String, Vec<Option<usize>>> = BTreeMap::new();
    for part in &model.parts {
        let resolved = model.real_symbol_of(&resolve_lib_id(part), part);
        // `derive_schematic` never splits a part across multiple placed
        // units (see `SymbolInstance::unit`'s own doc) -- every node it
        // builds is unit 1, regardless of how many units the resolved real
        // symbol actually declares.
        let (width, height) = geometry::node_size(part, resolved.as_ref(), 1);
        let (ports, pin_ports) = geometry::build_ports(part, width, height, resolved.as_ref(), 1);
        node_of.insert(part.reference.clone(), Node { id: 0, width, height, ports });
        pin_port_of.insert(part.reference.clone(), pin_ports);
    }

    // ---- nets -> wire candidates or rail labels ----
    let mut nets = model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    let net_pins_by_name: BTreeMap<String, Vec<String>> =
        nets.iter().map(|n| (n.name.clone(), n.pins.clone())).collect();

    // Third element: true for a power/ground-style net (draws a power
    // symbol at each pin), false for an ordinary signal net drawn as a
    // label only because it spans clusters or ran over the wire-length
    // budget (draws a local net-label flag instead).
    let mut label_nets: Vec<(String, Vec<String>, bool)> = Vec::new();
    let mut wire_nets: Vec<(usize, String, Vec<(String, String)>)> = Vec::new();
    // Every net name that got power/ground-symbol treatment below —
    // consulted by the `PWR_FLAG` loop further down, since a net can earn
    // that treatment purely by *name* (`GND`, say) with every one of its
    // pins genuinely `Passive`-kind (an all-resistor divider's ground rail,
    // e.g. `examples/passive_divider_ladder.yaml`): real KiCad still treats
    // each of those as an ordinary `power_in` sink once drawn as a `GND`
    // symbol, so the net still needs a driver on it, even though the
    // pin-kind-only check below would never see one to require it from.
    let mut power_style_nets: BTreeSet<String> = BTreeSet::new();

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
        // A power/ground-style net — either every pin on it is Power/Ground
        // kind, or the net's own name reads as a rail (GND/VCC/VDD/VIN/...,
        // see `geometry::is_power_or_ground_net_name`) — draws power-flag or
        // ground-glyph symbols at each pin instead of a wire across the
        // sheet, regardless of pin count.
        let is_power_or_ground_net = (all_power_or_ground && net.pins.len() >= 4)
            || geometry::is_power_or_ground_net_name(&net.name);

        if is_power_or_ground_net {
            power_style_nets.insert(net.name.clone());
            label_nets.push((net.name.clone(), net.pins.clone(), true));
            continue;
        }
        wire_nets.push((net_group, net.name.clone(), net_edge_pairs(net, &kinds, &parts_by_ref)));
    }

    // ---- functional blocks ----
    let clusters = compute_clusters(model, &parts_by_ref, &nets);
    let mut cluster_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (ci, c) in clusters.iter().enumerate() {
        for r in c {
            cluster_of.insert(r.as_str(), ci);
        }
    }

    // A net whose pins span two blocks is drawn as a label at every pin
    // rather than as a wire between blocks: this, not the length
    // measurement below, is what removes the page-crossing staircases.
    let mut per_cluster_nets: Vec<Vec<(usize, String, Vec<(String, String)>)>> = vec![Vec::new(); clusters.len()];
    for (group, name, pairs) in wire_nets {
        let spanned: BTreeSet<usize> =
            nets[group].pins.iter().filter_map(|p| cluster_of.get(split_pin_ref(p).0).copied()).collect();
        match spanned.len() {
            1 => per_cluster_nets[*spanned.iter().next().unwrap()].push((group, name, pairs)),
            _ => label_nets.push((name, nets[group].pins.clone(), false)),
        }
    }

    // ---- per-cluster two-pass layout ----
    let layout_opts = LayoutOptions {
        label_slot_above: geometry::SLOT_ABOVE_UM,
        label_slot_below: geometry::SLOT_BELOW_UM,
        ..LayoutOptions::default()
    };

    let mut blocks: Vec<Laid> = Vec::with_capacity(clusters.len());
    for (ci, members) in clusters.iter().enumerate() {
        let refs = cluster_order(members, opts.seed, ci);
        // Demotion is a fixed-point, not a single pass: dropping a long net
        // re-lays the block out, which can stretch a net that was inside
        // budget before. Iterate until nothing is over (bounded, since each
        // round strictly removes at least one net).
        let mut kept: Vec<(usize, String, Vec<(String, String)>)> = per_cluster_nets[ci].clone();
        let laid = loop {
            let laid = layout_cluster(&refs, &node_of, &pin_port_of, &parts_by_ref, &kept, &layout_opts);
            // Any net with an over-budget edge is demoted *whole* — never
            // half wired, half labelled — so a reader never has to guess
            // whether a labelled pin is also on the drawn wire.
            let mut over: BTreeSet<String> = BTreeSet::new();
            for (net, _, poly) in &laid.edges {
                if polyline_len(poly) > WIRE_BUDGET_UM {
                    over.insert(net.clone());
                }
            }
            if over.is_empty() {
                break laid;
            }
            for name in &over {
                if let Some(pins) = net_pins_by_name.get(name) {
                    label_nets.push((name.clone(), pins.clone(), false));
                }
            }
            kept.retain(|(_, n, _)| !over.contains(n));
        };
        blocks.push(laid);
    }

    // ---- pack the blocks ----
    let offsets = pack_blocks(&blocks, &node_of, CLUSTER_GAP_UM, CLUSTER_MARGIN_UM);

    let mut positions: BTreeMap<String, graph::Point> = BTreeMap::new();
    let mut wires: Vec<Wire> = Vec::new();
    for (block, off) in blocks.iter().zip(offsets.iter()) {
        for (r, p) in &block.positions {
            positions.insert(r.clone(), graph::Point { x: p.x + off.x, y: p.y + off.y });
        }
        for (net, pins, poly) in &block.edges {
            wires.push(Wire {
                id: String::new(),
                net: net.clone(),
                pins: pins.clone(),
                pts: poly.iter().map(|p| Point { x: p.x + off.x, y: p.y + off.y }).collect(),
                bus: false,
            });
        }
    }

    let mut symbols: Vec<SymbolInstance> = Vec::new();
    for part in &model.parts {
        let at = positions.get(&part.reference).copied().unwrap_or(graph::Point { x: 0, y: 0 });
        symbols.push(SymbolInstance {
            id: part.reference.clone(),
            at: Point { x: at.x, y: at.y },
            rot: 0,
            mirrored: false,
            mirror_y: false,
            lib_id: resolve_lib_id(part),
            unit: 1,
            value: part.value.clone().unwrap_or_default(),
            footprint: part.footprint.clone().unwrap_or_default(),
            datasheet: part.datasheet.clone().unwrap_or_default(),
            dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false,
        });
    }

    // ---- no-connect flags: one at every `nc`-kind pin ----
    // A part's own pin keeps whatever electrical type it would otherwise
    // get (see `eda_kicad`'s exporter): the flag, not a retyped pin, is
    // what tells KiCad's ERC the dangling pin is deliberate — exactly the
    // same convention a human draws for an MCU's unused GPIO.
    let mut no_connects: Vec<NoConnect> = Vec::new();
    for part in &model.parts {
        let Some(top_left) = positions.get(&part.reference).copied() else { continue };
        let node = &node_of[&part.reference];
        let resolved = model.real_symbol_of(&resolve_lib_id(part), part);
        for (pin_idx, local) in geometry::nc_pin_local_points(part, node.width, node.height, resolved.as_ref(), 1) {
            let at = graph::Point { x: top_left.x + local.x, y: top_left.y + local.y };
            no_connects.push(NoConnect { id: String::new(), at: Point { x: at.x, y: at.y }, pin: format!("{}.{}", part.reference, part.pins[pin_idx].number) });
        }
    }

    // ---- labels and power symbols ----
    label_nets.sort();
    label_nets.dedup();
    let mut labels = Vec::new();
    // Collected first, then turned into deterministically-numbered
    // `#PWR<nn>` instances below (KiCad's own auto-reference convention)
    // — numbering by iteration order would depend on `label_nets`'
    // (already-deterministic) order, but sorting explicitly here keeps the
    // numbering obviously stable under refactors of the loop above.
    let mut power_pins: Vec<(String, String)> = Vec::new(); // (net, "REF.PIN")
    for (net, pin_refs, is_power) in label_nets {
        // Group this net's pins by part, since two flags only merge when
        // they belong to the same symbol.
        let mut by_part: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
        for pin_ref in &pin_refs {
            let (part_ref, pin_num) = split_pin_ref(pin_ref);
            let Some(part) = parts_by_ref.get(part_ref) else { continue };
            if let Some(port_idx) = pin_port_of[part_ref][pin_index(part, pin_num)] {
                by_part.entry(part_ref).or_default().push((pin_ref.clone(), port_idx));
            }
        }
        if is_power {
            // Power/ground: every pin gets its own power symbol, coincident
            // with its own stub tip — real KiCad style (an MCU's VDD/VDD2
            // each get their own dropped-on symbol, never merged the way
            // adjacent signal-net flags are).
            for (_part_ref, pins) in by_part {
                for (pin_ref, _) in pins {
                    power_pins.push((net.clone(), pin_ref));
                }
            }
            continue;
        }
        for (part_ref, mut pins) in by_part {
            // Port indices are assigned side-by-side within one contiguous
            // block per side (`geometry::build_ports`), so consecutive
            // integers here are physically neighboring pins on the same
            // side of the box.
            pins.sort_by_key(|(_, idx)| *idx);
            let node = &node_of[part_ref];
            let top_left = positions.get(part_ref).copied().unwrap_or(graph::Point { x: 0, y: 0 });
            // Split into runs of strictly-consecutive port indices that also
            // share a side (a +1 step can straddle a side boundary, which
            // must never be treated as "adjacent").
            let mut runs: Vec<Vec<(String, usize)>> = Vec::new();
            for p in pins {
                let same_side = |a: usize, b: usize| node.ports[a].side == node.ports[b].side;
                match runs.last_mut() {
                    Some(run) if p.1 == run.last().unwrap().1 + 1 && same_side(run.last().unwrap().1, p.1) => {
                        run.push(p);
                    }
                    _ => runs.push(vec![p]),
                }
            }
            for run in runs {
                // The pin's own *electrical* connection point, in KiCad's
                // file, is its stub tip (`write_lib_symbol` draws every pin
                // `at` the tip, `length` back toward the box) — not the
                // port point on the box boundary. A label anchored at the
                // port point would sit on the pin's drawn line but not on
                // its connection point, which real KiCad ERC sees as two
                // separate dangling items (the label *and* the pin); anchor
                // at the stub tip instead so the label is genuinely on the
                // net.
                // Anchored at the stub tip (not `port_point`'s on-box
                // point): that's the pin's own *electrical* connection
                // point in KiCad's file (see the doc comment above), and
                // also exactly the polyline a merged run's connecting wire
                // below needs -- one vector serves both.
                let points: Vec<graph::Point> =
                    run.iter().map(|(_, port_idx)| node.stub_tip(top_left, *port_idx)).collect();
                let stub_len: i64 = points.windows(2).map(|w| (w[0].x - w[1].x).abs() + (w[0].y - w[1].y).abs()).sum();
                let stub_bends = points.len().saturating_sub(2);
                if points.len() == 1 || stub_len > MAX_MERGED_STUB_LEN_UM || stub_bends > MAX_MERGED_STUB_BENDS {
                    // A single pin, or a run too wide to read as one local
                    // jumper (see `MAX_MERGED_STUB_LEN_UM`): flag each pin
                    // independently, with no connecting wire at all.
                    for p in &points {
                        labels.push(NetLabel { id: String::new(), kind: LabelKind::Local, net: net.clone(), at: Point { x: p.x, y: p.y } });
                    }
                } else {
                    // Adjacent same-net pins on one part: a single flag at
                    // the run's midpoint plus a short bus stub joining the
                    // pins, so each still shows a physical connection
                    // instead of two texts crowding each other
                    // (`schematic_flag_adjacent`).
                    let mid_x = points.iter().map(|p| p.x).sum::<i64>() / points.len() as i64;
                    let mid_y = points.iter().map(|p| p.y).sum::<i64>() / points.len() as i64;
                    labels.push(NetLabel { id: String::new(), kind: LabelKind::Local, net: net.clone(), at: Point { x: mid_x, y: mid_y } });
                    wires.push(Wire {
                        id: String::new(),
                        net: net.clone(),
                        pins: run.iter().map(|(pin_ref, _)| pin_ref.clone()).collect(),
                        pts: points.into_iter().map(|p| Point { x: p.x, y: p.y }).collect(),
                        bus: false,
                    });
                }
            }
        }
    }

    power_pins.sort();
    power_pins.dedup();
    let mut power_symbols: Vec<PowerSymbol> = Vec::new();
    for (i, (net, pin_ref)) in power_pins.into_iter().enumerate() {
        let (part_ref, pin_num) = split_pin_ref(&pin_ref);
        let Some(part) = parts_by_ref.get(part_ref) else { continue };
        let Some(port_idx) = pin_port_of[part_ref][pin_index(part, pin_num)] else { continue };
        let node = &node_of[part_ref];
        let top_left = positions.get(part_ref).copied().unwrap_or(graph::Point { x: 0, y: 0 });
        let tip = node.stub_tip(top_left, port_idx);
        power_symbols.push(PowerSymbol {
            id: format!("#PWR{:02}", i + 1),
            lib_id: power_symbol_lib_id(&net),
            at: Point { x: tip.x, y: tip.y },
            rot: 0,
            net,
            pin: pin_ref,
        });
    }

    // ---- PWR_FLAG: one per net that has a power-input pin and no natural
    // power-output driver anywhere on it ----
    //
    // This is a whole-model concern, not just the power-symbol nets above:
    // a net stays an ordinary wire whenever it mixes power-kind pins with
    // passive/signal ones (VIN tied to a cap and a header pin, say), but a
    // `Power`-kind pin without an "OUT"-ish name on *any* net still maps to
    // KiCad's `power_in` electrical type (see `eda_kicad`'s
    // `electrical_type`), and a net with a `power_in` pin and no
    // `power_out` pin fails `power_pin_not_driven` in real KiCad ERC
    // exactly like it would if a human wired the same circuit — the
    // textbook case a `PWR_FLAG` exists for. A `Power`-kind pin whose name
    // *does* read as an output (a regulator's own VOUT) already satisfies
    // this on its own once mapped to `power_out`, the same name convention
    // `eda_kicad::electrical_type` uses, so it needs no flag.
    let mut nets_sorted = model.nets.clone();
    nets_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut flag_n = power_symbols.len();
    for net in &nets_sorted {
        // Seeded `true` for a net that earned power/ground-symbol
        // treatment by name alone (see `power_style_nets`'s own doc):
        // every pin on it was just drawn as a `power_in`-typed symbol
        // regardless of its own `PinKind`, so it needs a driver exactly
        // like one a real `Power`/`Ground`-kind pin put it there.
        let mut has_power_in = power_style_nets.contains(&net.name);
        let mut has_power_out = false;
        for pin_ref in &net.pins {
            let Some((_, part, pin)) = resolve_pin_ref(pin_ref, &parts_by_ref) else { continue };
            let _ = part;
            let is_out = pin.kind == PinKind::Power && pin.name.as_deref().unwrap_or("").to_ascii_uppercase().contains("OUT");
            match pin.kind {
                PinKind::Power if is_out => has_power_out = true,
                PinKind::Power | PinKind::Ground => has_power_in = true,
                _ => {}
            }
        }
        if !has_power_in || has_power_out {
            continue;
        }
        // Anchor on the net's own first (sorted) pin's stub tip — defined
        // for every real pin regardless of whether this net ended up drawn
        // as a wire, a label, or power symbols.
        let mut sorted_pins = net.pins.clone();
        sorted_pins.sort();
        let Some(anchor_ref) = sorted_pins.first() else { continue };
        let (anchor_ref_part, anchor_pin_num) = split_pin_ref(anchor_ref);
        let Some(part) = parts_by_ref.get(anchor_ref_part) else { continue };
        let Some(port_idx) = pin_port_of[anchor_ref_part][pin_index(part, anchor_pin_num)] else { continue };
        let node = &node_of[anchor_ref_part];
        let top_left = positions.get(anchor_ref_part).copied().unwrap_or(graph::Point { x: 0, y: 0 });
        let anchor_tip = node.stub_tip(top_left, port_idx);
        let anchor_at = Point { x: anchor_tip.x, y: anchor_tip.y };

        flag_n += 1;
        // Coincident with the anchor point, not offset by a drawn wire: a
        // `PWR_FLAG`'s own pin sits at that exact point (same convention as
        // every per-pin power symbol above — KiCad treats coincident points
        // as joined with no wire needed). An offset-plus-connector-wire was
        // tried first and reliably created an *accidental* T-junction
        // wherever that offset happened to land on some other already-
        // routed wire's path (`schematic_missing_junction`/
        // `schematic_wire_through_symbol`) — this sidesteps the problem by
        // never drawing new wire geometry for the flag at all. The
        // trade-off is purely cosmetic: the flag's glyph overlaps whatever
        // is already at that point, exactly like two power symbols
        // deliberately stacked in a hand-drawn KiCad sheet.
        power_symbols.push(PowerSymbol { id: format!("#FLG{flag_n:02}"), lib_id: "power:PWR_FLAG".to_string(), at: anchor_at, rot: 0, net: net.name.clone(), pin: String::new() });
    }

    Ok(Design {
        schema: 1,
        provenance: Provenance {
            engine_version: opts.engine_version.clone(),
            intent_hash: opts.intent_hash.clone(),
            seed: opts.seed,
            stage_hashes: Vec::new(),
        },
        schematic: Some(SchematicSection { symbols, wires, labels, texts: vec![], power_symbols, no_connects, bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false, title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![] }),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
    })
}

/// `"power:GND"` for a ground-style rail, `"power:<NAME>"` for any other
/// rail — see `eda_model::symbol::builtin`'s matching fallback, which
/// draws the generic upward-arrow glyph for any name it does not have a
/// hand-transcribed symbol for.
fn power_symbol_lib_id(net: &str) -> String {
    let upper = net.to_ascii_uppercase();
    if upper.starts_with("GND") || upper.starts_with("AGND") || upper.starts_with("DGND") {
        "power:GND".to_string()
    } else {
        format!("power:{net}")
    }
}

fn polyline_len(poly: &[graph::Point]) -> i64 {
    poly.windows(2).map(|w| (w[0].x - w[1].x).abs() + (w[0].y - w[1].y).abs()).sum()
}

/// Decomposes one non-rail net into ordered (driver pin ref, load pin ref)
/// pairs: 2-pin nets direct, connector-only nets as a left-to-right chain,
/// everything else as a star around a hub. Direction matters — layering
/// reads edge direction as signal flow.
fn net_edge_pairs(
    net: &eda_model::Net,
    kinds: &[PinKind],
    parts_by_ref: &HashMap<&str, &Part>,
) -> Vec<(String, String)> {
    if net.pins.len() == 2 {
        let (_, a_part, a_pin) = resolve_pin_ref(&net.pins[0], parts_by_ref).unwrap();
        let (_, b_part, b_pin) = resolve_pin_ref(&net.pins[1], parts_by_ref).unwrap();
        let a_score = drive_score(a_pin.kind, &a_pin.name, &a_part.reference);
        let b_score = drive_score(b_pin.kind, &b_pin.name, &b_part.reference);
        let a_drives = if a_score != b_score { a_score > b_score } else { net.pins[0] <= net.pins[1] };
        return if a_drives {
            vec![(net.pins[0].clone(), net.pins[1].clone())]
        } else {
            vec![(net.pins[1].clone(), net.pins[0].clone())]
        };
    }
    if is_connector_only_net(&net.pins, parts_by_ref) {
        // No hub to anchor on: chain the connectors left to right so each
        // drives the next, rather than fanning them all off whichever one
        // sorted first.
        let mut chain: Vec<&String> = net.pins.iter().collect();
        chain.sort();
        return chain.windows(2).map(|p| (p[0].clone(), p[1].clone())).collect();
    }
    let hub_idx = pick_hub(&net.pins, kinds, parts_by_ref);
    let hub_pin_ref = net.pins[hub_idx].clone();
    let hub_part = parts_by_ref[split_pin_ref(&hub_pin_ref).0];
    let mut out = Vec::new();
    for (i, other_pin_ref) in net.pins.iter().enumerate() {
        if i == hub_idx {
            continue;
        }
        let other_part = parts_by_ref[split_pin_ref(other_pin_ref).0];
        // A connector spoke hanging off an IC hub must drive *into* the hub,
        // or the star lands every connector to the right of the part it
        // feeds — the `schematic_flow_direction` defect, mirrored.
        let spoke_drives = is_flow_source_ref(&other_part.reference) && !is_flow_source_ref(&hub_part.reference);
        out.push(if spoke_drives {
            (other_pin_ref.clone(), hub_pin_ref.clone())
        } else {
            (hub_pin_ref.clone(), other_pin_ref.clone())
        });
    }
    out
}

/// Lays out one cluster as a set of connected components (over the nets
/// still drawn as wires), each run through the Sugiyama pipeline on its own
/// and then packed into the cluster's own block. Components are the finest
/// grain at which a schematic can be re-shaped without moving connected
/// parts apart, so packing them is what keeps a block — and therefore the
/// sheet — near square instead of a single tall ribbon.
fn layout_cluster(
    refs: &[String],
    node_of: &BTreeMap<String, Node>,
    pin_port_of: &BTreeMap<String, Vec<Option<usize>>>,
    parts_by_ref: &HashMap<&str, &Part>,
    cnets: &[(usize, String, Vec<(String, String)>)],
    opts: &LayoutOptions,
) -> Laid {
    let comps = connected_components(refs, cnets);
    if comps.len() <= 1 {
        return run_layout(refs, node_of, pin_port_of, parts_by_ref, cnets, opts);
    }
    let mut sub: Vec<Laid> = Vec::with_capacity(comps.len());
    for comp in &comps {
        let members: BTreeSet<&str> = comp.iter().map(|s| s.as_str()).collect();
        let nets: Vec<_> = cnets
            .iter()
            .filter(|(_, _, pairs)| pairs.iter().any(|(a, _)| members.contains(split_pin_ref(a).0)))
            .cloned()
            .collect();
        sub.push(run_layout(comp, node_of, pin_port_of, parts_by_ref, &nets, opts));
    }
    let offsets = pack_blocks(&sub, node_of, TIGHT_GAP_UM, TIGHT_MARGIN_UM);
    let mut positions = BTreeMap::new();
    let mut edges = Vec::new();
    for (block, off) in sub.iter().zip(offsets.iter()) {
        for (r, p) in &block.positions {
            positions.insert(r.clone(), graph::Point { x: p.x + off.x, y: p.y + off.y });
        }
        for (net, pins, poly) in &block.edges {
            edges.push((
                net.clone(),
                pins.clone(),
                poly.iter().map(|p| graph::Point { x: p.x + off.x, y: p.y + off.y }).collect(),
            ));
        }
    }
    Laid { positions, edges }
}

/// Connected components of `refs` under the wire nets in `cnets`, each
/// returned in `refs` order so the seeded per-cluster ordering carries
/// through.
fn connected_components(refs: &[String], cnets: &[(usize, String, Vec<(String, String)>)]) -> Vec<Vec<String>> {
    let index: BTreeMap<&str, usize> = refs.iter().enumerate().map(|(i, r)| (r.as_str(), i)).collect();
    let mut parent: Vec<usize> = (0..refs.len()).collect();
    fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (_, _, pairs) in cnets {
        for (a, b) in pairs {
            let (Some(&ia), Some(&ib)) = (index.get(split_pin_ref(a).0), index.get(split_pin_ref(b).0)) else {
                continue;
            };
            let (ra, rb) = (find(&mut parent, ia), find(&mut parent, ib));
            if ra != rb {
                parent[ra] = rb;
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for i in 0..refs.len() {
        let r = find(&mut parent, i);
        by_root.entry(r).or_default().push(refs[i].clone());
    }
    by_root.into_values().collect()
}

/// Lays out one cluster's members with the given nets as edges. Returns
/// local (block-relative) coordinates.
fn run_layout(
    refs: &[String],
    node_of: &BTreeMap<String, Node>,
    pin_port_of: &BTreeMap<String, Vec<Option<usize>>>,
    parts_by_ref: &HashMap<&str, &Part>,
    cnets: &[(usize, String, Vec<(String, String)>)],
    opts: &LayoutOptions,
) -> Laid {
    let mut g = LayoutGraph::new();
    let mut node_id_of: BTreeMap<&str, graph::NodeId> = BTreeMap::new();
    for (i, reference) in refs.iter().enumerate() {
        let mut n = node_of[reference].clone();
        n.id = i;
        g.add_node(n);
        node_id_of.insert(reference.as_str(), i);
    }

    let mut edge_meta: Vec<(String, Vec<String>)> = Vec::new();
    for (group, name, pairs) in cnets {
        for (from, to) in pairs {
            let (fr, fp) = split_pin_ref(from);
            let (tr, tp) = split_pin_ref(to);
            let (Some(&fid), Some(&tid)) = (node_id_of.get(fr), node_id_of.get(tr)) else { continue };
            let from_port = pin_port_of[fr][pin_index(parts_by_ref[fr], fp)].expect("NC pin cannot be wired");
            let to_port = pin_port_of[tr][pin_index(parts_by_ref[tr], tp)].expect("NC pin cannot be wired");
            g.add_edge_in_group(
                EdgeEndpoint { node: fid, port: from_port },
                EdgeEndpoint { node: tid, port: to_port },
                *group,
            );
            edge_meta.push((name.clone(), vec![from.clone(), to.clone()]));
        }
    }

    let result = layout(&g, opts);
    let positions = refs
        .iter()
        .enumerate()
        .map(|(i, r)| (r.clone(), result.positions.get(&i).copied().unwrap_or(graph::Point { x: 0, y: 0 })))
        .collect();
    let edges = edge_meta
        .into_iter()
        .enumerate()
        .map(|(i, (net, pins))| (net, pins, result.edge_polylines.get(i).cloned().unwrap_or_default()))
        .collect();
    Laid { positions, edges }
}

// ---------------------------------------------------------------- clusters

/// Public view of the same clustering `derive_schematic` lays out with, so
/// `eda-gates`' `schematic_cluster_split` judges the exact blocks the
/// engine built rather than a re-guess.
pub fn cluster_members(model: &ConstraintModel) -> Vec<Vec<String>> {
    let parts_by_ref: HashMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();
    let mut nets = model.nets.clone();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    compute_clusters(model, &parts_by_ref, &nets)
}

/// Functional blocks. Uses intent `clusters:` when the model carries them;
/// otherwise derives them: every part with at least `ANCHOR_MIN_PINS` pins
/// is an anchor (an IC), every other part joins the anchor it shares the
/// most non-rail net pins with, a second round attaches leftovers through an
/// already-assigned neighbour, and whatever remains forms one misc block.
/// Returned in signal-flow order — connector/passive blocks first, the
/// block with the largest IC last.
fn compute_clusters(
    model: &ConstraintModel,
    parts_by_ref: &HashMap<&str, &Part>,
    nets: &[eda_model::Net],
) -> Vec<Vec<String>> {
    let all_refs: Vec<String> = {
        let mut v: Vec<String> = model.parts.iter().map(|p| p.reference.clone()).collect();
        v.sort();
        v
    };
    if all_refs.is_empty() {
        return Vec::new();
    }

    let mut owner: BTreeMap<String, String> = BTreeMap::new();

    if !model.clusters.is_empty() {
        let mut cs = model.clusters.clone();
        cs.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        for c in &cs {
            if !parts_by_ref.contains_key(c.anchor.as_str()) {
                continue;
            }
            owner.entry(c.anchor.clone()).or_insert_with(|| c.anchor.clone());
            for m in &c.members {
                if parts_by_ref.contains_key(m.as_str()) {
                    owner.entry(m.clone()).or_insert_with(|| c.anchor.clone());
                }
            }
        }
    } else {
        let anchors: BTreeSet<String> = all_refs
            .iter()
            .filter(|r| parts_by_ref[r.as_str()].pins.len() >= ANCHOR_MIN_PINS)
            .cloned()
            .collect();
        if anchors.is_empty() {
            return vec![all_refs];
        }
        for a in &anchors {
            owner.insert(a.clone(), a.clone());
        }
        // Round 1: attach by shared non-rail nets, most shared pins wins.
        let mut affinity: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut neighbors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for net in nets {
            if geometry::is_power_or_ground_net_name(&net.name) {
                continue;
            }
            let members: BTreeSet<String> =
                net.pins.iter().map(|p| split_pin_ref(p).0.to_string()).filter(|r| parts_by_ref.contains_key(r.as_str())).collect();
            for a in &members {
                for b in &members {
                    if a != b {
                        neighbors.entry(a.clone()).or_default().insert(b.clone());
                    }
                }
            }
            for p in &net.pins {
                let r = split_pin_ref(p).0.to_string();
                if anchors.contains(&r) || !parts_by_ref.contains_key(r.as_str()) {
                    continue;
                }
                for a in members.iter().filter(|m| anchors.contains(*m)) {
                    *affinity.entry((r.clone(), a.clone())).or_insert(0) += 1;
                }
            }
        }
        for r in &all_refs {
            if owner.contains_key(r) {
                continue;
            }
            let best = anchors
                .iter()
                .filter_map(|a| affinity.get(&(r.clone(), a.clone())).map(|c| (*c, a.clone())))
                .max_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
            if let Some((_, a)) = best {
                owner.insert(r.clone(), a);
            }
        }
        // Round 2: attach through an already-assigned neighbour.
        for r in &all_refs {
            if owner.contains_key(r) {
                continue;
            }
            let a = neighbors
                .get(r)
                .into_iter()
                .flatten()
                .filter_map(|n| owner.get(n).cloned())
                .min();
            if let Some(a) = a {
                owner.insert(r.clone(), a);
            }
        }
        // Round 3: a decoupling capacitor's only nets are rails, so it has
        // no affinity to *any* IC by net topology — yet it is exactly the
        // part that must be drawn beside its IC. Fall back to the intent's
        // own declaration order, where such a part is written next to the
        // IC it belongs to; nearest anchor wins, preferring the one
        // declared before it (the "U2, then U2's caps" convention).
        let decl_index: BTreeMap<&str, usize> =
            model.parts.iter().enumerate().map(|(i, p)| (p.reference.as_str(), i)).collect();
        for r in &all_refs {
            if owner.contains_key(r) {
                continue;
            }
            let Some(&ri) = decl_index.get(r.as_str()) else { continue };
            let best = anchors
                .iter()
                .filter_map(|a| decl_index.get(a.as_str()).map(|&ai| (ri.abs_diff(ai), ai > ri, a.clone())))
                .min();
            if let Some((_, _, a)) = best {
                owner.insert(r.clone(), a);
            }
        }
    }

    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut misc: Vec<String> = Vec::new();
    for r in &all_refs {
        match owner.get(r) {
            Some(a) => groups.entry(a.clone()).or_default().push(r.clone()),
            None => misc.push(r.clone()),
        }
    }
    let mut out: Vec<Vec<String>> = groups.into_values().collect();
    if !misc.is_empty() {
        out.push(misc);
    }
    // Signal-flow order: rank a block by its largest part, so connector and
    // passive blocks come first and the MCU's block comes last.
    out.sort_by(|a, b| {
        let rank = |g: &Vec<String>| g.iter().map(|r| parts_by_ref[r.as_str()].pins.len()).max().unwrap_or(0);
        rank(a).cmp(&rank(b)).then_with(|| a.first().cmp(&b.first()))
    });
    out
}

/// Member order within one cluster: sorted, then perturbed by the seed (the
/// cluster index is mixed in so different blocks do not all permute alike).
fn cluster_order(members: &[String], seed: u64, cluster_index: usize) -> Vec<String> {
    let mut refs = members.to_vec();
    refs.sort();
    let mut idx: Vec<usize> = (0..refs.len()).collect();
    seeded_shuffle(&mut idx, seed ^ ((cluster_index as u64) << 32));
    idx.into_iter().map(|i| refs[i].clone()).collect()
}

// ---------------------------------------------------------------- block packing

/// A block's drawn extent in its own coordinates, grown by
/// `CLUSTER_MARGIN_UM` for net-label glyphs.
fn block_extent(b: &Laid, node_of: &BTreeMap<String, Node>, margin: i64) -> (graph::Point, i64, i64) {
    let (mut x0, mut x1, mut y0, mut y1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    for (r, p) in &b.positions {
        let n = &node_of[r];
        x0 = x0.min(p.x);
        x1 = x1.max(p.x + n.width);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y + n.height);
    }
    for (_, _, poly) in &b.edges {
        for p in poly {
            x0 = x0.min(p.x);
            x1 = x1.max(p.x);
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
    }
    if x0 == i64::MAX {
        return (graph::Point { x: 0, y: 0 }, 0, 0);
    }
    (graph::Point { x: x0 - margin, y: y0 - margin }, x1 - x0 + 2 * margin, y1 - y0 + 2 * margin)
}

/// Flow bias for a block: negative means it should tend left (it contains
/// an external connector/switch/header — a signal source), positive means
/// it should tend right (it contains an IC, likely the MCU or another
/// downstream hub), zero is neutral. Used only as a tie-break between
/// otherwise-equal skyline placements — never a hard row constraint.
fn block_flow_bias(b: &Laid) -> i32 {
    let mut score = 0i32;
    for r in b.positions.keys() {
        if is_flow_source_ref(r) {
            score -= 1;
        } else if r.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).starts_with('U') {
            score += 1;
        }
    }
    score
}

/// Places `w x h` (already padded with `gap`) on the skyline `segs`
/// (sorted, contiguous, covering `[0, width)`), returning the chosen
/// top-left `(x, y)`. Candidate x positions are the existing segment
/// boundaries (classic bottom-left skyline heuristic): for each, the block
/// would rest on the tallest segment it overlaps, and the "waste" is the
/// area left under it between that height and the segments it covers.
/// Ties on `(y, waste)` are broken by `bias`: a left-tending block prefers
/// the smallest tied x, a right-tending block the largest.
fn skyline_best_x(segs: &[(i64, i64, i64)], w: i64, width: i64, bias: i32) -> (i64, i64, i64) {
    let mut best: Option<(i64, i64, i64)> = None; // (y, waste, x)
    for &(sx0, _, _) in segs {
        let x = sx0;
        if x + w > width {
            continue;
        }
        let mut y = 0i64;
        for &(a, b, h) in segs {
            if b <= x || a >= x + w {
                continue;
            }
            y = y.max(h);
        }
        let mut waste = 0i64;
        for &(a, b, h) in segs {
            let ox0 = a.max(x);
            let ox1 = b.min(x + w);
            if ox1 > ox0 {
                waste += (y - h) * (ox1 - ox0);
            }
        }
        best = Some(match best {
            None => (y, waste, x),
            Some((by, bw, bx)) => {
                if (y, waste) < (by, bw) {
                    (y, waste, x)
                } else if (y, waste) == (by, bw) {
                    let prefer_new = if bias < 0 { x < bx } else if bias > 0 { x > bx } else { false };
                    if prefer_new {
                        (y, waste, x)
                    } else {
                        (by, bw, bx)
                    }
                } else {
                    (by, bw, bx)
                }
            }
        });
    }
    best.unwrap_or((0, 0, 0))
}

/// Runs a skyline placement of `order` (block indices, largest-first) into
/// a sheet of the given `width`, returning the per-block offset and the
/// resulting total height.
fn skyline_place(
    order: &[usize],
    ext: &[(graph::Point, i64, i64)],
    foot: &[(i64, i64)],
    biases: &[i32],
    width: i64,
) -> (Vec<graph::Point>, i64) {
    let n = ext.len();
    let mut offsets = vec![graph::Point { x: 0, y: 0 }; n];
    let mut segs: Vec<(i64, i64, i64)> = vec![(0, width, 0)];
    for &idx in order {
        let (fw, fh) = foot[idx];
        let w = fw.min(width).max(1);
        let (y, _, x) = skyline_best_x(&segs, w, width, biases[idx]);

        let (origin, _, _) = ext[idx];
        offsets[idx] = graph::Point { x: x - origin.x, y: y - origin.y };

        let mut new_segs: Vec<(i64, i64, i64)> = Vec::new();
        for &(a, b, h) in &segs {
            if b <= x || a >= x + w {
                new_segs.push((a, b, h));
            } else {
                if a < x {
                    new_segs.push((a, x, h));
                }
                if b > x + w {
                    new_segs.push((x + w, b, h));
                }
            }
        }
        new_segs.push((x, x + w, y + fh));
        new_segs.sort_unstable_by_key(|s| s.0);
        segs = new_segs;
    }
    let total_h = segs.iter().map(|s| s.2).max().unwrap_or(0);
    (offsets, total_h)
}

/// Simple row-major shelf packing at a given target width, in the blocks'
/// original (flow) order. The skyline packer above is denser, but at a
/// target width close to the single widest block it can be forced into a
/// flatter sheet than the skyline's minimal-height packing allows (the
/// widest block alone pins the sheet width, and packing everything else
/// into that width densely can undershoot `schematic_sheet_aspect`'s
/// minimum ratio). Shelf packing carries some structural waste (a row's
/// height is its tallest block, unfilled row remainders), which is exactly
/// what lets it reach a taller aspect at the same width — so it is kept as
/// a fallback candidate alongside the skyline ones, and only wins when it
/// scores better (i.e. the skyline candidates are all out of the aspect
/// window).
fn shelf_place(ext: &[(graph::Point, i64, i64)], foot: &[(i64, i64)], target: i64) -> (Vec<graph::Point>, i64, i64) {
    let n = ext.len();
    let mut offsets = vec![graph::Point { x: 0, y: 0 }; n];
    let (mut cur_x, mut row_h, mut row_y) = (0i64, 0i64, 0i64);
    let (mut total_w, mut total_h) = (0i64, 0i64);
    for i in 0..n {
        let (w, h) = foot[i];
        if cur_x > 0 && cur_x + w > target {
            row_y += row_h;
            cur_x = 0;
            row_h = 0;
        }
        let (origin, _, _) = ext[i];
        offsets[i] = graph::Point { x: cur_x - origin.x, y: row_y - origin.y };
        cur_x += w;
        row_h = row_h.max(h);
        total_w = total_w.max(cur_x);
        total_h = total_h.max(row_y + row_h);
    }
    (offsets, total_w, total_h)
}

/// Packs the blocks with a skyline bin packer: blocks are sorted
/// largest-area-first (which is what keeps a skyline packer dense — small
/// blocks fill notches left by big ones instead of forcing a new row), each
/// is dropped at the position that minimises wasted area under it, and a
/// range of candidate sheet widths is tried, keeping the one whose
/// resulting aspect stays inside `schematic_sheet_aspect`'s 0.4..2.5 window
/// with the least wasted area. Signal flow (connectors left, ICs right) is
/// only a tie-break between placements that are otherwise equally good.
/// Returns the translation to apply to each block.
fn pack_blocks(blocks: &[Laid], node_of: &BTreeMap<String, Node>, gap: i64, margin: i64) -> Vec<graph::Point> {
    let n = blocks.len();
    if n == 0 {
        return Vec::new();
    }
    let ext: Vec<(graph::Point, i64, i64)> = blocks.iter().map(|b| block_extent(b, node_of, margin)).collect();
    if n == 1 {
        return vec![graph::Point { x: 0, y: 0 }];
    }

    // Footprint padded with the gap on the right/bottom, so the skyline
    // packer's segments already reserve inter-block spacing.
    let foot: Vec<(i64, i64)> = ext.iter().map(|(_, w, h)| (w + gap, h + gap)).collect();
    let biases: Vec<i32> = blocks.iter().map(block_flow_bias).collect();
    let widest = foot.iter().map(|(w, _)| *w).max().unwrap_or(1).max(1);

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let area_a = foot[a].0 * foot[a].1;
        let area_b = foot[b].0 * foot[b].1;
        area_b.cmp(&area_a).then(a.cmp(&b))
    });

    let total_area: i64 = foot.iter().map(|(w, h)| w * h).sum();
    let sqrt_w = (total_area as f64).sqrt().max(widest as f64) as i64;

    // Try a spread of candidate widths around the "square" width, plus the
    // shelf-style prefix-sum widths, so both a wide/flat and a tall/narrow
    // packing are available for the aspect search to choose between.
    let mut candidates: Vec<i64> = Vec::new();
    for factor in [0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.3, 1.5, 1.75, 2.0, 2.5, 3.0] {
        candidates.push(((sqrt_w as f64 * factor) as i64).max(widest));
    }
    let mut acc = 0i64;
    for &i in &order {
        acc += foot[i].0;
        candidates.push(acc.max(widest));
    }
    candidates.sort_unstable();
    candidates.dedup();

    // Both wasted area (what `schematic_sheet_density` measures) and
    // squareness (what keeps `schematic_sheet_aspect` and
    // `schematic_cluster_split` happy — a cluster spread wide-and-flat can
    // still minimise raw area while scattering its members far past its
    // own packed size) matter, so both terms are normalised to a
    // comparable O(1) scale and weighted evenly: `waste_ratio` is the
    // packed area over the blocks' own content area minus one (0 for a
    // perfect tiling), `squareness` is `|ln(height/width)|` (0 for a
    // square sheet). Out-of-range aspect is penalised hard so an in-range
    // candidate always wins when one exists.
    let content_area = total_area.max(1) as f64;
    let score_of = |width: i64, total_h: i64| -> f64 {
        let ratio = (total_h.max(1) as f64) / (width.max(1) as f64);
        let in_range = (0.4..=2.5).contains(&ratio);
        let squareness = ratio.ln().abs();
        let aspect_penalty = if in_range { 0.0 } else { 1.0e3 };
        let area = (width * total_h) as f64;
        let waste_ratio = (area / content_area - 1.0).max(0.0);
        aspect_penalty + squareness + waste_ratio
    };

    let mut best: Option<(f64, Vec<graph::Point>)> = None;
    for &width in &candidates {
        let (offsets, total_h) = skyline_place(&order, &ext, &foot, &biases, width);
        let score = score_of(width, total_h);
        if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
            best = Some((score, offsets));
        }
    }
    // Shelf fallback (see `shelf_place`): guards against a degenerate
    // aspect where the skyline's minimal-height packing at every candidate
    // width falls outside the gate's window.
    for width in candidates {
        let (offsets, total_w, total_h) = shelf_place(&ext, &foot, width);
        let score = score_of(total_w, total_h);
        if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
            best = Some((score, offsets));
        }
    }

    let mut offsets = best.unwrap().1;

    // Structurally identical blocks landing at the same x would stack their
    // symbols into one over-long column (`schematic_column_overflow`); a
    // small per-occurrence stagger, capped well inside the inter-block gap,
    // breaks that alignment without meaningfully changing the packing.
    let mut seen_x: BTreeMap<i64, i64> = BTreeMap::new();
    let grid = eda_layout::DEFAULT_GRID.max(1);
    let stagger_unit = ((gap / 4) / grid).max(1) * grid;
    for off in offsets.iter_mut() {
        let key = off.x / eda_layout::DEFAULT_GRID.max(1);
        let count = seen_x.entry(key).or_insert(0);
        if *count > 0 {
            off.x += (*count % 4) * stagger_unit;
        }
        *count += 1;
    }
    offsets
}


// ---------------------------------------------------------------- ordering (clusters + seed)

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

fn drive_score(kind: PinKind, name: &Option<String>, reference: &str) -> i32 {
    if kind == PinKind::Power {
        return 3;
    }
    // External connectors (J*/SW*/P*) are the sources of a signal chain in
    // these boards (a USB jack, a switch, a header) — bias them to drive so
    // the layout reads left-to-right with the connector on the left, rather
    // than sitting downstream of (to the right of) what they feed.
    let ref_upper = reference.to_uppercase();
    let is_connector = ref_upper.starts_with('J') || ref_upper.starts_with("SW") || ref_upper.starts_with('P');
    let upper = name.clone().unwrap_or_default().to_uppercase();
    if upper.contains("OUT") {
        return 2;
    }
    if is_connector {
        return 1;
    }
    if upper.contains("IN") || upper.contains("EN") {
        return -2;
    }
    0
}

/// True for a reference that reads as an external connector/header/switch
/// (`J*`/`SW*`/`P*`) — the same convention `eda-gates`' `is_flow_source_part`
/// uses, since the model carries no first-class "connector" kind.
fn is_flow_source_ref(reference: &str) -> bool {
    let stripped = reference.trim_start_matches(|c: char| !c.is_ascii_alphabetic());
    let prefix: String = stripped.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    matches!(prefix.to_ascii_uppercase().as_str(), "J" | "SW" | "P")
}

fn pick_hub(pins: &[String], kinds: &[PinKind], parts_by_ref: &HashMap<&str, &Part>) -> usize {
    let power_ground: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| matches!(k, PinKind::Power | PinKind::Ground))
        .map(|(i, _)| i)
        .collect();
    if !power_ground.is_empty() {
        return *power_ground.iter().min_by_key(|&&i| &pins[i]).unwrap();
    }
    // Prefer an external connector/switch/header pin as the hub so it lands
    // as the driving (leftmost) node in the star — otherwise a connector can
    // end up a spoke placed to the right of what it feeds, which is what
    // `schematic_flow_direction` flags.
    let connectors: Vec<usize> = (0..pins.len())
        .filter(|&i| {
            let (r, _) = split_pin_ref(&pins[i]);
            parts_by_ref.get(r).map(|p| is_flow_source_ref(&p.reference)).unwrap_or(false)
        })
        .collect();
    // With exactly one connector on the net, that connector is the source
    // and belongs at the hub. With *two or more*, making one of them the hub
    // turns its peers into spokes hanging off it, which places them to its
    // right and reads as flow running connector-to-connector — so anchor on
    // a non-connector member (the IC the connectors all feed) instead and
    // let every connector be a spoke. `is_connector_only_net` handles the
    // case where there is no non-connector to anchor on.
    // Anchoring the star on a non-connector when the net has two or more
    // connectors (so every connector becomes a spoke) was tried and is NOT
    // what ships: it does cut l4's drawn crossings hard (650 -> 493), but it
    // also pushes l4's routing past the 6s budget and costs `opamp_filter`
    // its clean run, and the flow-direction problem it was meant to fix is
    // already handled correctly by `check_flow_direction` no longer judging
    // one connector against another. Kept as a note so it is not re-tried
    // blind.
    if !connectors.is_empty() {
        return *connectors.iter().min_by_key(|&&i| &pins[i]).unwrap();
    }
    (0..pins.len()).min_by_key(|&i| &pins[i]).unwrap_or(0)
}

/// True when every pin on the net belongs to a connector-style part (see
/// `pick_hub`). Such a net has no natural hub: a star would place every
/// member to the right of whichever one was picked. Drawn as a chain
/// instead, left to right in pin-ref order.
fn is_connector_only_net(pins: &[String], parts_by_ref: &HashMap<&str, &Part>) -> bool {
    !pins.is_empty()
        && pins.iter().all(|p| {
            let (r, _) = split_pin_ref(p);
            parts_by_ref.get(r).map(|x| is_flow_source_ref(&x.reference)).unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::Net;

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }

    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: None, package: None, footprint: None, pins, body_um: None, symbol: None, datasheet: None, edge: None }
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
        // VIN and VOUT are not true rail names (only GND/VCC/VDD/... etc. are,
        // see `geometry::is_power_or_ground_net_name`), and neither net has
        // every pin Power/Ground-kind (VIN includes U1's Signal EN pin), so
        // both stay as ordinary wires between the two parts.
        assert!(sch.wires.iter().any(|w| w.net == "VIN"), "VIN should be a wire");
        assert!(sch.wires.iter().any(|w| w.net == "VOUT"), "VOUT should be a wire");
        // GND is a true rail name, so it draws a power symbol at each pin
        // (no wire, no label).
        assert!(sch.wires.iter().all(|w| w.net != "GND"), "GND is a power-style net: no wires");
        assert!(sch.labels.iter().all(|l| l.net != "GND"), "GND is a power symbol, not a label");
        let gnd_power: Vec<_> = sch.power_symbols.iter().filter(|p| p.net == "GND" && p.lib_id == "power:GND").collect();
        assert_eq!(gnd_power.len(), 3, "one GND power symbol per pin (U1.2, CIN.2, COUT.2)");
        // GND's three pins are all `Ground`-kind (a sink), with no natural
        // `power_out` driver anywhere on the net, so it also earns its own
        // `PWR_FLAG` — the same "no source, only sinks" case a hand-drawn
        // KiCad ground net needs one for.
        assert!(sch.power_symbols.iter().any(|p| p.net == "GND" && p.lib_id == "power:PWR_FLAG"), "undriven GND net should get a PWR_FLAG");
        // Every power symbol is unique KiCad-style (#PWR01, #PWR02, ...) and
        // coincides exactly with its own pin's stub tip (checked precisely
        // in `power_symbol_sits_on_pin_stub_tip`).
        let mut ids: Vec<_> = sch.power_symbols.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), sch.power_symbols.len(), "power symbol ids must be unique");
    }

    #[test]
    fn power_symbol_sits_on_pin_stub_tip() {
        let model = ldo_model();
        let d = derive_schematic(&model, &opts(1)).unwrap();
        let sch = d.schematic.unwrap();
        let u1 = sch.symbols.iter().find(|s| s.id == "U1").unwrap();
        let part = model.part("U1").unwrap();
        let (width, height) = geometry::node_size(part, None, 1);
        let (ports, pin_port) = geometry::build_ports(part, width, height, None, 1);
        // U1 pin "2" (GND) -> its port -> stub tip, must equal the power
        // symbol's own `at`.
        let port_idx = pin_port[1].unwrap();
        let node = Node { id: 0, width, height, ports };
        let tip = node.stub_tip(graph::Point { x: u1.at.x, y: u1.at.y }, port_idx);
        let ps = sch.power_symbols.iter().find(|p| p.pin == "U1.2").expect("U1.2 has a power symbol");
        assert_eq!(ps.at, Point { x: tip.x, y: tip.y });
    }

    #[test]
    fn nc_pin_gets_a_no_connect_flag_not_a_wire_or_port() {
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
        assert_eq!(sch.no_connects.len(), 1, "one no_connect flag for U1's NC pin");
        assert_eq!(sch.no_connects[0].pin, "U1.5");
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
    fn six_pin_gnd_net_becomes_power_symbols_not_wires() {
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
        assert!(sch.labels.iter().all(|l| l.net != "GND"));
        let gnd_power: Vec<_> = sch.power_symbols.iter().filter(|p| p.net == "GND" && p.lib_id == "power:GND").collect();
        assert_eq!(gnd_power.len(), 6, "one power symbol per pin on the dense GND net");
        assert!(sch.power_symbols.iter().any(|p| p.net == "GND" && p.lib_id == "power:PWR_FLAG"), "undriven GND net should get a PWR_FLAG");
    }

    #[test]
    fn a_wide_multi_ground_pin_connector_flags_each_pin_independently() {
        // J1's real case: several ground pins on *one* part (a USB-C
        // receptacle's redundant GND pads), spread across a wide symbol.
        // All-Ground and 4+ pins makes GND a power-style net (see
        // `is_power_or_ground_net`): every pin gets its own real `power:GND`
        // symbol, not a label or a wire -- so a widely spread same-part run
        // can never become one sheet-spanning, multi-bend bus-stub wire in
        // the first place. This is the `power_pins`/`PowerSymbol` path
        // (`is_power == true` short-circuits straight to one power symbol
        // per pin -- see the `continue` right after `power_pins.push`), a
        // stronger fix than `MAX_MERGED_STUB_LEN_UM`/`_BENDS` capping a
        // merged run after the fact; that cap still guards the *ordinary*
        // (non-power) same-part run this test does not exercise.
        let j1 = part(
            "J1",
            vec![
                pin("1", "GND", PinKind::Ground),
                pin("2", "VBUS", PinKind::Power),
                pin("3", "CC1", PinKind::Signal),
                pin("4", "VBUS", PinKind::Power),
                pin("5", "GND", PinKind::Ground),
                pin("6", "GND", PinKind::Ground),
                pin("7", "VBUS", PinKind::Power),
                pin("8", "GND", PinKind::Ground),
                pin("9", "GND", PinKind::Ground),
            ],
        );
        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let model = ConstraintModel {
            parts: vec![j1, r1],
            nets: vec![
                net("GND", &["J1.1", "J1.5", "J1.6", "J1.8", "J1.9"]),
                net("VBUS", &["J1.2", "J1.4", "J1.7"]),
                net("CC1", &["J1.3", "R1.1"]),
            ],
            ..Default::default()
        };
        let d = derive_schematic(&model, &opts(7)).unwrap();
        let sch = d.schematic.unwrap();
        assert!(sch.wires.iter().all(|w| w.net != "GND"), "a widely spread same-part GND run must not become one bendy wire: {:?}", sch.wires);
        assert!(sch.labels.iter().all(|l| l.net != "GND"), "GND is power-style: a real power:GND symbol per pin, not a label");
        let gnd_power: Vec<_> = sch.power_symbols.iter().filter(|p| p.net == "GND" && p.lib_id == "power:GND").collect();
        assert_eq!(gnd_power.len(), 5, "one power symbol per ground pin, not one bus stub for the whole run");
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
        for p in &sch.power_symbols {
            assert_eq!(p.at.x % GRID, 0, "power symbol {} x off-grid", p.id);
            assert_eq!(p.at.y % GRID, 0, "power symbol {} y off-grid", p.id);
        }
        for nc in &sch.no_connects {
            assert_eq!(nc.at.x % GRID, 0, "no_connect {} x off-grid", nc.pin);
            assert_eq!(nc.at.y % GRID, 0, "no_connect {} y off-grid", nc.pin);
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
