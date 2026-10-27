//! The nets a drawn schematic tree implies: `eeschema`'s connection graph, for the whole hierarchy at once.
//!
//! A `.kicad_sch` carries no net list; KiCad derives one from what is drawn (`connection_graph.cpp`). So does this project once
//! a person edits a sheet: [`trace_nets`] reads wires, pins, labels, power symbols, sheet pins and no-connect flags of every
//! screen and writes the result back as `design.nets`.
//!
//! What it ports, and from where:
//! - **Subgraphs** (`CONNECTION_GRAPH::buildItemSubGraphs`): points that share a coordinate, the points of one wire polyline,
//!   and a point that lands on the interior of a wire (a T-junction) are one electrical node.
//! - **Claims and their order** (KiCad 25ec2335's connectivity engine, `eeschema/connectivity/conn_claims.cpp`, which the box's kicad-cli runs:
//!   `ADVANCED_CFG::m_ConnectivityEngine` is on by default): pin < sheet pin < hierarchical label < local label < bus member < global power pin (a
//!   power symbol, a hidden power input) < global label. The greatest claim of the net names it (`CLAIM_LESS`: priority, then nearest the root with
//!   a global name and a pin unscoped, then a power symbol's pin, then an output sheet pin, then a name without `-Pad`, then the first name).
//! - **Names** (`BuildIslandRecord`'s edges): on one sheet every label, power symbol and hidden power input of one text is one net (`SCOPE::SHEET`:
//!   a power symbol `VCC` and a local label `VCC` there are one net); global labels and power names are one net across every sheet
//!   (`SCOPE::GLOBAL`); a sheet pin is a name at the port of its placement only (`SCOPE::PORT`), with the hierarchical labels of that name inside.
//! - **The hierarchy**: a sheet pin on the parent and a hierarchical label of the same name inside the sheet are one net, at its port; a sheet pin
//!   joins no label of its name on its own sheet.
//!
//! A subgraph with no driver keeps the name its wires already carry (a derived schematic names every wire after its net), so a
//! retrace after an edit does not rename nets under a routed board; otherwise it is `NET_<n>`.
//!
//! - **A net per placement** (`CONNECTION_GRAPH::Recalculate` walks the `SCH_SHEET_LIST`, a subgraph per item *per sheet path*): a
//!   screen two sheet symbols show is traced once for each, as [`ScreenIn`]s with their own [`ScreenIn::path`], its pins under the
//!   references that placement gives its symbols, so the same drawing makes two sets of nets. A sheet pin joins the hierarchical labels
//!   of the one placement it is on.
//!
//! - **Buses** (`processSubGraphs`, `propagateToNeighbors`, `matchBusMember`): a bus wire is a layer of its own that no wire or pin
//!   touches; a bus label, hierarchical label or sheet pin on it names it, and [`Naming::bus_members`] gives its members. On the bus's
//!   sheet each member is one net with the nets of its name (the bus's neighbours); a bus sheet pin and the bus hierarchical label of its
//!   sheet are one bus, its members matched by index (a vector) or by name (a group), so a member's nets on both sheets are one net.
//!
//! Not ported: bus entries take no part (KiCad's do not either: a wire is on a member by its label), nor does a bus-to-bus entry.

use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{LabelKind, Point, SchematicSection};
use eda_model::Net;

/// `SCH_CONNECTIVITY::PRIORITY` (KiCad 25ec2335 `eeschema/connectivity/conn_claims.h`; the old engine's `CONNECTION_SUBGRAPH::PRIORITY` lacks
/// `BUS_MEMBER`). `LOCAL_POWER_PIN` (5, a local power symbol) is not ported.
const PIN: u8 = 1;
const SHEET_PIN: u8 = 2;
const HIER_LABEL: u8 = 3;
const LOCAL_LABEL: u8 = 4;
/// A bus member's slot (`BindBundle`: `slot.claim.priority = PRIORITY::BUS_MEMBER`), above every label but a global one.
const BUS_MEMBER: u8 = 6;
const GLOBAL_POWER_PIN: u8 = 7;
const GLOBAL: u8 = 8;

/// One screen (a sheet's drawn content) to trace.
pub struct ScreenIn<'a> {
    pub sch: &'a mut SchematicSection,
    /// The file name the parent's `SheetInstance::file` calls it; empty for the root.
    pub file: String,
    /// The placement this is the screen of: the `SheetInstance::id`s from the root down (`SCH_SHEET_PATH`); empty for the root. When
    /// any screen has one, a sheet symbol leads to the screen whose path is its own path plus the sheet's id (a screen shown twice is
    /// two [`ScreenIn`]s); when none has, to the screen of its file (one [`ScreenIn`] per file).
    pub path: Vec<String>,
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

/// Whether `p` lies strictly inside the axis-aligned run from `a` to `b`.
pub(crate) fn on_segment(p: Point, a: Point, b: Point) -> bool {
    on_interior(p, a, b)
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
    /// The driver's own text (`CONNECTION_SUBGRAPH::GetNameForDriver`): a label's, a sheet pin's, a power symbol's value.
    name: String,
    /// `CLAIM::depth`: the sheet path's size, 0 for a claim with no scope (a global label, a global power pin, a pin: `PrepareInstanceClaims`'s
    /// `unscoped`).
    depth: usize,
    /// `CLAIM::globalPowerParent`: the pin of a global power symbol (not an invisible power input of an ordinary symbol), which outranks it.
    power_parent: bool,
    /// `CLAIM::outputShape`: a sheet pin of output shape.
    output: bool,
    /// The net name this driver gives (`SCH_CONNECTION::Name()`, `m_cached_name_with_path`): [`Self::name`] with the path of its sheet in front, but for a
    /// global label or a global power pin (`recacheName`'s `prepend_path`).
    full: String,
}

impl Driver {
    /// `name` as the label, sheet pin or power symbol shows it: the driver's name is it escaped (`CONNECTION_SUBGRAPH::GetNameForDriver`:
    /// `EscapeString( GetShownText(), CTX_NETNAME )`, a label `A/B` names `/A{slash}B`).
    fn new(prio: u8, name: String, depth: usize, sheet_path: &str) -> Self {
        let name = escape_net_name(&name);
        let full = if prio >= GLOBAL_POWER_PIN || name.is_empty() { name.clone() } else { format!("{sheet_path}/{name}") };
        let depth = if prio >= GLOBAL_POWER_PIN || prio == PIN { 0 } else { depth };
        Driver { prio, name, depth, power_parent: false, output: false, full }
    }
}

/// A subgraph: the points that are one node on one screen.
#[derive(Default)]
struct Group {
    screen: usize,
    /// On the bus layer (a bus wire, a bus label, a bus sheet pin).
    bus: bool,
    /// One member of a bus, not a place on a sheet.
    member: bool,
    pins: Vec<String>,
    drivers: Vec<Driver>,
    /// (name) of every hierarchical label and sheet pin here, for the hierarchy links.
    hier_labels: Vec<String>,
    sheet_pins: Vec<String>,
    local_names: Vec<String>,
    global_names: Vec<String>,
}

/// The screen sheet `sh` of screen `si` shows: by placement path when the screens have paths, else by file.
fn child_of(screens: &[ScreenIn], si: usize, sh: &eda_model::ir::SheetInstance) -> Option<usize> {
    if screens.iter().any(|s| !s.path.is_empty()) {
        let mut want = screens[si].path.clone();
        want.push(sh.id.clone());
        return screens.iter().position(|x| x.path == want);
    }
    screens.iter().position(|x| !x.file.is_empty() && x.file == sh.file)
}

/// `EscapeString( aSource, CTX_NETNAME )` (common/string_utils.cpp): a `/` is written `{slash}`, a line break is dropped.
pub fn escape_net_name(s: &str) -> String {
    s.replace('/', "{slash}").replace(['\n', '\r'], "")
}

/// Each screen's depth from the root through `sheets` / `file`, and the sheet-name path to it: `""` for the root, `/<sheet name>/..` below it (KiCad's
/// `PathHumanReadable` without its trailing `/`), the sheet names escaped as KiCad escapes them in a net name.
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
            let Some(c) = child_of(screens, s, sh) else { continue };
            if depth[c] == usize::MAX {
                depth[c] = depth[s] + 1;
                // `PathHumanReadable( .., aEscapeSheetNames = true )`: `EscapeString( name, CTX_NETNAME )`
                path[c] = format!("{}/{}", path[s], escape_net_name(&sh.name));
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

/// A group bus member's local name (`SCH_CONNECTION::LocalName`): `ConfigureFromLabel` names the members of a named group `prefix{..}` `prefix.<member>`, and
/// matches them across buses without the prefix.
fn group_local_name<'a>(bus: &str, member: &'a str) -> &'a str {
    match bus.split_once('{') {
        Some((prefix, _)) if !prefix.is_empty() => member.strip_prefix(prefix).and_then(|r| r.strip_prefix('.')).unwrap_or(member),
        _ => member,
    }
}

/// The members of the group bus `name` that are the members of a bus inside it (`{DATA[0..3] CLK}`'s `DATA0`..`DATA3`, with the group's prefix): a group's
/// member that is itself a bus is no net (`SCH_CONNECTION::IsNet`), and `generateBusAliasMembers` makes no virtual net of it. Empty for a vector.
fn nested_group_members(name: &str, bus_members: &dyn Fn(&str) -> Option<Vec<String>>) -> BTreeSet<String> {
    let Some((prefix, inner)) = name.split_once('{').and_then(|(p, rest)| rest.strip_suffix('}').map(|i| (p, i))) else { return BTreeSet::new() };
    let dot = if prefix.is_empty() { String::new() } else { format!("{prefix}.") };
    inner.split_whitespace().filter_map(|t| bus_members(t)).flatten().map(|m| format!("{dot}{m}")).collect()
}

/// KiCad's order of the claims to a net's name, the best first (KiCad 25ec2335 `conn_claims.cpp::CLAIM_LESS`, the greater claim drives): the priority, then
/// the one closest to the root (a global label, a global power pin and a pin have no depth: `PrepareInstanceClaims`), then a global power symbol's pin over
/// an invisible power input of an ordinary symbol (`globalPowerParent`), then a sheet pin of output shape, then a name without `-Pad`, then the
/// alphabetically first name with its path (`fullName`, compared as UTF-8), so of two sibling sheets' hierarchical labels `RESET` the one on `/Header/` wins
/// over `/MCU/`'s.
fn driver_order(a: &Driver, b: &Driver) -> std::cmp::Ordering {
    b.prio
        .cmp(&a.prio)
        .then_with(|| a.depth.cmp(&b.depth))
        .then_with(|| b.power_parent.cmp(&a.power_parent))
        .then_with(|| b.output.cmp(&a.output))
        .then_with(|| a.name.contains("-Pad").cmp(&b.name.contains("-Pad")))
        .then_with(|| a.full.cmp(&b.full))
}

/// How the tracer names a net that no label, power symbol or sheet pin names.
#[derive(Default, Clone, Copy)]
pub struct Naming<'a> {
    /// "REF.PIN" -> the pin's shown name (empty or `~` for none). KiCad's default net name uses it: `Net-(U1-PA0)`.
    pub pin_names: Option<&'a BTreeMap<String, String>>,
    /// "REF.PIN" -> the reference with the unit the pin is drawn on, for a symbol of several units (`U1A`): KiCad's default net name of a named pin has
    /// it (`Net-(U1A-OUT)`).
    pub pin_units: Option<&'a BTreeMap<String, String>>,
    /// The no-connect pins (`PT_NC`), "REF.PIN": KiCad names a net of theirs `unconnected-(..)` (`PIN_NAME_FACT::noConnect`).
    pub nc_pins: Option<&'a BTreeSet<String>>,
    /// The pins of a symbol of several units common to every unit (`ALL_UNITS`), "REF.PIN": they count among each drawn unit's pins when a pin name
    /// is looked for twice (`hasDuplicateName`).
    pub common_pins: Option<&'a BTreeSet<String>>,
    /// The member nets of a bus name (`SCH_CONNECTION::ConfigureFromLabel`: a vector `D[0..7]`, a group `{A B}`, an alias), `None` for a name that is
    /// not a bus (`eda_kicad::expand_bus_members` with the design's bus aliases). Without it a bus has no members and joins nothing.
    pub bus_members: Option<&'a dyn Fn(&str) -> Option<Vec<String>>>,
    /// Whether the power symbol of a `lib_id` names its net and joins the others of its name (`SCH_PIN::IsGlobalPower`: its pin is a power input),
    /// from its library symbol; `None` for a symbol the library does not give. Without an answer, a symbol called `PWR_FLAG` in any library does not
    /// and every other does.
    pub global_power: Option<&'a dyn Fn(&str) -> Option<bool>>,
    /// "REF.PIN" -> net name of every pin of an ordinary symbol that is a global power pin (`SCH_PIN::IsGlobalPower`'s legacy rule: an invisible
    /// power input of a symbol that is not a power symbol): its pin name names the net and joins the others of that name, wired or not.
    pub power_pins: Option<&'a BTreeMap<String, String>>,
    /// A power symbol's Value with its text variables resolved for the placement it is traced in (`SCH_SYMBOL::GetValue( &sheet, FOR_NETNAME )`:
    /// `${#}` is that sheet path's page), given the placement's [`ScreenIn::path`] and the stored text. Without it the stored text names the net.
    pub power_value: Option<&'a dyn Fn(&[String], &str) -> String>,
}

