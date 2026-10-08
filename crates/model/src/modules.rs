//! Functional modules: the blocks a design is read in, and the blocks a hierarchical schematic is cut into.
//!
//! A schematic that holds every part on one sheet is hard to read, and a board of thirty parts already is. KiCad's answer is a
//! hierarchy: one sheet per block, a sheet symbol for each on the root, and the nets that cross blocks as sheet pins. This module
//! decides the blocks. It lives next to [`crate::floorplan`] because the placer wants the same cut (a module is also a patch of
//! board), and neither of them should invent its own.
//!
//! The cut is read, never guessed from names alone:
//! 1. **Declared modules first.** A recorded `placement.modules` list is taken as is. Failing that, the intent's `Proximity` rules
//!    are an author saying "these belong together", and [`crate::floorplan::partition`] already turns them into blocks.
//! 2. **Everything else is inferred, deterministically**, from the netlist alone:
//!    - every *anchor* (an IC, a connector) gets a module, with the passives that only serve it: decoupling capacitors on its
//!      rails, pull-ups, bias, a filter on one of its pins;
//!    - *repeated identical channels* hanging off one anchor (eight GPIO -> resistor -> LED -> ground chains) leave the anchor's
//!      module and form one module of their own, so the repetition is drawn once, in a tidy row;
//!    - connectors are modules of their own, never lumped into the IC they talk to;
//!    - whatever is left, or serves two ICs equally, goes into one `Misc` module.
//!
//! The same netlist always gives the same modules in the same order with the same names, so a schematic derived twice is the same
//! schematic.

use crate::ir::ModuleRegion;
use crate::{ConstraintModel, Part, PinKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where a module came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    /// Named by the design itself (a recorded placement module, or a block cut along the intent's proximity rules).
    Declared,
    /// An IC and the passives that only serve it.
    Ic,
    /// A set of identical chains hanging off one anchor (`LED channels (D1-D8)`).
    Channels,
    /// One connector and what serves only it.
    Connector,
    /// What no module owns.
    Misc,
}

/// One block of the design.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionalModule {
    /// What a reader sees: "MCU (U1)", "LED channels (D1\u{2013}D8)", "Header (J1)".
    pub name: String,
    pub kind: ModuleKind,
    /// The part the module is named after (the IC or the connector); `None` for channels and misc.
    pub lead: Option<String>,
    /// Every part in the module, in natural reference order (`C2` before `C10`).
    pub refs: Vec<String>,
    /// For [`ModuleKind::Channels`]: each channel's parts, outermost first (the part on the anchor's net, then the ones behind
    /// it), channels in natural order of their first part. Empty for every other kind.
    pub channels: Vec<Vec<String>>,
}

/// The reference's letters: `"C12"` -> `"C"`, `"SW3"` -> `"SW"`.
pub fn ref_prefix(reference: &str) -> String {
    reference.chars().take_while(|c| c.is_ascii_alphabetic() || *c == '#').collect::<String>().to_ascii_uppercase()
}

/// Sort key putting `R2` before `R10`: letters, then the number, then whatever follows.
pub fn natural_key(reference: &str) -> (String, u64, String) {
    let split = reference.find(|c: char| c.is_ascii_digit()).unwrap_or(reference.len());
    let (letters, rest) = reference.split_at(split);
    let digits_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, tail) = rest.split_at(digits_end);
    (letters.to_string(), digits.parse().unwrap_or(0), tail.to_string())
}

pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natural_key(a).cmp(&natural_key(b))
}

/// Whether a net name reads as a supply or ground rail. One definition for the schematic engine (rails draw as power symbols,
/// never as wires) and for module inference (rails never join parts into a block), so the two cannot disagree.
pub fn is_power_or_ground_net_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let trimmed = upper.trim_start_matches('+');
    const RAILS: &[&str] = &["GND", "AGND", "DGND", "VSS", "VCC", "VDD", "VDDA", "VBAT", "VBUS", "VSYS", "3V3", "5V", "1V8", "12V"];
    RAILS.iter().any(|r| *r == trimmed) || trimmed.starts_with("GND") || trimmed.starts_with("AGND") || trimmed.starts_with("DGND") || trimmed.starts_with("VSS")
}

/// A ground-style rail, as opposed to a supply: `GND`, `AGND`, `DGND`, `VSS`.
pub fn is_ground_net_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let trimmed = upper.trim_start_matches('+');
    trimmed.starts_with("GND") || trimmed.starts_with("AGND") || trimmed.starts_with("DGND") || trimmed.starts_with("VSS")
}

