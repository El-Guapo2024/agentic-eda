//! A faithful Rust port of Clipper2's `Clipper64` engine (the only
//! numeric/PolyTree flavor KiCad's `SHAPE_POLY_SET` ever instantiates), its
//! `ClipperOffset` path-offsetting module, and `RectClip64`.
//!
//! Ported from the KiCad vendored snapshot of Clipper2
//! (`thirdparty/clipper2/Clipper2Lib`) at commit `8303b2ad`, not reimplemented
//! from memory -- see each module's doc comment for the specific source
//! files and the handful of deliberate, documented scope cuts (no `ClipperD`
//! double-precision path, no `ReuseableDataContainer64`, no
//! `DeltaCallback64`, no `RectClipLines64`: none of these have a call site
//! anywhere in KiCad's `pcbnew`/`libs/kimath`).
//!
//! Original license: Boost Software License 1.0 (same as this port, being a
//! derivative work).

pub mod core;
pub mod engine;
pub mod offset;
pub mod rectclip;

pub use core::{
    area, area_paths, cross_product, cross_product_vec, dot_product, dot_product_vec, get_bounds, get_bounds_paths, get_intersect_point,
    get_closest_point_on_segment, is_positive, near_equal, point_in_polygon, segments_intersect, strip_duplicates, FillRule, Path64, PathD, Paths64, PathsD,
    Point64, PointD, PointInPolygonResult, Rect64, MAX_COORD, MIN_COORD, PI,
};
pub use engine::{poly_tree_to_paths64, ClipType, Clipper64, PathType, PolyPathNode, PolyTree64, ZCallback64};
pub use offset::{ClipperOffset, EndType, JoinType};
pub use rectclip::{rect_clip, RectClip64};

/// `SimplifyPath` (from `clipper.h`): Douglas-Peucker-style vertex removal
/// based on each point's perpendicular distance from the (dynamically
/// updated) line through its surviving neighbours. Used directly by KiCad's
/// `SHAPE_POLY_SET::inflate2` (`Clipper2Lib::SimplifyPaths(paths, ..., true)`).
pub fn simplify_path(path: &[Point64], epsilon: f64, is_closed_path: bool) -> Path64 {
    let len = path.len();
    if len < 4 {
        return path.to_vec();
    }
    let high = len - 1;
    let eps_sqr = core::sqr(epsilon);

    let mut flags = vec![false; len];
    let mut dist_sqr = vec![0.0f64; len];

    fn perp_dist_sqrd(pt: Point64, line1: Point64, line2: Point64) -> f64 {
        let a = (pt.x - line1.x) as f64;
        let b = (pt.y - line1.y) as f64;
        let c = (line2.x - line1.x) as f64;
        let d = (line2.y - line1.y) as f64;
        if c == 0.0 && d == 0.0 {
            return 0.0;
        }
        core::sqr(a * d - c * b) / (c * c + d * d)
    }

    if is_closed_path {
        dist_sqr[0] = perp_dist_sqrd(path[0], path[high], path[1]);
        dist_sqr[high] = perp_dist_sqrd(path[high], path[0], path[high - 1]);
    } else {
        dist_sqr[0] = f64::MAX;
        dist_sqr[high] = f64::MAX;
    }
    for i in 1..high {
        dist_sqr[i] = perp_dist_sqrd(path[i], path[i - 1], path[i + 1]);
    }

    fn get_next(current: usize, high: usize, flags: &[bool]) -> usize {
        let mut current = current + 1;
        while current <= high && flags[current] {
            current += 1;
        }
        if current <= high {
            return current;
        }
        current = 0;
        while flags[current] {
            current += 1;
        }
        current
    }

    fn get_prior(current: usize, high: usize, flags: &[bool]) -> usize {
        let mut current = if current == 0 { high } else { current - 1 };
        while current > 0 && flags[current] {
            current -= 1;
        }
        if !flags[current] {
            return current;
        }
        current = high;
        while flags[current] {
            current -= 1;
        }
        current
    }

    #[allow(unused_assignments)]
    let mut prior = high; // matches the original's never-read initial value
    let mut curr = 0usize;
    loop {
        if dist_sqr[curr] > eps_sqr {
            let start = curr;
            loop {
                curr = get_next(curr, high, &flags);
                if curr == start || dist_sqr[curr] <= eps_sqr {
                    break;
                }
            }
            if curr == start {
                break;
            }
        }

        prior = get_prior(curr, high, &flags);
        let mut next = get_next(curr, high, &flags);
        if next == prior {
            break;
        }

        let prior2;
        if dist_sqr[next] < dist_sqr[curr] {
            prior2 = prior;
            prior = curr;
            curr = next;
            next = get_next(next, high, &flags);
        } else {
            prior2 = get_prior(prior, high, &flags);
        }

        flags[curr] = true;
        curr = next;
        next = get_next(next, high, &flags);

        if is_closed_path || (curr != high && curr != 0) {
            dist_sqr[curr] = perp_dist_sqrd(path[curr], path[prior], path[next]);
        }
        if is_closed_path || (prior != 0 && prior != high) {
            dist_sqr[prior] = perp_dist_sqrd(path[prior], path[prior2], path[curr]);
        }
    }

    (0..len).filter(|&i| !flags[i]).map(|i| path[i]).collect()
}

/// `SimplifyPaths`.
pub fn simplify_paths(paths: &[Path64], epsilon: f64, is_closed_path: bool) -> Paths64 {
    paths.iter().map(|p| simplify_path(p, epsilon, is_closed_path)).collect()
}

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
