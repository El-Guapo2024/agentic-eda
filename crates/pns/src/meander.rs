//! `pcbnew/router/pns_meander.{h,cpp}` and the shape-fitting half of `pns_meander_placer_base.cpp`: the meander KiCad's length tuner draws.
//!
//! * [`MeanderSettings`] is `MEANDER_SETTINGS` (amplitude range and step, spacing, corner style and radius, single-sided, the initial side, the
//!   target length and skew with their tolerances).
//! * [`MeanderShape`] is `MEANDER_SHAPE`: one U (or half of a turn: `MT_START`, `MT_TURN`, `MT_FINISH`, `MT_SINGLE`) built by a turtle
//!   (`genMeanderShape`, `uShape`, `miter`, `makeMiterShape`) with rounded or chamfered corners, resized between the minimum and the maximum
//!   amplitude to fit what the world leaves it ([`MeanderShape::fit`], `Fit`), and dual (two lines at an offset) for a differential pair.
//! * [`MeanderedLine`] is `MEANDERED_LINE`: the shapes fitted one after the other along a baseline segment (`MeanderSegment`).
//! * [`tune_line_length`] is `MEANDER_PLACER_BASE::tuneLineLength`: the amplitudes shrunk, shape by shape, until the line is the length wanted
//!   (`findAmplitudeForLength`, `findAmplitudeBinarySearch`).
//!
//! The world the shapes have to fit in is the placer's ([`MeanderEnv::check_fit`], `MEANDER_PLACER_BASE::CheckFit`);
//! see [`crate::meander_placer`]. Nanometres throughout (see [`crate::kigeom`]).

use eda_model::units::Nm;
use crate::chain::ShapeLineChain;
use crate::kigeom::{ki_round, rotate_d, Seg, ShapeArc, V2};
use eda_model::ir::Point;

// ---------------------------------------------------------------------------------------------------------------------
// MEANDER_SETTINGS
// ---------------------------------------------------------------------------------------------------------------------

/// `pcbIUScale.mmToIU( 0.1 )`: `MEANDER_SETTINGS::DEFAULT_LENGTH_TOLERANCE`.
pub const DEFAULT_LENGTH_TOLERANCE: i64 = 100_000;
/// `MEANDER_SETTINGS::LENGTH_UNCONSTRAINED`: `1000000 * IU_PER_MM`.
pub const LENGTH_UNCONSTRAINED: i64 = 1_000_000_000_000;
/// `MEANDER_SETTINGS::SKEW_UNCONSTRAINED`: `std::numeric_limits<int>::max()`.
pub const SKEW_UNCONSTRAINED: i64 = i32::MAX as i64;

/// `MINOPTMAX<T>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MinOptMax {
    min: Option<i64>,
    opt: Option<i64>,
    max: Option<i64>,
}

impl MinOptMax {
    pub fn min(&self) -> i64 {
        self.min.unwrap_or(0)
    }
    pub fn max(&self) -> i64 {
        self.max.unwrap_or(i64::MAX)
    }
    pub fn opt(&self) -> i64 {
        self.opt.unwrap_or_else(|| self.min())
    }
    pub fn has_min(&self) -> bool {
        self.min.is_some()
    }
    pub fn has_max(&self) -> bool {
        self.max.is_some()
    }
    pub fn has_opt(&self) -> bool {
        self.opt.is_some()
    }
    pub fn set_min(&mut self, v: i64) {
        self.min = Some(v);
    }
    pub fn set_max(&mut self, v: i64) {
        self.max = Some(v);
    }
    pub fn set_opt(&mut self, v: i64) {
        self.opt = Some(v);
    }
    /// `IsNull`: nothing was ever set.
    pub fn is_null(&self) -> bool {
        self.min.is_none() && self.opt.is_none() && self.max.is_none()
    }
}

/// `MEANDER_STYLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeanderStyle {
    /// `MEANDER_STYLE_ROUND`: a 90-degree arc.
    Round,
    /// `MEANDER_STYLE_CHAMFER`: a 45-degree segment.
    Chamfer,
}

/// `MEANDER_SIDE`: the side the first meander is put on (`-1` left, `1` right, `0` the side of the cursor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeanderSide {
    Left = -1,
    Default = 0,
    Right = 1,
}

impl MeanderSide {
    pub fn value(self) -> i32 {
        self as i32
    }

    /// `(MEANDER_SIDE) -settings.m_initialSide`.
    pub fn flipped(self) -> MeanderSide {
        match self {
            MeanderSide::Left => MeanderSide::Right,
            MeanderSide::Right => MeanderSide::Left,
            MeanderSide::Default => MeanderSide::Default,
        }
    }
}

/// `MEANDER_SETTINGS` (the net class it carries is the tuner's to look up: [`crate::tuning_path::LengthEnv::delay_params`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MeanderSettings {
    pub min_amplitude: i64,
    pub max_amplitude: i64,
    pub spacing: i64,
    /// "Amplitude/spacing adjustment step".
    pub step: i64,
    pub len_pad_to_die: i64,
    pub target_length: MinOptMax,
    /// `m_targetLengthDelay`: the delay of the tuned line or pair, attoseconds ([`crate::delay`]), when the tuning is in the time domain.
    pub target_length_delay: MinOptMax,
    pub target_skew: MinOptMax,
    /// `m_targetSkewDelay`: the skew as a delay, attoseconds.
    pub target_skew_delay: MinOptMax,
    /// `m_isTimeDomain`: "Calculate tuning in the time domain".
    pub is_time_domain: bool,
    pub override_custom_rules: bool,
    pub corner_style: MeanderStyle,
    pub corner_radius_percentage: i64,
    pub single_sided: bool,
    pub initial_side: MeanderSide,
    pub length_tolerance: i64,
    pub keep_endpoints: bool,
}

impl Default for MeanderSettings {
    /// `MEANDER_SETTINGS::MEANDER_SETTINGS()`.
    fn default() -> Self {
        let mut s = MeanderSettings {
            min_amplitude: 200_000,
            max_amplitude: 1_000_000,
            spacing: 600_000,
            step: 50_000,
            len_pad_to_die: 0,
            target_length: MinOptMax::default(),
            target_length_delay: MinOptMax::default(),
            target_skew: MinOptMax::default(),
            target_skew_delay: MinOptMax::default(),
            is_time_domain: false,
            override_custom_rules: false,
            corner_style: MeanderStyle::Round,
            corner_radius_percentage: 80,
            single_sided: false,
            initial_side: MeanderSide::Left,
            length_tolerance: 0,
            keep_endpoints: false,
        };
        s.set_target_length(LENGTH_UNCONSTRAINED);
        s.set_target_length_delay(crate::delay::DELAY_UNCONSTRAINED);
        s.set_target_skew(0);
        s.set_target_skew_delay(0);
        s
    }
}