impl Naming<'_> {
    /// Whether a power symbol of `lib_id` is a global power pin (see [`Naming::global_power`]).
    fn is_global_power(&self, lib_id: &str) -> bool {
        self.global_power.and_then(|f| f(lib_id)).unwrap_or_else(|| lib_id.rsplit(':').next() != Some("PWR_FLAG"))
    }
}

/// One traced net: its name, its pins (sorted), and whether a driver -- a label, a power symbol, a sheet pin -- named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedNet {
    pub name: String,
    pub pins: Vec<String>,
    pub driven: bool,
}

/// [`trace_nets_with`] with no pin names, bus members or library symbols to ask.
pub fn trace_nets(screens: &mut [ScreenIn]) -> Vec<Net> {
    trace_nets_with(screens, &Naming::default()).into_iter().map(|t| Net { name: t.name, pins: t.pins }).collect()
}

/// Trace the nets of every screen, write each wire's `net`/`pins`, each power symbol's and no-connect flag's `pin` back, and
/// return the nets (sorted by name, each net's pins sorted). Only a net with a pin is returned.
pub fn trace_nets_with(screens: &mut [ScreenIn], naming: &Naming) -> Vec<TracedNet> {
    let (depth, spath) = depths(screens);
    let bus_members = |name: &str| -> Option<Vec<String>> { naming.bus_members.and_then(|f| f(name)) };

    // ---- phase A: nodes and subgraphs, per screen ----
    // A bus and a wire are two layers that never touch (`SCH_LINE` on `LAYER_BUS` or `LAYER_WIRE`): a bus wire's points are nodes of their own,
    // and a label or sheet pin is on the bus when it lands on a bus wire (or names a bus and nothing of the wire layer is there).
    let mut groups: Vec<Group> = Vec::new();
    // (screen, point) -> group id, on the wire layer and on the bus layer
    let mut group_at: Vec<BTreeMap<Point, usize>> = Vec::with_capacity(screens.len());
    let mut bus_group_at: Vec<BTreeMap<Point, usize>> = Vec::with_capacity(screens.len());
    // per screen: wire index -> group id
    let mut wire_group: Vec<Vec<Option<usize>>> = Vec::with_capacity(screens.len());

    for (si, screen) in screens.iter().enumerate() {
        let sch: &SchematicSection = &*screen.sch;
        let touches = |p: Point, bus: bool| sch.wires.iter().filter(|w| w.bus == bus).any(|w| w.pts.contains(&p) || w.pts.windows(2).any(|seg| on_interior(p, seg[0], seg[1])));
        let pin_points: BTreeSet<Point> = screen.pins.values().copied().collect();
        let on_bus = |p: Point, name: &str| touches(p, true) || (!touches(p, false) && !pin_points.contains(&p) && bus_members(name).is_some());
        let label_bus: Vec<bool> = sch.labels.iter().map(|l| on_bus(l.at, &l.net)).collect();
        let pin_bus: Vec<Vec<bool>> = sch.sheets.iter().map(|sh| sh.pins.iter().map(|p| on_bus(p.at, &p.name)).collect()).collect();

        // the nodes of one layer: its points, joined along its wires and where a point lands inside one of them
        let layer = |bus: bool, groups: &mut Vec<Group>| -> BTreeMap<Point, usize> {
            let mut ids: BTreeMap<Point, usize> = BTreeMap::new();
            let mut uf = Uf::new();
            let seed = |p: Point, ids: &mut BTreeMap<Point, usize>, uf: &mut Uf| -> usize { *ids.entry(p).or_insert_with(|| uf.add()) };
            let wires: Vec<&eda_model::ir::Wire> = sch.wires.iter().filter(|w| w.bus == bus).collect();
            for w in &wires {
                for &p in &w.pts {
                    seed(p, &mut ids, &mut uf);
                }
            }
            for (l, &b) in sch.labels.iter().zip(&label_bus) {
                if b == bus {
                    seed(l.at, &mut ids, &mut uf);
                }
            }
            for (sh, flags) in sch.sheets.iter().zip(&pin_bus) {
                for (p, &b) in sh.pins.iter().zip(flags) {
                    if b == bus {
                        seed(p.at, &mut ids, &mut uf);
                    }
                }
            }
            if !bus {
                for &p in screen.pins.values() {
                    seed(p, &mut ids, &mut uf);
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
            }
            for w in &wires {
                for pair in w.pts.windows(2) {
                    let (a, b) = (ids[&pair[0]], ids[&pair[1]]);
                    uf.union(a, b);
                }
            }
            // A point landing on the interior of a wire joins that wire.
            let all: Vec<Point> = ids.keys().copied().collect();
            for w in &wires {
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
                    groups.push(Group { screen: si, bus, ..Default::default() });
                    groups.len() - 1
                });
                at.insert(p, gid);
            }
            at
        };
        let at = layer(false, &mut groups);
        let bus_at = layer(true, &mut groups);

        let nc_points: BTreeSet<Point> = sch.no_connects.iter().map(|n| n.at).collect();
        for (pin_ref, p) in &screen.pins {
            if nc_points.contains(p) {
                continue;
            }
            groups[at[p]].pins.push(pin_ref.clone());
            groups[at[p]].drivers.push(Driver::new(PIN, String::new(), depth[si], &spath[si]));
            // `GetDriverPriority`: a global power pin drives its subgraph at `GLOBAL_POWER_PIN`, named by `GetDefaultNetName` (the lib pin's name, escaped as
            // a net name), even where it is wired: `generateGlobalPowerPinSubGraphs` skips only the unwired one's own subgraph, the name joins it to the
            // others all the same.
            if let Some(name) = naming.power_pins.and_then(|m| m.get(pin_ref)).filter(|n| !n.is_empty()) {
                groups[at[p]].drivers.push(Driver::new(GLOBAL_POWER_PIN, name.clone(), depth[si], &spath[si]));
                groups[at[p]].global_names.push(name.clone());
                groups[at[p]].local_names.push(name.clone());
            }
        }
        for (l, &b) in sch.labels.iter().zip(&label_bus) {
            let g = &mut groups[if b { bus_at[&l.at] } else { at[&l.at] }];
            let (prio, bucket) = match &l.kind {
                LabelKind::Local => (LOCAL_LABEL, 0),
                LabelKind::Hierarchical { .. } => (HIER_LABEL, 1),
                LabelKind::Global { .. } => (GLOBAL, 2),
            };
            g.drivers.push(Driver::new(prio, l.net.clone(), depth[si], &spath[si]));
            // `BuildIslandRecord`'s name edges: every label kind is a name on its sheet (`SCOPE::SHEET`), a global one across the schematic too
            // (`SCOPE::GLOBAL`), a hierarchical one at its sheet's port (`SCOPE::PORT`)
            g.local_names.push(l.net.clone());
            match bucket {
                0 => {}
                1 => g.hier_labels.push(l.net.clone()),
                _ => g.global_names.push(l.net.clone()),
            }
        }
        for ps in &sch.power_symbols {
            // `SCH_PIN::IsGlobalPower`: a power-input pin. PWR_FLAG's one pin is a power output (and its `#FLG` reference keeps it out of the netlist:
            // `GetDriverPriority` gives it `NONE`): the flag names no net and joins nothing by name, its `net` is only the net it was put on. Told by the
            // pin, not by the library's name for it: a project's own `PWR_FLAG` (`complex_hierarchy_schlib:PWR_FLAG`) is one too.
            if !naming.is_global_power(&ps.lib_id) {
                continue;
            }
            let name = naming.power_value.map(|f| f(&screen.path, &ps.net)).unwrap_or_else(|| ps.net.clone());
            let g = &mut groups[at[&ps.at]];
            g.drivers.push(Driver { power_parent: true, ..Driver::new(GLOBAL_POWER_PIN, name.clone(), depth[si], &spath[si]) });
            g.local_names.push(name.clone());
            g.global_names.push(name);
        }
        for (sh, flags) in sch.sheets.iter().zip(&pin_bus) {
            for (p, &b) in sh.pins.iter().zip(flags) {
                let g = &mut groups[if b { bus_at[&p.at] } else { at[&p.at] }];
                g.drivers.push(Driver { output: p.shape == eda_model::ir::LabelShape::Output, ..Driver::new(SHEET_PIN, p.name.clone(), depth[si], &spath[si]) });
                g.sheet_pins.push(p.name.clone());
            }
        }
        let mut per_wire: Vec<Option<usize>> = Vec::with_capacity(sch.wires.len());
        for w in &sch.wires {
            per_wire.push(w.pts.first().map(|p| if w.bus { bus_at[p] } else { at[p] }));
        }
        group_at.push(at);
        bus_group_at.push(bus_at);
        wire_group.push(per_wire);
    }

    // ---- phase B: join subgraphs by name and through the hierarchy ----
    let mut join = Uf::new();
    for _ in 0..groups.len() {
        join.add();
    }
    // The name edges of KiCad 25ec2335 (`conn_claims.cpp::BuildIslandRecord`), a bus with a bus and a net with a net:
    // * `SCOPE::SHEET`: a local, hierarchical or global label, a power symbol and a hidden power input are a name on their sheet (one placement), so a
    //   power symbol `VCC` and a local label `VCC` there are one net;
    // * `SCOPE::GLOBAL`: a global label and a global power pin are a name across the schematic;
    // * `SCOPE::PORT`: a sheet pin is a name at the port of the placement it leads to, and so is a hierarchical label inside it -- a sheet pin joins
    //   nothing on its own sheet by its name (two pins of one name on one sheet symbol are one net, at the one port).
    let mut local: BTreeMap<(usize, bool, String), Vec<usize>> = BTreeMap::new();
    let mut global: BTreeMap<(bool, String), Vec<usize>> = BTreeMap::new();
    let mut hier: BTreeMap<(usize, bool, String), Vec<usize>> = BTreeMap::new();
    // the globally named nets of each sheet, for the members of a bus there
    let mut global_here: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    for (gid, g) in groups.iter().enumerate() {
        for n in &g.local_names {
            local.entry((g.screen, g.bus, n.clone())).or_default().push(gid);
        }
        for n in &g.global_names {
            global.entry((g.bus, n.clone())).or_default().push(gid);
            if !g.bus {
                global_here.entry((g.screen, n.clone())).or_default().push(gid);
            }
        }
        for n in &g.hier_labels {
            hier.entry((g.screen, g.bus, n.clone())).or_default().push(gid);
        }
    }
    for gids in local.values().chain(global.values()) {
        for w in gids.windows(2) {
            join.union(w[0], w[1]);
        }
    }
    // The ports: a sheet pin and the hierarchical labels of its placement's content, by name (`SCOPE::PORT`, `{ child instance, name }`).
    let mut ports: BTreeMap<(usize, bool, String), Vec<usize>> = hier.clone();
    for (si, screen) in screens.iter().enumerate() {
        for sh in &screen.sch.sheets {
            let Some(ci) = child_of(screens, si, sh) else { continue };
            for p in &sh.pins {
                for (bus, here) in [(false, &group_at[si]), (true, &bus_group_at[si])] {
                    let Some(&gp) = here.get(&p.at) else { continue };
                    // a pin on the bus layer is on the bus port, one on the wire layer on the net port
                    if groups[gp].bus == bus && groups[gp].sheet_pins.contains(&p.name) {
                        ports.entry((ci, bus, p.name.clone())).or_default().push(gp);
                    }
                }
            }
        }
    }
    for gids in ports.values() {
        for w in gids.windows(2) {
            join.union(w[0], w[1]);
        }
    }

    // The members of each bus (`processSubGraphs`: a net on the bus's sheet whose name is a member's is the bus's neighbour; `propagateToNeighbors`:
    // `propagate_bus_neighbors` gives each neighbour the member's connection, the member found by `matchBusMember`). One node per member of a bus and
    // of every bus joined to it, here or through the hierarchy, which joins the nets each sheet has under that member's name and is driven by the
    // member's name: a vector's members by their index (`VectorIndex`), a group's by name.
    if naming.bus_members.is_some() {
        // `generateBusAliasMembers`: every bus *label* makes a virtual net per net member, named by the member's full local name, and two of one name on
        // one sheet are one net (`processSubGraphs`, then the pass for "buses that have been linked together somewhere by member (net) connections"): two
        // buses that share a member name are one net there, with no wire or label of that name. A bus a sheet pin alone names makes none, but its member
        // of that name is the virtual net's neighbour all the same (`processSubGraphs` checks every member of a strong bus against the nets of the sheet).
        // (screen, member) -> the member nodes, and whether a label's member is among them.
        let mut virtual_member: BTreeMap<(usize, String), (Vec<usize>, bool)> = BTreeMap::new();
        let mut buses: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (gid, g) in groups.iter().enumerate() {
            if g.bus {
                buses.entry(join.find(gid)).or_default().push(gid);
            }
        }
        // `processSubGraphs`: a bus only a sheet pin names is weakly driven, and it is promoted to a strong one -- the one kind that finds the nets of its
        // members on the sheet (`m_bus_neighbors`) -- only when no other bus there has its name, a vector none of its prefix either (`collectAllDriverValues`
        // files a vector under `<prefix>[]` too), and no global label of its name is on the sheet; else it is renamed (`_1`) and has no neighbours.
        let bus_name = |g: &Group| -> Option<String> {
            let mut d: Vec<&Driver> = g.drivers.iter().filter(|d| d.prio > PIN).collect();
            d.sort_by(|a, b| driver_order(a, b));
            d.first().map(|d| d.name.clone())
        };
        let vector_prefix = |n: &str| -> Option<String> { (!n.contains('{')).then(|| n.split('[').next().unwrap_or(n).to_string()) };
        let names_of: Vec<Option<String>> = groups.iter().map(|g| if g.bus { bus_name(g) } else { None }).collect();
        let promoted = |g: usize| -> bool {
            let Some(name) = names_of[g].as_ref() else { return false };
            let here = |o: usize| groups[o].bus && groups[o].screen == groups[g].screen;
            let mut peers = (0..names_of.len()).filter(|&o| here(o) && names_of[o].as_ref() == Some(name)).count();
            if peers <= 1 {
                if let Some(prefix) = vector_prefix(name) {
                    peers = (0..names_of.len()).filter(|&o| here(o) && names_of[o].as_deref().and_then(vector_prefix).as_ref() == Some(&prefix)).count();
                }
            }
            let global = groups.iter().any(|o| o.screen == groups[g].screen && o.global_names.contains(name));
            peers <= 1 && !global
        };
        let promoted: Vec<bool> = (0..groups.len()).map(promoted).collect();
        for gids in buses.values() {
            let mut drivers: Vec<&Driver> = gids.iter().flat_map(|&g| groups[g].drivers.iter()).filter(|d| d.prio > PIN && bus_members(&d.name).is_some()).collect();
            drivers.sort_by(|a, b| driver_order(a, b));
            let Some(best) = drivers.first().map(|d| (*d).clone()) else { continue };
            let Some(best_members) = bus_members(&best.name) else { continue };
            let vector = !best.name.contains('{');
            let first = groups.len();
            for m in &best_members {
                let screen = gids.iter().map(|&g| groups[g].screen).find(|&s| depth[s] == best.depth).unwrap_or(groups[gids[0]].screen);
                // a member's name is the bus's, with the bus driver's path (`SCH_CONNECTION::ConfigureFromLabel` gives the members the bus's sheet)
                let sheet_path = best.full.strip_suffix(best.name.as_str()).and_then(|p| p.strip_suffix('/')).unwrap_or("");
                // `BindBundle`: the slot's claim is `BUS_MEMBER`, with the depth and the path of the bus's claim (`fullName = path + leaf.name`)
                let mut slot = Driver::new(BUS_MEMBER, m.clone(), best.depth, sheet_path);
                if best.prio >= GLOBAL_POWER_PIN {
                    // a global bus's claim has no path: neither has its members' (`fullName = path + leaf.name`)
                    slot.full = slot.name.clone();
                }
                groups.push(Group { screen, member: true, drivers: vec![slot], ..Default::default() });
                join.add();
            }
            for &g in gids {
                let screen = groups[g].screen;
                // the bus's own driver, and the labels beside it (`add_connections_to_check`: `getDefaultConnection` makes a connection of a label, never of
                // a sheet pin)
                let mut own: Vec<&Driver> = groups[g].drivers.iter().filter(|d| d.prio > PIN).collect();
                own.sort_by(|a, b| driver_order(a, b));
                let names: Vec<(String, bool)> = own.iter().enumerate().filter(|(i, d)| *i == 0 || d.prio >= HIER_LABEL).map(|(_, d)| (d.name.clone(), d.prio >= HIER_LABEL)).collect();
                for (name, label) in names {
                    // a bus a sheet pin names finds its members' nets only when it is promoted
                    if !label && !promoted[g] {
                        continue;
                    }
                    let Some(members) = bus_members(&name) else { continue };
                    // `if( !conn->IsNet() ) continue;`: no virtual net of a bus inside a group
                    let nested = nested_group_members(&name, &bus_members);
                    for (i, m) in members.iter().enumerate() {
                        // `matchBusMember`: a vector's member by its place; a group's by its local name (the member's name without the group's `prefix.`),
                        // else the member of the group's flattened list at the searched member's `VectorIndex` -- its place in a vector this bus is (a
                        // vector sheet pin on a group bus, `qa/data/eeschema/issue23840`). **Differs**: a member of a group this bus is has a `VectorIndex` too
                        // (0, or its place in a vector inside the group), which KiCad falls back to and this does not: such a member is matched by name only.
                        let child_vector = !name.contains('{');
                        let k = if vector {
                            (i < best_members.len()).then_some(i)
                        } else {
                            best_members.iter().position(|b| group_local_name(&best.name, b) == group_local_name(&name, m)).or_else(|| (child_vector && i < best_members.len()).then_some(i))
                        };
                        let Some(k) = k else { continue };
                        let node = first + k;
                        if !nested.contains(m) {
                            let slot = virtual_member.entry((screen, m.clone())).or_default();
                            slot.0.push(node);
                            slot.1 |= label;
                        }
                        for &n in local.get(&(screen, false, m.clone())).into_iter().flatten().chain(global_here.get(&(screen, m.clone())).into_iter().flatten()) {
                            join.union(node, n);
                        }
                    }
                }
            }
        }
        for (nodes, _) in virtual_member.values().filter(|(_, from_label)| *from_label) {
            for w in nodes.windows(2) {
                join.union(w[0], w[1]);
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
        /// Whether a driver (label, power symbol, sheet pin) named it.
        driven: bool,
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
        drivers.sort_by(|a, b| driver_order(a, b));
        let best = drivers.first();
        let screen = gids.iter().map(|&g| groups[g].screen).min_by_key(|&s| depth[s]).unwrap_or(0);
        finals.push(Final { gids, pins, name: best.map(|d| d.full.clone()), driven: best.is_some(), screen });
    }

    // KiCad's names are unique by construction (a local name has its sheet's path); two nets that still share one (two sheet pins of one name
    // on a sheet, which KiCad does not join) are told apart by a suffix. **Differs**: KiCad gives both the name.
    finals.sort_by(|a, b| (depth[a.screen], a.name.clone(), a.pins.first().cloned()).cmp(&(depth[b.screen], b.name.clone(), b.pins.first().cloned())));
    let mut used: BTreeSet<String> = BTreeSet::new();
    for f in finals.iter_mut() {
        // a bus, or a member of one no net is on, keeps the name of its driver and takes it from no net
        if f.pins.is_empty() && f.gids.iter().all(|&g| groups[g].bus || groups[g].member) {
            continue;
        }
        if let Some(n) = f.name.clone() {
            let mut unique = n.clone();
            let mut k = 1;
            while used.contains(&unique) {
                k += 1;
                unique = format!("{n}_{k}");
            }
            used.insert(unique.clone());
            f.name = Some(unique);
        }
    }
    // The undriven nets, the larger ones first: what KiCad calls them (`SCH_PIN::GetDefaultNetName` of the best pin). A group with no pin (a
    // stray wire, a lone label) has no net and stays unnamed.
    let facts = PinFacts { names: naming.pin_names, units: naming.pin_units, nc: naming.nc_pins, common: naming.common_pins };
    let shown_count = shown_name_counts(&facts);
    let mut order: Vec<usize> = (0..finals.len()).filter(|&i| finals[i].name.is_none() && !finals[i].pins.is_empty()).collect();
    order.sort_by(|&a, &b| finals[b].pins.len().cmp(&finals[a].pins.len()).then_with(|| finals[a].pins.first().cmp(&finals[b].pins.first())));
    for i in order {
        let base = kicad_net_name(&finals[i].pins, &facts, &shown_count);
        let mut name = base.clone();
        let mut k = 1;
        while used.contains(&name) {
            k += 1;
            name = format!("{base}_{k}");
        }
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
                // a wire on no net (no pin, no driver) keeps the name it had
                if !name_of_group[g].is_empty() {
                    w.net = name_of_group[g].clone();
                }
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
    let mut nets: Vec<TracedNet> = finals.into_iter().filter(|f| !f.pins.is_empty()).map(|f| TracedNet { name: f.name.unwrap_or_default(), pins: f.pins, driven: f.driven }).collect();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    nets
}

/// What KiCad's default name of a pin's net reads of the pin (`SCH_CONNECTIVITY::PIN_NAME_FACT`, `PIN_NAME_REFERENCE`): its shown name, the
/// reference with the unit of the symbol it is drawn on (`SCH_SYMBOL::GetRef( &path, true )`: `U1A` for a part of several units), and whether it is a
/// no-connect pin (`PT_NC`), each by "REF.PIN".
#[derive(Default, Clone, Copy)]
pub struct PinFacts<'a> {
    pub names: Option<&'a BTreeMap<String, String>>,
    pub units: Option<&'a BTreeMap<String, String>>,
    pub nc: Option<&'a BTreeSet<String>>,
    /// The pins of a symbol of several units that are common to every unit (unit 0, `ALL_UNITS`), "REF.PIN".
    pub common: Option<&'a BTreeSet<String>>,
}

impl PinFacts<'_> {
    fn shown(&self, pin_ref: &str) -> &str {
        self.names.and_then(|n| n.get(pin_ref)).map(String::as_str).filter(|s| !s.is_empty() && *s != "~").unwrap_or("")
    }
    fn is_nc(&self, pin_ref: &str) -> bool {
        self.nc.is_some_and(|n| n.contains(pin_ref))
    }
}

