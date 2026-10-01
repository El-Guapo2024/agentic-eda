//! A faithful port of `Clipper2Lib/include/clipper2/clipper.engine.h` and
//! `Clipper2Lib/src/clipper.engine.cpp` -- the Vatti-style sweep-line
//! boolean-op engine. Only `Clipper64`/`ClipperBase` is ported (not the
//! `ClipperD` double-precision wrapper): KiCad's `SHAPE_POLY_SET` only ever
//! instantiates `Clipper2Lib::Clipper64`, so `ClipperBase` and `Clipper64`
//! are merged into a single concrete `Clipper64` struct here. Likewise
//! `ReuseableDataContainer64` is not ported: nothing in `shape_poly_set.cpp`
//! constructs one.
//!
//! ## Pointers -> arena indices
//!
//! The original is written in heap-pointer-heavy C++: `Vertex`, `OutPt`,
//! `OutRec` and `Active` are individually `new`-allocated and linked via raw
//! pointers (`prev`/`next`, `owner`, `outrec`, ...), with `nullptr` as the
//! "no link" sentinel. Rust can't safely alias raw pointers like that, so
//! every one of those node types instead lives in an append-only arena
//! (`Vec<T>` on `Clipper64`), addressed by a plain `usize` index
//! (`VertexIdx`/`OutPtIdx`/`OutRecIdx`/`ActiveIdx`), with `Option<Idx>`
//! standing in for `nullptr`. This is a 1:1 structural translation, not a
//! redesign:
//! - every `new Vertex`/`new OutPt`/`new OutRec`/`new Active()` becomes an
//!   arena `push`, returning its index;
//! - every `delete p` becomes nothing (Rust drops the whole arena at the
//!   end); the original's `delete` calls only ever run *after* the deleted
//!   node has been fully unlinked from every list that could reach it, so
//!   leaving the slot (unreachable, un-recycled) in the arena is exactly as
//!   safe as the C++ free -- nothing ever dereferences it again;
//! - pointer equality (`e1 == e2`) becomes index equality;
//! - `ptr->field` becomes `arena[idx].field`.
//!
//! The one place this requires care rather than being mechanical:
//! `minima_list_` is a `vector<unique_ptr<LocalMinima>>` in the original, so
//! `Reset()`'s `std::stable_sort` reorders the *pointers* while the pointees
//! (and any raw `LocalMinima*` captured elsewhere) stay put. A naive
//! `Vec<LocalMinima>` would instead move the *values* on sort, silently
//! invalidating any index captured beforehand. It's safe here only because
//! (verified by reading the call sites) nothing captures a `LocalMinima`
//! index before `Reset()` runs and sorts -- `Active::local_min` is only ever
//! assigned afterwards, inside `InsertLocalMinimaIntoAEL`, which `Reset()`
//! always precedes. `Vertex`/`OutRec`/`OutPt`/`Active` arenas are never
//! reordered (only appended to), so indices into them are stable for the
//! object's full lifetime, same as the original's heap addresses.
//!
//! Every method below carries the original C++ function name in its doc
//! comment for cross-referencing against `clipper.engine.cpp`.

use crate::core::*;

pub type VertexIdx = usize;
pub type OutPtIdx = usize;
pub type OutRecIdx = usize;
pub type ActiveIdx = usize;
pub type LocalMinimaIdx = usize;

/// Note: all clipping operations except for Difference are commutative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipType {
    None,
    Intersection,
    Union,
    Difference,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    Subject,
    Clip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinWith {
    None,
    Left,
    Right,
}

/// `VertexFlags` -- plain bit constants rather than a `bitflags`-style
/// wrapper type, since all the original ever does with them is `&`/`|`.
pub mod vflags {
    pub const NONE: u32 = 0;
    pub const OPEN_START: u32 = 1;
    pub const OPEN_END: u32 = 2;
    pub const LOCAL_MAX: u32 = 4;
    pub const LOCAL_MIN: u32 = 8;
}

#[derive(Debug, Clone)]
pub struct Vertex {
    pub pt: Point64,
    pub next: VertexIdx,
    pub prev: VertexIdx,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct OutPt {
    pub pt: Point64,
    pub next: OutPtIdx,
    pub prev: OutPtIdx,
    pub outrec: OutRecIdx,
    /// Stands in for the original's `HorzSegment* horz`: every call site
    /// only ever tests it for null or sets it non-null (`UpdateHorzSegment`
    /// checks `!hs.left_op->horz` then sets `hs.left_op->horz = &hs`) --
    /// the pointee is never read back through this field -- so a plain
    /// "claimed" flag is behaviourally identical.
    pub horz_claimed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OutRec {
    pub idx: OutRecIdx,
    pub owner: Option<OutRecIdx>,
    pub front_edge: Option<ActiveIdx>,
    pub back_edge: Option<ActiveIdx>,
    pub pts: Option<OutPtIdx>,
    pub polypath: Option<usize>,
    pub splits: Option<Vec<OutRecIdx>>,
    pub recursive_split: Option<OutRecIdx>,
    pub bounds: Rect64,
    pub path: Path64,
    pub is_open: bool,
}

///////////////////////////////////////////////////////////////////
// Important: UP and DOWN here are premised on Y-axis positive down
// displays, which is the orientation used in Clipper's development.
///////////////////////////////////////////////////////////////////
#[derive(Debug, Clone)]
pub struct Active {
    pub bot: Point64,
    pub top: Point64,
    pub curr_x: i64,
    pub dx: f64,
    pub wind_dx: i32,
    pub wind_cnt: i32,
    pub wind_cnt2: i32,
    pub outrec: Option<OutRecIdx>,
    // AEL: 'active edge list' (Vatti's AET) -- edges present in the current scanbeam.
    pub prev_in_ael: Option<ActiveIdx>,
    pub next_in_ael: Option<ActiveIdx>,
    // SEL: 'sorted edge list' (Vatti's ST) -- also reused for horizontal processing.
    pub prev_in_sel: Option<ActiveIdx>,
    pub next_in_sel: Option<ActiveIdx>,
    pub jump: Option<ActiveIdx>,
    pub vertex_top: VertexIdx,
    pub local_min: LocalMinimaIdx,
    pub is_left_bound: bool,
    pub join_with: JoinWith,
}

#[derive(Debug, Clone)]
pub struct LocalMinima {
    pub vertex: VertexIdx,
    pub polytype: PathType,
    pub is_open: bool,
}

#[derive(Debug, Clone)]
pub struct IntersectNode {
    pub pt: Point64,
    pub edge1: ActiveIdx,
    pub edge2: ActiveIdx,
}

#[derive(Debug, Clone)]
pub struct HorzSegment {
    pub left_op: OutPtIdx,
    pub right_op: Option<OutPtIdx>,
    pub left_to_right: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HorzJoin {
    pub op1: OutPtIdx,
    pub op2: OutPtIdx,
}

/// `ZCallback64`: `(e1bot, e1top, e2bot, e2top, &mut pt)`.
pub type ZCallback64 = Box<dyn FnMut(Point64, Point64, Point64, Point64, &mut Point64)>;

// PolyPath / PolyTree ---------------------------------------------------

/// A faithful (if flattened -- see the module doc comment on why an arena
/// is used throughout this port) rendition of `PolyPath64`/`PolyTree64`.
/// `level` is a precomputed `PolyPath::Level()` (0 would be the invisible
/// tree root; every real node here is level >= 1), so `is_hole` is exactly
/// `PolyPath::IsHole()`: `lvl != 0 && lvl % 2 == 0`.
#[derive(Debug, Clone)]
pub struct PolyPathNode {
    pub polygon: Path64,
    pub children: Vec<usize>,
    pub level: u32,
}

impl PolyPathNode {
    pub fn is_hole(&self) -> bool {
        self.level != 0 && self.level.is_multiple_of(2)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PolyTree64 {
    pub nodes: Vec<PolyPathNode>,
    pub roots: Vec<usize>,
}

impl PolyTree64 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.roots.clear();
    }

    pub fn count(&self) -> usize {
        self.roots.len()
    }

    /// `PolyPath64::AddChild` called on the tree root.
    pub fn add_child(&mut self, path: Path64) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PolyPathNode { polygon: path, children: Vec::new(), level: 1 });
        self.roots.push(idx);
        idx
    }

    /// `PolyPath64::AddChild` called on an existing node.
    pub fn add_child_to(&mut self, parent: usize, path: Path64) -> usize {
        let idx = self.nodes.len();
        let level = self.nodes[parent].level + 1;
        self.nodes.push(PolyPathNode { polygon: path, children: Vec::new(), level });
        self.nodes[parent].children.push(idx);
        idx
    }
}

/// `PolyTreeToPaths64`: flattens every node (outlines and holes alike) into
/// a flat `Paths64`, discarding the nesting.
pub fn poly_tree_to_paths64(tree: &PolyTree64) -> Paths64 {
    fn walk(tree: &PolyTree64, idx: usize, out: &mut Paths64) {
        out.push(tree.nodes[idx].polygon.clone());
        for &c in &tree.nodes[idx].children {
            walk(tree, c, out);
        }
    }
    let mut out = Vec::new();
    for &r in &tree.roots {
        walk(tree, r, &mut out);
    }
    out
}

// Free (pointer-free) helpers --------------------------------------------

#[inline]
fn is_odd(val: i32) -> bool {
    val & 1 != 0
}

/*******************************************************************************
  *  Dx:                             0(90deg)                                    *
  *                                  |                                           *
  *               +inf (180deg) <--- o ---> -inf (0deg)                          *
  *******************************************************************************/
#[inline]
fn get_dx(pt1: Point64, pt2: Point64) -> f64 {
    let dy = (pt2.y - pt1.y) as f64;
    if dy != 0.0 {
        (pt2.x - pt1.x) as f64 / dy
    } else if pt2.x > pt1.x {
        f64::MIN
    } else {
        f64::MAX
    }
}

#[inline]
fn top_x(ae: &Active, current_y: i64) -> i64 {
    if current_y == ae.top.y || ae.top.x == ae.bot.x {
        ae.top.x
    } else if current_y == ae.bot.y {
        ae.bot.x
    } else {
        ae.bot.x + (ae.dx * (current_y - ae.bot.y) as f64).round() as i64
    }
}

#[inline]
fn is_horizontal(e: &Active) -> bool {
    e.top.y == e.bot.y
}

#[inline]
fn is_heading_right_horz(e: &Active) -> bool {
    e.dx == f64::MIN
}

#[inline]
fn is_heading_left_horz(e: &Active) -> bool {
    e.dx == f64::MAX
}

#[inline]
fn set_dx(e: &mut Active) {
    e.dx = get_dx(e.bot, e.top);
}

#[inline]
fn is_joined(e: &Active) -> bool {
    e.join_with != JoinWith::None
}

#[inline]
fn area_triangle(pt1: Point64, pt2: Point64, pt3: Point64) -> f64 {
    (pt3.y + pt1.y) as f64 * (pt3.x - pt1.x) as f64
        + (pt1.y + pt2.y) as f64 * (pt1.x - pt2.x) as f64
        + (pt2.y + pt3.y) as f64 * (pt2.x - pt3.x) as f64
}

#[inline]
fn pts_really_close(pt1: Point64, pt2: Point64) -> bool {
    (pt1.x - pt2.x).abs() < 2 && (pt1.y - pt2.y).abs() < 2
}

fn intersect_list_sort(a: &IntersectNode, b: &IntersectNode) -> std::cmp::Ordering {
    // note different inequality tests ...
    if a.pt.y == b.pt.y {
        a.pt.x.cmp(&b.pt.x)
    } else {
        b.pt.y.cmp(&a.pt.y)
    }
}

impl Default for Active {
    fn default() -> Self {
        Active {
            bot: Point64::default(),
            top: Point64::default(),
            curr_x: 0,
            dx: 0.0,
            wind_dx: 1,
            wind_cnt: 0,
            wind_cnt2: 0,
            outrec: None,
            prev_in_ael: None,
            next_in_ael: None,
            prev_in_sel: None,
            next_in_sel: None,
            jump: None,
            vertex_top: 0,
            local_min: 0,
            is_left_bound: false,
            join_with: JoinWith::None,
        }
    }
}

/// `ClipperBase` + `Clipper64` merged into one concrete engine (see the
/// module doc comment for why).
pub struct Clipper64 {
    cliptype: ClipType,
    fillrule: FillRule,
    bot_y: i64,
    minima_list_sorted: bool,
    using_polytree: bool,
    ael_head: Option<ActiveIdx>,
    sel_head: Option<ActiveIdx>,
    minima_list: Vec<LocalMinima>,
    current_locmin_idx: usize,
    vertices: Vec<Vertex>,
    scanline_list: std::collections::BinaryHeap<i64>,
    intersect_nodes: Vec<IntersectNode>,
    horz_seg_list: Vec<HorzSegment>,
    horz_join_list: Vec<HorzJoin>,

    preserve_collinear: bool,
    reverse_solution: bool,
    error_code: i32,
    has_open_paths: bool,
    succeeded: bool,
    outrecs: Vec<OutRec>,
    active_arena: Vec<Active>,
    outpts: Vec<OutPt>,

    pub default_z: i64,
    z_callback: Option<ZCallback64>,
}

impl Default for Clipper64 {
    fn default() -> Self {
        Clipper64 {
            cliptype: ClipType::None,
            fillrule: FillRule::EvenOdd,
            bot_y: 0,
            minima_list_sorted: false,
            using_polytree: false,
            ael_head: None,
            sel_head: None,
            minima_list: Vec::new(),
            current_locmin_idx: 0,
            vertices: Vec::new(),
            scanline_list: std::collections::BinaryHeap::new(),
            intersect_nodes: Vec::new(),
            horz_seg_list: Vec::new(),
            horz_join_list: Vec::new(),
            preserve_collinear: true,
            reverse_solution: false,
            error_code: 0,
            has_open_paths: false,
            succeeded: true,
            outrecs: Vec::new(),
            active_arena: Vec::new(),
            outpts: Vec::new(),
            default_z: 0,
            z_callback: None,
        }
    }
}

impl Clipper64 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error_code(&self) -> i32 {
        self.error_code
    }
    pub fn preserve_collinear(&self) -> bool {
        self.preserve_collinear
    }
    pub fn set_preserve_collinear(&mut self, val: bool) {
        self.preserve_collinear = val;
    }
    pub fn reverse_solution(&self) -> bool {
        self.reverse_solution
    }
    pub fn set_reverse_solution(&mut self, val: bool) {
        self.reverse_solution = val;
    }
    pub fn set_z_callback(&mut self, cb: ZCallback64) {
        self.z_callback = Some(cb);
    }

    // ---- vertex / local-minima construction (`AddLocMin`, `AddPaths_`) ----

    /// `AddLocMin` (both the free-function and identical `ClipperBase`
    /// member overloads in the original collapse to this one method, since
    /// nothing in KiCad's call path needs the `ReuseableDataContainer64`
    /// copy of it).
    fn add_loc_min(&mut self, vert: VertexIdx, polytype: PathType, is_open: bool) {
        // make sure the vertex is added only once ...
        if self.vertices[vert].flags & vflags::LOCAL_MIN != 0 {
            return;
        }
        self.vertices[vert].flags |= vflags::LOCAL_MIN;
        self.minima_list.push(LocalMinima { vertex: vert, polytype, is_open });
    }

