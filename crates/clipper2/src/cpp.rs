//! The active Clipper2 back end: KiCad's vendored Clipper2 2.0.1 C++
//! (`third_party/clipper2`, copied from `kicad-src-25ec2335/thirdparty/clipper2`
//! and built with `USINGZ` as KiCad's `thirdparty/clipper2/CMakeLists.txt`
//! does), compiled by `build.rs` with `cc` and called through the small C
//! ABI in `cpp/shim.cpp`.
//!
//! The types keep the API of the Rust port (`crate::port`), so callers do not
//! change. Inputs stay in Rust: `Clipper64` and `ClipperOffset` record their
//! `AddPaths` calls and replay them, in order, into a fresh C++ object on
//! every `Execute`. Clipper2's `Execute` resets its scan state from the
//! stored minima on every call, so a fresh object given the same `AddPaths`
//! sequence is the same computation, and the Rust types stay `Send` with no
//! C++ object to free. The results come back through a sink callback; a
//! `PolyTree64` comes back in pre-order (each node before its children,
//! children in Clipper2's order), which is the order `PolyPath64`'s
//! iterators give.

use std::ffi::c_void;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

use crate::core::{FillRule, Path64, Paths64, Point64, Rect64};
use crate::types::{ClipType, EndType, JoinType, PathType, PolyTree64, ZCallback64};

#[repr(C)]
struct CPath {
    pts: *const Point64,
    len: usize,
}

#[repr(C)]
struct CGroup {
    paths: *const CPath,
    count: usize,
    a: i32,
    b: i32,
}

type Sink = extern "C" fn(ud: *mut c_void, kind: i32, parent: i64, pts: *const Point64, n: usize);
type ZCb = extern "C" fn(ud: *mut c_void, e1bot: *const Point64, e1top: *const Point64, e2bot: *const Point64, e2top: *const Point64, pt: *mut Point64);

extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn c2_clip(
        groups: *const CGroup,
        ngroups: usize,
        clip_type: i32,
        fill_rule: i32,
        preserve_collinear: i32,
        reverse_solution: i32,
        default_z: i64,
        zcb: Option<ZCb>,
        zud: *mut c_void,
        as_tree: i32,
        sink: Sink,
        ud: *mut c_void,
        error_code: *mut i32,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn c2_offset(
        groups: *const CGroup,
        ngroups: usize,
        delta: f64,
        miter_limit: f64,
        arc_tolerance: f64,
        preserve_collinear: i32,
        reverse_solution: i32,
        as_tree: i32,
        sink: Sink,
        ud: *mut c_void,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn c2_rect_clip(left: i64, top: i64, right: i64, bottom: i64, paths: *const CPath, count: usize, free_fn: i32, sink: Sink, ud: *mut c_void) -> i32;
    fn c2_simplify_path(pts: *const Point64, len: usize, epsilon: f64, is_closed_path: i32, sink: Sink, ud: *mut c_void) -> i32;
}

/// What the sink collects: closed paths, open paths, and tree nodes with the
/// index (in emission order) of their parent.
#[derive(Default)]
struct Out {
    closed: Paths64,
    open: Paths64,
    nodes: Vec<(i64, Path64)>,
}

extern "C" fn sink(ud: *mut c_void, kind: i32, parent: i64, pts: *const Point64, n: usize) {
    // SAFETY: `ud` is the `&mut Out` passed by the caller below, alive for the
    // whole C call; `pts` points at `n` Point64s owned by C++ for this call.
    let out = unsafe { &mut *(ud as *mut Out) };
    let path = if n == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(pts, n) }.to_vec() };
    match kind {
        0 => out.closed.push(path),
        1 => out.open.push(path),
        _ => out.nodes.push((parent, path)),
    }
}