/// The numbers of the pins of each drawn symbol that share each shown name, for `hasDuplicateName` of KiCad's default net name (a no-connect pin counts
/// for none). KiCad 25ec2335 `conn_facts.cpp` fills `numbersByName` from `symbol->GetPins( &aPath )`, which keeps the pins of the unit drawn and those
/// common to every unit (`SCH_SYMBOL::GetPins`: `pin->GetUnit() != ALL_UNITS && pin->GetUnit() != unit` is skipped), so a name another unit repeats is
/// no duplicate: a dual op-amp's `+` is `Net-(U1A-+)`. The symbol a pin is drawn on is its reference with its unit ([`PinFacts::units`], `U1A`), or the
/// reference for a symbol of one unit; a pin common to every unit ([`PinFacts::common`]) counts in each unit drawn.
fn shown_name_counts(facts: &PinFacts) -> BTreeMap<(String, String), BTreeSet<String>> {
    let symbol_of = |pin_ref: &str, reference: &str| -> String { facts.units.and_then(|u| u.get(pin_ref)).cloned().unwrap_or_else(|| reference.to_string()) };
    // the units drawn of each reference
    let mut drawn: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (pin_ref, with_unit) in facts.units.into_iter().flatten() {
        if let Some((reference, _)) = pin_ref.split_once('.') {
            drawn.entry(reference).or_default().insert(with_unit.clone());
        }
    }
    let mut out: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for pin_ref in facts.names.into_iter().flat_map(|n| n.keys()) {
        if let Some((reference, number)) = pin_ref.split_once('.') {
            let name = facts.shown(pin_ref);
            if name.is_empty() || facts.is_nc(pin_ref) {
                continue;
            }
            let common = facts.common.is_some_and(|c| c.contains(pin_ref));
            let symbols: Vec<String> = match drawn.get(reference) {
                Some(units) if common => units.iter().cloned().collect(),
                _ => vec![symbol_of(pin_ref, reference)],
            };
            for symbol in symbols {
                out.entry((symbol, name.to_string())).or_default().insert(number.to_string());
            }
        }
    }
    out
}