    /// `AddPaths_`: builds a circular doubly-linked vertex list per path and
    /// finds/flags its local minima and maxima.
    fn add_paths_(&mut self, paths: &Paths64, polytype: PathType, is_open: bool) {
        for path in paths {
            if path.is_empty() {
                continue;
            }
            let base = self.vertices.len();
            // First pass: dedup consecutive equal points (as the original's
            // `if (prev_v->pt == pt) continue;` does) while appending fresh
            // vertices with a temporary self-loop prev/next.
            let mut pts: Vec<Point64> = Vec::with_capacity(path.len());
            for &pt in path {
                if pts.last() != Some(&pt) {
                    pts.push(pt);
                }
            }
            if !is_open && pts.len() > 1 && pts[0] == *pts.last().unwrap() {
                pts.pop();
            }
            let cnt = pts.len();
            // `if (!prev_v || !prev_v->prev) continue;` -- needs at least 2
            // distinct vertices forming a real prev link.
            if cnt == 0 {
                continue;
            }
            for &pt in &pts {
                self.vertices.push(Vertex { pt, next: base, prev: base, flags: vflags::NONE });
            }
            let last = base + cnt - 1;
            for i in 0..cnt {
                let idx = base + i;
                self.vertices[idx].next = base + (i + 1) % cnt;
                self.vertices[idx].prev = if i == 0 { last } else { idx - 1 };
            }
            if cnt < 2 || (cnt == 2 && !is_open) {
                continue;
            }

            let v0 = base;
            let mut going_up;
            if is_open {
                let mut curr_v = self.vertices[v0].next;
                while curr_v != v0 && self.vertices[curr_v].pt.y == self.vertices[v0].pt.y {
                    curr_v = self.vertices[curr_v].next;
                }
                going_up = self.vertices[curr_v].pt.y <= self.vertices[v0].pt.y;
                if going_up {
                    self.vertices[v0].flags = vflags::OPEN_START;
                    self.add_loc_min(v0, polytype, true);
                } else {
                    self.vertices[v0].flags = vflags::OPEN_START | vflags::LOCAL_MAX;
                }
            } else {
                let mut prev_v = self.vertices[v0].prev;
                while prev_v != v0 && self.vertices[prev_v].pt.y == self.vertices[v0].pt.y {
                    prev_v = self.vertices[prev_v].prev;
                }
                if prev_v == v0 {
                    continue; // only open paths can be completely flat
                }
                going_up = self.vertices[prev_v].pt.y > self.vertices[v0].pt.y;
            }

            let going_up0 = going_up;
            let mut prev_v = v0;
            let mut curr_v = self.vertices[v0].next;
            while curr_v != v0 {
                if self.vertices[curr_v].pt.y > self.vertices[prev_v].pt.y && going_up {
                    self.vertices[prev_v].flags |= vflags::LOCAL_MAX;
                    going_up = false;
                } else if self.vertices[curr_v].pt.y < self.vertices[prev_v].pt.y && !going_up {
                    going_up = true;
                    self.add_loc_min(prev_v, polytype, is_open);
                }
                prev_v = curr_v;
                curr_v = self.vertices[curr_v].next;
            }

            if is_open {
                self.vertices[prev_v].flags |= vflags::OPEN_END;
                if going_up {
                    self.vertices[prev_v].flags |= vflags::LOCAL_MAX;
                } else {
                    self.add_loc_min(prev_v, polytype, is_open);
                }
            } else if going_up != going_up0 {
                if going_up0 {
                    self.add_loc_min(prev_v, polytype, false);
                } else {
                    self.vertices[prev_v].flags |= vflags::LOCAL_MAX;
                }
            }
        }
    }

    /// `ClipperBase::AddPath`.
    pub fn add_path(&mut self, path: &Path64, polytype: PathType, is_open: bool) {
        self.add_paths(std::slice::from_ref(path), polytype, is_open);
    }

    /// `ClipperBase::AddPaths`.
    pub fn add_paths(&mut self, paths: &[Path64], polytype: PathType, is_open: bool) {
        if is_open {
            self.has_open_paths = true;
        }
        self.minima_list_sorted = false;
        let owned: Paths64 = paths.to_vec();
        self.add_paths_(&owned, polytype, is_open);
    }

    pub fn add_subject(&mut self, subjects: &[Path64]) {
        self.add_paths(subjects, PathType::Subject, false);
    }
    pub fn add_open_subject(&mut self, open_subjects: &[Path64]) {
        self.add_paths(open_subjects, PathType::Subject, true);
    }
    pub fn add_clip(&mut self, clips: &[Path64]) {
        self.add_paths(clips, PathType::Clip, false);
    }

    // ---- small query helpers over the arenas ----

    #[inline]
    fn is_hot_edge(&self, e: ActiveIdx) -> bool {
        self.active_arena[e].outrec.is_some()
    }

    #[inline]
    fn is_open(&self, e: ActiveIdx) -> bool {
        self.minima_list[self.active_arena[e].local_min].is_open
    }

    #[inline]
    fn is_open_end_vertex(&self, v: VertexIdx) -> bool {
        self.vertices[v].flags & (vflags::OPEN_START | vflags::OPEN_END) != 0
    }

    #[inline]
    fn is_open_end(&self, ae: ActiveIdx) -> bool {
        self.is_open_end_vertex(self.active_arena[ae].vertex_top)
    }

    fn get_prev_hot_edge(&self, e: ActiveIdx) -> Option<ActiveIdx> {
        let mut prev = self.active_arena[e].prev_in_ael;
        while let Some(p) = prev {
            if self.is_open(p) || !self.is_hot_edge(p) {
                prev = self.active_arena[p].prev_in_ael;
            } else {
                break;
            }
        }
        prev
    }

    #[inline]
    fn is_front(&self, e: ActiveIdx) -> bool {
        let outrec = self.active_arena[e].outrec.unwrap();
        self.outrecs[outrec].front_edge == Some(e)
    }

    #[inline]
    fn get_poly_type(&self, e: ActiveIdx) -> PathType {
        self.minima_list[self.active_arena[e].local_min].polytype
    }

    #[inline]
    fn is_same_poly_type(&self, e1: ActiveIdx, e2: ActiveIdx) -> bool {
        self.get_poly_type(e1) == self.get_poly_type(e2)
    }

    /// `NextVertex`.
    fn next_vertex(&self, e: ActiveIdx) -> VertexIdx {
        let a = &self.active_arena[e];
        let vt = &self.vertices[a.vertex_top];
        if a.wind_dx > 0 {
            vt.next
        } else {
            vt.prev
        }
    }

    /// `PrevPrevVertex`: the (inverted Y-axis) top of the alternate bound,
    /// useful during edge insertion.
    fn prev_prev_vertex(&self, ae: ActiveIdx) -> VertexIdx {
        let a = &self.active_arena[ae];
        if a.wind_dx > 0 {
            let p = self.vertices[a.vertex_top].prev;
            self.vertices[p].prev
        } else {
            let n = self.vertices[a.vertex_top].next;
            self.vertices[n].next
        }
    }

    #[inline]
    fn is_maxima_vertex(&self, v: VertexIdx) -> bool {
        self.vertices[v].flags & vflags::LOCAL_MAX != 0
    }

    #[inline]
    fn is_maxima(&self, e: ActiveIdx) -> bool {
        self.is_maxima_vertex(self.active_arena[e].vertex_top)
    }

    fn get_curr_ymaxima_vertex_open(&self, e: ActiveIdx) -> Option<VertexIdx> {
        let a = &self.active_arena[e];
        let mut result = a.vertex_top;
        if a.wind_dx > 0 {
            loop {
                let n = self.vertices[result].next;
                if self.vertices[n].pt.y == self.vertices[result].pt.y
                    && self.vertices[result].flags & (vflags::OPEN_END | vflags::LOCAL_MAX) == 0
                {
                    result = n;
                } else {
                    break;
                }
            }
        } else {
            loop {
                let p = self.vertices[result].prev;
                if self.vertices[p].pt.y == self.vertices[result].pt.y
                    && self.vertices[result].flags & (vflags::OPEN_END | vflags::LOCAL_MAX) == 0
                {
                    result = p;
                } else {
                    break;
                }
            }
        }
        if !self.is_maxima_vertex(result) {
            None
        } else {
            Some(result)
        }
    }

    fn get_curr_ymaxima_vertex(&self, e: ActiveIdx) -> Option<VertexIdx> {
        let a = &self.active_arena[e];
        let mut result = a.vertex_top;
        if a.wind_dx > 0 {
            while self.vertices[self.vertices[result].next].pt.y == self.vertices[result].pt.y {
                result = self.vertices[result].next;
            }
        } else {
            while self.vertices[self.vertices[result].prev].pt.y == self.vertices[result].pt.y {
                result = self.vertices[result].prev;
            }
        }
        if !self.is_maxima_vertex(result) {
            None
        } else {
            Some(result)
        }
    }

    fn get_maxima_pair(&self, e: ActiveIdx) -> Option<ActiveIdx> {
        let vt = self.active_arena[e].vertex_top;
        let mut e2 = self.active_arena[e].next_in_ael;
        while let Some(cur) = e2 {
            if self.active_arena[cur].vertex_top == vt {
                return Some(cur);
            }
            e2 = self.active_arena[cur].next_in_ael;
        }
        None
    }

    // ---- OutPt / OutRec helpers ----

    /// `PointCount`: unused by the engine itself in upstream Clipper2
    /// either (no call site in `clipper.engine.cpp`) -- ported anyway for
    /// completeness/fidelity, kept available for callers (e.g. tests).
    #[allow(dead_code)]
    pub fn point_count(&self, op: OutPtIdx) -> i32 {
        let mut op2 = self.outpts[op].next;
        let mut cnt = 1;
        while op2 != op {
            op2 = self.outpts[op2].next;
            cnt += 1;
        }
        cnt
    }

    /// `DuplicateOp`.
    fn duplicate_op(&mut self, op: OutPtIdx, insert_after: bool) -> OutPtIdx {
        let pt = self.outpts[op].pt;
        let outrec = self.outpts[op].outrec;
        let result = self.outpts.len();
        self.outpts.push(OutPt { pt, next: result, prev: result, outrec, horz_claimed: false });
        if insert_after {
            let n = self.outpts[op].next;
            self.outpts[result].next = n;
            self.outpts[n].prev = result;
            self.outpts[result].prev = op;
            self.outpts[op].next = result;
        } else {
            let p = self.outpts[op].prev;
            self.outpts[result].prev = p;
            self.outpts[p].next = result;
            self.outpts[result].next = op;
            self.outpts[op].prev = result;
        }
        result
    }

    /// `DisposeOutPt`: unlinks `op`; the slot itself is simply abandoned in
    /// the arena (see the module doc comment).
    fn dispose_out_pt(&mut self, op: OutPtIdx) -> OutPtIdx {
        let result = self.outpts[op].next;
        let (p, n) = (self.outpts[op].prev, self.outpts[op].next);
        self.outpts[p].next = n;
        self.outpts[n].prev = p;
        result
    }

    /// `DisposeOutPts`: frees every node of `outrec`'s circular `OutPt`
    /// list. In the arena model (see the module doc comment) there's
    /// nothing to actually free -- the nodes just become unreachable --
    /// so this reduces to clearing the head pointer.
    #[inline]
    fn dispose_out_pts(&mut self, outrec: OutRecIdx) {
        self.outrecs[outrec].pts = None;
    }

    #[inline]
    fn set_sides(&mut self, outrec: OutRecIdx, start_edge: ActiveIdx, end_edge: ActiveIdx) {
        self.outrecs[outrec].front_edge = Some(start_edge);
        self.outrecs[outrec].back_edge = Some(end_edge);
    }

    fn swap_outrecs(&mut self, e1: ActiveIdx, e2: ActiveIdx) {
        let or1 = self.active_arena[e1].outrec;
        let or2 = self.active_arena[e2].outrec;
        if or1 == or2 {
            if let Some(or1) = or1 {
                let (fe, be) = (self.outrecs[or1].front_edge, self.outrecs[or1].back_edge);
                self.outrecs[or1].front_edge = be;
                self.outrecs[or1].back_edge = fe;
            }
            return;
        }
        if let Some(or1) = or1 {
            if self.outrecs[or1].front_edge == Some(e1) {
                self.outrecs[or1].front_edge = Some(e2);
            } else {
                self.outrecs[or1].back_edge = Some(e2);
            }
        }
        if let Some(or2) = or2 {
            if self.outrecs[or2].front_edge == Some(e2) {
                self.outrecs[or2].front_edge = Some(e1);
            } else {
                self.outrecs[or2].back_edge = Some(e1);
            }
        }
        self.active_arena[e1].outrec = or2;
        self.active_arena[e2].outrec = or1;
    }

    /// `Area(OutPt*)` (the shoelace-over-the-circular-outpt-list overload;
    /// named `out_pt_area` here to avoid clashing with `core::area`).
    fn out_pt_area(&self, op: OutPtIdx) -> f64 {
        let mut result = 0.0;
        let mut op2 = op;
        loop {
            let prev = self.outpts[op2].prev;
            result += (self.outpts[prev].pt.y + self.outpts[op2].pt.y) as f64
                * (self.outpts[prev].pt.x - self.outpts[op2].pt.x) as f64;
            op2 = self.outpts[op2].next;
            if op2 == op {
                break;
            }
        }
        result * 0.5
    }

    /// `ReverseOutPts`: also unused within `clipper.engine.cpp` itself
    /// (no call site) -- ported for completeness/fidelity.
    // `self.outpts[op1].next`/`.prev` are two fields of the *same* indexed
    // element, so `mem::swap(&mut a[i].x, &mut a[i].y)` doesn't borrow-check
    // (two overlapping `IndexMut` calls); the manual swap clippy flags is
    // the only way to do this through a `Vec`-backed arena.
    #[allow(dead_code, clippy::manual_swap)]
    fn reverse_out_pts(&mut self, op: OutPtIdx) {
        let mut op1 = op;
        loop {
            let op2 = self.outpts[op1].next;
            self.outpts[op1].next = self.outpts[op1].prev;
            self.outpts[op1].prev = op2;
            op1 = op2;
            if op1 == op {
                break;
            }
        }
    }

    fn swap_sides(&mut self, outrec: OutRecIdx) {
        let (fe, be) = (self.outrecs[outrec].front_edge, self.outrecs[outrec].back_edge);
        self.outrecs[outrec].front_edge = be;
        self.outrecs[outrec].back_edge = fe;
        let pts = self.outrecs[outrec].pts.unwrap();
        self.outrecs[outrec].pts = Some(self.outpts[pts].next);
    }

    /// Identical logic to `swap_sides` -- the original really does define
    /// both `SwapSides` and `SwapFrontBackSides` with the same body.
    #[inline]
    fn swap_front_back_sides(&mut self, outrec: OutRecIdx) {
        self.swap_sides(outrec);
    }

    fn get_real_outrec(&self, outrec: Option<OutRecIdx>) -> Option<OutRecIdx> {
        let mut o = outrec;
        while let Some(idx) = o {
            if self.outrecs[idx].pts.is_some() {
                break;
            }
            o = self.outrecs[idx].owner;
        }
        o
    }

    fn is_valid_owner(&self, outrec: OutRecIdx, test_owner: Option<OutRecIdx>) -> bool {
        // prevent outrec owning itself either directly or indirectly
        let mut t = test_owner;
        while let Some(idx) = t {
            if idx == outrec {
                break;
            }
            t = self.outrecs[idx].owner;
        }
        t.is_none()
    }

    fn uncouple_outrec(&mut self, ae: ActiveIdx) {
        let outrec = match self.active_arena[ae].outrec {
            Some(o) => o,
            None => return,
        };
        if let Some(fe) = self.outrecs[outrec].front_edge {
            self.active_arena[fe].outrec = None;
        }
        if let Some(be) = self.outrecs[outrec].back_edge {
            self.active_arena[be].outrec = None;
        }
        self.outrecs[outrec].front_edge = None;
        self.outrecs[outrec].back_edge = None;
    }

    fn is_very_small_triangle(&self, op: OutPtIdx) -> bool {
        let n = self.outpts[op].next;
        let p = self.outpts[op].prev;
        self.outpts[n].next == p
            && (pts_really_close(self.outpts[p].pt, self.outpts[n].pt)
                || pts_really_close(self.outpts[op].pt, self.outpts[n].pt)
                || pts_really_close(self.outpts[op].pt, self.outpts[p].pt))
    }

    fn is_valid_closed_path(&self, op: Option<OutPtIdx>) -> bool {
        match op {
            None => false,
            Some(o) => {
                let n = self.outpts[o].next;
                let p = self.outpts[o].prev;
                n != o && n != p && !self.is_very_small_triangle(o)
            }
        }
    }

