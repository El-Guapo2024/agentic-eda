//! Port of `DIRECTION_45` (`libs/kimath/include/geometry/direction45.h`,
//! `libs/kimath/src/geometry/direction_45.cpp`): the 8-octant compass
//! direction that drives every "45 degree posture" decision in the router --
//! which way a head segment points, which of the two L-shaped candidate paths
//! to build between two points, and how two adjacent segments compare
//! (straight/obtuse/right/acute/U-turn) for the optimizer's cost function.
//!
//! `CORNER_MODE` is KiCad's four (`ROUTING_SETTINGS::m_cornerMode`, the
//! Track Corner Mode actions): mitered and rounded, 45 and 90 degrees.
//! A rounded corner is an arc of the trace ([`Trace::arcs`],
//! [`crate::arcs::ArcPts`]), which a line keeps as the points of its
//! polyline ([`crate::line::Line::arcs`]); [`Direction45::build_initial_line`]
//! is `BuildInitialTrace`, computed on the nanometre grid as KiCad's is (the
//! IR's grid too).
use crate::arcs::ArcPts;
use crate::kigeom::{ki_round_nm, rotate_d, ShapeArc, V2};
use eda_model::ir::{Point, Nm};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction45 {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
    Undefined,
}

/// `DIRECTION_45::CORNER_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerMode {
    /// `MITERED_45`: H/V/45 with mitered corners (the default).
    #[default]
    Mitered45,
    /// `ROUNDED_45`: H/V/45 with filleted corners -- the diagonal leg is an arc.
    Rounded45,
    /// `MITERED_90`: H/V only, mitered corners.
    Mitered90,
    /// `ROUNDED_90`: H/V only, the corner a quarter arc.
    Rounded90,
}

impl CornerMode {
    /// `ROUTER::ToggleCornerMode` / `ACT_SwitchCornerModeToNext`: 45, arc 45, 90, arc 90, and round again.
    pub fn next(self) -> CornerMode {
        match self {
            CornerMode::Mitered45 => CornerMode::Rounded45,
            CornerMode::Rounded45 => CornerMode::Mitered90,
            CornerMode::Mitered90 => CornerMode::Rounded90,
            CornerMode::Rounded90 => CornerMode::Mitered45,
        }
    }

    /// `ROUNDED_90 || MITERED_90`.
    pub fn is_90(self) -> bool {
        matches!(self, CornerMode::Mitered90 | CornerMode::Rounded90)
    }

    /// The settings file's `corner_mode` (`PARAM_ENUM`, the enum's order) and the wire's name.
    pub fn name(self) -> &'static str {
        match self {
            CornerMode::Mitered45 => "mitered_45",
            CornerMode::Rounded45 => "rounded_45",
            CornerMode::Mitered90 => "mitered_90",
            CornerMode::Rounded90 => "rounded_90",
        }
    }

    pub fn from_name(name: &str) -> Option<CornerMode> {
        [CornerMode::Mitered45, CornerMode::Rounded45, CornerMode::Mitered90, CornerMode::Rounded90].into_iter().find(|m| m.name() == name)
    }
}

/// What `BuildInitialTrace` returns (a `SHAPE_LINE_CHAIN`): the points, with the polyline of each arc in them, and the arcs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trace {
    pub pts: Vec<Point>,
    pub arcs: Vec<ArcPts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleType {
    Straight,
    Obtuse,
    Right,
    Acute,
    HalfFull,
    Undefined,
}

const OCTANTS: [Direction45; 8] = [Direction45::N, Direction45::NE, Direction45::E, Direction45::SE, Direction45::S, Direction45::SW, Direction45::W, Direction45::NW];