/// Is the net a rail: named like one, or at least four pins that are all power or ground (the schematic engine's own rule).
pub fn is_rail_net(model: &ConstraintModel, net: &crate::Net) -> bool {
    if is_power_or_ground_net_name(&net.name) {
        return true;
    }
    if net.pins.len() < 4 {
        return false;
    }
    net.pins.iter().all(|p| {
        let (r, n) = p.split_once('.').unwrap_or((p.as_str(), ""));
        model.part(r).and_then(|part| part.pins.iter().find(|pin| pin.number == n)).is_some_and(|pin| matches!(pin.kind, PinKind::Power | PinKind::Ground))
    })
}

/// A connector: a reference that says so, or a part marked as sitting on the board edge.
pub fn is_connector_part(part: &Part) -> bool {
    let p = ref_prefix(&part.reference);
    p == "J" || p == "P" || p == "CN" || part.edge == Some(true)
}

/// A part a block is built around: an IC or a connector (everything else is a satellite that serves one).
pub fn is_anchor_part(part: &Part) -> bool {
    is_connector_part(part) || is_ic_part(part)
}

/// An IC: a `U`, or any non-connector with enough pins to anchor a block.
fn is_ic_part(part: &Part) -> bool {
    if is_connector_part(part) {
        return false;
    }
    let p = ref_prefix(&part.reference);
    p == "U" || (part.pins.len() >= 5 && !matches!(p.as_str(), "R" | "C" | "L" | "D" | "F" | "FB" | "LED"))
}

/// "Header" for a plain pin header, the part's own value for anything with a name ("USB-C"), else "Connector".
fn connector_descriptor(part: &Part) -> String {
    let value = part.value.clone().unwrap_or_default();
    let v = value.trim();
    let up = v.to_ascii_uppercase();
    let more = format!("{} {}", part.package.clone().unwrap_or_default(), part.mpn.clone().unwrap_or_default()).to_ascii_uppercase();
    let generic = v.is_empty() || matches!(up.as_str(), "HDR" | "HEADER" | "PINHEADER" | "CONN" | "CONNECTOR");
    if !generic {
        return v.to_string();
    }
    if up.contains("HDR") || up.contains("HEADER") || more.contains("HDR") || more.contains("HEADER") {
        "Header".to_string()
    } else {
        "Connector".to_string()
    }
}

/// "MCU (U1)": what the part is, then which one. A value that only repeats the reference says nothing, so the name falls back
/// to the reference alone.
fn ic_title(part: &Part) -> String {
    let candidates = [part.value.clone(), part.mpn.clone()];
    for c in candidates.into_iter().flatten() {
        let c = c.trim().to_string();
        if !c.is_empty() && !c.eq_ignore_ascii_case(&part.reference) && c.chars().count() <= 24 && c.chars().any(|ch| ch.is_ascii_alphabetic()) {
            return format!("{c} ({})", part.reference);
        }
    }
    part.reference.clone()
}

fn anchor_title(part: &Part) -> String {
    if is_connector_part(part) {
        format!("{} ({})", connector_descriptor(part), part.reference)
    } else {
        ic_title(part)
    }
}

/// What a satellite is called in a channel's name: "LED", "Resistor", ... and the reference letters it is counted by.
fn channel_word(part: &Part) -> (String, String) {
    let prefix = ref_prefix(&part.reference);
    let text = format!("{} {}", part.value.clone().unwrap_or_default(), part.mpn.clone().unwrap_or_default()).to_ascii_uppercase();
    let word = match prefix.as_str() {
        "D" | "LED" if text.contains("LED") => "LED",
        "D" | "LED" => "Diode",
        "Q" => "Transistor",
        "R" => "Resistor",
        "C" => "Capacitor",
        "L" => "Inductor",
        "F" | "FB" => "Ferrite",
        "SW" | "S" => "Switch",
        "Y" | "X" => "Crystal",
        "K" => "Relay",
        _ => "Channel",
    };
    (word.to_string(), prefix)
}

/// Priority of a part class when naming a channel group: the part that gives the channel its purpose.
fn channel_rank(prefix: &str) -> usize {
    ["Q", "D", "LED", "K", "SW", "S", "Y", "X", "L", "FB", "F", "C", "R"].iter().position(|p| *p == prefix).unwrap_or(99)
}

/// Netlist lookup tables, built once.
struct Graph<'a> {
    model: &'a ConstraintModel,
    /// "U1.3" -> index into `model.nets`.
    net_of_pin: HashMap<String, usize>,
    rail: Vec<bool>,
    /// Parts on each net (indices into `model.parts`), once each.
    parts_on: Vec<Vec<usize>>,
}

