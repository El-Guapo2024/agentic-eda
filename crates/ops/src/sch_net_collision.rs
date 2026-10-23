//! The net-collision overlay of a drag: red circles where the moved items would short two nets, and a red mark where something that was connected has been pulled apart.
//! Ported from `SCH_DRAG_NET_COLLISION_MONITOR` (eeschema/tools/sch_drag_net_collision.cpp at 8303b2ad), which `SCH_MOVE_TOOL::Main` runs for every held selection:
//! `Initialize( selection )` when the move starts, `Update( previewJunctions, selection )` after every `performItemMove`.
//!
//! What the C++ does, and how it is read here:
//! - **Initialize** records the net code of every connectable item that has a connection (`recordItemNet`: wires, buses, labels, bus entries, junctions, sheet pins; a
//!   symbol has none, its pins have), and the connections the selection had (`recordOriginalConnections`): for each connection point of a selected item, each other item
//!   that was connected there, with the index of that point on both.
//! - **Update** takes the junction dots the moved items would need (`JUNCTION_HELPERS::PreviewJunctions`) and marks each one where the items connected there were on two
//!   different nets and at least one of them is moved (`analyzeJunction`); then marks each of the recorded connections whose two points no longer meet
//!   (`collectDisconnectedMarkers`), a circle at each point and a line between them.
//! - The net of an item is the one it had when the move started. KiCad reads it off the connection graph; here the nets are the sheet's own geometry (points that share a
//!   place, the points of a wire and every point on it, labels of one name, power symbols of one net), which is what `eda_engine::nets` traces and the connection graph's
//!   subgraphs are. An item the drag made (a new wire between a held item and a fixed one) has no net of its own and is not counted, as in the C++ (`m_itemNetCodes` is
//!   filled before the first move).
//!
//! The monitor is read in the same code path as the move (`sch_move::run_with_overlay`), so the overlay the studio shows is the one that goes with the preview it shows.

use crate::sch_drag::Drag;
use crate::sch_scene::{on_segment, Item, Scene};
use eda_model::ir::Point;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// `DEFAULT_JUNCTION_DIAM`, 36 mil: the preview junction's effective diameter (`SCH_JUNCTION::GetEffectiveDiameter`), nanometres.
const JUNCTION_DIAMETER_NM: f64 = 914_400.0;
/// "std::max( base * 1.5, 800.0 )" and "std::max( { 800.0, ... } )": 800 nm.
const MIN_RADIUS_NM: f64 = 800.0;

/// A circle where the held items would join two nets (`COLLISION_MARKER`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollisionMarker {
    pub at: Point,
    pub radius_nm: f64,
}

/// Two points that were one connection and no longer meet, with the line between them (`DISCONNECTION_MARKER`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisconnectionMarker {
    pub a: Point,
    pub b: Point,
    pub radius_nm: f64,
}

/// What the monitor draws for one state of a held selection.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NetCollision {
    pub collisions: Vec<CollisionMarker>,
    pub disconnections: Vec<DisconnectionMarker>,
}

impl NetCollision {
    /// `m_hasCollision`: the cursor is the warning cursor while there is anything to draw.
    pub fn is_empty(&self) -> bool {
        self.collisions.is_empty() && self.disconnections.is_empty()
    }
}

/// An item as the monitor names it: the index of a wire segment (segments are only ever added to the scene during a move), the id of anything else (an index would shift
/// when a junction or a no-connect is removed at the start of a move).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Seg(usize),
    Other(&'static str, String),
}