    #[inline]
    fn outrec_is_ascending(&self, hot_edge: ActiveIdx) -> bool {
        let outrec = self.active_arena[hot_edge].outrec.unwrap();
        self.outrecs[outrec].front_edge == Some(hot_edge)
    }

    #[inline]
    fn edges_adjacent_in_ael(&self, inode: &IntersectNode) -> bool {
        self.active_arena[inode.edge1].next_in_ael == Some(inode.edge2)
            || self.active_arena[inode.edge1].prev_in_ael == Some(inode.edge2)
    }

    fn set_owner(&mut self, outrec: OutRecIdx, new_owner: OutRecIdx) {
        // precondition1: new_owner is never null
        loop {
            let owner_owner = self.outrecs[new_owner].owner;
            match owner_owner {
                Some(oo) if self.outrecs[oo].pts.is_none() => {
                    self.outrecs[new_owner].owner = self.outrecs[oo].owner;
                }
                _ => break,
            }
        }
        let mut tmp = Some(new_owner);
        while let Some(idx) = tmp {
            if idx == outrec {
                break;
            }
            tmp = self.outrecs[idx].owner;
        }
        if tmp.is_some() {
            self.outrecs[new_owner].owner = self.outrecs[outrec].owner;
        }
        self.outrecs[outrec].owner = Some(new_owner);
    }

    /// `PointInOpPolygon`: the `OutPt`-circular-list analogue of
    /// `core::point_in_polygon`.
    fn point_in_op_polygon(&self, pt: Point64, op0: OutPtIdx) -> PointInPolygonResult {
        let n0 = self.outpts[op0].next;
        if op0 == n0 || self.outpts[op0].prev == n0 {
            return PointInPolygonResult::IsOutside;
        }

        let mut op = op0;
        let op2_start = op0;
        loop {
            if self.outpts[op].pt.y != pt.y {
                break;
            }
            op = self.outpts[op].next;
            if op == op2_start {
                break;
            }
        }
        if self.outpts[op].pt.y == pt.y {
            // not a proper polygon
            return PointInPolygonResult::IsOutside;
        }

        let mut is_above = self.outpts[op].pt.y < pt.y;
        let starting_above = is_above;
        let mut val = 0i32;
        let mut op2 = self.outpts[op].next;
        while op2 != op {
            if is_above {
                while op2 != op && self.outpts[op2].pt.y < pt.y {
                    op2 = self.outpts[op2].next;
                }
            } else {
                while op2 != op && self.outpts[op2].pt.y > pt.y {
                    op2 = self.outpts[op2].next;
                }
            }
            if op2 == op {
                break;
            }

            // must have touched or crossed the pt.y horizontal, an even
            // number of times
            if self.outpts[op2].pt.y == pt.y {
                let prev = self.outpts[op2].prev;
                if self.outpts[op2].pt.x == pt.x
                    || (self.outpts[op2].pt.y == self.outpts[prev].pt.y
                        && (pt.x < self.outpts[prev].pt.x) != (pt.x < self.outpts[op2].pt.x))
                {
                    return PointInPolygonResult::IsOn;
                }
                op2 = self.outpts[op2].next;
                if op2 == op {
                    break;
                }
                continue;
            }

            let prev = self.outpts[op2].prev;
            if pt.x < self.outpts[op2].pt.x && pt.x < self.outpts[prev].pt.x {
                // only interested in edges crossing on the left
            } else if pt.x > self.outpts[prev].pt.x && pt.x > self.outpts[op2].pt.x {
                val = 1 - val;
            } else {
                let d = cross_product(self.outpts[prev].pt, self.outpts[op2].pt, pt);
                if d == 0.0 {
                    return PointInPolygonResult::IsOn;
                }
                if (d < 0.0) == is_above {
                    val = 1 - val;
                }
            }
            is_above = !is_above;
            op2 = self.outpts[op2].next;
        }

        if is_above != starting_above {
            let prev = self.outpts[op2].prev;
            let d = cross_product(self.outpts[prev].pt, self.outpts[op2].pt, pt);
            if d == 0.0 {
                return PointInPolygonResult::IsOn;
            }
            if (d < 0.0) == is_above {
                val = 1 - val;
            }
        }

        if val == 0 {
            PointInPolygonResult::IsOutside
        } else {
            PointInPolygonResult::IsInside
        }
    }

    /// `GetCleanPath`: strips collinear/axis-redundant points from the
    /// `OutPt` circular list for the `Path1InsidePath2` fallback midpoint
    /// test.
    fn get_clean_path(&self, op: OutPtIdx) -> Path64 {
        let mut op2 = op;
        loop {
            let n = self.outpts[op2].next;
            if n == op {
                break;
            }
            let p = self.outpts[op2].prev;
            let same_x = self.outpts[op2].pt.x == self.outpts[n].pt.x && self.outpts[op2].pt.x == self.outpts[p].pt.x;
            let same_y = self.outpts[op2].pt.y == self.outpts[n].pt.y && self.outpts[op2].pt.y == self.outpts[p].pt.y;
            if same_x || same_y {
                op2 = n;
            } else {
                break;
            }
        }
        let mut result = vec![self.outpts[op2].pt];
        let mut prev_op = op2;
        op2 = self.outpts[op2].next;
        while op2 != op {
            // NB: `op` here matches the original's loop-until-`op` (the
            // *adjusted* start), not the original caller's argument name.
            let n = self.outpts[op2].next;
            let same_x = self.outpts[op2].pt.x != self.outpts[n].pt.x || self.outpts[op2].pt.x != self.outpts[prev_op].pt.x;
            let same_y = self.outpts[op2].pt.y != self.outpts[n].pt.y || self.outpts[op2].pt.y != self.outpts[prev_op].pt.y;
            if same_x && same_y {
                result.push(self.outpts[op2].pt);
                prev_op = op2;
            }
            op2 = n;
        }
        result
    }

    fn path1_inside_path2(&self, op1: OutPtIdx, op2_: OutPtIdx) -> bool {
        // we need to make some accommodation for rounding errors, so we
        // won't jump if the first vertex is found outside
        let mut outside_cnt = 0i32;
        let mut op = op1;
        loop {
            match self.point_in_op_polygon(self.outpts[op].pt, op2_) {
                PointInPolygonResult::IsOutside => outside_cnt += 1,
                PointInPolygonResult::IsInside => outside_cnt -= 1,
                PointInPolygonResult::IsOn => {}
            }
            op = self.outpts[op].next;
            if op == op1 || outside_cnt.abs() >= 2 {
                break;
            }
        }
        if outside_cnt.abs() > 1 {
            return outside_cnt < 0;
        }
        // since path1's location is still equivocal, check its midpoint
        let clean1 = self.get_clean_path(op1);
        let mp = get_bounds(&clean1).mid_point();
        let clean2 = self.get_clean_path(op2_);
        point_in_polygon(mp, &clean2) != PointInPolygonResult::IsOutside
    }

    // ---- AEL / SEL list manipulation ----

    fn extract_from_sel(&mut self, ae: ActiveIdx) -> Option<ActiveIdx> {
        let res = self.active_arena[ae].next_in_sel;
        if let Some(r) = res {
            self.active_arena[r].prev_in_sel = self.active_arena[ae].prev_in_sel;
        }
        let prev = self.active_arena[ae].prev_in_sel.expect("extract_from_sel: ae has no prev_in_sel");
        self.active_arena[prev].next_in_sel = res;
        res
    }

    fn insert1_before2_in_sel(&mut self, ae1: ActiveIdx, ae2: ActiveIdx) {
        self.active_arena[ae1].prev_in_sel = self.active_arena[ae2].prev_in_sel;
        if let Some(p) = self.active_arena[ae1].prev_in_sel {
            self.active_arena[p].next_in_sel = Some(ae1);
        }
        self.active_arena[ae1].next_in_sel = Some(ae2);
        self.active_arena[ae2].prev_in_sel = Some(ae1);
    }

    /// `IsValidAelOrder`.
    // The two branches below really are identical in upstream too (two
    // different conditions that happen to return the same expression) --
    // kept exactly as-is for fidelity rather than merged with an `||`.
    #[allow(clippy::if_same_then_else)]
    fn is_valid_ael_order(&self, resident: ActiveIdx, newcomer: ActiveIdx) -> bool {
        let (r, n) = (&self.active_arena[resident], &self.active_arena[newcomer]);
        if n.curr_x != r.curr_x {
            return n.curr_x > r.curr_x;
        }

        // get the turning direction  a1.top, a2.bot, a2.top
        let d = cross_product(r.top, n.bot, n.top);
        if d != 0.0 {
            return d < 0.0;
        }

        // edges must be collinear to get here; for starting open paths,
        // place them according to the direction they're about to turn
        if !self.is_maxima(resident) && r.top.y > n.top.y {
            let nv = self.next_vertex(resident);
            return cross_product(n.bot, r.top, self.vertices[nv].pt) <= 0.0;
        } else if !self.is_maxima(newcomer) && n.top.y > r.top.y {
            let nv = self.next_vertex(newcomer);
            return cross_product(n.bot, n.top, self.vertices[nv].pt) >= 0.0;
        }

        let y = n.bot.y;
        let newcomer_is_left = n.is_left_bound;

        let r_local_min_vertex_y = self.vertices[self.minima_list[r.local_min].vertex].pt.y;
        if r.bot.y != y || r_local_min_vertex_y != y {
            return newcomer_is_left;
        } else if r.is_left_bound != newcomer_is_left {
            return newcomer_is_left;
        }

        let r_ppv = self.prev_prev_vertex(resident);
        if cross_product(self.vertices[r_ppv].pt, r.bot, r.top) == 0.0 {
            true
        } else {
            let n_ppv = self.prev_prev_vertex(newcomer);
            (cross_product(self.vertices[r_ppv].pt, n.bot, self.vertices[n_ppv].pt) > 0.0) == newcomer_is_left
        }
    }

    fn insert_left_edge(&mut self, e: ActiveIdx) {
        match self.ael_head {
            None => {
                self.active_arena[e].prev_in_ael = None;
                self.active_arena[e].next_in_ael = None;
                self.ael_head = Some(e);
            }
            Some(head) if !self.is_valid_ael_order(head, e) => {
                self.active_arena[e].prev_in_ael = None;
                self.active_arena[e].next_in_ael = Some(head);
                self.active_arena[head].prev_in_ael = Some(e);
                self.ael_head = Some(e);
            }
            Some(head) => {
                let mut e2 = head;
                while let Some(n) = self.active_arena[e2].next_in_ael {
                    if self.is_valid_ael_order(n, e) {
                        e2 = n;
                    } else {
                        break;
                    }
                }
                if self.active_arena[e2].join_with == JoinWith::Right {
                    e2 = self.active_arena[e2].next_in_ael.unwrap();
                }
                self.active_arena[e].next_in_ael = self.active_arena[e2].next_in_ael;
                if let Some(n) = self.active_arena[e2].next_in_ael {
                    self.active_arena[n].prev_in_ael = Some(e);
                }
                self.active_arena[e].prev_in_ael = Some(e2);
                self.active_arena[e2].next_in_ael = Some(e);
            }
        }
    }

    fn insert_right_edge(&mut self, e: ActiveIdx, e2: ActiveIdx) {
        self.active_arena[e2].next_in_ael = self.active_arena[e].next_in_ael;
        if let Some(n) = self.active_arena[e].next_in_ael {
            self.active_arena[n].prev_in_ael = Some(e2);
        }
        self.active_arena[e2].prev_in_ael = Some(e);
        self.active_arena[e].next_in_ael = Some(e2);
    }

    #[inline]
    fn push_horz(&mut self, e: ActiveIdx) {
        self.active_arena[e].next_in_sel = self.sel_head;
        self.sel_head = Some(e);
    }

    #[inline]
    fn pop_horz(&mut self) -> Option<ActiveIdx> {
        let e = self.sel_head;
        if let Some(e) = e {
            self.sel_head = self.active_arena[e].next_in_sel;
        }
        e
    }

    fn delete_from_ael(&mut self, e: ActiveIdx) {
        let prev = self.active_arena[e].prev_in_ael;
        let next = self.active_arena[e].next_in_ael;
        if prev.is_none() && next.is_none() && self.ael_head != Some(e) {
            return; // already deleted
        }
        match prev {
            Some(p) => self.active_arena[p].next_in_ael = next,
            None => self.ael_head = next,
        }
        if let Some(n) = next {
            self.active_arena[n].prev_in_ael = prev;
        }
        // nb: the original `delete`s the node here; we just leave it
        // unlinked and unreachable in the arena (see module doc comment).
    }

    fn swap_positions_in_ael(&mut self, e1: ActiveIdx, e2: ActiveIdx) {
        // precondition: e1 must be immediately to the left of e2
        let next = self.active_arena[e2].next_in_ael;
        if let Some(n) = next {
            self.active_arena[n].prev_in_ael = Some(e1);
        }
        let prev = self.active_arena[e1].prev_in_ael;
        if let Some(p) = prev {
            self.active_arena[p].next_in_ael = Some(e2);
        }
        self.active_arena[e2].prev_in_ael = prev;
        self.active_arena[e2].next_in_ael = Some(e1);
        self.active_arena[e1].prev_in_ael = Some(e2);
        self.active_arena[e1].next_in_ael = next;
        if self.active_arena[e2].prev_in_ael.is_none() {
            self.ael_head = Some(e2);
        }
    }

    // ---- lifecycle: Reset / Clear / CleanUp, scanline & local-minima queues ----

    /// `ClipperBase::CleanUp`: unlike `Clear`, preserves added paths
    /// (vertices/local minima) -- only the per-execution state is wiped.
    fn clean_up(&mut self) {
        self.ael_head = None;
        self.sel_head = None;
        self.active_arena.clear();
        self.scanline_list.clear();
        self.intersect_nodes.clear();
        self.outrecs.clear();
        self.outpts.clear();
        self.horz_seg_list.clear();
        self.horz_join_list.clear();
    }

    pub fn clear(&mut self) {
        self.clean_up();
        self.minima_list.clear();
        self.vertices.clear();
        self.current_locmin_idx = 0;
        self.minima_list_sorted = false;
        self.has_open_paths = false;
    }

    fn reset(&mut self) {
        if !self.minima_list_sorted {
            // `LocMinSorter`: descending y, then (on ties) descending x.
            // The original uses `std::stable_sort` directly on the
            // `vector<unique_ptr<LocalMinima>>`; sorting via an index
            // permutation computed from `self.vertices` first (rather than
            // a comparator closure that would need to borrow `self.vertices`
            // *while* `self.minima_list` is mutably borrowed for the sort)
            // sidesteps the borrow conflict while keeping the same stable
            // tie-break order.
            let keys: Vec<(i64, i64)> =
                self.minima_list.iter().map(|lm| (self.vertices[lm.vertex].pt.y, self.vertices[lm.vertex].pt.x)).collect();
            let mut order: Vec<usize> = (0..self.minima_list.len()).collect();
            order.sort_by(|&a, &b| {
                if keys[b].0 != keys[a].0 {
                    keys[b].0.cmp(&keys[a].0)
                } else {
                    keys[b].1.cmp(&keys[a].1)
                }
            });
            let old = std::mem::take(&mut self.minima_list);
            self.minima_list = order.into_iter().map(|i| old[i].clone()).collect();
            self.minima_list_sorted = true;
        }
        for lm in self.minima_list.iter().rev() {
            self.scanline_list.push(self.vertices[lm.vertex].pt.y);
        }
        self.current_locmin_idx = 0;
        self.ael_head = None;
        self.sel_head = None;
        self.succeeded = true;
    }

