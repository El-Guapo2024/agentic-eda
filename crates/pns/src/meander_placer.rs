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
//! Differences from KiCad: the world has no arcs (a track that is an arc is a run of chords in it); the time domain and the pad-to-die length are
//! not modelled (the model has no data for them); the line stops where the track width changes (KiCad rewrites the whole of it at the first
//! segment's width); the world is cloned once at `Start` instead of branched at every `Move`, as nothing in `Move` writes to it.

use crate::chain::ShapeLineChain;
use crate::item::{Item, ItemId, Net};
use crate::kigeom::{to_nm, to_um, Seg};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::meander::{tune_line_length, MeanderEnv, MeanderSettings, MeanderShape, MeanderSide, MeanderType, MeanderedLine};
use crate::node::Node;
use crate::tuning_path::{assemble_opts, assemble_tuning_path, LengthEnv, TuningPath};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
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

/// Does the polyline `chain` (nanometres) of a track of width `width_um` collide with the world? The shape's chords are tested as the stadiums
/// they are, on micrometres: the grid the board is on.
pub fn chain_collides(world: &Node, rules: &BoardRules, net: &Net, layer: i32, width_um: Um, chain: &ShapeLineChain) -> bool {
    let layers = LayerRange::single(layer);
    let pts: Vec<Point> = chain.points().iter().map(|&p| to_um(p)).collect();
    pts.windows(2).any(|w| w[0] != w[1] && world.first_colliding(&Shape::Stadium { a: w[0], b: w[1], r: width_um / 2 }, net, layers, rules, &[]).is_some())
}

/// What the shapes ask their placer for a single line: the settings, the clearance, and `CheckFit`.
struct FitEnv<'a> {
    settings: MeanderSettings,
    clearance: i64,
    world: &'a Node,
    rules: &'a BoardRules,
    net: Net,
    layer: i32,
    width_um: Um,
}

impl MeanderEnv for FitEnv<'_> {
    fn settings(&self) -> &MeanderSettings {
        &self.settings
    }

    fn update_settings(&mut self, s: MeanderSettings) {
        self.settings = s;
    }

    fn clearance(&self) -> i64 {
        self.clearance
    }

    /// `MEANDER_PLACER::CheckFit`.
    fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool {
        if chain_collides(self.world, self.rules, &self.net, self.layer, self.width_um, shape.c_line(0)) {
            return false;
        }
        let w = shape.width();
        let clearance = w + self.settings.spacing;
        MeanderedLine::check_self_intersections(existing, shape, clearance)
    }
}

/// What a skew placer adds: the partner net's tuning path length (`m_coupledLength`) and the lengths it is judged by.
#[derive(Debug, Clone)]
struct Skew {
    coupled_length: i64,
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
    width_nm: i64,
    settings: MeanderSettings,
    skew: Option<Skew>,
    /// The length of the tuning path before the tuning (nanometres): `origPathLength()`.
    path_length: i64,

    last_length: i64,
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
    /// `at` (micrometres). The error is KiCad's message.
    pub fn start(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings) -> Result<MeanderPlacer, String> {
        let Some(Item::Segment(seg)) = world.get(start) else { return Err("Please select a track whose length you want to tune.".into()) };
        let current_start = Seg::new(to_nm(seg.a), to_nm(seg.b)).nearest_point(to_nm(at));
        let origin_line = world.assemble_line_with(start, assemble_opts()).ok_or("Please select a track whose length you want to tune.")?;
        let path = assemble_tuning_path(world, start).ok_or("Please select a track whose length you want to tune.")?;
        let mut w = world.clone();
        w.remove_line_segments(&origin_line);
        let path_length = path.length(world, env);
        Ok(MeanderPlacer {
            world: w,
            env: env.clone(),
            initial_segment: start,
            origin_chain: chain_of(&origin_line),
            width_nm: origin_line.width * 1000,
            origin_line,
            path,
            current_start,
            settings,
            skew: None,
            path_length,
            last_length: 0,
            last_status: TuningStatus::TooShort,
            pre: ShapeLineChain::new(),
            raw: ShapeLineChain::new(),
            tuned: ShapeLineChain::new(),
            post: ShapeLineChain::new(),
            final_shape: ShapeLineChain::new(),
        })
    }

    /// `MEANDER_SKEW_PLACER::Start`: the same on the line of a pair, judged against the partner net: `coupled_length` is the partner's tuning path
    /// (nanometres, `m_coupledLength`), and the line tuned is the clicked one's.
    pub fn start_skew(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings, coupled_length: i64) -> Result<MeanderPlacer, String> {
        let mut p = MeanderPlacer::start(world, env, start, at, settings)?;
        p.skew = Some(Skew { coupled_length });
        p.last_length = p.path_length;
        Ok(p)
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
        let a = self.settings.max_amplitude + sign * self.settings.step;
        self.settings.max_amplitude = a.max(self.settings.min_amplitude);
    }