fn key_of(scene: &Scene, it: Item) -> Key {
    let named = |kind: &'static str, id: &str, i: usize| Key::Other(kind, if id.is_empty() { format!("#{i}") } else { id.to_string() });
    match it {
        Item::Seg(i) => Key::Seg(i),
        Item::Junction(i) => named("junction", &scene.sch.junctions[i].id, i),
        Item::NoConnect(i) => named("no_connect", &scene.sch.no_connects[i].id, i),
        Item::BusEntry(i) => named("bus_entry", &scene.sch.bus_entries[i].id, i),
        Item::Label(i) => named("label", &scene.sch.labels[i].id, i),
        Item::Power(i) => named("power", &scene.sch.power_symbols[i].id, i),
        Item::Symbol(i) => named("symbol", &format!("{}#{}", scene.sch.symbols[i].id, scene.sch.symbols[i].unit), i),
        Item::SheetPin(s, p) => named("sheet_pin", &scene.sch.sheets[s].pins[p].id, s * 10_000 + p),
        Item::Sheet(i) => named("sheet", &scene.sch.sheets[i].id, i),
        Item::Graphic(i) => named("graphic", &scene.sch.extras.graphics[i].id, i),
        Item::Table(i) => named("table", &scene.sch.extras.tables[i].id, i),
        Item::NoteLine(i) => named("line", &scene.sch.lines[i].id, i),
        Item::Text(i) => named("text", &scene.sch.texts[i].id, i),
    }
}

/// The live connectable items of the scene by key, as of now.
fn items_by_key(scene: &Scene) -> BTreeMap<Key, Item> {
    scene.items().into_iter().filter(|it| scene.is_connectable(*it)).map(|it| (key_of(scene, it), it)).collect()
}

/// Does this kind of item carry a connection of its own (`SCH_ITEM::Connection`)? A symbol does not: its pins do, and they are not items of the sheet here.
fn has_connection(it: Item) -> bool {
    matches!(it, Item::Seg(_) | Item::Label(_) | Item::BusEntry(_) | Item::Junction(_) | Item::SheetPin(..))
}

struct Uf(Vec<usize>);

impl Uf {
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

/// The net code of every item that has a connection (`recordItemNet`): the group its connection point is in. Two wires of one drawn net share it, as do the two sides of a
/// label's name and of a power symbol's net.
fn net_codes(scene: &Scene) -> BTreeMap<Key, usize> {
    let mut ids: BTreeMap<Point, usize> = BTreeMap::new();
    let mut uf = Uf(Vec::new());
    let mut seed = |p: Point, uf: &mut Uf| *ids.entry(p).or_insert_with(|| uf.add());
    for it in scene.items() {
        if !scene.is_connectable(it) {
            continue;
        }
        for p in scene.connection_points(it) {
            seed(p, &mut uf);
        }
    }
    // a wire is one node along its whole length, a bus likewise
    let points: Vec<Point> = ids.keys().copied().collect();
    for s in scene.segs.iter().filter(|s| !s.dead) {
        let (a, b) = (ids[&s.a], ids[&s.b]);
        uf.union(a, b);
        for q in points.iter().filter(|q| on_segment(s.a, s.b, **q)) {
            uf.union(a, ids[q]);
        }
    }
    // labels of one name are one net, and power symbols of one net
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for l in &scene.sch.labels {
        let at = ids[&l.at];
        match by_name.get(&l.net) {
            Some(&first) => uf.union(first, at),
            None => {
                by_name.insert(l.net.clone(), at);
            }
        }
    }
    for p in &scene.sch.power_symbols {
        let at = ids[&p.at];
        match by_name.get(&format!("power:{}", p.net)) {
            Some(&first) => uf.union(first, at),
            None => {
                by_name.insert(format!("power:{}", p.net), at);
            }
        }
    }
    let mut out = BTreeMap::new();
    for it in scene.items() {
        if !has_connection(it) || !scene.is_connectable(it) {
            continue;
        }
        let pts = scene.connection_points(it);
        // a bus entry's own net is the one on its wire side
        let at = match it {
            Item::BusEntry(_) => pts.iter().copied().find(|p| !scene.segs.iter().any(|s| !s.dead && s.bus && on_segment(s.a, s.b, *p))).or_else(|| pts.first().copied()),
            _ => pts.first().copied(),
        };
        if let Some(p) = at {
            out.insert(key_of(scene, it), uf.find(ids[&p]));
        }
    }
    out
}

/// `ORIGINAL_CONNECTION`.
#[derive(Debug, Clone)]
struct Original {
    a: Key,
    index_a: usize,
    b: Key,
    index_b: usize,
}

/// `SCH_DRAG_NET_COLLISION_MONITOR`: what the sheet was when the move started.
pub(crate) struct Monitor {
    nets: BTreeMap<Key, usize>,
    originals: Vec<Original>,
}

impl Monitor {
    /// `Initialize( aSelection )`, with the selection the scene holds.
    pub(crate) fn initialize(scene: &Scene) -> Monitor {
        let nets = net_codes(scene);
        let selection: BTreeSet<Item> = scene.in_selection().into_iter().collect();
        let mut originals: Vec<Original> = Vec::new();
        // `recordOriginalConnections`
        for &item in &selection {
            if !scene.is_connectable(item) {
                continue;
            }
            let points = scene.connection_points(item);
            for (index, &point) in points.iter().enumerate() {
                for candidate in scene.items() {
                    if candidate == item || !scene.is_connectable(candidate) || !scene.can_connect(candidate, item) {
                        continue;
                    }
                    let on_line = matches!(candidate, Item::Seg(i) if scene.seg_hit(i, point));
                    if !scene.is_connected(candidate, point) && !on_line {
                        continue;
                    }
                    // the point's place in the candidate's own list: a point in the middle of a line is none of them, and that connection is not recorded
                    let Some(candidate_index) = scene.connection_points(candidate).iter().position(|p| *p == point) else { continue };
                    let (first, first_index, second, second_index) = if candidate < item || (candidate == item && candidate_index < index) {
                        (candidate, candidate_index, item, index)
                    } else {
                        (item, index, candidate, candidate_index)
                    };
                    if first == second || !(selection.contains(&first) || selection.contains(&second)) {
                        continue;
                    }
                    let record = Original { a: key_of(scene, first), index_a: first_index, b: key_of(scene, second), index_b: second_index };
                    if !originals.iter().any(|o| o.a == record.a && o.index_a == record.index_a && o.b == record.b && o.index_b == record.index_b) {
                        originals.push(record);
                    }
                }
            }
        }
        Monitor { nets, originals }
    }

