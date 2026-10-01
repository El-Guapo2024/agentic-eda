//! Basic `RectClip64` sanity tests.

use eda_clipper2::{area_paths, rect_clip, Path64, Point64, Rect64};

fn square(x0: i64, y0: i64, side: i64) -> Path64 {
    vec![Point64::new(x0, y0), Point64::new(x0 + side, y0), Point64::new(x0 + side, y0 + side), Point64::new(x0, y0 + side)]
}

#[test]
fn path_fully_inside_rect_is_returned_as_is() {
    let r = Rect64::new(-100, -100, 100, 100);
    let sq = square(-10, -10, 20);
    let result = rect_clip(r, &[sq]);
    assert_eq!(result.len(), 1);
    assert_eq!(area_paths(&result).abs(), 400.0);
}

#[test]
fn path_fully_outside_rect_is_dropped() {
    let r = Rect64::new(-10, -10, 10, 10);
    let sq = square(100, 100, 20);
    let result = rect_clip(r, &[sq]);
    assert!(result.is_empty());
}

#[test]
fn path_straddling_rect_is_clipped_to_overlap() {
    let r = Rect64::new(0, 0, 10, 10);
    let sq = square(5, 5, 10); // overlaps [5,10]x[5,10] with the rect
    let result = rect_clip(r, &[sq]);
    assert_eq!(area_paths(&result).abs(), 25.0);
}

#[test]
fn rect_fully_inside_path_returns_the_rect() {
    let r = Rect64::new(-5, -5, 5, 5);
    let big = square(-100, -100, 200);
    let result = rect_clip(r, &[big]);
    assert_eq!(area_paths(&result).abs(), 100.0);
}