impl Direction45 {
    /// `DIRECTION_45(const VECTOR2I&)`: quantize a vector to the nearest of
    /// the 8 compass octants (±22.5°); the zero vector is `Undefined`.
    /// Board +y is down, matching the rest of this codebase, so "N" here
    /// means "-y" (up on screen) -- the label is cosmetic, only the 8-way
    /// relative geometry (used by `right`/`angle_to`/`build_initial_trace`)
    /// matters to any caller.
    pub fn from_vector(dx: Nm, dy: Nm) -> Self {
        if dx == Nm::ZERO && dy == Nm::ZERO {
            return Direction45::Undefined;
        }
        let deg = ((dy).as_f64()).atan2(dx.as_f64()).to_degrees(); // -180..180, 0 = +x (E)
        let idx = (((deg + 360.0) % 360.0) / 45.0 + 0.5).floor() as i64 % 8;
        // idx: 0=E,1=SE,2=S,3=SW,4=W,5=NW,6=N,7=NE (since +y is down, +45deg from E is SE)
        match idx {
            0 => Direction45::E,
            1 => Direction45::SE,
            2 => Direction45::S,
            3 => Direction45::SW,
            4 => Direction45::W,
            5 => Direction45::NW,
            6 => Direction45::N,
            _ => Direction45::NE,
        }
    }

    pub fn from_seg(a: Point, b: Point) -> Self {
        Self::from_vector(b.x - a.x, b.y - a.y)
    }

    fn index(&self) -> Option<i32> {
        OCTANTS.iter().position(|d| d == self).map(|i| i as i32)
    }

    pub fn is_diagonal(&self) -> bool {
        matches!(self, Direction45::NE | Direction45::SE | Direction45::SW | Direction45::NW)
    }

    /// `DIRECTION_45::Right()`: rotate by +1 octant (45 degrees). KiCad's
    /// `FlipPosture` always constructs this with `a90=false` inside the
    /// line placer (confirmed by the LINE_PLACER research spec), so a
    /// 90-step variant is not needed here.
    pub fn right(&self) -> Self {
        match self.index() {
            None => *self,
            Some(i) => OCTANTS[((i + 1) % 8) as usize],
        }
    }

    /// `DIRECTION_45::Left()`: rotate by -1 octant (45 degrees the other way from [`Self::right`]).
    pub fn left(&self) -> Self {
        match self.index() {
            None => *self,
            Some(i) => OCTANTS[((i + 7) % 8) as usize],
        }
    }

    /// Unit step for this direction, `x`/`y` each in `{-1,0,1}`.
    pub fn unit(&self) -> (i64, i64) {
        match self {
            Direction45::N => (0, -1),
            Direction45::NE => (1, -1),
            Direction45::E => (1, 0),
            Direction45::SE => (1, 1),
            Direction45::S => (0, 1),
            Direction45::SW => (-1, 1),
            Direction45::W => (-1, 0),
            Direction45::NW => (-1, -1),
            Direction45::Undefined => (0, 0),
        }
    }

    /// `DIRECTION_45::Angle(other)`: classify the turn between two
    /// directions by octant-index delta (not a continuous angle -- see
    /// `crate::optimizer`'s doc comment on why this differs from exact
    /// collinearity tolerance).
    pub fn angle_to(&self, other: &Direction45) -> AngleType {
        let (Some(a), Some(b)) = (self.index(), other.index()) else { return AngleType::Undefined };
        let d = (a - b).rem_euclid(8);
        let d = d.min(8 - d);
        match d {
            0 => AngleType::Straight,
            1 => AngleType::Obtuse,
            2 => AngleType::Right,
            3 => AngleType::Acute,
            4 => AngleType::HalfFull,
            _ => AngleType::Undefined,
        }
    }

    /// `DIRECTION_45::BuildInitialTrace`'s points (see [`Self::build_initial_line`]); in a rounded mode, an arc is the points of its
    /// polyline.
    pub fn build_initial_trace(&self, p0: Point, p1: Point, start_diagonal: bool, mode: CornerMode) -> Vec<Point> {
        self.build_initial_line(p0, p1, start_diagonal, mode).pts
    }