/// Rebuilds a `PolyTree64` from the pre-order node list.
fn fill_tree(nodes: Vec<(i64, Path64)>, tree: &mut PolyTree64) {
    let mut ids = Vec::with_capacity(nodes.len());
    for (parent, path) in nodes {
        let id = if parent < 0 { tree.add_child(path) } else { tree.add_child_to(ids[parent as usize], path) };
        ids.push(id);
    }
}

fn c_paths(paths: &[Path64]) -> Vec<CPath> {
    paths.iter().map(|p| CPath { pts: p.as_ptr(), len: p.len() }).collect()
}

fn clip_type_c(ct: ClipType) -> i32 {
    match ct {
        ClipType::None => 0,
        ClipType::Intersection => 1,
        ClipType::Union => 2,
        ClipType::Difference => 3,
        ClipType::Xor => 4,
    }
}

fn fill_rule_c(fr: FillRule) -> i32 {
    match fr {
        FillRule::EvenOdd => 0,
        FillRule::NonZero => 1,
        FillRule::Positive => 2,
        FillRule::Negative => 3,
    }
}

fn join_type_c(jt: JoinType) -> i32 {
    match jt {
        JoinType::Square => 0,
        JoinType::Bevel => 1,
        JoinType::Round => 2,
        JoinType::Miter => 3,
    }
}

fn end_type_c(et: EndType) -> i32 {
    match et {
        EndType::Polygon => 0,
        EndType::Joined => 1,
        EndType::Butt => 2,
        EndType::Square => 3,
        EndType::Round => 4,
    }
}

/// The Z callback's state for one C call: the user's closure and any panic
/// it raised (re-raised once C++ has returned, never unwound through C++).
struct ZState<'a> {
    cb: &'a mut ZCallback64,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

extern "C" fn z_trampoline(ud: *mut c_void, e1bot: *const Point64, e1top: *const Point64, e2bot: *const Point64, e2top: *const Point64, pt: *mut Point64) {
    // SAFETY: `ud` is the `ZState` built in `Clipper64::run`, alive for the C
    // call; the points are C++ references valid for this callback.
    let st = unsafe { &mut *(ud as *mut ZState) };
    if st.panic.is_some() {
        return;
    }
    let (a, b, c, d) = unsafe { (*e1bot, *e1top, *e2bot, *e2top) };
    let pt = unsafe { &mut *pt };
    if let Err(p) = catch_unwind(AssertUnwindSafe(|| (st.cb)(a, b, c, d, pt))) {
        st.panic = Some(p);
    }
}

/// `Clipper64` (`clipper.engine.h`), backed by the C++ engine.
pub struct Clipper64 {
    groups: Vec<(Paths64, PathType, bool)>,
    preserve_collinear: bool,
    reverse_solution: bool,
    error_code: i32,
    /// `ClipperBase::DefaultZ` (`USINGZ`).
    pub default_z: i64,
    z_callback: Option<ZCallback64>,
}