    /// `ClipperBase::SetZ` (`USINGZ`): called at every intersection so a
    /// user `ZCallback64` can tag the new point. Prioritizes subject over
    /// clip vertices, matching the original exactly.
    fn set_z(&mut self, e1: ActiveIdx, e2: ActiveIdx, ip: &mut Point64) {
        if self.z_callback.is_none() {
            return;
        }
        let (e1bot, e1top) = (self.active_arena[e1].bot, self.active_arena[e1].top);
        let (e2bot, e2top) = (self.active_arena[e2].bot, self.active_arena[e2].top);
        let default_z = self.default_z;
        if self.get_poly_type(e1) == PathType::Subject {
            if *ip == e1bot {
                ip.z = e1bot.z;
            } else if *ip == e1top {
                ip.z = e1top.z;
            } else if *ip == e2bot {
                ip.z = e2bot.z;
            } else if *ip == e2top {
                ip.z = e2top.z;
            } else {
                ip.z = default_z;
            }
            if let Some(cb) = self.z_callback.as_mut() {
                cb(e1bot, e1top, e2bot, e2top, ip);
            }
        } else {
            if *ip == e2bot {
                ip.z = e2bot.z;
            } else if *ip == e2top {
                ip.z = e2top.z;
            } else if *ip == e1bot {
                ip.z = e1bot.z;
            } else if *ip == e1top {
                ip.z = e1top.z;
            } else {
                ip.z = default_z;
            }
            if let Some(cb) = self.z_callback.as_mut() {
                cb(e2bot, e2top, e1bot, e1top, ip);
            }
        }
    }

    #[inline]
    fn insert_scanline(&mut self, y: i64) {
        self.scanline_list.push(y);
    }

    fn pop_scanline(&mut self) -> Option<i64> {
        let y = self.scanline_list.pop()?;
        while let Some(&top) = self.scanline_list.peek() {
            if top == y {
                self.scanline_list.pop();
            } else {
                break;
            }
        }
        Some(y)
    }

    fn pop_local_minima(&mut self, y: i64) -> Option<LocalMinimaIdx> {
        if self.current_locmin_idx >= self.minima_list.len() {
            return None;
        }
        if self.vertices[self.minima_list[self.current_locmin_idx].vertex].pt.y != y {
            return None;
        }
        let idx = self.current_locmin_idx;
        self.current_locmin_idx += 1;
        Some(idx)
    }

    // ---- winding counts & contribution tests ----

    fn is_contributing_closed(&self, e: ActiveIdx) -> bool {
        let wind_cnt = self.active_arena[e].wind_cnt;
        match self.fillrule {
            FillRule::EvenOdd => {}
            FillRule::NonZero => {
                if wind_cnt.abs() != 1 {
                    return false;
                }
            }
            FillRule::Positive => {
                if wind_cnt != 1 {
                    return false;
                }
            }
            FillRule::Negative => {
                if wind_cnt != -1 {
                    return false;
                }
            }
        }

        let wind_cnt2 = self.active_arena[e].wind_cnt2;
        match self.cliptype {
            ClipType::None => false,
            ClipType::Intersection => match self.fillrule {
                FillRule::Positive => wind_cnt2 > 0,
                FillRule::Negative => wind_cnt2 < 0,
                _ => wind_cnt2 != 0,
            },
            ClipType::Union => match self.fillrule {
                FillRule::Positive => wind_cnt2 <= 0,
                FillRule::Negative => wind_cnt2 >= 0,
                _ => wind_cnt2 == 0,
            },
            ClipType::Difference => {
                let result = match self.fillrule {
                    FillRule::Positive => wind_cnt2 <= 0,
                    FillRule::Negative => wind_cnt2 >= 0,
                    _ => wind_cnt2 == 0,
                };
                if self.get_poly_type(e) == PathType::Subject {
                    result
                } else {
                    !result
                }
            }
            ClipType::Xor => true,
        }
    }

    fn is_contributing_open(&self, e: ActiveIdx) -> bool {
        let (wind_cnt, wind_cnt2) = (self.active_arena[e].wind_cnt, self.active_arena[e].wind_cnt2);
        let (is_in_clip, is_in_subj) = match self.fillrule {
            FillRule::Positive => (wind_cnt2 > 0, wind_cnt > 0),
            FillRule::Negative => (wind_cnt2 < 0, wind_cnt < 0),
            _ => (wind_cnt2 != 0, wind_cnt != 0),
        };

        match self.cliptype {
            ClipType::Intersection => is_in_clip,
            ClipType::Union => !is_in_subj && !is_in_clip,
            _ => !is_in_clip,
        }
    }

    fn set_wind_count_for_closed_path_edge(&mut self, e: ActiveIdx) {
        // Wind counts refer to polygon regions not edges, so here an edge's
        // WindCnt indicates the higher of the wind counts for the two
        // regions touching the edge. (Adjacent regions can only ever have
        // their wind counts differ by one; open paths have no meaningful
        // wind directions or counts.)
        let pt = self.get_poly_type(e);
        let mut e2 = self.active_arena[e].prev_in_ael;
        while let Some(cur) = e2 {
            if self.get_poly_type(cur) != pt || self.is_open(cur) {
                e2 = self.active_arena[cur].prev_in_ael;
            } else {
                break;
            }
        }

        let mut e2_next: Option<ActiveIdx>;
        match e2 {
            None => {
                self.active_arena[e].wind_cnt = self.active_arena[e].wind_dx;
                e2_next = self.ael_head;
            }
            Some(e2i) if self.fillrule == FillRule::EvenOdd => {
                self.active_arena[e].wind_cnt = self.active_arena[e].wind_dx;
                self.active_arena[e].wind_cnt2 = self.active_arena[e2i].wind_cnt2;
                e2_next = self.active_arena[e2i].next_in_ael;
            }
            Some(e2i) => {
                // NonZero, positive, or negative filling here ... if e's
                // WindCnt is in the SAME direction as its WindDx, then polygon
                // filling will be on the right of 'e'. Neither e2.WindCnt nor
                // e2.WindDx should ever be 0.
                let (e2_wind_cnt, e2_wind_dx) = (self.active_arena[e2i].wind_cnt, self.active_arena[e2i].wind_dx);
                let e_wind_dx = self.active_arena[e].wind_dx;
                if e2_wind_cnt * e2_wind_dx < 0 {
                    // opposite directions so 'e' is outside 'e2' ...
                    if e2_wind_cnt.abs() > 1 {
                        // outside prev poly but still inside another.
                        if e2_wind_dx * e_wind_dx < 0 {
                            self.active_arena[e].wind_cnt = e2_wind_cnt;
                        } else {
                            self.active_arena[e].wind_cnt = e2_wind_cnt + e_wind_dx;
                        }
                    } else {
                        // now outside all polys of same polytype so set own WC
                        self.active_arena[e].wind_cnt = if self.is_open(e) { 1 } else { e_wind_dx };
                    }
                } else {
                    // 'e' must be inside 'e2'
                    if e2_wind_dx * e_wind_dx < 0 {
                        self.active_arena[e].wind_cnt = e2_wind_cnt;
                    } else {
                        self.active_arena[e].wind_cnt = e2_wind_cnt + e_wind_dx;
                    }
                }
                self.active_arena[e].wind_cnt2 = self.active_arena[e2i].wind_cnt2;
                e2_next = self.active_arena[e2i].next_in_ael; // ie get ready to calc WindCnt2
            }
        }

        // update wind_cnt2 ...
        if self.fillrule == FillRule::EvenOdd {
            while let Some(cur) = e2_next {
                if cur == e {
                    break;
                }
                if self.get_poly_type(cur) != pt && !self.is_open(cur) {
                    self.active_arena[e].wind_cnt2 = if self.active_arena[e].wind_cnt2 == 0 { 1 } else { 0 };
                }
                e2_next = self.active_arena[cur].next_in_ael;
            }
        } else {
            while let Some(cur) = e2_next {
                if cur == e {
                    break;
                }
                if self.get_poly_type(cur) != pt && !self.is_open(cur) {
                    self.active_arena[e].wind_cnt2 += self.active_arena[cur].wind_dx;
                }
                e2_next = self.active_arena[cur].next_in_ael;
            }
        }
    }

    fn set_wind_count_for_open_path_edge(&mut self, e: ActiveIdx) {
        let mut e2 = self.ael_head;
        if self.fillrule == FillRule::EvenOdd {
            let (mut cnt1, mut cnt2) = (0i32, 0i32);
            while let Some(cur) = e2 {
                if cur == e {
                    break;
                }
                if self.get_poly_type(cur) == PathType::Clip {
                    cnt2 += 1;
                } else if !self.is_open(cur) {
                    cnt1 += 1;
                }
                e2 = self.active_arena[cur].next_in_ael;
            }
            self.active_arena[e].wind_cnt = if is_odd(cnt1) { 1 } else { 0 };
            self.active_arena[e].wind_cnt2 = if is_odd(cnt2) { 1 } else { 0 };
        } else {
            while let Some(cur) = e2 {
                if cur == e {
                    break;
                }
                if self.get_poly_type(cur) == PathType::Clip {
                    let dx = self.active_arena[cur].wind_dx;
                    self.active_arena[e].wind_cnt2 += dx;
                } else if !self.is_open(cur) {
                    let dx = self.active_arena[cur].wind_dx;
                    self.active_arena[e].wind_cnt += dx;
                }
                e2 = self.active_arena[cur].next_in_ael;
            }
        }
    }

    // ---- building the output (OutRec/OutPt) ----

    fn new_outrec(&mut self) -> OutRecIdx {
        let idx = self.outrecs.len();
        self.outrecs.push(OutRec { idx, ..OutRec::default() });
        idx
    }

    fn add_out_pt(&mut self, e: ActiveIdx, pt: Point64) -> OutPtIdx {
        // Outrec.pts: a circular doubly-linked-list where
        // op_front[.prev]* ~~~> op_back && op_back == op_front.next
        let outrec = self.active_arena[e].outrec.unwrap();
        let to_front = self.is_front(e);
        let op_front = self.outrecs[outrec].pts.unwrap();
        let op_back = self.outpts[op_front].next;

        if to_front {
            if pt == self.outpts[op_front].pt {
                return op_front;
            }
        } else if pt == self.outpts[op_back].pt {
            return op_back;
        }

        let new_op = self.outpts.len();
        self.outpts.push(OutPt { pt, next: op_back, prev: op_front, outrec, horz_claimed: false });
        self.outpts[op_back].prev = new_op;
        self.outpts[op_front].next = new_op;
        if to_front {
            self.outrecs[outrec].pts = Some(new_op);
        }
        new_op
    }

    fn add_local_min_poly(&mut self, e1: ActiveIdx, e2: ActiveIdx, pt: Point64, is_new: bool) -> OutPtIdx {
        let outrec = self.new_outrec();
        self.active_arena[e1].outrec = Some(outrec);
        self.active_arena[e2].outrec = Some(outrec);

        if self.is_open(e1) {
            self.outrecs[outrec].owner = None;
            self.outrecs[outrec].is_open = true;
            if self.active_arena[e1].wind_dx > 0 {
                self.set_sides(outrec, e1, e2);
            } else {
                self.set_sides(outrec, e2, e1);
            }
        } else {
            let prev_hot_edge = self.get_prev_hot_edge(e1);
            // e.wind_dx is the winding direction of the **input** paths and
            // unrelated to the winding direction of output polygons. Output
            // orientation is determined by e.outrec.front_edge which is the
            // ascending edge.
            if let Some(prev_hot_edge) = prev_hot_edge {
                if self.using_polytree {
                    let owner = self.active_arena[prev_hot_edge].outrec.unwrap();
                    self.set_owner(outrec, owner);
                }
                if self.outrec_is_ascending(prev_hot_edge) == is_new {
                    self.set_sides(outrec, e2, e1);
                } else {
                    self.set_sides(outrec, e1, e2);
                }
            } else {
                self.outrecs[outrec].owner = None;
                if is_new {
                    self.set_sides(outrec, e1, e2);
                } else {
                    self.set_sides(outrec, e2, e1);
                }
            }
        }

        let op = self.outpts.len();
        self.outpts.push(OutPt { pt, next: op, prev: op, outrec, horz_claimed: false });
        self.outrecs[outrec].pts = Some(op);
        op
    }

    fn add_local_max_poly(&mut self, e1: ActiveIdx, e2: ActiveIdx, pt: Point64) -> Option<OutPtIdx> {
        if is_joined(&self.active_arena[e1]) {
            self.split(e1, pt);
        }
        if is_joined(&self.active_arena[e2]) {
            self.split(e2, pt);
        }

        if self.is_front(e1) == self.is_front(e2) {
            if self.is_open_end(e1) {
                self.swap_front_back_sides(self.active_arena[e1].outrec.unwrap());
            } else if self.is_open_end(e2) {
                self.swap_front_back_sides(self.active_arena[e2].outrec.unwrap());
            } else {
                self.succeeded = false;
                return None;
            }
        }

        let mut result = self.add_out_pt(e1, pt);
        if self.active_arena[e1].outrec == self.active_arena[e2].outrec {
            let outrec = self.active_arena[e1].outrec.unwrap();
            self.outrecs[outrec].pts = Some(result);

            if self.using_polytree {
                let e = self.get_prev_hot_edge(e1);
                match e {
                    None => self.outrecs[outrec].owner = None,
                    Some(e) => {
                        let owner = self.active_arena[e].outrec.unwrap();
                        self.set_owner(outrec, owner);
                    }
                }
                // nb: outrec.owner here is likely NOT the real owner but
                // this will be checked in RecursiveCheckOwners()
            }

            self.uncouple_outrec(e1);
            result = self.outrecs[outrec].pts.unwrap();
            if let Some(owner) = self.outrecs[outrec].owner {
                if self.outrecs[owner].front_edge.is_none() {
                    self.outrecs[outrec].owner = self.get_real_outrec(Some(owner));
                }
            }
        } else if self.is_open(e1) {
            if self.active_arena[e1].wind_dx < 0 {
                self.join_outrec_paths(e1, e2);
            } else {
                self.join_outrec_paths(e2, e1);
            }
        } else if self.outrecs[self.active_arena[e1].outrec.unwrap()].idx < self.outrecs[self.active_arena[e2].outrec.unwrap()].idx {
            self.join_outrec_paths(e1, e2);
        } else {
            self.join_outrec_paths(e2, e1);
        }
        Some(result)
    }

    fn join_outrec_paths(&mut self, e1: ActiveIdx, e2: ActiveIdx) {
        // join e2 outrec path onto e1 outrec path and then delete e2 outrec
        // path pointers (only very rarely do the joining ends share coords).
        let or1 = self.active_arena[e1].outrec.unwrap();
        let or2 = self.active_arena[e2].outrec.unwrap();
        let p1_st = self.outrecs[or1].pts.unwrap();
        let p2_st = self.outrecs[or2].pts.unwrap();
        let p1_end = self.outpts[p1_st].next;
        let p2_end = self.outpts[p2_st].next;
        if self.is_front(e1) {
            self.outpts[p2_end].prev = p1_st;
            self.outpts[p1_st].next = p2_end;
            self.outpts[p2_st].next = p1_end;
            self.outpts[p1_end].prev = p2_st;
            self.outrecs[or1].pts = Some(p2_st);
            self.outrecs[or1].front_edge = self.outrecs[or2].front_edge;
            if let Some(fe) = self.outrecs[or1].front_edge {
                self.active_arena[fe].outrec = Some(or1);
            }
        } else {
            self.outpts[p1_end].prev = p2_st;
            self.outpts[p2_st].next = p1_end;
            self.outpts[p1_st].next = p2_end;
            self.outpts[p2_end].prev = p1_st;
            self.outrecs[or1].back_edge = self.outrecs[or2].back_edge;
            if let Some(be) = self.outrecs[or1].back_edge {
                self.active_arena[be].outrec = Some(or1);
            }
        }

        // after joining, or2 must contain no vertices ...
        self.outrecs[or2].front_edge = None;
        self.outrecs[or2].back_edge = None;
        self.outrecs[or2].pts = None;

        if self.is_open_end(e1) {
            self.outrecs[or2].pts = self.outrecs[or1].pts;
            self.outrecs[or1].pts = None;
        } else {
            self.set_owner(or2, or1);
        }

        // e1 and e2 are maxima and are about to be dropped from the Actives list.
        self.active_arena[e1].outrec = None;
        self.active_arena[e2].outrec = None;
    }

