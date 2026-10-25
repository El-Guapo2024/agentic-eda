//! `PNS::MEANDER_PLACER` (`pns_meander_placer.cpp`) and `PNS::MEANDER_SKEW_PLACER` (`pns_meander_skew_placer.cpp`): the single-track length
//! tuner and its skew sibling. A placer is started on a track of the world with a point on it, moved to where the pattern ends, and gives the
//! pieces the line is made of afterwards: what comes before the pattern, the meander, what comes after (the studio commits them as tracks).
//!
//! * `Start`: the line the clicked segment is part of is assembled (`AssembleLine`), the tuning path it belongs to (`AssembleTuningPath`) gives the
//!   length that is tuned, and the line is taken out of the world the shapes have to fit in.
//! * `Move` (`doMove`): the line is cut at the start and at the point asked for, a meander is fitted along each segment between them
//!   (`MeanderSegment`), the amplitudes are adjusted until the net is as long as the target (`tuneLineLength`), and the status is whether that
//!   length is inside the target's range.
//! * `CheckFit`: a shape fits when it collides with nothing in the world (the line being tuned is not in it) and keeps `width + spacing` from
//!   the meanders already fitted.
//!
//! Skew tuning is the same placer lengthening the clicked line until it is the partner net's length plus the target skew
//! (`doMove( aP, aEndItem, m_coupledLength + targetSkew.Opt(), .. )`).
//!
//! The time domain (`m_isTimeDomain`): the target is a delay, and `calculateTimeDomainTargets` turns it into the length it comes to on the layer being
//! tuned, from the delay the tuning path has now ([`crate::delay`]: the tuning profile of the net's class); the placer judges and tunes the length, and
//! reports the delay (`TuningDelayResult`).
//!
//! Differences from KiCad: the pad-to-die length and delay are not modelled (the model has no data for them); the line stops where the track width
//! changes (KiCad rewrites the whole of it at the first segment's width); the world is cloned once at `Start` instead of branched at every `Move`, as
//! nothing in `Move` writes to it.

use crate::chain::ShapeLineChain;
use crate::delay::DelayParams;
use crate::item::{Item, ItemId, Net};
use crate::kigeom::ShapeArc;
use crate::kigeom::Seg;
use crate::layer::LayerRange;
use crate::line::Line;
use crate::meander::{tune_line_length, MeanderEnv, MeanderSettings, MeanderShape, MeanderSide, MeanderType, MeanderedLine};
use crate::node::Node;
use crate::tuning_path::{assemble_opts, assemble_tuning_path, LengthEnv, TuningPath};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;

/// `MEANDER_PLACER_BASE::TUNING_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningStatus {
    TooShort,
    TooLong,
    Tuned,
}

impl TuningStatus {
    /// What the pattern's properties and the file call it (`statusToString`).
    pub fn name(self) -> &'static str {
        match self {
            TuningStatus::TooShort => "too_short",
            TuningStatus::TooLong => "too_long",
            TuningStatus::Tuned => "tuned",
        }
    }

    pub fn from_name(s: &str) -> TuningStatus {
        match s {
            "too_long" => TuningStatus::TooLong,
            "too_short" => TuningStatus::TooShort,
            _ => TuningStatus::Tuned,
        }
    }
}

/// Does the polyline `chain` of a track of width `width_nm` collide with the world? The shape's chords are tested as the stadiums
/// they are.
pub fn chain_collides(world: &Node, rules: &BoardRules, net: &Net, layer: i32, width_nm: Nm, chain: &ShapeLineChain) -> bool {
    let layers = LayerRange::single(layer);
    let pts: Vec<Point> = chain.points().iter().map(|&p| p).collect();
    pts.windows(2).any(|w| w[0] != w[1] && world.first_colliding(&Shape::Stadium { a: w[0], b: w[1], r: width_nm / 2 }, net, layers, rules, &[]).is_some())
}

/// What the shapes ask their placer for a single line: the settings, the clearance, and `CheckFit`.
struct FitEnv<'a> {
    settings: MeanderSettings,
    clearance: Nm,
    world: &'a Node,
    rules: &'a BoardRules,
    net: Net,
    layer: i32,
    width_nm: Nm,
}