impl Default for Clipper64 {
    fn default() -> Self {
        // `ClipperBase`'s defaults: PreserveCollinear true, ReverseSolution false.
        Clipper64 { groups: Vec::new(), preserve_collinear: true, reverse_solution: false, error_code: 0, default_z: 0, z_callback: None }
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
    /// `Clipper64::SetZCallback`.
    pub fn set_z_callback(&mut self, cb: ZCallback64) {
        self.z_callback = Some(cb);
    }
    /// `ClipperBase::Clear`: drops every path added so far.
    pub fn clear(&mut self) {
        self.groups.clear();
    }
    pub fn add_path(&mut self, path: &Path64, polytype: PathType, is_open: bool) {
        self.add_paths(std::slice::from_ref(path), polytype, is_open);
    }
    pub fn add_paths(&mut self, paths: &[Path64], polytype: PathType, is_open: bool) {
        self.groups.push((paths.to_vec(), polytype, is_open));
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

    fn run(&mut self, clip_type: ClipType, fill_rule: FillRule, as_tree: bool) -> (bool, Out) {
        let cpaths: Vec<Vec<CPath>> = self.groups.iter().map(|(p, _, _)| c_paths(p)).collect();
        let cgroups: Vec<CGroup> = self
            .groups
            .iter()
            .zip(&cpaths)
            .map(|((_, pt, open), cp)| CGroup { paths: cp.as_ptr(), count: cp.len(), a: if *pt == PathType::Subject { 0 } else { 1 }, b: *open as i32 })
            .collect();
        let mut out = Out::default();
        let mut err = 0i32;
        let mut zstate = self.z_callback.as_mut().map(|cb| ZState { cb, panic: None });
        let (zcb, zud): (Option<ZCb>, *mut c_void) = match zstate.as_mut() {
            Some(st) => (Some(z_trampoline as ZCb), st as *mut ZState as *mut c_void),
            None => (None, std::ptr::null_mut()),
        };
        // SAFETY: every pointer handed over (paths, groups, out, zstate)
        // outlives the call; C++ copies the inputs and never keeps a pointer.
        let ok = unsafe {
            c2_clip(
                cgroups.as_ptr(),
                cgroups.len(),
                clip_type_c(clip_type),
                fill_rule_c(fill_rule),
                self.preserve_collinear as i32,
                self.reverse_solution as i32,
                self.default_z,
                zcb,
                zud,
                as_tree as i32,
                sink,
                &mut out as *mut Out as *mut c_void,
                &mut err,
            )
        };
        if let Some(p) = zstate.and_then(|s| s.panic) {
            resume_unwind(p);
        }
        self.error_code = err;
        (ok != 0, out)
    }

    /// `Clipper64::Execute` (`Paths64` overload).
    pub fn execute(&mut self, clip_type: ClipType, fill_rule: FillRule, closed_paths: &mut Paths64, open_paths: &mut Paths64) -> bool {
        let (ok, out) = self.run(clip_type, fill_rule, false);
        *closed_paths = out.closed;
        *open_paths = out.open;
        ok
    }

    /// `Clipper64::Execute` (`PolyTree64` overload). As in C++, the tree and
    /// the open paths are left as they were when the execution fails.
    pub fn execute_tree(&mut self, clip_type: ClipType, fill_rule: FillRule, polytree: &mut PolyTree64, open_paths: &mut Paths64) -> bool {
        let (ok, out) = self.run(clip_type, fill_rule, true);
        if ok {
            polytree.clear();
            *open_paths = out.open;
            fill_tree(out.nodes, polytree);
        }
        ok
    }
}

/// `ClipperOffset` (`clipper.offset.h`), backed by the C++ class.
pub struct ClipperOffset {
    groups: Vec<(Paths64, JoinType, EndType)>,
    error_code: i32,
    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: bool,
    reverse_solution: bool,
}

impl ClipperOffset {
    pub fn new(miter_limit: f64, arc_tolerance: f64, preserve_collinear: bool, reverse_solution: bool) -> Self {
        ClipperOffset { groups: Vec::new(), error_code: 0, miter_limit, arc_tolerance, preserve_collinear, reverse_solution }
    }
    pub fn error_code(&self) -> i32 {
        self.error_code
    }
    pub fn miter_limit(&self) -> f64 {
        self.miter_limit
    }
    pub fn set_miter_limit(&mut self, v: f64) {
        self.miter_limit = v;
    }
    pub fn arc_tolerance(&self) -> f64 {
        self.arc_tolerance
    }
    pub fn set_arc_tolerance(&mut self, v: f64) {
        self.arc_tolerance = v;
    }
    pub fn preserve_collinear(&self) -> bool {
        self.preserve_collinear
    }
    pub fn set_preserve_collinear(&mut self, v: bool) {
        self.preserve_collinear = v;
    }
    pub fn reverse_solution(&self) -> bool {
        self.reverse_solution
    }
    pub fn set_reverse_solution(&mut self, v: bool) {
        self.reverse_solution = v;
    }
    pub fn clear(&mut self) {
        self.groups.clear();
    }
    pub fn add_path(&mut self, path: &Path64, jt: JoinType, et: EndType) {
        self.add_paths(std::slice::from_ref(path), jt, et);
    }
    /// `ClipperOffset::AddPaths`: an empty list adds no group.
    pub fn add_paths(&mut self, paths: &[Path64], jt: JoinType, et: EndType) {
        if paths.is_empty() {
            return;
        }
        self.groups.push((paths.to_vec(), jt, et));
    }

    fn run(&mut self, delta: f64, as_tree: bool) -> Out {
        let cpaths: Vec<Vec<CPath>> = self.groups.iter().map(|(p, _, _)| c_paths(p)).collect();
        let cgroups: Vec<CGroup> = self
            .groups
            .iter()
            .zip(&cpaths)
            .map(|((_, jt, et), cp)| CGroup { paths: cp.as_ptr(), count: cp.len(), a: join_type_c(*jt), b: end_type_c(*et) })
            .collect();
        let mut out = Out::default();
        // SAFETY: as in `Clipper64::run`.
        self.error_code = unsafe {
            c2_offset(
                cgroups.as_ptr(),
                cgroups.len(),
                delta,
                self.miter_limit,
                self.arc_tolerance,
                self.preserve_collinear as i32,
                self.reverse_solution as i32,
                as_tree as i32,
                sink,
                &mut out as *mut Out as *mut c_void,
            )
        };
        out
    }

    /// `ClipperOffset::Execute(double, Paths64&)`.
    pub fn execute(&mut self, delta: f64, paths: &mut Paths64) {
        *paths = self.run(delta, false).closed;
    }

    /// `ClipperOffset::Execute(double, PolyTree64&)`.
    pub fn execute_tree(&mut self, delta: f64, polytree: &mut PolyTree64) {
        polytree.clear();
        fill_tree(self.run(delta, true).nodes, polytree);
    }
}

fn rect_clip_c(rect: Rect64, paths: &[Path64], free_fn: bool) -> Paths64 {
    let cp = c_paths(paths);
    let mut out = Out::default();
    // SAFETY: as in `Clipper64::run`.
    let rc = unsafe { c2_rect_clip(rect.left, rect.top, rect.right, rect.bottom, cp.as_ptr(), cp.len(), free_fn as i32, sink, &mut out as *mut Out as *mut c_void) };
    assert_eq!(rc, 0, "Clipper2 RectClip threw a C++ exception");
    out.closed
}

/// `RectClip64` (`clipper.rectclip.h`), backed by the C++ class.
pub struct RectClip64 {
    rect: Rect64,
}

impl RectClip64 {
    pub fn new(rect: Rect64) -> Self {
        RectClip64 { rect }
    }
    /// `RectClip64::Execute`.
    pub fn execute(&mut self, paths: &[Path64]) -> Paths64 {
        rect_clip_c(self.rect, paths, false)
    }
}

/// `RectClip` (the `clipper.h` wrapper).
pub fn rect_clip(rect: Rect64, paths: &[Path64]) -> Paths64 {
    rect_clip_c(rect, paths, true)
}

/// `SimplifyPath` (`clipper.h`).
pub fn simplify_path(path: &[Point64], epsilon: f64, is_closed_path: bool) -> Path64 {
    let mut out = Out::default();
    // SAFETY: as in `Clipper64::run`.
    let rc = unsafe { c2_simplify_path(path.as_ptr(), path.len(), epsilon, is_closed_path as i32, sink, &mut out as *mut Out as *mut c_void) };
    assert_eq!(rc, 0, "Clipper2 SimplifyPath threw a C++ exception");
    out.closed.pop().unwrap_or_default()
}

/// `SimplifyPaths`.
pub fn simplify_paths(paths: &[Path64], epsilon: f64, is_closed_path: bool) -> Paths64 {
    paths.iter().map(|p| simplify_path(p, epsilon, is_closed_path)).collect()
}