impl<'a> Graph<'a> {
    fn new(model: &'a ConstraintModel) -> Self {
        let mut net_of_pin = HashMap::new();
        for (i, n) in model.nets.iter().enumerate() {
            for p in &n.pins {
                net_of_pin.entry(p.clone()).or_insert(i);
            }
        }
        let index: BTreeMap<&str, usize> = model.parts.iter().enumerate().map(|(i, p)| (p.reference.as_str(), i)).collect();
        let rail: Vec<bool> = model.nets.iter().map(|n| is_rail_net(model, n)).collect();
        let parts_on: Vec<Vec<usize>> = model
            .nets
            .iter()
            .map(|n| {
                let mut v: Vec<usize> = n.pins.iter().filter_map(|p| index.get(p.split('.').next().unwrap_or("")).copied()).collect();
                v.sort();
                v.dedup();
                v
            })
            .collect();
        Graph { model, net_of_pin, rail, parts_on }
    }

    fn part(&self, i: usize) -> &'a Part {
        &self.model.parts[i]
    }

    /// The net a pin sits on.
    fn net_of(&self, part: usize, pin: &str) -> Option<usize> {
        self.net_of_pin.get(&format!("{}.{}", self.part(part).reference, pin)).copied()
    }

    /// Every net a part touches, with the pin numbers on it.
    fn nets_of(&self, part: usize) -> Vec<(String, Option<usize>)> {
        let p = self.part(part);
        let mut pins: Vec<&str> = p.pins.iter().map(|p| p.number.as_str()).collect();
        pins.sort_by(|a, b| natural_cmp(a, b));
        pins.into_iter().map(|n| (n.to_string(), self.net_of(part, n))).collect()
    }

    /// How many pins of `part` sit on any of `nets`.
    fn pins_on(&self, part: usize, nets: &BTreeSet<usize>) -> usize {
        self.nets_of(part).into_iter().filter(|(_, n)| n.is_some_and(|n| nets.contains(&n))).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Ic,
    Connector,
}

/// A group of satellites joined by non-rail nets.
struct Component {
    parts: Vec<usize>,
    /// Every net any of its parts touches.
    nets: BTreeSet<usize>,
    /// Anchors reachable through a non-rail net.
    anchors: BTreeSet<usize>,
}

fn find(up: &mut Vec<usize>, mut i: usize) -> usize {
    while up[i] != i {
        up[i] = up[up[i]];
        i = up[i];
    }
    i
}

/// A part's class and fields, without its reference: two parts with the same text are interchangeable.
fn part_descriptor(p: &Part) -> String {
    format!(
        "{}[{}|{}|{}|{}|{}]",
        ref_prefix(&p.reference),
        p.value.clone().unwrap_or_default(),
        p.package.clone().unwrap_or_default(),
        p.footprint.clone().unwrap_or_default(),
        p.symbol.clone().unwrap_or_default(),
        p.pins.len()
    )
}

/// A component's shape as seen from `anchor`, independent of its reference numbers: two channels with the same string are the same
/// circuit, repeated. Which of a part's pins faces the anchor is part of the string, and so is which rail each far end lands on.
fn canonical(g: &Graph, comp: &Component, anchor: usize) -> String {
    // The parts that sit on a non-rail net with an anchor pin start the walk, in a reference-free order.
    let mut roots: Vec<usize> = comp
        .parts
        .iter()
        .copied()
        .filter(|&p| g.nets_of(p).iter().any(|(_, n)| n.is_some_and(|n| !g.rail[n] && g.parts_on[n].contains(&anchor))))
        .collect();
    roots.sort_by(|&a, &b| part_descriptor(g.part(a)).cmp(&part_descriptor(g.part(b))).then_with(|| natural_cmp(&g.part(a).reference, &g.part(b).reference)));
    let mut visited: BTreeSet<usize> = BTreeSet::new();
    let mut out = Vec::new();
    for r in roots {
        if !visited.contains(&r) {
            out.push(walk(g, comp, anchor, r, &mut visited));
        }
    }
    out.join("+")
}

fn walk(g: &Graph, comp: &Component, anchor: usize, part: usize, visited: &mut BTreeSet<usize>) -> String {
    visited.insert(part);
    let mut s = part_descriptor(g.part(part));
    for (pin, net) in g.nets_of(part) {
        s.push_str(&format!(":{pin}="));
        let Some(n) = net else {
            s.push_str("nc");
            continue;
        };
        if g.rail[n] {
            s.push_str(&format!("rail({})", g.model.nets[n].name));
            continue;
        }
        let mut next: Vec<usize> = g.parts_on[n].iter().copied().filter(|&q| q != part && comp.parts.contains(&q) && !visited.contains(&q)).collect();
        next.sort_by(|&a, &b| part_descriptor(g.part(a)).cmp(&part_descriptor(g.part(b))).then_with(|| natural_cmp(&g.part(a).reference, &g.part(b).reference)));
        let others_in_component = g.parts_on[n].iter().any(|&q| q != part && comp.parts.contains(&q));
        if g.parts_on[n].contains(&anchor) {
            s.push_str("anchor");
        } else if !others_in_component {
            s.push_str("out");
        } else if next.is_empty() {
            s.push_str("link");
        }
        for q in next {
            if visited.contains(&q) {
                continue;
            }
            s.push_str(&format!("({})", walk(g, comp, anchor, q, visited)));
        }
    }
    s
}

