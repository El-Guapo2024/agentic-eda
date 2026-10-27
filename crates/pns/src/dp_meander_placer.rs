//! `PNS::DP_MEANDER_PLACER` (`pns_dp_meander_placer.cpp`): length tuning of a differential pair, and `TOPOLOGY::AssembleDiffPair` (the pair a
//! clicked track belongs to).
//!
//! The pair's two lines are cut at the start and at the point asked for. Where a segment of P and a segment of N run side by side at the pair's
//! gap (`DIFF_PAIR::CoupledSegmentPairs`), one meander is fitted along the midline of the two ([`baseline_segment`]) as a *dual* shape: each of its
//! two lines is that shape shifted sideways by half the pair's pitch (`(gap + width) / 2`, [`MeanderShape`]'s baseline offset), the corners
//! of the lines left outside the coupled stretches being copied. The pair is as long as its longer net (`origPathLength`), and the amplitudes are
//! adjusted until that length is the target (`tuneLineLength`).
//!
//! `CheckFit` for a pair: each line collides with nothing in the world (the pair being tuned is not in it), and the shape keeps four widths from
//! the shapes before it.
//!
//! Differences from KiCad as in [`crate::meander_placer`]: no pad-to-die length, the lines stop where the track width changes.

use crate::chain::ShapeLineChain;
use crate::delay::DelayParams;
use crate::diff_pair::{dp_coupled_net_name, dp_net_polarity};
use crate::item::{net_of, Item, ItemId, Net};
use crate::kigeom::{common_parallel_projection, ki_round_nm, Seg};
use crate::line::Line;
use crate::meander::{tune_line_length, MeanderEnv, MeanderSettings, MeanderShape, MeanderSide, MeanderType, MeanderedLine};
use crate::meander_placer::{chain_collides, chain_of, Tuned, TuningStatus};
use crate::node::Node;
use crate::tuning_path::{assemble_opts, assemble_tuning_path, LengthEnv, TuningPath};
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;

/// `DP_PARALLELITY_THRESHOLD` (`pns_topology.h`), nanometres.
const DP_PARALLELITY_THRESHOLD: Nm = Nm::new(5);
/// `RANGED_NUM<int>( m_gap, 10000, 10000 )`: how far from the pair's gap a coupled segment may be, nanometres.
const GAP_TOLERANCE: Nm = Nm::new(10_000);

/// A pair as `TOPOLOGY::AssembleDiffPair` finds it: the two lines, P and N, and the width and gap they were routed with.
#[derive(Debug, Clone)]
pub struct PairAssembly {
    pub p: Line,
    pub n: Line,
    pub net_p: Net,
    pub net_n: Net,
    /// Track width of the pair.
    pub width: Nm,
    /// Edge-to-edge gap (-1 nm, KiCad's `-1`: the two segments are not parallel enough to tell).
    pub gap: Nm,
}

/// `SEG::SquaredDistance` from a point to the stadium of a segment of `width` (what `Shape( -1 )->SquaredDistance( p )` of a segment is, up to the
/// polygonisation of the stadium's ends).
fn stadium_sq_distance(seg: &Seg, p: Point, width_nm: Nm) -> i128 {
    let d = Nm::new(crate::kigeom::isqrt(seg.squared_distance(p))) - width_nm / 2;
    let d = d.max(Nm::ZERO);
    d.area(d)
}

/// The best pair of a segment of the clicked line and a segment of the partner net `findNItem` has found so far.
struct BestPair {
    dist_sq: i128,
    dist_target_sq: i128,
    found: Option<(ItemId, ItemId)>,
}

