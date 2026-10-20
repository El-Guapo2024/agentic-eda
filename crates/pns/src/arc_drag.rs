//! `DM_ARC`: dragging an arc track. `DRAGGER::startDragArc` takes the line the arc is part of and sets the mode, and KiCad's PNS then does nothing
//! with it (`dragMarkObstacles`, `dragWalkaround` and `dragShove` have no case for `DM_ARC`); `ROUTER_TOOL::performDragging` refuses an arc
//! ("We don't support dragging arcs inside the PNS right now") and hands the drag to `EDIT_TOOL::DragArcTrack`, which changes the arc's curvature
//! without any check against the rest of the board. This is that function (`pcbnew/tools/edit_tool.cpp:1040`) as a drag of the router, so that
//! the same three modes judge the result.
//!
//! The arc is the fillet between two straight tracks: the cursor is a point the new arc has to go through, the arc stays tangent to the two
//! tracks next to it, and its ends slide along them. [`ArcDrag::new`] is what `DragArcTrack` works out before its loop (the tangent lines of the
//! arc's ends, the tracks on them -- or, where there is none, a new one that starts at the arc's end -- the largest circle that fits between the two
//! tracks, and the triangle the cursor is held in), [`ArcDrag::at`] is one turn of the loop.

use crate::arcs::{calc_arc_mid, ArcPts};
use crate::line::{ArcRun, Line};
use eda_model::ir::{Point, Um};

/// `ADVANCED_CFG::m_MaxTangentAngleDeviation`: how far (degrees) a track may be off the arc's tangent and still be the one it is a fillet of.
pub const MAX_TANGENT_ANGLE_DEVIATION: f64 = 1.0;
/// `ADVANCED_CFG::m_MaxTrackLengthToKeep` (0.0005 mm): a track the drag leaves shorter than this is removed.
pub const MAX_TRACK_LENGTH_TO_KEEP_UM: f64 = 0.5;

type V = (f64, f64);

fn v(p: Point) -> V {
    (p.x as f64, p.y as f64)
}

fn sub(a: V, b: V) -> V {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: V, b: V) -> V {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: V, f: f64) -> V {
    (a.0 * f, a.1 * f)
}

fn dot(a: V, b: V) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: V, b: V) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn norm(a: V) -> f64 {
    a.0.hypot(a.1)
}

fn round(a: V) -> Point {
    Point { x: a.0.round() as Um, y: a.1.round() as Um }
}

/// A segment of two real points (`SEG`), used as the line through them.
#[derive(Clone, Copy, Debug)]
struct Sg {
    a: V,
    b: V,
}

impl Sg {
    /// `SEG::IntersectLines`: where the lines through the two cross.
    fn intersect_lines(&self, o: &Sg) -> Option<V> {
        let (d1, d2) = (sub(self.b, self.a), sub(o.b, o.a));
        let den = cross(d1, d2);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = cross(sub(o.a, self.a), d2) / den;
        Some(add(self.a, scale(d1, t)))
    }

    /// `SEG::LineProject`: the foot of the perpendicular from `p` to the line.
    fn line_project(&self, p: V) -> V {
        let d = sub(self.b, self.a);
        let l2 = dot(d, d);
        if l2 == 0.0 {
            return self.a;
        }
        add(self.a, scale(d, dot(sub(p, self.a), d) / l2))
    }

    /// `SEG::NearestPoint`: the nearest point of the segment itself.
    fn nearest_point(&self, p: V) -> V {
        let d = sub(self.b, self.a);
        let l2 = dot(d, d);
        if l2 == 0.0 {
            return self.a;
        }
        add(self.a, scale(d, (dot(sub(p, self.a), d) / l2).clamp(0.0, 1.0)))
    }

    /// `SEG::Side`: -1, 0 or 1 by which side of the line through the segment the point is on.
    fn side(&self, p: V) -> i32 {
        let c = cross(sub(self.b, self.a), sub(p, self.a));
        if c < 0.0 {
            -1
        } else if c > 0.0 {
            1
        } else {
            0
        }
    }
}

/// A circle (`CIRCLE`): centre and radius.
#[derive(Clone, Copy, Debug)]
struct Circle {
    c: V,
    r: f64,
}

impl Circle {
    /// `CIRCLE::NearestPoint`.
    fn nearest_point(&self, p: V) -> V {
        let mut d = sub(p, self.c);
        if d == (0.0, 0.0) {
            d = (1.0, 0.0);
        }
        add(scale(d, self.r / norm(d)), self.c)
    }
}

