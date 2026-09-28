//! The board as routing changes it: items inserted and removed with the
//! plain search tree kept up to date, and the queries the push, clean-up
//! and insertion algorithms make of it. Ported from the editing half of
//! FreeRouting's `BasicBoard` and `RoutingBoard`, the contact queries of
//! `Item`, `Trace` and `DrillItem`, and `ChangedArea`.
//!
//! Items keep their index into [`Board::items`] for good; a removed item
//! stays in the list, marked off the board, as the Java's objects outlive
//! their removal and are asked `is_on_the_board`. The board's own order is
//! by item number descending, which new items, always numbered higher,
//! join at the front of.

pub mod insert;
pub mod pull_tight;
pub mod shove;
pub mod trace;
pub(crate) mod tree;
pub mod via;

pub use tree::{AutorouteTree, DefaultTree};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::geometry::{FloatPoint, IntBox, IntOctagon, Point, Polyline, TileShape};
use crate::model::{AreaKind, Board, FixedState, Item, ItemKind};

/// Where the board has changed since the last clean-up, per layer, as an
/// octagon of floating point bounds. `ChangedArea`.
#[derive(Debug, Clone)]
pub struct ChangedArea {
    /// Per layer: lx, ly, rx, uy, ulx, lrx, llx, urx.
    arr: Vec<[f64; 8]>,
}

const EMPTY_AREA: [f64; 8] = [i32::MAX as f64, i32::MAX as f64, i32::MIN as f64, i32::MIN as f64, i32::MAX as f64, i32::MIN as f64, i32::MAX as f64, i32::MIN as f64];

impl ChangedArea {
    pub fn new(layer_count: usize) -> Self {
        ChangedArea { arr: vec![EMPTY_AREA; layer_count] }
    }

    /// Grow layer `layer`'s octagon to hold `p`. `join(FloatPoint, int)`.
    pub fn join(&mut self, p: FloatPoint, layer: i32) {
        let c = &mut self.arr[layer as usize];
        c[0] = p.x.min(c[0]);
        c[1] = p.y.min(c[1]);
        c[2] = c[2].max(p.x);
        c[3] = c[3].max(p.y);
        let tmp = p.x - p.y;
        c[4] = c[4].min(tmp);
        c[5] = c[5].max(tmp);
        let tmp = p.x + p.y;
        c[6] = c[6].min(tmp);
        c[7] = c[7].max(tmp);
    }

    /// Layer `layer`'s octagon, rounded outwards; `IntOctagon::EMPTY` if
    /// nothing changed there. `get_area`.
    pub fn get_area(&self, layer: i32) -> IntOctagon {
        let c = &self.arr[layer as usize];
        if c[2] < c[0] || c[3] < c[1] || c[5] < c[4] || c[7] < c[6] {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(
            c[0].floor() as i64,
            c[1].floor() as i64,
            c[2].ceil() as i64,
            c[3].ceil() as i64,
            c[4].floor() as i64,
            c[5].ceil() as i64,
            c[6].floor() as i64,
            c[7].ceil() as i64,
        )
    }

    pub fn set_empty(&mut self, layer: i32) {
        self.arr[layer as usize] = EMPTY_AREA;
    }
}

/// The board being routed. FreeRouting's `RoutingBoard`, with the search
/// trees of its `SearchTreeManager`.
pub struct RoutingBoard {
    pub board: Board,
    pub tree: DefaultTree,
    /// The autoroute trees made so far, by clearance class, each `None`
    /// while a search holds it.
    autoroute_trees: RefCell<Vec<(i32, Option<AutorouteTree>)>>,
    /// Each area's convex pieces once split, by item index: FreeRouting
    /// keeps them with the area, and splitting a pour again for every
    /// contact asked about is slow.
    area_pieces: RefCell<HashMap<usize, Rc<Vec<TileShape>>>>,
    on_board: Vec<bool>,
    /// `ItemIdNoGenerator.max_generated_no`.
    id_max: Cell<u32>,
    pub changed_area: Option<ChangedArea>,
    /// `BasicBoard.min_trace_half_width` and `max_trace_half_width`: the
    /// narrowest and widest trace inserted so far, starting from 10000 and
    /// 1000.
    pub min_trace_half_width: i64,
    pub max_trace_half_width: i64,
}

/// `Nets.max_legal_net_no`.
pub const MAX_LEGAL_NET_NO: i32 = 9_999_999;

/// Which items `pick_items` keeps. The `ItemSelectionFilter`s the router
/// uses, each with fixed and unfixed items selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Traces,
    Pins,
    Vias,
    Conduction,
    All,
}