/// `TOPOLOGY::AssembleDiffPair( aStart, aPair )`.
pub fn assemble_diff_pair(world: &Node, start: ItemId) -> Option<PairAssembly> {
    let Some(Item::Segment(start_seg)) = world.get(start) else { return None };
    let ref_net = start_seg.net.clone();
    let ref_name = ref_net.as_deref()?.to_string();
    let coupled_name = dp_coupled_net_name(&ref_name)?;
    let coupled_net = net_of(&coupled_name);
    let lp = world.assemble_line_with(start, assemble_opts())?;

    let layer = start_seg.layer;
    let mut n_items: Vec<ItemId> = world.iter().filter(|(_, it)| matches!(it, Item::Segment(s) if s.net == coupled_net && s.layer == layer)).map(|(id, _)| id).collect();
    n_items.sort_unstable();

    let seg_nm = |id: ItemId| -> Option<(Seg, Nm)> {
        match world.get(id) {
            Some(Item::Segment(s)) => Some((Seg::new(s.a, s.b), s.width)),
            _ => None,
        }
    };
    let (start_nm, _) = seg_nm(start)?;
    // `Shape( -1 )->Centre()`
    let center = Point { x: start_nm.a.x + ki_round_nm((start_nm.b.x - start_nm.a.x).as_f64() / 2.0), y: start_nm.a.y + ki_round_nm((start_nm.b.y - start_nm.a.y).as_f64() / 2.0) };

    // `findNItem( p_item )`
    let find_n_item = |best: &mut BestPair, p_item: ItemId| {
        let Some((p_seg, p_width)) = seg_nm(p_item) else { return };
        for &n_item in &n_items {
            let Some((n_seg, n_width)) = seg_nm(n_item) else { continue };
            if n_width != p_width || !p_seg.approx_parallel(&n_seg, DP_PARALLELITY_THRESHOLD) || common_parallel_projection(p_seg, n_seg).is_none() {
                continue;
            }
            let dist_sq = n_seg.squared_distance_seg(&p_seg);
            if dist_sq <= best.dist_sq {
                let dist_target_sq = stadium_sq_distance(&n_seg, center, n_width);
                if dist_target_sq < best.dist_target_sq {
                    best.dist_target_sq = dist_target_sq;
                    best.dist_sq = dist_sq;
                    best.found = Some((p_item, n_item));
                }
            }
        }
    };
    let mut best = BestPair { dist_sq: i128::MAX, dist_target_sq: i128::MAX, found: None };
    find_n_item(&mut best, start);
    if best.found.is_none() {
        // the other tracks that end where the clicked one does
        let mut links: Vec<ItemId> = Vec::new();
        for anchor in [start_seg.a, start_seg.b] {
            if let Some(j) = world.joint_at(anchor, &ref_net) {
                for &l in &j.links {
                    if l != start && !links.contains(&l) {
                        links.push(l);
                    }
                }
            }
        }
        links.sort_unstable();
        for l in links {
            if matches!(world.get(l), Some(Item::Segment(_))) {
                find_n_item(&mut best, l);
            }
        }
    }
    let (ref_item, coupled_item) = best.found?;
    let ln = world.assemble_line_with(coupled_item, assemble_opts())?;

    let (mut lp, mut ln) = (lp, ln);
    if dp_net_polarity(&ref_name) < 0 {
        std::mem::swap(&mut lp, &mut ln);
    }
    // "Segments are parallel -> compute pair gap"
    let gap = match (seg_nm(ref_item), seg_nm(coupled_item)) {
        (Some((r, _)), Some((c, _))) => {
            let dir = (r.b.x - r.a.x, r.b.y - r.a.y);
            let disp = (r.b.x - c.b.x, r.b.y - c.b.y);
            let cross = dir.0.area(disp.1) - dir.1.area(disp.0);
            let norm = crate::kigeom::norm_i(dir.0, dir.1).max(Nm::new(1)).get() as i128;
            Nm::new((cross.abs() / norm) as i64) - lp.width
        }
        _ => Nm::new(-1),
    };
    Some(PairAssembly { net_p: lp.net.clone(), net_n: ln.net.clone(), width: lp.width, gap, p: lp, n: ln })
}

/// `DIFF_PAIR::COUPLED_SEGMENTS`: where a segment of P and one of N run side by side at the gap (the parts that face each other, and the
/// indices of the two segments in their lines).
#[derive(Debug, Clone)]
pub struct CoupledSegments {
    pub coupled_p: Seg,
    pub coupled_n: Seg,
    pub index_p: usize,
    pub index_n: usize,
}

/// `DIFF_PAIR::CoupledSegmentPairs`, nanometres: `width` is the pair's track width, `gap` its gap.
pub fn coupled_segment_pairs(p: &ShapeLineChain, n: &ShapeLineChain, width: Nm, gap: Nm) -> Vec<CoupledSegments> {
    let mut out = Vec::new();
    for i in 0..p.segment_count() {
        if p.is_arc_segment(i) {
            continue;
        }
        for j in 0..n.segment_count() {
            if n.is_arc_segment(j) {
                continue;
            }
            let (sp, sn) = (p.c_segment(i), n.c_segment(j));
            let dist = (sp.distance_seg(&sn) - width).abs();
            if sp.approx_parallel(&sn, Nm::new(2)) && dist >= gap - GAP_TOLERANCE && dist <= gap + GAP_TOLERANCE {
                if let Some((coupled_p, coupled_n)) = common_parallel_projection(sp, sn) {
                    out.push(CoupledSegments { coupled_p, coupled_n, index_p: i, index_n: j });
                }
            }
        }
    }
    out
}