impl MeanderSettings {
    /// `SetTargetLength( long long )`: the value, with the default tolerance round it (none for "unconstrained").
    pub fn set_target_length(&mut self, opt: i64) {
        self.target_length.set_opt(opt);
        if opt == LENGTH_UNCONSTRAINED {
            self.target_length.set_min(0);
            self.target_length.set_max(opt);
        } else {
            self.target_length.set_min(opt - DEFAULT_LENGTH_TOLERANCE);
            self.target_length.set_max(opt + DEFAULT_LENGTH_TOLERANCE);
        }
    }

    /// `SetTargetLength( const MINOPTMAX<int>& )`: a constraint's value; its own minimum and maximum win over the default tolerance.
    pub fn set_target_length_constraint(&mut self, c: &MinOptMax) {
        self.set_target_length(c.opt());
        if c.has_min() {
            self.target_length.set_min(c.min());
        }
        if c.has_max() {
            self.target_length.set_max(c.max());
        }
    }

    /// `SetTargetLengthDelay( long long )`: the delay, with the default tolerance (0.1 ps) round it (none for "unconstrained").
    pub fn set_target_length_delay(&mut self, opt: i64) {
        self.target_length_delay.set_opt(opt);
        if opt == crate::delay::DELAY_UNCONSTRAINED {
            self.target_length_delay.set_min(0);
            self.target_length_delay.set_max(opt);
        } else {
            self.target_length_delay.set_min(opt - crate::delay::DEFAULT_DELAY_TOLERANCE);
            self.target_length_delay.set_max(opt + crate::delay::DEFAULT_DELAY_TOLERANCE);
        }
    }

    /// `SetTargetLengthDelay( const MINOPTMAX<int>& )`: a constraint's value; its own minimum and maximum win over the default tolerance.
    pub fn set_target_length_delay_constraint(&mut self, c: &MinOptMax) {
        self.set_target_length_delay(c.opt());
        if c.has_min() {
            self.target_length_delay.set_min(c.min());
        }
        if c.has_max() {
            self.target_length_delay.set_max(c.max());
        }
    }

    /// `SetTargetSkewDelay( int )`: the skew as a delay, with a tolerance of `DEFAULT_LENGTH_TOLERANCE` either side of it (KiCad uses that constant here; it
    /// is the same 100 000 as the delay tolerance in attoseconds).
    pub fn set_target_skew_delay(&mut self, opt: i64) {
        self.target_skew_delay.set_opt(opt);
        if opt == SKEW_UNCONSTRAINED {
            self.target_skew_delay.set_min(0);
            self.target_skew_delay.set_max(opt);
        } else {
            self.target_skew_delay.set_min(opt - DEFAULT_LENGTH_TOLERANCE);
            self.target_skew_delay.set_max(opt + DEFAULT_LENGTH_TOLERANCE);
        }
    }

    /// `SetTargetSkewDelay( const MINOPTMAX<int>& )`.
    pub fn set_target_skew_delay_constraint(&mut self, c: &MinOptMax) {
        self.set_target_skew_delay(c.opt());
        if c.has_min() {
            self.target_skew_delay.set_min(c.min());
        }
        if c.has_max() {
            self.target_skew_delay.set_max(c.max());
        }
    }

    /// `SetTargetSkew( int )`.
    pub fn set_target_skew(&mut self, opt: i64) {
        self.target_skew.set_opt(opt);
        if opt == SKEW_UNCONSTRAINED {
            self.target_skew.set_min(0);
            self.target_skew.set_max(opt);
        } else {
            self.target_skew.set_min(opt - DEFAULT_LENGTH_TOLERANCE);
            self.target_skew.set_max(opt + DEFAULT_LENGTH_TOLERANCE);
        }
    }

    /// `SetTargetSkew( const MINOPTMAX<int>& )`.
    pub fn set_target_skew_constraint(&mut self, c: &MinOptMax) {
        self.set_target_skew(c.opt());
        if c.has_min() {
            self.target_skew.set_min(c.min());
        }
        if c.has_max() {
            self.target_skew.set_max(c.max());
        }
    }
}

/// What the shapes ask the placer (`MEANDER_PLACER_BASE`): its settings (which a fit may flip the initial side of), the clearance of the
/// line being tuned, and whether a shape fits the board and its own earlier shapes (`CheckFit`).
pub trait MeanderEnv {
    fn settings(&self) -> &MeanderSettings;
    /// `UpdateSettings`.
    fn update_settings(&mut self, settings: MeanderSettings);
    /// `MEANDER_PLACER_BASE::Clearance`.
    fn clearance(&self) -> i64;
    /// `MEANDER_PLACER_BASE::CheckFit`: `existing` is the line's shapes so far (`m_result`).
    fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool;
}

// ---------------------------------------------------------------------------------------------------------------------
// MEANDER_SHAPE
// ---------------------------------------------------------------------------------------------------------------------

/// `MEANDER_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeanderType {
    /// `_|^|_`, single-sided.
    Single,
    /// `_|^|`.
    Start,
    /// `|^|_`.
    Finish,
    /// `|^|` or `|_|`.
    Turn,
    /// Try fitting a start, but produce no line.
    CheckStart,
    /// Try fitting a finish, but produce no line.
    CheckFinish,
    /// A corner of the line.
    Corner,
    /// An arc corner.
    Arc,
    /// No meander (a straight line).
    Empty,
}

/// `DEG2RAD( 22.5 )`.
fn deg22_5() -> f64 {
    22.5 * std::f64::consts::PI / 180.0
}

/// `MEANDER_SHAPE`: the geometry of one meander. The settings and the clearance the shape reads off its placer are copied when it is made.
#[derive(Debug, Clone)]
pub struct MeanderShape {
    settings: MeanderSettings,
    clearance: i64,
    m_type: MeanderType,
    dual: bool,
    width: i64,
    amplitude: i64,
    baseline_offset: i64,
    mean_corner_radius: i64,
    target_base_len: i64,
    p0: Point,
    base_seg: Seg,
    clipped_base_seg: Seg,
    side: bool,
    shapes: [ShapeLineChain; 2],
    base_index: i64,
}

/// The turtle `MEANDER_SHAPE::start / forward / turn / miter / uShape` draw with.
struct Turtle {
    lc: ShapeLineChain,
    dir: V2,
    pos: V2,
}