impl MeanderEnv for FitEnv<'_> {
    fn settings(&self) -> &MeanderSettings {
        &self.settings
    }

    fn update_settings(&mut self, s: MeanderSettings) {
        self.settings = s;
    }

    fn clearance(&self) -> Nm {
        self.clearance
    }

    /// `MEANDER_PLACER::CheckFit`.
    fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool {
        if chain_collides(self.world, self.rules, &self.net, self.layer, self.width_nm, shape.c_line(0)) {
            return false;
        }
        let w = shape.width();
        let clearance = w + Nm::new(self.settings.spacing);
        MeanderedLine::check_self_intersections(existing, shape, clearance)
    }
}

/// What a skew placer adds: the partner net's tuning path length (`m_coupledLength`) and delay (`m_coupledDelay`) and the lengths it is judged by.
#[derive(Debug, Clone)]
struct Skew {
    coupled_length: Nm,
    coupled_delay: i64,
}

/// `MEANDER_PLACER` / `MEANDER_SKEW_PLACER`.
pub struct MeanderPlacer {
    /// The world the shapes fit in: the router's with the line being tuned taken out (`m_world->Remove( m_originLine )`).
    world: Node,
    env: LengthEnv,
    initial_segment: ItemId,
    origin_line: Line,
    origin_chain: ShapeLineChain,
    path: TuningPath,
    /// `m_currentStart` (nanometres): where the pattern starts, on the line.
    current_start: Point,
    width_nm: Nm,
    settings: MeanderSettings,
    skew: Option<Skew>,
    /// The length of the tuning path before the tuning (nanometres): `origPathLength()`.
    path_length: Nm,
    /// The delays of the tuning profile of the net's class ([`crate::delay`]), and the delay of the tuning path before the tuning (attoseconds):
    /// `origPathDelay()`.
    delay_params: Option<DelayParams>,
    path_delay: i64,

    last_length: Nm,
    last_delay: i64,
    last_status: TuningStatus,
    pre: ShapeLineChain,
    raw: ShapeLineChain,
    tuned: ShapeLineChain,
    post: ShapeLineChain,
    final_shape: ShapeLineChain,
}

/// The pieces a placer's last move left, for whoever commits them (`crate::tuning_commit`): what comes before the pattern, the stretch the
/// pattern is on as the line had it (`raw`, before it was meandered), the meander, and what comes after. The pieces run from the start of the
/// pattern outwards -- `pre` ends where the pattern starts, `post` starts where it ends -- which is the line's own direction or its reverse
/// (`SHAPE_LINE_CHAIN::Split` turns the chain round when the end of the pattern is before its start).
#[derive(Debug, Clone, Default)]
pub struct Tuned {
    pub pre: ShapeLineChain,
    pub raw: ShapeLineChain,
    pub tuned: ShapeLineChain,
    pub post: ShapeLineChain,
}

impl MeanderPlacer {
    /// `MEANDER_PLACER::Start( aP, aStartItem )`: the tuner on the track segment `start` of `world`, starting at the point of it nearest to
    /// `at`. The error is KiCad's message.
    pub fn start(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings) -> Result<MeanderPlacer, String> {
        let mut placer = Self::start_inner(world, env, start, at, settings)?;
        placer.calculate_time_domain_targets();
        Ok(placer)
    }

    fn start_inner(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings) -> Result<MeanderPlacer, String> {
        // `getSnappedStartPoint`: the point of a segment nearest the click, the nearer end of an arc
        let current_start = match world.get(start) {
            Some(Item::Segment(seg)) => Seg::new(seg.a, seg.b).nearest_point(at),
            Some(Item::Arc(a)) => if crate::seg_ops::length(at, a.start()) <= crate::seg_ops::length(at, a.end()) { a.start() } else { a.end() },
            _ => return Err("Please select a track whose length you want to tune.".into()),
        };
        let origin_line = world.assemble_line_with(start, assemble_opts()).ok_or("Please select a track whose length you want to tune.")?;
        let path = assemble_tuning_path(world, start).ok_or("Please select a track whose length you want to tune.")?;
        let mut w = world.clone();
        w.remove_line_segments(&origin_line);
        let path_length = path.length_nm(world, env);
        let delay_params = origin_line.net.as_deref().and_then(|net| env.delay_params(net));
        let path_delay = delay_params.as_ref().map_or(0, |p| path.delay(world, env, p));
        Ok(MeanderPlacer {
            world: w,
            env: env.clone(),
            initial_segment: start,
            origin_chain: chain_of(&origin_line),
            width_nm: origin_line.width,
            origin_line,
            path,
            current_start,
            settings,
            skew: None,
            path_length,
            delay_params,
            path_delay,
            last_length: Nm::ZERO,
            last_delay: 0,
            last_status: TuningStatus::TooShort,
            pre: ShapeLineChain::new(),
            raw: ShapeLineChain::new(),
            tuned: ShapeLineChain::new(),
            post: ShapeLineChain::new(),
            final_shape: ShapeLineChain::new(),
        })
    }