/// `baselineSegment`: the midline of a coupled pair of segments.
pub fn baseline_segment(c: &CoupledSegments) -> Seg {
    let mid = |a: Point, b: Point| Point { x: ki_round_nm((a.x + b.x).as_f64() / 2.0), y: ki_round_nm((a.y + b.y).as_f64() / 2.0) };
    Seg::new(mid(c.coupled_p.a, c.coupled_n.a), mid(c.coupled_p.b, c.coupled_n.b))
}

/// `pairOrientation`: is the midpoint of the pair on the positive side of P's segment?
pub fn pair_orientation(c: &CoupledSegments) -> bool {
    let midp = Point { x: ki_round_nm((c.coupled_p.a.x + c.coupled_n.a.x).as_f64() / 2.0), y: ki_round_nm((c.coupled_p.a.y + c.coupled_n.a.y).as_f64() / 2.0) };
    c.coupled_p.side(midp) > 0
}

/// What the shapes ask their placer for a pair.
struct FitEnv<'a> {
    settings: MeanderSettings,
    clearance: Nm,
    world: &'a Node,
    rules: &'a BoardRules,
    net_p: Net,
    net_n: Net,
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

    /// `DP_MEANDER_PLACER::CheckFit`.
    fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool {
        if chain_collides(self.world, self.rules, &self.net_p, self.layer, self.width_nm, shape.c_line(0)) {
            return false;
        }
        if chain_collides(self.world, self.rules, &self.net_n, self.layer, self.width_nm, shape.c_line(1)) {
            return false;
        }
        let w = shape.width();
        MeanderedLine::check_self_intersections(existing, shape, w + w * 3)
    }
}

/// `DP_MEANDER_PLACER`.
pub struct DpMeanderPlacer {
    world: Node,
    env: LengthEnv,
    initial_segment: ItemId,
    pair: PairAssembly,
    cp: ShapeLineChain,
    cn: ShapeLineChain,
    path_p: TuningPath,
    path_n: TuningPath,
    len_p: Nm,
    len_n: Nm,
    /// The delays of the two nets' tuning paths before the tuning (attoseconds), and the tuning profile of their class ([`crate::delay`]).
    delay_p: i64,
    delay_n: i64,
    delay_params: Option<DelayParams>,
    current_start: Point,
    width_nm: Nm,
    settings: MeanderSettings,

    last_length: Nm,
    last_delay: i64,
    last_status: TuningStatus,
    pre_p: ShapeLineChain,
    raw_p: ShapeLineChain,
    tuned_p: ShapeLineChain,
    post_p: ShapeLineChain,
    pre_n: ShapeLineChain,
    raw_n: ShapeLineChain,
    tuned_n: ShapeLineChain,
    post_n: ShapeLineChain,
    final_p: ShapeLineChain,
    final_n: ShapeLineChain,
}

/// The pieces of the last move of a pair: each line's, as [`Tuned`] has them for a single one.
#[derive(Debug, Clone, Default)]
pub struct TunedPair {
    pub pre_p: ShapeLineChain,
    pub raw_p: ShapeLineChain,
    pub tuned_p: ShapeLineChain,
    pub post_p: ShapeLineChain,
    pub pre_n: ShapeLineChain,
    pub raw_n: ShapeLineChain,
    pub tuned_n: ShapeLineChain,
    pub post_n: ShapeLineChain,
}

impl TunedPair {
    /// The P line's pieces.
    pub fn p(&self) -> Tuned {
        Tuned { pre: self.pre_p.clone(), raw: self.raw_p.clone(), tuned: self.tuned_p.clone(), post: self.post_p.clone() }
    }

    /// The N line's pieces.
    pub fn n(&self) -> Tuned {
        Tuned { pre: self.pre_n.clone(), raw: self.raw_n.clone(), tuned: self.tuned_n.clone(), post: self.post_n.clone() }
    }
}

/// `MEANDER_SKEW_PLACER::Start`'s refusal of a net that is not half of a pair.
pub const NO_PAIR_FOR_SKEW: &str = "Unable to find complementary differential pair net for skew tuning. Make sure the names of the nets belonging to a differential pair end with either _N/_P or +/-.";

/// What a skew placer starts from: the pair the clicked track belongs to, the tuning path length of the clicked net and of its partner
/// (`m_lastLength` and `m_coupledLength`).
#[derive(Debug, Clone)]
pub struct SkewSetup {
    pub partner_net: Net,
    pub own_length: Nm,
    pub coupled_length: Nm,
    /// The same as delays, attoseconds (`m_lastDelay` and `m_coupledDelay`): zero without a tuning profile that has the time domain.
    pub own_delay: i64,
    pub coupled_delay: i64,
    pub own_is_p: bool,
}