    /// `DIRECTION_45::BuildInitialTrace( aP0, aP1, aStartDiagonal, aMode )` (`direction_45.cpp`): the trace from `p0` to `p1` with one corner.
    /// `start_diagonal` is only read when `self` is `Undefined`; a direction starts diagonal when it is a diagonal. 45 degrees: a straight leg
    /// and a diagonal one (`ROUNDED_45`: the diagonal one an arc of 45 degrees tangent to the straight one); 90 degrees: a horizontal and a vertical
    /// leg (`ROUNDED_90`: the corner a quarter arc of the shorter leg's length). The arithmetic is KiCad's, on the nanometre grid.
    pub fn build_initial_line(&self, p0: Point, p1: Point, start_diagonal: bool, mode: CornerMode) -> Trace {
        let start_diagonal = if *self == Direction45::Undefined { start_diagonal } else { self.is_diagonal() };
        let (a0, a1) = (p0, p1);
        let (w, h) = ((a1.x - a0.x).abs(), (a1.y - a0.y).abs());
        let (sw, sh) = ((a1.x - a0.x).signum(), (a1.y - a0.y).signum());
        let is90 = mode.is_90();
        let mut out = TraceBuilder::default();
        // "Shortcut where we can generate just one segment and quit."
        if w == Nm::ZERO || h == Nm::ZERO || (!is90 && h == w) {
            return Trace { pts: vec![p0, p1], arcs: Vec::new() };
        }
        let add = |a: Point, b: Point| Point { x: a.x + b.x, y: a.y + b.y };
        let sub = |a: Point, b: Point| Point { x: a.x - b.x, y: a.y - b.y };
        let norm = |v: Point| ki_round_nm(v.x.as_f64().hypot(v.y.as_f64()));
        let resize = |v: Point, len: Nm| {
            let (x, y) = crate::hull::resize(v.x, v.y, len);
            Point { x, y }
        };
        let (mp0, mp1, tangent_length) = if is90 {
            // direction E, else N
            (if start_diagonal == (h >= w) { Point { x: w * sw, y: Nm::ZERO } } else { Point { x: Nm::ZERO, y: sh * h } }, Point::default(), Nm::ZERO)
        } else if w > h {
            let mp1 = Point { x: h * sw, y: h * sh };
            (Point { x: (w - h) * sw, y: Nm::ZERO }, mp1, (w - h) - norm(mp1))
        } else {
            let mp1 = Point { x: sw * w, y: sh * w };
            (Point { x: Nm::ZERO, y: sh * (h - w) }, mp1, (h - w) - norm(mp1))
        };
        match mode {
            CornerMode::Mitered45 => {
                out.point(a0);
                out.point(if start_diagonal { add(a0, mp1) } else { add(a0, mp0) });
                out.point(a1);
            }
            CornerMode::Rounded45 => {
                let sq = |v: Point| ((v.x).as_f64()) * ((v.x).as_f64()) + ((v.y).as_f64()) * ((v.y).as_f64());
                let diag2 = if tangent_length >= Nm::ZERO { sq(mp1) } else { sq(mp0) };
                let diag_length = ((2.0 * diag2) - (2.0 * diag2 * (3.0 * std::f64::consts::FRAC_PI_4).cos())).sqrt();
                let arc_radius = ki_round_nm(diag_length / (2.0 * (67.5f64.to_radians()).cos()));
                let rotation_sign = if w > h { sw * sh * -1 } else { sw * sh };
                if start_diagonal {
                    if tangent_length >= Nm::ZERO {
                        // "Positive tangentLength, diagonal start: arc goes at the start"
                        let arc_endpoint = sub(a1, resize(mp0, tangent_length));
                        out.arc_or_point(ShapeArc::from_start_end_angle(a0, arc_endpoint, 45.0 * rotation_sign as f64, Nm::ZERO), a0);
                        out.point(a1);
                    } else {
                        // "Negative tangentLength, diagonal start: arc goes at the end"
                        let arc_endpoint = add(a0, resize(mp1, tangent_length.abs()));
                        out.point(a0);
                        out.arc_or_point(ShapeArc::from_start_end_angle(arc_endpoint, a1, 45.0 * rotation_sign as f64, Nm::ZERO), a1);
                    }
                } else {
                    let center_dir = rotate_d(V2::of(mp0), 90.0 * rotation_sign as f64);
                    if tangent_length >= Nm::ZERO {
                        // "Positive tangentLength: arc goes at the end"
                        let arc_endpoint = add(a0, resize(mp0, tangent_length));
                        out.point(a0);
                        out.arc_or_point(ShapeArc::from_start_end_angle(arc_endpoint, a1, -45.0 * rotation_sign as f64, Nm::ZERO), a1);
                    } else {
                        // "Negative tangentLength: arc goes at the start"
                        let c = center_dir.resize(arc_radius.as_f64());
                        let arc_center = Point { x: a0.x + ki_round_nm(c.x), y: a0.y + ki_round_nm(c.y) };
                        let mut ca = arc_from_center(arc_center, a0, -45.0 * rotation_sign as f64);
                        // "Constructing with a center can lead to imprecise endpoint. We need to guarantee tangency of the endpoint."
                        let endpoint = ca.end;
                        const MIN_PRECISION_IU: Nm = Nm::new(4);
                        if (endpoint.y - a1.y).abs() < MIN_PRECISION_IU {
                            ca = ShapeArc::from_start_end_angle(ca.start, Point { x: endpoint.x, y: a1.y }, 45.0 * rotation_sign as f64, Nm::ZERO);
                        } else if (endpoint.x - a1.x).abs() < MIN_PRECISION_IU {
                            ca = ShapeArc::from_start_end_angle(ca.start, Point { x: a1.x, y: endpoint.y }, 45.0 * rotation_sign as f64, Nm::ZERO);
                        }
                        out.arc_or_point(ca, a0);
                        out.point(a1);
                    }
                }
            }
            CornerMode::Mitered90 => {
                out.point(a0);
                out.point(add(a0, mp0));
                out.point(a1);
            }
            CornerMode::Rounded90 => {
                if w == h {
                    // "we only need one arc without a straight line"
                    out.arc(ShapeArc::from_start_end_center(a0, a1, sub(a1, mp0), (sh == sw) != start_diagonal));
                    return out.finish(false);
                }
                if start_diagonal {
                    // "Means start with the arc first"
                    if h > w {
                        let y = a0.y + w * sh;
                        out.arc(ShapeArc::from_start_end_center(a0, Point { x: a1.x, y }, Point { x: a0.x, y }, sh != sw));
                    } else {
                        let x = a0.x + h * sw;
                        out.arc(ShapeArc::from_start_end_center(a0, Point { x, y: a1.y }, Point { x, y: a0.y }, sh == sw));
                    }
                    out.point(a1);
                } else {
                    out.point(a0);
                    if w > h {
                        let x = a1.x - h * sw;
                        out.arc(ShapeArc::from_start_end_center(Point { x, y: a0.y }, a1, Point { x, y: a1.y }, sh != sw));
                    } else {
                        let y = a1.y - w * sh;
                        out.arc(ShapeArc::from_start_end_center(Point { x: a0.x, y }, a1, Point { x: a1.x, y }, sh == sw));
                    }
                }
            }
        }
        // `pl.Simplify()`
        out.finish(true)
    }
}

