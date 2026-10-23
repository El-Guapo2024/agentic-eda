//! Placement rule areas and the geometry of the multichannel tool (GAPS.md items 24 and 32, package P24).
//!
//! A rule area (`ZONE::GetIsRuleArea`) can carry a *placement source* (`ZONE::m_placementAreaEnabled`, `m_placementAreaSourceType`, `m_placementAreaSource`): the
//! sheet, the component class or the group whose footprints the area stands for. That is all a channel is to KiCad's multichannel tool
//! (`pcbnew/tools/multichannel_tool.cpp`): a rule area with a source names the footprints of one channel (`findComponentsInRuleArea`), the area's outline holds the
//! routing and the other items that go with them, and Repeat Layout copies the first onto the others.
//!
//! This file has the IR (the zone's placement settings, written as `(placement (enabled yes) (sheetname ".."))`) and the geometry the tool makes areas with:
//! `BuildConvexHull`, `KIGEOM::RectifyPolygon`, `SHAPE_LINE_CHAIN::Simplify` and `SHAPE_LINE_CHAIN::Centre`. The matching of footprints is [`crate::topo_match`]; what a
//! board does with both is `eda_ops`.

use crate::ir::{Point, Um};
use serde::{Deserialize, Serialize};

/// `PLACEMENT_SOURCE_T` (`pcbnew/zone_settings.h`) as a rule area saves it. KiCad has a fourth kind, `DESIGN_BLOCK`, that Apply Design Block Layout gives its two temporary areas
/// and that is "transitory and should not be saved": it is not here, the tool's own areas ([`eda_ops`'s `RuleAreaSource`]) have it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementSourceType {
    /// `SHEETNAME`: the footprints on a sheet or on a sheet below it (`memberOfSheetOrChildren`). KiCad's default.
    #[default]
    Sheetname,
    /// `COMPONENT_CLASS`: the footprints that have the class (`hasComponentClass`).
    ComponentClass,
    /// `GROUP_PLACEMENT`: the footprints in a group of that name, or in a group below it (`memberOfGroup`).
    Group,
}

impl PlacementSourceType {
    /// The token `(placement ..)` writes for it: `sheetname`, `component_class` or `group`.
    pub fn token(self) -> &'static str {
        match self {
            PlacementSourceType::Sheetname => "sheetname",
            PlacementSourceType::ComponentClass => "component_class",
            PlacementSourceType::Group => "group",
        }
    }

    /// Inverse of [`PlacementSourceType::token`].
    pub fn from_token(token: &str) -> Option<PlacementSourceType> {
        Some(match token {
            "sheetname" => PlacementSourceType::Sheetname,
            "component_class" => PlacementSourceType::ComponentClass,
            "group" => PlacementSourceType::Group,
            _ => return None,
        })
    }

    /// The choice the Properties panel's "Source Type" row shows (`ENUM_MAP<PLACEMENT_SOURCE_T>`).
    pub fn label(self) -> &'static str {
        match self {
            PlacementSourceType::Sheetname => "Sheet Name",
            PlacementSourceType::ComponentClass => "Component Class",
            PlacementSourceType::Group => "Group",
        }
    }
}

/// A rule area's placement settings: `ZONE::GetPlacementAreaEnabled`, `GetPlacementAreaSourceType` and `GetPlacementAreaSource`. The default -- no placement, a sheet name
/// of "" -- is what every rule area has that was never given one, and what KiCad writes for it as `(placement (enabled no) (sheetname ""))`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementArea {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub source_type: PlacementSourceType,
    /// The sheet path (`/i2c_thingy_ch0/`), the component class or the group name, by `source_type`.
    #[serde(default)]
    pub source: String,
}

impl PlacementArea {
    /// Nothing set: the area stands for no channel.
    pub fn is_default(&self) -> bool {
        *self == PlacementArea::default()
    }
}

/// `x0, y0, x1, y1`.
pub type Box4 = (Um, Um, Um, Um);