/// `MEANDER_SKEW_PLACER::Start`'s side of the pair: the partner's tuning path is the length the clicked line is tuned against.
pub fn skew_setup(world: &Node, env: &LengthEnv, start: ItemId) -> Result<SkewSetup, String> {
    let Some(Item::Segment(seg)) = world.get(start) else { return Err("Please select a differential pair track you want to tune.".into()) };
    let pair = assemble_diff_pair(world, start).ok_or(NO_PAIR_FOR_SKEW)?;
    if pair.p.segment_ids.is_empty() || pair.n.segment_ids.is_empty() {
        return Err("Unable to find the lines of the pair.".into());
    }
    let len = |ids: &[ItemId]| assemble_tuning_path(world, ids[0]).map(|p| p.length(world, env));
    let (len_p, len_n) = (len(&pair.p.segment_ids).ok_or("Please select a differential pair track you want to tune.")?, len(&pair.n.segment_ids).ok_or("Please select a differential pair track you want to tune.")?);
    // `lineDelay` of each net's path, by the profile of the clicked net's class
    let params = seg.net.as_deref().and_then(|net| env.delay_params(net));
    let delay = |ids: &[ItemId]| match (&params, assemble_tuning_path(world, ids[0])) {
        (Some(p), Some(path)) => path.delay(world, env, p),
        _ => 0,
    };
    let (delay_p, delay_n) = (delay(&pair.p.segment_ids), delay(&pair.n.segment_ids));
    let own_is_p = pair.net_p == seg.net;
    let (own_length, coupled_length, partner_net) = if own_is_p { (len_p, len_n, pair.net_n.clone()) } else { (len_n, len_p, pair.net_p.clone()) };
    let (own_delay, coupled_delay) = if own_is_p { (delay_p, delay_n) } else { (delay_n, delay_p) };
    Ok(SkewSetup { partner_net, own_length, coupled_length, own_delay, coupled_delay, own_is_p })
}

pub const NO_PAIR: &str = "Unable to find complementary differential pair net for length tuning. Make sure the names of the nets belonging to a differential pair end with either _N/_P or +/-.";

impl DpMeanderPlacer {
    /// `DP_MEANDER_PLACER::Start( aP, aStartItem )`.
    pub fn start(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings) -> Result<DpMeanderPlacer, String> {
        let Some(Item::Segment(seg)) = world.get(start) else { return Err("Please select a track whose length you want to tune.".into()) };
        let current_start = Seg::new(seg.a, seg.b).nearest_point(at);
        let mut pair = assemble_diff_pair(world, start).ok_or(NO_PAIR)?;
        if pair.gap < Nm::ZERO {
            pair.gap = env.rules.diff_pair_gap_of(pair.net_p.as_deref().unwrap_or(""));
        }
        if pair.p.segment_ids.is_empty() || pair.n.segment_ids.is_empty() {
            return Err("Unable to find the lines of the pair.".into());
        }
        let path_p = assemble_tuning_path(world, pair.p.segment_ids[0]).ok_or("Please select a track whose length you want to tune.")?;
        let path_n = assemble_tuning_path(world, pair.n.segment_ids[0]).ok_or("Please select a track whose length you want to tune.")?;
        let (len_p, len_n) = (path_p.length(world, env), path_n.length(world, env));
        // `m_netClass = conItem->GetEffectiveNetClass()`: the clicked net's class, and its tuning profile
        let delay_params = seg.net.as_deref().and_then(|net| env.delay_params(net));
        let (delay_p, delay_n) = match &delay_params {
            Some(p) => (path_p.delay(world, env, p), path_n.delay(world, env, p)),
            None => (0, 0),
        };
        let mut w = world.clone();
        w.remove_line_segments(&pair.p);
        w.remove_line_segments(&pair.n);
        let mut placer = DpMeanderPlacer {
            world: w,
            env: env.clone(),
            initial_segment: start,
            cp: chain_of(&pair.p),
            cn: chain_of(&pair.n),
            width_nm: pair.width,
            pair,
            path_p,
            path_n,
            len_p,
            len_n,
            delay_p,
            delay_n,
            delay_params,
            current_start,
            settings,
            last_length: Nm::ZERO,
            last_delay: 0,
            last_status: TuningStatus::TooShort,
            pre_p: ShapeLineChain::new(),
            raw_p: ShapeLineChain::new(),
            tuned_p: ShapeLineChain::new(),
            post_p: ShapeLineChain::new(),
            pre_n: ShapeLineChain::new(),
            raw_n: ShapeLineChain::new(),
            tuned_n: ShapeLineChain::new(),
            post_n: ShapeLineChain::new(),
            final_p: ShapeLineChain::new(),
            final_n: ShapeLineChain::new(),
        };
        placer.calculate_time_domain_targets();
        Ok(placer)
    }