    /// `MEANDER_SKEW_PLACER::Start`: the same on the line of a pair, judged against the partner net: `coupled_length` is the partner's tuning path
    /// (nanometres, `m_coupledLength`) and `coupled_delay` its delay (attoseconds, `m_coupledDelay`), and the line tuned is the clicked one's.
    /// (`coupled_length` is an `i64` for the studio's `tune_edit` until the length type's wave 3.)
    pub fn start_skew(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings, coupled_length: i64, coupled_delay: i64) -> Result<MeanderPlacer, String> {
        let mut p = MeanderPlacer::start_inner(world, env, start, at, settings)?;
        p.skew = Some(Skew { coupled_length: Nm::new(coupled_length), coupled_delay });
        p.last_length = p.path_length;
        p.last_delay = p.path_delay;
        p.calculate_time_domain_targets();
        Ok(p)
    }

    /// The layer the tuning is on, by name: the one the tuned line is on (`m_router->GetCurrentLayer()`).
    fn layer_name(&self) -> String {
        self.env.rules.layers.get(self.origin_line.layer as usize).cloned().unwrap_or_default()
    }

    /// The delays of the net's tuning profile, or none (a net whose class names no profile has zero delays: KiCad's interface answers 0).
    fn params(&self) -> DelayParams {
        self.delay_params.clone().unwrap_or(DelayParams { enabled: false, track_as_per_mm: Default::default(), via_as_per_mm: 0, via_overrides: Vec::new() })
    }

    /// `MEANDER_PLACER::calculateTimeDomainTargets` / `MEANDER_SKEW_PLACER::calculateTimeDomainTargets`: with the time domain on, the target delay (min, opt,
    /// max) becomes the target length (or skew) it comes to on this layer, counted from what the tuning path has now.
    pub fn calculate_time_domain_targets(&mut self) {
        if !self.settings.is_time_domain {
            return;
        }
        let (params, layer) = (self.params(), self.layer_name());
        if let Some(skew) = &self.skew {
            // `calculateTargetSkew`: the skew now plus the length of the delay still to make up
            let cur_skew = self.last_length - skew.coupled_length;
            let target_skew = |target_delay: i64| {
                let cur_skew_delay = self.last_delay - skew.coupled_delay;
                let difference = target_delay - cur_skew_delay;
                let length = params.length_for_delay(&layer, difference.abs());
                let length = if difference > 0 { length } else { -length };
                // `static_cast<int>`, into the skew's `MINOPTMAX` (a count: nanometres here, attoseconds for a delay)
                (cur_skew + length).get() as i32 as i64
            };
            let d = self.settings.target_skew_delay;
            let (min, opt, max) = (target_skew(d.min()), target_skew(d.opt()), target_skew(d.max()));
            self.settings.target_skew.set_min(min);
            self.settings.target_skew.set_opt(opt);
            self.settings.target_skew.set_max(max);
            return;
        }
        let cur_delay = self.path_delay;
        let d = self.settings.target_length_delay;
        let (desired_min, desired_opt, desired_max) = (d.min(), d.opt(), d.max());
        let difference_opt = desired_opt - cur_delay;
        let cur_length = self.path_length;
        let length_diff_min = params.length_for_delay(&layer, desired_opt - desired_min);
        let mut length_diff_opt = params.length_for_delay(&layer, difference_opt.abs());
        let length_diff_max = params.length_for_delay(&layer, desired_max - desired_opt);
        if difference_opt <= 0 {
            length_diff_opt = -length_diff_opt;
        }
        self.settings.target_length.set_min((cur_length + length_diff_opt - length_diff_min).get());
        self.settings.target_length.set_opt((cur_length + length_diff_opt).get());
        self.settings.target_length.set_max((cur_length + length_diff_opt + length_diff_max).get());
    }

    /// `origPathDelay()`: the delay of the tuning path before the tuning, attoseconds.
    pub fn orig_path_delay(&self) -> i64 {
        self.path_delay
    }

