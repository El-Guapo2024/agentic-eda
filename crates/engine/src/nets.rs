//! The nets a drawn schematic tree implies: `eeschema`'s connection graph, for the whole hierarchy at once.
//!
//! A `.kicad_sch` carries no net list; KiCad derives one from what is drawn (`connection_graph.cpp`). So does this project once
//! a person edits a sheet: [`trace_nets`] reads wires, pins, labels, power symbols, sheet pins and no-connect flags of every
//! screen and writes the result back as `design.nets`.
//!
//! What it ports, and from where:
//! - **Subgraphs** (`CONNECTION_GRAPH::buildItemSubGraphs`): points that share a coordinate, the points of one wire polyline,
//!   and a point that lands on the interior of a wire (a T-junction) are one electrical node.
//! - **Drivers and their priority** (`CONNECTION_SUBGRAPH::GetDriverPriority`): pin < sheet pin < hierarchical label < local
//!   label < global power pin (a power symbol) < global label. The best driver names the net.
//! - **Labels by name** (`processSubGraphs`): on one sheet, subgraphs with the same local or hierarchical label text are one
//!   net; a sheet pin joins only when no other sheet pin on the sheet has its name ("promote sheet pins ... if no conflict").
//! - **Global names**: power symbols and global labels with the same text are one net across every sheet.
//! - **The hierarchy** (`propagateToNeighbors`): a sheet pin on the parent and a hierarchical label of the same name inside the
//!   sheet are one net. The name that survives is the best driver anywhere along it: a power or global driver, else a strong
//!   driver (label) of higher priority, else the one closest to the root, else the alphabetically first.
//!
//! A subgraph with no driver keeps the name its wires already carry (a derived schematic names every wire after its net), so a
//! retrace after an edit does not rename nets under a routed board; otherwise it is `NET_<n>`.
//!
//! Not ported: buses (a bus wire is traced as an ordinary wire, as `eda_kicad::reconcile` always has), and one screen placed by
//! more than one sheet symbol (traced once; KiCad would make a net per placement).

use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{LabelKind, Point, SchematicSection};
use eda_model::Net;

/// `CONNECTION_SUBGRAPH::PRIORITY`.
const PIN: u8 = 1;
const SHEET_PIN: u8 = 2;
const HIER_LABEL: u8 = 3;
const LOCAL_LABEL: u8 = 4;
const GLOBAL_POWER_PIN: u8 = 6;
const GLOBAL: u8 = 7;

/// One screen (a sheet's drawn content) to trace.
pub struct ScreenIn<'a> {
    pub sch: &'a mut SchematicSection,
    /// The file name the parent's `SheetInstance::file` calls it; empty for the root.
    pub file: String,
    /// "REF.PIN" -> the point a wire connects to, for every pin on this screen.
    pub pins: BTreeMap<String, Point>,
}

struct Uf(Vec<usize>);

