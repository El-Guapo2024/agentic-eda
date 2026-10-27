//! Clipper2 for the workspace: `Clipper64`, `ClipperOffset`, `RectClip64`
//! and `SimplifyPath`, the parts KiCad's `SHAPE_POLY_SET` uses.
//!
//! Two back ends with one API:
//! - **`cpp` (active, default):** KiCad's own vendored Clipper2 2.0.1 C++
//!   (`third_party/clipper2`, Boost 1.0, copied from
//!   `kicad-src-25ec2335/thirdparty/clipper2`), compiled by `build.rs` with
//!   `cc` and the `USINGZ` define KiCad builds it with. This follows the
//!   reuse rule in `docs/ARCHITECTURE.md`: a public C++ library the backend
//!   can call is called, not ported.
//! - **`port` (INACTIVE since 2026-10-28):** the faithful Rust port of the
//!   same sources, kept for a future all-Rust build. The `rust-port` cargo
//!   feature switches every caller to it. It compiles in every build, and
//!   `tests/differential.rs` checks it against the binding.
//!
//! `core` (the point, rect and path types and the header-only helpers of
//! `clipper.core.h`) and `types` (the enums and `PolyTree64`) are shared.
//!
//! Original licence: Boost Software License 1.0.

pub mod core;
pub mod cpp;
pub mod port;
pub mod types;

pub use core::{
    area, area_paths, cross_product, cross_product_vec, dot_product, dot_product_vec, get_bounds, get_bounds_paths, get_intersect_point,
    get_closest_point_on_segment, is_positive, near_equal, point_in_polygon, segments_intersect, strip_duplicates, FillRule, Path64, PathD, Paths64, PathsD,
    Point64, PointD, PointInPolygonResult, Rect64, MAX_COORD, MIN_COORD, PI,
};
pub use types::{poly_tree_to_paths64, ClipType, EndType, JoinType, PathType, PolyPathNode, PolyTree64, ZCallback64};

#[cfg(not(feature = "rust-port"))]
pub use cpp::{rect_clip, simplify_path, simplify_paths, Clipper64, ClipperOffset, RectClip64};
#[cfg(feature = "rust-port")]
pub use port::{rect_clip, simplify_path, simplify_paths, Clipper64, ClipperOffset, RectClip64};

/// `InflatePaths` (the `Paths64` convenience wrapper from `clipper.h`).
pub fn inflate_paths(paths: &Paths64, delta: f64, jt: JoinType, et: EndType, miter_limit: f64, arc_tolerance: f64) -> Paths64 {
    if delta == 0.0 {
        return paths.clone();
    }
    let mut co = ClipperOffset::new(miter_limit, arc_tolerance, false, false);
    co.add_paths(paths, jt, et);
    let mut solution = Vec::new();
    co.execute(delta, &mut solution);
    solution
}

/// `Union`/`Intersect`/`Difference`/`Xor` (the `Paths64` convenience
/// wrappers from `clipper.h`).
pub fn boolean_op(clip_type: ClipType, fill_rule: FillRule, subjects: &Paths64, clips: &Paths64) -> Paths64 {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    c.add_clip(clips);
    let mut result = Vec::new();
    let mut open = Vec::new();
    c.execute(clip_type, fill_rule, &mut result, &mut open);
    result
}

pub fn union_paths(subjects: &Paths64, clips: &Paths64, fill_rule: FillRule) -> Paths64 {
    boolean_op(ClipType::Union, fill_rule, subjects, clips)
}
pub fn intersect_paths(subjects: &Paths64, clips: &Paths64, fill_rule: FillRule) -> Paths64 {
    boolean_op(ClipType::Intersection, fill_rule, subjects, clips)
}
pub fn difference_paths(subjects: &Paths64, clips: &Paths64, fill_rule: FillRule) -> Paths64 {
    boolean_op(ClipType::Difference, fill_rule, subjects, clips)
}
pub fn xor_paths(subjects: &Paths64, clips: &Paths64, fill_rule: FillRule) -> Paths64 {
    boolean_op(ClipType::Xor, fill_rule, subjects, clips)
}