    /// `TuningDelayResult()`: the delay of the net after the last move (the skew as a delay, for a skew placer), attoseconds.
    pub fn tuning_delay_result(&self) -> i64 {
        match &self.skew {
            Some(s) => self.last_delay - s.coupled_delay,
            None => {
                if self.last_delay != 0 {
                    self.last_delay
                } else {
                    self.path_delay
                }
            }
        }
    }

    /// `CalculatePropagationDelayForShapeLineChain`: the delay of a stretch of the tuned layer.
    fn chain_delay(&self, chain: &ShapeLineChain) -> i64 {
        self.params().track_delay(&self.layer_name(), chain.length_nm())
    }

    pub fn settings(&self) -> &MeanderSettings {
        &self.settings
    }

    /// `UpdateSettings`.
    pub fn update_settings(&mut self, s: MeanderSettings) {
        self.settings = s;
    }

    /// `MEANDER_PLACER_BASE::AmplitudeStep`: the maximum amplitude one step more or less, not below the minimum.
    pub fn amplitude_step(&mut self, sign: i64) {
        let a = self.settings.max_amplitude + sign * self.settings.step.get();
        self.settings.max_amplitude = a.max(self.settings.min_amplitude);
    }

    /// `MEANDER_PLACER_BASE::SpacingStep`: the spacing one step more or less, not below the width and clearance.
    pub fn spacing_step(&mut self, sign: i64) {
        let s = Nm::new(self.settings.spacing) + sign * self.settings.step;
        let clearance = Node::clearance(&self.env.rules, &self.origin_line.net, &None);
        self.settings.spacing = s.max(self.width_nm + clearance).get();
    }

    pub fn origin_line(&self) -> &Line {
        &self.origin_line
    }

    pub fn path(&self) -> &TuningPath {
        &self.path
    }

    pub fn world(&self) -> &Node {
        &self.world
    }

    pub fn initial_segment(&self) -> ItemId {
        self.initial_segment
    }

    /// Where the pattern starts (`CurrentStart`).
    pub fn current_start(&self) -> Point {
        self.current_start
    }

    /// `origPathLength()`: the tuning path's length before the tuning, nanometres.
    pub fn orig_path_length(&self) -> Nm {
        self.path_length
    }

    /// `TuningLengthResult()`: the length of the net after the last move (the skew, for a skew placer), nanometres (an `i64` for the
    /// studio's `tune_edit` until the length type's wave 3).
    pub fn tuning_length_result(&self) -> i64 {
        let l = match &self.skew {
            Some(s) => self.last_length - s.coupled_length,
            None => {
                if self.last_length != Nm::ZERO {
                    self.last_length
                } else {
                    self.path_length
                }
            }
        };
        l.get()
    }

    /// The length the net would be without tuning, as the status box starts: for a skew placer, the skew.
    pub fn current_skew(&self) -> Option<Nm> {
        self.skew.as_ref().map(|s| self.last_length - s.coupled_length)
    }

    pub fn tuning_status(&self) -> TuningStatus {
        self.last_status
    }

    pub fn is_skew(&self) -> bool {
        self.skew.is_some()
    }

    /// The target the status is judged against, nanometres: the net's length (`m_targetLength`), or the partner's plus the target skew. A count
    /// of the `MINOPTMAX` it comes from (an `i64`, for the studio's `tune_edit` too).
    pub fn target(&self) -> (i64, i64, i64) {
        let c = |s: &Skew| s.coupled_length.get();
        match &self.skew {
            Some(s) => (c(s) + self.settings.target_skew.min(), c(s) + self.settings.target_skew.opt(), c(s) + self.settings.target_skew.max()),
            None => (self.settings.target_length.min(), self.settings.target_length.opt(), self.settings.target_length.max()),
        }
    }

    /// `Move( aP, aEndItem )`.
    pub fn do_move(&mut self, to: Point) -> bool {
        self.calculate_time_domain_targets();
        let (min, opt, max) = self.target();
        self.do_move_to(to, Nm::new(opt), Nm::new(min), Nm::new(max))
    }