fn cross(o: Point, a: Point, b: Point) -> i128 {
    (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
}

/// `KIGEOM::CollectBoxCorners`: left-top, right-top, right-bottom, left-bottom.
pub fn collect_box_corners(b: Box4, out: &mut Vec<Point>) {
    out.push(Point { x: b.0, y: b.1 });
    out.push(Point { x: b.2, y: b.1 });
    out.push(Point { x: b.2, y: b.3 });
    out.push(Point { x: b.0, y: b.3 });
}

/// `BuildConvexHull( aResult, aPoly )`: Andrew's monotone chain. The points of the hull, starting at the lowest x (then y), in the order the screen shows clockwise.
/// Fewer than two points give nothing.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut poly = points.to_vec();
    poly.sort_by(|a, b| (a.x, a.y).cmp(&(b.x, b.y)));
    let n = poly.len();
    let mut hull: Vec<Point> = Vec::with_capacity(2 * n);
    for p in poly.iter() {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], *p) <= 0 {
            hull.pop();
        }
        hull.push(*p);
    }
    let t = hull.len() + 1;
    for p in poly.iter().rev().skip(1) {
        while hull.len() >= t && cross(hull[hull.len() - 2], hull[hull.len() - 1], *p) <= 0 {
            hull.pop();
        }
        hull.push(*p);
    }
    if hull.len() > 1 && hull[0] == hull[hull.len() - 1] {
        hull.pop();
    }
    hull
}

/// `SHAPE_LINE_CHAIN::TestSegmentHit` with a distance of 0 (`libs/kimath/src/trigo.cpp`): the point is on the segment.
fn point_on_segment(p: Point, a: Point, b: Point) -> bool {
    let (xmin, xmax) = (a.x.min(b.x), a.x.max(b.x));
    let (ymin, ymax) = (a.y.min(b.y), a.y.max(b.y));
    if p.y < ymin || p.y > ymax || p.x < xmin || p.x > xmax {
        return false;
    }
    if a.x == b.x {
        return p.x == a.x;
    }
    if a.y == b.y {
        return p.y == a.y;
    }
    // `SEG::SquaredDistance( p ) < 1` for an integer point: the point is on the line to within one unit of squared distance.
    let c = cross(a, b, p);
    let len2 = (b.x - a.x) as i128 * (b.x - a.x) as i128 + (b.y - a.y) as i128 * (b.y - a.y) as i128;
    c * c < len2
}

/// `SHAPE_LINE_CHAIN::Simplify( 0 )` of a closed chain: a vertex that lies on the segment between its neighbours goes, as do the repeats.
pub fn simplify_closed(pts: &[Point]) -> Vec<Point> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out: Vec<Point> = Vec::with_capacity(n);
    let mut start = 0usize;
    while start < n {
        out.push(pts[start]);
        let mut end = (start + 2) % n;
        let mut can = true;
        while can && end != start {
            let mut test = (start + 1) % n;
            while test != end {
                if !point_on_segment(pts[test], pts[start], pts[end]) {
                    can = false;
                    break;
                }
                test = (test + 1) % n;
            }
            if can {
                end = (end + 1) % n;
            }
        }
        if end == (start + 2) % n {
            start += 1;
        } else {
            let new_start = (end + n - 1) % n;
            if new_start <= start {
                break;
            }
            start = new_start;
        }
    }
    out
}

/// `KIGEOM::RectifyPolygon( hull )`: every edge of the hull becomes a vertical run and a horizontal one, through `(A.x, B.y)`. KiCad asks `aPoly.PointInside( p0 )` to choose the
/// corner outside the hull, but the chain it is given is an open one (`SHAPE_LINE_CHAIN( hullVertices )` is not closed), for which `PointInside` is always false: the corner is
/// always `(A.x, B.y)`, and the areas it makes (the L-shaped `auto-placement-area` polygons of KiCad's own boards) are what this makes. The last edge runs from the last
/// vertex back to the first, and the result is closed and simplified.
pub fn rectify_polygon(hull: &[Point]) -> Vec<Point> {
    let n = hull.len();
    if n < 2 {
        return hull.to_vec();
    }
    let mut out: Vec<Point> = Vec::with_capacity(2 * n);
    for i in 0..n {
        let (a, b) = (hull[i], hull[(i + 1) % n]);
        out.push(a);
        out.push(Point { x: a.x, y: b.y });
    }
    simplify_closed(&out)
}

/// `buildRAOutline`: the outline of a rule area for boxes (`x0, y0, x1, y1`), each grown by `margin`: the convex hull of their corners, rectified. Empty for no box.
pub fn rule_area_outline(boxes: &[Box4], margin: Um) -> Vec<Point> {
    let mut corners: Vec<Point> = Vec::with_capacity(boxes.len() * 4);
    for b in boxes {
        collect_box_corners((b.0 - margin, b.1 - margin, b.2 + margin, b.3 + margin), &mut corners);
    }
    rectify_polygon(&convex_hull(&corners))
}

/// The box of a polygon (`SHAPE::BBox`).
pub fn polygon_box(pts: &[Point]) -> Option<Box4> {
    let first = pts.first()?;
    Some(pts.iter().fold((first.x, first.y, first.x, first.y), |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y))))
}