    pub fn pair(&self) -> &PairAssembly {
        &self.pair
    }

    /// The tuning paths of P and of N.
    pub fn paths(&self) -> (&TuningPath, &TuningPath) {
        (&self.path_p, &self.path_n)
    }

    pub fn settings(&self) -> &MeanderSettings {
        &self.settings
    }

    pub fn update_settings(&mut self, s: MeanderSettings) {
        self.settings = s;
    }

    pub fn initial_segment(&self) -> ItemId {
        self.initial_segment
    }

    pub fn current_start(&self) -> Point {
        self.current_start
    }

    /// `origPathLength()`: the pair is as long as its longer net, nanometres.
    pub fn orig_path_length(&self) -> Nm {
        self.len_p.max(self.len_n)
    }

    /// The lengths of the two nets before the tuning.
    pub fn net_lengths(&self) -> (Nm, Nm) {
        (self.len_p, self.len_n)
    }

    /// `origPathDelay()`: the pair is as slow as its slower net, attoseconds.
    pub fn orig_path_delay(&self) -> i64 {
        self.delay_p.max(self.delay_n)
    }

    /// `TuningDelayResult()`.
    pub fn tuning_delay_result(&self) -> i64 {
        if self.last_delay != 0 {
            self.last_delay
        } else {
            self.orig_path_delay()
        }
    }

    fn params(&self) -> DelayParams {
        self.delay_params.clone().unwrap_or(DelayParams { enabled: false, track_as_per_mm: Default::default(), via_as_per_mm: 0, via_overrides: Vec::new() })
    }

    fn layer_name(&self) -> String {
        self.env.rules.layers.get(self.pair.p.layer as usize).cloned().unwrap_or_default()
    }

    /// `CalculatePropagationDelayForShapeLineChain`: the delay of a stretch of the tuned layer.
    fn chain_delay(&self, chain: &ShapeLineChain) -> i64 {
        self.params().track_delay(&self.layer_name(), chain.length())
    }