    /// `MEANDER_PLACER::doMove( aP, aEndItem, aTargetLength, aTargetMin, aTargetMax )`.
    pub fn do_move_to(&mut self, to: Point, target_length: Nm, target_min: Nm, target_max: Nm) -> bool {
        let p = to;
        if self.current_start == p {
            return false;
        }
        let (pre, mut tuned, post) = self.origin_chain.split3(self.current_start, p);
        let raw = tuned.clone();

        let clearance = Node::clearance(&self.env.rules, &self.origin_line.net, &None);
        let mut env = FitEnv { settings: self.settings.clone(), clearance, world: &self.world, rules: &self.env.rules, net: self.origin_line.net.clone(), layer: self.origin_line.layer, width_nm: self.origin_line.width };
        let zero = Point { x: Nm::ZERO, y: Nm::ZERO };
        let mut result = MeanderedLine::new(false);
        result.set_width(self.width_nm);
        result.set_baseline_offset(Nm::ZERO);

        // `AddArc` for an arc of the stretch (it is passed through as it is, a meander goes on the straight segments), `NextShape` to its end
        let empty_arc = ShapeArc::new_nm(zero, zero, zero, Nm::ZERO);
        let mut i = 0;
        while i < tuned.segment_count() {
            if tuned.is_arc_segment(i) {
                let arc_index = tuned.arc_index(i);
                result.add_arc(&env, tuned.arc(arc_index as usize), &empty_arc);
                // `i = tuned.NextShape( i )`: the vertex where the arc ends, which is the segment that comes after it. KiCad's `for` loop then
                // increments `i` again and so never meanders (or counts the length of) the segment that follows an arc; here that one is next.
                let next = tuned.next_shape(i as isize);
                i = if next < 0 { tuned.segment_count() } else { next as usize };
                continue;
            }
            let s = tuned.c_segment(i);
            let side = if env.settings.initial_side == MeanderSide::Default { s.side(p) < 0 } else { env.settings.initial_side.value() < 0 };
            result.add_corner(&env, s.a, zero);
            result.meander_segment(&mut env, s, side, 0);
            result.add_corner(&env, s.b, zero);
            i += 1;
        }

        let line_len = self.path_length;
        let line_delay = self.path_delay;
        self.last_length = line_len;
        self.last_delay = line_delay;
        self.last_status = TuningStatus::Tuned;

        if line_len > Nm::new(self.settings.target_length.max()) {
            self.last_status = TuningStatus::TooLong;
        } else {
            self.last_length = line_len - tuned.length_nm();
            if self.settings.is_time_domain {
                self.last_delay = line_delay - self.chain_delay(&tuned);
            }
            tune_line_length(&mut result, target_length - line_len);
        }

        if self.last_status != TuningStatus::TooLong {
            tuned.clear();
            for m in result.meanders() {
                if m.kind() != MeanderType::Empty {
                    tuned.append_chain(m.c_line(0));
                }
            }
            self.last_length += tuned.length_nm();
            if self.settings.is_time_domain {
                self.last_delay += self.chain_delay(&tuned);
            }
            self.last_status = if self.last_length > target_max {
                TuningStatus::TooLong
            } else if self.last_length < target_min {
                TuningStatus::TooShort
            } else {
                TuningStatus::Tuned
            };
        }

        // a flipped initial side is the placer's to remember (`flipInitialSide` -> `UpdateSettings`)
        self.settings = env.settings;

        let mut pre = pre;
        let mut post = post;
        let mut final_shape = ShapeLineChain::new();
        if self.settings.keep_endpoints {
            pre.simplify();
            tuned.simplify();
            post.simplify();
            final_shape.append_chain(&pre);
            final_shape.append_chain(&tuned);
            final_shape.append_chain(&post);
        } else {
            final_shape.append_chain(&pre);
            final_shape.append_chain(&tuned);
            final_shape.append_chain(&post);
            final_shape.simplify();
        }
        self.pre = pre;
        self.raw = raw;
        self.tuned = tuned;
        self.post = post;
        self.final_shape = final_shape;
        true
    }

    /// The pieces of the last move.
    pub fn tuned(&self) -> Tuned {
        Tuned { pre: self.pre.clone(), raw: self.raw.clone(), tuned: self.tuned.clone(), post: self.post.clone() }
    }

    /// `m_finalShape`.
    pub fn final_shape(&self) -> &ShapeLineChain {
        &self.final_shape
    }
}