impl MeanderShape {
    /// `MEANDER_SHAPE( aPlacer, aWidth, aIsDual )`.
    pub fn new(env: &dyn MeanderEnv, width: i64, dual: bool) -> MeanderShape {
        let zero = Point { x: Nm::ZERO, y: Nm::ZERO };
        MeanderShape {
            settings: env.settings().clone(),
            clearance: env.clearance(),
            m_type: MeanderType::Single,
            dual,
            width,
            amplitude: 0,
            baseline_offset: 0,
            mean_corner_radius: 0,
            target_base_len: 0,
            p0: zero,
            base_seg: Seg::new(zero, zero),
            clipped_base_seg: Seg::new(zero, zero),
            side: false,
            shapes: [ShapeLineChain::new(), ShapeLineChain::new()],
            base_index: 0,
        }
    }

    pub fn set_type(&mut self, t: MeanderType) {
        self.m_type = t;
    }

    pub fn kind(&self) -> MeanderType {
        self.m_type
    }

    pub fn set_base_index(&mut self, i: i64) {
        self.base_index = i;
    }

    pub fn base_index(&self) -> i64 {
        self.base_index
    }

    pub fn amplitude(&self) -> i64 {
        self.amplitude
    }

    pub fn is_dual(&self) -> bool {
        self.dual
    }

    pub fn side(&self) -> bool {
        self.side
    }

    /// `End`: the end of the base segment of the shape.
    pub fn end(&self) -> Point {
        self.clipped_base_seg.b
    }

    /// `CLine( aShape )`.
    pub fn c_line(&self, which: usize) -> &ShapeLineChain {
        &self.shapes[which]
    }

    pub fn base_segment(&self) -> Seg {
        self.clipped_base_seg
    }

    pub fn width(&self) -> i64 {
        self.width
    }

    pub fn set_baseline_offset(&mut self, offset: i64) {
        self.baseline_offset = offset;
    }

    pub fn baseline_offset(&self) -> i64 {
        self.baseline_offset
    }

    /// `SetTargetBaselineLength`.
    pub fn set_target_baseline_length(&mut self, len: i64) {
        self.target_base_len = len;
    }

    /// `MakeCorner`: a dummy shape for a corner of the line (the second point is the other line's, for a pair).
    pub fn make_corner(&mut self, p1: Point, p2: Point) {
        self.m_type = MeanderType::Corner;
        self.shapes[0] = ShapeLineChain::from_points(&[p1]);
        self.shapes[1] = ShapeLineChain::from_points(&[p2]);
        self.clipped_base_seg = Seg::new(p1, p1);
    }

    /// `MakeArc`: a dummy shape for an arc of the line.
    pub fn make_arc(&mut self, a1: &ShapeArc, a2: &ShapeArc) {
        self.m_type = MeanderType::Corner;
        self.shapes[0].clear();
        self.shapes[1].clear();
        self.shapes[0].append_arc(a1);
        self.shapes[1].append_arc(a2);
        self.clipped_base_seg = Seg::new(a1.end, a1.end); // `m_clippedBaseSeg.A = m_clippedBaseSeg.B = aArc1.GetP1()`
    }

    // ----------------------------------------------------------------------------------------------- sizes

    /// `MinAmplitude`.
    pub fn min_amplitude(&self) -> i64 {
        let mut min_amplitude = self.settings.min_amplitude;
        if self.settings.corner_style == MeanderStyle::Round {
            min_amplitude = min_amplitude.max(self.baseline_offset.abs() + self.width);
        } else {
            // `int correction = m_width * tan( 1 - tan( DEG2RAD( 22.5 ) ) )`: the expression is KiCad's.
            let correction = (self.width as f64 * (1.0 - deg22_5().tan()).tan()) as i64;
            min_amplitude = min_amplitude.max(self.baseline_offset.abs() + correction);
        }
        min_amplitude
    }

    /// `spacing`.
    pub fn spacing(&self) -> i64 {
        if !self.dual {
            (self.width + self.clearance).max(self.settings.spacing)
        } else {
            let sp = self.width + self.clearance + 2 * self.baseline_offset.abs();
            sp.max(self.settings.spacing)
        }
    }

    /// `cornerRadius`.
    pub fn corner_radius(&self) -> i64 {
        if self.amplitude == 0 {
            return 0;
        }
        let min_cr: i64 = if self.settings.corner_style == MeanderStyle::Round {
            self.baseline_offset.abs() + self.width / 2
        } else {
            (self.baseline_offset.abs() as f64 + (self.width / 2) as f64 * (1.0 - deg22_5().tan())) as i64
        };
        let max_cr1 = (self.amplitude + self.baseline_offset.abs()) / 2;
        let max_cr2 = self.spacing() / 2;
        let max_cr = max_cr1.min(max_cr2);
        // `wxCHECK2_MSG( maxCr >= minCr, return maxCr, ... )`
        if max_cr < min_cr {
            return max_cr;
        }
        let r_percent = self.settings.corner_radius_percentage;
        let opt_cr = (self.spacing() as i128 * r_percent as i128 / 200) as i64;
        opt_cr.clamp(min_cr, max_cr)
    }

    // ----------------------------------------------------------------------------------------------- turtle

    fn start(t: &mut Turtle, where_: V2, dir: V2) {
        t.lc.clear();
        t.lc.append_point(where_.trunc());
        t.dir = dir;
        t.pos = where_;
    }

    /// `forward`: "Very small segments cause problems."
    fn forward(t: &mut Turtle, length: i64) {
        if length < 5 {
            return;
        }
        t.pos = t.pos.add(t.dir.resize(length as f64));
        t.lc.append_point(t.pos.trunc());
    }

    fn turn(t: &mut Turtle, angle_deg: f64) {
        t.dir = rotate_d(t.dir, angle_deg);
    }

    /// `makeMiterShape`.
    fn make_miter_shape(&self, p: V2, dir: V2, side: bool) -> ShapeLineChain {
        let mut lc = ShapeLineChain::new();
        if dir.norm() == 0.0 {
            lc.append_point(p.trunc());
            return lc;
        }
        let dir_u = dir;
        let dir_v = dir.perpendicular();
        let sgn = if side { -1.0 } else { 1.0 };
        let end_point = p.add(dir_u).add(dir_v.scale(sgn));
        lc.append_point(p.trunc());
        match self.settings.corner_style {
            MeanderStyle::Round => {
                let arc_end = end_point.trunc();
                let arc = ShapeArc::from_start_end_angle(p.trunc(), arc_end, if side { -90.0 } else { 90.0 }, 0);
                lc.append_arc(&arc);
            }
            MeanderStyle::Chamfer => {
                let radius = dir.norm();
                let mut correction = 0.0;
                if self.dual && radius > self.mean_corner_radius as f64 {
                    correction = (-2 * self.baseline_offset.abs()) as f64 * deg22_5().tan();
                }
                let dir_cu = dir_u.resize(correction);
                let dir_cv = dir_v.resize(correction);
                let pt = p.sub(dir_cu);
                lc.append_point(pt.trunc());
                let pt = p.add(dir_u).add(dir_v.add(dir_cv).scale(sgn));
                lc.append_point(pt.trunc());
                lc.append_point(end_point.trunc());
            }
        }
        lc
    }