/// `CIRCLE::ConstructFromTanTanPt( aLineA, aLineB, aP )`: the circle tangent to both lines that goes through `p` -- of the two there are, the one
/// whose side nearest the point where the lines cross passes through `p` (the fillet; KiCad finds it by homothety, this is the same circle computed directly). The lines are the
/// segments' `A -> B`, the wedge the one they open towards their far ends from the crossing.
fn tan_tan_pt(a: &Sg, b: &Sg, p: V) -> Option<Circle> {
    let i = a.intersect_lines(b)?;
    let far = |s: &Sg| if norm(sub(s.a, i)) > norm(sub(s.b, i)) { s.a } else { s.b };
    let (ra, rb) = (sub(far(a), i), sub(far(b), i));
    let (la, lb) = (norm(ra), norm(rb));
    if la == 0.0 || lb == 0.0 {
        return None;
    }
    let (ua, ub) = (scale(ra, 1.0 / la), scale(rb, 1.0 / lb));
    let bis = add(ua, ub);
    let lbis = norm(bis);
    if lbis < 1e-9 {
        return None;
    }
    let u = scale(bis, 1.0 / lbis);
    // the half angle of the wedge: cos = |ua + ub| / 2
    let cos_h = lbis / 2.0;
    let sin_h = (1.0 - cos_h * cos_h).max(0.0).sqrt();
    let d = sub(p, i);
    if norm(d) == 0.0 {
        return Some(Circle { c: p, r: 0.0 });
    }
    // |p - (i + t u)| = t sin_h  =>  t^2 cos_h^2 - 2 t (u . d) + |d|^2 = 0; the root with `p` on the side of the circle that faces the crossing is the larger one
    let ud = dot(u, d);
    let disc = ud * ud - dot(d, d) * cos_h * cos_h;
    // a point on one of the lines touches the circle there: a double root, which rounding can leave a hair below zero
    if disc < -1e-9 * dot(d, d) || cos_h == 0.0 {
        return None;
    }
    let t = (ud + disc.max(0.0).sqrt()) / (cos_h * cos_h);
    let c = add(i, scale(u, t));
    Some(Circle { c, r: t.abs() * sin_h })
}

/// What `DragArcTrack` works out before its loop, for the arc of a line.
#[derive(Debug, Clone)]
pub struct ArcDrag {
    /// The arc as the line goes: its start first.
    pub arc: ArcPts,
    /// The points of the line that are the arc's polyline.
    pub first: usize,
    pub last: usize,
    /// The straight segment of the line before the arc, if it is tangent to it (`trackOnStart`): the point of the line it starts at.
    start_track_far: Option<Point>,
    /// ... and after it (`trackOnEnd`).
    end_track_far: Option<Point>,
    c_seg_tan_start: Sg,
    c_seg_tan_end: Sg,
    c_seg_chord: Sg,
    sides: [i32; 3],
    max_tan_circle: Circle,
}

/// Why an arc cannot be dragged (`EDIT_TOOL::DragArcTrack`'s own refusal).
pub const TOO_WIDE: &str = "Unable to resize arc tracks of 179° or greater.";