    fn start_open_path(&mut self, e: ActiveIdx, pt: Point64) -> OutPtIdx {
        let outrec = self.new_outrec();
        self.outrecs[outrec].is_open = true;

        if self.active_arena[e].wind_dx > 0 {
            self.outrecs[outrec].front_edge = Some(e);
            self.outrecs[outrec].back_edge = None;
        } else {
            self.outrecs[outrec].front_edge = None;
            self.outrecs[outrec].back_edge = Some(e);
        }

        self.active_arena[e].outrec = Some(outrec);

        let op = self.outpts.len();
        self.outpts.push(OutPt { pt, next: op, prev: op, outrec, horz_claimed: false });
        self.outrecs[outrec].pts = Some(op);
        op
    }

    fn new_active(&mut self) -> ActiveIdx {
        self.active_arena.push(Active::default());
        self.active_arena.len() - 1
    }

    /// `InsertLocalMinimaIntoAEL`.
    fn insert_local_minima_into_ael(&mut self, bot_y: i64) {
        while let Some(local_minima) = self.pop_local_minima(bot_y) {
            let lm_vertex = self.minima_list[local_minima].vertex;
            let (polytype, lm_is_open) = (self.minima_list[local_minima].polytype, self.minima_list[local_minima].is_open);
            let _ = polytype;

            let mut left_bound: Option<ActiveIdx> = if self.vertices[lm_vertex].flags & vflags::OPEN_START != 0 {
                None
            } else {
                let lb = self.new_active();
                let a = &mut self.active_arena[lb];
                a.bot = self.vertices[lm_vertex].pt;
                a.curr_x = a.bot.x;
                a.wind_dx = -1;
                a.vertex_top = self.vertices[lm_vertex].prev; // descending
                a.top = self.vertices[a.vertex_top].pt;
                a.local_min = local_minima;
                set_dx(a);
                Some(lb)
            };

            let mut right_bound: Option<ActiveIdx> = if self.vertices[lm_vertex].flags & vflags::OPEN_END != 0 {
                None
            } else {
                let rb = self.new_active();
                let a = &mut self.active_arena[rb];
                a.bot = self.vertices[lm_vertex].pt;
                a.curr_x = a.bot.x;
                a.wind_dx = 1;
                a.vertex_top = self.vertices[lm_vertex].next; // ascending
                a.top = self.vertices[a.vertex_top].pt;
                a.local_min = local_minima;
                set_dx(a);
                Some(rb)
            };

            // Currently left_bound is just the descending bound and
            // right_bound is the ascending. If left_bound isn't on the left
            // of right_bound then swap them.
            if let (Some(lb), Some(rb)) = (left_bound, right_bound) {
                if is_horizontal(&self.active_arena[lb]) {
                    if is_heading_right_horz(&self.active_arena[lb]) {
                        std::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if is_horizontal(&self.active_arena[rb]) {
                    if is_heading_left_horz(&self.active_arena[rb]) {
                        std::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if self.active_arena[lb].dx < self.active_arena[rb].dx {
                    std::mem::swap(&mut left_bound, &mut right_bound);
                }
            } else if left_bound.is_none() {
                left_bound = right_bound;
                right_bound = None;
            }

            let left_bound = left_bound.unwrap();
            self.active_arena[left_bound].is_left_bound = true;
            self.insert_left_edge(left_bound);

            let contributing = if self.is_open(left_bound) {
                self.set_wind_count_for_open_path_edge(left_bound);
                self.is_contributing_open(left_bound)
            } else {
                self.set_wind_count_for_closed_path_edge(left_bound);
                self.is_contributing_closed(left_bound)
            };

            if let Some(right_bound) = right_bound {
                self.active_arena[right_bound].is_left_bound = false;
                self.active_arena[right_bound].wind_cnt = self.active_arena[left_bound].wind_cnt;
                self.active_arena[right_bound].wind_cnt2 = self.active_arena[left_bound].wind_cnt2;
                self.insert_right_edge(left_bound, right_bound);
                if contributing {
                    let bot = self.active_arena[left_bound].bot;
                    self.add_local_min_poly(left_bound, right_bound, bot, true);
                    if !is_horizontal(&self.active_arena[left_bound]) {
                        self.check_join_left(left_bound, bot, false);
                    }
                }

                while let Some(next) = self.active_arena[right_bound].next_in_ael {
                    if !self.is_valid_ael_order(next, right_bound) {
                        break;
                    }
                    let bot = self.active_arena[right_bound].bot;
                    self.intersect_edges(right_bound, next, bot);
                    self.swap_positions_in_ael(right_bound, next);
                }

                if is_horizontal(&self.active_arena[right_bound]) {
                    self.push_horz(right_bound);
                } else {
                    let bot = self.active_arena[right_bound].bot;
                    self.check_join_right(right_bound, bot, false);
                    let top_y = self.active_arena[right_bound].top.y;
                    self.insert_scanline(top_y);
                }
            } else if contributing {
                let bot = self.active_arena[left_bound].bot;
                self.start_open_path(left_bound, bot);
            }

            if is_horizontal(&self.active_arena[left_bound]) {
                self.push_horz(left_bound);
            } else {
                let top_y = self.active_arena[left_bound].top.y;
                self.insert_scanline(top_y);
            }

            let _ = lm_is_open;
        }
    }

    // ---- collinear cleanup & self-intersection splitting ----

    fn clean_collinear(&mut self, outrec: Option<OutRecIdx>) {
        let outrec = match self.get_real_outrec(outrec) {
            Some(o) if !self.outrecs[o].is_open => o,
            _ => return,
        };
        if !self.is_valid_closed_path(self.outrecs[outrec].pts) {
            self.dispose_out_pts(outrec);
            return;
        }

        let mut start_op = self.outrecs[outrec].pts.unwrap();
        let mut op2 = start_op;
        loop {
            // NB if preserve_collinear == true, only remove 180deg spikes
            let (prev, next) = (self.outpts[op2].prev, self.outpts[op2].next);
            let (pp, pc, pn) = (self.outpts[prev].pt, self.outpts[op2].pt, self.outpts[next].pt);
            if cross_product(pp, pc, pn) == 0.0
                && (pc == pp || pc == pn || !self.preserve_collinear || dot_product(pp, pc, pn) < 0.0)
            {
                if op2 == self.outrecs[outrec].pts.unwrap() {
                    self.outrecs[outrec].pts = Some(prev);
                }
                op2 = self.dispose_out_pt(op2);
                if !self.is_valid_closed_path(Some(op2)) {
                    self.dispose_out_pts(outrec);
                    return;
                }
                start_op = op2;
                continue;
            }
            op2 = self.outpts[op2].next;
            if op2 == start_op {
                break;
            }
        }
        self.fix_self_intersects(outrec);
    }

    fn do_split_op(&mut self, outrec: OutRecIdx, split_op: OutPtIdx) {
        // splitOp.prev -> splitOp && splitOp.next -> splitOp.next.next are intersecting
        let prev_op = self.outpts[split_op].prev;
        let split_next = self.outpts[split_op].next;
        let next_next_op = self.outpts[split_next].next;
        self.outrecs[outrec].pts = Some(prev_op);

        let mut ip = match get_intersect_point(
            self.outpts[prev_op].pt,
            self.outpts[split_op].pt,
            self.outpts[split_next].pt,
            self.outpts[next_next_op].pt,
        ) {
            Some(p) => p,
            None => self.outpts[prev_op].pt,
        };

        if self.z_callback.is_some() {
            let (a, b, c, d) = (self.outpts[prev_op].pt, self.outpts[split_op].pt, self.outpts[split_next].pt, self.outpts[next_next_op].pt);
            if let Some(cb) = self.z_callback.as_mut() {
                cb(a, b, c, d, &mut ip);
            }
        }

        let area1 = self.out_pt_area(self.outrecs[outrec].pts.unwrap());
        let abs_area1 = area1.abs();
        if abs_area1 < 2.0 {
            self.dispose_out_pts(outrec);
            return;
        }

        let area2 = area_triangle(ip, self.outpts[split_op].pt, self.outpts[split_next].pt);
        let abs_area2 = area2.abs();

        // de-link splitOp and splitOp.next from the path while inserting
        // the intersection point
        let ip_is_endpoint = ip == self.outpts[prev_op].pt || ip == self.outpts[next_next_op].pt;
        if ip_is_endpoint {
            self.outpts[next_next_op].prev = prev_op;
            self.outpts[prev_op].next = next_next_op;
        } else {
            let new_op2 = self.outpts.len();
            self.outpts.push(OutPt { pt: ip, next: next_next_op, prev: prev_op, outrec: self.outpts[prev_op].outrec, horz_claimed: false });
            self.outpts[next_next_op].prev = new_op2;
            self.outpts[prev_op].next = new_op2;
        }

        // area1 is the path's area *before* splitting, area2 is the area of
        // the triangle containing splitOp & splitOp.next. The only way for
        // these areas to have the same sign is if the split triangle is
        // larger than the path containing prevOp, or there's more than one
        // self-intersection.
        if abs_area2 >= 1.0 && (abs_area2 > abs_area1 || (area2 > 0.0) == (area1 > 0.0)) {
            let new_or = self.new_outrec();
            self.outrecs[new_or].owner = self.outrecs[outrec].owner;

            self.outpts[split_op].outrec = new_or;
            self.outpts[split_next].outrec = new_or;
            let new_op = self.outpts.len();
            self.outpts.push(OutPt { pt: ip, next: split_op, prev: split_next, outrec: new_or, horz_claimed: false });
            self.outrecs[new_or].pts = Some(new_op);
            self.outpts[split_op].prev = new_op;
            self.outpts[split_next].next = new_op;

            if self.using_polytree {
                if self.path1_inside_path2(prev_op, new_op) {
                    self.outrecs[new_or].splits = Some(vec![outrec]);
                } else {
                    self.outrecs[outrec].splits.get_or_insert_with(Vec::new).push(new_or);
                }
            }
        }
        // else: the original `delete`s splitOp and splitOp->next; left as
        // unreachable arena slots here (see module doc comment).
    }

    fn fix_self_intersects(&mut self, outrec: OutRecIdx) {
        let mut op2 = self.outrecs[outrec].pts.unwrap();
        loop {
            // triangles can't self-intersect
            let (prev, next) = (self.outpts[op2].prev, self.outpts[op2].next);
            let next_next = self.outpts[next].next;
            if prev == next_next {
                break;
            }
            if segments_intersect(self.outpts[prev].pt, self.outpts[op2].pt, self.outpts[next].pt, self.outpts[next_next].pt, false) {
                if op2 == self.outrecs[outrec].pts.unwrap() || next == self.outrecs[outrec].pts.unwrap() {
                    self.outrecs[outrec].pts = Some(self.outpts[self.outrecs[outrec].pts.unwrap()].prev);
                }
                self.do_split_op(outrec, op2);
                match self.outrecs[outrec].pts {
                    None => break,
                    Some(pts) => op2 = pts,
                }
                continue;
            } else {
                op2 = self.outpts[op2].next;
            }
            if Some(op2) == self.outrecs[outrec].pts {
                break;
            }
        }
    }

    // ---- edge maintenance: trimming, promoting into AEL, intersections ----

    fn trim_horz(&mut self, horz_edge: ActiveIdx, preserve_collinear: bool) {
        let mut was_trimmed = false;
        let mut pt = self.vertices[self.next_vertex(horz_edge)].pt;
        while pt.y == self.active_arena[horz_edge].top.y {
            // always trim 180deg spikes (in closed paths) but otherwise
            // break if preserve_collinear == true
            if preserve_collinear {
                let top = self.active_arena[horz_edge].top;
                let bot_x = self.active_arena[horz_edge].bot.x;
                if (pt.x < top.x) != (bot_x < top.x) {
                    break;
                }
            }

            let nv = self.next_vertex(horz_edge);
            self.active_arena[horz_edge].vertex_top = nv;
            self.active_arena[horz_edge].top = pt;
            was_trimmed = true;
            if self.is_maxima(horz_edge) {
                break;
            }
            pt = self.vertices[self.next_vertex(horz_edge)].pt;
        }
        if was_trimmed {
            set_dx(&mut self.active_arena[horz_edge]); // +/-infinity
        }
    }

    fn update_edge_into_ael(&mut self, e: ActiveIdx) {
        self.active_arena[e].bot = self.active_arena[e].top;
        let nv = self.next_vertex(e);
        self.active_arena[e].vertex_top = nv;
        self.active_arena[e].top = self.vertices[nv].pt;
        self.active_arena[e].curr_x = self.active_arena[e].bot.x;
        set_dx(&mut self.active_arena[e]);

        if is_joined(&self.active_arena[e]) {
            let bot = self.active_arena[e].bot;
            self.split(e, bot);
        }

        if is_horizontal(&self.active_arena[e]) {
            if !self.is_open(e) {
                self.trim_horz(e, self.preserve_collinear);
            }
            return;
        }

        let top_y = self.active_arena[e].top.y;
        self.insert_scanline(top_y);
        let bot = self.active_arena[e].bot;
        self.check_join_left(e, bot, false);
        self.check_join_right(e, bot, true); // (#500)
    }

    fn find_edge_with_matching_loc_min(&self, e: ActiveIdx) -> Option<ActiveIdx> {
        let lm = self.active_arena[e].local_min;
        let mut result = self.active_arena[e].next_in_ael;
        while let Some(r) = result {
            if self.active_arena[r].local_min == lm {
                return Some(r);
            } else if !is_horizontal(&self.active_arena[r]) && self.active_arena[e].bot != self.active_arena[r].bot {
                result = None;
            } else {
                result = self.active_arena[r].next_in_ael;
            }
        }
        let mut result = self.active_arena[e].prev_in_ael;
        while let Some(r) = result {
            if self.active_arena[r].local_min == lm {
                return Some(r);
            } else if !is_horizontal(&self.active_arena[r]) && self.active_arena[e].bot != self.active_arena[r].bot {
                return None;
            } else {
                result = self.active_arena[r].prev_in_ael;
            }
        }
        result
    }

    /// `IntersectEdges`: the heart of the Vatti sweep -- handles open/open
    /// (skip), open/closed (toggling contribution for the open path based
    /// on the closed path's winding), and closed/closed (full winding-count
    /// update plus `AddLocalMinPoly`/`AddLocalMaxPoly`/`AddOutPt` dispatch)
    /// intersections, exactly mirroring the branch structure of the
    /// original (including its `#ifdef USINGZ` `SetZ` calls, all of which
    /// are unconditionally present here since KiCad always builds with
    /// `USINGZ`).
    fn intersect_edges(&mut self, e1: ActiveIdx, e2: ActiveIdx, pt: Point64) -> Option<OutPtIdx> {
        // MANAGE OPEN PATH INTERSECTIONS SEPARATELY ...
        if self.has_open_paths && (self.is_open(e1) || self.is_open(e2)) {
            if self.is_open(e1) && self.is_open(e2) {
                return None;
            }
            let (edge_o, edge_c) = if self.is_open(e1) { (e1, e2) } else { (e2, e1) };
            if is_joined(&self.active_arena[edge_c]) {
                self.split(edge_c, pt); // needed for safety
            }

            if self.active_arena[edge_c].wind_cnt.abs() != 1 {
                return None;
            }
            match self.cliptype {
                ClipType::Union => {
                    if !self.is_hot_edge(edge_c) {
                        return None;
                    }
                }
                _ => {
                    let lm = self.active_arena[edge_c].local_min;
                    if self.minima_list[lm].polytype == PathType::Subject {
                        return None;
                    }
                }
            }

            match self.fillrule {
                FillRule::Positive => {
                    if self.active_arena[edge_c].wind_cnt != 1 {
                        return None;
                    }
                }
                FillRule::Negative => {
                    if self.active_arena[edge_c].wind_cnt != -1 {
                        return None;
                    }
                }
                _ => {
                    if self.active_arena[edge_c].wind_cnt.abs() != 1 {
                        return None;
                    }
                }
            }

            let result_op: OutPtIdx;
            if self.is_hot_edge(edge_o) {
                let r = self.add_out_pt(edge_o, pt);
                if self.is_front(edge_o) {
                    self.outrecs[self.active_arena[edge_o].outrec.unwrap()].front_edge = None;
                } else {
                    self.outrecs[self.active_arena[edge_o].outrec.unwrap()].back_edge = None;
                }
                self.active_arena[edge_o].outrec = None;
                result_op = r;
            } else if pt == self.vertices[self.minima_list[self.active_arena[edge_o].local_min].vertex].pt
                && !self.is_open_end_vertex(self.minima_list[self.active_arena[edge_o].local_min].vertex)
            {
                // horizontal edges can pass under open paths at a LocMin:
                // find the other side of the LocMin and if it's 'hot' join
                // up with it ...
                let e3 = self.find_edge_with_matching_loc_min(edge_o);
                match e3 {
                    Some(e3) if self.is_hot_edge(e3) => {
                        let outrec = self.active_arena[e3].outrec.unwrap();
                        self.active_arena[edge_o].outrec = Some(outrec);
                        if self.active_arena[edge_o].wind_dx > 0 {
                            self.set_sides(outrec, edge_o, e3);
                        } else {
                            self.set_sides(outrec, e3, edge_o);
                        }
                        return self.outrecs[outrec].pts;
                    }
                    _ => {
                        result_op = self.start_open_path(edge_o, pt);
                    }
                }
            } else {
                result_op = self.start_open_path(edge_o, pt);
            }

            if self.z_callback.is_some() {
                let mut p = self.outpts[result_op].pt;
                self.set_z(edge_o, edge_c, &mut p);
                self.outpts[result_op].pt = p;
            }
            return Some(result_op);
        } // end of an open path intersection

        // MANAGING CLOSED PATHS FROM HERE ON

        if is_joined(&self.active_arena[e1]) {
            self.split(e1, pt);
        }
        if is_joined(&self.active_arena[e2]) {
            self.split(e2, pt);
        }

        // UPDATE WINDING COUNTS...
        let (old_e1_windcnt, old_e2_windcnt);
        if self.get_poly_type(e1) == self.get_poly_type(e2) {
            if self.fillrule == FillRule::EvenOdd {
                let tmp = self.active_arena[e1].wind_cnt;
                self.active_arena[e1].wind_cnt = self.active_arena[e2].wind_cnt;
                self.active_arena[e2].wind_cnt = tmp;
            } else {
                let e2_wind_dx = self.active_arena[e2].wind_dx;
                if self.active_arena[e1].wind_cnt + e2_wind_dx == 0 {
                    self.active_arena[e1].wind_cnt = -self.active_arena[e1].wind_cnt;
                } else {
                    self.active_arena[e1].wind_cnt += e2_wind_dx;
                }
                let e1_wind_dx = self.active_arena[e1].wind_dx;
                if self.active_arena[e2].wind_cnt - e1_wind_dx == 0 {
                    self.active_arena[e2].wind_cnt = -self.active_arena[e2].wind_cnt;
                } else {
                    self.active_arena[e2].wind_cnt -= e1_wind_dx;
                }
            }
        } else if self.fillrule != FillRule::EvenOdd {
            let (dx1, dx2) = (self.active_arena[e1].wind_dx, self.active_arena[e2].wind_dx);
            self.active_arena[e1].wind_cnt2 += dx2;
            self.active_arena[e2].wind_cnt2 -= dx1;
        } else {
            self.active_arena[e1].wind_cnt2 = if self.active_arena[e1].wind_cnt2 == 0 { 1 } else { 0 };
            self.active_arena[e2].wind_cnt2 = if self.active_arena[e2].wind_cnt2 == 0 { 1 } else { 0 };
        }

        match self.fillrule {
            FillRule::EvenOdd | FillRule::NonZero => {
                old_e1_windcnt = self.active_arena[e1].wind_cnt.abs();
                old_e2_windcnt = self.active_arena[e2].wind_cnt.abs();
            }
            FillRule::Positive => {
                old_e1_windcnt = self.active_arena[e1].wind_cnt;
                old_e2_windcnt = self.active_arena[e2].wind_cnt;
            }
            FillRule::Negative => {
                old_e1_windcnt = -self.active_arena[e1].wind_cnt;
                old_e2_windcnt = -self.active_arena[e2].wind_cnt;
            }
        }

        let e1_windcnt_in_01 = old_e1_windcnt == 0 || old_e1_windcnt == 1;
        let e2_windcnt_in_01 = old_e2_windcnt == 0 || old_e2_windcnt == 1;

        if (!self.is_hot_edge(e1) && !e1_windcnt_in_01) || (!self.is_hot_edge(e2) && !e2_windcnt_in_01) {
            return None;
        }

        // NOW PROCESS THE INTERSECTION ...
        let mut result_op: Option<OutPtIdx> = None;
        if self.is_hot_edge(e1) && self.is_hot_edge(e2) {
            if (old_e1_windcnt != 0 && old_e1_windcnt != 1)
                || (old_e2_windcnt != 0 && old_e2_windcnt != 1)
                || (self.get_poly_type(e1) != self.get_poly_type(e2) && self.cliptype != ClipType::Xor)
            {
                result_op = self.add_local_max_poly(e1, e2, pt);
                if self.z_callback.is_some() {
                    if let Some(r) = result_op {
                        let mut p = self.outpts[r].pt;
                        self.set_z(e1, e2, &mut p);
                        self.outpts[r].pt = p;
                    }
                }
            } else if self.is_front(e1) || self.active_arena[e1].outrec == self.active_arena[e2].outrec {
                // not strictly needed, but sensible to split polygons that
                // only touch at a common vertex (not at common edges).
                result_op = self.add_local_max_poly(e1, e2, pt);
                let op2 = self.add_local_min_poly(e1, e2, pt, false);
                if self.z_callback.is_some() {
                    if let Some(r) = result_op {
                        let mut p = self.outpts[r].pt;
                        self.set_z(e1, e2, &mut p);
                        self.outpts[r].pt = p;
                    }
                    let mut p2 = self.outpts[op2].pt;
                    self.set_z(e1, e2, &mut p2);
                    self.outpts[op2].pt = p2;
                }
            } else {
                let r = self.add_out_pt(e1, pt);
                result_op = Some(r);
                let op2 = self.add_out_pt(e2, pt);
                if self.z_callback.is_some() {
                    let mut p = self.outpts[r].pt;
                    self.set_z(e1, e2, &mut p);
                    self.outpts[r].pt = p;
                    let mut p2 = self.outpts[op2].pt;
                    self.set_z(e1, e2, &mut p2);
                    self.outpts[op2].pt = p2;
                }
                self.swap_outrecs(e1, e2);
            }
        } else if self.is_hot_edge(e1) {
            let r = self.add_out_pt(e1, pt);
            result_op = Some(r);
            if self.z_callback.is_some() {
                let mut p = self.outpts[r].pt;
                self.set_z(e1, e2, &mut p);
                self.outpts[r].pt = p;
            }
            self.swap_outrecs(e1, e2);
        } else if self.is_hot_edge(e2) {
            let r = self.add_out_pt(e2, pt);
            result_op = Some(r);
            if self.z_callback.is_some() {
                let mut p = self.outpts[r].pt;
                self.set_z(e1, e2, &mut p);
                self.outpts[r].pt = p;
            }
            self.swap_outrecs(e1, e2);
        } else {
            let (e1wc2, e2wc2) = match self.fillrule {
                FillRule::EvenOdd | FillRule::NonZero => (self.active_arena[e1].wind_cnt2.abs(), self.active_arena[e2].wind_cnt2.abs()),
                FillRule::Positive => (self.active_arena[e1].wind_cnt2, self.active_arena[e2].wind_cnt2),
                FillRule::Negative => (-self.active_arena[e1].wind_cnt2, -self.active_arena[e2].wind_cnt2),
            };

            if !self.is_same_poly_type(e1, e2) {
                result_op = Some(self.add_local_min_poly(e1, e2, pt, false));
                if self.z_callback.is_some() {
                    if let Some(r) = result_op {
                        let mut p = self.outpts[r].pt;
                        self.set_z(e1, e2, &mut p);
                        self.outpts[r].pt = p;
                    }
                }
            } else if old_e1_windcnt == 1 && old_e2_windcnt == 1 {
                result_op = None;
                match self.cliptype {
                    ClipType::Union => {
                        if e1wc2 <= 0 && e2wc2 <= 0 {
                            result_op = Some(self.add_local_min_poly(e1, e2, pt, false));
                        }
                    }
                    ClipType::Difference => {
                        if (self.get_poly_type(e1) == PathType::Clip && e1wc2 > 0 && e2wc2 > 0)
                            || (self.get_poly_type(e1) == PathType::Subject && e1wc2 <= 0 && e2wc2 <= 0)
                        {
                            result_op = Some(self.add_local_min_poly(e1, e2, pt, false));
                        }
                    }
                    ClipType::Xor => {
                        result_op = Some(self.add_local_min_poly(e1, e2, pt, false));
                    }
                    _ => {
                        if e1wc2 > 0 && e2wc2 > 0 {
                            result_op = Some(self.add_local_min_poly(e1, e2, pt, false));
                        }
                    }
                }
                if let Some(r) = result_op {
                    if self.z_callback.is_some() {
                        let mut p = self.outpts[r].pt;
                        self.set_z(e1, e2, &mut p);
                        self.outpts[r].pt = p;
                    }
                }
            }
        }
        result_op
    }

    // ---- scanbeam-top intersection processing ----

    fn adjust_curr_x_and_copy_to_sel(&mut self, top_y: i64) {
        let mut e = self.ael_head;
        self.sel_head = e;
        while let Some(cur) = e {
            self.active_arena[cur].prev_in_sel = self.active_arena[cur].prev_in_ael;
            self.active_arena[cur].next_in_sel = self.active_arena[cur].next_in_ael;
            self.active_arena[cur].jump = self.active_arena[cur].next_in_sel;
            if self.active_arena[cur].join_with == JoinWith::Left {
                // also avoids complications
                let p = self.active_arena[cur].prev_in_ael.unwrap();
                self.active_arena[cur].curr_x = self.active_arena[p].curr_x;
            } else {
                self.active_arena[cur].curr_x = top_x(&self.active_arena[cur], top_y);
            }
            e = self.active_arena[cur].next_in_ael;
        }
    }

    fn add_new_intersect_node(&mut self, e1: ActiveIdx, e2: ActiveIdx, top_y: i64) {
        let (e1bot, e1top) = (self.active_arena[e1].bot, self.active_arena[e1].top);
        let (e2bot, e2top) = (self.active_arena[e2].bot, self.active_arena[e2].top);
        let mut ip = match get_intersect_point(e1bot, e1top, e2bot, e2top) {
            Some(p) => p,
            None => Point64::new(self.active_arena[e1].curr_x, top_y), // parallel edges
        };

        // rounding errors can occasionally place the calculated
        // intersection point either below or above the scanbeam, so check
        // and correct ...
        if ip.y > self.bot_y || ip.y < top_y {
            let abs_dx1 = self.active_arena[e1].dx.abs();
            let abs_dx2 = self.active_arena[e2].dx.abs();
            if abs_dx1 > 100.0 && abs_dx2 > 100.0 {
                if abs_dx1 > abs_dx2 {
                    ip = get_closest_point_on_segment(ip, e1bot, e1top);
                } else {
                    ip = get_closest_point_on_segment(ip, e2bot, e2top);
                }
            } else if abs_dx1 > 100.0 {
                ip = get_closest_point_on_segment(ip, e1bot, e1top);
            } else if abs_dx2 > 100.0 {
                ip = get_closest_point_on_segment(ip, e2bot, e2top);
            } else {
                if ip.y < top_y {
                    ip.y = top_y;
                } else {
                    ip.y = self.bot_y;
                }
                if abs_dx1 < abs_dx2 {
                    ip.x = top_x(&self.active_arena[e1], ip.y);
                } else {
                    ip.x = top_x(&self.active_arena[e2], ip.y);
                }
            }
        }
        self.intersect_nodes.push(IntersectNode { pt: ip, edge1: e1, edge2: e2 });
    }

    /// `BuildIntersectList`: finds every adjacent-edge intersection needed
    /// to reach the top-of-scanbeam positions, via a bottom-up stable merge
    /// sort over the SEL `jump` chain (Vatti's approach, preserved exactly
    /// including the `jump`-pointer bookkeeping).
    fn build_intersect_list(&mut self, top_y: i64) -> bool {
        match self.ael_head {
            Some(h) if self.active_arena[h].next_in_ael.is_some() => {}
            _ => return false,
        }

        // Find all edge intersections in the current scanbeam using a
        // stable merge sort that ensures only adjacent edges are
        // intersecting (https://stackoverflow.com/a/46319131/359538): a
        // direct, pointer-for-index transliteration of the original, with
        // `jump` repurposed (exactly as upstream does) to chain together
        // same-size merge blocks as they're built.
        self.adjust_curr_x_and_copy_to_sel(top_y);

        let mut left: Option<ActiveIdx> = self.sel_head;
        while left.is_some() && self.active_arena[left.unwrap()].jump.is_some() {
            let mut prev_base: Option<ActiveIdx> = None;
            while left.is_some() && self.active_arena[left.unwrap()].jump.is_some() {
                let mut curr_base = left;
                let mut right = self.active_arena[left.unwrap()].jump;
                let mut l_end = right;
                let r_end = self.active_arena[right.unwrap()].jump;
                self.active_arena[left.unwrap()].jump = r_end;
                while left != l_end && right != r_end {
                    let (l, r) = (left.unwrap(), right.unwrap());
                    if self.active_arena[r].curr_x < self.active_arena[l].curr_x {
                        let mut tmp = self.active_arena[r].prev_in_sel;
                        loop {
                            self.add_new_intersect_node(tmp.unwrap(), r, top_y);
                            if tmp == left {
                                break;
                            }
                            tmp = self.active_arena[tmp.unwrap()].prev_in_sel;
                        }

                        let tmp2 = right;
                        right = self.extract_from_sel(tmp2.unwrap());
                        l_end = right;
                        self.insert1_before2_in_sel(tmp2.unwrap(), left.unwrap());
                        if left == curr_base {
                            curr_base = tmp2;
                            self.active_arena[curr_base.unwrap()].jump = r_end;
                            match prev_base {
                                None => self.sel_head = curr_base,
                                Some(pb) => self.active_arena[pb].jump = curr_base,
                            }
                        }
                    } else {
                        left = self.active_arena[l].next_in_sel;
                    }
                }
                prev_base = curr_base;
                left = r_end;
            }
            left = self.sel_head;
        }
        !self.intersect_nodes.is_empty()
    }

    fn process_intersect_list(&mut self) {
        // first a quicksort so intersections proceed bottom-up ...
        self.intersect_nodes.sort_by(intersect_list_sort);
        // then, as we process these, sometimes adjust the order to ensure
        // intersecting edges are always adjacent ...
        let mut i = 0usize;
        while i < self.intersect_nodes.len() {
            if !self.edges_adjacent_in_ael(&self.intersect_nodes[i]) {
                let mut j = i + 1;
                while !self.edges_adjacent_in_ael(&self.intersect_nodes[j]) {
                    j += 1;
                }
                self.intersect_nodes.swap(i, j);
            }

            let (edge1, edge2, pt) = {
                let n = &self.intersect_nodes[i];
                (n.edge1, n.edge2, n.pt)
            };
            self.intersect_edges(edge1, edge2, pt);
            self.swap_positions_in_ael(edge1, edge2);

            self.active_arena[edge1].curr_x = pt.x;
            self.active_arena[edge2].curr_x = pt.x;
            self.check_join_left(edge2, pt, true);
            self.check_join_right(edge1, pt, true);
            i += 1;
        }
    }

    fn do_intersections(&mut self, top_y: i64) {
        if self.build_intersect_list(top_y) {
            self.process_intersect_list();
            self.intersect_nodes.clear();
        }
    }

    // ---- horizontal-edge processing ----

    fn get_last_op(&self, hot_edge: ActiveIdx) -> OutPtIdx {
        let outrec = self.active_arena[hot_edge].outrec.unwrap();
        let result = self.outrecs[outrec].pts.unwrap();
        if self.outrecs[outrec].front_edge != Some(hot_edge) {
            self.outpts[result].next
        } else {
            result
        }
    }

    fn add_trial_horz_join(&mut self, op: OutPtIdx) {
        if self.outrecs[self.outpts[op].outrec].is_open {
            return;
        }
        self.horz_seg_list.push(HorzSegment { left_op: op, right_op: None, left_to_right: true });
    }

    fn reset_horz_direction(&self, horz: ActiveIdx, max_vertex: Option<VertexIdx>) -> (bool, i64, i64) {
        let h = &self.active_arena[horz];
        if h.bot.x == h.top.x {
            // the horizontal edge is going nowhere ...
            let horz_left = h.curr_x;
            let horz_right = h.curr_x;
            let mut e = h.next_in_ael;
            while let Some(cur) = e {
                if Some(self.active_arena[cur].vertex_top) == max_vertex {
                    break;
                }
                e = self.active_arena[cur].next_in_ael;
            }
            (e.is_some(), horz_left, horz_right)
        } else if h.curr_x < h.top.x {
            (true, h.curr_x, h.top.x)
        } else {
            (false, h.top.x, h.curr_x) // right to left
        }
    }

    /*******************************************************************************
        * Notes: Horizontal edges (HEs) at scanline intersections (ie at the top or    *
        * bottom of a scanbeam) are processed as if layered. The order in which HEs    *
        * are processed doesn't matter. HEs intersect with the bottom vertices of      *
        * other HEs and with non-horizontal edges. Once these intersections are       *
        * completed, intermediate HEs are 'promoted' to the next edge in their        *
        * bounds, and they in turn may be intersected by other HEs.                   *
        *******************************************************************************/
    fn do_horizontal(&mut self, horz: ActiveIdx) {
        let horz_is_open = self.is_open(horz);
        let y = self.active_arena[horz].bot.y;
        let vertex_max = if horz_is_open {
            self.get_curr_ymaxima_vertex_open(horz)
        } else {
            self.get_curr_ymaxima_vertex(horz)
        };

        let (mut is_left_to_right, mut horz_left, mut horz_right) = self.reset_horz_direction(horz, vertex_max);

        if self.is_hot_edge(horz) {
            let (curr_x, bot_z) = (self.active_arena[horz].curr_x, self.active_arena[horz].bot.z);
            let op = self.add_out_pt(horz, Point64::with_z(curr_x, y, bot_z));
            self.add_trial_horz_join(op);
        }

        loop {
            // loop through consecutive horizontal edges
            let mut e = if is_left_to_right { self.active_arena[horz].next_in_ael } else { self.active_arena[horz].prev_in_ael };

            while let Some(ei) = e {
                if Some(self.active_arena[ei].vertex_top) == vertex_max {
                    if self.is_hot_edge(horz) && is_joined(&self.active_arena[ei]) {
                        let top = self.active_arena[ei].top;
                        self.split(ei, top);
                    }

                    if self.is_hot_edge(horz) {
                        while self.active_arena[horz].vertex_top != vertex_max.unwrap() {
                            let top = self.active_arena[horz].top;
                            self.add_out_pt(horz, top);
                            self.update_edge_into_ael(horz);
                        }
                        let top = self.active_arena[horz].top;
                        if is_left_to_right {
                            self.add_local_max_poly(horz, ei, top);
                        } else {
                            self.add_local_max_poly(ei, horz, top);
                        }
                    }
                    self.delete_from_ael(ei);
                    self.delete_from_ael(horz);
                    return;
                }

                // if horz is a maxima, keep going until we reach its maxima
                // pair, otherwise check for break conditions
                if vertex_max != Some(self.active_arena[horz].vertex_top) || self.is_open_end(horz) {
                    // otherwise stop when 'e' is beyond the end of the horizontal line
                    let e_curr_x = self.active_arena[ei].curr_x;
                    if (is_left_to_right && e_curr_x > horz_right) || (!is_left_to_right && e_curr_x < horz_left) {
                        break;
                    }

                    if e_curr_x == self.active_arena[horz].top.x && !is_horizontal(&self.active_arena[ei]) {
                        let pt = self.vertices[self.next_vertex(horz)].pt;
                        if is_left_to_right {
                            // with open paths we'll only break once past horz's end
                            if self.is_open(ei) && !self.is_same_poly_type(ei, horz) && !self.is_hot_edge(ei) {
                                if top_x(&self.active_arena[ei], pt.y) > pt.x {
                                    break;
                                }
                            } else if top_x(&self.active_arena[ei], pt.y) >= pt.x {
                                // otherwise break when horz's outslope >= e's
                                break;
                            }
                        } else if self.is_open(ei) && !self.is_same_poly_type(ei, horz) && !self.is_hot_edge(ei) {
                            if top_x(&self.active_arena[ei], pt.y) < pt.x {
                                break;
                            }
                        } else if top_x(&self.active_arena[ei], pt.y) <= pt.x {
                            break;
                        }
                    }
                }

                let pt = Point64::new(self.active_arena[ei].curr_x, self.active_arena[horz].bot.y);
                if is_left_to_right {
                    self.intersect_edges(horz, ei, pt);
                    self.swap_positions_in_ael(horz, ei);
                    self.check_join_left(ei, pt, false);
                    self.active_arena[horz].curr_x = self.active_arena[ei].curr_x;
                    e = self.active_arena[horz].next_in_ael;
                } else {
                    self.intersect_edges(ei, horz, pt);
                    self.swap_positions_in_ael(ei, horz);
                    self.check_join_right(ei, pt, false);
                    self.active_arena[horz].curr_x = self.active_arena[ei].curr_x;
                    e = self.active_arena[horz].prev_in_ael;
                }

                if self.active_arena[horz].outrec.is_some() {
                    // nb: the outrec containing the op returned by
                    // IntersectEdges above may no longer be horz's.
                    let last = self.get_last_op(horz);
                    self.add_trial_horz_join(last);
                }
            }

            // check if we've finished with (consecutive) horizontals ...
            if horz_is_open && self.is_open_end(horz) {
                // ie open at top
                if self.is_hot_edge(horz) {
                    let top = self.active_arena[horz].top;
                    self.add_out_pt(horz, top);
                    if self.is_front(horz) {
                        self.outrecs[self.active_arena[horz].outrec.unwrap()].front_edge = None;
                    } else {
                        self.outrecs[self.active_arena[horz].outrec.unwrap()].back_edge = None;
                    }
                    self.active_arena[horz].outrec = None;
                }
                self.delete_from_ael(horz);
                return;
            } else if self.vertices[self.next_vertex(horz)].pt.y != self.active_arena[horz].top.y {
                break;
            }

            // still more horizontals in bound to process ...
            if self.is_hot_edge(horz) {
                let top = self.active_arena[horz].top;
                self.add_out_pt(horz, top);
            }
            self.update_edge_into_ael(horz);
            let r = self.reset_horz_direction(horz, vertex_max);
            is_left_to_right = r.0;
            horz_left = r.1;
            horz_right = r.2;
        }

        if self.is_hot_edge(horz) {
            let top = self.active_arena[horz].top;
            let op = self.add_out_pt(horz, top);
            self.add_trial_horz_join(op);
        }

        self.update_edge_into_ael(horz); // end of an intermediate horiz.
    }

    // ---- horizontal-segment -> join conversion (`ConvertHorzSegsToJoins`) ----

    fn fix_outrec_pts(&mut self, outrec: OutRecIdx) {
        let start = self.outrecs[outrec].pts.unwrap();
        let mut op = start;
        loop {
            self.outpts[op].outrec = outrec;
            op = self.outpts[op].next;
            if op == start {
                return;
            }
        }
    }

    fn set_horz_seg_heading_forward(&self, hs: &mut HorzSegment, op_p: OutPtIdx, op_n: OutPtIdx) -> bool {
        if self.outpts[op_p].pt.x == self.outpts[op_n].pt.x {
            return false;
        }
        if self.outpts[op_p].pt.x < self.outpts[op_n].pt.x {
            hs.left_op = op_p;
            hs.right_op = Some(op_n);
            hs.left_to_right = true;
        } else {
            hs.left_op = op_n;
            hs.right_op = Some(op_p);
            hs.left_to_right = false;
        }
        true
    }

    fn update_horz_segment(&mut self, hs_idx: usize) -> bool {
        let op = self.horz_seg_list[hs_idx].left_op;
        let outrec = self.get_real_outrec(Some(self.outpts[op].outrec)).unwrap();
        let outrec_has_edges = self.outrecs[outrec].front_edge.is_some();
        let curr_y = self.outpts[op].pt.y;
        let (mut op_p, mut op_n) = (op, op);
        if outrec_has_edges {
            let op_a = self.outrecs[outrec].pts.unwrap();
            let op_z = self.outpts[op_a].next;
            while op_p != op_z && self.outpts[self.outpts[op_p].prev].pt.y == curr_y {
                op_p = self.outpts[op_p].prev;
            }
            while op_n != op_a && self.outpts[self.outpts[op_n].next].pt.y == curr_y {
                op_n = self.outpts[op_n].next;
            }
        } else {
            loop {
                let prev = self.outpts[op_p].prev;
                if prev != op_n && self.outpts[prev].pt.y == curr_y {
                    op_p = prev;
                } else {
                    break;
                }
            }
            loop {
                let next = self.outpts[op_n].next;
                if next != op_p && self.outpts[next].pt.y == curr_y {
                    op_n = next;
                } else {
                    break;
                }
            }
        }
        let mut hs = self.horz_seg_list[hs_idx].clone();
        let set_ok = self.set_horz_seg_heading_forward(&mut hs, op_p, op_n) && !self.outpts[hs.left_op].horz_claimed;
        let result = set_ok;
        if result {
            self.outpts[hs.left_op].horz_claimed = true;
        } else {
            hs.right_op = None; // (for sorting)
        }
        self.horz_seg_list[hs_idx] = hs;
        result
    }

    fn convert_horz_segs_to_joins(&mut self) {
        let mut j = 0usize;
        for i in 0..self.horz_seg_list.len() {
            if self.update_horz_segment(i) {
                j += 1;
            }
        }
        if j < 2 {
            return;
        }
        // HorzSegSorter: segments with no right_op sort last; otherwise by
        // descending left_op.pt.x (`stable_sort` in the original).
        self.horz_seg_list.sort_by(|hs1, hs2| {
            match (hs1.right_op, hs2.right_op) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(_), Some(_)) => {
                    let x2 = self.outpts[hs2.left_op].pt.x;
                    let x1 = self.outpts[hs1.left_op].pt.x;
                    // original predicate: `hs2.left_op->pt.x > hs1.left_op->pt.x`
                    // (true => hs1 before hs2); `x1.cmp(&x2)` reproduces
                    // that (including the tie => Equal case the original's
                    // boolean predicate implies) rather than a `>`
                    // comparison that would collapse ties into `Greater`.
                    x1.cmp(&x2)
                }
            }
        });

        let hs_end = j; // indices [0, hs_end) are the ones with a right_op, after sorting
        let mut hs1 = 0usize;
        while hs1 < hs_end.saturating_sub(1) {
            let mut hs2 = hs1 + 1;
            while hs2 < hs_end {
                let (h1_left, h1_right, h1_ltr) = {
                    let h = &self.horz_seg_list[hs1];
                    (h.left_op, h.right_op.unwrap(), h.left_to_right)
                };
                let (h2_left, h2_right, h2_ltr) = {
                    let h = &self.horz_seg_list[hs2];
                    (h.left_op, h.right_op.unwrap(), h.left_to_right)
                };
                if self.outpts[h2_left].pt.x >= self.outpts[h1_right].pt.x
                    || h2_ltr == h1_ltr
                    || self.outpts[h2_right].pt.x <= self.outpts[h1_left].pt.x
                {
                    hs2 += 1;
                    continue;
                }
                let curr_y = self.outpts[h1_left].pt.y;
                if h1_ltr {
                    let mut hs1_left = h1_left;
                    while self.outpts[self.outpts[hs1_left].next].pt.y == curr_y
                        && self.outpts[self.outpts[hs1_left].next].pt.x <= self.outpts[h2_left].pt.x
                    {
                        hs1_left = self.outpts[hs1_left].next;
                    }
                    let mut hs2_left = h2_left;
                    while self.outpts[self.outpts[hs2_left].prev].pt.y == curr_y
                        && self.outpts[self.outpts[hs2_left].prev].pt.x <= self.outpts[hs1_left].pt.x
                    {
                        hs2_left = self.outpts[hs2_left].prev;
                    }
                    self.horz_seg_list[hs1].left_op = hs1_left;
                    self.horz_seg_list[hs2].left_op = hs2_left;
                    let op1 = self.duplicate_op(hs1_left, true);
                    let op2 = self.duplicate_op(hs2_left, false);
                    self.horz_join_list.push(HorzJoin { op1, op2 });
                } else {
                    let mut hs1_left = h1_left;
                    while self.outpts[self.outpts[hs1_left].prev].pt.y == curr_y
                        && self.outpts[self.outpts[hs1_left].prev].pt.x <= self.outpts[h2_left].pt.x
                    {
                        hs1_left = self.outpts[hs1_left].prev;
                    }
                    let mut hs2_left = h2_left;
                    while self.outpts[self.outpts[hs2_left].next].pt.y == curr_y
                        && self.outpts[self.outpts[hs2_left].next].pt.x <= self.outpts[hs1_left].pt.x
                    {
                        hs2_left = self.outpts[hs2_left].next;
                    }
                    self.horz_seg_list[hs1].left_op = hs1_left;
                    self.horz_seg_list[hs2].left_op = hs2_left;
                    let op1 = self.duplicate_op(hs2_left, true);
                    let op2 = self.duplicate_op(hs1_left, false);
                    self.horz_join_list.push(HorzJoin { op1, op2 });
                }
                hs2 += 1;
            }
            hs1 += 1;
        }
    }

    fn move_splits(&mut self, from_or: OutRecIdx, to_or: OutRecIdx) {
        let from_splits = match self.outrecs[from_or].splits.take() {
            Some(s) => s,
            None => return,
        };
        self.outrecs[to_or].splits.get_or_insert_with(Vec::new).extend(from_splits);
        self.outrecs[from_or].splits = Some(Vec::new());
    }

    fn process_horz_joins(&mut self) {
        let joins = self.horz_join_list.clone();
        for j in joins {
            let or1 = self.get_real_outrec(Some(self.outpts[j.op1].outrec)).unwrap();
            let mut or2 = self.get_real_outrec(Some(self.outpts[j.op2].outrec)).unwrap();

            let op1b = self.outpts[j.op1].next;
            let op2b = self.outpts[j.op2].prev;
            self.outpts[j.op1].next = j.op2;
            self.outpts[j.op2].prev = j.op1;
            self.outpts[op1b].prev = op2b;
            self.outpts[op2b].next = op1b;

            if or1 == or2 {
                // 'join' is really a split
                or2 = self.new_outrec();
                self.outrecs[or2].pts = Some(op1b);
                self.fix_outrec_pts(or2);

                // if or1->pts has moved to or2 then update or1->pts!!
                if self.outpts[self.outrecs[or1].pts.unwrap()].outrec == or2 {
                    self.outrecs[or1].pts = Some(j.op1);
                    self.outpts[j.op1].outrec = or1;
                }

                if self.using_polytree {
                    let (or1_pts, or2_pts) = (self.outrecs[or1].pts.unwrap(), self.outrecs[or2].pts.unwrap());
                    if self.path1_inside_path2(or1_pts, or2_pts) {
                        // swap or1's & or2's pts
                        let tmp = self.outrecs[or1].pts;
                        self.outrecs[or1].pts = self.outrecs[or2].pts;
                        self.outrecs[or2].pts = tmp;
                        self.fix_outrec_pts(or1);
                        self.fix_outrec_pts(or2);
                        // or2 is now inside or1
                        self.outrecs[or2].owner = Some(or1);
                    } else if self.path1_inside_path2(or2_pts, or1_pts) {
                        self.outrecs[or2].owner = Some(or1);
                    } else {
                        self.outrecs[or2].owner = self.outrecs[or1].owner;
                    }

                    self.outrecs[or1].splits.get_or_insert_with(Vec::new).push(or2);
                } else {
                    self.outrecs[or2].owner = Some(or1);
                }
            } else {
                self.outrecs[or2].pts = None;
                if self.using_polytree {
                    self.set_owner(or2, or1);
                    self.move_splits(or2, or1); // #618
                } else {
                    self.outrecs[or2].owner = Some(or1);
                }
            }
        }
    }

    // ---- top-of-scanbeam / maxima handling ----

    fn do_top_of_scanbeam(&mut self, y: i64) {
        self.sel_head = None; // reused to flag horizontals (see push_horz)
        let mut e = self.ael_head;
        while let Some(cur) = e {
            // nb: 'cur' will never be horizontal here
            if self.active_arena[cur].top.y == y {
                self.active_arena[cur].curr_x = self.active_arena[cur].top.x;
                if self.is_maxima(cur) {
                    e = self.do_maxima(cur); // TOP OF BOUND (MAXIMA)
                    continue;
                } else {
                    // INTERMEDIATE VERTEX ...
                    if self.is_hot_edge(cur) {
                        let top = self.active_arena[cur].top;
                        self.add_out_pt(cur, top);
                    }
                    self.update_edge_into_ael(cur);
                    if is_horizontal(&self.active_arena[cur]) {
                        self.push_horz(cur); // horizontals are processed later
                    }
                }
            } else {
                self.active_arena[cur].curr_x = top_x(&self.active_arena[cur], y);
            }
            e = self.active_arena[cur].next_in_ael;
        }
    }

    fn do_maxima(&mut self, e: ActiveIdx) -> Option<ActiveIdx> {
        let prev_e = self.active_arena[e].prev_in_ael;
        let next_e = self.active_arena[e].next_in_ael;
        if self.is_open_end(e) {
            if self.is_hot_edge(e) {
                let top = self.active_arena[e].top;
                self.add_out_pt(e, top);
            }
            if !is_horizontal(&self.active_arena[e]) {
                if self.is_hot_edge(e) {
                    if self.is_front(e) {
                        self.outrecs[self.active_arena[e].outrec.unwrap()].front_edge = None;
                    } else {
                        self.outrecs[self.active_arena[e].outrec.unwrap()].back_edge = None;
                    }
                    self.active_arena[e].outrec = None;
                }
                self.delete_from_ael(e);
            }
            return next_e;
        }

        let max_pair = match self.get_maxima_pair(e) {
            Some(mp) => mp,
            None => return next_e, // e_max_pair is horizontal
        };

        if is_joined(&self.active_arena[e]) {
            let top = self.active_arena[e].top;
            self.split(e, top);
        }
        if is_joined(&self.active_arena[max_pair]) {
            let top = self.active_arena[max_pair].top;
            self.split(max_pair, top);
        }

        // only non-horizontal maxima here; process any edges between the
        // maxima pair ...
        let mut next_e = self.active_arena[e].next_in_ael;
        while next_e != Some(max_pair) {
            let ne = next_e.unwrap();
            let top = self.active_arena[e].top;
            self.intersect_edges(e, ne, top);
            self.swap_positions_in_ael(e, ne);
            next_e = self.active_arena[e].next_in_ael;
        }

        if self.is_open(e) {
            if self.is_hot_edge(e) {
                let top = self.active_arena[e].top;
                self.add_local_max_poly(e, max_pair, top);
            }
            self.delete_from_ael(max_pair);
            self.delete_from_ael(e);
            return if let Some(p) = prev_e { self.active_arena[p].next_in_ael } else { self.ael_head };
        }

        // e.next_in_ael == max_pair ...
        if self.is_hot_edge(e) {
            let top = self.active_arena[e].top;
            self.add_local_max_poly(e, max_pair, top);
        }

        self.delete_from_ael(e);
        self.delete_from_ael(max_pair);
        if let Some(p) = prev_e {
            self.active_arena[p].next_in_ael
        } else {
            self.ael_head
        }
    }

    fn split(&mut self, e: ActiveIdx, pt: Point64) {
        if self.active_arena[e].join_with == JoinWith::Right {
            self.active_arena[e].join_with = JoinWith::None;
            let n = self.active_arena[e].next_in_ael.unwrap();
            self.active_arena[n].join_with = JoinWith::None;
            self.add_local_min_poly(e, n, pt, true);
        } else {
            self.active_arena[e].join_with = JoinWith::None;
            let p = self.active_arena[e].prev_in_ael.unwrap();
            self.active_arena[p].join_with = JoinWith::None;
            self.add_local_min_poly(p, e, pt, true);
        }
    }

    fn check_join_left(&mut self, e: ActiveIdx, pt: Point64, check_curr_x: bool) {
        let prev = match self.active_arena[e].prev_in_ael {
            Some(p) => p,
            None => return,
        };
        if self.is_open(e) || !self.is_hot_edge(e) || self.is_open(prev) || !self.is_hot_edge(prev) {
            return;
        }
        let (e_top, prev_top, e_bot, prev_bot) =
            (self.active_arena[e].top, self.active_arena[prev].top, self.active_arena[e].bot, self.active_arena[prev].bot);
        if (pt.y < e_top.y + 2 || pt.y < prev_top.y + 2) && (e_bot.y > pt.y || prev_bot.y > pt.y) {
            return; // avoid trivial joins
        }

        if check_curr_x {
            if distance_from_line_sqrd(pt, prev_bot, prev_top) > 0.25 {
                return;
            }
        } else if self.active_arena[e].curr_x != self.active_arena[prev].curr_x {
            return;
        }
        if cross_product(e_top, pt, prev_top) != 0.0 {
            return;
        }

        let (e_outrec, prev_outrec) = (self.active_arena[e].outrec.unwrap(), self.active_arena[prev].outrec.unwrap());
        if self.outrecs[e_outrec].idx == self.outrecs[prev_outrec].idx {
            self.add_local_max_poly(prev, e, pt);
        } else if self.outrecs[e_outrec].idx < self.outrecs[prev_outrec].idx {
            self.join_outrec_paths(e, prev);
        } else {
            self.join_outrec_paths(prev, e);
        }
        self.active_arena[prev].join_with = JoinWith::Right;
        self.active_arena[e].join_with = JoinWith::Left;
    }

    fn check_join_right(&mut self, e: ActiveIdx, pt: Point64, check_curr_x: bool) {
        let next = match self.active_arena[e].next_in_ael {
            Some(n) => n,
            None => return,
        };
        if self.is_open(e) || !self.is_hot_edge(e) || self.is_open(next) || !self.is_hot_edge(next) {
            return;
        }
        let (e_top, next_top, e_bot, next_bot) =
            (self.active_arena[e].top, self.active_arena[next].top, self.active_arena[e].bot, self.active_arena[next].bot);
        if (pt.y < e_top.y + 2 || pt.y < next_top.y + 2) && (e_bot.y > pt.y || next_bot.y > pt.y) {
            return; // avoid trivial joins
        }

        if check_curr_x {
            if distance_from_line_sqrd(pt, next_bot, next_top) > 0.35 {
                return;
            }
        } else if self.active_arena[e].curr_x != self.active_arena[next].curr_x {
            return;
        }
        if cross_product(e_top, pt, next_top) != 0.0 {
            return;
        }

        let (e_outrec, next_outrec) = (self.active_arena[e].outrec.unwrap(), self.active_arena[next].outrec.unwrap());
        if self.outrecs[e_outrec].idx == self.outrecs[next_outrec].idx {
            self.add_local_max_poly(e, next, pt);
        } else if self.outrecs[e_outrec].idx < self.outrecs[next_outrec].idx {
            self.join_outrec_paths(e, next);
        } else {
            self.join_outrec_paths(next, e);
        }

        self.active_arena[e].join_with = JoinWith::Right;
        self.active_arena[next].join_with = JoinWith::Left;
    }

    // ---- top-level execution & output building ----

    fn execute_internal(&mut self, ct: ClipType, fillrule: FillRule, use_polytrees: bool) -> bool {
        self.cliptype = ct;
        self.fillrule = fillrule;
        self.using_polytree = use_polytrees;
        self.reset();
        if ct == ClipType::None {
            return true;
        }
        let mut y = match self.pop_scanline() {
            Some(y) => y,
            None => return true,
        };

        while self.succeeded {
            self.insert_local_minima_into_ael(y);
            while let Some(e) = self.pop_horz() {
                self.do_horizontal(e);
            }
            if !self.horz_seg_list.is_empty() {
                self.convert_horz_segs_to_joins();
                self.horz_seg_list.clear();
            }
            self.bot_y = y; // bottom of scanbeam
            match self.pop_scanline() {
                Some(new_y) => y = new_y, // y == new top of scanbeam
                None => break,
            }
            self.do_intersections(y);
            self.do_top_of_scanbeam(y);
            while let Some(e) = self.pop_horz() {
                self.do_horizontal(e);
            }
        }
        if self.succeeded {
            self.process_horz_joins();
        }
        self.succeeded
    }

    /// `BuildPath64`: walks an `OutPt` circular list into a plain `Path64`,
    /// deduping consecutive equal points and rejecting degenerate
    /// (collapsed-to-a-line or very-small-triangle) results.
    fn build_path64(&self, op: Option<OutPtIdx>, reverse: bool, is_open: bool) -> Option<Path64> {
        let op = op?;
        let next = self.outpts[op].next;
        let prev = self.outpts[op].prev;
        if next == op || (!is_open && next == prev) {
            return None;
        }

        let mut path = Vec::new();
        let (start, mut last_pt, mut op2);
        if reverse {
            last_pt = self.outpts[op].pt;
            start = op;
            op2 = prev;
        } else {
            start = self.outpts[op].next;
            last_pt = self.outpts[start].pt;
            op2 = self.outpts[start].next;
        }
        path.push(last_pt);

        while op2 != start {
            if self.outpts[op2].pt != last_pt {
                last_pt = self.outpts[op2].pt;
                path.push(last_pt);
            }
            op2 = if reverse { self.outpts[op2].prev } else { self.outpts[op2].next };
        }

        if path.len() == 3 && self.is_very_small_triangle(op2) {
            None
        } else {
            Some(path)
        }
    }

    /// `CheckBounds`: lazily computes (and caches, via `bounds.is_empty()`
    /// as the "not yet computed" sentinel -- matching the original's
    /// default-constructed all-zero `Rect64`) an outrec's final cleaned
    /// path and bounding box.
    fn check_bounds(&mut self, outrec: OutRecIdx) -> bool {
        if self.outrecs[outrec].pts.is_none() {
            return false;
        }
        if !self.outrecs[outrec].bounds.is_empty() {
            return true;
        }
        self.clean_collinear(Some(outrec));
        match self.build_path64(self.outrecs[outrec].pts, self.reverse_solution, false) {
            None => false,
            Some(path) => {
                self.outrecs[outrec].bounds = get_bounds(&path);
                self.outrecs[outrec].path = path;
                true
            }
        }
    }

    fn check_split_owner(&mut self, outrec: OutRecIdx, splits: &[OutRecIdx]) -> bool {
        for &split0 in splits {
            let split = match self.get_real_outrec(Some(split0)) {
                Some(s) => s,
                None => continue,
            };
            if split == outrec || self.outrecs[split].recursive_split == Some(outrec) {
                continue;
            }
            self.outrecs[split].recursive_split = Some(outrec); // prevent infinite loops

            let nested = self.outrecs[split].splits.clone();
            if let Some(nested) = nested {
                if self.check_split_owner(outrec, &nested) {
                    return true;
                }
            }
            if self.check_bounds(split)
                && self.is_valid_owner(outrec, Some(split))
                && self.outrecs[split].bounds.contains_rect(&self.outrecs[outrec].bounds)
                && self.path1_inside_path2(self.outrecs[outrec].pts.unwrap(), self.outrecs[split].pts.unwrap())
            {
                self.outrecs[outrec].owner = Some(split); // found in split
                return true;
            }
        }
        false
    }

    fn recursive_check_owners(&mut self, outrec: OutRecIdx, tree: &mut PolyTree64) {
        // pre-condition: outrec has valid bounds
        // post-condition: if a valid path, outrec will have a polypath
        if self.outrecs[outrec].polypath.is_some() || self.outrecs[outrec].bounds.is_empty() {
            return;
        }

        while let Some(owner) = self.outrecs[outrec].owner {
            let owner_splits = self.outrecs[owner].splits.clone();
            if let Some(splits) = owner_splits {
                if self.check_split_owner(outrec, &splits) {
                    break;
                }
            }
            if self.outrecs[owner].pts.is_some()
                && self.check_bounds(owner)
                && self.outrecs[owner].bounds.contains_rect(&self.outrecs[outrec].bounds)
                && self.path1_inside_path2(self.outrecs[outrec].pts.unwrap(), self.outrecs[owner].pts.unwrap())
            {
                break;
            }
            self.outrecs[outrec].owner = self.outrecs[owner].owner;
        }

        match self.outrecs[outrec].owner {
            Some(owner) => {
                if self.outrecs[owner].polypath.is_none() {
                    self.recursive_check_owners(owner, tree);
                }
                let path = self.outrecs[outrec].path.clone();
                let node = match self.outrecs[owner].polypath {
                    Some(p) => tree.add_child_to(p, path),
                    None => tree.add_child(path),
                };
                self.outrecs[outrec].polypath = Some(node);
            }
            None => {
                let path = self.outrecs[outrec].path.clone();
                self.outrecs[outrec].polypath = Some(tree.add_child(path));
            }
        }
    }

    fn build_paths64(&mut self) -> (Paths64, Paths64) {
        let mut solution_closed = Vec::with_capacity(self.outrecs.len());
        let mut solution_open = Vec::with_capacity(self.outrecs.len());

        // nb: `self.outrecs.len()` may grow in this loop (`CleanCollinear`
        // -> `FixSelfIntersects` can split a polygon into a new OutRec).
        let mut i = 0;
        while i < self.outrecs.len() {
            if self.outrecs[i].pts.is_some() {
                if self.outrecs[i].is_open {
                    if let Some(path) = self.build_path64(self.outrecs[i].pts, self.reverse_solution, true) {
                        solution_open.push(path);
                    }
                } else {
                    self.clean_collinear(Some(i));
                    // closed paths should always return a Positive orientation
                    if let Some(path) = self.build_path64(self.outrecs[i].pts, self.reverse_solution, false) {
                        solution_closed.push(path);
                    }
                }
            }
            i += 1;
        }
        (solution_closed, solution_open)
    }

    fn build_tree64(&mut self, tree: &mut PolyTree64) -> Paths64 {
        tree.clear();
        let mut open_paths = Vec::new();

        // nb: `self.outrecs.len()` is not fixed here either -- `CheckBounds`
        // can indirectly add OutRecs via `FixOutRecPts`/`CleanCollinear`.
        let mut i = 0;
        while i < self.outrecs.len() {
            if self.outrecs[i].pts.is_none() {
                i += 1;
                continue;
            }
            if self.outrecs[i].is_open {
                if let Some(path) = self.build_path64(self.outrecs[i].pts, self.reverse_solution, true) {
                    open_paths.push(path);
                }
                i += 1;
                continue;
            }

            if self.check_bounds(i) {
                self.recursive_check_owners(i, tree);
            }
            i += 1;
        }
        open_paths
    }

    // ---- public API ----

    /// `Clipper64::Execute` (`Paths64` overload).
    pub fn execute(&mut self, clip_type: ClipType, fill_rule: FillRule, closed_paths: &mut Paths64, open_paths: &mut Paths64) -> bool {
        closed_paths.clear();
        open_paths.clear();
        if self.execute_internal(clip_type, fill_rule, false) {
            let (c, o) = self.build_paths64();
            *closed_paths = c;
            *open_paths = o;
        }
        self.clean_up();
        self.succeeded
    }

    /// `Clipper64::Execute` (`PolyTree64` overload).
    pub fn execute_tree(&mut self, clip_type: ClipType, fill_rule: FillRule, polytree: &mut PolyTree64, open_paths: &mut Paths64) -> bool {
        if self.execute_internal(clip_type, fill_rule, true) {
            open_paths.clear();
            polytree.clear();
            *open_paths = self.build_tree64(polytree);
        }
        self.clean_up();
        self.succeeded
    }
}