    /// `MEANDER_PLACER_BASE::SpacingStep`: the spacing one step more or less, not below the width and clearance.
    pub fn spacing_step(&mut self, sign: i64) {
        let s = self.settings.spacing + sign * self.settings.step;
        let clearance = Node::clearance(&self.env.rules, &self.origin_line.net, &None) * 1000;
        self.settings.spacing = s.max(self.width_nm + clearance);
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

    /// Where the pattern starts (`CurrentStart`), micrometres.
    pub fn current_start(&self) -> Point {
        to_um(self.current_start)
    }

    /// `origPathLength()`: the tuning path's length before the tuning, nanometres.
    pub fn orig_path_length(&self) -> i64 {
        self.path_length
    }

    /// `TuningLengthResult()`: the length of the net after the last move (the skew, for a skew placer), nanometres.
    pub fn tuning_length_result(&self) -> i64 {
        match &self.skew {
            Some(s) => self.last_length - s.coupled_length,
            None => {
                if self.last_length != 0 {
                    self.last_length
                } else {
                    self.path_length
                }
            }
        }
    }

    /// The length the net would be without tuning, as the status box starts: for a skew placer, the skew.
    pub fn current_skew(&self) -> Option<i64> {
        self.skew.as_ref().map(|s| self.last_length - s.coupled_length)
    }

    pub fn tuning_status(&self) -> TuningStatus {
        self.last_status
    }

    pub fn is_skew(&self) -> bool {
        self.skew.is_some()
    }

    /// The target the status is judged against, nanometres: the net's length (`m_targetLength`), or the partner's plus the target skew.
    pub fn target(&self) -> (i64, i64, i64) {
        match &self.skew {
            Some(s) => (s.coupled_length + self.settings.target_skew.min(), s.coupled_length + self.settings.target_skew.opt(), s.coupled_length + self.settings.target_skew.max()),
            None => (self.settings.target_length.min(), self.settings.target_length.opt(), self.settings.target_length.max()),
        }
    }

    /// `Move( aP, aEndItem )`.
    pub fn do_move(&mut self, to: Point) -> bool {
        let (min, opt, max) = self.target();
        self.do_move_to(to, opt, min, max)
    }

    /// `MEANDER_PLACER::doMove( aP, aEndItem, aTargetLength, aTargetMin, aTargetMax )`.
    pub fn do_move_to(&mut self, to: Point, target_length: i64, target_min: i64, target_max: i64) -> bool {
        let p = to_nm(to);
        if self.current_start == p {
            return false;
        }
        let (pre, mut tuned, post) = self.origin_chain.split3(self.current_start, p);
        let raw = tuned.clone();

        let clearance = Node::clearance(&self.env.rules, &self.origin_line.net, &None) * 1000;
        let mut env = FitEnv { settings: self.settings.clone(), clearance, world: &self.world, rules: &self.env.rules, net: self.origin_line.net.clone(), layer: self.origin_line.layer, width_um: self.origin_line.width };
        let zero = Point { x: 0, y: 0 };
        let mut result = MeanderedLine::new(false);
        result.set_width(self.width_nm);
        result.set_baseline_offset(0);

        for i in 0..tuned.segment_count() {
            if tuned.is_arc_segment(i) {
                continue; // the world has no arcs: a track that is one is a run of chords, none of which is an arc segment here
            }
            let s = tuned.c_segment(i);
            let side = if env.settings.initial_side == MeanderSide::Default { s.side(p) < 0 } else { env.settings.initial_side.value() < 0 };
            result.add_corner(&env, s.a, zero);
            result.meander_segment(&mut env, s, side, 0);
            result.add_corner(&env, s.b, zero);
        }

        let line_len = self.path_length;
        self.last_length = line_len;
        self.last_status = TuningStatus::Tuned;

        if line_len > self.settings.target_length.max() {
            self.last_status = TuningStatus::TooLong;
        } else {
            self.last_length = line_len - tuned.length();
            tune_line_length(&mut result, target_length - line_len);
        }

        if self.last_status != TuningStatus::TooLong {
            tuned.clear();
            for m in result.meanders() {
                if m.kind() != MeanderType::Empty {
                    tuned.append_chain(m.c_line(0));
                }
            }
            self.last_length += tuned.length();
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
    let mut c = ShapeLineChain::new();
    for &p in &line.pts {
        c.append_point(to_nm(p));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Solid};
    use crate::tuning_path::LengthEnv;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn env() -> LengthEnv {
        let mut rules = BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        rules.clearance = 200;
        LengthEnv { stackup: eda_model::rules::default_stackup(2, 1_600), rules }
    }

    fn pad(net: &str, at: Point, layers: LayerRange) -> Item {
        Item::Solid(Solid { net: net_of(net), layers, pos: at, shape: Shape::Circle { c: at, r: 400 }, source: "U1.1".into(), edge: false })
    }

    /// Two pads 40 mm apart on net SIG joined by a straight track.
    fn board() -> (Node, ItemId) {
        let mut w = Node::new();
        w.add(pad("SIG", p(0, 0), LayerRange::single(0)));
        w.add(pad("SIG", p(40_000, 0), LayerRange::single(0)));
        let s = w.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(40_000, 0), width: 200, source_track: Some(("trk".into(), 0)), locked: false }));
        (w, s)
    }