    /// `miter`.
    fn miter(&self, t: &mut Turtle, radius: i64, side: bool) {
        if radius <= 0 {
            Self::turn(t, if side { 90.0 } else { -90.0 });
            return;
        }
        let dir = t.dir.resize(radius as f64);
        let lc = self.make_miter_shape(t.pos, dir, side);
        t.pos = V2::of(lc.c_last_point());
        Self::turn(t, if side { 90.0 } else { -90.0 });
        t.lc.append_chain(&lc);
    }

    /// `uShape`.
    fn u_shape(&self, t: &mut Turtle, sides: i64, corner: i64, top: i64) {
        Self::forward(t, sides);
        self.miter(t, corner, true);
        Self::forward(t, top);
        self.miter(t, corner, true);
        Self::forward(t, sides);
    }

    /// `genMeanderShape`: the line of one meander of type `ty` that starts at `p` and goes along `dir`, on `side` (true: the right) of it; the
    /// line is `baseline_offset` off the baseline (the lines of a pair).
    fn gen_meander_shape(&mut self, p: V2, dir: V2, side: bool, ty: MeanderType, baseline_offset: i64) -> ShapeLineChain {
        let mut cr = self.corner_radius();
        let mut offset = baseline_offset;
        let spc = self.spacing();
        let amplitude = self.amplitude;
        let target_base_len = self.target_base_len;

        if side {
            offset *= -1;
        }

        let dir_u_b = dir.resize(offset as f64);
        let dir_v_b = dir_u_b.perpendicular();

        if 2 * cr > amplitude + offset.abs() {
            cr = (amplitude + offset.abs()) / 2;
        }
        if 2 * cr > spc {
            cr = spc / 2;
        }
        if cr - offset < 0 {
            cr = offset;
        }
        self.mean_corner_radius = cr;

        let s_corner = cr - offset;
        let u_corner = cr + offset;
        let start_side = amplitude - 2 * cr + offset.abs();
        let turn_side = amplitude - cr;
        let mut top = spc - 2 * cr;

        let mut t = Turtle { lc: ShapeLineChain::new(), dir, pos: p };
        Self::start(&mut t, p.add(dir_v_b), dir);

        match ty {
            MeanderType::Empty => {
                t.lc.append_point(p.add(dir_v_b).add(dir).trunc());
            }
            MeanderType::Start => {
                if target_base_len != 0 {
                    top = top.max(target_base_len - s_corner - u_corner * 2 + offset);
                }
                self.miter(&mut t, s_corner, false);
                self.u_shape(&mut t, start_side, u_corner, top);
                Self::forward(&mut t, s_corner.min(u_corner));
                Self::forward(&mut t, offset.abs());
            }
            MeanderType::Finish => {
                if target_base_len != 0 {
                    top = top.max(target_base_len - cr - spc);
                }
                Self::start(&mut t, p.sub(dir_u_b), dir);
                Self::turn(&mut t, -90.0);
                Self::forward(&mut t, s_corner.min(u_corner));
                Self::forward(&mut t, offset.abs());
                self.u_shape(&mut t, start_side, u_corner, top);
                self.miter(&mut t, s_corner, false);
                if target_base_len >= spc + cr {
                    t.lc.append_point(p.add(dir_v_b).add(dir.resize(target_base_len as f64)).trunc());
                } else {
                    t.lc.append_point(p.add(dir_v_b).add(dir.resize((2 * spc - cr) as f64)).trunc());
                }
            }
            MeanderType::Turn => {
                if target_base_len != 0 {
                    top = top.max(target_base_len - u_corner * 2 + offset * 2);
                }
                Self::start(&mut t, p.sub(dir_u_b), dir);
                Self::turn(&mut t, -90.0);
                Self::forward(&mut t, offset.abs());
                self.u_shape(&mut t, turn_side, u_corner, top);
                Self::forward(&mut t, offset.abs());
            }
            MeanderType::Single => {
                if target_base_len != 0 {
                    top = top.max((target_base_len - s_corner * 2 - u_corner * 2) / 2);
                }
                self.miter(&mut t, s_corner, false);
                self.u_shape(&mut t, start_side, u_corner, top);
                self.miter(&mut t, s_corner, false);
                t.lc.append_point(p.add(dir_v_b).add(dir.resize((2 * spc) as f64)).trunc());
            }
            _ => {}
        }

        let mut lc = t.lc;
        if side {
            let axis = Seg::new(p.trunc(), p.add(dir).trunc());
            lc.mirror(&axis);
        }
        lc
    }

    /// `updateBaseSegment`: the part of the base segment the shape covers.
    fn update_base_segment(&mut self) {
        if self.dual {
            // `( CLine( 0 ).CPoint( 0 ) + CLine( 1 ).CPoint( 0 ) ) / 2`: `VECTOR2I::operator/( double )` rounds.
            let mid = |p: Point, q: Point| Point { x: Nm::from_raw_shim(ki_round((p.x + q.x).raw_shim() as f64 / 2.0)), y: Nm::from_raw_shim(ki_round((p.y + q.y).raw_shim() as f64 / 2.0)) };
            let a = mid(self.c_line(0).c_point(0), self.c_line(1).c_point(0));
            let b = mid(self.c_line(0).c_last_point(), self.c_line(1).c_last_point());
            self.clipped_base_seg = Seg::new(self.base_seg.line_project(a), self.base_seg.line_project(b));
        } else {
            self.clipped_base_seg = Seg::new(self.base_seg.line_project(self.c_line(0).c_point(0)), self.base_seg.line_project(self.c_line(0).c_last_point()));
        }
    }

    // ----------------------------------------------------------------------------------------------- fitting