    /// `Update( previewJunctions, aSelection )` for the scene as the move has left it. `drag` is the state of a drag (`m_newDragLines`, `m_changedDragLines`), none for a move.
    pub(crate) fn update(&self, scene: &Scene, drag: Option<&Drag>) -> NetCollision {
        let selection: BTreeSet<Item> = scene.in_selection().into_iter().collect();
        let by_key = items_by_key(scene);
        let mut out = NetCollision::default();

        // `analyzeJunction` for each of `JUNCTION_HELPERS::PreviewJunctions`
        for at in self.preview_junctions(scene, &selection, drag) {
            if self.junction_collides(scene, &selection, &by_key, at) {
                let radius_nm = (JUNCTION_DIAMETER_NM * 1.5).max(MIN_RADIUS_NM);
                if !out.collisions.iter().any(|m| m.at == at) {
                    out.collisions.push(CollisionMarker { at, radius_nm });
                }
            }
        }

        // `collectDisconnectedMarkers`
        for o in &self.originals {
            let (Some(&a), Some(&b)) = (by_key.get(&o.a), by_key.get(&o.b)) else { continue };
            let (points_a, points_b) = (scene.connection_points(a), scene.connection_points(b));
            if o.index_a >= points_a.len() || o.index_b >= points_b.len() {
                continue;
            }
            let (point_a, point_b) = (points_a[o.index_a], points_b[o.index_b]);
            // "Check if the connection is still valid. Points match exactly", or "for lines, the point is anywhere on the line"
            let hit = |it: Item, p: Point| matches!(it, Item::Seg(i) if scene.seg_hit(i, p));
            if point_a == point_b || hit(b, point_a) || hit(a, point_b) {
                continue;
            }
            if !(selection.contains(&a) || selection.contains(&b)) {
                continue;
            }
            let radius_nm = MIN_RADIUS_NM.max(pen_width_nm(scene, a)).max(pen_width_nm(scene, b));
            out.disconnections.push(DisconnectionMarker { a: point_a, b: point_b, radius_nm });
        }
        out
    }

