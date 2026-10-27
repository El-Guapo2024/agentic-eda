//! `SimplifyPath`/`SimplifyPaths` from `clipper.h` (header-only templates in
//! Clipper2).

use crate::core::{self, Path64, Point64, Paths64};

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