    /// `Fit`: puts a meander of type `ty` on `seg` from `p`, on `side`, as tall as the placer's maximum amplitude lets it fit, shrinking in steps
    /// to the minimum. `existing` are the line's shapes so far. Returns whether one fit.
    pub fn fit(&mut self, env: &mut dyn MeanderEnv, existing: &[MeanderShape], ty: MeanderType, seg: Seg, p: Point, side: bool) -> bool {
        let (prim1, prim2, check_mode) = match ty {
            MeanderType::CheckStart => (MeanderType::Start, MeanderType::Turn, true),
            MeanderType::CheckFinish => (MeanderType::Turn, MeanderType::Finish, true),
            _ => (MeanderType::Empty, MeanderType::Empty, false),
        };

        if check_mode {
            let mut m1 = MeanderShape::new(&*env, self.width, self.dual);
            let mut m2 = MeanderShape::new(&*env, self.width, self.dual);
            m1.set_baseline_offset(self.baseline_offset);
            m2.set_baseline_offset(self.baseline_offset);
            let c1 = m1.fit(env, existing, prim1, seg, p, side);
            let mut c2 = false;
            if c1 {
                c2 = m2.fit(env, existing, prim2, seg, m1.end(), !side);
            }
            if c1 && c2 {
                self.m_type = prim1;
                self.shapes = m1.shapes.clone();
                self.base_seg = seg;
                self.p0 = p;
                self.side = side;
                self.amplitude = m1.amplitude();
                self.dual = m1.dual;
                self.base_seg = m1.base_seg;
                self.base_index = m1.base_index;
                self.update_base_segment();
                self.baseline_offset = m1.baseline_offset;
                return true;
            }
            return false;
        }

        let min_ampl = self.min_amplitude();
        let max_ampl = self.settings.max_amplitude.max(min_ampl);
        // "Calculate minimum acceptable corner radius for visible rounding": at least half the track width (issue #8629).
        let min_corner_radius = self.width / 2;

        let mut ampl = max_ampl;
        while ampl >= min_ampl {
            self.amplitude = ampl;
            let d = V2::of(Point { x: seg.b.x - seg.a.x, y: seg.b.y - seg.a.y });
            if self.dual {
                let off = self.baseline_offset;
                self.shapes[0] = self.gen_meander_shape(V2::of(p), d, side, ty, off);
                self.shapes[1] = self.gen_meander_shape(V2::of(p), d, side, ty, -off);
            } else {
                self.shapes[0] = self.gen_meander_shape(V2::of(p), d, side, ty, 0);
            }
            self.m_type = ty;
            self.base_seg = seg;
            self.p0 = p;
            self.side = side;
            self.update_base_segment();

            // "Reject configurations that would result in nearly-square corners (issue #8629)."
            if self.mean_corner_radius >= min_corner_radius && env.check_fit(self, existing) {
                return true;
            }
            // (a step of nothing would never leave the loop)
            ampl -= self.settings.step.max(1);
        }
        false
    }

    /// `Recalculate`.
    pub fn recalculate(&mut self) {
        let d = V2::of(Point { x: self.base_seg.b.x - self.base_seg.a.x, y: self.base_seg.b.y - self.base_seg.a.y });
        let off = if self.dual { self.baseline_offset } else { 0 };
        self.shapes[0] = self.gen_meander_shape(V2::of(self.p0), d, self.side, self.m_type, off);
        if self.dual {
            self.shapes[1] = self.gen_meander_shape(V2::of(self.p0), d, self.side, self.m_type, -self.baseline_offset);
        }
        self.update_base_segment();
    }

    /// `Resize`: the amplitude, no lower than the minimum "needed for proper corner radii".
    pub fn resize(&mut self, ampl: i64) {
        if ampl < 0 {
            return;
        }
        let min_ampl = self.min_amplitude();
        self.amplitude = ampl.max(min_ampl);
        self.recalculate();
    }

    /// `MakeEmpty`: the shape becomes the straight bypass.
    pub fn make_empty(&mut self) {
        self.update_base_segment();
        let dir = V2::of(Point { x: self.clipped_base_seg.b.x - self.clipped_base_seg.a.x, y: self.clipped_base_seg.b.y - self.clipped_base_seg.a.y });
        self.m_type = MeanderType::Empty;
        self.amplitude = 0;
        let off = if self.dual { self.baseline_offset } else { 0 };
        self.shapes[0] = self.gen_meander_shape(V2::of(self.p0), dir, self.side, self.m_type, off);
        if self.dual {
            self.shapes[1] = self.gen_meander_shape(V2::of(self.p0), dir, self.side, self.m_type, -self.baseline_offset);
        }
    }

    /// `BaselineLength`.
    pub fn baseline_length(&self) -> i64 {
        self.clipped_base_seg.length()
    }

    /// `CurrentLength`.
    pub fn current_length(&self) -> i64 {
        self.c_line(0).length()
    }