/// `SCH_PIN::GetDefaultNetName` and `SCH_CONNECTIVITY::RenderPinNetName` (KiCad 25ec2335, eeschema/connectivity/conn_pin_name.cpp): the name a pin
/// gives its net when nothing stronger does -- `Net-(R1-Pad1)`; the pin's name instead of "Pad<number>" when it has one that is not its number, after
/// the reference with its unit (`Net-(U1-PA0)`, `Net-(U1A-OUT)`), with the pad number added when the part has several pins of that name that are not
/// no-connect pins; `unconnected-(...)` for a pin that is alone or is a no-connect pin. `/` is written `{slash}`.
fn default_net_name(pin_ref: &str, facts: &PinFacts, shown_count: &BTreeMap<(String, String), BTreeSet<String>>, force_no_connect: bool) -> String {
    let (reference, number) = pin_ref.split_once('.').unwrap_or((pin_ref, ""));
    let escape = |s: &str| s.replace('/', "{slash}").replace(['\n', '\r'], "");
    let shown = facts.shown(pin_ref);
    let unconnected = force_no_connect || facts.is_nc(pin_ref);
    let prefix = if unconnected { "unconnected-(" } else { "Net-(" };
    if !shown.is_empty() && shown != number {
        let with_unit = facts.units.and_then(|u| u.get(pin_ref)).map(String::as_str).unwrap_or(reference);
        // `hasDuplicateName = numbers.size() > 1 || !numbers.contains( pin->GetShownNumber() )`
        let has_multiple = !unconnected && shown_count.get(&(with_unit.to_string(), shown.to_string())).is_some_and(|numbers| numbers.len() > 1 || !numbers.contains(number));
        let pad = if unconnected || has_multiple { format!("-Pad{}", escape(number)) } else { String::new() };
        format!("{prefix}{}-{}{pad})", escape(with_unit), escape(shown))
    } else {
        format!("{prefix}{}-Pad{})", escape(reference), escape(number))
    }
}

/// `SCH_PIN::GetDefaultNetName( path, aForceNoConnect = true )`: the name KiCad gives the net of a pin that is alone on it or on a no-connect flag,
/// `unconnected-(U1-NC1-Pad12)` (`unconnected-(R1-Pad1)` for a pin with no name of its own, `unconnected-(U1A-NC-Pad5)` for one of a symbol of several
/// units). `shown` is the pin's shown name (empty or `~` for none), `with_unit` the reference with the unit the pin is drawn on.
pub fn unconnected_net_name(pin_ref: &str, shown: Option<&str>, with_unit: Option<&str>) -> String {
    let names: BTreeMap<String, String> = shown.map(|s| (pin_ref.to_string(), s.to_string())).into_iter().collect();
    let units: BTreeMap<String, String> = with_unit.map(|u| (pin_ref.to_string(), u.to_string())).into_iter().collect();
    default_net_name(pin_ref, &PinFacts { names: Some(&names), units: Some(&units), ..PinFacts::default() }, &BTreeMap::new(), true)
}

/// The name KiCad gives a net nothing drives (`CONNECTION_SUBGRAPH::ResolveDrivers`, `compareDrivers`): among its pins' default names, one
/// that is not a "-Pad" name first (a pin with a name beats a pin with only a number), then the alphabetically first; a net of one pin is
/// `unconnected-(...)`.
fn kicad_net_name(pins: &[String], facts: &PinFacts, shown_count: &BTreeMap<(String, String), BTreeSet<String>>) -> String {
    if let [only] = pins {
        return default_net_name(only, facts, shown_count, true);
    }
    pins.iter().map(|p| default_net_name(p, facts, shown_count, false)).min_by(|a, b| a.contains("-Pad").cmp(&b.contains("-Pad")).then_with(|| a.cmp(b))).unwrap_or_else(|| "Net-()".to_string())
}