/// `SHAPE_ARC( aArcCenter, aArcStartPoint, aCenterAngle )`: the arc from the start turned about the centre by `angle` degrees (the middle by half),
/// rounded.
fn arc_from_center(center: Point, start: Point, angle_deg: f64) -> ShapeArc {
    let turn = |deg: f64| {
        let v = rotate_d(V2::new((start.x - center.x).as_f64(), (start.y - center.y).as_f64()), deg);
        Point { x: center.x + ki_round_nm(v.x), y: center.y + ki_round_nm(v.y) }
    };
    ShapeArc::new_nm(start, turn(-angle_deg / 2.0), turn(-angle_deg), Nm::ZERO)
}

/// The `SHAPE_LINE_CHAIN` `BuildInitialTrace` appends to; [`Self::finish`] hands it back as a [`Trace`].
#[derive(Default)]
struct TraceBuilder {
    /// Points (nm) and arcs (nm), in order.
    parts: Vec<Result<Point, ShapeArc>>,
}

impl TraceBuilder {
    fn point(&mut self, p: Point) {
        self.parts.push(Ok(p));
    }

    fn arc(&mut self, a: ShapeArc) {
        self.parts.push(Err(a));
    }

    /// `if( arc.GetP0() == arc.GetP1() ) pl.Append( p ) else pl.Append( arc )`.
    fn arc_or_point(&mut self, a: ShapeArc, p: Point) {
        if a.start == a.end {
            self.point(p)
        } else {
            self.arc(a)
        }
    }