    /// `JUNCTION_HELPERS::PreviewJunctions( screen, items )`: the places where the moved items (and the lines a drag made or changed) meet something and a junction dot is
    /// needed. The C++ builds a tree of the screen's other items and the moved ones; in the scene they are all in one place.
    fn preview_junctions(&self, scene: &Scene, selection: &BTreeSet<Item>, drag: Option<&Drag>) -> Vec<Point> {
        let mut preview: BTreeSet<Item> = selection.clone();
        if let Some(st) = drag {
            preview.extend(st.new_lines.iter().filter(|i| !scene.segs[**i].dead).map(|i| Item::Seg(*i)));
            preview.extend(st.changed_lines.iter().filter(|i| !scene.segs[**i].dead).map(|i| Item::Seg(*i)));
        }
        // `aScreen->GetConnections()`: every connection point of the sheet
        let connections: BTreeSet<Point> = scene.items().into_iter().filter(|it| scene.is_connectable(*it)).flat_map(|it| scene.connection_points(it)).collect();
        let mut pts: BTreeSet<Point> = BTreeSet::new();
        for &it in &preview {
            if !scene.is_connectable(it) {
                continue;
            }
            pts.extend(scene.connection_points(it));
            if let Item::Seg(i) = it {
                let s = &scene.segs[i];
                pts.extend(connections.iter().copied().filter(|p| on_segment(s.a, s.b, *p)));
            }
        }
        pts.into_iter()
            .filter(|p| {
                let info = scene.analyze_point(*p, false, &BTreeSet::new());
                info.is_junction && (!info.has_bus_entry || info.has_bus_entry_to_multiple_wires)
            })
            .collect()
    }

    /// `analyzeJunction`: do the items connected at `at` come from two nets, with one of them held? (The preview net assignments are never given, so the nets are the
    /// ones recorded at the start: `previewCollision` and `originalCollision` ask the same thing.)
    fn junction_collides(&self, scene: &Scene, selection: &BTreeSet<Item>, by_key: &BTreeMap<Key, Item>, at: Point) -> bool {
        let mut all: BTreeSet<usize> = BTreeSet::new();
        let mut moved: BTreeSet<usize> = BTreeSet::new();
        for (key, &item) in by_key {
            // "if( !item->IsConnected( position ) && !( item->IsType( { SCH_LINE_T } ) && item->HitTest( position ) ) ) return"
            let on_line = matches!(item, Item::Seg(i) if scene.seg_hit(i, at));
            if !scene.is_connected(item, at) && !on_line {
                continue;
            }
            // an item the move made has no recorded net and is not counted; a symbol has none either
            let Some(&code) = self.nets.get(key) else { continue };
            all.insert(code);
            if selection.contains(&item) {
                moved.insert(code);
            }
        }
        !moved.is_empty() && all.len() >= 2
    }
}

/// `SCH_ITEM::GetPenWidth` of what a connection joins, micrometres: a wire is 6 mil and a bus 12 mil (`m_DefaultLineWidth`, `m_DefaultBusThickness`), a label's text is an
/// eighth of its size; a symbol, a junction and the rest are 0 here.
fn pen_width_nm(scene: &Scene, it: Item) -> f64 {
    match it {
        Item::Seg(i) => {
            let stroke = scene.segs[i].from.and_then(|(wi, _)| scene.sch.extras.strokes.get(&scene.meta[wi].id)).map(|s| s.width_nm).filter(|w| *w > 0);
            match stroke {
                Some(w) => w as f64,
                None if scene.segs[i].bus => 304_800.0,
                None => 152_400.0,
            }
        }
        Item::Label(i) => {
            let size = scene.sch.extras.label_looks.get(&scene.sch.labels[i].id).map_or(0, |l| l.text_size_nm());
            (if size > 0 { size } else { eda_model::ir::DEFAULT_FIELD_SIZE_NM }) as f64 / 8.0
        }
        _ => 0.0,
    }
}
