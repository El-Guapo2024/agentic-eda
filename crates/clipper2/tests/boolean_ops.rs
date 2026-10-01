//! Analytic-area, hole-handling and degenerate-input tests for `Clipper64`
//! boolean ops, per the task's stage-1 test requirements.

use eda_clipper2::{area, area_paths, ClipType, Clipper64, FillRule, Path64, Point64, PolyTree64};

fn square(x0: i64, y0: i64, side: i64) -> Path64 {
    vec![Point64::new(x0, y0), Point64::new(x0 + side, y0), Point64::new(x0 + side, y0 + side), Point64::new(x0, y0 + side)]
}

fn run(subjects: &[Path64], clips: &[Path64], ct: ClipType, fr: FillRule) -> (Vec<Path64>, Vec<Path64>) {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    c.add_clip(clips);
    let mut closed = Vec::new();
    let mut open = Vec::new();
    c.execute(ct, fr, &mut closed, &mut open);
    (closed, open)
}

#[test]
fn union_of_disjoint_squares_sums_areas() {
    let a = square(0, 0, 10);
    let b = square(20, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Union, FillRule::NonZero);
    assert_eq!(closed.len(), 2);
    assert_eq!(area_paths(&closed), 200.0);
}

#[test]
fn union_of_overlapping_squares() {
    // two 10x10 squares overlapping in a 5x10 strip -> union area = 100+100-50 = 150
    let a = square(0, 0, 10);
    let b = square(5, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Union, FillRule::NonZero);
    assert_eq!(closed.len(), 1);
    assert_eq!(area_paths(&closed), 150.0);
}

#[test]
fn intersection_of_overlapping_squares() {
    let a = square(0, 0, 10);
    let b = square(5, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Intersection, FillRule::NonZero);
    assert_eq!(closed.len(), 1);
    assert_eq!(area_paths(&closed), 50.0);
}

#[test]
fn intersection_of_disjoint_squares_is_empty() {
    let a = square(0, 0, 10);
    let b = square(20, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Intersection, FillRule::NonZero);
    assert!(closed.is_empty());
}

#[test]
fn difference_of_overlapping_squares() {
    let a = square(0, 0, 10);
    let b = square(5, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Difference, FillRule::NonZero);
    assert_eq!(closed.len(), 1);
    assert_eq!(area_paths(&closed), 50.0); // left 5x10 strip of `a` not covered by `b`
}

#[test]
fn xor_of_overlapping_squares() {
    let a = square(0, 0, 10);
    let b = square(5, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Xor, FillRule::NonZero);
    // symmetric difference: total area 200 minus 2x the 50-area overlap
    assert_eq!(area_paths(&closed), 100.0);
}

#[test]
fn hole_outline_minus_inner_area_evenodd() {
    // outer 20x20 square with an inner, reverse-wound 10x10 hole, unioned
    // with nothing: EvenOdd fill should report net area outer-hole.
    let outer = square(0, 0, 20);
    let mut hole = square(5, 5, 10);
    hole.reverse(); // CW -> negative area, matching a KiCad-style hole
    assert!(area(&hole) < 0.0);

    let (closed, _) = run(&[outer, hole], &[], ClipType::Union, FillRule::EvenOdd);
    // one outer ring + one hole ring in the result paths
    assert_eq!(closed.len(), 2);
    let net: f64 = closed.iter().map(|p| area(p)).sum();
    assert_eq!(net.abs(), 300.0); // 400 - 100
}

#[test]
fn hole_outline_minus_inner_area_nonzero() {
    let outer = square(0, 0, 20);
    let mut hole = square(5, 5, 10);
    hole.reverse();
    let (closed, _) = run(&[outer, hole], &[], ClipType::Union, FillRule::NonZero);
    let net: f64 = closed.iter().map(|p| area(p)).sum();
    assert_eq!(net.abs(), 300.0);
}

