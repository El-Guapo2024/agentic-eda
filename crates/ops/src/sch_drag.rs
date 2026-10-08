//! The drag path of `SCH_MOVE_TOOL` (eeschema/tools/sch_move_tool.cpp at 8303b2ad): which wires, labels and other items an item being
//! dragged takes with it (`getConnectedDragItems`), the cache of what each wire's loose end was attached to (`getConnectedItems`), how a
//! wire with one end dragged keeps its right angles (`orthoLineDrag`), and the move itself (`performItemMove`, `moveItem`).
//!
//! KiCad runs these once per mouse event; here the whole drag is one step, so the move is applied once for the final offset (an `x` move,
//! then a `y` move, as `performItemMove` splits every move). Where a function differs from the C++ the difference is said at that function.

use crate::sch_scene::{on_segment, parallel, Item, Scene, Seg, F_BY_DRAG, F_END, F_NEW, F_SELECTED, F_START};
use eda_model::ir::{Point, Um};
use std::collections::{BTreeMap, BTreeSet};

/// `SPECIAL_CASE_LABEL_INFO`: a label on a wire with one end dragged slides along as the wire's ends move.
#[derive(Clone, Debug)]
pub(crate) struct SpecialLabel {
    pub line: usize,
    pub original_pos: Point,
    pub original_start: Point,
    pub original_end: Point,
    pub track_moving_end: bool,
}

/// The state `SCH_MOVE_TOOL` keeps between `setupItemsForDrag` and the end of the drag.
#[derive(Default)]
pub(crate) struct Drag {
    /// `m_dragAdditions`: what the drag added to the selection.
    pub additions: Vec<Item>,
    /// `m_lineConnectionCache`: what was attached to a wire's end that stays where it is.
    pub line_conn: BTreeMap<usize, Vec<Item>>,
    /// `m_newDragLines`.
    pub new_lines: BTreeSet<usize>,
    /// `m_changedDragLines`: lines the drag changed that nobody selected.
    pub changed_lines: BTreeSet<usize>,
    /// `m_specialCaseLabels`.
    pub special_labels: BTreeMap<Item, SpecialLabel>,
    /// `m_specialCaseSheetPins`: a selected sheet pin and the end of a wire it is on (`true` = the start).
    pub special_pins: Vec<(Item, usize, bool)>,
    /// `m_moveOffset`.
    pub move_offset: Point,
    /// `isLineModeConstrained`: keep partially dragged wires at right angles.
    pub ortho: bool,
    /// `lineGrid`: the wire grid, for the bends.
    pub grid: Um,
}

impl<'m> Scene<'m> {
    /// Every item of the selection (what the user picked and what the drag added), in a stable order.
    pub(crate) fn in_selection(&self) -> Vec<Item> {
        let mut out: BTreeSet<Item> = self.selected.iter().copied().collect();
        out.extend(self.by_drag.iter().copied());
        for (i, s) in self.segs.iter().enumerate() {
            if !s.dead && (s.has(F_SELECTED) || s.has(F_BY_DRAG)) {
                out.insert(Item::Seg(i));
            }
        }
        out.into_iter().collect()
    }

    /// `SCH_ITEM::IsSelected`: the picked items, and the pins of a picked sheet (`highlight` flags a selected item's children too).
    fn is_user_selected(&self, it: Item) -> bool {
        match it {
            Item::Seg(i) => self.segs[i].has(F_SELECTED),
            Item::SheetPin(s, _) => self.selected.contains(&it) || self.selected.contains(&Item::Sheet(s)),
            other => self.selected.contains(&other),
        }
    }

    fn has_by_drag(&self, it: Item) -> bool {
        match it {
            Item::Seg(i) => self.segs[i].has(F_BY_DRAG),
            other => self.by_drag.contains(&other),
        }
    }