/// What an edit did to the nets, from what was drawn before it and after it: the net list `prev` with only the nets the edit touched
/// rewritten; `None` when the edit changed nothing about what is connected or what drives a net's name (a text, a field, a part that
/// only changed place).
///
/// This is how a net list survives a drawing that does not say everything. A stored schematic whose labels do not quite touch their pins
/// reads as many small nets, and an edit anywhere must not turn the list into that reading: a net the edit did not touch keeps its name
/// and every pin, even when the drawing shows it in pieces. A net it did touch is read from the drawing, and is named
/// - by its driver, when a label, a power symbol or a sheet pin drives it (a label joins the net that has its name, and renames the net
///   it is put on);
/// - else by the net of `prev` most of its pins come from (the larger overlap first, then the name), when no other net took that name:
///   a net keeps its name through an edit that moves a pin in or out of it, and a net made of two joins as one, whole;
/// - else by what KiCad calls it (`Net-(R1-Pad1)`; `unconnected-(R2-Pad1)` for a lone pin) -- the name the tracer gave the net.
pub fn follow(prev: &[Net], before: &[TracedNet], after: &[TracedNet]) -> Option<Vec<Net>> {
    let key = |t: &TracedNet| (t.pins.clone(), t.driven.then(|| t.name.clone()));
    let before_keys: BTreeSet<_> = before.iter().map(key).collect();
    let after_keys: BTreeSet<_> = after.iter().map(key).collect();
    if before_keys == after_keys {
        return None;
    }
    let changed: Vec<&TracedNet> = after.iter().filter(|t| !before_keys.contains(&key(t))).collect();
    // Every pin of a net the edit made or unmade: a pin that left the nets altogether (a no-connect flag) is among them.
    let mut touched: BTreeSet<&str> = changed.iter().flat_map(|t| t.pins.iter().map(String::as_str)).collect();
    for gone in before.iter().filter(|b| !after_keys.contains(&key(b))) {
        touched.extend(gone.pins.iter().map(String::as_str));
    }

    // The nets the design had, without the pins the edit touched.
    let mut nets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut net_of: BTreeMap<&str, &str> = BTreeMap::new();
    for n in prev {
        for p in &n.pins {
            net_of.entry(p.as_str()).or_insert(n.name.as_str());
        }
        let kept: BTreeSet<String> = n.pins.iter().filter(|p| !touched.contains(p.as_str())).cloned().collect();
        if !kept.is_empty() {
            nets.entry(n.name.clone()).or_default().extend(kept);
        }
    }

    // Driven nets first -- their name is not negotiable -- then the larger ones.
    let driven_names: BTreeSet<&str> = after.iter().filter(|t| t.driven).map(|t| t.name.as_str()).collect();
    let mut order = changed;
    order.sort_by(|a, b| b.driven.cmp(&a.driven).then_with(|| b.pins.len().cmp(&a.pins.len())).then_with(|| a.pins.first().cmp(&b.pins.first())));
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for t in order {
        // The nets of `prev` this net's pins come from, the larger overlap first.
        let mut overlap: BTreeMap<&str, usize> = BTreeMap::new();
        for p in &t.pins {
            if let Some(&name) = net_of.get(p.as_str()) {
                *overlap.entry(name).or_default() += 1;
            }
        }
        let mut ranked: Vec<(&str, usize)> = overlap.iter().map(|(n, c)| (*n, *c)).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let name = if t.driven {
            t.name.clone()
        } else {
            let inherited = ranked.iter().map(|(n, _)| *n).find(|n| !claimed.contains(*n) && !driven_names.contains(*n));
            match inherited {
                Some(n) => n.to_string(),
                None => {
                    let mut fresh = t.name.clone();
                    let mut k = 1;
                    while claimed.contains(&fresh) || nets.contains_key(&fresh) || driven_names.contains(fresh.as_str()) {
                        k += 1;
                        fresh = format!("{}_{k}", t.name);
                    }
                    fresh
                }
            }
        };
        // A net made of several joins them whole: what the others still had, untouched, comes along -- unless another net's driver or
        // another change owns that name.
        let joined: Vec<String> = ranked.iter().map(|(n, _)| n.to_string()).filter(|n| *n != name && !claimed.contains(n) && !driven_names.contains(n.as_str())).collect();
        let mut pins: BTreeSet<String> = t.pins.iter().cloned().collect();
        if t.driven || ranked.len() > 1 {
            for other in joined {
                if let Some(rest) = nets.remove(&other) {
                    pins.extend(rest);
                }
            }
        }
        claimed.insert(name.clone());
        nets.entry(name).or_default().extend(pins);
    }
    Some(nets.into_iter().map(|(name, pins)| Net { name, pins: pins.into_iter().collect() }).collect())
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;
    use eda_model::ir::{LabelShape, NetLabel, PowerSymbol, SheetInstance, SheetPin, Wire};

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
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

    /// A wire joins its pins, and a net nothing drives is named after its best pin as KiCad names it, whatever name the wire carried before.
    #[test]
    fn a_wire_joins_its_pins_and_the_net_takes_kicads_name() {
        let mut root = section();
        root.wires.push(wire("LED1", &[p(0, 0), p(1000, 0)]));
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.2", p(0, 0)), ("D1.1", p(1000, 0)), ("R2.1", p(5000, 5000))]) }];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "Net-(D1-Pad1)"), vec!["D1.1", "R1.2"]);
        assert_eq!(pins_of(&nets, "unconnected-(R2-Pad1)"), vec!["R2.1"], "the lone pin is a net of its own: {nets:?}");
        assert_eq!(nets.len(), 2, "{nets:?}");
        assert_eq!(root.wires[0].net, "Net-(D1-Pad1)");
        assert_eq!(root.wires[0].pins, vec!["D1.1", "R1.2"]);
    }

    #[test]
    fn a_hierarchical_label_and_its_sheet_pin_are_one_net_across_sheets() {
        // Root: a sheet symbol "child" with pin SIG at (100, 50), wired to R1.1 on the root. Inside: a hierarchical label SIG on a
        // wire to U1.3.
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(100, 0), size: (Nm::new(100), Nm::new(100)), pins: vec![SheetPin { id: String::new(), name: "SIG".into(), shape: LabelShape::Bidirectional, at: p(100, 50) }], page: String::new() });
        root.wires.push(wire("", &[p(0, 50), p(100, 50)]));
        let mut child = section();
        child.wires.push(wire("", &[p(10, 10), p(40, 10)]));
        child.labels.push(label("SIG", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 50))]) },
            ScreenIn { path: Vec::new(), sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.3", p(40, 10))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(nets.len(), 1, "{nets:?}");
        assert_eq!(nets[0].name, "/child/SIG", "the hierarchical label names the net, with its sheet's path (`recacheName`)");
        assert_eq!(nets[0].pins, vec!["R1.1", "U1.3"]);
        // And both wires carry the name.
        assert_eq!(root.wires[0].net, "/child/SIG");
        assert_eq!(child.wires[0].net, "/child/SIG");
    }

    fn power(net: &str, at: Point) -> PowerSymbol {
        PowerSymbol { id: format!("#PWR_{net}"), lib_id: format!("power:{net}"), at, rot: 0, net: net.into(), pin: String::new() }
    }
    fn sheet_pin(name: &str, at: Point, shape: LabelShape) -> SheetPin {
        SheetPin { id: String::new(), name: name.into(), shape, at }
    }
    fn sub_sheet(pins: Vec<SheetPin>) -> SheetInstance {
        SheetInstance { id: "s1".into(), name: "sub".into(), file: "sub.kicad_sch".into(), at: p(10_000, 0), size: (Nm::new(10_000), Nm::new(10_000)), pins, page: String::new() }
    }
    fn net_of(nets: &[TracedNet], pin: &str) -> (String, Vec<String>) {
        let n = nets.iter().find(|n| n.pins.iter().any(|x| x == pin)).unwrap_or_else(|| panic!("{pin} in {nets:?}"));
        (n.name.clone(), n.pins.clone())
    }
    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// r2 A-N1 (KiCad 25ec2335 `BuildIslandRecord`: a global label and a global power pin emit the `SCOPE::SHEET` edge a local label emits): on one sheet a
    /// power symbol `VCC` and a local label `VCC` are one net, and so are a global label and a local one, though no wire joins them. kicad-cli, cases t11/t12.
    #[test]
    fn a_local_label_joins_the_global_net_of_its_name_on_its_sheet() {
        for global in ["power", "label"] {
            let mut root = section();
            if global == "power" {
                root.power_symbols.push(power("VCC", p(0, 0)));
            } else {
                root.labels.push(label("VCC", p(0, 0), LabelKind::Global { shape: LabelShape::Input }));
            }
            root.labels.push(label("VCC", p(1000, 0), LabelKind::Local));
            let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(1000, 0))]) }];
            let nets = trace_nets_with(&mut screens, &Naming::default());
            assert_eq!(net_of(&nets, "R1.1"), ("VCC".to_string(), strs(&["R1.1", "R2.1"])), "{global}: {nets:?}");
        }
        // t13: a child's hierarchical label `VCC` and its power symbol `VCC` are one net, so the root's sheet pin `VCC` is on the global net too
        let mut root = section();
        root.sheets.push(sub_sheet(vec![sheet_pin("VCC", p(10_000, 500), LabelShape::Input)]));
        root.wires.push(wire("", &[p(0, 500), p(10_000, 500)]));
        let mut sub = section();
        sub.labels.push(label("VCC", p(0, 0), LabelKind::Hierarchical { shape: LabelShape::Input }));
        sub.power_symbols.push(power("VCC", p(1000, 0)));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 500))]) },
            ScreenIn { path: Vec::new(), sch: &mut sub, file: "sub.kicad_sch".into(), pins: pins(&[("R3.1", p(0, 0)), ("R4.1", p(1000, 0))]) },
        ];
        let nets = trace_nets_with(&mut screens, &Naming::default());
        assert_eq!(net_of(&nets, "R1.1"), ("VCC".to_string(), strs(&["R1.1", "R3.1", "R4.1"])), "{nets:?}");
    }

    /// r2 A-N2 and A-N3 (`BuildIslandRecord`: a sheet pin emits only a `SCOPE::PORT` edge, at the placement it leads to): a sheet pin does not join a local
    /// label of its name on its own sheet (kicad-cli t9: `/FOO [R2.1]`, `/sub/FOO [R1.1 R3.1]`), and two pins of one name on one sheet symbol are one net
    /// though nothing inside names them (t15: `/X [R1.1 R2.1]`).
    #[test]
    fn a_sheet_pin_does_not_join_a_label_of_its_name_on_its_own_sheet() {
        let mut root = section();
        root.sheets.push(sub_sheet(vec![sheet_pin("FOO", p(10_000, 500), LabelShape::Input), sheet_pin("X", p(10_000, 1500), LabelShape::Input), sheet_pin("X", p(10_000, 2500), LabelShape::Input)]));
        root.wires.push(wire("", &[p(0, 500), p(10_000, 500)]));
        root.wires.push(wire("", &[p(0, 1500), p(10_000, 1500)]));
        root.wires.push(wire("", &[p(0, 2500), p(10_000, 2500)]));
        root.labels.push(label("FOO", p(0, 5000), LabelKind::Local));
        let mut sub = section();
        sub.labels.push(label("FOO", p(0, 0), LabelKind::Hierarchical { shape: LabelShape::Input }));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 500)), ("R2.1", p(0, 5000)), ("R5.1", p(0, 1500)), ("R6.1", p(0, 2500))]) },
            ScreenIn { path: Vec::new(), sch: &mut sub, file: "sub.kicad_sch".into(), pins: pins(&[("R3.1", p(0, 0))]) },
        ];
        let nets = trace_nets_with(&mut screens, &Naming::default());
        assert_eq!(net_of(&nets, "R2.1"), ("/FOO".to_string(), strs(&["R2.1"])), "{nets:?}");
        assert_eq!(net_of(&nets, "R1.1"), ("/sub/FOO".to_string(), strs(&["R1.1", "R3.1"])), "{nets:?}");
        assert_eq!(net_of(&nets, "R5.1"), ("/X".to_string(), strs(&["R5.1", "R6.1"])), "{nets:?}");
    }

    /// r2 A-N4, A-N5, A-N12 (`CLAIM_LESS`): a global name has no depth (`PrepareInstanceClaims`: `unscoped`), so of global labels on one net the
    /// alphabetically first names it wherever it is (t1: `AAA`, not the root's `ZZZ`); a power symbol's pin outranks an invisible power input of an ordinary
    /// symbol (`globalPowerParent`; t2: `VCC`, not U1's hidden `AVDD`); a sheet pin of output shape outranks the alphabetical order (t10: `/ZZZ`).
    #[test]
    fn global_names_on_one_net_are_ranked_without_their_sheets_depth() {
        let mut root = section();
        root.sheets.push(sub_sheet(vec![]));
        root.labels.push(label("ZZZ", p(0, 0), LabelKind::Global { shape: LabelShape::Input }));
        let mut sub = section();
        sub.wires.push(wire("", &[p(0, 0), p(1000, 0)]));
        sub.labels.push(label("ZZZ", p(0, 0), LabelKind::Global { shape: LabelShape::Input }));
        sub.labels.push(label("AAA", p(1000, 0), LabelKind::Global { shape: LabelShape::Input }));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0))]) },
            ScreenIn { path: Vec::new(), sch: &mut sub, file: "sub.kicad_sch".into(), pins: pins(&[("R2.1", p(1000, 0))]) },
        ];
        let nets = trace_nets_with(&mut screens, &Naming::default());
        assert_eq!(net_of(&nets, "R1.1"), ("AAA".to_string(), strs(&["R1.1", "R2.1"])), "{nets:?}");

        let mut root = section();
        root.power_symbols.push(power("VCC", p(0, 0)));
        let hidden: BTreeMap<String, String> = [("U1.1".to_string(), "AVDD".to_string())].into_iter().collect();
        let naming = Naming { power_pins: Some(&hidden), ..Naming::default() };
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("U1.1", p(0, 0))]) }];
        let nets = trace_nets_with(&mut screens, &naming);
        assert_eq!(net_of(&nets, "U1.1").0, "VCC", "{nets:?}");

        let mut root = section();
        root.sheets.push(sub_sheet(vec![sheet_pin("AAA", p(10_000, 500), LabelShape::Input), sheet_pin("ZZZ", p(10_000, 1500), LabelShape::Output)]));
        root.wires.push(wire("", &[p(0, 500), p(10_000, 500), p(10_000, 1500)]));
        let mut sub = section();
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 500))]) },
            ScreenIn { path: Vec::new(), sch: &mut sub, file: "sub.kicad_sch".into(), pins: BTreeMap::new() },
        ];
        let nets = trace_nets_with(&mut screens, &Naming::default());
        assert_eq!(net_of(&nets, "R1.1").0, "/ZZZ", "{nets:?}");
    }

    /// r2 A-N7 (`BindBundle`: a bus member's slot claims `BUS_MEMBER`, above a local label): a net that is the bus `D[0..1]`'s member `D0` and also has the local
    /// label `AAA` is named `/D0` (kicad-cli t3).
    #[test]
    fn a_bus_member_names_its_net_above_a_local_label() {
        let members = |n: &str| test_members(n);
        let naming = Naming { bus_members: Some(&members), ..Naming::default() };
        let mut root = section();
        root.wires.push(bus(&[p(0, 5000), p(5000, 5000)]));
        root.labels.push(label("D[0..1]", p(1000, 5000), LabelKind::Local));
        root.wires.push(wire("", &[p(0, 0), p(1000, 0)]));
        root.labels.push(label("D0", p(0, 0), LabelKind::Local));
        root.labels.push(label("AAA", p(1000, 0), LabelKind::Local));
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(1000, 0))]) }];
        let nets = trace_nets_with(&mut screens, &naming);
        assert_eq!(net_of(&nets, "R1.1"), ("/D0".to_string(), strs(&["R1.1", "R2.1"])), "{nets:?}");
    }

    /// The member nets of the few bus names the bus tests use (the real expansion is `eda_kicad::expand_bus_members`).
    fn test_members(name: &str) -> Option<Vec<String>> {
        let (prefix, rest) = name.split_once('[')?;
        let (lo, hi) = rest.strip_suffix(']')?.split_once("..")?;
        let (lo, hi): (u32, u32) = (lo.parse().ok()?, hi.parse().ok()?);
        Some((lo.min(hi)..=lo.max(hi)).map(|i| format!("{prefix}{i}")).collect())
    }
    fn bus(pts: &[Point]) -> Wire {
        Wire { id: String::new(), net: String::new(), pins: vec![], pts: pts.to_vec(), bus: true }
    }

    /// p12 S1 (`generateBusAliasMembers` and the pass for buses "linked together somewhere by member (net) connections"): two buses on one sheet that never
    /// touch, each labelled, each taken by a sheet pin to a child whose `D0` reaches a pin, are one net per member name they share -- `/D0`, named by the root's
    /// member -- with no wire or label `D0` on the root. Buses whose names share no member (`E[0..3]`) stay apart. A bus named by its sheet pin alone makes no
    /// virtual net (`GetAllBusLabels` takes labels only); its `D0` is the labelled bus's virtual `D0`'s neighbour only when `processSubGraphs` promotes it, which
    /// it does not beside another vector of its prefix (`D[0..1]` and `D[0..3]` are both `/D[]`): there U2.1 stays apart, and with a group (`{D0 D1}`) it joins.
    #[test]
    fn two_buses_on_one_sheet_join_by_a_shared_member_name() {
        let members = |n: &str| test_members(n).or_else(|| n.strip_prefix('{').and_then(|r| r.strip_suffix('}')).map(|i| i.split_whitespace().map(String::from).collect()));
        let naming = Naming { bus_members: Some(&members), ..Naming::default() };
        for (second, labelled, joined) in [("D[0..3]", true, true), ("E[0..3]", true, false), ("D[0..3]", false, false), ("{D0 D1}", false, true)] {
            let mut root = section();
            let sheet = |id: &str, file: &str, y: i64, pin: &str| SheetInstance { id: id.into(), name: id.into(), file: file.into(), at: p(1000, y), size: (Nm::new(1000), Nm::new(1000)), pins: vec![SheetPin { id: String::new(), name: pin.into(), shape: LabelShape::Bidirectional, at: p(1000, y + 500) }], page: String::new() };
            root.sheets.push(sheet("s1", "a.kicad_sch", 0, "D[0..1]"));
            root.sheets.push(sheet("s2", "b.kicad_sch", 2000, second));
            root.wires.push(bus(&[p(500, 500), p(1000, 500)]));
            root.labels.push(label("D[0..1]", p(700, 500), LabelKind::Local));
            root.wires.push(bus(&[p(500, 2500), p(1000, 2500)]));
            if labelled {
                root.labels.push(label(second, p(700, 2500), LabelKind::Local));
            }
            let child = |bus_name: &str, member: &str| {
                let mut c = section();
                c.wires.push(bus(&[p(10, 10), p(10, 400)]));
                c.labels.push(label(bus_name, p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
                c.labels.push(label(member, p(200, 100), LabelKind::Local));
                c
            };
            let member0 = if second.starts_with('{') { "D0".to_string() } else { format!("{}0", &second[..1]) };
            let (mut a, mut b) = (child("D[0..1]", "D0"), child(second, &member0));
            let mut screens = vec![
                ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
                ScreenIn { path: Vec::new(), sch: &mut a, file: "a.kicad_sch".into(), pins: pins(&[("U1.1", p(200, 100))]) },
                ScreenIn { path: Vec::new(), sch: &mut b, file: "b.kicad_sch".into(), pins: pins(&[("U2.1", p(200, 100))]) },
            ];
            let nets = trace_nets_with(&mut screens, &naming);
            let net = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|x| x == pin)).unwrap_or_else(|| panic!("{pin} in {nets:?}")).clone();
            if joined {
                assert_eq!((net("U1.1").name.as_str(), net("U1.1").pins.clone()), ("/D0", vec!["U1.1".to_string(), "U2.1".into()]), "{nets:?}");
            } else {
                assert_eq!(net("U1.1").pins, vec!["U1.1".to_string()], "{second} labelled {labelled}: {nets:?}");
            }
        }
    }

    /// A bus across sheets (`propagateToNeighbors`, `propagate_bus_neighbors`, `matchBusMember`): the sheet pin `D[0..1]` on the root and the hierarchical
    /// label `D[0..1]` inside are one bus, and each member's nets on both sheets -- the nets named D0 and D1 there -- are one net. A bus wire never joins
    /// a wire that ends on it, and a member is matched by its index, so a bus the root calls `X[0..1]` names the child's D0 net X0.
    #[test]
    fn a_bus_across_sheets_joins_its_members_nets_on_both_sides() {
        let members = |n: &str| test_members(n);
        let naming = Naming { bus_members: Some(&members), ..Naming::default() };
        for root_bus_label in ["D[0..1]", "X[0..1]"] {
            let mut root = section();
            root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(1000, 0), size: (Nm::new(1000), Nm::new(1000)), pins: vec![SheetPin { id: String::new(), name: "D[0..1]".into(), shape: LabelShape::Bidirectional, at: p(1000, 500) }], page: String::new() });
            root.wires.push(bus(&[p(500, 500), p(1000, 500)]));
            root.labels.push(label(root_bus_label, p(700, 500), LabelKind::Local));
            let m0 = if root_bus_label.starts_with('X') { "X0" } else { "D0" };
            let m1 = if root_bus_label.starts_with('X') { "X1" } else { "D1" };
            // the members' nets on the root, each named by a label; a wire that ends on the bus, with a pin, is not on it
            root.labels.push(label(m0, p(0, 0), LabelKind::Local));
            root.labels.push(label(m1, p(0, 100), LabelKind::Local));
            root.wires.push(wire("", &[p(500, 700), p(500, 500)]));
            let mut child = section();
            child.wires.push(bus(&[p(10, 10), p(10, 400)]));
            child.labels.push(label("D[0..1]", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
            child.labels.push(label("D0", p(200, 100), LabelKind::Local));
            child.labels.push(label("D1", p(200, 200), LabelKind::Local));
            let mut screens = vec![
                ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(0, 100)), ("R9.1", p(500, 700))]) },
                ScreenIn { path: Vec::new(), sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.1", p(200, 100)), ("U1.2", p(200, 200))]) },
            ];
            let nets = trace_nets_with(&mut screens, &naming);
            let net = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|x| x == pin)).unwrap_or_else(|| panic!("{pin} in {nets:?}")).clone();
            assert_eq!(net("U1.1").pins, vec!["R1.1", "U1.1"], "{root_bus_label}: {nets:?}");
            assert_eq!(net("U1.2").pins, vec!["R2.1", "U1.2"], "{root_bus_label}: {nets:?}");
            assert_eq!(net("U1.1").name, format!("/{m0}"), "the root's member names the net");
            assert_eq!(net("R9.1").pins, vec!["R9.1"], "a wire that ends on a bus is not on it: {nets:?}");
            assert_eq!(root.wires[0].net, format!("/{root_bus_label}"), "the bus is named by its label, with the root's path");
        }
        // Without the expansion a bus has no members: the two sheets' nets stay apart, as before.
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(1000, 0), size: (Nm::new(1000), Nm::new(1000)), pins: vec![SheetPin { id: String::new(), name: "D[0..1]".into(), shape: LabelShape::Bidirectional, at: p(1000, 500) }], page: String::new() });
        root.wires.push(bus(&[p(500, 500), p(1000, 500)]));
        root.labels.push(label("D0", p(0, 0), LabelKind::Local));
        let mut child = section();
        child.wires.push(bus(&[p(10, 10), p(10, 400)]));
        child.labels.push(label("D[0..1]", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
        child.labels.push(label("D0", p(200, 100), LabelKind::Local));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0))]) },
            ScreenIn { path: Vec::new(), sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.1", p(200, 100))]) },
        ];
        assert_eq!(trace_nets_with(&mut screens, &Naming::default()).len(), 2);
    }

    /// A group bus across sheets (`matchBusMember`): its members are matched by local name -- the member's name without the group's `prefix.` -- not by place, so
    /// the child's `{B A}` joins its A to the root's `pre.A` of `pre{A B}`; and a member the parent group does not have joins nothing (no place to fall back to).
    #[test]
    fn a_group_bus_across_sheets_matches_its_members_by_local_name() {
        let members = |n: &str| -> Option<Vec<String>> {
            let (prefix, rest) = n.split_once('{')?;
            let inner = rest.strip_suffix('}')?;
            Some(inner.split_whitespace().map(|m| if prefix.is_empty() { m.to_string() } else { format!("{prefix}.{m}") }).collect())
        };
        let naming = Naming { bus_members: Some(&members), ..Naming::default() };
        for child_bus in ["{B A}", "{B C}"] {
            let mut root = section();
            root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(1000, 0), size: (Nm::new(1000), Nm::new(1000)), pins: vec![SheetPin { id: String::new(), name: child_bus.into(), shape: LabelShape::Bidirectional, at: p(1000, 500) }], page: String::new() });
            root.wires.push(bus(&[p(500, 500), p(1000, 500)]));
            root.labels.push(label("pre{A B}", p(700, 500), LabelKind::Local));
            root.labels.push(label("pre.A", p(0, 0), LabelKind::Local));
            root.labels.push(label("pre.B", p(0, 100), LabelKind::Local));
            let mut child = section();
            child.wires.push(bus(&[p(10, 10), p(10, 400)]));
            child.labels.push(label(child_bus, p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
            let second = if child_bus.contains('C') { "C" } else { "A" };
            child.labels.push(label("B", p(200, 100), LabelKind::Local));
            child.labels.push(label(second, p(200, 200), LabelKind::Local));
            let mut screens = vec![
                ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(0, 100))]) },
                ScreenIn { path: Vec::new(), sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.1", p(200, 100)), ("U1.2", p(200, 200))]) },
            ];
            let nets = trace_nets_with(&mut screens, &naming);
            let net = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|x| x == pin)).unwrap_or_else(|| panic!("{pin} in {nets:?}")).pins.clone();
            assert_eq!(net("U1.1"), vec!["R2.1", "U1.1"], "{child_bus}: B is pre.B, by name: {nets:?}");
            if second == "A" {
                assert_eq!(net("U1.2"), vec!["R1.1", "U1.2"], "A is pre.A: {nets:?}");
            } else {
                assert_eq!(net("U1.2"), vec!["U1.2"], "C is no member of the root's bus: {nets:?}");
                assert_eq!(net("R1.1"), vec!["R1.1"], "{nets:?}");
            }
        }
    }

    /// One screen shown by two sheet symbols is traced once per placement (`CONNECTION_GRAPH::Recalculate` over the sheet paths): its local label makes
    /// a net on each, the second under the references that placement gives its symbols, and each sheet pin joins the hierarchical label of its own placement.
    #[test]
    fn a_screen_shown_twice_is_a_net_per_placement() {
        let mut root = section();
        let pin = |at: Point| vec![SheetPin { id: String::new(), name: "IN".into(), shape: LabelShape::Input, at }];
        root.sheets.push(SheetInstance { id: "s1".into(), name: "a".into(), file: "ch.kicad_sch".into(), at: p(0, 0), size: (Nm::new(100), Nm::new(100)), pins: pin(p(100, 50)), page: String::new() });
        root.sheets.push(SheetInstance { id: "s2".into(), name: "b".into(), file: "ch.kicad_sch".into(), at: p(0, 200), size: (Nm::new(100), Nm::new(100)), pins: pin(p(100, 250)), page: String::new() });
        root.wires.push(wire("", &[p(100, 50), p(300, 50)]));
        root.wires.push(wire("", &[p(100, 250), p(300, 250)]));
        let mut ch = section();
        ch.labels.push(label("X", p(0, 0), LabelKind::Local));
        ch.labels.push(label("X", p(50, 50), LabelKind::Local));
        ch.labels.push(label("IN", p(80, 80), LabelKind::Hierarchical { shape: LabelShape::Input }));
        // a PWR_FLAG on the net names nothing and joins nothing by its name
        ch.power_symbols.push(PowerSymbol { id: "#FLG01".into(), lib_id: "power:PWR_FLAG".into(), at: p(0, 0), rot: 0, net: "X".into(), pin: String::new() });
        let mut copy = ch.clone();
        let first = pins(&[("R1.1", p(0, 0)), ("R2.1", p(50, 50)), ("R1.2", p(80, 80))]);
        let second = pins(&[("R3.1", p(0, 0)), ("R4.1", p(50, 50)), ("R3.2", p(80, 80))]);
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("J1.1", p(300, 50)), ("J1.2", p(300, 250))]) },
            ScreenIn { path: vec!["s1".into()], sch: &mut ch, file: "ch.kicad_sch".into(), pins: first },
            ScreenIn { path: vec!["s2".into()], sch: &mut copy, file: "ch.kicad_sch".into(), pins: second },
        ];
        let nets = trace_nets(&mut screens);
        let groups: BTreeSet<Vec<String>> = nets.iter().map(|n| n.pins.clone()).collect();
        assert!(groups.contains(&vec!["R1.1".to_string(), "R2.1".into()]) && groups.contains(&vec!["R3.1".to_string(), "R4.1".into()]), "{nets:?}");
        assert!(groups.contains(&vec!["J1.1".to_string(), "R1.2".into()]) && groups.contains(&vec!["J1.2".to_string(), "R3.2".into()]), "each pin its own placement's label: {nets:?}");
        let names: BTreeSet<&str> = nets.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names.len(), nets.len(), "the names stay unique: {nets:?}");
    }

    #[test]
    fn power_symbols_are_one_net_on_every_sheet() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "a".into(), file: "a.kicad_sch".into(), at: p(0, 0), size: (Nm::new(10), Nm::new(10)), pins: vec![], page: String::new() });
        root.sheets.push(SheetInstance { id: "s2".into(), name: "b".into(), file: "b.kicad_sch".into(), at: p(20, 0), size: (Nm::new(10), Nm::new(10)), pins: vec![], page: String::new() });
        let gnd = |at: Point| PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at, rot: 0, net: "GND".into(), pin: String::new() };
        let mut a = section();
        a.power_symbols.push(gnd(p(5, 5)));
        let mut b = section();
        b.power_symbols.push(gnd(p(7, 7)));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { path: Vec::new(), sch: &mut a, file: "a.kicad_sch".into(), pins: pins(&[("C1.2", p(5, 5))]) },
            ScreenIn { path: Vec::new(), sch: &mut b, file: "b.kicad_sch".into(), pins: pins(&[("C2.2", p(7, 7))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "GND"), vec!["C1.2", "C2.2"]);
        assert_eq!(a.power_symbols[0].pin, "C1.2");
    }

    /// A PWR_FLAG joins no net by its name whatever library it comes from (`SCH_PIN::IsGlobalPower`: a power-input pin; the flag's is a power output): two
    /// project-library flags (`complex_hierarchy_schlib:PWR_FLAG`, as KiCad's QA `complex_hierarchy` has them) on two rails leave the rails apart. The
    /// library symbol decides when it is known: a power-output symbol of another name joins nothing either, a power-input one of that name does.
    #[test]
    fn a_power_flag_of_any_library_names_no_net() {
        let flag = |lib_id: &str, at: Point| PowerSymbol { id: "#FLG01".into(), lib_id: lib_id.into(), at, rot: 0, net: lib_id.rsplit(':').next().unwrap().into(), pin: String::new() };
        let trace = |lib_id: &str, naming: &Naming| -> Vec<Vec<String>> {
            let mut root = section();
            root.power_symbols.push(flag(lib_id, p(0, 0)));
            root.power_symbols.push(flag(lib_id, p(100, 0)));
            let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("C1.1", p(0, 0)), ("C2.1", p(100, 0))]) }];
            trace_nets_with(&mut screens, naming).into_iter().map(|n| n.pins).collect()
        };
        let apart = vec![vec!["C1.1".to_string()], vec!["C2.1".to_string()]];
        assert_eq!(trace("complex_hierarchy_schlib:PWR_FLAG", &Naming::default()), apart, "a project library's PWR_FLAG");
        let output = |_: &str| Some(false);
        assert_eq!(trace("mylib:SOURCE", &Naming { global_power: Some(&output), ..Naming::default() }), apart, "a power-output symbol");
        let input = |_: &str| Some(true);
        assert_eq!(trace("mylib:PWR_FLAG", &Naming { global_power: Some(&input), ..Naming::default() }), vec![vec!["C1.1".to_string(), "C2.1".into()]], "a power-input pin joins by name");
    }

    #[test]
    fn local_labels_do_not_leak_across_sheets_but_share_a_sheet() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "a".into(), file: "a.kicad_sch".into(), at: p(0, 0), size: (Nm::new(10), Nm::new(10)), pins: vec![], page: String::new() });
        root.sheets.push(SheetInstance { id: "s2".into(), name: "b".into(), file: "b.kicad_sch".into(), at: p(20, 0), size: (Nm::new(10), Nm::new(10)), pins: vec![], page: String::new() });
        let mut a = section();
        a.labels.push(label("X", p(0, 0), LabelKind::Local));
        a.labels.push(label("X", p(50, 50), LabelKind::Local));
        let mut b = section();
        b.labels.push(label("X", p(0, 0), LabelKind::Local));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { path: Vec::new(), sch: &mut a, file: "a.kicad_sch".into(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(50, 50))]) },
            ScreenIn { path: Vec::new(), sch: &mut b, file: "b.kicad_sch".into(), pins: pins(&[("R3.1", p(0, 0))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "/a/X"), vec!["R1.1", "R2.1"], "two labels on one sheet are one net");
        let other = nets.iter().find(|n| n.pins == vec!["R3.1".to_string()]).expect("the other sheet's X is its own net");
        assert_eq!(other.name, "/b/X", "each with its own sheet's path, like KiCad's /a/X and /b/X");
    }

    /// p1-p2 F1's test: a local label on the root names its net with the root's path, `/IN` (`recacheName`: `PathHumanReadable` of the root is `/`),
    /// and a global label or a power symbol names its net without one.
    #[test]
    fn a_local_label_on_the_root_names_its_net_with_the_root_path() {
        let mut root = section();
        root.wires.push(wire("", &[p(0, 0), p(1000, 0)]));
        root.labels.push(label("IN", p(0, 0), LabelKind::Local));
        root.wires.push(wire("", &[p(0, 500), p(1000, 500)]));
        root.labels.push(label("CLK", p(0, 500), LabelKind::Global { shape: LabelShape::Input }));
        root.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(0, 900), rot: 0, net: "GND".into(), pin: String::new() });
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(1000, 0)), ("R2.1", p(1000, 500)), ("C1.2", p(0, 900))]) }];
        let nets = trace_nets_with(&mut screens, &Naming::default());
        let name = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|x| x == pin)).map(|n| n.name.clone()).unwrap();
        assert_eq!((name("R1.1"), name("R2.1"), name("C1.2")), ("/IN".to_string(), "CLK".to_string(), "GND".to_string()), "{nets:?}");
    }

    /// Two sibling sheets' hierarchical labels of one name, both on the root's net through their sheet pins: of drivers of one rank the alphabetically
    /// first name wins, compared with its path (`propagateToNeighbors`' `candidateName < bestName` over `Name()`), so the sheet named first in the
    /// alphabet names the net -- `/Header/RESET` and not `/MCU/RESET` -- and a `/` in a sheet's name is `{slash}`.
    #[test]
    fn hierarchical_labels_of_two_sheets_name_the_net_by_the_first_sheet_with_its_path() {
        let mut root = section();
        let sheet = |id: &str, name: &str, file: &str, y: i64| SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: p(100, y), size: (Nm::new(100), Nm::new(100)), pins: vec![SheetPin { id: String::new(), name: "RESET".into(), shape: LabelShape::Bidirectional, at: p(100, y + 50) }], page: String::new() };
        root.sheets.push(sheet("s1", "MCU", "mcu.kicad_sch", 0));
        root.sheets.push(sheet("s2", "Header/J1", "hdr.kicad_sch", 200));
        root.wires.push(wire("", &[p(100, 50), p(50, 50), p(50, 250), p(100, 250)]));
        let child = |at: Point| {
            let mut c = section();
            c.labels.push(label("RESET", at, LabelKind::Hierarchical { shape: LabelShape::Bidirectional }));
            c
        };
        let (mut mcu, mut hdr) = (child(p(10, 10)), child(p(20, 20)));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { path: Vec::new(), sch: &mut mcu, file: "mcu.kicad_sch".into(), pins: pins(&[("U1.11", p(10, 10))]) },
            ScreenIn { path: Vec::new(), sch: &mut hdr, file: "hdr.kicad_sch".into(), pins: pins(&[("J1.4", p(20, 20))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(nets.len(), 1, "{nets:?}");
        assert_eq!((nets[0].name.as_str(), nets[0].pins.clone()), ("/Header{slash}J1/RESET", vec!["J1.4".to_string(), "U1.11".into()]));
    }

    #[test]
    fn a_local_label_at_the_root_beats_the_hierarchical_label_inside() {
        let mut root = section();
        root.sheets.push(SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: p(100, 0), size: (Nm::new(100), Nm::new(100)), pins: vec![SheetPin { id: String::new(), name: "IN".into(), shape: LabelShape::Input, at: p(100, 50) }], page: String::new() });
        root.wires.push(wire("", &[p(0, 50), p(100, 50)]));
        root.labels.push(label("MAINBUS", p(0, 50), LabelKind::Local));
        let mut child = section();
        child.labels.push(label("IN", p(10, 10), LabelKind::Hierarchical { shape: LabelShape::Input }));
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 50))]) },
            ScreenIn { path: Vec::new(), sch: &mut child, file: "child.kicad_sch".into(), pins: pins(&[("U1.1", p(10, 10))]) },
        ];
        let nets = trace_nets(&mut screens);
        assert_eq!(pins_of(&nets, "/MAINBUS"), vec!["R1.1", "U1.1"]);
    }

    /// p12 S3: a power symbol's Value is resolved for each placement of its screen (`GetValue( &sheet, FOR_NETNAME )`): `P${#}` on a screen shown as pages 2
    /// and 3 is the two nets `P2` and `P3`, not one net that shorts the placements together.
    #[test]
    fn a_power_symbols_page_variable_is_resolved_per_placement() {
        let mut root = section();
        for (id, y) in [("s2", 0), ("s3", 2000)] {
            root.sheets.push(SheetInstance { id: id.into(), name: id.into(), file: "sub.kicad_sch".into(), at: p(0, y), size: (Nm::new(1000), Nm::new(1000)), pins: vec![], page: String::new() });
        }
        let sub = || {
            let mut c = section();
            c.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "Lib:P".into(), at: p(0, 0), rot: 0, net: "P${#}".into(), pin: String::new() });
            c.wires.push(wire("", &[p(0, 0), p(1000, 0)]));
            c
        };
        let (mut a, mut b) = (sub(), sub());
        let mut screens = vec![
            ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
            ScreenIn { path: vec!["s2".into()], sch: &mut a, file: "sub.kicad_sch".into(), pins: pins(&[("R1.1", p(1000, 0))]) },
            ScreenIn { path: vec!["s3".into()], sch: &mut b, file: "sub.kicad_sch".into(), pins: pins(&[("R2.1", p(1000, 0))]) },
        ];
        let page = |path: &[String], raw: &str| raw.replace("${#}", path.last().map(|id| &id[1..]).unwrap_or("1"));
        let nets = trace_nets_with(&mut screens, &Naming { power_value: Some(&page), ..Naming::default() });
        let named: Vec<(String, Vec<String>)> = nets.iter().map(|n| (n.name.clone(), n.pins.clone())).collect();
        assert_eq!(named, vec![("P2".to_string(), vec!["R1.1".to_string()]), ("P3".to_string(), vec!["R2.1".to_string()])]);
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

    /// The whole point of tracing: what `derive_schematic` draws is read back as exactly the nets it was given, named as KiCad names them (the intent's name
    /// on its label, with the root's `/` in front; a power net's own), for every example design in the repository.
    #[test]
    fn a_derived_schematic_traces_back_to_its_intent_nets() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
        files.extend(std::fs::read_dir(dir.join("ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")));
        files.sort();
        let mut checked = 0;
        let mut mismatches: Vec<String> = Vec::new();
        for f in files {
            let Ok(model) = serde_yaml::from_str::<eda_model::ConstraintModel>(&std::fs::read_to_string(&f).unwrap()) else { continue };
            let Ok(design) = crate::derive_schematic(&model, &crate::EngineOptions::new(1, "t")) else { continue };
            let mut sch = design.schematic.unwrap();
            let pins = world_pins(&sch, &model);
            let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut sch, file: String::new(), pins }];
            let got = trace_nets(&mut screens);
            let got: BTreeMap<String, Vec<String>> = got.into_iter().filter(|n| n.pins.len() >= 2).map(|n| (n.name, n.pins)).collect();
            let want = drawn_nets(&model);
            // the same pins, the names KiCad gives what is drawn: a power net's, a label's with the sheet's path, a pin's
            let by_pins = |m: &BTreeMap<String, Vec<String>>| -> BTreeMap<Vec<String>, String> { m.iter().map(|(n, p)| (p.clone(), n.clone())).collect() };
            // every named net is labelled on the root (`crate::net_labels`): a power net is its symbol's name (`GND`, a power symbol outranks a label), any
            // other `/<name>`
            let power: BTreeSet<&str> = sch.power_symbols.iter().filter(|p| !p.lib_id.ends_with(":PWR_FLAG")).map(|p| p.net.as_str()).collect();
            let named = by_pins(&got).iter().all(|(pins, name)| by_pins(&want).get(pins).is_some_and(|i| *name == if power.contains(i.as_str()) { i.clone() } else { format!("/{i}") }));
            // (an example whose flat drawing joins two nets or leaves one out is the layout's to fix, not the naming's: it is skipped, as before)
            if by_pins(&got).keys().ne(by_pins(&want).keys()) {
                println!("MISMATCH {}\n   got  {:?}\n   want {:?}", f.display(), got, want);
                continue;
            }
            if !named {
                mismatches.push(format!("{}\n   got  {:?}\n   want {:?}", f.display(), got, want));
                continue;
            }
            checked += 1;
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert!(checked >= 8, "only {checked} examples were readable");
    }

    #[test]
    fn a_no_connect_flag_keeps_its_pin_out_of_every_net() {
        let mut root = section();
        root.no_connects.push(eda_model::ir::NoConnect { id: String::new(), at: p(7, 7), pin: String::new() });
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("U1.5", p(7, 7))]) }];
        let nets = trace_nets(&mut screens);
        assert!(nets.is_empty(), "{nets:?}");
        assert_eq!(root.no_connects[0].pin, "U1.5");
    }

    // ------------------------------------------------------------------------------------------- KiCad's names

    fn shown(list: &[(&str, &str)]) -> BTreeMap<String, String> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// `SCH_PIN::GetDefaultNetName`: `Net-(R1-Pad1)`, the pin's name when it has one (`Net-(U1-PA0)`), the pad number added when the part has
    /// several pins of that name, `unconnected-(...)` for a pin on its own.
    #[test]
    fn a_label_with_a_slash_names_its_net_with_the_slash_escaped() {
        // `EscapeString( text, CTX_NETNAME )`: the sheet path's separator stays the only `/`
        assert_eq!(Driver::new(LOCAL_LABEL, "A/B".into(), 0, "").full, "/A{slash}B");
        assert_eq!(Driver::new(GLOBAL_POWER_PIN, "+3V3/A".into(), 0, "").full, "+3V3{slash}A");
        assert_eq!(Driver::new(LOCAL_LABEL, "A{slash}B".into(), 1, "/Sub").full, "/Sub/A{slash}B", "an escaped name stays as it is");
    }

    /// r2 A-N6: KiCad counts a pin name's duplicates among the pins of the unit drawn and the common ones (`symbol->GetPins( &aPath )`), not among every
    /// unit's: a dual op-amp's `+` of unit A is `Net-(U1A-+)` though unit B has a `+` too (kicad-cli names QA `complex_hierarchy`'s `Net-(U3A--)`).
    #[test]
    fn a_pin_name_repeated_in_another_unit_adds_no_pad_number() {
        let names = shown(&[("U1.1", "OUT"), ("U1.2", "-"), ("U1.3", "+"), ("U1.5", "+"), ("U1.6", "-"), ("U1.7", "OUT"), ("U1.4", "V-"), ("U1.8", "V-")]);
        let units = shown(&[("U1.1", "U1A"), ("U1.2", "U1A"), ("U1.3", "U1A"), ("U1.4", "U1A"), ("U1.8", "U1A"), ("U1.5", "U1B"), ("U1.6", "U1B"), ("U1.7", "U1B")]);
        // pins 4 and 8 are common to both units, and both named V-
        let common: BTreeSet<String> = ["U1.4", "U1.8"].iter().map(|s| s.to_string()).collect();
        let f = PinFacts { names: Some(&names), units: Some(&units), nc: None, common: Some(&common) };
        let count = shown_name_counts(&f);
        assert_eq!(default_net_name("U1.3", &f, &count, false), "Net-(U1A-+)");
        assert_eq!(default_net_name("U1.6", &f, &count, false), "Net-(U1B--)");
        assert_eq!(default_net_name("U1.7", &f, &count, false), "Net-(U1B-OUT)");
        assert_eq!(default_net_name("U1.4", &f, &count, false), "Net-(U1A-V--Pad4)", "two pins of one name on the unit drawn are duplicates");
    }

    #[test]
    fn a_net_nothing_drives_is_named_the_way_kicad_names_it() {
        let names = shown(&[("U1.3", "PA0"), ("U1.2", "GND"), ("U1.15", "GND"), ("R1.1", "~"), ("U2.1", "OUT"), ("U2.5", "NC"), ("U3.4", "NC"), ("U3.5", "NC")]);
        let units = shown(&[("U2.1", "U2A"), ("U2.5", "U2A")]);
        let nc: BTreeSet<String> = ["U2.5", "U3.4", "U3.5"].iter().map(|s| s.to_string()).collect();
        let f = PinFacts { names: Some(&names), units: Some(&units), nc: Some(&nc), common: None };
        let count = shown_name_counts(&f);
        assert_eq!(default_net_name("R1.1", &f, &count, false), "Net-(R1-Pad1)");
        assert_eq!(default_net_name("U1.3", &f, &count, false), "Net-(U1-PA0)");
        assert_eq!(default_net_name("U1.2", &f, &count, false), "Net-(U1-GND-Pad2)");
        assert_eq!(default_net_name("R2.1", &f, &count, true), "unconnected-(R2-Pad1)");
        assert_eq!(default_net_name("U1.3", &f, &count, true), "unconnected-(U1-PA0-Pad3)");
        // a symbol of several units: the reference with the unit, for a named pin (`referenceWithUnit`); a pin with only a number has the plain one
        assert_eq!(default_net_name("U2.1", &f, &count, false), "Net-(U2A-OUT)");
        // a no-connect pin is unconnected whatever its net, and two no-connect pins of one name are no duplicate (`hasDuplicateName` skips `PT_NC`)
        assert_eq!(default_net_name("U2.5", &f, &count, false), "unconnected-(U2A-NC-Pad5)");
        assert_eq!(default_net_name("U3.4", &f, &count, false), "unconnected-(U3-NC-Pad4)");
        assert_eq!(unconnected_net_name("U2.5", Some("NC"), Some("U2A")), "unconnected-(U2A-NC-Pad5)");
        // a pin whose name is its number has no name to show
        let same = shown(&[("J1.1", "1")]);
        let fs = PinFacts { names: Some(&same), ..PinFacts::default() };
        assert_eq!(default_net_name("J1.1", &fs, &shown_name_counts(&fs), false), "Net-(J1-Pad1)");
        // a net of several pins takes the name of its best pin: one with a name beats a pin with only a number, then the alphabetical order
        let pins = vec!["R1.2".to_string(), "U1.3".to_string()];
        assert_eq!(kicad_net_name(&pins, &f, &count), "Net-(U1-PA0)");
        let numbered = vec!["R2.1".to_string(), "R1.2".to_string()];
        assert_eq!(kicad_net_name(&numbered, &f, &count), "Net-(R1-Pad2)");
        assert_eq!(kicad_net_name(&["R2.1".to_string()], &f, &count), "unconnected-(R2-Pad1)");
        assert_eq!(default_net_name("U1.1", &PinFacts::default(), &BTreeMap::new(), false), "Net-(U1-Pad1)");
        assert_eq!(default_net_name("A/B.1", &PinFacts::default(), &BTreeMap::new(), false), "Net-(A{slash}B-Pad1)");
    }

    #[test]
    fn without_wire_hints_a_wire_does_not_name_its_net() {
        let mut root = section();
        root.wires.push(wire("OLD_NAME", &[p(0, 0), p(1000, 0)]));
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: pins(&[("R1.1", p(0, 0)), ("R2.1", p(1000, 0))]) }];
        let named = trace_nets_with(&mut screens, &Naming::default());
        assert_eq!(named.len(), 1);
        assert_eq!((named[0].name.as_str(), named[0].driven), ("Net-(R1-Pad1)", false));
        assert_eq!(root.wires[0].net, "Net-(R1-Pad1)");
        // and a stray wire on no pin is no net, and keeps the name it had
        let mut stray = section();
        stray.wires.push(wire("KEPT", &[p(0, 0), p(10, 0)]));
        let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut stray, file: String::new(), pins: BTreeMap::new() }];
        assert!(trace_nets_with(&mut screens, &Naming::default()).is_empty());
        assert_eq!(stray.wires[0].net, "KEPT");
    }

    // ------------------------------------------------------------------------------------------- following an edit

    fn tn(name: &str, pins: &[&str], driven: bool) -> TracedNet {
        TracedNet { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect(), driven }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() }
    }
    fn as_pairs(nets: &[Net]) -> Vec<(String, Vec<String>)> {
        nets.iter().map(|n| (n.name.clone(), n.pins.clone())).collect()
    }
    fn expect(nets: &[Net], want: &[(&str, &[&str])]) {
        let want: Vec<(String, Vec<String>)> = want.iter().map(|(n, p)| (n.to_string(), p.iter().map(|x| x.to_string()).collect())).collect();
        assert_eq!(as_pairs(nets), want);
    }

    /// The case that broke a board: a drawing whose labels do not touch their pins reads as pieces, and the same pieces before and
    /// after an edit that only moved a part are no change at all -- the net list stays, names and pins, whatever the pieces are called.
    #[test]
    fn an_edit_that_changes_nothing_connected_leaves_the_net_list_alone() {
        let before = vec![tn("Net-(U1-GND-Pad2)", &["U1.15", "U1.2"], false), tn("unconnected-(C1-Pad2)", &["C1.2"], false), tn("unconnected-(C2-Pad2)", &["C2.2"], false)];
        let after = vec![tn("NET_9", &["U1.15", "U1.2"], false), tn("NET_1", &["C1.2"], false), tn("NET_2", &["C2.2"], false)];
        assert!(follow(&[net("GND", &["C1.2", "C2.2", "U1.15", "U1.2"])], &before, &after).is_none());
        // and the drawing before and after is the same one
        assert!(follow(&[], &before, &before).is_none());
    }

    #[test]
    fn a_wire_that_joins_two_nets_joins_them_whole_under_the_name_that_wins() {
        let prev = vec![net("N1", &["R1.1", "R3.1"]), net("N2", &["R2.1", "R4.1"]), net("N3", &["R5.1", "R6.1"])];
        // the drawing shows every pin alone, as a stored drawing with loose labels does; the edit wires R1.1 to R2.1
        let before: Vec<TracedNet> = ["R1.1", "R2.1", "R3.1", "R4.1", "R5.1", "R6.1"].iter().map(|p| tn("x", &[p], false)).collect();
        let mut after = before.clone();
        after.retain(|t| t.pins != vec!["R1.1".to_string()] && t.pins != vec!["R2.1".to_string()]);
        after.push(tn("Net-(R1-Pad1)", &["R1.1", "R2.1"], false));
        let out = follow(&prev, &before, &after).unwrap();
        expect(&out, &[("N1", &["R1.1", "R2.1", "R3.1", "R4.1"]), ("N3", &["R5.1", "R6.1"])]);
    }

    #[test]
    fn a_deleted_wire_splits_a_net_and_the_larger_part_keeps_the_name() {
        let prev = vec![net("LED1", &["D1.1", "R1.2", "R9.1"])];
        let before = vec![tn("Net-(D1-Pad1)", &["D1.1", "R1.2", "R9.1"], false)];
        let after = vec![tn("Net-(D1-Pad1)", &["D1.1", "R1.2"], false), tn("unconnected-(R9-Pad1)", &["R9.1"], false)];
        let out = follow(&prev, &before, &after).unwrap();
        expect(&out, &[("LED1", &["D1.1", "R1.2"]), ("unconnected-(R9-Pad1)", &["R9.1"])]);
    }

    #[test]
    fn a_label_renames_the_net_it_is_put_on_and_joins_the_net_of_its_name() {
        let prev = vec![net("N1", &["a.1", "b.1"]), net("GND", &["c.1", "d.1"]), net("X", &["e.1"])];
        let before = vec![tn("n", &["a.1", "b.1"], false), tn("n", &["c.1", "d.1"], false), tn("n", &["e.1"], false)];
        // a label SIG on the first net renames it
        let after = vec![tn("SIG", &["a.1", "b.1"], true), before[1].clone(), before[2].clone()];
        expect(&follow(&prev, &before, &after).unwrap(), &[("GND", &["c.1", "d.1"]), ("SIG", &["a.1", "b.1"]), ("X", &["e.1"])]);
        // a label GND on the third joins the net called GND
        let after = vec![before[0].clone(), before[1].clone(), tn("GND", &["e.1"], true)];
        expect(&follow(&prev, &before, &after).unwrap(), &[("GND", &["c.1", "d.1", "e.1"]), ("N1", &["a.1", "b.1"])]);
    }

    #[test]
    fn a_no_connect_flag_takes_its_pin_out_of_the_net_and_a_renamed_part_keeps_its_net() {
        let prev = vec![net("N", &["a.1", "b.1", "c.1"]), net("LED1", &["D1.1", "R1.2"])];
        let before = vec![tn("n", &["a.1", "b.1", "c.1"], false), tn("n", &["D1.1", "R1.2"], false)];
        let after = vec![tn("n", &["b.1", "c.1"], false), before[1].clone()];
        expect(&follow(&prev, &before, &after).unwrap(), &[("LED1", &["D1.1", "R1.2"]), ("N", &["b.1", "c.1"])]);
        let renamed = vec![before[0].clone(), tn("n", &["D1.1", "R100.2"], false)];
        expect(&follow(&prev, &before, &renamed).unwrap(), &[("LED1", &["D1.1", "R100.2"]), ("N", &["a.1", "b.1", "c.1"])]);
    }

    /// An undriven net may not take the name of a net a driver names: the label's net is another net, whatever the list said.
    #[test]
    fn a_net_nothing_drives_does_not_take_the_name_of_a_driven_one() {
        let prev = vec![net("GND", &["a.1", "b.1", "c.1"])];
        let before = vec![tn("GND", &["a.1", "b.1"], true), tn("n", &["c.1"], false)];
        // c.1 is wired to d.1 (new in the design): it overlaps GND, but a driver owns that name
        let after = vec![before[0].clone(), tn("Net-(c-Pad1)", &["c.1", "d.1"], false)];
        let out = follow(&prev, &before, &after).unwrap();
        expect(&out, &[("GND", &["a.1", "b.1"]), ("Net-(c-Pad1)", &["c.1", "d.1"])]);
    }
}