/// The chain (nanometres) of a world line.
pub fn chain_of(line: &Line) -> ShapeLineChain {
    crate::tuning_path::line_chain_nm(line)
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, um};
    use super::*;
    use crate::item::{net_of, Segment, Solid};
    use crate::tuning_path::LengthEnv;

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn env() -> LengthEnv {
        let mut rules = BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        rules.clearance = mm(0.2);
        LengthEnv { stackup: eda_model::rules::default_stackup(2, mm(1.6)), rules }
    }

    fn pad(net: &str, at: Point, layers: LayerRange) -> Item {
        Item::Solid(Solid { net: net_of(net), layers, pos: at, shape: Shape::Circle { c: at, r: mm(0.4) }, source: "U1.1".into(), edge: false })
    }

    /// Two pads 40 mm apart on net SIG joined by a straight track.
    fn board() -> (Node, ItemId) {
        let mut w = Node::new();
        w.add(pad("SIG", p(0, 0), LayerRange::single(0)));
        w.add(pad("SIG", p(40_000_000, 0), LayerRange::single(0)));
        let s = w.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(40_000_000, 0), width: mm(0.2), source_track: Some(("trk".into(), 0)), locked: false }));
        (w, s)
    }

    fn settings(target_nm: i64) -> MeanderSettings {
        let mut s = MeanderSettings::default();
        s.set_target_length(target_nm);
        s.keep_endpoints = true;
        s
    }

    #[test]
    fn a_track_is_tuned_to_the_length_asked_and_the_status_says_so() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(44_000_000)).expect("a placer");
        assert_eq!(placer.orig_path_length(), mm(40.0));
        assert!(placer.do_move(p(35_000_000, 0)));
        let len = placer.tuning_length_result();
        assert!((len - 44_000_000).abs() < 100_000, "{len}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let t = placer.tuned();
        assert_eq!(t.pre.points(), &[p(0, 0), p(5_000_000, 0)]);
        assert_eq!(t.post.points(), &[p(35_000_000, 0), p(40_000_000, 0)]);
        assert_eq!(t.tuned.c_point(0), p(5_000_000, 0));
        assert_eq!(t.tuned.c_last_point(), p(35_000_000, 0));
        assert!(!t.tuned.arcs().is_empty());
        // the whole line is as long as the net
        assert_eq!(placer.final_shape().length(), len);
    }

    /// pad -- 20 mm straight -- quarter arc of radius 5 mm -- 15 mm straight -- pad, one net.
    fn bent_board() -> (Node, ItemId, ItemId, crate::arcs::ArcPts) {
        let arc = crate::arcs::ArcPts::new(p(20_000_000, 0), p(23_536_000, 1_464_000), p(25_000_000, 5_000_000));
        let mut w = Node::new();
        w.add(pad("SIG", p(0, 0), LayerRange::single(0)));
        w.add(pad("SIG", p(25_000_000, 20_000_000), LayerRange::single(0)));
        let s1 = w.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: arc.start, width: mm(0.2), source_track: Some(("t1".into(), 0)), locked: false }));
        let a = w.add(Item::Arc(crate::item::Arc::new(net_of("SIG"), 0, mm(0.2), arc, Some("t2".into()), false)));
        w.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: arc.end, b: p(25_000_000, 20_000_000), width: mm(0.2), source_track: Some(("t3".into(), 0)), locked: false }));
        (w, s1, a, arc)
    }

    #[test]
    fn a_track_with_an_arc_in_it_is_tuned_along_the_straight_parts_and_the_arc_passes_through_as_it_is() {
        let (w, seg, _, arc) = bent_board();
        let arc_len = arc.length();
        let straight = 20_000_000.0 + 15_000_000.0;
        let original = (straight + arc_len) as i64;
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(3_000_000, 0), settings(((straight + arc_len) as i64) + 4_000_000)).expect("a placer on the first segment");
        assert!((placer.orig_path_length().get() - original).abs() < 50_000, "the arc is measured as an arc: {} vs {original}", placer.orig_path_length());
        assert!(placer.do_move(p(25_000_000, 15_000_000)));
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let t = placer.tuned();
        let arcs: Vec<_> = t.tuned.arcs().iter().filter(|a| a.start == arc.start && a.end == arc.end).collect();
        assert_eq!(arcs.len(), 1, "the arc of the track is in the tuned stretch, as the arc it was");
        assert!((placer.tuning_length_result() - original - 4_000_000).abs() < 100_000, "{}", placer.tuning_length_result());
        // the meanders fill the straight before the arc (rounded corners: arcs of their own) and the stretch still goes on through the arc to where it ended,
        // the straight after the arc counted in the length (KiCad's loop steps over that segment)
        assert!(t.tuned.arcs().iter().filter(|a| a.start != arc.start).count() >= 4);
        assert_eq!(t.tuned.c_last_point(), p(25_000_000, 15_000_000));
        assert!((placer.final_shape().length() - placer.tuning_length_result()).abs() < 1_000, "the shape is as long as the status says, to a micrometre");
    }

    #[test]
    fn a_pattern_can_start_on_the_arc_itself_at_the_end_it_is_nearer_to() {
        let (w, _, arc_id, arc) = bent_board();
        let placer = MeanderPlacer::start(&w, &env(), arc_id, p(24_000_000, 4_000_000), settings(60_000_000)).expect("a placer on the arc");
        assert_eq!(placer.current_start(), arc.end);
        let placer = MeanderPlacer::start(&w, &env(), arc_id, p(20_500_000, 100_000), settings(60_000_000)).unwrap();
        assert_eq!(placer.current_start(), arc.start);
    }

    #[test]
    fn a_target_the_track_cannot_reach_leaves_it_too_short_and_one_it_exceeds_too_long() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(400_000_000)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::TooShort);
        assert!(placer.tuning_length_result() > 40_000_000 && placer.tuning_length_result() < 400_000_000);
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(30_000_000)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::TooLong);
        assert_eq!(placer.tuning_length_result(), 40_000_000, "nothing is added to a net that is already too long");
    }

    #[test]
    fn the_meanders_keep_the_clearance_to_another_net() {
        let (mut w, seg) = board();
        // a GND track 1.2 mm above the line (so a 1 mm meander fits on the other side only)
        w.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(0, 1_200_000), b: p(40_000_000, 1_200_000), width: mm(0.2), source_track: Some(("gnd".into(), 0)), locked: false }));
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(44_000_000)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        let t = placer.tuned().tuned;
        assert!((placer.tuning_length_result() - 44_000_000).abs() < 100_000);
        // nothing on the GND side of the line beyond what fits: every vertex at least (200 + 200 + 100) um from the GND track's axis
        for q in t.points() {
            assert!(mm(1.2) - q.y >= mm(0.2) + mm(0.1) + mm(0.2) - um(1.0), "{q:?} is too close to the GND track");
        }
        assert!(t.points().iter().any(|q| q.y < Nm::ZERO), "the meanders went to the free side");
    }

    #[test]
    fn a_cursor_on_the_start_changes_nothing() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(44_000_000)).unwrap();
        assert!(!placer.do_move(p(5_000_000, 0)));
    }

    #[test]
    fn amplitude_and_spacing_steps_follow_the_settings() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000_000, 0), settings(44_000_000)).unwrap();
        placer.amplitude_step(-1);
        assert_eq!(placer.settings().max_amplitude, 950_000);
        for _ in 0..40 {
            placer.amplitude_step(-1);
        }
        assert_eq!(placer.settings().max_amplitude, 200_000, "never below the minimum");
        placer.spacing_step(1);
        assert_eq!(placer.settings().spacing, 650_000);
        for _ in 0..40 {
            placer.spacing_step(-1);
        }
        assert_eq!(placer.settings().spacing, 200_000 + 200_000, "never below the track width and its clearance");
    }

    /// `env()` with a tuning profile on the Default class: 6.1 ps/mm on F.Cu, 6.5 ps/mm on B.Cu, 5 ps/mm for a via over the stackup it crosses.
    fn profiled_env(time_domain: bool) -> LengthEnv {
        use eda_model::setup_pages::{ProfileLayerEntry, ProfileType, TuningProfile, TuningProfiles};
        let mut e = env();
        let entry = |layer: &str, delay: i64| ProfileLayerEntry { signal_layer: layer.into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: mm(0.2), diff_pair_gap_nm: Nm::ZERO, delay_as_per_mm: delay };
        let profile = TuningProfile { name: "FR4".into(), profile_type: ProfileType::Single, target_impedance: 50.0, enable_time_domain_tuning: time_domain, layer_entries: vec![entry("F.Cu", 6_100_000), entry("B.Cu", 6_500_000)], via_propagation_delay_as: 5_000_000, via_overrides: vec![] };
        e.rules.tuning_profiles = TuningProfiles { profiles: vec![profile], class_profiles: [("Default".to_string(), "FR4".to_string())].into() };
        e
    }

    fn delay_settings(target_ps: i64) -> MeanderSettings {
        let mut s = MeanderSettings::default();
        s.is_time_domain = true;
        s.set_target_length_delay(target_ps * crate::delay::ATTOSECONDS_PER_PS);
        s.keep_endpoints = true;
        s
    }

    /// The time domain: the target is a delay (`m_targetLengthDelay`), `calculateTimeDomainTargets` makes it the length it comes to on this layer from the
    /// delay of the path now, and the tuner tunes to that length and reports the delay.
    #[test]
    fn a_track_is_tuned_to_a_delay_through_the_tuning_profile_of_its_class() {
        let (w, seg) = board();
        let e = profiled_env(true);
        // 40 mm at 6.1 ps/mm is 244 ps; 300 ps is 49.18 mm
        let mut placer = MeanderPlacer::start(&w, &e, seg, p(5_000_000, 0), delay_settings(300)).expect("a placer");
        assert_eq!(placer.orig_path_delay(), 244_000_000);
        let target = placer.settings().target_length.opt();
        assert!((target - 49_180_327).abs() < 5, "300 ps is {target} nm of track on F.Cu");
        assert!(placer.do_move(p(35_000_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let delay = placer.tuning_delay_result();
        assert!((delay - 300_000_000).abs() < 200_000, "the net takes {delay} as");
        assert!((placer.tuning_length_result() - target).abs() < 20_000, "{}", placer.tuning_length_result());
        // a target the path already beats is too long (a delay lower than the net's own): nothing is added
        let mut placer = MeanderPlacer::start(&w, &e, seg, p(5_000_000, 0), delay_settings(100)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::TooLong);
        assert_eq!(placer.tuning_delay_result(), 244_000_000);
    }

    #[test]
    fn without_a_profile_or_with_one_whose_time_domain_is_off_the_delay_targets_come_to_the_length_the_net_has() {
        let (w, seg) = board();
        // KiCad's interface answers 0 for the delays and the lengths of delays: the target length is the path's own
        for e in [env(), profiled_env(false)] {
            let placer = MeanderPlacer::start(&w, &e, seg, p(5_000_000, 0), delay_settings(300)).unwrap();
            assert_eq!(placer.orig_path_delay(), 0);
            assert_eq!((placer.settings().target_length.min(), placer.settings().target_length.opt(), placer.settings().target_length.max()), (40_000_000, 40_000_000, 40_000_000));
        }
    }

    /// `MEANDER_SKEW_PLACER::calculateTimeDomainTargets`: the skew as a delay is made up for in length on this layer.
    #[test]
    fn a_skew_placer_tunes_to_a_skew_in_picoseconds() {
        let (w, seg) = board();
        let e = profiled_env(true);
        // the partner is 43 mm (262.3 ps) long and this line 40 mm (244 ps): a skew of -18.3 ps; to a skew of -5 ps it needs 2.1 mm more
        let mut s = MeanderSettings::default();
        s.keep_endpoints = true;
        s.is_time_domain = true;
        s.set_target_skew_delay(-5 * crate::delay::ATTOSECONDS_PER_PS);
        let mut placer = MeanderPlacer::start_skew(&w, &e, seg, p(5_000_000, 0), s, 43_000_000, 262_300_000).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        let skew = placer.tuning_length_result();
        assert!((skew - (-5_000_000_000_i64 / 6_100_000_i64 * 1000) as i64).abs() < 50_000, "the skew is {skew} nm: -5 ps of 6.1 ps/mm is -0.82 mm");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let skew_delay = placer.tuning_delay_result();
        assert!((skew_delay + 5_000_000).abs() < 300_000, "{skew_delay} as");
    }

    #[test]
    fn a_skew_placer_lengthens_the_line_until_it_is_the_partner_plus_the_target_skew() {
        let (w, seg) = board();
        let mut s = MeanderSettings::default();
        s.keep_endpoints = true;
        s.set_target_skew(0);
        let mut placer = MeanderPlacer::start_skew(&w, &env(), seg, p(5_000_000, 0), s, 43_000_000, 0).unwrap();
        assert_eq!(placer.current_skew(), Some(mm(40.0) - mm(43.0)));
        assert!(placer.do_move(p(35_000_000, 0)));
        let skew = placer.tuning_length_result();
        assert!(skew.abs() < 100_000, "the skew is {skew}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
    }
}