    fn set_by_drag(&mut self, it: Item) {
        match it {
            Item::Seg(i) => self.segs[i].flags |= F_BY_DRAG,
            other => {
                self.by_drag.insert(other);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------- setup

    /// `SCH_MOVE_TOOL::setupItemsForDrag`: add the things attached to the selection, and cache what each wire's fixed end is attached to.
    pub(crate) fn setup_items_for_drag(&mut self, st: &mut Drag) {
        let selection = self.in_selection();
        let mut connected: Vec<Item> = Vec::new();
        // all but labels first, so a junction is not dragged when the wire it sits on is going to be
        let mut stage_two: Vec<Item> = Vec::new();
        for &it in &selection {
            let points: Vec<Point> = match it {
                Item::Label(_) | Item::SheetPin(..) => {
                    stage_two.push(it);
                    Vec::new()
                }
                Item::Graphic(i) if self.is_label_like(Item::Graphic(i)) => {
                    stage_two.push(it);
                    Vec::new()
                }
                Item::Seg(i) => {
                    // `SCH_LINE::GetSelectedPoints`
                    let s = &self.segs[i];
                    let mut v = Vec::new();
                    if s.has(F_START) {
                        v.push(s.a);
                    }
                    if s.has(F_END) {
                        v.push(s.b);
                    }
                    v
                }
                other => self.connection_points(other),
            };
            for p in points {
                self.get_connected_drag_items(st, it, p, &mut connected);
            }
        }
        // labels, now that the wires they are on may be dragged too
        for it in stage_two {
            for p in self.connection_points(it) {
                self.get_connected_drag_items(st, it, p, &mut connected);
            }
        }
        for it in connected {
            if !st.additions.contains(&it) {
                st.additions.push(it);
            }
            // `AddItemToSel`: a line is in the selection by its flag, anything else by the set
            if let Item::Seg(_) = it {
                // already flagged `F_BY_DRAG`
            } else {
                self.by_drag.insert(it);
            }
        }
        // what each line's loose end is attached to, to see if the drag can stretch a neighbour instead of adding a bend
        for it in self.in_selection() {
            let Item::Seg(i) = it else { continue };
            let (start, end) = (self.segs[i].has(F_START), self.segs[i].has(F_END));
            if self.segs[i].length() > 0.0 {
                self.segs[i].stored = self.segs[i].dir();
            }
            // Only the end that stays put is special, and only when the line is partly selected: a fully selected line, and the moving end
            // of a partly selected one, "are moved around normally and don't care about their connections". (KiCad also caches what is
            // at the moving end, so a wire whose far end is free still counts as attached and gets bends; this reads the intent.)
            if start != end {
                let loose = if start { self.segs[i].b } else { self.segs[i].a };
                let mut found = Vec::new();
                self.get_connected_items(st, Item::Seg(i), loose, &mut found);
                st.line_conn.entry(i).or_default().extend(found);
            }
        }
    }

    /// `SCH_MOVE_TOOL::getConnectedItems`.
    pub(crate) fn get_connected_items(&self, st: &mut Drag, original: Item, p: Point, list: &mut Vec<Item>) {
        let items = self.items();
        let mut found_junction: Option<Item> = None;
        let mut found_symbol: Option<Item> = None;
        for &it in &items {
            if it != original && self.is_connected(it, p) {
                match it {
                    Item::Junction(_) => found_junction = Some(it),
                    Item::Symbol(_) | Item::Power(_) => found_symbol = Some(it),
                    _ => {}
                }
            }
        }
        // "If you're connected to a junction, you're only connected to the junction. But, if you're connected to a junction on a pin,
        // you're only connected to the pin."
        if let (Some(s), Some(_)) = (found_symbol, found_junction) {
            list.push(s);
            return;
        }
        if let Some(j) = found_junction {
            list.push(j);
            return;
        }
        for &test in &items {
            if test == original || !self.can_connect(test, original) {
                continue;
            }
            match test {
                Item::Seg(ti) => {
                    let t = &self.segs[ti];
                    // only the end that stays is special: a line whose own selected end is here moves with us
                    if (t.has(F_START) && t.a == p) || (t.has(F_END) && t.b == p) {
                        continue;
                    }
                    if t.is_endpoint(p) {
                        list.push(test);
                    }
                    // labels can connect to a wire anywhere along its length
                    if self.is_label_like(original) && self.seg_hit(ti, self.position(original)) {
                        list.push(test);
                    }
                }
                Item::SheetPin(..) => {
                    if let Item::Seg(oi) = original {
                        if self.is_connected(test, p) {
                            if self.is_user_selected(test) {
                                st.special_pins.push((test, oi, self.segs[oi].a == p));
                            }
                            list.push(test);
                        }
                    }
                }
                Item::Symbol(_) | Item::Power(_) | Item::Junction(_) | Item::NoConnect(_) => {
                    if self.is_connected(test, p) {
                        list.push(test);
                    }
                }
                Item::Label(_) | Item::Graphic(_) if self.is_label_like(test) => {
                    if let Item::Seg(oi) = original {
                        if self.seg_hit(oi, self.position(test)) {
                            list.push(test);
                        }
                    }
                }
                Item::BusEntry(_) => {
                    if let Item::Seg(oi) = original {
                        if self.seg_hit(oi, p) {
                            list.push(test);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A new wire between a fixed item and a selected one (`makeNewWire`): a line of no length at `p`, so the selected item can be dragged
    /// away from the fixed one and leave a wire to it. A bus when either end is a bus or a bus label.
    fn make_new_wire(&mut self, fixed: Item, selected: Item, start: Point, end: Point) -> usize {
        let bus_label = |scene: &Scene, it: Item| match it {
            Item::Label(i) => crate::sch_scene::is_bus_label(&scene.sch.labels[i].net),
            _ => false,
        };
        let is_bus = |scene: &Scene, it: Item| matches!(it, Item::Seg(i) if scene.segs[i].bus);
        let bus = is_bus(self, fixed) || is_bus(self, selected) || bus_label(self, fixed) || bus_label(self, selected);
        let mut w = Seg::new(start, end, bus);
        w.flags = F_NEW;
        // the connection of the line it is made from (`cloneWireConnection`)
        let from = match (selected, fixed) {
            (Item::Seg(i), _) | (_, Item::Seg(i)) => Some(i),
            _ => None,
        };
        if let Some(i) = from {
            w.net = self.segs[i].net.clone();
        }
        self.segs.push(w);
        self.segs.len() - 1
    }

    /// `SCH_MOVE_TOOL::getConnectedDragItems`: what is attached to `selected` at `p` and must follow it.
    pub(crate) fn get_connected_drag_items(&mut self, st: &mut Drag, selected: Item, p: Point, list: &mut Vec<Item>) {
        let all = self.items();
        let mut connectable: Vec<Item> = Vec::new();
        for &it in &all {
            match it {
                Item::Sheet(_) => continue,
                Item::SheetPin(..) => {
                    // a sheet pin that is not selected, at this point, and can connect to us
                    if !self.is_user_selected(it) && self.position(it) == p && self.can_connect(it, selected) {
                        connectable.push(it);
                    }
                }
                _ => {
                    // skip ourselves, skip selected items (but not lines, which need both ends tested) and the unconnectable
                    if it == selected || (!matches!(it, Item::Seg(_)) && self.is_user_selected(it)) || !self.can_connect(it, selected) {
                        continue;
                    }
                    connectable.push(it);
                }
            }
        }
        let has_unselected_junction = connectable.iter().any(|&it| matches!(it, Item::Junction(_)) && self.is_connected(it, p) && !self.is_user_selected(it));
        let mut new_wire: Option<usize> = None;

        for test in connectable {
            match test {
                Item::Seg(ti) => {
                    // "Select the connected end of wires/bus connections that don't have an unselected junction isolating them from the drag"
                    if has_unselected_junction {
                        continue;
                    }
                    if self.segs[ti].dead {
                        continue;
                    }
                    let (a, b) = (self.segs[ti].a, self.segs[ti].b);
                    if a == p || b == p {
                        let flag = if a == p { F_START } else { F_END };
                        self.segs[ti].flags |= flag;
                        if self.segs[ti].has(F_SELECTED) || self.segs[ti].has(F_BY_DRAG) {
                            continue;
                        }
                        self.segs[ti].flags |= F_BY_DRAG;
                        list.push(Item::Seg(ti));
                    } else {
                        // these items can connect anywhere along a line
                        let anywhere = matches!(selected, Item::BusEntry(_)) || self.is_label_like(selected);
                        if anywhere && self.seg_hit(ti, p) && !self.segs[ti].has(F_SELECTED) && !self.segs[ti].has(F_BY_DRAG) {
                            let nw = self.make_new_wire(test, selected, p, p);
                            self.segs[nw].flags |= F_BY_DRAG | F_START;
                            let (dx, dy) = self.segs[ti].dir();
                            self.segs[nw].stored = (-dy, dx);
                            list.push(Item::Seg(nw));
                            // split the line in half at the point, with a junction there
                            let old_end = self.segs[ti].b;
                            self.segs[ti].b = p;
                            let mut tail = self.segs[ti].clone();
                            tail.a = p;
                            tail.b = old_end;
                            tail.flags = F_NEW;
                            tail.from = self.segs[ti].from;
                            self.segs.push(tail);
                            self.sch.junctions.push(eda_model::ir::Junction { id: String::new(), at: p });
                            st.line_conn.insert(nw, vec![Item::Seg(ti)]);
                            st.line_conn.insert(ti, vec![Item::Seg(nw)]);
                        }
                        continue;
                    }
                    // "When only one end moves, keep attached labels tracking the moving end so they stay connected to the line."
                    for li in self.items() {
                        if !self.is_label_like(li) || matches!(li, Item::SheetPin(..)) {
                            continue;
                        }
                        if self.is_user_selected(li) || self.has_by_drag(li) {
                            continue;
                        }
                        if self.can_connect(li, Item::Seg(ti)) && self.seg_hit(ti, self.position(li)) {
                            self.set_by_drag(li);
                            list.push(li);
                            st.special_labels.insert(
                                li,
                                SpecialLabel { line: ti, original_pos: self.position(li), original_start: self.segs[ti].a, original_end: self.segs[ti].b, track_moving_end: false },
                            );
                        }
                    }
                }
                Item::Symbol(_) | Item::Power(_) | Item::Junction(_) => {
                    // "Add a new wire between the symbol or junction and the selected item so the selected item can be dragged."
                    if self.is_connected(test, p) && new_wire.is_none() {
                        let nw = self.make_new_wire(test, selected, p, p);
                        self.segs[nw].flags |= F_BY_DRAG | F_START;
                        new_wire = Some(nw);
                        list.push(Item::Seg(nw));
                    }
                }
                Item::NoConnect(_) => {
                    // "Select no-connects that are connected to items being moved."
                    if !self.has_by_drag(test) && self.is_connected(test, p) {
                        list.push(test);
                        self.set_by_drag(test);
                    }
                }
                Item::Label(_) | Item::Graphic(_) | Item::SheetPin(..) if self.is_label_like(test) => {
                    if self.has_by_drag(test) {
                        continue;
                    }
                    // "Select labels that are connected to a wire (or bus) being moved."
                    if let (Item::Seg(si), true) = (selected, self.can_connect(test, selected)) {
                        let one_end_fixed = !self.segs[si].has(F_START) || !self.segs[si].has(F_END);
                        let lp = self.position(test);
                        if self.seg_hit(si, lp) {
                            let (a, b) = (self.segs[si].a, self.segs[si].b);
                            if (!self.segs[si].has(F_START) && lp == a) || (!self.segs[si].has(F_END) && lp == b) {
                                // "If we have a line selected at only one end, don't grab labels connected directly to the unselected endpoint"
                                continue;
                            }
                            self.set_by_drag(test);
                            list.push(test);
                            if one_end_fixed {
                                st.special_labels.insert(test, SpecialLabel { line: si, original_pos: lp, original_start: a, original_end: b, track_moving_end: false });
                            }
                        }
                    } else if self.is_connected(test, p) && new_wire.is_none() {
                        // "Add a new wire between the label and the selected item so the selected item can be dragged."
                        let nw = self.make_new_wire(test, selected, p, p);
                        self.segs[nw].flags |= F_BY_DRAG | F_START;
                        new_wire = Some(nw);
                        list.push(Item::Seg(nw));
                    }
                }
                Item::BusEntry(_) => {
                    if self.has_by_drag(test) {
                        continue;
                    }
                    // "Select bus entries that are connected to a bus being moved."
                    if let (Item::Seg(si), true) = (selected, self.can_connect(test, selected)) {
                        let s = &self.segs[si];
                        let touches = |sc: &Scene, e: Point| sc.is_connected(test, e);
                        if (!s.has(F_START) && touches(self, s.a)) || (!s.has(F_END) && touches(self, s.b)) {
                            continue;
                        }
                        for end in self.connection_points(test) {
                            if self.seg_hit(si, end) {
                                self.set_by_drag(test);
                                list.push(test);
                                // "A bus entry needs its wire & label as well"
                                let ends = self.connection_points(test);
                                let other = if ends[0] == end { ends[1] } else { ends[0] };
                                self.get_connected_drag_items(st, test, other, list);
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------------------ move

    /// `SCH_MOVE_TOOL::moveItem`: one item by `delta`, as a drag does it (a line moves only the ends that are flagged).
    pub(crate) fn move_item_drag(&mut self, st: &Drag, it: Item, delta: Point) {
        match it {
            Item::Seg(i) => {
                let s = &mut self.segs[i];
                if s.has(F_START) {
                    s.a = Point { x: s.a.x + delta.x, y: s.a.y + delta.y };
                }
                if s.has(F_END) {
                    s.b = Point { x: s.b.x + delta.x, y: s.b.y + delta.y };
                }
            }
            // a label on a wire with one end dragged is placed by the wire's ends instead
            Item::Label(_) | Item::Graphic(_) if st.special_labels.contains_key(&it) => {}
            other => self.move_item(other, delta),
        }
    }

    /// `SCH_ITEM::Move( delta )` and `MoveSchematicItem`'s sheet pin case.
    pub(crate) fn move_item(&mut self, it: Item, d: Point) {
        let add = |p: Point| Point { x: p.x + d.x, y: p.y + d.y };
        match it {
            Item::Junction(i) => self.sch.junctions[i].at = add(self.sch.junctions[i].at),
            Item::NoConnect(i) => self.sch.no_connects[i].at = add(self.sch.no_connects[i].at),
            Item::BusEntry(i) => self.sch.bus_entries[i].at = add(self.sch.bus_entries[i].at),
            Item::Seg(i) => {
                self.segs[i].a = add(self.segs[i].a);
                self.segs[i].b = add(self.segs[i].b);
            }
            Item::NoteLine(i) => {
                for p in self.sch.lines[i].pts.iter_mut() {
                    *p = Point { x: p.x + d.x, y: p.y + d.y };
                }
            }
            Item::Graphic(i) => crate::sch_move::move_graphic(&mut self.sch.extras.graphics[i], d),
            Item::Text(i) => self.sch.texts[i].at = add(self.sch.texts[i].at),
            Item::Label(i) => self.sch.labels[i].at = add(self.sch.labels[i].at),
            Item::Power(i) => self.sch.power_symbols[i].at = add(self.sch.power_symbols[i].at),
            Item::Symbol(i) => self.sch.symbols[i].at = add(self.sch.symbols[i].at),
            Item::Sheet(i) => {
                let s = &mut self.sch.sheets[i];
                s.at = Point { x: s.at.x + d.x, y: s.at.y + d.y };
                for p in s.pins.iter_mut() {
                    p.at = Point { x: p.at.x + d.x, y: p.at.y + d.y };
                }
            }
            Item::SheetPin(s, p) => {
                // `MoveSchematicItem`: the pin follows the delta along the sheet's edge (and may change edge)
                let sheet = &self.sch.sheets[s];
                let target = add(sheet.pins[p].at);
                self.sch.sheets[s].pins[p].at = constrain_on_edge(&self.sch.sheets[s], target, true, sheet.pins[p].at);
            }
        }
    }

    /// `SCH_MOVE_TOOL::performItemMove` for the whole drag: `delta` is applied as an `x` move, then a `y` move.
    pub(crate) fn perform_item_move(&mut self, st: &mut Drag, delta: Point, drag_mode: bool) {
        let mut x_bend: i64 = 1;
        let mut y_bend: i64 = 1;
        // The move is split in x and y so a dragged orthogonal line can be told which way it is going. (KiCad also splits a move that
        // changes the sign of the running offset; a drag applied in one step never does.)
        let splits = [Point { x: delta.x, y: 0 }, Point { x: 0, y: delta.y }];
        st.move_offset = Point { x: st.move_offset.x + delta.x, y: st.move_offset.y + delta.y };
        for split in splits {
            if split == (Point { x: 0, y: 0 }) {
                continue;
            }
            for it in self.sorted_selection(delta.x >= 0, delta.y >= 0) {
                if drag_mode && st.ortho {
                    if let Item::Seg(i) = it {
                        let s = &self.segs[i];
                        // only a line the user picked or the drag reached, with exactly one end moving, is held at right angles
                        if s.has(F_START) != s.has(F_END) && !(s.has(F_BY_DRAG) && s.has(F_NEW)) {
                            self.ortho_line_drag(st, i, split, &mut x_bend, &mut y_bend);
                        }
                    }
                }
                if drag_mode {
                    self.move_item_drag(st, it, split);
                } else {
                    self.move_item(it, split);
                }
                // lines on a sheet pin follow the pin to where the edge put it
                let pins = st.special_pins.clone();
                for (pin, line, is_start) in pins {
                    let pos = self.position(pin);
                    if is_start && self.segs[line].has(F_START) {
                        self.segs[line].a = pos;
                    } else if !is_start && self.segs[line].has(F_END) {
                        self.segs[line].b = pos;
                    }
                }
            }
            self.place_special_labels(st, split);
        }
    }

    /// `SELECTION::GetItemsSortedByTypeAndXY`.
    pub(crate) fn sorted_selection(&self, left_before_right: bool, top_before_bottom: bool) -> Vec<Item> {
        let mut v = self.in_selection();
        v.sort_by(|a, b| {
            if a.rank() != b.rank() {
                return a.rank().cmp(&b.rank());
            }
            let (pa, pb) = (self.sort_position(*a), self.sort_position(*b));
            if pa.x == pb.x {
                if pa.y == pb.y {
                    return a.cmp(b);
                }
                return if top_before_bottom { pa.y.cmp(&pb.y) } else { pb.y.cmp(&pa.y) };
            }
            if left_before_right {
                pa.x.cmp(&pb.x)
            } else {
                pb.x.cmp(&pa.x)
            }
        });
        v
    }

    /// The label part of `performItemMove`: "Needed to keep labels attached to a line when dragging a sheet/wire combo with a label on
    /// the line. The label moves by splitDelta for each part of the split move, but the line endpoints may not follow splitDelta due to
    /// orthogonal drag or sheet pin constraints, which can put the label off the line."
    fn place_special_labels(&mut self, st: &mut Drag, split: Point) {
        let keys: Vec<Item> = st.special_labels.keys().copied().collect();
        for label in keys {
            let info = st.special_labels[&label].clone();
            if self.segs[info.line].dead {
                continue;
            }
            if info.track_moving_end {
                self.move_item(label, split);
                let (start, end) = (self.segs[info.line].a, self.segs[info.line].b);
                if self.segs[info.line].length() > 0.0 && self.seg_hit(info.line, info.original_pos) && info.original_pos != start && info.original_pos != end {
                    self.set_label_pos(label, info.original_pos);
                    let e = st.special_labels.get_mut(&label).unwrap();
                    e.track_moving_end = false;
                    e.original_start = start;
                    e.original_end = end;
                }
                continue;
            }
            let (start, end) = (self.segs[info.line].a, self.segs[info.line].b);
            let delta_start = Point { x: start.x - info.original_start.x, y: start.y - info.original_start.y };
            let delta_end = Point { x: end.x - info.original_end.x, y: end.y - info.original_end.y };
            if delta_start == delta_end {
                self.set_label_pos(label, Point { x: info.original_pos.x + delta_start.x, y: info.original_pos.y + delta_start.y });
            } else {
                let start_drags = self.segs[info.line].has(F_START);
                let fixed = if start_drags { delta_end } else { delta_start };
                let mut pos = Point { x: info.original_pos.x + fixed.x, y: info.original_pos.y + fixed.y };
                self.set_label_pos(label, pos);
                // "If the line shrank while dragging, keep the label on the line, otherwise the label can drift off the end of the
                // line, and change connectivity"
                if !self.seg_hit(info.line, pos) {
                    pos = nearest_on_segment(start, end, pos);
                    self.set_label_pos(label, pos);
                    let moving_end = if start_drags { start } else { end };
                    if pos == moving_end {
                        st.special_labels.get_mut(&label).unwrap().track_moving_end = true;
                    }
                }
            }
        }
    }

    pub(crate) fn set_label_pos(&mut self, it: Item, p: Point) {
        match it {
            Item::Label(i) => self.sch.labels[i].at = p,
            Item::Graphic(i) => crate::sch_move::set_graphic_position(&mut self.sch.extras.graphics[i], p),
            Item::SheetPin(s, k) => self.sch.sheets[s].pins[k].at = p,
            _ => {}
        }
    }

    // ------------------------------------------------------------------------------------------------------------ orthoLineDrag

    /// `SCH_MOVE_TOOL::orthoLineDrag`: `line` has one end that moves by `split` and one that stays. A move along the line only shortens
    /// or lengthens it. Any other move would make it slant, so the neighbour at the fixed end that runs the way we are moving is dragged
    /// along (or a line of no length is lengthened), and when the fixed end is attached to something that cannot move two lines are
    /// added to keep it there, joined by a right angle.
    pub(crate) fn ortho_line_drag(&mut self, st: &mut Drag, line: usize, split: Point, x_bend: &mut i64, y_bend: &mut i64) {
        let sd = (split.x, split.y);
        if parallel(sd, self.segs[line].dir()) && self.segs[line].length() != 0.0 {
            return;
        }
        let start_flagged = self.segs[line].has(F_START);
        let (unselected_end, selected_end) = if start_flagged { (self.segs[line].b, self.segs[line].a) } else { (self.segs[line].a, self.segs[line].b) };

        // look for pre-existing lines we can drag with us instead of creating new ones
        let mut found_attachment = false;
        let mut found_junction = false;
        let found_pin = false;
        let mut found_line: Option<usize> = None;
        for &c in st.line_conn.get(&line).cloned().unwrap_or_default().iter() {
            found_attachment = true;
            match c {
                Item::Seg(ci) => {
                    if self.segs[ci].dead {
                        continue;
                    }
                    // a matching angle on a line with length means lengthen/shorten works
                    if parallel(sd, self.segs[ci].dir()) && self.segs[ci].length() != 0.0 {
                        found_line = Some(ci);
                    }
                    // "Zero length lines are lines that this algorithm has shortened to 0 so they also work but we should prefer using a
                    // segment with length and angle matching when we can"
                    if found_line.is_none() && self.segs[ci].length() == 0.0 {
                        found_line = Some(ci);
                    }
                }
                Item::Junction(_) => found_junction = true,
                _ => {}
            }
        }

        // "Ok... what if our original line is length zero from moving in its direction, and the last added segment of the 90 bend we are
        // connected to is zero from moving it in its direction after it was added?"
        let mut prefer_original_line = false;
        if let Some(fl) = found_line {
            if self.segs[fl].length() == 0.0 && self.segs[line].length() == 0.0 && parallel(sd, self.segs[line].stored) {
                prefer_original_line = true;
            }
        } else if found_junction && !found_pin {
            // "If we have found an attachment, but not a line, we want to check if it's a junction. These are special-cased and get a
            // single line added instead of a 90-degree bend."
            let mut nl = Seg::new(unselected_end, unselected_end, self.segs[line].bus);
            nl.flags = F_NEW;
            nl.net = self.segs[line].net.clone();
            self.segs.push(nl);
            let ni = self.segs.len() - 1;
            st.new_lines.insert(ni);
            let old = st.line_conn.get(&line).cloned().unwrap_or_default();
            st.line_conn.insert(ni, old);
            st.line_conn.insert(line, vec![Item::Seg(ni)]);
            found_line = Some(ni);
        }

        if let (Some(fl), false) = (found_line, prefer_original_line) {
            // move the connected line found oriented in the direction of our move
            if !self.segs[fl].has(F_NEW) && !self.segs[fl].has(F_BY_DRAG | F_SELECTED) && !self.segs[fl].has(F_SELECTED) && !self.segs[fl].has(F_BY_DRAG) {
                st.changed_lines.insert(fl);
            }
            if self.segs[fl].a == unselected_end {
                self.segs[fl].a = add(self.segs[fl].a, split);
            } else if self.segs[fl].b == unselected_end {
                self.segs[fl].b = add(self.segs[fl].b, split);
            }
            let bend_line = match st.line_conn.get(&fl) {
                Some(v) if v.len() == 1 => match v[0] {
                    Item::Seg(b) => Some(b),
                    _ => None,
                },
                _ => None,
            };
            // "Remerge segments we've created if this is a segment that we've added whose only other connection is also an added segment"
            if self.segs[fl].has(F_NEW) && self.segs[fl].length() == 0.0 && bend_line.is_some_and(|b| self.segs[b].has(F_NEW)) {
                let bend = bend_line.unwrap();
                let bend_end = self.segs[bend].b;
                if start_flagged {
                    self.segs[line].b = bend_end;
                } else {
                    self.segs[line].a = bend_end;
                }
                let (ls, le) = (self.segs[line].a, self.segs[line].b);
                for info in st.special_labels.values_mut() {
                    if info.line == bend || info.line == fl {
                        info.line = line;
                        info.original_start = ls;
                        info.original_end = le;
                    }
                }
                let inherited = st.line_conn.get(&bend).cloned().unwrap_or_default();
                st.line_conn.insert(line, inherited);
                st.line_conn.entry(bend).or_default().clear();
                st.line_conn.entry(fl).or_default().clear();
                self.segs[bend].dead = true;
                self.segs[fl].dead = true;
                st.new_lines.remove(&bend);
                st.new_lines.remove(&fl);
            } else if start_flagged {
                // move the unselected end of our item
                self.segs[line].b = add(self.segs[line].b, split);
            } else {
                self.segs[line].a = add(self.segs[line].a, split);
            }
        } else if self.segs[line].length() == 0.0 {
            // "We didn't find another line to shorten/lengthen, (or we did but it's also zero) so now is a good time to use our existing
            // zero-length original line"
        } else if found_attachment && self.segs[line].is_orthogonal() {
            // "Either no line was at the "right" angle, or this was a junction, pin, sheet, etc. We need to add segments to keep the
            // soon-to-move unselected end connected to these items. To keep our drag selections all the same, we'll move our unselected
            // end point and then put wires between it and its original endpoint."
            let grid = st.grid;
            let x_move_bit = (split.x != 0) as i64;
            let y_move_bit = (split.y != 0) as i64;
            let x_length = (unselected_end.x - selected_end.x).abs();
            let y_length = (unselected_end.y - selected_end.y).abs();
            let x_move = (x_length - *x_bend * grid) * (selected_end.x - unselected_end.x).signum();
            let y_move = (y_length - *y_bend * grid) * (selected_end.y - unselected_end.y).signum();
            let bus = self.segs[line].bus;
            let net = self.segs[line].net.clone();

            // a: from the unselected end to a point one grid short of where the selected end is; b: from there to where the end moves to
            let mut a = Seg::new(Point { x: unselected_end.x + x_move, y: unselected_end.y + y_move }, unselected_end, bus);
            a.flags = F_NEW;
            a.net = net.clone();
            let a_start = a.a;
            self.segs.push(a);
            let ai = self.segs.len() - 1;
            st.new_lines.insert(ai);
            let mut b = Seg::new(Point { x: a_start.x + split.x, y: a_start.y + split.y }, a_start, bus);
            b.flags = F_NEW | F_START;
            b.net = net;
            self.segs.push(b);
            let bi = self.segs.len() - 1;
            st.new_lines.insert(bi);

            *x_bend += y_move_bit;
            *y_bend += x_move_bit;

            // move the unselected end of our item
            let step = Point { x: if split.x != 0 { split.x } else { x_move }, y: if split.y != 0 { split.y } else { y_move } };
            if start_flagged {
                self.segs[line].b = add(self.segs[line].b, step);
            } else {
                self.segs[line].a = add(self.segs[line].a, step);
            }

            // "Update our cache of the connected items. First, attach our drag labels to the line left behind."
            for c in st.line_conn.get(&line).cloned().unwrap_or_default() {
                if !self.is_label_like(c) || !st.special_labels.contains_key(&c) {
                    continue;
                }
                if self.position(c) == selected_end {
                    st.special_labels.get_mut(&c).unwrap().track_moving_end = true;
                } else {
                    let e = st.special_labels.get_mut(&c).unwrap();
                    e.line = ai;
                    e.original_start = self.segs[ai].a;
                    e.original_end = self.segs[ai].b;
                }
            }
            // "We just broke off of the existing items, so replace all of them with our new end connection."
            let inherited = st.line_conn.get(&line).cloned().unwrap_or_default();
            st.line_conn.insert(ai, inherited);
            st.line_conn.entry(bi).or_default().push(Item::Seg(ai));
            st.line_conn.insert(line, vec![Item::Seg(bi)]);
        } else if !found_attachment {
            // original line has no attachments, just move the unselected end
            if start_flagged {
                self.segs[line].b = add(self.segs[line].b, split);
            } else {
                self.segs[line].a = add(self.segs[line].a, split);
            }
        }
    }

    // ----------------------------------------------------------------------------------------------------------------- finish

    /// The tail of `SCH_MOVE_TOOL::finalizeMoveOperation` that edits the sheet: a junction where the items left a point three wires still
    /// meet at, `TrimOverLappingWires`, `AddJunctionsIfNeeded`, `trimDanglingLines` for a drag, and `CleanUp`.
    pub(crate) fn finalize(&mut self, st: Option<&Drag>, internal_points: &[Point]) {
        // "If we move items away from a junction, we _may_ want to add a junction there to denote the state"
        for &p in internal_points {
            if self.is_explicit_junction_needed(p) {
                self.add_junction(p);
            }
        }
        let selection = self.in_selection();
        let mut copy: Vec<Item> = selection.clone();
        if let Some(st) = st {
            copy.extend(st.new_lines.iter().filter(|i| !self.segs[**i].dead).map(|i| Item::Seg(*i)));
            copy.extend(st.changed_lines.iter().filter(|i| !self.segs[**i].dead).map(|i| Item::Seg(*i)));
        }
        copy.sort();
        copy.dedup();
        self.trim_overlapping_wires(&copy);
        self.add_junctions_if_needed(&copy);
        if let Some(st) = st {
            self.trim_dangling_lines(st);
        }
        // "Clear SELECTED_BY_DRAG and other temp flags before CleanUp so that cleanup can properly process all items"
        self.by_drag.clear();
        for s in self.segs.iter_mut() {
            s.flags &= !(F_BY_DRAG | F_NEW);
        }
        self.clean_up();
    }

    /// `SCH_LINE_WIRE_BUS_TOOL::AddJunctionsIfNeeded` / `SCH_SCREEN::GetNeededJunctions`.
    pub(crate) fn add_junctions_if_needed(&mut self, items: &[Item]) {
        let connections: BTreeSet<Point> = self.items().into_iter().flat_map(|it| self.connection_points(it)).collect();
        let mut pts: BTreeSet<Point> = BTreeSet::new();
        for &it in items {
            if !self.is_connectable(it) {
                continue;
            }
            pts.extend(self.connection_points(it));
            // "If the item is a line, we also add any connection points from the rest of the schematic that terminate on the line"
            if let Item::Seg(i) = it {
                if self.segs[i].dead {
                    continue;
                }
                let (a, b) = (self.segs[i].a, self.segs[i].b);
                pts.extend(connections.iter().copied().filter(|&c| on_segment(a, b, c)));
            }
        }
        for p in pts {
            if self.is_explicit_junction_needed(p) {
                self.add_junction(p);
            }
        }
    }

    /// `SCH_LINE_WIRE_BUS_TOOL::TrimOverLappingWires`: a part with two pins that both land on one wire cuts the wire between them.
    pub(crate) fn trim_overlapping_wires(&mut self, items: &[Item]) {
        for &it in items {
            if matches!(it, Item::Seg(_)) || !self.is_connectable(it) {
                continue;
            }
            let pts = self.connection_points(it);
            let lines: Vec<usize> = (0..self.segs.len()).filter(|&i| !self.segs[i].dead && !self.segs[i].bus).collect();
            for li in lines {
                if self.segs[li].dead {
                    continue;
                }
                let on: Vec<Point> = pts.iter().copied().filter(|&p| on_segment(self.segs[li].a, self.segs[li].b, p)).take(3).collect();
                if on.len() == 2 {
                    self.trim_wire(on[0], on[1]);
                }
            }
        }
    }

    /// `SCH_EDIT_FRAME::TrimWire`: remove the part of a wire between `start` and `end` (never a whole wire, never a wire that is moving).
    pub(crate) fn trim_wire(&mut self, start: Point, end: Point) -> bool {
        if start == end {
            return false;
        }
        for li in 0..self.segs.len() {
            let s = &self.segs[li];
            if s.dead || s.bus || s.has(F_SELECTED) || s.has(F_BY_DRAG) {
                continue;
            }
            if !on_segment(s.a, s.b, start) || !on_segment(s.a, s.b, end) {
                continue;
            }
            if (s.a == start && s.b == end) || (s.a == end && s.b == start) {
                continue;
            }
            // break at one end, keep working on the piece that has the other, break again, drop the piece between
            let mut line = li;
            let tail = self.break_seg(line, start);
            if on_segment(self.segs[tail].a, self.segs[tail].b, end) {
                line = tail;
            }
            let tail = self.break_seg(line, end);
            if on_segment(self.segs[tail].a, self.segs[tail].b, start) {
                line = tail;
            }
            self.segs[line].dead = true;
            return true;
        }
        false
    }

    /// `SCH_MOVE_TOOL::trimDanglingLines`: a line the drag made that ended up with both ends free is a stub, and goes.
    pub(crate) fn trim_dangling_lines(&mut self, st: &Drag) {
        self.clean_up();
        let mut danglers: Vec<usize> = Vec::new();
        for i in 0..self.segs.len() {
            let s = &self.segs[i];
            if s.dead || s.has(F_SELECTED) || !s.has(F_NEW) {
                continue;
            }
            let _ = st;
            if self.end_is_dangling(i, s.a) && self.end_is_dangling(i, s.b) {
                danglers.push(i);
            }
        }
        for i in danglers {
            self.segs[i].dead = true;
        }
    }

    /// `SCH_LINE::UpdateDanglingState` for one end: nothing else ends there.
    pub(crate) fn end_is_dangling(&self, line: usize, p: Point) -> bool {
        let bus = self.segs[line].bus;
        for (i, s) in self.segs.iter().enumerate() {
            if i != line && !s.dead && s.bus == bus && s.is_endpoint(p) {
                return false;
            }
        }
        for it in self.items() {
            match it {
                Item::Seg(_) | Item::NoteLine(_) | Item::Text(_) | Item::Sheet(_) => {}
                Item::BusEntry(_) if bus => {}
                other => {
                    if self.is_connected(other, p) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

fn add(p: Point, d: Point) -> Point {
    Point { x: p.x + d.x, y: p.y + d.y }
}

/// `SEG::NearestPoint`.
fn nearest_on_segment(a: Point, b: Point, p: Point) -> Point {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return a;
    }
    let t = (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / len2).clamp(0.0, 1.0);
    Point { x: a.x + (dx * t).round() as i64, y: a.y + (dy * t).round() as i64 }
}

/// `SCH_SHEET_PIN::ConstrainOnEdge( aPos, aAllowEdgeSwitch )`: where a pin dragged to `pos` sits on its sheet's border. `current` is the
/// pin's place now, which decides the edge when edge switching is off.
pub(crate) fn constrain_on_edge(sheet: &eda_model::ir::SheetInstance, pos: Point, allow_edge_switch: bool, current: Point) -> Point {
    #[derive(Clone, Copy, PartialEq)]
    enum Side {
        Top,
        Right,
        Bottom,
        Left,
    }
    let (left, top) = (sheet.at.x, sheet.at.y);
    let (right, bottom) = (sheet.at.x + sheet.size.0, sheet.at.y + sheet.size.1);
    let edge_of = |p: Point| {
        if p.x <= left {
            Side::Left
        } else if p.x >= right {
            Side::Right
        } else if p.y <= top {
            Side::Top
        } else {
            Side::Bottom
        }
    };
    let side = if allow_edge_switch {
        // the nearest of the four edges (`SHAPE_LINE_CHAIN::NearestSegment`)
        let d_top = distance_to_segment(pos, Point { x: left, y: top }, Point { x: right, y: top });
        let d_right = distance_to_segment(pos, Point { x: right, y: top }, Point { x: right, y: bottom });
        let d_bottom = distance_to_segment(pos, Point { x: right, y: bottom }, Point { x: left, y: bottom });
        let d_left = distance_to_segment(pos, Point { x: left, y: bottom }, Point { x: left, y: top });
        let mut best = (d_top, Side::Top);
        for (d, s) in [(d_right, Side::Right), (d_bottom, Side::Bottom), (d_left, Side::Left)] {
            if d < best.0 {
                best = (d, s);
            }
        }
        best.1
    } else {
        edge_of(current)
    };
    match side {
        Side::Left => Point { x: left, y: pos.y.clamp(top, bottom) },
        Side::Right => Point { x: right, y: pos.y.clamp(top, bottom) },
        Side::Top => Point { x: pos.x.clamp(left, right), y: top },
        Side::Bottom => Point { x: pos.x.clamp(left, right), y: bottom },
    }
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let n = nearest_on_segment(a, b, p);
    ((p.x - n.x) as f64).hypot((p.y - n.y) as f64)
}