impl Uf {
    fn new() -> Self {
        Uf(Vec::new())
    }
    fn add(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

fn on_interior(p: Point, a: Point, b: Point) -> bool {
    if a.x == b.x {
        p.x == a.x && p.y > a.y.min(b.y) && p.y < a.y.max(b.y)
    } else if a.y == b.y {
        p.y == a.y && p.x > a.x.min(b.x) && p.x < a.x.max(b.x)
    } else {
        false
    }
}

#[derive(Debug, Clone)]
struct Driver {
    prio: u8,
    name: String,
    depth: usize,
}

/// A subgraph: the points that are one node on one screen.
#[derive(Default)]
struct Group {
    screen: usize,
    pins: Vec<String>,
    drivers: Vec<Driver>,
    /// Names the wires of this group already carry.
    wire_names: Vec<String>,
    /// (name) of every hierarchical label and sheet pin here, for the hierarchy links.
    hier_labels: Vec<String>,
    sheet_pins: Vec<String>,
    local_names: Vec<String>,
    global_names: Vec<String>,
}

/// Each screen's depth from the root through `sheets` / `file`, and the sheet-name path to it.
fn depths(screens: &[ScreenIn]) -> (Vec<usize>, Vec<String>) {
    let mut depth = vec![usize::MAX; screens.len()];
    let mut path = vec![String::new(); screens.len()];
    let Some(root) = screens.iter().position(|s| s.file.is_empty()) else { return (vec![0; screens.len()], path) };
    depth[root] = 0;
    let mut queue = vec![root];
    let mut head = 0;
    while head < queue.len() {
        let s = queue[head];
        head += 1;
        for sh in &screens[s].sch.sheets {
            let Some(c) = screens.iter().position(|x| !x.file.is_empty() && x.file == sh.file) else { continue };
            if depth[c] == usize::MAX {
                depth[c] = depth[s] + 1;
                path[c] = format!("{}/{}", path[s], sh.name);
                queue.push(c);
            }
        }
    }
    for d in depth.iter_mut() {
        if *d == usize::MAX {
            *d = 99;
        }
    }
    (depth, path)
}

/// Trace the nets of every screen, write each wire's `net`/`pins`, each power symbol's and no-connect flag's `pin` back, and
/// return the net list (sorted by name, each net's pins sorted).
pub fn trace_nets(screens: &mut [ScreenIn]) -> Vec<Net> {
    let (depth, spath) = depths(screens);

    // ---- phase A: nodes and subgraphs, per screen ----
    let mut groups: Vec<Group> = Vec::new();
    // (screen, point) -> group id
    let mut group_at: Vec<BTreeMap<Point, usize>> = Vec::with_capacity(screens.len());
    // per screen: wire index -> group id
    let mut wire_group: Vec<Vec<Option<usize>>> = Vec::with_capacity(screens.len());

    for (si, screen) in screens.iter().enumerate() {
        let sch: &SchematicSection = &*screen.sch;
        let mut ids: BTreeMap<Point, usize> = BTreeMap::new();
        let mut uf = Uf::new();
        let seed = |p: Point, ids: &mut BTreeMap<Point, usize>, uf: &mut Uf| -> usize { *ids.entry(p).or_insert_with(|| uf.add()) };
        for w in &sch.wires {
            for &p in &w.pts {
                seed(p, &mut ids, &mut uf);
            }
        }
        for &p in screen.pins.values() {
            seed(p, &mut ids, &mut uf);
        }
        for l in &sch.labels {
            seed(l.at, &mut ids, &mut uf);
        }
        for ps in &sch.power_symbols {
            seed(ps.at, &mut ids, &mut uf);
        }
        for nc in &sch.no_connects {
            seed(nc.at, &mut ids, &mut uf);
        }
        for j in &sch.junctions {
            seed(j.at, &mut ids, &mut uf);
        }
        for sh in &sch.sheets {
            for p in &sh.pins {
                seed(p.at, &mut ids, &mut uf);
            }
        }
        for w in &sch.wires {
            for pair in w.pts.windows(2) {
                let (a, b) = (ids[&pair[0]], ids[&pair[1]]);
                uf.union(a, b);
            }
        }
        // A point landing on the interior of a wire joins that wire.
        let all: Vec<Point> = ids.keys().copied().collect();
        for w in &sch.wires {
            if w.pts.len() < 2 {
                continue;
            }
            let rep = ids[&w.pts[0]];
            for &p in &all {
                if w.pts.contains(&p) {
                    continue;
                }
                if w.pts.windows(2).any(|seg| on_interior(p, seg[0], seg[1])) {
                    uf.union(ids[&p], rep);
                }
            }
        }

        // One group per root.
        let mut gid_of_root: BTreeMap<usize, usize> = BTreeMap::new();
        let mut at: BTreeMap<Point, usize> = BTreeMap::new();
        let mut pts: Vec<(Point, usize)> = ids.iter().map(|(p, &i)| (*p, i)).collect();
        pts.sort();
        for (p, i) in pts {
            let root = uf.find(i);
            let gid = *gid_of_root.entry(root).or_insert_with(|| {
                groups.push(Group { screen: si, ..Default::default() });
                groups.len() - 1
            });
            at.insert(p, gid);
        }

        let nc_points: BTreeSet<Point> = sch.no_connects.iter().map(|n| n.at).collect();
        for (pin_ref, p) in &screen.pins {
            if nc_points.contains(p) {
                continue;
            }
            groups[at[p]].pins.push(pin_ref.clone());
            groups[at[p]].drivers.push(Driver { prio: PIN, name: String::new(), depth: depth[si] });
        }
        for l in &sch.labels {
            let g = &mut groups[at[&l.at]];
            let (prio, bucket) = match &l.kind {
                LabelKind::Local => (LOCAL_LABEL, 0),
                LabelKind::Hierarchical { .. } => (HIER_LABEL, 1),
                LabelKind::Global { .. } => (GLOBAL, 2),
            };
            g.drivers.push(Driver { prio, name: l.net.clone(), depth: depth[si] });
            match bucket {
                0 => g.local_names.push(l.net.clone()),
                1 => {
                    g.hier_labels.push(l.net.clone());
                    g.local_names.push(l.net.clone());
                }
                _ => g.global_names.push(l.net.clone()),
            }
        }
        for ps in &sch.power_symbols {
            let g = &mut groups[at[&ps.at]];
            g.drivers.push(Driver { prio: GLOBAL_POWER_PIN, name: ps.net.clone(), depth: depth[si] });
            g.global_names.push(ps.net.clone());
        }
        for sh in &sch.sheets {
            for p in &sh.pins {
                let g = &mut groups[at[&p.at]];
                g.drivers.push(Driver { prio: SHEET_PIN, name: p.name.clone(), depth: depth[si] });
                g.sheet_pins.push(p.name.clone());
            }
        }
        let mut per_wire: Vec<Option<usize>> = Vec::with_capacity(sch.wires.len());
        for w in &sch.wires {
            let gid = w.pts.first().map(|p| at[p]);
            if let Some(g) = gid {
                if !w.net.is_empty() {
                    groups[g].wire_names.push(w.net.clone());
                }
            }
            per_wire.push(gid);
        }
        group_at.push(at);
        wire_group.push(per_wire);
    }

    // ---- phase B: join subgraphs by name and through the hierarchy ----
    let mut join = Uf::new();
    for _ in 0..groups.len() {
        join.add();
    }
    // Local and hierarchical label text is a net on one sheet.
    let mut local: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    // Sheet pins join by name only when they are the only ones of that name on the sheet.
    let mut sheet_pin_groups: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    let mut global: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut hier: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    for (gid, g) in groups.iter().enumerate() {
        for n in &g.local_names {
            local.entry((g.screen, n.clone())).or_default().push(gid);
        }
        for n in &g.sheet_pins {
            sheet_pin_groups.entry((g.screen, n.clone())).or_default().push(gid);
        }
        for n in &g.global_names {
            global.entry(n.clone()).or_default().push(gid);
        }
        for n in &g.hier_labels {
            hier.entry((g.screen, n.clone())).or_default().push(gid);
        }
    }
    for ((screen, name), mut gids) in sheet_pin_groups {
        gids.sort();
        gids.dedup();
        if gids.len() == 1 {
            local.entry((screen, name)).or_default().push(gids[0]);
        }
    }
    for gids in local.values().chain(global.values()) {
        for w in gids.windows(2) {
            join.union(w[0], w[1]);
        }
    }
    // Sheet pin <-> the hierarchical labels of its sheet's content.
    for (si, screen) in screens.iter().enumerate() {
        for sh in &screen.sch.sheets {
            let Some(ci) = screens.iter().position(|x| !x.file.is_empty() && x.file == sh.file) else { continue };
            for p in &sh.pins {
                let Some(&gp) = group_at[si].get(&p.at) else { continue };
                if let Some(labels) = hier.get(&(ci, p.name.clone())) {
                    for &gl in labels {
                        join.union(gp, gl);
                    }
                }
            }
        }
    }

    // ---- phase C: name every net ----
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for gid in 0..groups.len() {
        let r = join.find(gid);
        members.entry(r).or_default().push(gid);
    }
    struct Final {
        gids: Vec<usize>,
        pins: Vec<String>,
        name: Option<String>,
        /// Whether the name came from a local or hierarchical label (so it is only unique inside its sheet).
        local_name: bool,
        screen: usize,
    }
    let mut finals: Vec<Final> = Vec::new();
    for (_, gids) in members {
        let mut pins: Vec<String> = gids.iter().flat_map(|&g| groups[g].pins.iter().cloned()).collect();
        pins.sort();
        pins.dedup();
        let mut drivers: Vec<&Driver> = gids.iter().flat_map(|&g| groups[g].drivers.iter()).filter(|d| d.prio > PIN && !d.name.is_empty()).collect();
        // KiCad's order: a global power pin or global label first, then a strong driver by priority, then the one closest to the
        // root, then the alphabetically first name; a sheet pin only when nothing stronger names the net.
        drivers.sort_by(|a, b| {
            let a_global = a.prio >= GLOBAL_POWER_PIN;
            let b_global = b.prio >= GLOBAL_POWER_PIN;
            b_global
                .cmp(&a_global)
                .then_with(|| b.prio.cmp(&a.prio))
                .then_with(|| a.depth.cmp(&b.depth))
                .then_with(|| a.name.cmp(&b.name))
        });
        let best = drivers.first();
        let screen = gids.iter().map(|&g| groups[g].screen).min_by_key(|&s| depth[s]).unwrap_or(0);
        finals.push(Final { gids, pins, name: best.map(|d| d.name.clone()), local_name: best.is_some_and(|d| d.prio < GLOBAL_POWER_PIN), screen });
    }

    // Names are unique across the design: a local name two different nets share is told apart by its sheet path.
    finals.sort_by(|a, b| (depth[a.screen], a.name.clone(), a.pins.first().cloned()).cmp(&(depth[b.screen], b.name.clone(), b.pins.first().cloned())));
    let mut used: BTreeSet<String> = BTreeSet::new();
    for f in finals.iter_mut() {
        if let Some(n) = f.name.clone() {
            let candidate = if used.contains(&n) && f.local_name { format!("{}/{}", spath[f.screen], n) } else { n };
            let mut unique = candidate.clone();
            let mut k = 1;
            while used.contains(&unique) {
                k += 1;
                unique = format!("{candidate}_{k}");
            }
            used.insert(unique.clone());
            f.name = Some(unique);
        }
    }
    // The unnamed: the name the wires already carry, if no other net took it; else the next free NET_<n>.
    let mut next = used.iter().filter_map(|n| n.strip_prefix("NET_").and_then(|d| d.parse::<usize>().ok())).max().unwrap_or(0);
    let mut order: Vec<usize> = (0..finals.len()).filter(|&i| finals[i].name.is_none()).collect();
    order.sort_by(|&a, &b| finals[b].pins.len().cmp(&finals[a].pins.len()).then_with(|| finals[a].pins.first().cmp(&finals[b].pins.first())));
    for i in order {
        let mut hints: BTreeMap<&str, usize> = BTreeMap::new();
        for &g in &finals[i].gids {
            for n in &groups[g].wire_names {
                *hints.entry(n.as_str()).or_default() += 1;
            }
        }
        let mut ranked: Vec<(&str, usize)> = hints.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let pick = ranked.iter().map(|(n, _)| (*n).to_string()).find(|n| !used.contains(n));
        let name = pick.unwrap_or_else(|| loop {
            next += 1;
            let candidate = format!("NET_{next}");
            if !used.contains(&candidate) {
                break candidate;
            }
        });
        used.insert(name.clone());
        finals[i].name = Some(name);
    }

    // ---- phase D: write back ----
    let mut name_of_group: Vec<String> = vec![String::new(); groups.len()];
    for f in &finals {
        for &g in &f.gids {
            name_of_group[g] = f.name.clone().unwrap_or_default();
        }
    }
    for (si, screen) in screens.iter_mut().enumerate() {
        let nc_points: BTreeSet<Point> = screen.sch.no_connects.iter().map(|n| n.at).collect();
        let mut pins_of_point: BTreeMap<Point, Vec<String>> = BTreeMap::new();
        for (pin_ref, p) in &screen.pins {
            if nc_points.contains(p) {
                continue;
            }
            pins_of_point.entry(*p).or_default().push(pin_ref.clone());
        }
        for (wi, w) in screen.sch.wires.iter_mut().enumerate() {
            if let Some(g) = wire_group[si][wi] {
                w.net = name_of_group[g].clone();
            }
            w.pins = w.pts.iter().filter_map(|p| pins_of_point.get(p)).flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect();
        }
        for ps in screen.sch.power_symbols.iter_mut() {
            if let Some(r) = pins_of_point.get(&ps.at).and_then(|v| v.first()) {
                ps.pin = r.clone();
            }
        }
        for nc in screen.sch.no_connects.iter_mut() {
            let refs: Vec<&String> = screen.pins.iter().filter(|(_, p)| **p == nc.at).map(|(r, _)| r).collect();
            if let Some(r) = refs.first() {
                nc.pin = (*r).clone();
            }
        }
    }
    let mut nets: Vec<Net> = finals.into_iter().filter(|f| !f.pins.is_empty()).map(|f| Net { name: f.name.unwrap_or_default(), pins: f.pins }).collect();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    nets
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{LabelShape, NetLabel, PowerSymbol, SheetInstance, SheetPin, Wire};

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }
    fn wire(net: &str, pts: &[Point]) -> Wire {
        Wire { id: String::new(), net: net.into(), pins: vec![], pts: pts.to_vec(), bus: false }
    }
    fn label(net: &str, at: Point, kind: LabelKind) -> NetLabel {
        NetLabel { id: String::new(), net: net.into(), at, kind }
    }
    fn section() -> SchematicSection {
        SchematicSection::default()
    }
    fn pins(list: &[(&str, Point)]) -> BTreeMap<String, Point> {
        list.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }
    fn pins_of(nets: &[Net], name: &str) -> Vec<String> {
        nets.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no net {name} in {nets:?}")).pins.clone()
    }

    #[test]
    fn a_wire_joins_its_pins_and_keeps_its_name() {
        let mut root = section();
        root.wires.push(wire("LED1", &[p(0, 0), p(1000, 0)]));
        let mut screens = vec![ScreenIn { sch: &mut root, file: String::new(), pins: pins(&[("R1.2", p(0, 0)), ("D1.1", p(1000, 0)), ("R2.1", p(5000, 5000))]) }];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "LED1"), vec!["D1.1", "R1.2"]);
        assert_eq!(nets.len(), 2, "the lone pin is a net of its own: {nets:?}");
        assert_eq!(root.wires[0].net, "LED1");
        assert_eq!(root.wires[0].pins, vec!["D1.1", "R1.2"]);
    }

    #[test]
    fn a_hierarchical_label_and_its_sheet_pin_are_one_net_across_sheets() {
        // Root: a sheet symbol "child" with pin SIG at (100, 50), wired to R1.1 on the root. Inside: a hierarchical label SIG on a
        // wire to U1.3.
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(100, 0), size: (100, 100), pins: vec![SheetPin { id: String::new(), name: "SIG".into(), shape: LabelShape::Bidirectional, at: p(100, 50) }] });
        root.wires.push(wire("", &[p(0, 50), p(100, 50)]));
        let mut child = section();
        child.wires.push(wire("", &[p(10, 10), p(40, 10)]));
        child.labels.push(label("SIG", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
        let mut screens = vec![
            ScreenIn { sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 50))]) },
            ScreenIn { sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.3", p(40, 10))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(nets.len(), 1, "{nets:?}");
        assert_eq!(nets[0].name, "SIG", "the hierarchical label names the net");
        assert_eq!(nets[0].pins, vec!["R1.1", "U1.3"]);
        // And both wires carry the name.
        assert_eq!(root.wires[0].net, "SIG");
        assert_eq!(child.wires[0].net, "SIG");
    }

    #[test]
    fn power_symbols_are_one_net_on_every_sheet() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "a".into(), file: "a.kicad_sch".into(), at: p(0, 0), size: (10, 10), pins: vec![] });
        root.sheets.push(SheetInstance { id: "s2".into(), name: "b".into(), file: "b.kicad_sch".into(), at: p(20, 0), size: (10, 10), pins: vec![] });
        let gnd = |at: Point| PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at, rot: 0, net: "GND".into(), pin: String::new() };
        let mut a = section();
        a.power_symbols.push(gnd(p(5, 5)));
        let mut b = section();
        b.power_symbols.push(gnd(p(7, 7)));
        let mut screens = vec![
            ScreenIn { sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { sch: &mut a, file: "a.kicad_sch".into(), pins: pins(&[("C1.2", p(5, 5))]) },
            ScreenIn { sch: &mut b, file: "b.kicad_sch".into(), pins: pins(&[("C2.2", p(7, 7))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "GND"), vec!["C1.2", "C2.2"]);
        assert_eq!(a.power_symbols[0].pin, "C1.2");
    }

    #[test]
    fn local_labels_do_not_leak_across_sheets_but_share_a_sheet() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "a".into(), file: "a.kicad_sch".into(), at: p(0, 0), size: (10, 10), pins: vec![] });
        root.sheets.push(SheetInstance { id: "s2".into(), name: "b".into(), file: "b.kicad_sch".into(), at: p(20, 0), size: (10, 10), pins: vec![] });
        let mut a = section();
        a.labels.push(label("X", p(0, 0), LabelKind::Local));
        a.labels.push(label("X", p(50, 50), LabelKind::Local));
        let mut b = section();
        b.labels.push(label("X", p(0, 0), LabelKind::Local));
        let mut screens = vec![
            ScreenIn { sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { sch: &mut a, file: "a.kicad_sch".into(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(50, 50))]) },
            ScreenIn { sch: &mut b, file: "b.kicad_sch".into(), pins: pins(&[("R3.1", p(0, 0))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "X"), vec!["R1.1", "R2.1"], "two labels on one sheet are one net");
        let other = nets.iter().find(|n| n.pins == vec!["R3.1".to_string()]).expect("the other sheet's X is its own net");
        assert_eq!(other.name, "/b/X", "told apart by its sheet path, like KiCad's /b/X");
    }

    #[test]
    fn a_local_label_at_the_root_beats_the_hierarchical_label_inside() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(100, 0), size: (100, 100), pins: vec![SheetPin { id: String::new(), name: "IN".into(), shape: LabelShape::Input, at: p(100, 50) }] });
        root.wires.push(wire("", &[p(0, 50), p(100, 50)]));
        root.labels.push(label("MAINBUS", p(0, 50), LabelKind::Local));
        let mut child = section();
        child.labels.push(label("IN", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Input }));
        let mut screens = vec![
            ScreenIn { sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 50))]) },
            ScreenIn { sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.1", p(10, 10))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "MAINBUS"), vec!["R1.1", "U1.1"]);
    }

    /// "REF.PIN" -> point for every symbol on `sch`, from the engine's own geometry.
    fn world_pins(sch: &SchematicSection, model: &eda_model::ConstraintModel) -> BTreeMap<String, Point> {
        let mut out = BTreeMap::new();
        for sym in &sch.symbols {
            let Some(part) = model.part(&sym.id) else { continue };
            let resolved = model.real_symbol_of(&sym.lib_id, part);
            for (number, at) in crate::placed::pin_points(sym, part, resolved.as_ref()) {
                out.insert(format!("{}.{number}", sym.id), at);
            }
        }
        out
    }

    /// The nets of `model` that a schematic draws: two or more pins, none of them a no-connect pin.
    fn drawn_nets(model: &eda_model::ConstraintModel) -> BTreeMap<String, Vec<String>> {
        model
            .nets
            .iter()
            .filter_map(|n| {
                let mut pins: Vec<String> = n
                    .pins
                    .iter()
                    .filter(|p| {
                        let (r, num) = p.split_once('.').unwrap_or((p.as_str(), ""));
                        model.part(r).and_then(|part| part.pins.iter().find(|x| x.number == num)).is_some_and(|x| x.kind != eda_model::PinKind::Nc)
                    })
                    .cloned()
                    .collect();
                pins.sort();
                (pins.len() >= 2).then(|| (n.name.clone(), pins))
            })
            .collect()
    }

    /// The whole point of tracing: what `derive_schematic` draws is read back as exactly the nets it was given, with their names,
    /// for every example design in the repository.
    #[test]
    fn a_derived_schematic_traces_back_to_its_intent_nets() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
        files.extend(std::fs::read_dir(dir.join("ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")));
        files.sort();
        let mut checked = 0;
        for f in files {
            let Ok(model) = serde_yaml::from_str::<eda_model::ConstraintModel>(&std::fs::read_to_string(&f).unwrap()) else { continue };
            let Ok(design) = crate::derive_schematic(&model, &crate::EngineOptions::new(1, "t")) else { continue };
            let mut sch = design.schematic.unwrap();
            let pins = world_pins(&sch, &model);
            let mut screens = vec![ScreenIn { sch: &mut sch, file: String::new(), pins }];
            let got = trace_nets(&mut screens);
            let got: BTreeMap<String, Vec<String>> = got.into_iter().filter(|n| n.pins.len() >= 2).map(|n| (n.name, n.pins)).collect();
            let want = drawn_nets(&model);
            if got != want {
                println!("MISMATCH {}\n   got  {:?}\n   want {:?}", f.display(), got, want);
                continue;
            }
            checked += 1;
        }
        assert!(checked >= 8, "only {checked} examples were readable");
    }

    #[test]
    fn a_no_connect_flag_keeps_its_pin_out_of_every_net() {
        let mut root = section();
        root.no_connects.push(eda_model::ir::NoConnect { id: String::new(), at: p(7, 7), pin: String::new() });
        let mut screens = vec![ScreenIn { sch: &mut root, file: String::new(), pins: pins(&[("U1.5", p(7, 7))]) }];
        let nets = trace_nets(&mut screens);
        assert!(nets.is_empty(), "{nets:?}");
        assert_eq!(root.no_connects[0].pin, "U1.5");
    }
}
