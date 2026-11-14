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
//! The pair is started on a segment or an arc of either net (`SEGMENT_T | ARC_T`): `AssembleDiffPair` pairs arcs by their centre and radius, and a stretch
//! of the pair through arcs keeps them as arcs (`addCornersUntilIndex`'s `AddArc( arcP, arcN )`). Each net's length counts the pad-to-die length of the
//! pads its path ends on.
//!
//! Differences from KiCad as in [`crate::meander_placer`]: the lines stop where the track width changes.

use crate::chain::ShapeLineChain;
use crate::delay::DelayParams;
use crate::diff_pair::{dp_coupled_net_name, dp_net_polarity};
use crate::item::{net_of, Item, ItemId, Net};
use crate::kigeom::{common_parallel_projection, ki_round_nm, Seg, ShapeArc};
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

/// What `Shape( -1 )->SquaredDistance( p )` of an arc track is (`SHAPE::SquaredDistance`: the distance to the arc's outline at its width, zero inside it).
fn arc_sq_distance(arc: &crate::arcs::ArcPts, p: Point, width_nm: Nm) -> i128 {
    let d = Nm::new(arc.distance_to(p).round() as i64) - width_nm / 2;
    let d = d.max(Nm::ZERO);
    d.area(d)
}

/// `SHAPE_ARC::BBox( 0 ).Centre()` (`SHAPE::Centre`: the box of the start, the middle, the end and the quadrant points the arc passes, `update_values`),
/// the centre of the box by `BOX2I::Centre`'s integer halves.
fn arc_bbox_centre(a: &ShapeArc) -> Point {
    let mut pts = vec![a.start, a.mid, a.end];
    let mut start_angle = a.start_angle();
    let mut end_angle = start_angle + a.central_angle();
    // "we always count quadrants clockwise (increasing angle)"
    if start_angle > end_angle {
        std::mem::swap(&mut start_angle, &mut end_angle);
    }
    let (q0, q1) = ((start_angle / 90.0).ceil() as i64, (end_angle / 90.0).floor() as i64);
    if a.radius() < (i32::MAX as f64) / 2.0 {
        let r = Nm::new(a.radius().round() as i64);
        let c = a.center();
        for q in q0..=q1 {
            pts.push(match q.rem_euclid(4) {
                0 => Point { x: c.x + r, y: c.y },
                1 => Point { x: c.x, y: c.y + r },
                2 => Point { x: c.x - r, y: c.y },
                _ => Point { x: c.x, y: c.y - r },
            });
        }
    }
    let (x0, x1) = (pts.iter().map(|p| p.x).min().unwrap(), pts.iter().map(|p| p.x).max().unwrap());
    let (y0, y1) = (pts.iter().map(|p| p.y).min().unwrap(), pts.iter().map(|p| p.y).max().unwrap());
    Point { x: x0 + Nm::new((x1 - x0).get() / 2), y: y0 + Nm::new((y1 - y0).get() / 2) }
}

/// A track item of a pair as `AssembleDiffPair` compares it: a segment or an arc (`ITEM::SEGMENT_T | ITEM::ARC_T`), with its width and layer.
enum Track {
    Seg(Seg),
    Arc(crate::arcs::ArcPts, ShapeArc),
}

/// The segment or arc `id` of `world`: its geometry, width, layer and net.
fn track_of(world: &Node, id: ItemId) -> Option<(Track, Nm, i32, Net)> {
    match world.get(id) {
        Some(Item::Segment(s)) => Some((Track::Seg(Seg::new(s.a, s.b)), s.width, s.layer, s.net.clone())),
        Some(Item::Arc(a)) => Some((Track::Arc(a.arc, a.arc.shape_nm()), a.width, a.layer, a.net.clone())),
        _ => None,
    }
}

/// The best pair of a segment of the clicked line and a segment of the partner net `findNItem` has found so far.
struct BestPair {
    dist_sq: i128,
    dist_target_sq: i128,
    found: Option<(ItemId, ItemId)>,
}