impl ArcDrag {
    /// `startDragArc` + the set-up of `DragArcTrack` for the arc `run` of `line`. `Err` is KiCad's message; `Ok(None)` when no fillet can be made of it (its
    /// tangents do not cross, or the cursor has no triangle to move in).
    pub fn new(line: &Line, run: ArcRun) -> Result<Option<ArcDrag>, String> {
        let arc = if run.reversed { line.arcs[run.arc].reversed() } else { line.arcs[run.arc] };
        if arc.is_degenerate() {
            return Ok(None);
        }
        // "Unable to resize arc tracks of 179 or greater": the angle plus the deviation reaches 180
        if arc.central_angle().abs() + MAX_TANGENT_ANGLE_DEVIATION >= 180.0 {
            return Err(TOO_WIDE.to_string());
        }
        let center = arc.center_f();
        let (s, e) = (v(arc.start), v(arc.end));
        let tan_start = Sg { a: s, b: add(s, (-(s.1 - center.1), s.0 - center.0)) };
        let tan_end = Sg { a: e, b: add(e, (-(e.1 - center.1), e.0 - center.0)) };
        let Some(mut intersect) = tan_start.intersect_lines(&tan_end) else { return Ok(None) };
        let (mut tan_start, mut tan_end) = (Sg { a: intersect, b: s }, Sg { a: intersect, b: e });

        // `getUniqueTrackAtAnchorCollinear`: the straight track of the line next to the arc, if it is on the arc's tangent
        let tangent_ok = |far: Point, anchor: Point, tangent: &Sg| -> bool {
            let t = sub(tangent.b, tangent.a);
            let d = sub(v(anchor), v(far));
            let (lt, ld) = (norm(t), norm(d));
            if lt == 0.0 || ld == 0.0 {
                return false;
            }
            // the angle between the two lines, whichever way each is walked
            let sin = (cross(t, d) / (lt * ld)).abs();
            sin <= MAX_TANGENT_ANGLE_DEVIATION.to_radians().sin()
        };
        let before = (run.first >= 1 && !line.is_arc_segment(run.first - 1)).then(|| line.pts[run.first - 1]).filter(|&far| tangent_ok(far, arc.start, &tan_start));
        let after = (run.last + 1 < line.pts.len() && !line.is_arc_segment(run.last)).then(|| line.pts[run.last + 1]).filter(|&far| tangent_ok(far, arc.end, &tan_end));
        if let Some(far) = before {
            tan_start = Sg { a: v(far), b: s };
        }
        if let Some(far) = after {
            tan_end = Sg { a: v(far), b: e };
        }
        // "Recalculate intersection point"
        let Some(i2) = tan_start.intersect_lines(&tan_end) else { return Ok(None) };
        intersect = i2;

        // the constraints: `maxTanCircle` is the largest circle tangent to the two tracks whose tangent points are on them
        let furthest = |p: V, q: V| if norm(sub(p, intersect)) > norm(sub(q, intersect)) { p } else { q };
        let tan_start_point = furthest(tan_start.a, tan_start.b);
        let tan_end_point = furthest(tan_end.a, tan_end.b);
        let temp_tangent_point = if furthest(tan_start_point, tan_end_point) == tan_end_point { tan_start_point } else { tan_end_point };
        let Some(max_tan_circle) = tan_tan_pt(&tan_start, &tan_end, temp_tangent_point) else { return Ok(None) };
        let max_tan_pt_start = tan_start.line_project(max_tan_circle.c);
        let max_tan_pt_end = tan_end.line_project(max_tan_circle.c);
        let c_seg_tan_start = Sg { a: max_tan_pt_start, b: intersect };
        let c_seg_tan_end = Sg { a: max_tan_pt_end, b: intersect };
        let c_seg_chord = Sg { a: max_tan_pt_start, b: max_tan_pt_end };
        let mid = v(arc.mid);
        let sides = [c_seg_tan_start.side(mid), c_seg_tan_end.side(mid), c_seg_chord.side(mid)];
        Ok(Some(ArcDrag { arc, first: run.first, last: run.last, start_track_far: before, end_track_far: after, c_seg_tan_start, c_seg_tan_end, c_seg_chord, sides, max_tan_circle }))
    }

    /// The cursor as `DragArcTrack` holds it: "Constrain cursor within the isosceles triangle", then "Constrain cursor to be outside maxTanCircle".
    fn constrained(&self, cursor: Point) -> V {
        let mut c = v(cursor);
        if self.sides[0] != self.c_seg_tan_start.side(c) || self.sides[1] != self.c_seg_tan_end.side(c) || self.sides[2] != self.c_seg_chord.side(c) {
            let mut closest = self.c_seg_tan_start.nearest_point(c);
            for candidate in [self.c_seg_tan_end.nearest_point(c), self.c_seg_chord.nearest_point(c)] {
                if dot(sub(candidate, c), sub(candidate, c)) < dot(sub(closest, c), sub(closest, c)) {
                    closest = candidate;
                }
            }
            c = closest;
        }
        if norm(sub(c, self.max_tan_circle.c)) < self.max_tan_circle.r {
            c = self.max_tan_circle.nearest_point(c);
        }
        c
    }