    /// `MinTunableLength`: the length of the shape at its minimum amplitude.
    pub fn min_tunable_length(&self) -> i64 {
        let mut copy = self.clone();
        copy.set_target_baseline_length(self.baseline_length());
        let min = copy.min_amplitude();
        copy.resize(min);
        copy.current_length()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MEANDERED_LINE
// ---------------------------------------------------------------------------------------------------------------------

/// `MEANDERED_LINE`: the meanders (and the corners and arcs between them) of a line, in order.
#[derive(Debug, Clone, Default)]
pub struct MeanderedLine {
    last: Point,
    meanders: Vec<MeanderShape>,
    dual: bool,
    width: i64,
    baseline_offset: i64,
}

impl MeanderedLine {
    /// `MEANDERED_LINE( aPlacer, aIsDual )`.
    pub fn new(dual: bool) -> MeanderedLine {
        MeanderedLine { last: Point { x: Nm::ZERO, y: Nm::ZERO }, meanders: Vec::new(), dual, width: 0, baseline_offset: 0 }
    }

    pub fn set_width(&mut self, w: i64) {
        self.width = w;
    }

    pub fn set_baseline_offset(&mut self, o: i64) {
        self.baseline_offset = o;
    }

    pub fn meanders(&self) -> &[MeanderShape] {
        &self.meanders
    }

    pub fn meanders_mut(&mut self) -> &mut [MeanderShape] {
        &mut self.meanders
    }

    /// `AddCorner`.
    pub fn add_corner(&mut self, env: &dyn MeanderEnv, a: Point, b: Point) {
        let mut m = MeanderShape::new(env, self.width, self.dual);
        m.make_corner(a, b);
        self.last = a;
        self.meanders.push(m);
    }

    /// `AddArc`.
    pub fn add_arc(&mut self, env: &dyn MeanderEnv, a1: &ShapeArc, a2: &ShapeArc) {
        let mut m = MeanderShape::new(env, self.width, self.dual);
        m.make_arc(a1, a2);
        self.last = a1.end; // `m_last = aArc1.GetP1()`
        self.meanders.push(m);
    }

    /// `AddMeander`.
    pub fn add_meander(&mut self, shape: MeanderShape) {
        self.last = shape.base_segment().b;
        self.meanders.push(shape);
    }

    /// `CheckSelfIntersections`: does `shape` keep `clearance` from the line's earlier meanders (those not parallel to it)?
    pub fn check_self_intersections(meanders: &[MeanderShape], shape: &MeanderShape, clearance: i64) -> bool {
        for m in meanders.iter().rev() {
            if m.kind() == MeanderType::Empty || m.kind() == MeanderType::Corner {
                continue;
            }
            let b1 = shape.base_segment();
            let b2 = m.base_segment();
            if b1.approx_parallel(&b2, 1) {
                continue;
            }
            let n = m.c_line(0).segment_count();
            for j in (0..n).rev() {
                if shape.c_line(0).collides_seg(&m.c_line(0).c_segment(j), clearance) {
                    return false;
                }
            }
        }
        true
    }

    /// `MeanderSegment`: fits maximum-amplitude meanders on the base segment `base`, starting on `side`.
    pub fn meander_segment(&mut self, env: &mut dyn MeanderEnv, base: Seg, side_in: bool, base_index: i64) {
        let base_len = base.length() as f64;
        let single_sided = env.settings().single_sided;
        let mut side = side_in;
        let dir = V2::of(Point { x: base.b.x - base.a.x, y: base.b.y - base.a.y });

        if !self.dual {
            self.add_corner(&*env, base.a, Point { x: Nm::ZERO, y: Nm::ZERO });
        }

        let mut turning = false;
        let mut started = false;
        self.last = base.a;

        loop {
            let mut m = MeanderShape::new(&*env, self.width, self.dual);
            m.set_baseline_offset(self.baseline_offset);
            m.set_base_index(base_index);

            let thr = m.spacing() as f64;
            let mut fail = false;
            let mut remaining = base_len - crate::kigeom::norm_i((self.last.x - base.a.x).raw_shim(), (self.last.y - base.a.y).raw_shim()) as f64;

            if (remaining as f64) < env.settings().step as f64 {
                break;
            }

            // `addSingleIfFits`
            macro_rules! add_single_if_fits {
                () => {{
                    fail = true;
                    if m.fit(env, &self.meanders, MeanderType::Single, base, self.last, side) {
                        self.add_meander(m.clone());
                        fail = false;
                        started = false;
                    }
                    if fail && !single_sided && m.fit(env, &self.meanders, MeanderType::Single, base, self.last, !side) {
                        if !started {
                            flip_initial_side(env);
                        }
                        self.add_meander(m.clone());
                        fail = false;
                        started = false;
                        side = !side;
                    }
                }};
            }

            if !single_sided && remaining > 3.0 * thr {
                if !turning {
                    for i in 0..2 {
                        let check_side = if i == 0 { side } else { !side };
                        if m.fit(env, &self.meanders, MeanderType::CheckStart, base, self.last, check_side) {
                            if !started && check_side != side {
                                flip_initial_side(env);
                            }
                            turning = true;
                            self.add_meander(m.clone());
                            side = !check_side;
                            started = true;
                            break;
                        }
                    }
                    if !turning {
                        add_single_if_fits!();
                    }
                } else {
                    let rv = m.fit(env, &self.meanders, MeanderType::CheckFinish, base, self.last, side);
                    if rv {
                        m.fit(env, &self.meanders, MeanderType::Turn, base, self.last, side);
                        self.add_meander(m.clone());
                        side = !side;
                        started = true;
                    } else {
                        m.fit(env, &self.meanders, MeanderType::Finish, base, self.last, side);
                        started = false;
                        self.add_meander(m.clone());
                        turning = false;
                    }
                }
            } else if !single_sided && started {
                let rv = m.fit(env, &self.meanders, MeanderType::Finish, base, self.last, side);
                if rv {
                    self.add_meander(m.clone());
                }
                break;
            } else if !turning && remaining > thr * 2.0 {
                add_single_if_fits!();
            } else {
                fail = true;
            }

            remaining = base_len - crate::kigeom::norm_i((self.last.x - base.a.x).raw_shim(), (self.last.y - base.a.y).raw_shim()) as f64;
            if remaining < env.settings().step as f64 {
                break;
            }

            if fail {
                let mut tmp = MeanderShape::new(&*env, self.width, self.dual);
                tmp.set_baseline_offset(self.baseline_offset);
                tmp.set_base_index(base_index);
                let next_p = tmp.spacing() - 2 * tmp.corner_radius() + env.settings().step;
                // `VECTOR2I pn = m_last + dir.Resize( nextP )`: the sum is a `VECTOR2D`, truncated to integers.
                let step = dir.resize(next_p as f64);
                let pn = Point { x: Nm::from_raw_shim((self.last.x.raw_shim() as f64 + step.x) as i64), y: Nm::from_raw_shim((self.last.y.raw_shim() as f64 + step.y) as i64) };
                if base.contains(pn) && !self.dual {
                    self.add_corner(&*env, pn, Point { x: Nm::ZERO, y: Nm::ZERO });
                } else {
                    break;
                }
            }
        }

        if !self.dual {
            self.add_corner(&*env, base.b, Point { x: Nm::ZERO, y: Nm::ZERO });
        }
    }
}

/// `flipInitialSide`: `settings.m_initialSide = -settings.m_initialSide` and `UpdateSettings`.
fn flip_initial_side(env: &mut dyn MeanderEnv) {
    let mut s = env.settings().clone();
    s.initial_side = s.initial_side.flipped();
    env.update_settings(s);
}

// ---------------------------------------------------------------------------------------------------------------------
// tuneLineLength
// ---------------------------------------------------------------------------------------------------------------------

/// `LENGTH_TARGET_TOLERANCE`: nanometres.
pub const LENGTH_TARGET_TOLERANCE: i64 = 20;

/// `findAmplitudeBinarySearch`.
fn find_amplitude_binary_search(copy: &mut MeanderShape, target_length: i64, min_amp: i64, max_amp: i64) -> i64 {
    if min_amp == max_amp {
        return max_amp;
    }
    copy.resize(min_amp);
    // `int minLen = aCopy.CurrentLength();` (a `long long` narrowed to `int`)
    let min_len = copy.current_length() as i32 as i64;
    copy.resize(max_amp);
    let max_len = copy.current_length() as i32 as i64;

    if min_len > target_length {
        return 0;
    }
    if max_len < target_length {
        return 0;
    }
    let min_error = min_len - target_length;
    let max_error = max_len - target_length;

    if min_error.abs() < LENGTH_TARGET_TOLERANCE || max_error.abs() < LENGTH_TARGET_TOLERANCE {
        return if min_error.abs() < max_error.abs() { min_amp } else { max_amp };
    }
    let left = find_amplitude_binary_search(copy, target_length, min_amp, (min_amp + max_amp) / 2);
    if left != 0 {
        return left;
    }
    let right = find_amplitude_binary_search(copy, target_length, (min_amp + max_amp) / 2, max_amp);
    if right != 0 {
        return right;
    }
    0
}

/// `findAmplitudeForLength`.
fn find_amplitude_for_length(m: &MeanderShape, target_length: i64, min_amp: i64, max_amp: i64) -> i64 {
    let mut copy = m.clone();
    // "Try to keep the same baseline length"
    copy.set_target_baseline_length(m.baseline_length());
    let initial_guess = m.amplitude() - (m.current_length() - target_length) / 2;
    if initial_guess >= min_amp && initial_guess <= max_amp {
        copy.resize(min_amp);
        if (copy.current_length() - target_length).abs() < LENGTH_TARGET_TOLERANCE {
            return initial_guess;
        }
    }
    // "The length is non-trivial, use binary search"
    find_amplitude_binary_search(&mut copy, target_length, min_amp, max_amp)
}

/// `MEANDER_PLACER_BASE::tuneLineLength`: the meanders of `tuned` made to lengthen the line by `elongation`: the shapes the elongation cannot
/// use are emptied (the last one that is needed is turned into an end shape), the others shrunk in amplitude, one by one, until the sum is right.
pub fn tune_line_length(tuned: &mut MeanderedLine, elongation: i64) {
    let mut max_elongation: i64 = 0;
    let mut min_elongation: i64 = 0;
    let mut finished = false;

    for m in tuned.meanders_mut() {
        if m.kind() != MeanderType::Corner && m.kind() != MeanderType::Arc {
            let mut end = m.clone();
            let end_type = if m.kind() == MeanderType::Start || m.kind() == MeanderType::Single { MeanderType::Single } else { MeanderType::Finish };
            end.set_type(end_type);
            end.recalculate();

            let max_end_elongation = end.current_length() - end.baseline_length();

            if max_elongation + max_end_elongation > elongation {
                if !finished {
                    m.set_type(end_type);
                    m.recalculate();

                    if end_type == MeanderType::Single {
                        // "Check if we need to fit this meander"
                        let end_min_elongation = m.min_tunable_length() - m.baseline_length();
                        if min_elongation + end_min_elongation >= elongation {
                            m.make_empty();
                        }
                    }
                    finished = true;
                } else {
                    m.make_empty();
                }
            }

            max_elongation += m.current_length() - m.baseline_length();
            min_elongation += m.min_tunable_length() - m.baseline_length();
        }
    }

    let mut remaining_elongation = elongation;
    let mut meander_count: i64 = 0;
    for m in tuned.meanders() {
        if m.kind() != MeanderType::Corner && m.kind() != MeanderType::Arc && m.kind() != MeanderType::Empty {
            remaining_elongation -= m.current_length() - m.baseline_length();
            meander_count += 1;
        }
    }

    let mut len_reduction_left = -remaining_elongation;
    let mut meanders_left = meander_count;

    if len_reduction_left < 0 || meanders_left == 0 {
        return;
    }

    for m in tuned.meanders_mut() {
        if m.kind() != MeanderType::Corner && m.kind() != MeanderType::Arc && m.kind() != MeanderType::Empty {
            let len_reduction_here = len_reduction_left / meanders_left;
            let initial_len = m.current_length();
            let min_ampl = m.min_amplitude();

            // `findAmplitudeForLength( m, int targetLength, int minAmp, int maxAmp )`: the arguments are `int`s.
            let mut amp = find_amplitude_for_length(m, (initial_len - len_reduction_here) as i32 as i64, min_ampl, m.amplitude());
            if amp < min_ampl {
                amp = min_ampl;
            }

            m.set_target_baseline_length(m.baseline_length());
            m.resize(amp);

            len_reduction_left -= initial_len - m.current_length();
            meanders_left -= 1;

            if meanders_left == 0 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    /// A placer with nothing in the way and a fixed clearance.
    struct Free {
        settings: MeanderSettings,
        clearance: i64,
        fits: usize,
    }

    impl MeanderEnv for Free {
        fn settings(&self) -> &MeanderSettings {
            &self.settings
        }
        fn update_settings(&mut self, s: MeanderSettings) {
            self.settings = s;
        }
        fn clearance(&self) -> i64 {
            self.clearance
        }
        fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool {
            self.fits += 1;
            let clearance = shape.width() + self.settings.spacing;
            MeanderedLine::check_self_intersections(existing, shape, clearance)
        }
    }

    fn free() -> Free {
        Free { settings: MeanderSettings::default(), clearance: 200_000, fits: 0 }
    }

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
    }

    #[test]
    fn the_settings_start_as_kicads_and_a_target_gets_its_default_tolerance() {
        let s = MeanderSettings::default();
        assert_eq!((s.min_amplitude, s.max_amplitude, s.spacing, s.step), (200_000, 1_000_000, 600_000, 50_000));
        assert_eq!((s.corner_style, s.corner_radius_percentage, s.single_sided, s.initial_side), (MeanderStyle::Round, 80, false, MeanderSide::Left));
        assert_eq!((s.target_length.min(), s.target_length.opt(), s.target_length.max()), (0, LENGTH_UNCONSTRAINED, LENGTH_UNCONSTRAINED));
        assert_eq!(s.target_skew.opt(), 0);
        let mut s = s;
        s.set_target_length(50_000_000);
        assert_eq!((s.target_length.min(), s.target_length.opt(), s.target_length.max()), (49_900_000, 50_000_000, 50_100_000));
        let mut c = MinOptMax::default();
        c.set_min(48_000_000);
        c.set_opt(50_000_000);
        s.set_target_length_constraint(&c);
        assert_eq!((s.target_length.min(), s.target_length.max()), (48_000_000, 50_100_000), "a constraint's own minimum wins, its missing maximum is the default tolerance");
    }

    #[test]
    fn a_single_meander_is_a_u_with_rounded_corners_on_the_side_asked() {
        let mut env = free();
        let mut m = MeanderShape::new(&env, 200_000, false);
        let base = Seg::new(p(0, 0), p(20_000_000, 0));
        assert!(m.fit(&mut env, &[], MeanderType::Single, base, p(0, 0), false));
        let line = m.c_line(0);
        assert_eq!(line.c_point(0), p(0, 0));
        // it climbs to the maximum amplitude on the side a "false" side is, and comes back to the baseline
        let ys: Vec<Nm> = line.points().iter().map(|q| q.y).collect();
        let (lo, hi) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        assert_eq!((lo, hi), (Nm::ZERO, mm(1.0)), "amplitude 1 mm up (y grows downward: the left of a line along +x is +y)");
        assert_eq!(line.arcs().len(), 4, "four rounded corners");
        assert_eq!(line.c_last_point().y, Nm::ZERO);
        // the right side is the mirror image
        let mut right = MeanderShape::new(&env, 200_000, false);
        assert!(right.fit(&mut env, &[], MeanderType::Single, base, p(0, 0), true));
        let ys: Vec<Nm> = right.c_line(0).points().iter().map(|q| q.y).collect();
        assert_eq!((*ys.iter().min().unwrap(), *ys.iter().max().unwrap()), (mm(-1.0), Nm::ZERO));
        assert_eq!(right.c_line(0).length(), m.c_line(0).length());
        assert_eq!(m.baseline_length(), m.end().x.raw_shim());
    }

    #[test]
    fn a_chamfered_meander_has_forty_five_degree_corners_and_no_arcs() {
        let mut env = free();
        env.settings.corner_style = MeanderStyle::Chamfer;
        let mut m = MeanderShape::new(&env, 200_000, false);
        assert!(m.fit(&mut env, &[], MeanderType::Single, Seg::new(p(0, 0), p(20_000_000, 0)), p(0, 0), false));
        assert_eq!(m.c_line(0).arcs().len(), 0);
        let mut diagonals = 0;
        for i in 0..m.c_line(0).segment_count() {
            let s = m.c_line(0).c_segment(i);
            let (dx, dy) = ((s.b.x - s.a.x).abs(), (s.b.y - s.a.y).abs());
            assert!(dx == Nm::ZERO || dy == Nm::ZERO || dx == dy || (dx - dy).abs() <= Nm::new(2), "segment {i} {s:?} is not on a 45-degree heading");
            diagonals += (dx != Nm::ZERO && dy != Nm::ZERO) as usize;
        }
        assert_eq!(diagonals, 4);
    }

    #[test]
    fn the_amplitude_steps_down_to_what_the_room_allows_and_no_lower_than_the_minimum() {
        // a placer that only lets a shape through that stays within 0.4 mm of the baseline
        struct Narrow(Free);
        impl MeanderEnv for Narrow {
            fn settings(&self) -> &MeanderSettings {
                self.0.settings()
            }
            fn update_settings(&mut self, s: MeanderSettings) {
                self.0.update_settings(s)
            }
            fn clearance(&self) -> i64 {
                self.0.clearance()
            }
            fn check_fit(&mut self, shape: &MeanderShape, _: &[MeanderShape]) -> bool {
                shape.c_line(0).points().iter().all(|q| q.y.abs() <= mm(0.401))
            }
        }
        let mut env = Narrow(free());
        let mut m = MeanderShape::new(&env, 200_000, false);
        assert!(m.fit(&mut env, &[], MeanderType::Single, Seg::new(p(0, 0), p(20_000_000, 0)), p(0, 0), false));
        assert_eq!(m.amplitude(), 400_000, "1 mm, 0.95, ... down to the first step that fits");
        let mut env = Narrow(free());
        env.0.settings.min_amplitude = 500_000;
        let mut m = MeanderShape::new(&env, 200_000, false);
        assert!(!m.fit(&mut env, &[], MeanderType::Single, Seg::new(p(0, 0), p(20_000_000, 0)), p(0, 0), false), "nothing fits above the minimum");
    }

    #[test]
    fn a_line_is_meandered_from_one_end_to_the_other_on_alternating_sides() {
        let mut env = free();
        let mut line = MeanderedLine::new(false);
        line.set_width(200_000);
        let base = Seg::new(p(0, 0), p(30_000_000, 0));
        line.meander_segment(&mut env, base, false, 0);
        let kinds: Vec<MeanderType> = line.meanders().iter().map(|m| m.kind()).collect();
        assert_eq!(kinds[0], MeanderType::Corner);
        assert_eq!(*kinds.last().unwrap(), MeanderType::Corner);
        assert!(kinds.iter().filter(|&&k| k == MeanderType::Start).count() == 1 && kinds.contains(&MeanderType::Turn) && kinds.contains(&MeanderType::Finish), "{kinds:?}");
        // the shapes follow each other along the baseline without overlapping
        let mut at = 0;
        for m in line.meanders() {
            if matches!(m.kind(), MeanderType::Corner) {
                continue;
            }
            assert!(m.base_segment().a.x >= Nm::from_raw_shim(at), "{:?} starts before the last one ended at {at}", m.base_segment());
            at = m.base_segment().b.x.raw_shim();
        }
        assert!(at <= 30_000_000);
        // it is a clear line: no two meanders of it come closer than the spacing
        let mut chain = ShapeLineChain::new();
        for m in line.meanders() {
            chain.append_chain(m.c_line(0));
        }
        assert!(chain.length() > 30_000_000 + 10_000_000, "{}", chain.length());
    }

    #[test]
    fn single_sided_keeps_every_meander_on_one_side() {
        let mut env = free();
        env.settings.single_sided = true;
        let mut line = MeanderedLine::new(false);
        line.set_width(200_000);
        line.meander_segment(&mut env, Seg::new(p(0, 0), p(30_000_000, 0)), false, 0);
        let mut ys = Vec::new();
        for m in line.meanders() {
            ys.extend(m.c_line(0).points().iter().map(|q| q.y));
        }
        assert!(ys.iter().all(|&y| y >= Nm::ZERO), "all on the left");
        assert!(line.meanders().iter().filter(|m| m.kind() == MeanderType::Single).count() > 5);
    }

    fn tuned_line(env: &mut Free, length: i64, elongation: i64) -> (MeanderedLine, ShapeLineChain) {
        let mut line = MeanderedLine::new(false);
        line.set_width(200_000);
        line.meander_segment(env, Seg::new(p(0, 0), p(length, 0)), false, 0);
        tune_line_length(&mut line, elongation);
        let mut chain = ShapeLineChain::new();
        for m in line.meanders() {
            if m.kind() != MeanderType::Empty {
                chain.append_chain(m.c_line(0));
            }
        }
        (line, chain)
    }

    #[test]
    fn tuning_a_line_lands_within_the_tolerance_of_the_length_asked() {
        for (length, elongation) in [(30_000_000, 3_000_000), (30_000_000, 12_000_000), (20_000_000, 1_500_000), (50_000_000, 25_000_000)] {
            let mut env = free();
            let (_, chain) = tuned_line(&mut env, length, elongation);
            let got = chain.length() - length;
            assert!((got - elongation).abs() < 2_000, "asked {elongation} of elongation on {length}, got {got}");
        }
    }

    #[test]
    fn an_elongation_too_small_for_the_first_meander_leaves_the_line_straight() {
        let mut env = free();
        let (line, chain) = tuned_line(&mut env, 30_000_000, 100);
        assert!(line.meanders().iter().all(|m| matches!(m.kind(), MeanderType::Corner | MeanderType::Empty)));
        assert_eq!(chain.length(), 30_000_000);
    }

    #[test]
    fn an_elongation_more_than_the_room_gives_stays_at_the_longest_the_room_allows() {
        let mut env = free();
        let (_, chain) = tuned_line(&mut env, 10_000_000, 80_000_000);
        let gain = chain.length() - 10_000_000;
        assert!(gain > 5_000_000 && gain < 80_000_000, "{gain}");
    }
}
