//! The `delaunator` crate (active, `delaunay::Triangulation`) against the
//! INACTIVE from-scratch port (`delaunay_port::Triangulation`) on the port's
//! own test cases and on random point sets.
//!
//! Compared: the arrays exactly; the set of triangles (as unordered
//! triples); the hull as a set; and what the ratsnest reads (the triangle
//! edges plus the adjacency diagonals, `ratsnest.rs`) as an edge set. Where
//! points are co-circular the Delaunay triangulation is not unique, so the
//! triangle sets may differ there; then both must be Delaunay and the
//! Euclidean MST over each edge set must have the same length (the MST the
//! ratsnest draws).

use eda_connectivity::delaunay::{Triangulation, INVALID_INDEX};
use eda_connectivity::delaunay_port::Triangulation as Port;
use std::collections::BTreeSet;

fn xorshift(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

fn tri_set(t: &[usize]) -> BTreeSet<[usize; 3]> {
    t.chunks_exact(3)
        .map(|c| {
            let mut a = [c[0], c[1], c[2]];
            a.sort();
            a
        })
        .collect()
}

/// The ratsnest's candidate edges (`ratsnest.rs`: every triangle side, and
/// for every interior half-edge the start vertices of the two half-edges).
fn edge_set(triangles: &[usize], halfedges: &[usize]) -> BTreeSet<(usize, usize)> {
    let mut out = BTreeSet::new();
    let mut add = |a: usize, b: usize| {
        out.insert((a.min(b), a.max(b)));
    };
    for k in (0..triangles.len()).step_by(3) {
        add(triangles[k], triangles[k + 1]);
        add(triangles[k + 1], triangles[k + 2]);
        add(triangles[k + 2], triangles[k]);
    }
    for (e, &he) in halfedges.iter().enumerate() {
        if he != INVALID_INDEX {
            add(triangles[e], triangles[he]);
        }
    }
    out
}

fn mst_length(coords: &[f64], edges: &BTreeSet<(usize, usize)>) -> f64 {
    let n = coords.len() / 2;
    let d = |a: usize, b: usize| ((coords[2 * a] - coords[2 * b]).powi(2) + (coords[2 * a + 1] - coords[2 * b + 1]).powi(2)).sqrt();
    let mut es: Vec<(f64, usize, usize)> = edges.iter().map(|&(a, b)| (d(a, b), a, b)).collect();
    es.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut total = 0.0;
    for (w, a, b) in es {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
            total += w;
        }
    }
    total
}

fn is_delaunay(coords: &[f64], t: &[usize]) -> bool {
    let n = coords.len() / 2;
    let p = |i: usize| (coords[2 * i], coords[2 * i + 1]);
    t.chunks_exact(3).all(|c| {
        let (a, b, cc) = (p(c[0]), p(c[1]), p(c[2]));
        (0..n).filter(|i| !c.contains(i)).all(|i| {
            let d = p(i);
            let (adx, ady, bdx, bdy, cdx, cdy) = (a.0 - d.0, a.1 - d.1, b.0 - d.0, b.1 - d.1, cc.0 - d.0, cc.1 - d.1);
            let det = (adx * adx + ady * ady) * (bdx * cdy - cdx * bdy) - (bdx * bdx + bdy * bdy) * (adx * cdy - cdx * ady)
                + (cdx * cdx + cdy * cdy) * (adx * bdy - bdx * ady);
            let orient = (b.0 - a.0) * (cc.1 - a.1) - (b.1 - a.1) * (cc.0 - a.0);
            let scale = (adx.abs() + ady.abs() + bdx.abs() + bdy.abs() + cdx.abs() + cdy.abs()).powi(4);
            // Inside the circumcircle (beyond rounding) means not Delaunay.
            if orient > 0.0 { det <= 1e-9 * scale } else { det >= -1e-9 * scale }
        })
    })
}