/// `SHAPE_LINE_CHAIN::Centre()`: the centre of the box of the outline (`BOX2I::Centre`: the corner plus half the size, truncated). A rule area's `m_center`, and what
/// Repeat Layout moves a channel by when no anchor footprint is chosen.
pub fn outline_centre(pts: &[Point]) -> Option<Point> {
    let (x0, y0, x1, y1) = polygon_box(pts)?;
    Some(Point { x: x0 + (x1 - x0) / 2, y: y0 + (y1 - y0) / 2 })
}

/// `SHAPE_LINE_CHAIN::PointInside( p, 0 )` for a closed chain: the even-odd test along +x, with the division rounded to the nearest as `rescale` does.
pub fn point_inside(chain: &[Point], pt: Point) -> bool {
    let n = chain.len();
    if n < 3 {
        return false;
    }
    let rescale = |a: i64, b: i64, c: i64| -> i64 {
        let num = a as i128 * b as i128;
        let c = c as i128;
        (if (num < 0) ^ (c < 0) { (num - c / 2) / c } else { (num + c / 2) / c }) as i64
    };
    let mut inside = false;
    for i in 0..n {
        let (p1, p2) = (chain[i], chain[(i + 1) % n]);
        let (dx, dy) = (p2.x - p1.x, p2.y - p1.y);
        if dy == 0 {
            continue;
        }
        let d = rescale(dx, pt.y - p1.y, dy);
        if ((p1.y >= pt.y) != (p2.y >= pt.y)) && (pt.x - p1.x < d) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    #[test]
    fn the_hull_of_a_box_is_its_four_corners_clockwise_on_the_screen() {
        let mut c = Vec::new();
        collect_box_corners((0, 0, 10, 5), &mut c);
        assert_eq!(convex_hull(&c), vec![pt(0, 0), pt(10, 0), pt(10, 5), pt(0, 5)]);
        assert!(convex_hull(&[pt(1, 1)]).is_empty());
    }

    #[test]
    fn a_box_rectifies_to_itself() {
        let o = rule_area_outline(&[(0, 0, 10_000, 5_000)], 100);
        assert_eq!(o, vec![pt(-100, -100), pt(10_100, -100), pt(10_100, 5_100), pt(-100, 5_100)]);
    }

    /// The polygon KiCad made for `i2c_thingy_ch0` of `qa/data/pcbnew/issue21739/topology_mismatch.kicad_pcb`, from the boxes of its resistor and its amplifier
    /// (the courtyards of `R_0603_1608Metric` turned a quarter and of `QSOP-24`, with the stroke): the staircase of the open-chain rectify, the notch at the top
    /// between the two parts and all.
    #[test]
    fn two_boxes_make_the_l_shaped_area_kicad_makes() {
        let resistor = (112_245, 87_570, 113_755, 90_580);
        let amplifier = (114_775, 88_175, 122_225, 97_425);
        let outline = rule_area_outline(&[resistor, amplifier], 100);
        assert_eq!(
            outline,
            vec![pt(112_145, 87_470), pt(113_855, 87_470), pt(113_855, 88_075), pt(122_325, 88_075), pt(122_325, 97_525), pt(114_675, 97_525), pt(114_675, 90_680), pt(112_145, 90_680)]
        );
    }

    #[test]
    fn the_centre_is_the_middle_of_the_box() {
        assert_eq!(outline_centre(&[pt(0, 0), pt(10, 0), pt(10, 7), pt(0, 7)]), Some(pt(5, 3)));
        assert_eq!(outline_centre(&[]), None);
    }

    #[test]
    fn simplify_drops_repeats_and_points_on_a_straight_run() {
        let pts = [pt(0, 0), pt(0, 0), pt(5, 0), pt(10, 0), pt(10, 10), pt(10, 10), pt(0, 10)];
        assert_eq!(simplify_closed(&pts), vec![pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10)]);
    }

    #[test]
    fn the_source_types_round_trip_through_their_tokens() {
        for t in [PlacementSourceType::Sheetname, PlacementSourceType::ComponentClass, PlacementSourceType::Group] {
            assert_eq!(PlacementSourceType::from_token(t.token()), Some(t));
        }
        assert_eq!(PlacementSourceType::from_token("nonsense"), None);
        assert!(PlacementArea::default().is_default());
        assert!(!PlacementArea { enabled: true, ..Default::default() }.is_default());
    }

    #[test]
    fn point_inside_is_the_even_odd_test() {
        let sq = [pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10)];
        assert!(point_inside(&sq, pt(5, 5)));
        assert!(!point_inside(&sq, pt(15, 5)));
    }
}