impl RoutingBoard {
    pub fn new(board: Board) -> Self {
        let tree = DefaultTree::new(&board);
        let on_board = vec![true; board.items.len()];
        let id_max = board.id_max.max(board.items.iter().map(|i| i.id).max().unwrap_or(0));
        let min_trace_half_width = board.rules.min_trace_half_width;
        let max_trace_half_width = board.rules.board_max_trace_half_width;
        RoutingBoard { board, tree, autoroute_trees: RefCell::new(Vec::new()), area_pieces: RefCell::new(HashMap::new()), on_board, id_max: Cell::new(id_max), changed_area: None, min_trace_half_width, max_trace_half_width }
    }

    /// The autoroute tree for traces of clearance class `class`, made from
    /// the board the first time it is asked for, for a search to hold until
    /// it hands it back. `SearchTreeManager.get_autoroute_tree`. `None`
    /// where FreeRouting throws making it: the tree stays as far as it got.
    pub(crate) fn take_autoroute_tree(&self, class: i32) -> Option<AutorouteTree> {
        let mut trees = self.autoroute_trees.borrow_mut();
        if let Some((_, t)) = trees.iter_mut().find(|(c, _)| *c == class) {
            return Some(t.take().expect("the autoroute tree is held by another search"));
        }
        drop(trees);
        let built = AutorouteTree::build(&self.board, &self.items_in_order(), class);
        let mut trees = self.autoroute_trees.borrow_mut();
        match built {
            Ok(tree) => {
                trees.push((class, None));
                Some(tree)
            }
            Err(partial) => {
                trees.push((class, Some(partial)));
                None
            }
        }
    }

    /// Hand back a tree [`take_autoroute_tree`](Self::take_autoroute_tree)
    /// gave out.
    pub(crate) fn give_back_autoroute_tree(&self, tree: AutorouteTree) {
        let mut trees = self.autoroute_trees.borrow_mut();
        let slot = trees.iter_mut().find(|(c, _)| *c == tree.class).expect("a tree given out");
        slot.1 = Some(tree);
    }

    /// The autoroute trees, but those a search holds, which cannot change
    /// meanwhile: the board is borrowed.
    fn autoroute_trees(&mut self) -> impl Iterator<Item = &mut AutorouteTree> {
        self.autoroute_trees.get_mut().iter_mut().filter_map(|(_, t)| t.as_mut())
    }

    /// Enter the item into every search tree. `SearchTreeManager.insert`.
    fn trees_insert(&mut self, item: usize) {
        self.tree.insert(&self.board, item);
        let board = &self.board;
        for t in self.autoroute_trees.get_mut().iter_mut().filter_map(|(_, t)| t.as_mut()) {
            t.insert(board, item);
        }
    }

    /// Take the item out of every search tree. `SearchTreeManager.remove`.
    fn trees_remove(&mut self, item: usize) {
        self.tree.remove(item);
        for t in self.autoroute_trees() {
            t.remove(item);
        }
    }

    /// The autoroute tree for class `class`, made if there is none yet, a
    /// node a line: see [`AutorouteTree::fingerprint`].
    pub fn autoroute_tree_listing(&self, class: i32) -> String {
        let Some(tree) = self.take_autoroute_tree(class) else { return String::from("tree (not made)\n") };
        let listing = tree.fingerprint(true).2.expect("asked for in full");
        self.give_back_autoroute_tree(tree);
        listing
    }