    fn finish(self, simplify: bool) -> Trace {
        let mut line = crate::line::Line::new(None, 0, Nm::ZERO);
        for part in self.parts {
            match part {
                Ok(p) => {
                    let q = p;
                    if line.pts.last() != Some(&q) {
                        line.pts.push(q);
                    }
                }
                Err(a) => {
                    let arc = ArcPts::new(a.start, a.mid, a.end);
                    // `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`: an arc whose polyline is one segment ("if( chain.PointCount() > 2 )", the
                    // polygonisation error on the nanometre grid) is not an arc of the chain but its chord
                    if arc.is_degenerate() || a.convert_to_polyline(crate::kigeom::ARC_POLYGONIZATION_MAX_ERROR_NM).len() <= 2 {
                        for q in [arc.start, arc.end] {
                            if line.pts.last() != Some(&q) {
                                line.pts.push(q);
                            }
                        }
                    } else {
                        line.append_arc(arc);
                    }
                }
            }
        }
        if simplify {
            line.simplify();
        }
        // the arcs whose points are still in the line (a degenerate arc is its chord)
        let arcs = line.arc_runs().into_iter().map(|r| line.arcs[r.arc]).collect();
        Trace { pts: line.pts, arcs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_vector_quantizes_to_nearest_octant() {
        assert_eq!(Direction45::from_vector(Nm::new(100), Nm::ZERO), Direction45::E);
        assert_eq!(Direction45::from_vector(Nm::new(100), Nm::new(100)), Direction45::SE);
        assert_eq!(Direction45::from_vector(Nm::ZERO, Nm::new(-100)), Direction45::N);
        assert_eq!(Direction45::from_vector(Nm::ZERO, Nm::ZERO), Direction45::Undefined);
    }

    #[test]
    fn right_rotates_one_octant_and_wraps() {
        assert_eq!(Direction45::N.right(), Direction45::NE);
        assert_eq!(Direction45::NW.right(), Direction45::N);
    }

    #[test]
    fn left_is_the_other_way_round_and_undefined_stays_undefined() {
        assert_eq!(Direction45::N.left(), Direction45::NW);
        assert_eq!(Direction45::E.left(), Direction45::NE);
        for d in [Direction45::N, Direction45::SE, Direction45::W] {
            assert_eq!(d.left().right(), d);
        }
        assert_eq!(Direction45::Undefined.left(), Direction45::Undefined);
    }

    #[test]
    fn angle_classification_matches_table() {
        assert_eq!(Direction45::N.angle_to(&Direction45::N), AngleType::Straight);
        assert_eq!(Direction45::N.angle_to(&Direction45::NE), AngleType::Obtuse);
        assert_eq!(Direction45::N.angle_to(&Direction45::E), AngleType::Right);
        assert_eq!(Direction45::N.angle_to(&Direction45::SE), AngleType::Acute);
        assert_eq!(Direction45::N.angle_to(&Direction45::S), AngleType::HalfFull);
    }

    #[test]
    fn build_initial_trace_45_degree_elbow() {
        let p0 = Point { x: Nm::ZERO, y: Nm::ZERO };
        let p1 = Point { x: Nm::new(1000), y: Nm::new(500) };
        let path = Direction45::E.build_initial_trace(p0, p1, false, CornerMode::Mitered45);
        assert_eq!(path.len(), 3);
        // E is not diagonal -> straight leg first (due east, matching the
        // heading), covering the longer axis's excess (1000-500=500),
        // then a 45-degree leg covers the remaining (500,500) into p1.
        assert_eq!(path[1], Point { x: Nm::new(500), y: Nm::ZERO });
    }

    #[test]
    fn build_initial_trace_degenerates_to_one_segment_on_pure_diagonal() {
        let path = Direction45::N.build_initial_trace(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(500), y: Nm::new(500) }, false, CornerMode::Mitered45);
        assert_eq!(path, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(500), y: Nm::new(500) }]);
    }

    #[test]
    fn build_initial_trace_90_mode_is_axis_aligned() {
        let path = Direction45::E.build_initial_trace(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::new(500) }, false, CornerMode::Mitered90);
        assert_eq!(path.len(), 3);
        assert!(path[1] == Point { x: Nm::new(1000), y: Nm::ZERO } || path[1] == Point { x: Nm::ZERO, y: Nm::new(500) });
    }
}