/// `TOPOLOGY::AssembleDiffPair( aStart, aPair )`: the segment or arc `start` and the nearest parallel segment (an arc of the same centre) of the coupled
/// net on its layer make the pair; their lines are P and N. Segments are paired when they have the same width, are parallel within
/// `DP_PARALLELITY_THRESHOLD` and face each other (`commonParallelProjection`), arcs when they have the same width and centres within the threshold; of
/// those, the nearest to the clicked item, then the one nearest the clicked item's centre (`Shape( -1 )->Centre()`). The gap is the distance between the two
/// segments (the difference of the two radii) less the width.
pub fn assemble_diff_pair(world: &Node, start: ItemId) -> Option<PairAssembly> {
    let (start_track, _, layer, ref_net) = track_of(world, start)?;
    let ref_name = ref_net.as_deref()?.to_string();
    let coupled_name = dp_coupled_net_name(&ref_name)?;
    let coupled_net = net_of(&coupled_name);
    let lp = world.assemble_line_with(start, assemble_opts())?;

    // `AllItemsInNet( coupledNet )`: the segments and arcs of the coupled net on the layers of the start item
    let mut n_items: Vec<ItemId> = world.iter().filter(|(_, it)| matches!(it, Item::Segment(s) if s.net == coupled_net && s.layer == layer) || matches!(it, Item::Arc(a) if a.net == coupled_net && a.layer == layer)).map(|(id, _)| id).collect();
    n_items.sort_unstable();

    // `aStart->Shape( -1 )->Centre()`: a segment's middle (`SHAPE_SEGMENT::Centre`), an arc's box's centre
    let center = match &start_track {
        Track::Seg(s) => Point { x: s.a.x + ki_round_nm((s.b.x - s.a.x).as_f64() / 2.0), y: s.a.y + ki_round_nm((s.b.y - s.a.y).as_f64() / 2.0) },
        Track::Arc(_, shape) => arc_bbox_centre(shape),
    };

    // `findNItem( p_item )`
    let find_n_item = |best: &mut BestPair, p_item: ItemId| {
        let Some((p_track, p_width, _, _)) = track_of(world, p_item) else { return };
        for &n_item in &n_items {
            let Some((n_track, n_width, _, _)) = track_of(world, n_item) else { continue };
            // `if( n_item->Kind() != p_item->Kind() ) continue;`
            let dist_sq = match (&p_track, &n_track) {
                (Track::Seg(p_seg), Track::Seg(n_seg)) => {
                    if n_width != p_width || !p_seg.approx_parallel(n_seg, DP_PARALLELITY_THRESHOLD) || common_parallel_projection(*p_seg, *n_seg).is_none() {
                        continue;
                    }
                    n_seg.squared_distance_seg(p_seg)
                }
                (Track::Arc(_, p_arc), Track::Arc(_, n_arc)) => {
                    if n_width != p_width {
                        continue;
                    }
                    let (cp, cn) = (p_arc.center(), n_arc.center());
                    let (dx, dy) = (cn.x - cp.x, cn.y - cp.y);
                    if dx.area(dx) + dy.area(dy) > DP_PARALLELITY_THRESHOLD.area(DP_PARALLELITY_THRESHOLD) {
                        continue;
                    }
                    // `SEG::Square( p_arc->CArc().GetRadius() - n_arc->CArc().GetRadius() )`: the difference of the radii as an integer, squared
                    let d = (p_arc.radius() - n_arc.radius()) as i64 as i128;
                    d * d
                }
                _ => continue,
            };
            if dist_sq <= best.dist_sq {
                let dist_target_sq = match &n_track {
                    Track::Seg(n_seg) => stadium_sq_distance(n_seg, center, n_width),
                    Track::Arc(pts, _) => arc_sq_distance(pts, center, n_width),
                };
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
        // the other tracks that end where the clicked one does (`linked->Anchor( i )` of a segment or an arc)
        let anchors = match world.get(start) {
            Some(Item::Segment(s)) => [s.a, s.b],
            Some(Item::Arc(a)) => [a.start(), a.end()],
            _ => return None,
        };
        let mut links: Vec<ItemId> = Vec::new();
        for anchor in anchors {
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
            // (`findNItem` of a via or a pad finds nothing: the coupled items are segments and arcs, of the other kind)
            if matches!(world.get(l), Some(Item::Segment(_) | Item::Arc(_))) {
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
    let gap = match (track_of(world, ref_item), track_of(world, coupled_item)) {
        // "Segments are parallel -> compute pair gap"
        (Some((Track::Seg(r), ..)), Some((Track::Seg(c), ..))) => {
            let dir = (r.b.x - r.a.x, r.b.y - r.a.y);
            let disp = (r.b.x - c.b.x, r.b.y - c.b.y);
            let cross = dir.0.area(disp.1) - dir.1.area(disp.0);
            let norm = crate::kigeom::norm_i(dir.0, dir.1).max(Nm::new(1)).get() as i128;
            Nm::new((cross.abs() / norm) as i64) - lp.width
        }
        // `(int) std::abs( refArc->CArc().GetRadius() - coupledArc->CArc().GetRadius() ) - lp.Width()`
        (Some((Track::Arc(_, r), ..)), Some((Track::Arc(_, c), ..))) => Nm::new((r.radius() - c.radius()).abs() as i64) - lp.width,
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

/// `MEANDER_SKEW_PLACER::Start`'s side of the pair: the partner's tuning path is the length the clicked line is tuned against. The clicked item is a
/// segment or an arc (`SEGMENT_T | ARC_T`); each net's length and delay count the pad-to-die length and delay of the pads its path ends on
/// (`m_padToDieLengthP + lineLength( m_tunedPathP, .. )`).
pub fn skew_setup(world: &Node, env: &LengthEnv, start: ItemId) -> Result<SkewSetup, String> {
    let Some((_, _, _, net)) = track_of(world, start) else { return Err("Please select a differential pair track you want to tune.".into()) };
    let pair = assemble_diff_pair(world, start).ok_or(NO_PAIR_FOR_SKEW)?;
    if pair.p.segment_ids.is_empty() || pair.n.segment_ids.is_empty() {
        return Err("Unable to find the lines of the pair.".into());
    }
    let len = |ids: &[ItemId]| assemble_tuning_path(world, ids[0]).map(|p| p.full_length(world, env));
    let (len_p, len_n) = (len(&pair.p.segment_ids).ok_or("Please select a differential pair track you want to tune.")?, len(&pair.n.segment_ids).ok_or("Please select a differential pair track you want to tune.")?);
    // `m_padToDieDelayP + lineDelay( m_tunedPathP, .. )` of each net's path, by the profile of the clicked net's class
    let params = net.as_deref().and_then(|net| env.delay_params(net));
    let delay = |ids: &[ItemId]| assemble_tuning_path(world, ids[0]).map_or(0, |path| path.full_delay(world, env, params.as_ref()));
    let (delay_p, delay_n) = (delay(&pair.p.segment_ids), delay(&pair.n.segment_ids));
    let own_is_p = pair.net_p == net;
    let (own_length, coupled_length, partner_net) = if own_is_p { (len_p, len_n, pair.net_n.clone()) } else { (len_n, len_p, pair.net_p.clone()) };
    let (own_delay, coupled_delay) = if own_is_p { (delay_p, delay_n) } else { (delay_n, delay_p) };
    Ok(SkewSetup { partner_net, own_length, coupled_length, own_delay, coupled_delay, own_is_p })
}

pub const NO_PAIR: &str = "Unable to find complementary differential pair net for length tuning. Make sure the names of the nets belonging to a differential pair end with either _N/_P or +/-.";

impl DpMeanderPlacer {
    /// `DP_MEANDER_PLACER::Start( aP, aStartItem )`.
    ///
    /// The clicked item is a segment or an arc (`SEGMENT_T | ARC_T`); the pattern starts at the point of a segment nearest `at`, at the nearer end of an
    /// arc (`getSnappedStartPoint`).
    pub fn start(world: &Node, env: &LengthEnv, start: ItemId, at: Point, settings: MeanderSettings) -> Result<DpMeanderPlacer, String> {
        let (current_start, net) = match world.get(start) {
            Some(Item::Segment(seg)) => (Seg::new(seg.a, seg.b).nearest_point(at), seg.net.clone()),
            Some(Item::Arc(a)) => (if crate::seg_ops::length(at, a.start()) <= crate::seg_ops::length(at, a.end()) { a.start() } else { a.end() }, a.net.clone()),
            _ => return Err("Please select a track whose length you want to tune.".into()),
        };
        let mut pair = assemble_diff_pair(world, start).ok_or(NO_PAIR)?;
        if pair.gap < Nm::ZERO {
            pair.gap = env.rules.diff_pair_gap_of(pair.net_p.as_deref().unwrap_or(""));
        }
        if pair.p.segment_ids.is_empty() || pair.n.segment_ids.is_empty() {
            return Err("Unable to find the lines of the pair.".into());
        }
        let path_p = assemble_tuning_path(world, pair.p.segment_ids[0]).ok_or("Please select a track whose length you want to tune.")?;
        let path_n = assemble_tuning_path(world, pair.n.segment_ids[0]).ok_or("Please select a track whose length you want to tune.")?;
        // `m_padToDieLengthP + lineLength( m_tunedPathP, .. )`, the same for N: each net with the pad-to-die length of the pads its path ends on
        let (len_p, len_n) = (path_p.full_length(world, env), path_n.full_length(world, env));
        // `m_netClass = conItem->GetEffectiveNetClass()`: the clicked net's class, and its tuning profile
        let delay_params = net.as_deref().and_then(|net| env.delay_params(net));
        // `origPathDelay()`: `totalP = m_padToDieDelayP + lineDelay( m_tunedPathP, .. )` and `totalN = m_padToDieDelayP + lineDelay( m_tunedPathN, .. )` -- KiCad
        // adds P's pad-to-die delay to N's line too (8303b2ad), and so does this
        let (die_p, die_n) = (path_p.pad_to_die(world, env).1, path_n.pad_to_die(world, env).1);
        let line_delay = |path: &TuningPath, die: i64| path.full_delay(world, env, delay_params.as_ref()) - die;
        let (delay_p, delay_n) = (die_p + line_delay(&path_p, die_p), die_p + line_delay(&path_n, die_n));
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

        // `addCornersUntilIndex`: the corners of the two lines, up to the given vertex of each; an arc of both lines is `AddArc( arcP, arcN )`, and
        // where only one line has an arc here the other's corners are added until it has its arc too
        let (mut cur_p, mut cur_n): (isize, isize) = (0, 0);
        // `getItem( aChain, aIndex )`: the arc the vertex starts (with its ends) or the segment's ends; `GetSegment( i )`: a negative index counts from the
        // last segment, and the last point's "segment" is the point itself
        let item_at = |chain: &ShapeLineChain, i: isize| -> (Option<ShapeArc>, Point) {
            // (`IsArcSegment( size_t )` of a negative index is false)
            if i >= 0 && (i as usize) < chain.point_count() && chain.is_arc_segment(i as usize) {
                let arc = chain.arc(chain.arc_index(i as usize) as usize).clone();
                let start = arc.start;
                return (Some(arc), start);
            }
            let n = chain.segment_count() as isize;
            let i = if i < 0 { i + n } else { i };
            if i >= 0 && i < n {
                (None, chain.c_segment(i as usize).a)
            } else {
                (None, chain.c_last_point())
            }
        };
        let mut add_corners_until = |result: &mut MeanderedLine, env: &FitEnv, tuned_p: &ShapeLineChain, tuned_n: &ShapeLineChain, last_p: isize, last_n: isize| {
            let check = |cur: isize, last: isize| cur <= last && cur != -1;
            loop {
                let (mut p_ok, mut n_ok) = (check(cur_p, last_p), check(cur_n, last_n));
                if !p_ok && !n_ok {
                    break;
                }
                let (p_arc, p_start) = item_at(tuned_p, cur_p);
                let (mut n_arc, mut n_start) = item_at(tuned_n, cur_n);
                match (&p_arc, &n_arc) {
                    (None, None) => result.add_corner(env, p_start, n_start),
                    (Some(pa), Some(na)) => result.add_arc(env, pa, na),
                    (Some(pa), None) => {
                        result.add_corner(env, p_start, n_start);
                        // "Find arc in N"
                        loop {
                            n_ok = check(cur_n, last_n);
                            if !n_ok {
                                break;
                            }
                            cur_n = tuned_n.next_shape(cur_n);
                            (n_arc, n_start) = item_at(tuned_n, cur_n);
                            if let Some(na) = &n_arc {
                                result.add_arc(env, pa, na);
                                break;
                            }
                            result.add_corner(env, p_start, n_start);
                        }
                    }
                    (None, Some(na)) => {
                        let na = na.clone();
                        result.add_corner(env, p_start, n_start);
                        // "Find arc in P"
                        loop {
                            p_ok = check(cur_p, last_p);
                            if !p_ok {
                                break;
                            }
                            cur_p = tuned_p.next_shape(cur_p);
                            let (pa, ps) = item_at(tuned_p, cur_p);
                            if let Some(pa) = &pa {
                                result.add_arc(env, pa, &na);
                                break;
                            }
                            result.add_corner(env, ps, n_start);
                        }
                    }
                }
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
        LengthEnv { stackup: eda_model::rules::default_stackup(2, mm(1.6)), rules, pad_to_die: Default::default() }
    }

    fn pad(net: &str, at: Point) -> Item {
        Item::Solid(Solid { net: net_of(net), layers: LayerRange::single(0), pos: at, shape: Shape::Circle { c: at, r: mm(0.3) }, source: "U1.1".into(), edge: false, rule: 0, free_pad: false, anchor_points: Vec::new() })
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
            ..Default::default()
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

    fn arc(net: &str, start: Point, mid: Point, end: Point) -> Item {
        Item::Arc(crate::item::Arc::new(net_of(net), 0, mm(0.2), crate::arcs::ArcPts::new(start, mid, end), None, false))
    }

    /// A pair that turns a corner on two arcs of one centre (P outside at r 5 mm, N inside at r 4.6 mm: a pitch of 0.4 mm, a gap of 0.2 mm), then runs on
    /// downwards: P from (0, 0) along to (20, 0) mm, round to (25, 5) mm and down to (25, 30) mm; N alongside.
    fn arc_board() -> (Node, ItemId, ItemId, ItemId) {
        let mut w = Node::new();
        let named = |net: &str, at: Point, name: &str| Item::Solid(Solid { net: net_of(net), layers: LayerRange::single(0), pos: at, shape: Shape::Circle { c: at, r: mm(0.15) }, source: name.into(), edge: false, rule: 0, free_pad: false, anchor_points: Vec::new() });
        w.add(named("USB_P", p(0, 0), "J1.1"));
        w.add(named("USB_N", p(0, 400_000), "J1.2"));
        w.add(named("USB_P", p(25_000_000, 30_000_000), "U1.1"));
        w.add(named("USB_N", p(24_600_000, 30_000_000), "U1.2"));
        let s45 = std::f64::consts::FRAC_1_SQRT_2;
        let mid = |r: f64| p(20_000_000 + (r * s45).round() as i64, 5_000_000 - (r * s45).round() as i64);
        let sp = w.add(seg("USB_P", p(0, 0), p(20_000_000, 0)));
        let ap = w.add(arc("USB_P", p(20_000_000, 0), mid(5_000_000.0), p(25_000_000, 5_000_000)));
        w.add(seg("USB_P", p(25_000_000, 5_000_000), p(25_000_000, 30_000_000)));
        w.add(seg("USB_N", p(0, 400_000), p(20_000_000, 400_000)));
        let an = w.add(arc("USB_N", p(20_000_000, 400_000), mid(4_600_000.0), p(24_600_000, 5_000_000)));
        w.add(seg("USB_N", p(24_600_000, 5_000_000), p(24_600_000, 30_000_000)));
        (w, sp, ap, an)
    }

    /// F4-2: `TOPOLOGY::AssembleDiffPair` pairs an arc with the arc of the coupled net that has its centre (within `DP_PARALLELITY_THRESHOLD`), the gap being the
    /// difference of the radii less the width; a straight segment is paired with the straight one.
    #[test]
    fn a_pair_is_assembled_from_an_arc_by_its_centre_and_radius() {
        let (w, sp, ap, an) = arc_board();
        let pair = assemble_diff_pair(&w, ap).expect("a pair from the arc");
        assert_eq!((pair.net_p.as_deref(), pair.net_n.as_deref()), (Some("USB_P"), Some("USB_N")));
        assert!((pair.gap - mm(0.2)).abs() <= Nm::new(2), "|5 - 4.6| mm - 0.2 mm: {}", pair.gap);
        assert_eq!((pair.p.pts.first().copied(), pair.p.pts.last().copied()), (Some(p(0, 0)), Some(p(25_000_000, 30_000_000))), "the whole P line, through its arc");
        assert_eq!(pair.p.arcs.len(), 1);
        let from_n = assemble_diff_pair(&w, an).expect("a pair from the N arc");
        assert_eq!(from_n.net_p.as_deref(), Some("USB_P"));
        assert!((from_n.gap - mm(0.2)).abs() <= Nm::new(2));
        let from_seg = assemble_diff_pair(&w, sp).expect("a pair from the straight");
        assert_eq!(from_seg.gap, mm(0.2));
        // an arc of the other net with another centre is no partner
        let mut w2 = Node::new();
        let a = w2.add(arc("USB_P", p(0, 0), p(1_000_000, 1_000_000), p(2_000_000, 0)));
        w2.add(arc("USB_N", p(0, 3_000_000), p(1_000_000, 4_000_000), p(2_000_000, 3_000_000)));
        assert!(assemble_diff_pair(&w2, a).is_none());
    }

    /// F4-2: `DP_MEANDER_PLACER::Start` takes an arc (`SEGMENT_T | ARC_T`; the pattern starts at its nearer end, `getSnappedStartPoint`), and a pattern
    /// across the corner keeps the two arcs as arcs (`addCornersUntilIndex`'s `AddArc( arcP, arcN )`) while the straights on either side are meandered.
    #[test]
    fn a_pair_is_tuned_from_an_arc_and_across_its_arcs() {
        let (w, _, ap, _) = arc_board();
        let placer = DpMeanderPlacer::start(&w, &env(), ap, p(21_000_000, 100_000), settings(60_000_000)).expect("a placer on the arc");
        assert_eq!(placer.current_start(), p(20_000_000, 0), "the arc's nearer end");
        let (w, sp, _, _) = arc_board();
        let base = assemble_diff_pair(&w, sp).unwrap();
        let mut placer = DpMeanderPlacer::start(&w, &env(), sp, p(5_000_000, 0), settings(60_000_000)).expect("a placer");
        let orig = placer.orig_path_length();
        assert!((orig - (mm(20.0) + mm(25.0) + Nm::new((5_000_000.0 * std::f64::consts::FRAC_PI_2) as i64))).abs() < um(2.0), "P is the longer: {orig}");
        assert!(placer.do_move(p(25_000_000, 25_000_000)));
        let t = placer.tuned();
        let corner_arc = |c: &ShapeLineChain, r: f64| c.arcs().iter().any(|a| (a.center().x - mm(20.0)).abs() < um(1.0) && (a.center().y - mm(5.0)).abs() < um(1.0) && (a.radius() - r).abs() < 1_000.0);
        assert!(corner_arc(&t.tuned_p, 5_000_000.0), "P keeps its corner arc: {:?}", t.tuned_p.arcs());
        assert!(corner_arc(&t.tuned_n, 4_600_000.0), "N keeps its corner arc: {:?}", t.tuned_n.arcs());
        let len = placer.tuning_length_result();
        assert!(len > orig + mm(5.0), "the pair was lengthened: {len} from {orig}");
        assert!((len - mm(60.0)).abs() < mm(0.1), "{len}");
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
        // the pair keeps the pitch where it is coupled: the meandered P is 0.4 mm from N
        let d = t.tuned_p.points().iter().map(|&q| (0..t.tuned_n.segment_count()).map(|i| t.tuned_n.c_segment(i).distance(q)).min().unwrap()).min().unwrap();
        assert!((d - mm(0.4)).abs() < um(3.0), "closest approach {d}");
        let _ = base;
    }

    /// F4-2: `MEANDER_SKEW_PLACER::Start` takes an arc of the pair too; the skew of the arc pair is the length P has more than N round the corner.
    #[test]
    fn the_skew_of_a_pair_is_set_up_from_an_arc() {
        let (w, _, ap, an) = arc_board();
        let setup = skew_setup(&w, &env(), an).expect("a skew setup from the N arc");
        assert!(!setup.own_is_p);
        let skew = setup.coupled_length - setup.own_length;
        // P is 0.4 mm longer on the straights (it ends 0.4 mm further) and 0.4 mm x pi/2 longer round the corner
        let expected = Nm::new((400_000.0 * std::f64::consts::FRAC_PI_2) as i64);
        assert!((skew - expected).abs() < um(2.0), "{skew} vs {expected}");
        assert!(skew_setup(&w, &env(), ap).expect("from the P arc").own_is_p);
        let mut st = MeanderSettings::default();
        st.spacing = mm(1.0);
        st.set_target_skew(0);
        st.keep_endpoints = true;
        let mut placer = crate::meander_placer::MeanderPlacer::start_skew(&w, &env(), an, p(24_600_000, 5_000_000), st, setup.coupled_length, setup.coupled_delay).expect("a skew placer on the arc");
        assert!(placer.do_move(p(24_600_000, 25_000_000)));
        assert!(placer.tuning_length_result().abs() <= um(100.0), "N lengthened to P: skew {}", placer.tuning_length_result());
        assert_eq!(placer.tuning_status(), TuningStatus::Tuned);
    }

    /// `DP_MEANDER_PLACER::Start`: each net counts the pad-to-die length of its own pads (`m_padToDieLengthP`, `m_padToDieLengthN`), the pair being its longer
    /// net; the delay of N takes P's pad-to-die delay, as KiCad 8303b2ad's `origPathDelay` adds it (`totalN = m_padToDieDelayP + lineDelay( m_tunedPathN, .. )`).
    #[test]
    fn each_net_of_a_pair_counts_the_pad_to_die_length_of_its_pads() {
        let (w, sp, _, _) = arc_board();
        let mut e = env();
        e.pad_to_die = [("J1.2".to_string(), (mm(3.0), 5_000_000)), ("U1.2".to_string(), (mm(1.0), 0)), ("J1.1".to_string(), (Nm::ZERO, 2_000_000))].into();
        let placer = DpMeanderPlacer::start(&w, &e, sp, p(5_000_000, 0), settings(60_000_000)).expect("a placer");
        let (lp, ln) = placer.net_lengths();
        let plain = DpMeanderPlacer::start(&w, &env(), sp, p(5_000_000, 0), settings(60_000_000)).unwrap().net_lengths();
        assert_eq!((lp, ln), (plain.0, plain.1 + mm(4.0)));
        assert_eq!(placer.orig_path_length(), ln, "N is the longer now");
        assert_eq!(placer.orig_path_delay(), 2_000_000, "both nets' delays carry P's 2 ps (KiCad's), none has a profile");
        // the skew tuner sees the same lengths
        let setup = skew_setup(&w, &e, sp).unwrap();
        assert_eq!((setup.own_length, setup.coupled_length), (lp, ln));
        assert_eq!((setup.own_delay, setup.coupled_delay), (2_000_000, 5_000_000), "each net its own pads' delay (`MEANDER_SKEW_PLACER::Start`)");
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