    /// The layout of each autoroute tree not held by a search, by class:
    /// see [`ShapeTree::fingerprint`](crate::searchtree::ShapeTree::fingerprint).
    pub fn autoroute_tree_fingerprints(&self) -> Vec<(i32, usize, u64)> {
        let trees = self.autoroute_trees.borrow();
        trees.iter().filter_map(|(c, t)| t.as_ref().map(|t| (*c, t.fingerprint(false)))).map(|(c, (n, h, _))| (c, n, h)).collect()
    }

    pub fn is_on_board(&self, item: usize) -> bool {
        self.on_board[item]
    }

    pub fn item(&self, item: usize) -> &Item {
        &self.board.items[item]
    }

    /// `max_generated_no` of the item number generator.
    pub fn id_max(&self) -> u32 {
        self.id_max.get()
    }

    /// The index of the item numbered `id` on the board.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.board.items.iter().enumerate().position(|(i, it)| it.id == id && self.on_board[i])
    }

    /// The items on the board in its order: by number descending.
    pub fn items_in_order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.board.items.len()).filter(|&i| self.on_board[i]).collect();
        v.sort_by(|a, b| self.board.items[*b].id.cmp(&self.board.items[*a].id));
        v
    }

    /// Sort item indices by number descending, dropping repeats: a
    /// `TreeSet<Item>`.
    pub fn sort_items(&self, items: &mut Vec<usize>) {
        items.sort_by(|a, b| self.board.items[*b].id.cmp(&self.board.items[*a].id));
        items.dedup();
    }

    /// Put a new item on the board, numbered next. `insert_item`, with the
    /// number drawn as the Java's item constructors draw it.
    pub fn insert_new(&mut self, mut item: Item) -> usize {
        item.id = self.skip_id();
        self.insert_numbered(item)
    }

    /// Draw the next item number without putting anything on the board, as
    /// the Java does constructing an item it then discards -- checks too,
    /// which make trial traces, so this takes the board shared.
    pub fn skip_id(&self) -> u32 {
        let id = self.id_max.get() + 1;
        self.id_max.set(id);
        id
    }

    fn insert_numbered(&mut self, item: Item) -> usize {
        self.board.items.push(item);
        self.on_board.push(true);
        let i = self.board.items.len() - 1;
        self.trees_insert(i);
        i
    }

    /// Take an item off the board. `remove_item`.
    pub fn remove_item(&mut self, item: usize) {
        if !self.on_board[item] {
            return;
        }
        self.trees_remove(item);
        self.on_board[item] = false;
    }

    /// Remove the items not fixed against it; false if some were.
    /// `remove_items`.
    pub fn remove_items(&mut self, items: &[usize], with_delete_fixed: bool) -> bool {
        let mut result = true;
        for &i in items {
            let it = &self.board.items[i];
            if (!with_delete_fixed && self.is_delete_fixed(i)) || it.is_user_fixed() {
                result = false;
            } else {
                self.remove_item(i);
            }
        }
        result
    }

    /// Items of a component, fixed by the user, or planes.
    /// `Item.is_delete_fixed`.
    fn is_delete_fixed(&self, item: usize) -> bool {
        let it = &self.board.items[item];
        if it.component > 0 || it.is_user_fixed() {
            return true;
        }
        match &it.kind {
            ItemKind::Area { kind: AreaKind::Conduction { .. }, layer, .. } => !self.board.layers[*layer as usize].is_signal,
            _ => false,
        }
    }

    /// Insert a trace along `polyline` as it is, with no clean-up: `None` if
    /// it has fewer than two corners, or starts where it ends and is not
    /// fixed by the user -- the second after drawing its number.
    /// `insert_trace_without_cleaning`.
    pub fn insert_trace_without_cleaning(&mut self, polyline: Polyline, layer: i32, half_width: i64, nets: &[i32], clearance_class: i32, fixed: FixedState) -> Option<usize> {
        if polyline.corner_count() < 2 {
            return None;
        }
        let layer = layer.clamp(0, self.board.layer_count() as i32 - 1);
        if polyline.first_corner().java_equals(&polyline.last_corner()) && fixed < FixedState::UserFixed {
            self.skip_id();
            return None;
        }
        let item = Item {
            id: 0,
            kind: ItemKind::Trace { layer, half_width, polyline },
            first_layer: layer,
            last_layer: layer,
            clearance_class,
            fixed,
            component: 0,
            nets: nets.to_vec(),
        };
        let i = self.insert_new(item);
        if self.nets_normal(nets) {
            self.max_trace_half_width = self.max_trace_half_width.max(half_width);
            self.min_trace_half_width = self.min_trace_half_width.min(half_width);
        }
        Some(i)
    }

    /// `Item.nets_normal`: every net a real one.
    pub fn nets_normal(&self, nets: &[i32]) -> bool {
        nets.iter().all(|&n| n > 0 && n <= MAX_LEGAL_NET_NO)
    }

    /// Replace a trace's polyline in place, its tree entries renewed.
    pub(crate) fn set_polyline(&mut self, item: usize, polyline: Polyline) {
        let on = self.on_board[item];
        if on {
            self.trees_remove(item);
        }
        self.put_polyline(item, polyline);
        if on {
            self.trees_insert(item);
        }
    }

    /// Replace a trace's polyline, leaving the trees to the caller.
    fn put_polyline(&mut self, item: usize, polyline: Polyline) {
        if let ItemKind::Trace { polyline: p, .. } = &mut self.board.items[item].kind {
            *p = polyline;
        }
    }

    /// Replace a trace's polyline, the autoroute trees renewing only the
    /// shapes from `keep_start` to `keep_end` before the end.
    /// `SearchTreeManager.change_entries`; the default tree, whose layout
    /// never shows, has the trace entered afresh.
    pub(crate) fn change_polyline_entries(&mut self, item: usize, polyline: Polyline, keep_start: usize, keep_end: usize) {
        self.tree.remove(item);
        self.put_polyline(item, polyline);
        self.tree.insert(&self.board, item);
        let board = &self.board;
        for t in self.autoroute_trees.get_mut().iter_mut().filter_map(|(_, t)| t.as_mut()) {
            t.change_entries(board, item, keep_start, keep_end);
        }
    }

    /// Give trace `to` the polyline joining trace `from` to it, `in_front`
    /// or at its end, the autoroute trees moving `from`'s leaves over;
    /// `change_order` if `from` runs the other way. `from` is left with no
    /// leaves there, to be removed. `SearchTreeManager.merge_entries_in_front`
    /// and `merge_entries_at_end`.
    pub(crate) fn merge_polyline_entries(&mut self, from: usize, to: usize, joined: Polyline, in_front: bool, change_order: bool) {
        self.tree.remove(to);
        self.put_polyline(to, joined);
        self.tree.insert(&self.board, to);
        let board = &self.board;
        for t in self.autoroute_trees.get_mut().iter_mut().filter_map(|(_, t)| t.as_mut()) {
            if in_front {
                t.merge_in_front(board, from, to, change_order);
            } else {
                t.merge_at_end(board, from, to, change_order);
            }
        }
    }

    /// `start_marking_changed_area`.
    pub fn start_marking_changed_area(&mut self) {
        if self.changed_area.is_none() {
            self.changed_area = Some(ChangedArea::new(self.board.layer_count()));
        }
    }

    /// `join_changed_area`.
    pub fn join_changed_area(&mut self, p: FloatPoint, layer: i32) {
        if let Some(c) = &mut self.changed_area {
            c.join(p, layer);
        }
    }

    /// The changed area's octagon on `layer`, `None` with no changed area:
    /// the clip shape the Java takes from it, `null` without one.
    pub fn changed_area_on(&self, layer: i32) -> Option<IntOctagon> {
        self.changed_area.as_ref().map(|c| c.get_area(layer))
    }

    // ---------------------------------------------------------------
    // Trace accessors.

    /// A trace's polyline, layer and half width.
    pub fn trace(&self, item: usize) -> (&Polyline, i32, i64) {
        match &self.board.items[item].kind {
            ItemKind::Trace { polyline, layer, half_width } => (polyline, *layer, *half_width),
            _ => panic!("item {} is not a trace", self.board.items[item].id),
        }
    }

    pub fn is_trace(&self, item: usize) -> bool {
        matches!(self.board.items[item].kind, ItemKind::Trace { .. })
    }

    pub fn first_corner(&self, item: usize) -> Point {
        self.trace(item).0.first_corner()
    }

    pub fn last_corner(&self, item: usize) -> Point {
        self.trace(item).0.last_corner()
    }

    // ---------------------------------------------------------------
    // Queries on the plain tree.

    /// The items whose shapes on `layer` (every layer if negative) meet
    /// `shape`, by number descending. `overlapping_objects`.
    pub fn overlapping_objects(&self, shape: &TileShape, layer: i32) -> Vec<usize> {
        let mut v: Vec<usize> = self.tree.overlapping_entries(&self.board, shape, layer, &[]).into_iter().map(|(i, _, _)| i).collect();
        self.sort_items(&mut v);
        v
    }

    /// The items on `layer` holding `p`, of the kinds `pick` selects.
    /// `pick_items`.
    pub fn pick_items(&self, p: &Point, layer: i32, pick: Pick) -> Vec<usize> {
        let shape = point_shape(p);
        let mut v = self.overlapping_objects(&shape, layer);
        v.retain(|&i| match pick {
            Pick::Traces => self.is_trace(i),
            Pick::Pins => matches!(self.board.items[i].kind, ItemKind::Pin { .. }),
            Pick::Vias => matches!(self.board.items[i].kind, ItemKind::Via { .. }),
            Pick::Conduction => matches!(self.board.items[i].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. }),
            Pick::All => true,
        });
        v
    }

    // ---------------------------------------------------------------
    // Contacts.

    /// The items of the trace's nets ending at `p` on its layer, where `p`
    /// is one of its end corners: traces ending there, pins and vias
    /// centred there, pours holding it. `Trace.get_normal_contacts(Point,
    /// false)`.
    pub fn trace_contacts_at(&self, item: usize, p: &Point) -> Vec<usize> {
        let first = self.first_corner(item);
        let last = self.last_corner(item);
        if !(p.java_equals(&first) || p.java_equals(&last)) {
            return Vec::new();
        }
        let this = &self.board.items[item];
        let (_, layer, _) = self.trace(item);
        let mut result = Vec::new();
        for i in self.overlapping_objects(&point_shape(p), layer) {
            let other = &self.board.items[i];
            if i == item || !shares_layer(other, this) || !other.shares_net(this) {
                continue;
            }
            if self.touches_at(i, p) {
                result.push(i);
            }
        }
        result
    }

    /// Whether item `i` connects at `p`: a trace ending there, a pin or via
    /// centred there, a pour holding it.
    fn touches_at(&self, i: usize, p: &Point) -> bool {
        let other = &self.board.items[i];
        match &other.kind {
            ItemKind::Trace { polyline, .. } => p.java_equals(&polyline.first_corner()) || p.java_equals(&polyline.last_corner()),
            ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => p.java_equals(&Point::Int(*center)),
            ItemKind::Area { kind: AreaKind::Conduction { .. }, .. } => self.area_pieces(i).iter().any(|s| s.contains_point(p)),
            _ => false,
        }
    }

    /// `get_start_contacts`.
    pub fn start_contacts(&self, item: usize) -> Vec<usize> {
        let p = self.first_corner(item);
        self.trace_contacts_at(item, &p)
    }

    /// `get_end_contacts`.
    pub fn end_contacts(&self, item: usize) -> Vec<usize> {
        let p = self.last_corner(item);
        self.trace_contacts_at(item, &p)
    }

    /// Everything an item connects to directly at its connection points.
    /// `get_normal_contacts()`, per kind.
    pub fn normal_contacts(&self, item: usize) -> Vec<usize> {
        let this = &self.board.items[item];
        match &this.kind {
            ItemKind::Trace { .. } => {
                let mut v = self.start_contacts(item);
                v.extend(self.end_contacts(item));
                self.sort_items(&mut v);
                v
            }
            ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => {
                let p = Point::Int(*center);
                let mut result = Vec::new();
                for i in self.overlapping_objects(&point_shape(&p), -1) {
                    let other = &self.board.items[i];
                    if i != item && other.shares_net(this) && shares_layer(other, this) && self.touches_at(i, &p) {
                        result.push(i);
                    }
                }
                result
            }
            ItemKind::Area { kind: AreaKind::Conduction { .. }, layer, .. } => {
                let mut result = Vec::new();
                for piece in self.area_pieces(item).iter() {
                    for i in self.overlapping_objects(piece, *layer) {
                        let other = &self.board.items[i];
                        if i == item || !other.shares_net(this) || !shares_layer(other, this) {
                            continue;
                        }
                        let hit = match &other.kind {
                            ItemKind::Trace { polyline, .. } => piece.contains_point(&polyline.first_corner()) || piece.contains_point(&polyline.last_corner()),
                            ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => piece.contains_point(&Point::Int(*center)),
                            _ => false,
                        };
                        if hit {
                            result.push(i);
                        }
                    }
                }
                self.sort_items(&mut result);
                result
            }
            _ => Vec::new(),
        }
    }

    /// The connectable items of its nets touching the item's shapes on
    /// `layer`. `Item.get_all_contacts(int)`.
    pub fn all_contacts_on(&self, item: usize, layer: i32) -> Vec<usize> {
        let this = &self.board.items[item];
        if !this.is_connectable_kind() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let count = self.tile_shape_count(item);
        for k in 0..count {
            if self.shape_layer(item, k) != layer {
                continue;
            }
            let Some(shape) = self.tile_shape(item, k) else { continue };
            for i in self.overlapping_objects(&shape, layer) {
                let other = &self.board.items[i];
                if i != item && other.is_connectable_kind() && other.shares_net(this) {
                    result.push(i);
                }
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// An area's convex pieces, none where the split fails; split once.
    /// `Area.split_to_convex`.
    pub fn area_pieces(&self, item: usize) -> Rc<Vec<TileShape>> {
        if let Some(p) = self.area_pieces.borrow().get(&item) {
            return p.clone();
        }
        let ItemKind::Area { shape, .. } = &self.board.items[item].kind else {
            panic!("item {} is not an area", self.board.items[item].id)
        };
        let pieces = Rc::new(shape.split_to_convex().unwrap_or_default());
        self.area_pieces.borrow_mut().insert(item, pieces.clone());
        pieces
    }

    /// `Item.tile_shape_count`: a pad per layer of the stack, a shape per
    /// segment of a trace, the convex pieces of an area.
    pub fn tile_shape_count(&self, item: usize) -> usize {
        match &self.board.items[item].kind {
            ItemKind::Area { .. } => self.area_pieces(item).len(),
            _ => self.tree.shape_count(item),
        }
    }

    /// `Item.get_tile_shape`: the plain tree shape, but an area's convex
    /// piece.
    pub fn tile_shape(&self, item: usize, index: usize) -> Option<TileShape> {
        match &self.board.items[item].kind {
            ItemKind::Area { .. } => self.area_pieces(item).get(index).cloned(),
            _ => self.tree.get_shape(item, index as u32).cloned(),
        }
    }

    /// The layer of shape `index`. `shape_layer`.
    pub fn shape_layer(&self, item: usize, index: usize) -> i32 {
        let it = &self.board.items[item];
        it.shape_layer(index, self.board.layer_count(), self.tree.shape_count(item))
    }

    /// The pins of the trace's nets within a half width of either end,
    /// clearance included. `Trace.touching_pins_at_end_corners`.
    pub fn touching_pins_at_end_corners(&self, item: usize) -> Vec<usize> {
        let this = &self.board.items[item];
        let (polyline, layer, half_width) = self.trace(item);
        let mut result = Vec::new();
        for p in [polyline.first_corner(), polyline.last_corner()] {
            let Some(o) = surrounding_octagon(&p) else { continue };
            let shape = TileShape::Octagon(o.offset(half_width as f64));
            for i in self.overlapping_items_with_clearance(&shape, layer, &[], this.clearance_class) {
                let other = &self.board.items[i];
                if matches!(other.kind, ItemKind::Pin { .. }) && other.shares_net(this) {
                    result.push(i);
                }
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// The trace of exactly `nets` ending at `p` on `layer` with nothing
    /// connected there. `get_trace_tail`.
    pub fn get_trace_tail(&self, p: &Point, layer: i32, nets: &[i32]) -> Option<usize> {
        for i in self.overlapping_objects(&point_shape(p), layer) {
            if !self.is_trace(i) {
                continue;
            }
            if !nets_equal(&self.board.items[i].nets, nets) {
                continue;
            }
            if self.first_corner(i).java_equals(p) && self.start_contacts(i).is_empty() {
                return Some(i);
            }
            if self.last_corner(i).java_equals(p) && self.end_contacts(i).is_empty() {
                return Some(i);
            }
        }
        None
    }

    /// `BasicBoard.clearance_value`: with the safety margin.
    pub fn clearance_value(&self, class_1: i32, class_2: i32, layer: i32) -> i64 {
        self.board.rules.clearance.get_with_margin(class_1, class_2, layer)
    }

    pub fn bounds(&self) -> IntBox {
        self.board.bounds
    }
}

/// `Item.shares_layer`.
pub fn shares_layer(a: &Item, b: &Item) -> bool {
    a.first_layer.max(b.first_layer) <= a.last_layer.min(b.last_layer)
}

/// `Item.nets_equal(int[])`.
pub fn nets_equal(nets: &[i32], other: &[i32]) -> bool {
    nets.len() == other.len() && other.iter().all(|&n| n > 0 && nets.contains(&n))
}

/// `TileShape.get_instance(Point)`: the box of the point itself.
pub fn point_shape(p: &Point) -> TileShape {
    match p {
        Point::Int(q) => TileShape::Box(IntBox::new(q.x, q.y, q.x, q.y)),
        Point::Rational(_) => {
            let (x, y) = p.to_float();
            TileShape::Box(IntBox::new(x.floor() as i64, y.floor() as i64, x.ceil() as i64, y.ceil() as i64))
        }
    }
}

/// `Point.surrounding_octagon`: the point itself for an integer point, the
/// octagon of its floating point rounded outwards for a rational one.
pub fn surrounding_octagon(p: &Point) -> Option<IntOctagon> {
    match p {
        Point::Int(q) => Some(IntOctagon::new(q.x, q.y, q.x, q.y, q.x - q.y, q.x - q.y, q.x + q.y, q.x + q.y)),
        Point::Rational(_) => {
            let (x, y) = p.to_float();
            let (d, s) = (x - y, x + y);
            Some(IntOctagon::new(x.floor() as i64, y.floor() as i64, x.ceil() as i64, y.ceil() as i64, d.floor() as i64, d.ceil() as i64, s.floor() as i64, s.ceil() as i64))
        }
    }
}