    /// One turn of `DragArcTrack`'s loop: the arc for a cursor at `cursor`, and the line with it -- the tracks next to the arc moved to its new ends, a new
    /// track made where there was none to move, a track the drag leaves shorter than [`MAX_TRACK_LENGTH_TO_KEEP_UM`] removed (the arc then goes on to the far
    /// end of it: "Amend the end points of the arc if we delete the joining tracks").
    pub fn at(&self, line: &Line, cursor: Point) -> Option<(ArcPts, Line)> {
        let cursor = self.constrained(cursor);
        let circle = tan_tan_pt(&self.c_seg_tan_start, &self.c_seg_tan_end, cursor)?;
        let (ns, ne, nc) = (round(self.c_seg_tan_start.line_project(circle.c)), round(self.c_seg_tan_end.line_project(circle.c)), round(circle.c));
        if ns == ne {
            return None;
        }
        let mid = calc_arc_mid(ns, ne, nc);
        let short = |a: Point, b: Point| norm(sub(v(a), v(b))) <= MAX_TRACK_LENGTH_TO_KEEP_UM;

        // what comes before the arc's start, and where the arc starts
        let (mut pts, arc_start): (Vec<Point>, Point) = match self.start_track_far {
            // the track next to the arc ends at the new start -- or is gone, and the arc starts at its far end
            Some(far) if short(far, ns) => (line.pts[..self.first].to_vec(), far),
            Some(_) => (line.pts[..self.first].to_vec(), ns),
            // no track: one is made from where the arc started (the old start stays), unless it would be as short as that
            None if short(self.arc.start, ns) => (line.pts[..=self.first].to_vec(), self.arc.start),
            None => (line.pts[..=self.first].to_vec(), ns),
        };
        // and after the end
        let (suffix, arc_end): (&[Point], Point) = match self.end_track_far {
            Some(far) if short(far, ne) => (&line.pts[self.last + 2..], far),
            Some(_) => (&line.pts[self.last + 1..], ne),
            None if short(self.arc.end, ne) => (&line.pts[self.last + 1..], self.arc.end),
            None => (&line.pts[self.last..], ne),
        };
        let final_arc = ArcPts { start: arc_start, mid, end: arc_end };
        if pts.last() != Some(&arc_start) {
            pts.push(arc_start);
        }
        pts.extend(final_arc.polyline().into_iter().skip(1));
        for &p in suffix {
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
        let mut out = line.clone();
        out.pts = pts;
        // the line's other arcs stay; this one is the new one
        match line.arcs.iter().position(|a| *a == self.arc || a.reversed() == self.arc) {
            Some(k) => out.arcs[k] = final_arc,
            None => out.arcs.push(final_arc),
        }
        Some((final_arc, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    /// The corner of two tracks at (10000, 0): one comes along y = 0 from the left, one goes down x = 10000; a quarter circle of radius 4000 rounds it:
    /// the arc from (6000, 0) to (10000, 4000) about (6000, 4000).
    fn fillet() -> Line {
        let arc = ArcPts::from_start_end_center(p(6000, 0), p(10000, 4000), p(6000, 4000));
        let mut l = Line::from_points(None, 0, 200, vec![p(0, 0), p(6000, 0)]);
        l.append_arc(arc);
        l.pts.push(p(10000, 12000));
        l
    }

    #[test]
    fn a_fillet_between_two_tracks_is_found_with_both_of_them() {
        let l = fillet();
        let run = l.arc_runs()[0];
        let d = ArcDrag::new(&l, run).unwrap().expect("a fillet");
        assert_eq!(d.start_track_far, Some(p(0, 0)));
        assert_eq!(d.end_track_far, Some(p(10000, 12000)));
        // the largest circle that fits: the shorter track is 6 mm long from the corner (4 mm of it is the arc's own radius... the end track is 12 mm, the start one 10 mm)
        assert!(d.max_tan_circle.r > 4000.0 && d.max_tan_circle.r <= 10000.0, "{:?}", d.max_tan_circle);
    }

    #[test]
    fn dragging_the_arc_towards_the_corner_makes_it_tighter_and_away_from_it_larger_and_the_tracks_follow() {
        let l = fillet();
        let run = l.arc_runs()[0];
        let d = ArcDrag::new(&l, run).unwrap().unwrap();
        // the corner is (10000, 0); the arc's middle is about (9657, 343)'s side: toward the corner = nearer (10000, 0)
        let (tight, tight_line) = d.at(&l, p(9800, 200)).expect("a tighter arc");
        assert!(tight.radius() < 4000.0, "{}", tight.radius());
        assert!(tight_line.pts.first() == Some(&p(0, 0)) && tight_line.pts.last() == Some(&p(10000, 12000)), "the far ends of the tracks stay");
        assert_eq!(tight_line.arc_count(), 1);
        // tangent: its ends are on the tracks' lines
        assert_eq!(tight.start.y, 0);
        assert_eq!(tight.end.x, 10000);
        let (wide, wide_line) = d.at(&l, p(8000, 2000)).expect("a larger arc");
        assert!(wide.radius() > 4000.0, "{}", wide.radius());
        assert_eq!((wide.start.y, wide.end.x), (0, 10000));
        assert!(wide_line.arc_count() == 1);
        // the cursor can not take it past the biggest circle that fits
        let (far, _) = d.at(&l, p(3000, 9000)).unwrap();
        assert!(far.radius() <= d.max_tan_circle.r + 2.0, "{} vs {}", far.radius(), d.max_tan_circle.r);
    }

    #[test]
    fn an_arc_of_179_degrees_or_more_is_refused_with_kicads_words() {
        let arc = ArcPts::new(p(0, 0), p(4000, -4000), p(8000, 0)); // a half circle
        let mut l = Line::from_points(None, 0, 200, vec![p(-2000, 0)]);
        l.append_arc(arc);
        let run = l.arc_runs()[0];
        assert_eq!(ArcDrag::new(&l, run).unwrap_err(), TOO_WIDE);
    }
}