    /// `DP_MEANDER_PLACER::calculateTimeDomainTargets`: with the time domain on, the target delay (min, opt, max) becomes the target length it comes to on
    /// this layer, counted from the pair's length and delay now.
    pub fn calculate_time_domain_targets(&mut self) {
        if !self.settings.is_time_domain {
            return;
        }
        let (params, layer) = (self.params(), self.layer_name());
        let cur_delay = self.orig_path_delay();
        let d = self.settings.target_length_delay;
        let (desired_min, desired_opt, desired_max) = (d.min(), d.opt(), d.max());
        let difference_opt = desired_opt - cur_delay;
        let cur_length = self.orig_path_length();
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

    pub fn amplitude_step(&mut self, sign: i64) {
        let a = self.settings.max_amplitude + sign * self.settings.step;
        self.settings.max_amplitude = a.max(self.settings.min_amplitude);
    }

    pub fn spacing_step(&mut self, sign: i64) {
        let s = self.settings.spacing + sign * self.settings.step;
        let clearance = self.world.track_clearance(&self.env.rules, &self.pair.net_p, self.pair.p.layer);
        self.settings.spacing = s.max(self.width_nm + clearance);
    }

    /// `TuningLengthResult()`.
    pub fn tuning_length_result(&self) -> Nm {
        if self.last_length != Nm::ZERO { self.last_length } else { self.orig_path_length() }
    }

    pub fn tuning_status(&self) -> TuningStatus {
        self.last_status
    }

    pub fn world(&self) -> &Node {
        &self.world
    }

    pub fn tuned(&self) -> TunedPair {
        TunedPair {
            pre_p: self.pre_p.clone(),
            raw_p: self.raw_p.clone(),
            tuned_p: self.tuned_p.clone(),
            post_p: self.post_p.clone(),
            pre_n: self.pre_n.clone(),
            raw_n: self.raw_n.clone(),
            tuned_n: self.tuned_n.clone(),
            post_n: self.post_n.clone(),
        }
    }

    pub fn final_shapes(&self) -> (&ShapeLineChain, &ShapeLineChain) {
        (&self.final_p, &self.final_n)
    }

    fn update_status(&mut self) {
        self.last_status = if self.last_length > Nm::new(self.settings.target_length.max()) {
            TuningStatus::TooLong
        } else if self.last_length < Nm::new(self.settings.target_length.min()) {
            TuningStatus::TooShort
        } else {
            TuningStatus::Tuned
        };
    }

    /// `Move( aP, aEndItem )`.
    pub fn do_move(&mut self, to: Point) -> bool {
        self.calculate_time_domain_targets();
        let p = to;
        if self.current_start == p {
            return false;
        }
        let (mut pre_p, mut tuned_p, mut post_p) = self.cp.split3(self.current_start, p);
        let (mut pre_n, mut tuned_n, mut post_n) = self.cn.split3(self.current_start, p);
        let (raw_p, raw_n) = (tuned_p.clone(), tuned_n.clone());
        tuned_p.simplify();
        tuned_n.simplify();

        // "Bail out early if the tuned sections are empty (issue #22041)"
        if tuned_p.point_count() == 0 || tuned_n.point_count() == 0 {
            self.final_p = self.cp.clone();
            self.final_n = self.cn.clone();
            self.last_length = self.orig_path_length();
            self.last_status = TuningStatus::TooShort;
            return false;
        }

        let coupled = coupled_segment_pairs(&tuned_p, &tuned_n, self.width_nm, self.pair.gap);
        if coupled.is_empty() {
            // "Tuning started at an uncoupled area of the DP": the original, as it was
            self.final_p = self.cp.clone();
            self.final_n = self.cn.clone();
            self.last_length = self.orig_path_length();
            self.update_status();
            return false;
        }

        let clearance = self.world.track_clearance(&self.env.rules, &self.pair.net_p, self.pair.p.layer);
        let mut env = FitEnv { settings: self.settings.clone(), clearance, world: &self.world, rules: &self.env.rules, net_p: self.pair.net_p.clone(), net_n: self.pair.net_n.clone(), layer: self.pair.p.layer, width_nm: self.pair.width };
        let mut result = MeanderedLine::new(true);
        result.set_width(self.width_nm);
        let mut offset = (self.pair.gap + self.width_nm) / 2;
        if pair_orientation(&coupled[0]) {
            offset *= -1;
        }
        result.set_baseline_offset(offset);

        // `addCornersUntilIndex`: the corners of the two lines, up to the given vertex of each (a chain without arcs)
        let (mut cur_p, mut cur_n): (isize, isize) = (0, 0);
        let corner_at = |chain: &ShapeLineChain, i: isize| -> Point {
            // `GetSegment( i ).A`: a negative index counts from the last segment, and the last point's "segment" is the point itself
            let n = chain.segment_count() as isize;
            let i = if i < 0 { i + n } else { i };
            if i >= 0 && i < n {
                chain.c_segment(i as usize).a
            } else {
                chain.c_last_point()
            }
        };
        let mut add_corners_until = |result: &mut MeanderedLine, env: &FitEnv, tuned_p: &ShapeLineChain, tuned_n: &ShapeLineChain, last_p: isize, last_n: isize| {
            loop {
                let p_ok = cur_p <= last_p && cur_p != -1;
                let n_ok = cur_n <= last_n && cur_n != -1;
                if !p_ok && !n_ok {
                    break;
                }
                result.add_corner(env, corner_at(tuned_p, cur_p), corner_at(tuned_n, cur_n));
                if p_ok {
                    cur_p = tuned_p.next_shape(cur_p);
                }
                if n_ok {
                    cur_n = tuned_n.next_shape(cur_n);
                }
            }
        };

        for sp in &coupled {
            let base = baseline_segment(sp);
            let side = if env.settings.initial_side == MeanderSide::Default { base.side(p) < 0 } else { env.settings.initial_side.value() < 0 };
            add_corners_until(&mut result, &env, &tuned_p, &tuned_n, sp.index_p as isize, sp.index_n as isize);
            result.meander_segment(&mut env, base, side, 0);
        }
        add_corners_until(&mut result, &env, &tuned_p, &tuned_n, tuned_p.point_count() as isize - 1, tuned_n.point_count() as isize - 1);
        result.add_corner(&env, tuned_p.c_last_point(), tuned_n.c_last_point());
        // (a flipped initial side is the placer's to remember)
        self.settings = env.settings;

        let dp_len = self.orig_path_length();
        let dp_delay = self.orig_path_delay();
        self.last_status = TuningStatus::Tuned;
        if dp_len > Nm::new(self.settings.target_length.max()) {
            self.last_status = TuningStatus::TooLong;
            self.last_length = dp_len;
            self.last_delay = dp_delay;
        } else {
            self.last_length = dp_len - tuned_p.length().max(tuned_n.length());
            if self.settings.is_time_domain {
                self.last_delay = dp_delay - self.chain_delay(&tuned_p).max(self.chain_delay(&tuned_n));
            }
            tune_line_length(&mut result, Nm::new(self.settings.target_length.opt()) - dp_len);
        }

        if self.last_status != TuningStatus::TooLong {
            tuned_p.clear();
            tuned_n.clear();
            for m in result.meanders() {
                if m.kind() != MeanderType::Empty {
                    tuned_p.append_chain(m.c_line(0));
                    tuned_n.append_chain(m.c_line(1));
                }
            }
            self.last_length += tuned_p.length().max(tuned_n.length());
            if self.settings.is_time_domain {
                self.last_delay += self.chain_delay(&tuned_p).max(self.chain_delay(&tuned_n));
            }
            self.update_status();
        }

        let mut final_p = ShapeLineChain::new();
        let mut final_n = ShapeLineChain::new();
        if self.settings.keep_endpoints {
            for c in [&mut pre_p, &mut tuned_p, &mut post_p, &mut pre_n, &mut tuned_n, &mut post_n] {
                c.simplify();
            }
            final_p.append_chain(&pre_p);
            final_p.append_chain(&tuned_p);
            final_p.append_chain(&post_p);
            final_n.append_chain(&pre_n);
            final_n.append_chain(&tuned_n);
            final_n.append_chain(&post_n);
        } else {
            final_p.append_chain(&pre_p);
            final_p.append_chain(&tuned_p);
            final_p.append_chain(&post_p);
            final_p.simplify();
            final_n.append_chain(&pre_n);
            final_n.append_chain(&tuned_n);
            final_n.append_chain(&post_n);
            final_n.simplify();
        }
        self.pre_p = pre_p;
        self.raw_p = raw_p;
        self.tuned_p = tuned_p;
        self.post_p = post_p;
        self.pre_n = pre_n;
        self.raw_n = raw_n;
        self.tuned_n = tuned_n;
        self.post_n = post_n;
        self.final_p = final_p;
        self.final_n = final_n;
        true
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, um};
    use super::*;
    use crate::item::{Segment, Solid};
    use crate::layer::LayerRange;
    use eda_drc::kimath::Shape;

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn env() -> LengthEnv {
        let mut rules = BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        rules.clearance = mm(0.15);
        LengthEnv { stackup: eda_model::rules::default_stackup(2, mm(1.6)), rules }
    }

    fn pad(net: &str, at: Point) -> Item {
        Item::Solid(Solid { net: net_of(net), layers: LayerRange::single(0), pos: at, shape: Shape::Circle { c: at, r: mm(0.3) }, source: "U1.1".into(), edge: false })
    }

    fn seg(net: &str, a: Point, b: Point) -> Item {
        Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width: mm(0.2), source_track: None, locked: false })
    }

    /// USB_P and USB_N, 40 mm long, 0.5 mm apart (centre to centre 0.4 mm: gap 0.2 mm).
    fn board() -> (Node, ItemId, ItemId) {
        let mut w = Node::new();
        for (net, y) in [("USB_P", 0), ("USB_N", 400_000)] {
            w.add(pad(net, p(0, y)));
            w.add(pad(net, p(40_000_000, y)));
        }
        let a = w.add(seg("USB_P", p(0, 0), p(40_000_000, 0)));
        let b = w.add(seg("USB_N", p(0, 400_000), p(40_000_000, 400_000)));
        (w, a, b)
    }

    fn settings(target_nm: i64) -> MeanderSettings {
        let mut s = MeanderSettings::default();
        s.spacing = mm(1.0);
        s.set_target_length(target_nm);
        s.keep_endpoints = true;
        s
    }

    #[test]
    fn the_pair_a_track_belongs_to_is_assembled_with_its_gap() {
        let (w, a, b) = board();
        let pair = assemble_diff_pair(&w, a).expect("a pair");
        assert_eq!((pair.net_p.as_deref(), pair.net_n.as_deref()), (Some("USB_P"), Some("USB_N")));
        assert_eq!(pair.width, mm(0.2));
        assert_eq!(pair.gap, mm(0.2), "0.4 mm between the centres less a width");
        // from the N line the pair is the same, P first
        let from_n = assemble_diff_pair(&w, b).expect("a pair");
        assert_eq!(from_n.net_p.as_deref(), Some("USB_P"));
        // a net that is not half of a pair
        let mut w2 = Node::new();
        let s = w2.add(seg("CLK", p(0, 0), p(1_000, 0)));
        assert!(assemble_diff_pair(&w2, s).is_none());
    }

    #[test]
    fn a_pair_is_lengthened_together_and_stays_a_pitch_apart() {
        let (w, a, _) = board();
        let mut placer = DpMeanderPlacer::start(&w, &env(), a, p(5_000_000, 0), settings(43_000_000)).expect("a placer");
        assert_eq!(placer.orig_path_length(), mm(40.0));
        assert!(placer.do_move(p(35_000_000, 0)));
        let len = placer.tuning_length_result();
        assert!((len - mm(43.0)).abs() < mm(0.1), "{len}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let t = placer.tuned();
        assert!(!t.tuned_p.arcs().is_empty() && !t.tuned_n.arcs().is_empty());
        // the lines run alongside at the pitch: every vertex of one is about 0.4 mm from the other line
        let min_dist = |a: &ShapeLineChain, b: &ShapeLineChain| {
            a.points().iter().map(|&q| (0..b.segment_count()).map(|i| b.c_segment(i).distance(q)).min().unwrap()).min().unwrap()
        };
        let d = min_dist(&t.tuned_p, &t.tuned_n);
        assert!((d - mm(0.4)).abs() < um(3.0), "closest approach {d}");
        // the two nets are within a miter of each other in length
        let (lp, ln) = (t.pre_p.length() + t.tuned_p.length() + t.post_p.length(), t.pre_n.length() + t.tuned_n.length() + t.post_n.length());
        assert!((lp - ln).abs() < mm(0.7), "{lp} vs {ln}");
        assert!(lp.max(ln) == len);
    }

    /// The time domain for a pair: the pair is as slow as its slower net (`origPathDelay`), and the target delay is made the target length from it.
    #[test]
    fn a_pair_is_tuned_to_a_delay_through_the_tuning_profile_of_its_class() {
        use eda_model::setup_pages::{ProfileLayerEntry, ProfileType, TuningProfile, TuningProfiles};
        let (w, a, _) = board();
        let mut e = env();
        let profile = TuningProfile {
            name: "USB".into(),
            profile_type: ProfileType::Differential,
            target_impedance: 90.0,
            enable_time_domain_tuning: true,
            layer_entries: vec![ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: mm(0.2), diff_pair_gap_nm: mm(0.2), delay_as_per_mm: 6_100_000 }],
            via_propagation_delay_as: 0,
            via_overrides: vec![],
        };
        e.rules.tuning_profiles = TuningProfiles { profiles: vec![profile], class_profiles: [("Default".to_string(), "USB".to_string())].into() };
        let mut s = MeanderSettings::default();
        s.spacing = mm(1.0);
        s.keep_endpoints = true;
        s.is_time_domain = true;
        s.set_target_length_delay(280 * crate::delay::ATTOSECONDS_PER_PS);
        let mut placer = DpMeanderPlacer::start(&w, &e, a, p(5_000_000, 0), s).expect("a placer");
        // 40 mm at 6.1 ps/mm is 244 ps; 280 ps is 45.90 mm
        assert_eq!(placer.orig_path_delay(), 244_000_000);
        assert!((placer.settings().target_length.opt() - 45_901_639).abs() < 5, "{}", placer.settings().target_length.opt());
        assert!(placer.do_move(p(35_000_000, 0)));
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        let delay = placer.tuning_delay_result();
        assert!((delay - 280_000_000).abs() < 300_000, "the pair takes {delay} as");
        assert!((placer.tuning_length_result() - Nm::new(placer.settings().target_length.opt())).abs() < um(20.0));
    }

    #[test]
    fn a_net_that_is_not_in_a_pair_is_refused_with_kicads_words() {
        let mut w = Node::new();
        let s = w.add(seg("CLK", p(0, 0), p(1_000, 0)));
        let e = DpMeanderPlacer::start(&w, &env(), s, p(0, 0), settings(2_000_000)).err().expect("an error");
        assert!(e.starts_with("Unable to find complementary differential pair net for length tuning"), "{e}");
    }

    #[test]
    fn coupled_segments_are_the_stretches_that_face_each_other_at_the_gap() {
        let p_line = ShapeLineChain::from_points(&[p(0, 0), p(10_000_000, 0), p(10_000_000, 5_000_000)]);
        let n_line = ShapeLineChain::from_points(&[p(0, 400_000), p(9_600_000, 400_000), p(9_600_000, 5_000_000)]);
        let c = coupled_segment_pairs(&p_line, &n_line, mm(0.2), mm(0.2));
        assert_eq!(c.len(), 2, "the two straight runs are coupled, the corners are not");
        assert_eq!((c[0].index_p, c[0].index_n), (0, 0));
        let base = baseline_segment(&c[0]);
        assert_eq!(base.a.y, mm(0.2));
        assert!(pair_orientation(&c[0]), "N is below P, on the positive side");
        // a line too far for the gap
        assert!(coupled_segment_pairs(&p_line, &n_line, mm(0.2), mm(0.5)).is_empty());
    }
}