/// The component's parts in channel order: the part on the anchor's net first, then along the chain.
fn chain_order(g: &Graph, comp: &Component, anchor: usize) -> Vec<usize> {
    let mut roots: Vec<usize> = comp
        .parts
        .iter()
        .copied()
        .filter(|&p| g.nets_of(p).iter().any(|(_, n)| n.is_some_and(|n| !g.rail[n] && g.parts_on[n].contains(&anchor))))
        .collect();
    roots.sort_by(|&a, &b| natural_cmp(&g.part(a).reference, &g.part(b).reference));
    let mut order: Vec<usize> = Vec::new();
    let mut queue: Vec<usize> = roots;
    let mut head = 0;
    while head < queue.len() {
        let p = queue[head];
        head += 1;
        if order.contains(&p) {
            continue;
        }
        order.push(p);
        let mut next: Vec<usize> = Vec::new();
        for (_, n) in g.nets_of(p) {
            let Some(n) = n else { continue };
            if g.rail[n] {
                continue;
            }
            for &q in &g.parts_on[n] {
                if q != p && comp.parts.contains(&q) && !order.contains(&q) {
                    next.push(q);
                }
            }
        }
        next.sort_by(|&a, &b| natural_cmp(&g.part(a).reference, &g.part(b).reference));
        next.dedup();
        queue.extend(next);
    }
    // A part the walk never reached (it hangs off the far end only) still belongs to the channel.
    let mut rest: Vec<usize> = comp.parts.iter().copied().filter(|p| !order.contains(p)).collect();
    rest.sort_by(|&a, &b| natural_cmp(&g.part(a).reference, &g.part(b).reference));
    order.extend(rest);
    order
}

/// A lone resistor, capacitor or inductor with one end on a rail: a pull-up, a bias, a filter. It serves its anchor; it is not a
/// channel even when there are several of them.
fn is_bias_like(g: &Graph, comp: &Component) -> bool {
    if comp.parts.len() != 1 {
        return false;
    }
    let p = g.part(comp.parts[0]);
    if p.pins.len() != 2 || !matches!(ref_prefix(&p.reference).as_str(), "R" | "C" | "L" | "F" | "FB") {
        return false;
    }
    g.nets_of(comp.parts[0]).iter().any(|(_, n)| n.is_some_and(|n| g.rail[n]))
}