#[derive(Default, Debug)]
struct Tally {
    sets: usize,
    exact: usize,
    same_triangles: usize,
    same_hull: usize,
    same_edges: usize,
    same_mst: usize,
    hull_diffs: Vec<String>,
}

fn compare(t: &mut Tally, coords: &[f64]) {
    let a = Triangulation::new(coords);
    let b = Port::new(coords);
    t.sets += 1;
    if a.triangles == b.triangles && a.halfedges == b.halfedges && a.hull == b.hull {
        t.exact += 1;
    }
    let same_tris = tri_set(&a.triangles) == tri_set(&b.triangles);
    if same_tris {
        t.same_triangles += 1;
        let ha: BTreeSet<usize> = a.hull.iter().copied().collect();
        let hb: BTreeSet<usize> = b.hull.iter().copied().collect();
        // The hull is not read by the ratsnest; the port's is wrong in
        // places (the duplicate-point case leaves it as one point), so it is
        // counted, not asserted.
        if ha == hb {
            t.same_hull += 1;
        } else if t.hull_diffs.len() < 5 {
            t.hull_diffs.push(format!("{coords:?}: delaunator {ha:?}, port {hb:?}"));
        }
    } else {
        assert!(is_delaunay(coords, &a.triangles), "delaunator's triangulation is not Delaunay: {coords:?}");
        assert!(is_delaunay(coords, &b.triangles), "the port's triangulation is not Delaunay: {coords:?}");
    }
    let (ea, eb) = (edge_set(&a.triangles, &a.halfedges), edge_set(&b.triangles, &b.halfedges));
    if ea == eb {
        t.same_edges += 1;
    }
    let (ma, mb) = (mst_length(coords, &ea), mst_length(coords, &eb));
    assert!((ma - mb).abs() <= 1e-9 * ma.max(1.0), "MST lengths differ ({ma} vs {mb}): {coords:?}");
    t.same_mst += 1;
}

#[test]
fn delaunator_crate_matches_the_port() {
    let mut t = Tally::default();
    // The port's own unit-test inputs.
    compare(&mut t, &[0.0, 0.0, 10.0, 0.0, 5.0, 10.0]);
    compare(&mut t, &[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]);
    compare(&mut t, &[0.0, 0.0, 10.0, 0.0, 20.0, 0.0, 30.0, 0.0]);
    compare(&mut t, &[0.0, 0.0, 10.0, 0.0, 5.0, 10.0, 5.0, 10.0, 2.0, 3.0]);
    let mut grid = Vec::new();
    for gy in 0..6 {
        for gx in 0..6 {
            grid.extend([gx as f64 * 37.0, gy as f64 * 53.0]);
        }
    }
    compare(&mut t, &grid);
    for seed in [0x1234_5678_9abc_def1u64, 0xdead_beef_1234_5678, 0x0f0f_0f0f_f0f0_f0f0, 1, 42] {
        let mut s = seed;
        for trial in 0..12 {
            let n = 5 + trial * 7;
            let coords: Vec<f64> = (0..2 * n).map(|_| xorshift(&mut s) * 100_000.0).collect();
            compare(&mut t, &coords);
        }
    }
    let mut s = 7u64;
    for _ in 0..3 {
        let mut coords = Vec::new();
        for gy in 0..8 {
            for gx in 0..8 {
                let jitter = (xorshift(&mut s) * 3.0).round();
                coords.extend([gx as f64 * 10.0 + jitter, gy as f64 * 10.0]);
            }
        }
        compare(&mut t, &coords);
    }
    // More: pad-like positions on a 0.05 mm grid in nm (as the ratsnest gets them), 3..200 points.
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    for k in 0..2000 {
        let n = 3 + (k % 198);
        let coords: Vec<f64> = (0..2 * n).map(|_| (xorshift(&mut s) * 2000.0).round() * 50_000.0).collect();
        compare(&mut t, &coords);
    }
    eprintln!("{t:?}");
}