#[test]
fn polytree_reports_hole_nesting() {
    let outer = square(0, 0, 20);
    let mut hole = square(5, 5, 10);
    hole.reverse();
    let island = square(8, 8, 2); // a positively-wound island inside the hole

    let mut c = Clipper64::new();
    c.add_subject(&[outer, hole, island]);
    let mut tree = PolyTree64::new();
    let mut open = Vec::new();
    c.execute_tree(ClipType::Union, FillRule::NonZero, &mut tree, &mut open);

    assert_eq!(tree.count(), 1); // one top-level outline
    let root = tree.roots[0];
    assert!(!tree.nodes[root].is_hole());
    assert_eq!(tree.nodes[root].children.len(), 1); // one hole
    let hole_node = tree.nodes[root].children[0];
    assert!(tree.nodes[hole_node].is_hole());
    assert_eq!(tree.nodes[hole_node].children.len(), 1); // one island inside the hole
    let island_node = tree.nodes[hole_node].children[0];
    assert!(!tree.nodes[island_node].is_hole());
}

#[test]
fn degenerate_empty_paths_produce_empty_output() {
    let (closed, open) = run(&[], &[], ClipType::Union, FillRule::NonZero);
    assert!(closed.is_empty());
    assert!(open.is_empty());
}

#[test]
fn degenerate_single_point_and_line_are_dropped() {
    let point = vec![Point64::new(0, 0)];
    let line = vec![Point64::new(0, 0), Point64::new(10, 0)];
    let (closed, _) = run(&[point, line], &[], ClipType::Union, FillRule::NonZero);
    assert!(closed.is_empty());
}

#[test]
fn degenerate_collinear_points_still_clip_correctly() {
    // a square with a redundant collinear midpoint on one edge
    let a = vec![Point64::new(0, 0), Point64::new(5, 0), Point64::new(10, 0), Point64::new(10, 10), Point64::new(0, 10)];
    let b = square(5, 0, 10);
    let (closed, _) = run(&[a], &[b], ClipType::Intersection, FillRule::NonZero);
    assert_eq!(area_paths(&closed), 50.0);
}

#[test]
fn self_touching_bowtie_still_succeeds_without_panicking() {
    // a bowtie / figure-eight self-intersecting polygon
    let bowtie = vec![Point64::new(0, 0), Point64::new(10, 10), Point64::new(10, 0), Point64::new(0, 10)];
    let mut c = Clipper64::new();
    c.add_subject(&[bowtie]);
    let mut closed = Vec::new();
    let mut open = Vec::new();
    let ok = c.execute(ClipType::Union, FillRule::NonZero, &mut closed, &mut open);
    assert!(ok);
    // two triangles of area 50 each
    let total: f64 = closed.iter().map(|p| area(p).abs()).sum();
    assert_eq!(total, 50.0);
}

#[test]
fn evenodd_vs_nonzero_on_overlapping_same_direction_squares() {
    // two identical, same-orientation overlapping squares: NonZero union
    // should merge to the union footprint; EvenOdd leaves the overlap as a
    // "hole" (wind count 2 -> even -> not filled).
    let a = square(0, 0, 10);
    let b = square(0, 0, 10);
    let (closed_nz, _) = run(std::slice::from_ref(&a), std::slice::from_ref(&b), ClipType::Union, FillRule::NonZero);
    assert_eq!(area_paths(&closed_nz), 100.0);

    let mut c = Clipper64::new();
    c.add_subject(&[a]);
    c.add_subject(&[b]);
    let mut closed_eo = Vec::new();
    let mut open = Vec::new();
    c.execute(ClipType::Union, FillRule::EvenOdd, &mut closed_eo, &mut open);
    // fully overlapping identical squares under EvenOdd: net area is 0
    // (every point is covered an even number of times).
    let net: f64 = closed_eo.iter().map(|p| area(p)).sum();
    assert_eq!(net, 0.0);
}
