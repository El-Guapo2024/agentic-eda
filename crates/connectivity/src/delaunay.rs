//! The Delaunay triangulation the ratsnest runs Kruskal's MST over
//! (`RN_NET::TRIANGULATOR_STATE::Triangulate` in
//! `pcbnew/ratsnest/ratsnest_data.cpp`, which calls KiCad's
//! `thirdparty/delaunator`, delaunator-cpp, MIT).
//!
//! The triangulation comes from the `delaunator` crate (ISC), the Rust port
//! of Delaunator by Delaunator's own author: the reuse rule of
//! `docs/ARCHITECTURE.md` (call a public library, do not keep a port of
//! it). This module keeps the API the ratsnest was written against; the
//! earlier from-scratch port is `crate::delaunay_port` (INACTIVE), and the
//! `rust-port` cargo feature switches back to it.
//!
//! What callers need: [`Triangulation::new`] takes `[x, y, x, y, ...]`
//! point coordinates and returns `triangles` (point indices, 3 per
//! triangle, CCW in Delaunator's y-up sense) and `halfedges` (for each
//! directed edge `e` of `triangles`, the opposite directed edge in the
//! adjacent triangle, or [`INVALID_INDEX`] on the hull) -- the two arrays
//! `TRIANGULATOR_STATE::Triangulate` reads off `delaunator::Delaunator`.

/// `delaunator::INVALID_INDEX` (KiCad tests
/// `delaunator.halfedges[i] == delaunator::INVALID_INDEX`); the crate's
/// `EMPTY` is the same value.
pub const INVALID_INDEX: usize = usize::MAX;

#[cfg(not(feature = "rust-port"))]
const _: () = assert!(delaunator::EMPTY == INVALID_INDEX);

pub struct Triangulation {
    /// Point indices, 3 per triangle.
    pub triangles: Vec<usize>,
    /// Same length as `triangles`; `halfedges[e]` is the opposite half-edge
    /// of edge `e`, or `INVALID_INDEX` if `e` is on the convex hull.
    pub halfedges: Vec<usize>,
    /// The convex hull, point indices.
    pub hull: Vec<usize>,
}

impl Triangulation {
    /// `coords` is `[x0, y0, x1, y1, ...]`. Points that coincide with an
    /// earlier one are skipped, and collinear points make no triangles, as
    /// in every Delaunator.
    #[cfg(not(feature = "rust-port"))]
    pub fn new(coords: &[f64]) -> Self {
        let pts: Vec<delaunator::Point> = coords.chunks_exact(2).map(|c| delaunator::Point { x: c[0], y: c[1] }).collect();
        let t = delaunator::triangulate(&pts);
        Triangulation { triangles: t.triangles, halfedges: t.halfedges, hull: t.hull }
    }

    /// The INACTIVE port (`rust-port` feature).
    #[cfg(feature = "rust-port")]
    pub fn new(coords: &[f64]) -> Self {
        let t = crate::delaunay_port::Triangulation::new(coords);
        Triangulation { triangles: t.triangles, halfedges: t.halfedges, hull: t.hull }
    }
}