/// Cut `model` into modules.
///
/// `recorded` is a recorded `placement.modules` list; when it is not empty it is the cut (parts it does not name are inferred).
/// Otherwise the intent's proximity rules give declared modules ([`crate::floorplan::partition`]) and what they leave free is
/// inferred. A model with no parts gives no modules.
pub fn infer_modules(model: &ConstraintModel, recorded: &[ModuleRegion]) -> Vec<FunctionalModule> {
    let mut out: Vec<FunctionalModule> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();

    // ---- 1. declared modules ----
    if !recorded.is_empty() {
        for m in recorded {
            let mut refs: Vec<String> = m.refs.iter().filter(|r| model.part(r).is_some() && !taken.contains(*r)).cloned().collect();
            refs.sort_by(|a, b| natural_cmp(a, b));
            refs.dedup();
            if refs.is_empty() {
                continue;
            }
            taken.extend(refs.iter().cloned());
            out.push(FunctionalModule { name: m.name.clone(), kind: ModuleKind::Declared, lead: None, refs, channels: Vec::new() });
        }
    } else {
        let (blocks, _free) = crate::floorplan::partition(model);
        for b in blocks {
            let mut refs: Vec<String> = b.refs.iter().filter(|r| !taken.contains(*r)).cloned().collect();
            refs.sort_by(|a, b| natural_cmp(a, b));
            if refs.is_empty() {
                continue;
            }
            let lead = b.name.strip_prefix("mod_").map(str::to_string).filter(|l| model.part(l).is_some());
            let name = lead.as_ref().and_then(|l| model.part(l)).map(anchor_title).unwrap_or_else(|| b.name.clone());
            taken.extend(refs.iter().cloned());
            out.push(FunctionalModule { name, kind: ModuleKind::Declared, lead, refs, channels: Vec::new() });
        }
    }

    // ---- 2. inference over what is left ----
    let g = Graph::new(model);
    let free: Vec<usize> = (0..model.parts.len()).filter(|&i| !taken.contains(&model.parts[i].reference)).collect();
    if free.is_empty() {
        return disambiguate(out);
    }
    let anchor_of: BTreeMap<usize, Anchor> = free
        .iter()
        .filter_map(|&i| {
            let p = g.part(i);
            if is_connector_part(p) {
                Some((i, Anchor::Connector))
            } else if is_ic_part(p) {
                Some((i, Anchor::Ic))
            } else {
                None
            }
        })
        .collect();
    let satellites: Vec<usize> = free.iter().copied().filter(|i| !anchor_of.contains_key(i)).collect();

    // Satellites joined through non-rail nets form components.
    let slot: BTreeMap<usize, usize> = satellites.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let mut up: Vec<usize> = (0..satellites.len()).collect();
    for (n, parts) in g.parts_on.iter().enumerate() {
        if g.rail[n] {
            continue;
        }
        let on: Vec<usize> = parts.iter().filter_map(|p| slot.get(p).copied()).collect();
        for w in on.windows(2) {
            let (a, b) = (find(&mut up, w[0]), find(&mut up, w[1]));
            if a != b {
                up[a] = b;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, &i) in satellites.iter().enumerate() {
        let root = find(&mut up, k);
        groups.entry(root).or_default().push(i);
    }
    let mut components: Vec<Component> = groups
        .into_values()
        .map(|mut parts| {
            parts.sort_by(|&a, &b| natural_cmp(&g.part(a).reference, &g.part(b).reference));
            let mut nets = BTreeSet::new();
            let mut anchors = BTreeSet::new();
            for &p in &parts {
                for (_, n) in g.nets_of(p) {
                    let Some(n) = n else { continue };
                    nets.insert(n);
                    if !g.rail[n] {
                        anchors.extend(g.parts_on[n].iter().copied().filter(|q| anchor_of.contains_key(q)));
                    }
                }
            }
            Component { parts, nets, anchors }
        })
        .collect();
    components.sort_by(|a, b| natural_cmp(&g.part(a.parts[0]).reference, &g.part(b.parts[0]).reference));

    // Anchors in module order: ICs by size then reference, then connectors by reference.
    let mut anchors: Vec<usize> = anchor_of.keys().copied().collect();
    anchors.sort_by(|&a, &b| {
        let (ka, kb) = (anchor_of[&a], anchor_of[&b]);
        let rank = |k: Anchor| if k == Anchor::Ic { 0 } else { 1 };
        rank(ka)
            .cmp(&rank(kb))
            .then_with(|| if ka == Anchor::Ic { g.part(b).pins.len().cmp(&g.part(a).pins.len()) } else { std::cmp::Ordering::Equal })
            .then_with(|| natural_cmp(&g.part(a).reference, &g.part(b).reference))
    });

    // ---- channels: identical circuits repeated off one anchor ----
    let mut owner: Vec<Option<usize>> = vec![None; components.len()]; // anchor index
    let mut channel_groups: Vec<(usize, Vec<usize>)> = Vec::new(); // (anchor, component indices)
    let mut in_channel: Vec<bool> = vec![false; components.len()];
    for &x in &anchors {
        let mut by_shape: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (ci, c) in components.iter().enumerate() {
            if in_channel[ci] || !c.anchors.contains(&x) || is_bias_like(&g, c) {
                continue;
            }
            // A circuit that touches a net no anchor pin is on (a rail-only part) is a decoupling part, not a channel.
            let touches_anchor_net = c.parts.iter().any(|&p| g.nets_of(p).iter().any(|(_, n)| n.is_some_and(|n| !g.rail[n] && g.parts_on[n].contains(&x))));
            if !touches_anchor_net {
                continue;
            }
            by_shape.entry(canonical(&g, c, x)).or_default().push(ci);
        }
        for (_, cis) in by_shape {
            if cis.len() >= 2 {
                for &ci in &cis {
                    in_channel[ci] = true;
                }
                channel_groups.push((x, cis));
            }
        }
    }

    // ---- owners for the rest ----
    let owner_of = |c: &Component| -> Option<usize> {
        if !c.anchors.is_empty() {
            // The IC first, then the larger part, then the lower reference.
            return c
                .anchors
                .iter()
                .copied()
                .min_by(|&a, &b| {
                    let rank = |k: usize| if anchor_of[&k] == Anchor::Ic { 0 } else { 1 };
                    rank(a).cmp(&rank(b)).then_with(|| g.part(b).pins.len().cmp(&g.part(a).pins.len())).then_with(|| natural_cmp(&g.part(a).reference, &g.part(b).reference))
                });
        }
        // A component on rails alone: the one anchor that sits on every rail it touches (decoupling), ICs before connectors.
        let rails: BTreeSet<usize> = c.nets.iter().copied().filter(|&n| g.rail[n]).collect();
        if rails.is_empty() {
            return None;
        }
        for kind in [Anchor::Ic, Anchor::Connector] {
            let cands: Vec<usize> = anchors.iter().copied().filter(|a| anchor_of[a] == kind && rails.iter().all(|&n| g.parts_on[n].contains(a))).collect();
            match cands.len() {
                0 => continue,
                1 => return Some(cands[0]),
                _ => {
                    // Several sit on all of them: the one with clearly the most pins on those rails, else nobody.
                    let best = cands.iter().map(|&a| (g.pins_on(a, &rails), a)).max_by_key(|(n, _)| *n).map(|(n, _)| n).unwrap_or(0);
                    let top: Vec<usize> = cands.iter().copied().filter(|&a| g.pins_on(a, &rails) == best).collect();
                    return if top.len() == 1 { Some(top[0]) } else { None };
                }
            }
        }
        // No anchor sits on all of them (a filter between a supply and a regulator's output, say): the one non-ground rail's only
        // IC, if there is exactly one.
        let supplies: BTreeSet<usize> = rails.iter().copied().filter(|&n| !is_ground_net_name(&g.model.nets[n].name)).collect();
        if !supplies.is_empty() {
            let cands: Vec<usize> = anchors.iter().copied().filter(|a| anchor_of[a] == Anchor::Ic && supplies.iter().all(|&n| g.parts_on[n].contains(a))).collect();
            if cands.len() == 1 {
                return Some(cands[0]);
            }
        }
        None
    };
    for (ci, c) in components.iter().enumerate() {
        if !in_channel[ci] {
            owner[ci] = owner_of(c);
        }
    }

    // ---- assemble ----
    let mut inferred: Vec<FunctionalModule> = Vec::new();
    for &x in &anchors {
        let mut refs: Vec<String> = vec![g.part(x).reference.clone()];
        for (ci, c) in components.iter().enumerate() {
            if owner[ci] == Some(x) {
                refs.extend(c.parts.iter().map(|&p| g.part(p).reference.clone()));
            }
        }
        refs.sort_by(|a, b| natural_cmp(a, b));
        let kind = if anchor_of[&x] == Anchor::Ic { ModuleKind::Ic } else { ModuleKind::Connector };
        inferred.push(FunctionalModule { name: anchor_title(g.part(x)), kind, lead: Some(g.part(x).reference.clone()), refs, channels: Vec::new() });
    }
    for (x, cis) in &channel_groups {
        let mut chains: Vec<Vec<String>> = cis.iter().map(|&ci| chain_order(&g, &components[ci], *x).into_iter().map(|p| g.part(p).reference.clone()).collect()).collect();
        chains.sort_by(|a, b| natural_cmp(&a[0], &b[0]));
        let mut refs: Vec<String> = chains.iter().flatten().cloned().collect();
        refs.sort_by(|a, b| natural_cmp(a, b));
        // Named for the part that gives the channel its purpose, counted over every channel.
        let first = &chains[0];
        let best = first.iter().filter_map(|r| model.part(r)).min_by_key(|p| channel_rank(&ref_prefix(&p.reference))).unwrap_or_else(|| model.part(&first[0]).unwrap());
        let (word, prefix) = channel_word(best);
        let mut of_kind: Vec<&String> = refs.iter().filter(|r| ref_prefix(r) == prefix).collect();
        of_kind.sort_by(|a, b| natural_cmp(a, b));
        let range = match (of_kind.first(), of_kind.last()) {
            (Some(a), Some(b)) if a != b => format!("{a}\u{2013}{b}"),
            (Some(a), _) => (*a).clone(),
            _ => refs[0].clone(),
        };
        inferred.push(FunctionalModule { name: format!("{word} channels ({range})"), kind: ModuleKind::Channels, lead: None, refs, channels: chains });
    }
    // Order: ICs (largest first, as `anchors` is), then channel groups, then connectors, then misc.
    let mut ics: Vec<FunctionalModule> = Vec::new();
    let mut chans: Vec<FunctionalModule> = Vec::new();
    let mut conns: Vec<FunctionalModule> = Vec::new();
    for m in inferred {
        match m.kind {
            ModuleKind::Ic => ics.push(m),
            ModuleKind::Channels => chans.push(m),
            ModuleKind::Connector => conns.push(m),
            _ => {}
        }
    }
    chans.sort_by(|a, b| natural_cmp(&a.channels[0][0], &b.channels[0][0]));
    let mut ordered: Vec<FunctionalModule> = ics;
    ordered.extend(chans);
    ordered.extend(conns);

    // Whatever no module owns.
    let mut owned: BTreeSet<String> = ordered.iter().flat_map(|m| m.refs.iter().cloned()).collect();
    owned.extend(out.iter().flat_map(|m| m.refs.iter().cloned()));
    let mut leftovers: Vec<String> = model.parts.iter().map(|p| p.reference.clone()).filter(|r| !owned.contains(r)).collect();
    leftovers.sort_by(|a, b| natural_cmp(a, b));

    out.extend(ordered);
    if !leftovers.is_empty() {
        out.push(FunctionalModule { name: "Misc".to_string(), kind: ModuleKind::Misc, lead: None, refs: leftovers, channels: Vec::new() });
    }
    disambiguate(out)
}

/// Two modules never share a name (the name becomes a sheet name and a file name): the second gets a number.
fn disambiguate(mut modules: Vec<FunctionalModule>) -> Vec<FunctionalModule> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for m in modules.iter_mut() {
        let n = seen.entry(m.name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            m.name = format!("{} {}", m.name, n);
        }
    }
    modules
}

/// The index of the module holding `reference`.
pub fn module_of<'a>(modules: &'a [FunctionalModule], reference: &str) -> Option<usize> {
    modules.iter().position(|m| m.refs.iter().any(|r| r == reference))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Net, Pin};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, value: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(value.into()), package: None, footprint: None, symbol: None, datasheet: None, pins, body_um: None, edge: None }
    }
    fn two(reference: &str, value: &str) -> Part {
        part(reference, value, vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)])
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// The thirty-part MCU board the schematic work is judged on.
    fn mcu30() -> ConstraintModel {
        serde_yaml::from_str(include_str!("../../../examples/mcu_board_30plus.yaml")).expect("mcu30 intent parses")
    }

    #[test]
    fn natural_order_puts_c2_before_c10() {
        let mut v = vec!["C10", "C2", "U1", "C1", "R9", "R10"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["C1", "C2", "C10", "R9", "R10", "U1"]);
        assert_eq!(ref_prefix("SW12"), "SW");
    }

    #[test]
    fn mcu30_is_an_mcu_a_row_of_led_channels_and_a_header() {
        let m = infer_modules(&mcu30(), &[]);
        let names: Vec<&str> = m.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["MCU (U1)", "LED channels (D1\u{2013}D8)", "Header (J1)"]);
        assert_eq!(m[0].kind, ModuleKind::Ic);
        assert_eq!(m[1].kind, ModuleKind::Channels);
        assert_eq!(m[2].kind, ModuleKind::Connector);
        // The MCU keeps the twelve decoupling capacitors that sit on its rails.
        let mut mcu: Vec<String> = (1..=12).map(|i| format!("C{i}")).collect();
        mcu.push("U1".into());
        assert_eq!(m[0].refs, mcu);
        // The channels: each resistor with the LED behind it, in order, and every one of the sixteen parts exactly once.
        let chains: Vec<Vec<&str>> = m[1].channels.iter().map(|c| c.iter().map(String::as_str).collect()).collect();
        let want: Vec<Vec<String>> = (1..=8).map(|i| vec![format!("R{i}"), format!("D{i}")]).collect();
        let want: Vec<Vec<&str>> = want.iter().map(|c| c.iter().map(String::as_str).collect()).collect();
        assert_eq!(chains, want);
        assert_eq!(m[1].refs.len(), 16);
        assert_eq!(m[2].refs, vec!["J1"]);
        assert_eq!(m.iter().map(|m| m.refs.len()).sum::<usize>(), 30, "every part lands in exactly one module");
    }

    #[test]
    fn inference_is_deterministic() {
        let a = infer_modules(&mcu30(), &[]);
        let b = infer_modules(&mcu30(), &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn a_pull_up_stays_with_its_ic_and_is_not_a_channel() {
        // Two I2C pull-ups are identical and hang off one IC, but each is a lone resistor to a rail: bias, not a channel.
        let model = ConstraintModel {
            parts: vec![
                part("U1", "SENSOR", vec![pin("1", "VDD", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "SDA", PinKind::Signal), pin("4", "SCL", PinKind::Signal), pin("5", "INT", PinKind::Signal)]),
                two("R1", "4.7k"),
                two("R2", "4.7k"),
            ],
            nets: vec![net("VDD", &["U1.1", "R1.1", "R2.1"]), net("GND", &["U1.2"]), net("SDA", &["U1.3", "R1.2"]), net("SCL", &["U1.4", "R2.2"])],
            ..Default::default()
        };
        let m = infer_modules(&model, &[]);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].refs, vec!["R1", "R2", "U1"]);
        assert_eq!(m[0].name, "SENSOR (U1)");
    }

    #[test]
    fn a_capacitor_on_the_rails_of_two_ics_equally_goes_to_misc() {
        let ic = |r: &str| part(r, "CHIP", vec![pin("1", "VDD", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "IO", PinKind::Signal), pin("4", "IO2", PinKind::Signal), pin("5", "IO3", PinKind::Signal)]);
        let model = ConstraintModel {
            parts: vec![ic("U1"), ic("U2"), two("C1", "10u")],
            nets: vec![net("VDD", &["U1.1", "U2.1", "C1.1"]), net("GND", &["U1.2", "U2.2", "C1.2"]), net("SIG", &["U1.3", "U2.3"])],
            ..Default::default()
        };
        let m = infer_modules(&model, &[]);
        let names: Vec<&str> = m.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["CHIP (U1)", "CHIP (U2)", "Misc"], "{m:?}");
        assert_eq!(m[2].refs, vec!["C1"]);
    }

    #[test]
    fn a_series_part_between_an_ic_and_a_connector_goes_with_the_ic() {
        let model = ConstraintModel {
            parts: vec![
                part("U1", "CHIP", vec![pin("1", "VDD", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "TX", PinKind::Signal), pin("4", "RX", PinKind::Signal), pin("5", "IO", PinKind::Signal)]),
                part("J1", "USB-C", vec![pin("1", "TX", PinKind::Signal), pin("2", "GND", PinKind::Ground)]),
                two("R1", "33"),
            ],
            nets: vec![net("GND", &["U1.2", "J1.2"]), net("TXA", &["U1.3", "R1.1"]), net("TXB", &["R1.2", "J1.1"])],
            ..Default::default()
        };
        let m = infer_modules(&model, &[]);
        assert_eq!(m.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["CHIP (U1)", "USB-C (J1)"]);
        assert_eq!(m[0].refs, vec!["R1", "U1"]);
    }

    #[test]
    fn declared_modules_win_over_inference() {
        let model = mcu30();
        let recorded = vec![ModuleRegion { name: "indicators".into(), refs: (1..=8).flat_map(|i| [format!("D{i}"), format!("R{i}")]).collect(), rect: (0, 0, 1, 1) }];
        let m = infer_modules(&model, &recorded);
        assert_eq!(m[0].name, "indicators");
        assert_eq!(m[0].kind, ModuleKind::Declared);
        assert_eq!(m[0].refs.len(), 16);
        // Everything it does not name is inferred, without the indicators.
        assert_eq!(m.iter().map(|m| m.refs.len()).sum::<usize>(), 30);
        assert!(m.iter().skip(1).all(|m| m.refs.iter().all(|r| !r.starts_with('D') && !(r.starts_with('R') && r.len() == 2))));
    }

    #[test]
    fn proximity_rules_declare_modules() {
        let mut model = mcu30();
        model.placement_rules.push(crate::PlacementRule::Proximity { a: "C1".into(), b: "U1".into(), max_mm: 3.0, reason: None });
        let m = infer_modules(&model, &[]);
        assert_eq!(m[0].kind, ModuleKind::Declared);
        assert_eq!(m[0].name, "MCU (U1)", "{m:?}");
        // The rule pulls C1 to U1, and `partition` then adopts the parts only U1's own pins reach (the channels and the header).
        assert!(m[0].refs.iter().any(|r| r == "C1") && m[0].refs.iter().any(|r| r == "U1") && m[0].refs.iter().any(|r| r == "J1"), "{:?}", m[0].refs);
        // The eleven decoupling capacitors it left free have no anchor left to belong to.
        assert_eq!(m.last().unwrap().kind, ModuleKind::Misc);
        assert_eq!(m.last().unwrap().refs.len(), 11);
        assert_eq!(m.iter().map(|m| m.refs.len()).sum::<usize>(), 30);
    }

    #[test]
    fn a_board_of_passives_alone_is_one_misc_module() {
        let model = ConstraintModel { parts: vec![two("R1", "1k"), two("R2", "2k")], nets: vec![net("MID", &["R1.2", "R2.1"])], ..Default::default() };
        let m = infer_modules(&model, &[]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].kind, ModuleKind::Misc);
        assert_eq!(m[0].refs, vec!["R1", "R2"]);
    }
}