    fn settings(target_um: i64) -> MeanderSettings {
        let mut s = MeanderSettings::default();
        s.set_target_length(target_um * 1000);
        s.keep_endpoints = true;
        s
    }

    #[test]
    fn a_track_is_tuned_to_the_length_asked_and_the_status_says_so() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(44_000)).expect("a placer");
        assert_eq!(placer.orig_path_length(), 40_000_000);
        assert!(placer.do_move(p(35_000, 0)));
        let len = placer.tuning_length_result();
        assert!((len - 44_000_000).abs() < 100_000, "{len}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let t = placer.tuned();
        assert_eq!(t.pre.points(), &[to_nm(p(0, 0)), to_nm(p(5_000, 0))]);
        assert_eq!(t.post.points(), &[to_nm(p(35_000, 0)), to_nm(p(40_000, 0))]);
        assert_eq!(t.tuned.c_point(0), to_nm(p(5_000, 0)));
        assert_eq!(t.tuned.c_last_point(), to_nm(p(35_000, 0)));
        assert!(!t.tuned.arcs().is_empty());
        // the whole line is as long as the net
        assert_eq!(placer.final_shape().length(), len);
    }

    #[test]
    fn a_target_the_track_cannot_reach_leaves_it_too_short_and_one_it_exceeds_too_long() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(400_000)).unwrap();
        assert!(placer.do_move(p(35_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::TooShort);
        assert!(placer.tuning_length_result() > 40_000_000 && placer.tuning_length_result() < 400_000_000);
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(30_000)).unwrap();
        assert!(placer.do_move(p(35_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::TooLong);
        assert_eq!(placer.tuning_length_result(), 40_000_000, "nothing is added to a net that is already too long");
    }

    #[test]
    fn the_meanders_keep_the_clearance_to_another_net() {
        let (mut w, seg) = board();
        // a GND track 1.2 mm above the line (so a 1 mm meander fits on the other side only)
        w.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(0, 1_200), b: p(40_000, 1_200), width: 200, source_track: Some(("gnd".into(), 0)), locked: false }));
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(44_000)).unwrap();
        assert!(placer.do_move(p(35_000, 0)));
        let t = placer.tuned().tuned;
        assert!((placer.tuning_length_result() - 44_000_000).abs() < 100_000);
        // nothing on the GND side of the line beyond what fits: every vertex at least (200 + 200 + 100) um from the GND track's axis
        for q in t.points() {
            assert!(1_200_000 - q.y >= 200_000 + 100_000 + 200_000 - 1_000, "{q:?} is too close to the GND track");
        }
        assert!(t.points().iter().any(|q| q.y < 0), "the meanders went to the free side");
    }

    #[test]
    fn a_cursor_on_the_start_changes_nothing() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(44_000)).unwrap();
        assert!(!placer.do_move(p(5_000, 0)));
    }

    #[test]
    fn amplitude_and_spacing_steps_follow_the_settings() {
        let (w, seg) = board();
        let mut placer = MeanderPlacer::start(&w, &env(), seg, p(5_000, 0), settings(44_000)).unwrap();
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

    #[test]
    fn a_skew_placer_lengthens_the_line_until_it_is_the_partner_plus_the_target_skew() {
        let (w, seg) = board();
        let mut s = MeanderSettings::default();
        s.keep_endpoints = true;
        s.set_target_skew(0);
        let mut placer = MeanderPlacer::start_skew(&w, &env(), seg, p(5_000, 0), s, 43_000_000).unwrap();
        assert_eq!(placer.current_skew(), Some(40_000_000 - 43_000_000));
        assert!(placer.do_move(p(35_000, 0)));
        let skew = placer.tuning_length_result();
        assert!(skew.abs() < 100_000, "the skew is {skew}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
    }
}
