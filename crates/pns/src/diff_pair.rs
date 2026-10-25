//! Port of the geometry of a differential pair: `PNS::DP_PRIMITIVE_PAIR`, `DP_GATEWAY`, `DP_GATEWAYS` and `DIFF_PAIR`
//! (`pcbnew/router/pns_diff_pair.{h,cpp}`). The pair placer that drives them, `DIFF_PAIR_PLACER`
//! (`pns_diff_pair_placer.cpp`), is [`crate::dp_placer`].
//!
//! A pair is routed the way KiCad routes it: from a **gateway** at the start to a gateway at the end, where a gateway is two
//! anchors (one per net) with an orientation, a priority and, optionally, entry lines that fan the two nets out of a pad pair.
//! [`DpGateways::build_from_primitive_pair`] makes the gateways of a pair of pads, vias or track ends (the start, and the end
//! when the cursor is on a coupled pair of them), [`DpGateways::build_for_cursor`] those around the cursor,
//! [`DpGateways::fit_gateways`] tries every start x end x posture and keeps the best one that [`DiffPair::build_initial`]
//! accepts (gap kept, no crossing, only obtuse turns).
//!
//! ## Units
//!
//! KiCad's coordinates are nanometres and so are this IR's: `checkGap`'s 100 nm slack is [`GAP_CHECK_SLACK`], the 22.5 degree
//! gateway's alignment test is KiCad's 5 nm, and the +-10 um coupled-gap tolerance (`RANGED_NUM( m_gap, 10000, 10000 )`) is
//! [`GAP_TOLERANCE_NM`].
//!
//! ## Pair detection and sizing
//!
//! [`dp_coupled_net_name`] is `BOARD::MatchDpSuffix` ported verbatim (`pcbnew/board.cpp`): walk a net name backward past any
//! trailing digits/underscores, and if the next character is `+`/`-`/`P`/`N`, swap it for its complement (`+`<->`-`, `P`<->`N`),
//! preserving whatever digits/underscores followed it (`"LVDS_P0"` -> `"LVDS_N0"`). [`dp_net_polarity`] is its return value
//! (+1 for `+`/`P`, -1 for `-`/`N`), which decides which net is the pair's P and which its N (`DpNetPair`).

use crate::direction45::{AngleType, CornerMode, Direction45};
use crate::item::{same_net, Item, Net, Segment, Via};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::{Node, QueryOpts};
use crate::seg_ops::{self, rescale, Seg2};
use eda_drc::kimath::{isqrt, Seg, Shape};
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;

// ------------------------------------------------------------------------------------------------
// net names
// ------------------------------------------------------------------------------------------------

/// `BOARD::MatchDpSuffix`: the complementary net name for a differential pair, by KiCad's own naming convention, or `None` if
/// `net_name` doesn't end (after any trailing digits/underscores) in a recognized suffix character. See this module's header
/// comment for the exact algorithm and worked examples.
pub fn dp_coupled_net_name(net_name: &str) -> Option<String> {
    let chars: Vec<char> = net_name.chars().collect();
    let mut count = 0usize;
    let mut complement: Option<char> = None;
    for &ch in chars.iter().rev() {
        count += 1;
        if ch.is_ascii_digit() || ch == '_' {
            continue;
        }
        complement = match ch {
            '+' => Some('-'),
            '-' => Some('+'),
            'N' => Some('P'),
            'P' => Some('N'),
            _ => None,
        };
        break;
    }
    let complement = complement?;
    let len = chars.len();
    if count == 0 || count > len {
        return None;
    }
    let prefix: String = chars[..len - count].iter().collect();
    let suffix: String = chars[(len - count + 1)..].iter().collect();
    Some(format!("{prefix}{complement}{suffix}"))
}

/// `BOARD::MatchDpSuffix`'s return value: `1` for the positive net of a pair (`+`, `P`), `-1` for the negative one (`-`, `N`), `0`
/// for a net name that is not half of a pair.
pub fn dp_net_polarity(net_name: &str) -> i32 {
    for ch in net_name.chars().rev() {
        if ch.is_ascii_digit() || ch == '_' {
            continue;
        }
        return match ch {
            '+' | 'P' => 1,
            '-' | 'N' => -1,
            _ => 0,
        };
    }
    0
}

/// Offsets a 45-degree polyline perpendicular by `offset` (signed: positive rotates each segment's direction -90 degrees --
/// clockwise, since board +y is down -- negative the other way), re-joining consecutive offset segments at their own line-line
/// meeting point rather than just translating every vertex by the same amount, which is what actually keeps corners mitered
/// instead of gapped/overlapping -- the standard "parallel curve of a polyline" construction. A run of exactly-collinear segments
/// (e.g. either side of a point that isn't a real corner) degenerates `intersect_lines` to `None`; the shared endpoint is used
/// directly in that case, which is exact for collinear input (the two offset segments are themselves exactly collinear too).
///
/// The length-tuning module builds its pair from this (`crate::meander`); the pair placer does not.
pub fn offset_polyline(centerline: &[Point], offset: i64) -> Vec<Point> {
    if centerline.len() < 2 {
        return centerline.to_vec();
    }
    let mut offset_segs: Vec<(Point, Point)> = Vec::with_capacity(centerline.len() - 1);
    for w in centerline.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = ((b.x - a.x).as_f64(), (b.y - a.y).as_f64());
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            continue; // a zero-length leg contributes no direction to offset by; skip it.
        }
        let (nx, ny) = (dy / len, -dx / len);
        let (ox, oy) = (Nm::from_raw_shim((nx * offset as f64).round() as i64), Nm::from_raw_shim((ny * offset as f64).round() as i64));
        offset_segs.push((Point { x: a.x + ox, y: a.y + oy }, Point { x: b.x + ox, y: b.y + oy }));
    }
    let Some(&(first_a, _)) = offset_segs.first() else { return centerline.to_vec() };
    let mut out = vec![first_a];
    for pair in offset_segs.windows(2) {
        let (s1a, s1b) = pair[0];
        let (s2a, s2b) = pair[1];
        out.push(crate::optimizer::intersect_lines(s1a, s1b, s2a, s2b).unwrap_or(s1b));
    }
    out.push(offset_segs.last().unwrap().1);
    out
}

// ------------------------------------------------------------------------------------------------
// small geometry: VECTOR2I, SEG and SHAPE_LINE_CHAIN as the pair code uses them
// ------------------------------------------------------------------------------------------------

/// A `VECTOR2I`.
pub type Vec2 = (Nm, Nm);

/// `DIRECTION_45::AngleType` is a bit mask in KiCad (`ANG_OBTUSE | ANG_STRAIGHT`), where [`AngleType`] is one value.
pub const ANG_OBTUSE: u32 = 0x01;
pub const ANG_RIGHT: u32 = 0x02;
pub const ANG_ACUTE: u32 = 0x04;
pub const ANG_STRAIGHT: u32 = 0x08;
pub const ANG_HALF_FULL: u32 = 0x10;
pub const ANG_UNDEFINED: u32 = 0x20;

/// The `DIRECTION_45::AngleType` bit of an [`AngleType`].
pub fn angle_bit(a: AngleType) -> u32 {
    match a {
        AngleType::Obtuse => ANG_OBTUSE,
        AngleType::Right => ANG_RIGHT,
        AngleType::Acute => ANG_ACUTE,
        AngleType::Straight => ANG_STRAIGHT,
        AngleType::HalfFull => ANG_HALF_FULL,
        AngleType::Undefined => ANG_UNDEFINED,
    }
}

/// `DIRECTION_45( vec )`.
pub fn direction_of(v: Vec2) -> Direction45 {
    Direction45::from_vector(v.0, v.1)
}

/// `DIRECTION_45( a ).Angle( DIRECTION_45( b ) )` as the bit it is.
fn angle_between(a: Vec2, b: Vec2) -> u32 {
    angle_bit(direction_of(a).angle_to(&direction_of(b)))
}

pub(crate) fn vsub(a: Point, b: Point) -> Vec2 {
    (a.x - b.x, a.y - b.y)
}

pub(crate) fn vadd(p: Point, v: Vec2) -> Point {
    Point { x: p.x + v.0, y: p.y + v.1 }
}

fn vneg(v: Vec2) -> Vec2 {
    (-v.0, -v.1)
}

/// `VECTOR2I::EuclideanNorm`.
pub(crate) fn vnorm(v: Vec2) -> Nm {
    Nm::from_raw_shim(seg_ops::length(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: v.0, y: v.1 }))
}

/// `VECTOR2I::Resize`.
pub(crate) fn vresize(v: Vec2, len: Nm) -> Vec2 {
    crate::hull::resize(v.0, v.1, len)
}

/// `VECTOR2I::Perpendicular`.
fn vperp(v: Vec2) -> Vec2 {
    (-v.1, v.0)
}

fn vdot(a: Vec2, b: Vec2) -> i128 {
    a.0.area(b.0) + a.1.area(b.1)
}

/// `( a + b ) / 2` on a `VECTOR2I`: integer division of each component, truncating toward zero.
fn midpoint(a: Point, b: Point) -> Point {
    Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
}

/// `SEG::SquaredLength`.
fn seg_sq_len(s: Seg2) -> i128 {
    let d = vsub(s.1, s.0);
    vdot(d, d)
}

/// `SEG::LineProject`.
pub(crate) fn line_project(s: Seg2, p: Point) -> Point {
    let d = vsub(s.1, s.0);
    let l_squared = vdot(d, d);
    if l_squared == 0 {
        return s.0;
    }
    let t = vdot(d, vsub(p, s.0));
    Point { x: s.0.x + Nm::new(rescale(t, d.0.get() as i128, l_squared) as i64), y: s.0.y + Nm::new(rescale(t, d.1.get() as i128, l_squared) as i64) }
}

/// `SEG::TCoef`.
fn t_coef(s: Seg2, p: Point) -> i128 {
    vdot(vsub(s.1, s.0), vsub(p, s.0))
}

/// `SEG::Distance( SEG )`.
pub(crate) fn seg_distance(a: Seg2, b: Seg2) -> Nm {
    Nm::new(isqrt(Seg::new(a.0, a.1).sq_distance_to_seg(&Seg::new(b.0, b.1))))
}

/// `SEG::mutualDistanceSquared`: the signed squared distances of the shorter segment's ends to the line of the longer.
fn mutual_distance_squared(s: Seg2, o: Seg2) -> Option<(i128, i128)> {
    let (a, b) = if seg_sq_len(s) < seg_sq_len(o) { (o, s) } else { (s, o) };
    let p = a.0.y.get() as i128 - a.1.y.get() as i128;
    let q = a.1.x.get() as i128 - a.0.x.get() as i128;
    let r = -p * a.0.x.get() as i128 - q * a.0.y.get() as i128;
    let l = p * p + q * q;
    if l == 0 {
        return None;
    }
    let det1 = p * b.0.x.get() as i128 + q * b.0.y.get() as i128 + r;
    let det2 = p * b.1.x.get() as i128 + q * b.1.y.get() as i128 + r;
    Some((det1.signum() * rescale(det1, det1, l), det2.signum() * rescale(det2, det2, l)))
}

/// `SEG::ApproxParallel( aSeg, aDistanceThreshold )`.
pub(crate) fn approx_parallel(s: Seg2, o: Seg2, threshold: Nm) -> bool {
    match mutual_distance_squared(s, o) {
        None => false,
        Some((d1, d2)) => (d1 - d2).abs() <= ((threshold).get() as i128) * ((threshold).get() as i128),
    }
}

/// `SHAPE_LINE_CHAIN::Append( chain )`: the points of `other`, without repeating the one `into` ends on.
fn append_chain(into: &mut Vec<Point>, other: &[Point]) {
    for &p in other {
        if into.last() != Some(&p) {
            into.push(p);
        }
    }
}

/// `SHAPE_LINE_CHAIN::SegmentCount` of an open chain.
pub(crate) fn segment_count(pts: &[Point]) -> usize {
    pts.len().saturating_sub(1)
}

/// `SHAPE_LINE_CHAIN::Length`: the sum of the rounded segment lengths.
pub(crate) fn chain_length(pts: &[Point]) -> i64 {
    pts.windows(2).map(|w| seg_ops::length(w[0], w[1])).sum()
}

/// `SHAPE_LINE_CHAIN::SelfIntersecting` for an open chain: some segment contains the start or end of a later one (the doubling-back
/// case, for a neighbour only through its end), or crosses it.
pub(crate) fn chain_self_intersecting(pts: &[Point]) -> bool {
    let n = segment_count(pts);
    for s1 in 0..n {
        let seg1 = (pts[s1], pts[s1 + 1]);
        for s2 in (s1 + 1)..n {
            let seg2 = (pts[s2], pts[s2 + 1]);
            if s1 + 1 != s2 && seg_ops::contains(seg1, seg2.0) {
                return true;
            }
            if seg_ops::contains(seg1, seg2.1) {
                return true;
            }
            if seg_ops::intersect(seg1, seg2, true, false).is_some() {
                return true;
            }
        }
    }
    false
}

/// `SHAPE_LINE_CHAIN::Intersects( chain )`: some segment of one touches or crosses some segment of the other.
pub(crate) fn chains_intersect(a: &[Point], b: &[Point]) -> bool {
    a.windows(2).any(|sa| b.windows(2).any(|sb| seg_ops::intersect((sa[0], sa[1]), (sb[0], sb[1]), false, false).is_some()))
}

/// `DIRECTION_45().BuildInitialTrace( a, b, start_diagonal )` in the default 45-degree mitered corner mode. Two equal points are one
/// point (`SHAPE_LINE_CHAIN::Append` does not repeat the last point), so a trace from a point to itself has no segment.
pub(crate) fn initial_trace(a: Point, b: Point, start_diagonal: bool) -> Vec<Point> {
    let mut pts = Direction45::Undefined.build_initial_trace(a, b, start_diagonal, CornerMode::Mitered45);
    pts.dedup();
    pts
}

// ------------------------------------------------------------------------------------------------
// a line of a pair, as the collision checks see it
// ------------------------------------------------------------------------------------------------

/// A line of a pair as the router checks it: its chain, net, layer and width, and the via it ends in if it does (`LINE`, with
/// `LINE::EndsWithVia`). Borrowed, so a candidate chain does not have to be copied into a [`Line`].
#[derive(Clone, Copy)]
pub struct PairLine<'a> {
    pub pts: &'a [Point],
    pub net: &'a Net,
    pub layer: i32,
    pub width: Nm,
    pub via: Option<&'a Via>,
}

impl PairLine<'_> {
    /// `NODE::CheckColliding( &line, aKindMask )`: some segment of the line, or its via, is within the clearance of an item of `node`
    /// of one of the kinds in `mask` ([`crate::node::kind_mask`]).
    pub fn hits_node(&self, node: &Node, rules: &BoardRules, mask: u8) -> bool {
        let opts = QueryOpts { kind_mask: mask, filter: None, use_epsilon: true, box_hull: false };
        let layers = LayerRange::single(self.layer);
        self.pts.windows(2).any(|w| !node.colliding_with(&Shape::Stadium { a: w[0], b: w[1], r: self.width / 2 }, self.net, layers, rules, &[], opts).is_empty())
            || self.via.is_some_and(|v| !node.colliding_with(&Shape::Circle { c: v.pos, r: v.diameter / 2 }, &v.net, v.layers, rules, &[], opts).is_empty())
    }

    /// `LINE::Collide( &other, aNode, aLayer )`: the two lines, and each one's via against the other, are within the rules' clearance of
    /// each other (no epsilon: `collideSimple` without a context). Lines of the same net never collide.
    pub fn hits_line(&self, other: &PairLine, rules: &BoardRules) -> bool {
        // a segment's shape and the box it can reach, for the pairs that are near enough to be worth the exact test
        let segs = |l: &PairLine, reach: Nm| {
            l.pts
                .windows(2)
                .map(|w| {
                    let shape = Shape::Stadium { a: w[0], b: w[1], r: l.width / 2 };
                    let bbox = shape.bbox(reach);
                    (shape, bbox)
                })
                .collect::<Vec<_>>()
        };
        let via_shape = |v: &Via| Shape::Circle { c: v.pos, r: v.diameter / 2 };
        if !same_net(self.net, other.net) && self.layer == other.layer {
            let clearance = Node::clearance(rules, self.net, other.net);
            let (mine, mut theirs) = (segs(self, clearance), segs(other, Nm::ZERO));
            // sweep along x: for each of mine, only the segments of theirs that start before it ends are candidates
            theirs.sort_by_key(|(_, b)| b.0);
            for (a, ab) in &mine {
                let reach = theirs.partition_point(|(_, b)| b.0 <= ab.2);
                if theirs[..reach].iter().any(|(b, bb)| bb.2 >= ab.0 && bb.3 >= ab.1 && bb.1 <= ab.3 && a.collides(b, clearance).is_some()) {
                    return true;
                }
            }
        }
        for (mine, theirs) in [(self, other), (other, self)] {
            if let Some(v) = mine.via {
                if !same_net(&v.net, theirs.net) && v.layers.overlaps_layer(theirs.layer) {
                    let clearance = Node::clearance(rules, &v.net, theirs.net);
                    if segs(theirs, Nm::ZERO).iter().any(|(s, _)| via_shape(v).collides(s, clearance).is_some()) {
                        return true;
                    }
                }
            }
        }
        if let (Some(a), Some(b)) = (self.via, other.via) {
            if !same_net(&a.net, &b.net) && a.layers.overlaps(&b.layers) && via_shape(a).collides(&via_shape(b), Node::clearance(rules, &a.net, &b.net)).is_some() {
                return true;
            }
        }
        false
    }
}

// ------------------------------------------------------------------------------------------------
// RANGED_NUM
// ------------------------------------------------------------------------------------------------

/// `RANGED_NUM<int>`: a value with a tolerance either way, which `Matches` a number within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RangedNum {
    pub value: Nm,
    pub tolerance_plus: Nm,
    pub tolerance_minus: Nm,
}

impl RangedNum {
    pub fn exact(value: Nm) -> Self {
        RangedNum { value, tolerance_plus: Nm::ZERO, tolerance_minus: Nm::ZERO }
    }

    pub fn matches(&self, other: Nm) -> bool {
        other >= self.value - self.tolerance_minus && other <= self.value + self.tolerance_plus
    }
}

/// `DIFF_PAIR::SetGap`: `RANGED_NUM<int>( m_gap, 10000, 10000 )`, 10 um either way.
pub const GAP_TOLERANCE_NM: Nm = Nm::from_raw_shim(10_000);

/// `checkGap`'s `SEG::Square( gap - 100 )`: 100 nm of slack.
pub const GAP_CHECK_SLACK: Nm = Nm::from_raw_shim(100);

/// `checkGap`: no segment of `p` is nearer to a segment of `n` than `gap` (less the slack).
fn check_gap(p: &[Point], n: &[Point], gap: Nm) -> bool {
    let gap_sq = ((gap - GAP_CHECK_SLACK).get() as i128).pow(2);
    for sp in p.windows(2) {
        for sn in n.windows(2) {
            if Seg::new(sp[0], sp[1]).sq_distance_to_seg(&Seg::new(sn[0], sn[1])) < gap_sq {
                return false;
            }
        }
    }
    true
}

// ------------------------------------------------------------------------------------------------
// DP_PRIMITIVE_PAIR
// ------------------------------------------------------------------------------------------------

/// What a pair starts or ends on, item by item: `DP_PRIMITIVE_PAIR`. Two pads, two vias or two track ends, each with the point the
/// pair's line is to start at (the item's own anchor, or the dangling end of a track), or just two points.
#[derive(Debug, Clone, Default)]
pub struct DpPrimitivePair {
    pub prim_p: Option<Item>,
    pub prim_n: Option<Item>,
    pub anchor_p: Point,
    pub anchor_n: Point,
}

/// `ITEM::Anchor( 0 )`.
fn anchor0(item: &Item) -> Point {
    item.anchors().first().copied().unwrap_or_default()
}

impl DpPrimitivePair {
    /// `DP_PRIMITIVE_PAIR( aPrimP, aPrimN )`: the anchors are the items' first.
    pub fn new(prim_p: Item, prim_n: Item) -> Self {
        let (anchor_p, anchor_n) = (anchor0(&prim_p), anchor0(&prim_n));
        DpPrimitivePair { prim_p: Some(prim_p), prim_n: Some(prim_n), anchor_p, anchor_n }
    }

    /// `DP_PRIMITIVE_PAIR( aAnchorP, aAnchorN )`: two points and no items.
    pub fn from_anchors(anchor_p: Point, anchor_n: Point) -> Self {
        DpPrimitivePair { prim_p: None, prim_n: None, anchor_p, anchor_n }
    }

    /// `DP_PRIMITIVE_PAIR::Directional`: the pair ends on track segments, which have a heading.
    pub fn directional(&self) -> bool {
        matches!(self.prim_p, Some(Item::Segment(_)))
    }

    /// `anchorDirection`: the heading of a track at the end it is joined at, pointing away from the track (the way the pair
    /// goes on); undefined for anything but a track.
    fn anchor_direction(item: &Item, p: Point) -> Direction45 {
        match item {
            Item::Segment(s) if s.a == p => direction_of(vsub(s.a, s.b)),
            Item::Segment(s) => direction_of(vsub(s.b, s.a)),
            _ => Direction45::Undefined,
        }
    }

    /// `DP_PRIMITIVE_PAIR::DirP`.
    pub fn dir_p(&self) -> Direction45 {
        self.prim_p.as_ref().map_or(Direction45::Undefined, |i| Self::anchor_direction(i, self.anchor_p))
    }

    /// `DP_PRIMITIVE_PAIR::DirN`.
    pub fn dir_n(&self) -> Direction45 {
        self.prim_n.as_ref().map_or(Direction45::Undefined, |i| Self::anchor_direction(i, self.anchor_n))
    }

    /// `DP_PRIMITIVE_PAIR::CursorOrientation`: the middle of the pair and the way it points to the cursor's side. Two parallel track
    /// segments point along themselves, anything else perpendicular to the line through the two anchors.
    pub fn cursor_orientation(&self, cursor: Point) -> (Point, Vec2) {
        let (Some(prim_p), Some(prim_n)) = (&self.prim_p, &self.prim_n) else { return (midpoint(self.anchor_p, self.anchor_n), (Nm::ZERO, Nm::ZERO)) };
        let (a_p, a_n);
        if let (Item::Segment(sp), Item::Segment(sn)) = (prim_p, prim_n) {
            a_p = sp.b;
            a_n = sn.b;
            // "If both segments are parallel, use that as the direction. Otherwise, fall back on the direction perpendicular to
            // the anchor points."
            if sp.b != sp.a && sn.b != sn.a && approx_parallel((sp.a, sp.b), (sn.a, sn.b), Nm::from_raw_shim(1)) {
                let mid = midpoint(a_p, a_n);
                return (mid, vresize(vsub(sp.b, sp.a), vnorm(vsub(a_p, a_n))));
            }
        } else {
            a_p = anchor0(prim_p);
            a_n = anchor0(prim_n);
        }
        let mid = midpoint(a_p, a_n);
        let mut dir = vperp(vsub(a_p, a_n));
        if vdot(dir, vsub(cursor, mid)) < 0 {
            dir = vneg(dir);
        }
        (mid, dir)
    }
}

// ------------------------------------------------------------------------------------------------
// DP_GATEWAY / DP_GATEWAYS
// ------------------------------------------------------------------------------------------------

/// `DP_GATEWAY`: two anchors the pair is to be routed to or from, how a line may enter it, how much it is worth, and optionally the
/// two lines that lead from the pads (or vias) to it.
#[derive(Debug, Clone)]
pub struct DpGateway {
    pub anchor_p: Point,
    pub anchor_n: Point,
    pub is_diagonal: bool,
    pub allowed_entry_angles: u32,
    pub priority: i32,
    pub entry_p: Vec<Point>,
    pub entry_n: Vec<Point>,
    pub has_entry_lines: bool,
}

impl DpGateway {
    /// `DP_GATEWAY( aAnchorP, aAnchorN, aIsDiagonal )`: obtuse entries only, priority 0.
    pub fn new(anchor_p: Point, anchor_n: Point, is_diagonal: bool) -> Self {
        Self::with(anchor_p, anchor_n, is_diagonal, ANG_OBTUSE, 0)
    }

    /// `DP_GATEWAY( aAnchorP, aAnchorN, aIsDiagonal, aAllowedEntryAngles, aPriority )`.
    pub fn with(anchor_p: Point, anchor_n: Point, is_diagonal: bool, allowed_entry_angles: u32, priority: i32) -> Self {
        DpGateway { anchor_p, anchor_n, is_diagonal, allowed_entry_angles, priority, entry_p: Vec::new(), entry_n: Vec::new(), has_entry_lines: false }
    }

    /// `DP_GATEWAY::SetEntryLines`.
    pub fn set_entry_lines(&mut self, entry_p: Vec<Point>, entry_n: Vec<Point>) {
        self.entry_p = entry_p;
        self.entry_n = entry_n;
        self.has_entry_lines = true;
    }

    /// `DP_GATEWAY::Reverse`: the entry lines run the other way.
    pub fn reverse(&mut self) {
        self.entry_n.reverse();
        self.entry_p.reverse();
    }

    /// `DP_GATEWAY::Entry`.
    pub fn entry(&self) -> DiffPair {
        DiffPair { p: self.entry_p.clone(), n: self.entry_n.clone(), ..DiffPair::default() }
    }
}

/// The gateways computed for the cursor or for a starting or ending primitive pair: `DP_GATEWAYS`.
#[derive(Debug, Clone)]
pub struct DpGateways {
    /// The pair's pitch, centre line to centre line (`gap() = DiffPairGap + DiffPairWidth`).
    gap: Nm,
    via_gap: Nm,
    via_diameter: Nm,
    fit_vias: bool,
    pub gateways: Vec<DpGateway>,
}

/// `makeGapVector`: the shortest vector along `dir` whose double is at least `length`.
fn make_gap_vector(dir: Vec2, length: Nm) -> Vec2 {
    let mut l = length / 2;
    if vnorm(dir) == Nm::ZERO {
        return dir;
    }
    for _ in 0..1_000_000 {
        let rv = vresize(dir, l);
        l += Nm::from_raw_shim(1);
        if vnorm((rv.0 * 2, rv.1 * 2)) >= length {
            return rv;
        }
    }
    vresize(dir, l)
}

impl DpGateways {
    /// `DP_GATEWAYS( aGap )`: as KiCad's, which fits vias until told not to.
    pub fn new(gap: Nm) -> Self {
        DpGateways { gap, via_gap: gap, via_diameter: Nm::ZERO, fit_vias: true, gateways: Vec::new() }
    }

    /// `DP_GATEWAYS::SetFitVias`: gateways for the cursor are for two vias `via_gap` apart (edge to edge) when `enable`.
    pub fn set_fit_vias(&mut self, enable: bool, diameter: Nm, via_gap: Option<Nm>) {
        self.fit_vias = enable;
        self.via_diameter = diameter;
        self.via_gap = via_gap.unwrap_or(self.gap);
    }

    /// `DP_GATEWAYS::BuildForCursor`: a ring of pairs of anchors around the cursor, the lines through them diagonal and straight.
    pub fn build_for_cursor(&mut self, cursor: Point) {
        let gap = if self.fit_vias { self.via_gap + self.via_diameter } else { self.gap };
        for diagonal in [false, true] {
            for i in 0..4 {
                let dir: Vec2 = if !diagonal {
                    let mut d = make_gap_vector((gap, gap), gap);
                    if i % 2 == 0 {
                        d.0 = -d.0;
                    }
                    if i / 2 == 0 {
                        d.1 = -d.1;
                    }
                    d
                } else if i / 2 == 0 {
                    ((gap + Nm::from_raw_shim(1)) / 2 * if i % 2 != 0 { -1 } else { 1 }, Nm::ZERO)
                } else {
                    (Nm::ZERO, (gap + Nm::from_raw_shim(1)) / 2 * if i % 2 != 0 { -1 } else { 1 })
                };
                if self.fit_vias {
                    self.build_generic(vadd(cursor, dir), vadd(cursor, vneg(dir)), true, true);
                } else {
                    self.gateways.push(DpGateway::new(vadd(cursor, dir), vadd(cursor, vneg(dir)), diagonal));
                }
            }
        }
    }

    /// `DP_GATEWAYS::FilterByOrientation`: drops the gateways whose anchors line up with `reference` by one of the angles in `mask`.
    pub fn filter_by_orientation(&mut self, mask: u32, reference: Direction45) {
        self.gateways.retain(|g| {
            let orient = direction_of(vsub(g.anchor_p, g.anchor_n));
            angle_bit(orient.angle_to(&reference)) & mask == 0
        });
    }

    /// `DP_GATEWAYS::checkDiagonalAlignment`: the two points are on a horizontal, vertical or diagonal line.
    fn check_diagonal_alignment(a: Point, b: Point) -> bool {
        let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
        (dx == Nm::ZERO && dy != Nm::ZERO) || dx == dy || (dy == Nm::ZERO && dx != Nm::ZERO)
    }

    /// `DP_GATEWAYS::buildEntries`: every gateway without entry lines gets the two lines that lead from `p0_p` and `p0_n` to it.
    fn build_entries(&mut self, p0_p: Point, p0_n: Point) {
        for g in &mut self.gateways {
            if !g.has_entry_lines {
                let mut lead_p = initial_trace(g.anchor_p, p0_p, g.is_diagonal);
                let mut lead_n = initial_trace(g.anchor_n, p0_n, g.is_diagonal);
                lead_p.reverse();
                lead_n.reverse();
                g.set_entry_lines(lead_p, lead_n);
            }
        }
    }

    /// `DP_GATEWAYS::buildDpContinuation`: the pair goes on from where two tracks end -- straight on, and, when they are aligned,
    /// at 22.5 degrees too (for a track 45 degrees off the existing direction).
    fn build_dp_continuation(&mut self, pair: &DpPrimitivePair, is_diagonal: bool) {
        let mut gw = DpGateway::new(pair.anchor_p, pair.anchor_n, is_diagonal);
        gw.priority = 100;
        self.gateways.push(gw);
        if !pair.directional() {
            return;
        }
        // "If we're at a "normal" angle (cardinal or 45-degree-diagonal), then add gateways that angle the anchor points by
        // 22.5-degrees for connection to tracks which are at +/- 45 degrees from the existing direction."
        const EPSILON: Nm = Nm::from_raw_shim(5);
        const SIN_22_5: f64 = 0.38268;
        const SIN_23_5: f64 = 0.39875;
        let add_angled = |this: &mut Self, length: Nm, priority: i32| {
            let ux = pair.dir_p().unit();
            let entry_line_p = vec![pair.anchor_p, vadd(pair.anchor_p, vresize((Nm::from_raw_shim(ux.0), Nm::from_raw_shim(ux.1)), length))];
            let mut gw_extend_p = DpGateway::new(*entry_line_p.last().unwrap(), pair.anchor_n, is_diagonal);
            gw_extend_p.priority = priority;
            gw_extend_p.set_entry_lines(entry_line_p, Vec::new());
            this.gateways.push(gw_extend_p);

            let un = pair.dir_n().unit();
            let entry_line_n = vec![pair.anchor_n, vadd(pair.anchor_n, vresize((Nm::from_raw_shim(un.0), Nm::from_raw_shim(un.1)), length))];
            let mut gw_extend_n = DpGateway::new(pair.anchor_p, *entry_line_n.last().unwrap(), is_diagonal);
            gw_extend_n.priority = priority;
            gw_extend_n.set_entry_lines(Vec::new(), entry_line_n);
            this.gateways.push(gw_extend_n);
        };
        let delta = vsub(pair.anchor_p, pair.anchor_n);
        if delta.0.abs() < EPSILON || delta.1.abs() < EPSILON || (delta.0 - delta.1).abs() < EPSILON {
            add_angled(self, Nm::from_raw_shim((self.gap.as_f64() * SIN_22_5).round() as i64), 20);
            // "fixme; sin(22.5) doesn't always work, so we also add some lower priority ones with a bit of wiggle room."
            add_angled(self, Nm::from_raw_shim((self.gap.as_f64() * SIN_23_5).round() as i64), 5);
        }
    }

    /// `DP_GATEWAYS::BuildGeneric`: gateways around a pair of points that need no particular fan-out -- straight out from between
    /// the two, diagonal and straight where the lines through them meet, and the weirder mixes of the two. In `via_mode` (the
    /// points are the centres of two vias that will fit) only the first two kinds.
    pub fn build_generic(&mut self, p0_p: Point, p0_n: Point, build_entries: bool, via_mode: bool) {
        const PAD_TO_GAP_THRESHOLD: Nm = Nm::from_raw_shim(3);
        let pad_dist = vnorm(vsub(p0_n, p0_p));
        let at = |p: Point, dx: Nm, dy: Nm| Point { x: p.x + dx, y: p.y + dy };
        let st_p: [Seg2; 2] = [(at(p0_p, Nm::from_raw_shim(-100), Nm::ZERO), at(p0_p, Nm::from_raw_shim(100), Nm::ZERO)), (at(p0_p, Nm::ZERO, Nm::from_raw_shim(-100)), at(p0_p, Nm::ZERO, Nm::from_raw_shim(100)))];
        let st_n: [Seg2; 2] = [(at(p0_n, Nm::from_raw_shim(-100), Nm::ZERO), at(p0_n, Nm::from_raw_shim(100), Nm::ZERO)), (at(p0_n, Nm::ZERO, Nm::from_raw_shim(-100)), at(p0_n, Nm::ZERO, Nm::from_raw_shim(100)))];
        let d_p: [Seg2; 2] = [(at(p0_p, Nm::from_raw_shim(-100), Nm::from_raw_shim(-100)), at(p0_p, Nm::from_raw_shim(100), Nm::from_raw_shim(100))), (at(p0_p, Nm::from_raw_shim(100), Nm::from_raw_shim(-100)), at(p0_p, Nm::from_raw_shim(-100), Nm::from_raw_shim(100)))];
        let d_n: [Seg2; 2] = [(at(p0_n, Nm::from_raw_shim(-100), Nm::from_raw_shim(-100)), at(p0_n, Nm::from_raw_shim(100), Nm::from_raw_shim(100))), (at(p0_n, Nm::from_raw_shim(100), Nm::from_raw_shim(-100)), at(p0_n, Nm::from_raw_shim(-100), Nm::from_raw_shim(100)))];
        let gap = self.gap;

        // midpoint exit & side-by exits
        for i in 0..2 {
            let straight_coll = seg_ops::collinear(st_p[i], st_n[i]);
            let diag_coll = seg_ops::collinear(d_p[i], d_n[i]);
            if straight_coll || diag_coll {
                let dir = make_gap_vector(vsub(p0_n, p0_p), gap / 2);
                let m = midpoint(p0_p, p0_n);
                let prio = if pad_dist > PAD_TO_GAP_THRESHOLD * gap.raw_shim() { 2 } else { 1 };
                if !via_mode {
                    self.gateways.push(DpGateway::with(vadd(m, vneg(dir)), vadd(m, dir), diag_coll, ANG_RIGHT, prio));

                    let dir = make_gap_vector(vsub(p0_n, p0_p), 2 * gap);
                    let perp = vperp(dir);
                    self.gateways.push(DpGateway::new(vadd(p0_p, vneg(dir)), vadd(vadd(p0_p, vneg(dir)), perp), diag_coll));
                    self.gateways.push(DpGateway::new(vadd(p0_p, vneg(dir)), vadd(vadd(p0_p, vneg(dir)), vneg(perp)), diag_coll));
                    self.gateways.push(DpGateway::new(vadd(vadd(p0_n, dir), perp), vadd(p0_n, dir), diag_coll));
                    self.gateways.push(DpGateway::new(vadd(vadd(p0_n, dir), vneg(perp)), vadd(p0_n, dir), diag_coll));
                }
            }
        }

        for i in 0..2 {
            for j in 0..2 {
                let mut ips: [Option<Point>; 2] = [seg_ops::intersect_lines(d_n[i], d_p[j]), seg_ops::intersect_lines(st_p[i], st_n[j])];
                if seg_ops::collinear(d_n[i], d_p[j]) {
                    ips[0] = None;
                }
                // KiCad tests `st_p[i]` against `st_p[j]` here, not `st_n[j]`: kept, it is what decides which crossings count
                if seg_ops::collinear(st_p[i], st_p[j]) {
                    ips[1] = None;
                }
                // diagonal-diagonal and straight-straight cases - the most typical case if the pads are on the same
                // straight/diagonal line
                for (k, ip) in ips.iter().enumerate() {
                    let Some(m) = *ip else { continue };
                    if m != p0_p && m != p0_n {
                        let prio = if pad_dist > PAD_TO_GAP_THRESHOLD * gap.raw_shim() { 10 } else { 20 };
                        let reach = Nm::from_raw_shim((gap.as_f64() * std::f64::consts::FRAC_1_SQRT_2).ceil() as i64);
                        let g_p = vresize(vsub(p0_p, m), reach);
                        let g_n = vresize(vsub(p0_n, m), reach);
                        self.gateways.push(DpGateway::with(vadd(m, g_p), vadd(m, g_n), k == 0, ANG_OBTUSE, prio));
                    }
                }

                let ips2: [Option<Point>; 2] = [seg_ops::intersect_lines(st_n[i], d_p[j]), seg_ops::intersect_lines(st_p[i], d_n[j])];
                // diagonal-straight cases: 8 possibilities of "weirder" exists
                for ip in ips2.iter() {
                    let Some(m) = *ip else { continue };
                    if !via_mode && m != p0_p && m != p0_n {
                        let mut g_p = vresize(vsub(p0_p, m), Nm::from_raw_shim((gap.as_f64() * std::f64::consts::SQRT_2).ceil() as i64));
                        let mut g_n = vresize(vsub(p0_n, m), gap);
                        if angle_between(g_p, g_n) != ANG_ACUTE {
                            self.gateways.push(DpGateway::new(vadd(m, g_p), vadd(m, g_n), true));
                        }
                        g_p = vresize(vsub(p0_p, m), gap);
                        g_n = vresize(vsub(p0_n, m), Nm::from_raw_shim((gap.as_f64() * std::f64::consts::SQRT_2).ceil() as i64));
                        if angle_between(g_p, g_n) != ANG_ACUTE {
                            self.gateways.push(DpGateway::new(vadd(m, g_p), vadd(m, g_n), true));
                        }
                    }
                }
            }
        }

        if build_entries {
            self.build_entries(p0_p, p0_n);
        }
    }

    /// `DP_GATEWAYS::BuildFromPrimitivePair`: the gateways of a pair of pads, vias or track ends. Pads that are lined up (on a
    /// horizontal, vertical or diagonal line) also get gateways with entry lines that fan the two nets out sideways, as far
    /// as the pad is long.
    pub fn build_from_primitive_pair(&mut self, pair: &DpPrimitivePair, prefer_diagonal: bool) {
        let (Some(prim_p), Some(prim_n)) = (&pair.prim_p, &pair.prim_n) else {
            self.build_generic(pair.anchor_p, pair.anchor_n, true, false);
            return;
        };
        let pad_or_via = |i: &Item| matches!(i, Item::Solid(_) | Item::Via(_));
        let track = |i: &Item| matches!(i, Item::Segment(_));
        let (p0_p, p0_n);
        let shape: Shape;
        if pad_or_via(prim_p) && pad_or_via(prim_n) {
            p0_p = pair.anchor_p;
            p0_n = pair.anchor_n;
            shape = prim_p.shape(prim_p.layers().start());
        } else if track(prim_p) && track(prim_n) {
            self.build_dp_continuation(pair, prefer_diagonal);
            return;
        } else {
            return;
        }
        let major_direction = vperp(vsub(p0_p, p0_n));
        let (ortho_fan_distance, diag_fan_distance);
        match &shape {
            Shape::Circle { .. } => {
                self.build_generic(p0_p, p0_n, true, false);
                return;
            }
            Shape::Rect { x0, y0, x1, y1 } => {
                let (mut w, mut h) = (*x1 - *x0, *y1 - *y0);
                if w < h {
                    std::mem::swap(&mut w, &mut h);
                }
                ortho_fan_distance = (w + Nm::from_raw_shim(1)) * 3 / 2;
                diag_fan_distance = w - h;
            }
            Shape::Stadium { a, b, r } => {
                let w = 2 * *r;
                let len = vnorm(vsub(*b, *a));
                ortho_fan_distance = w + len;
                diag_fan_distance = len;
            }
            other => {
                // SH_SIMPLE, SH_COMPOUND: the box
                let (x0, y0, x1, y1) = other.bbox(Nm::ZERO);
                let (mut w, mut h) = (x1 - x0, y1 - y0);
                if w < h {
                    std::mem::swap(&mut w, &mut h);
                }
                ortho_fan_distance = (w + Nm::from_raw_shim(1)) * 3 / 2;
                diag_fan_distance = w - h;
            }
        }
        if Self::check_diagonal_alignment(p0_p, p0_n) {
            let pad_dist = vnorm(vsub(p0_p, p0_n));
            for k in 0..2 {
                let dir = if k == 0 { make_gap_vector(major_direction, ortho_fan_distance) } else { make_gap_vector(major_direction, diag_fan_distance) };
                let d = Nm::ZERO.max(pad_dist - self.gap);
                let dp = make_gap_vector(dir, d);
                let dv = make_gap_vector(vsub(p0_n, p0_p), d);
                for i in 0..2 {
                    let sign: Nm = if i != 0 { Nm::from_raw_shim(-1) } else { Nm::from_raw_shim(1) };
                    let fan = (sign * (dir.0 + dp.0).raw_shim(), sign * (dir.1 + dp.1).raw_shim());
                    let gw_p = vadd(vadd(p0_p, fan), dv);
                    let gw_n = vadd(vadd(p0_n, fan), vneg(dv));
                    let entry_p = vec![p0_p, vadd(p0_p, (sign * dir.0.raw_shim(), sign * dir.1.raw_shim())), gw_p];
                    let entry_n = vec![p0_n, vadd(p0_n, (sign * dir.0.raw_shim(), sign * dir.1.raw_shim())), gw_n];
                    let mut gw = DpGateway::new(gw_p, gw_n, false);
                    gw.set_entry_lines(entry_p, entry_n);
                    gw.priority = 100 - k;
                    self.gateways.push(gw);
                }
            }
        }
        self.build_generic(p0_p, p0_n, true, false);
    }

    /// `DP_GATEWAYS::FitGateways`: the best pair of lines from one of these (the entry) gateways to one of `target`'s, scored by
    /// the gateways' priorities, with a bonus for the preferred posture. KiCad calls it as `gwsEntry.FitGateways( gwsEntry,
    /// gwsTarget, .. )`, the pitch being the entry set's. Leaves `dp` alone and returns false when no combination works.
    pub fn fit_gateways(&self, target: &DpGateways, pref_diagonal: bool, dp: &mut DiffPair) -> bool {
        let mut best: Option<(Vec<Point>, Vec<Point>)> = None;
        let mut best_score = -1000;
        for g_entry in &self.gateways {
            for g_target in &target.gateways {
                for preferred in [false, true] {
                    let score = if preferred { 0 } else { -3 } + g_entry.priority + g_target.priority;
                    if score >= best_score {
                        let mut l = DiffPair::with_gap(self.gap);
                        if l.build_initial(g_entry, g_target, if preferred { pref_diagonal } else { !pref_diagonal }) {
                            best = Some((l.p, l.n));
                            best_score = score;
                        }
                    }
                }
            }
        }
        match best {
            Some((p, n)) => {
                dp.set_gap(self.gap);
                dp.set_shape(p, n, false);
                true
            }
            None => false,
        }
    }
}

// ------------------------------------------------------------------------------------------------
// DIFF_PAIR
// ------------------------------------------------------------------------------------------------

/// `DIFF_PAIR::COUPLED_SEGMENTS`: where a segment of P and a segment of N run side by side at the gap.
#[derive(Debug, Clone)]
pub struct CoupledSegments {
    pub coupled_p: Seg2,
    pub coupled_n: Seg2,
    pub parent_p: Seg2,
    pub parent_n: Seg2,
    pub index_p: usize,
    pub index_n: usize,
}

/// A pair of lines: the two chains, the nets, the width and the gap they are routed with, and the vias they may end in (`DIFF_PAIR`).
#[derive(Debug, Clone, Default)]
pub struct DiffPair {
    pub p: Vec<Point>,
    pub n: Vec<Point>,
    pub net_p: Net,
    pub net_n: Net,
    pub layer: i32,
    pub width: Nm,
    /// The gap in force: edge to edge once the pair is placed, centre to centre while its gateways are being fitted.
    pub gap: Nm,
    pub gap_constraint: RangedNum,
    pub via_p: Option<Via>,
    pub via_n: Option<Via>,
}

impl DiffPair {
    /// `DIFF_PAIR( aGap )`: a gap that has to be matched exactly.
    pub fn with_gap(gap: Nm) -> Self {
        DiffPair { gap_constraint: RangedNum::exact(gap), ..DiffPair::default() }
    }

    /// `DIFF_PAIR::SetGap`: the gap, matched to within [`GAP_TOLERANCE_NM`].
    pub fn set_gap(&mut self, gap: Nm) {
        self.gap = gap;
        self.gap_constraint = RangedNum { value: gap, tolerance_plus: GAP_TOLERANCE_NM, tolerance_minus: GAP_TOLERANCE_NM };
    }

    /// `DIFF_PAIR::SetShape( aP, aN, aSwapLanes )`.
    pub fn set_shape(&mut self, p: Vec<Point>, n: Vec<Point>, swap_lanes: bool) {
        if swap_lanes {
            self.p = n;
            self.n = p;
        } else {
            self.p = p;
            self.n = n;
        }
    }

    /// `DIFF_PAIR::SetShape( aPair )`: only the chains.
    pub fn set_shape_from(&mut self, other: &DiffPair) {
        self.p = other.p.clone();
        self.n = other.n.clone();
    }

    /// `DIFF_PAIR::SetNets`.
    pub fn set_nets(&mut self, p: Net, n: Net) {
        self.net_p = p;
        self.net_n = n;
    }

    /// `DIFF_PAIR::AppendVias`.
    pub fn append_vias(&mut self, via_p: Via, via_n: Via) {
        self.via_p = Some(via_p);
        self.via_n = Some(via_n);
    }

    /// `DIFF_PAIR::RemoveVias`.
    pub fn remove_vias(&mut self) {
        self.via_p = None;
        self.via_n = None;
    }

    /// `DIFF_PAIR::EndsWithVias`.
    pub fn ends_with_vias(&self) -> bool {
        self.via_p.is_some() && self.via_n.is_some()
    }

    /// `DIFF_PAIR::Empty`: either line has no segment.
    pub fn is_empty(&self) -> bool {
        segment_count(&self.n) == 0 || segment_count(&self.p) == 0
    }

    /// P as a checkable line, with its via if the pair ends in vias.
    pub fn p_pair_line(&self) -> PairLine<'_> {
        PairLine { pts: &self.p, net: &self.net_p, layer: self.layer, width: self.width, via: self.via_p.as_ref() }
    }

    /// N as a checkable line, with its via if the pair ends in vias.
    pub fn n_pair_line(&self) -> PairLine<'_> {
        PairLine { pts: &self.n, net: &self.net_n, layer: self.layer, width: self.width, via: self.via_n.as_ref() }
    }

    /// `DIFF_PAIR::PLine`, without the via.
    pub fn p_line(&self) -> Line {
        Line::from_points(self.net_p.clone(), self.layer, self.width, self.p.clone())
    }

    /// `DIFF_PAIR::NLine`, without the via.
    pub fn n_line(&self) -> Line {
        Line::from_points(self.net_n.clone(), self.layer, self.width, self.n.clone())
    }

    /// `DIFF_PAIR::EndingPrimitives`: what the pair ends on, to go on from -- the two vias, or the last segment of each line.
    pub fn ending_primitives(&self) -> DpPrimitivePair {
        if let (Some(vp), Some(vn)) = (&self.via_p, &self.via_n) {
            return DpPrimitivePair::new(Item::Via(vp.clone()), Item::Via(vn.clone()));
        }
        let seg = |pts: &[Point], net: &Net| Segment { net: net.clone(), layer: self.layer, a: pts[pts.len() - 2], b: pts[pts.len() - 1], width: self.width, source_track: None, locked: false };
        let (sp, sn) = (seg(&self.p, &self.net_p), seg(&self.n, &self.net_n));
        let (b_p, b_n) = (sp.b, sn.b);
        let mut pair = DpPrimitivePair::new(Item::Segment(sp), Item::Segment(sn));
        pair.anchor_p = b_p;
        pair.anchor_n = b_n;
        pair
    }

    /// `DIFF_PAIR::CheckConnectionAngle`: the last segment of this pair's lines turns into the first of `other`'s by one of the
    /// angles in `allowed` (a line with no segment passes).
    pub fn check_connection_angle(&self, other: &DiffPair, allowed: u32) -> bool {
        let check = |mine: &[Point], theirs: &[Point]| {
            if segment_count(mine) == 0 || segment_count(theirs) == 0 {
                return true;
            }
            let d0 = Direction45::from_seg(mine[mine.len() - 2], mine[mine.len() - 1]);
            let d1 = Direction45::from_seg(theirs[0], theirs[1]);
            angle_bit(d0.angle_to(&d1)) & allowed != 0
        };
        check(&self.p, &other.p) && check(&self.n, &other.n)
    }

    /// `DIFF_PAIR::BuildInitial`: the lines from `entry` to `target` -- each the 45-degree trace between its anchors, with the
    /// gateways' entry lines at either end -- if they keep the gap, do not cross each other or themselves, and turn only by the
    /// angles the gateways allow.
    pub fn build_initial(&mut self, entry: &DpGateway, target: &DpGateway, pref_diagonal: bool) -> bool {
        let p = initial_trace(entry.anchor_p, target.anchor_p, pref_diagonal);
        let n = initial_trace(entry.anchor_n, target.anchor_n, pref_diagonal);

        let mask = entry.allowed_entry_angles | ANG_STRAIGHT | ANG_OBTUSE;
        self.p = p.clone();
        self.n = n.clone();
        if entry.has_entry_lines {
            let e = entry.entry();
            if !e.check_connection_angle(self, mask) {
                return false;
            }
            self.p = e.p;
            self.n = e.n;
            append_chain(&mut self.p, &p);
            append_chain(&mut self.n, &n);
        } else {
            self.p = p.clone();
            self.n = n.clone();
        }

        let mask = target.allowed_entry_angles | ANG_STRAIGHT | ANG_OBTUSE;
        if target.has_entry_lines {
            let mut t = target.clone();
            t.reverse();
            let te = t.entry();
            if !self.check_connection_angle(&te, mask) {
                return false;
            }
            append_chain(&mut self.p, &te.p);
            append_chain(&mut self.n, &te.n);
        }

        if !check_gap(&p, &n, self.gap_constraint.value) {
            return false;
        }
        if chain_self_intersecting(&p) || chain_self_intersecting(&n) {
            return false;
        }
        !chains_intersect(&p, &n)
    }

    /// `DIFF_PAIR::Skew`: P is longer than N by.
    pub fn skew(&self) -> f64 {
        (chain_length(&self.p) - chain_length(&self.n)) as f64
    }

    /// `DIFF_PAIR::TotalLength`: the average of the two lines.
    pub fn total_length(&self) -> f64 {
        (chain_length(&self.p) + chain_length(&self.n)) as f64 / 2.0
    }

    /// `DIFF_PAIR::CoupledSegmentPairs`: every segment of P with every segment of N that runs beside it at the gap.
    pub fn coupled_segment_pairs(&self) -> Vec<CoupledSegments> {
        let mut out = Vec::new();
        for (i, sp) in self.p.windows(2).enumerate() {
            let sp = (sp[0], sp[1]);
            for (j, sn) in self.n.windows(2).enumerate() {
                let sn = (sn[0], sn[1]);
                let dist = (seg_distance(sp, sn) - self.width).abs();
                if approx_parallel(sp, sn, Nm::from_raw_shim(2)) && self.gap_constraint.matches(dist) {
                    if let Some((p_clip, n_clip)) = common_parallel_projection(sp, sn) {
                        out.push(CoupledSegments { coupled_p: p_clip, coupled_n: n_clip, parent_p: sp, parent_n: sn, index_p: i, index_n: j });
                    }
                }
            }
        }
        out
    }

    /// `DIFF_PAIR::CoupledLength()`: the length over which the two lines run side by side.
    pub fn coupled_length(&self) -> f64 {
        self.coupled_segment_pairs().iter().map(|c| seg_ops::length(c.coupled_p.0, c.coupled_p.1) as f64).sum()
    }

    /// `DIFF_PAIR::CoupledLength( aP, aN )`: the same for two chains, with this pair's width and gap.
    pub fn coupled_length_of(&self, p: &[Point], n: &[Point]) -> i64 {
        let mut total = 0;
        for sp in p.windows(2) {
            let sp = (sp[0], sp[1]);
            for sn in n.windows(2) {
                let sn = (sn[0], sn[1]);
                total += self.coupled_length_of_segs(sp, sn);
            }
        }
        total
    }

    /// `DIFF_PAIR::CoupledLength( aP, aN )` for two segments.
    pub fn coupled_length_of_segs(&self, sp: Seg2, sn: Seg2) -> i64 {
        let dist = (seg_distance(sp, sn) - self.width).abs();
        if approx_parallel(sp, sn, Nm::from_raw_shim(1)) && self.gap_constraint.matches(dist) {
            if let Some((p_clip, _)) = common_parallel_projection(sp, sn) {
                return seg_ops::length(p_clip.0, p_clip.1);
            }
        }
        0
    }

    /// `DIFF_PAIR::CoupledLengthFactor`.
    pub fn coupled_length_factor(&self) -> f64 {
        let t = self.total_length();
        if t == 0.0 {
            0.0
        } else {
            self.coupled_length() / t
        }
    }
}

/// `commonParallelProjection`: the parts of two parallel segments that face each other -- the stretch of `p` that `n` projects
/// onto, and the stretch of `n` that is the projection of that. `None` when they do not overlap.
pub(crate) fn common_parallel_projection(p: Seg2, n: Seg2) -> Option<(Seg2, Seg2)> {
    let n_proj_p = (line_project(p, n.0), line_project(p, n.1));
    let (mut t_a, mut t_b) = (0i128, t_coef(p, p.1));
    let (mut tproj_a, mut tproj_b) = (t_coef(p, n_proj_p.0), t_coef(p, n_proj_p.1));
    if t_b < t_a {
        std::mem::swap(&mut t_a, &mut t_b);
    }
    if tproj_b < tproj_a {
        std::mem::swap(&mut tproj_a, &mut tproj_b);
    }
    if t_b <= tproj_a || t_a >= tproj_b {
        return None;
    }
    let mut tv = [0i128, t_coef(p, p.1), t_coef(p, n_proj_p.0), t_coef(p, n_proj_p.1)];
    tv.sort_unstable(); // "fixme: awful and disgusting way of finding 2 midpoints"
    let p_len_sq = seg_sq_len(p);
    let dp = vsub(p.1, p.0);
    let clip = |t: i128| Point { x: p.0.x + Nm::new(rescale(dp.0.get() as i128, t, p_len_sq) as i64), y: p.0.y + Nm::new(rescale(dp.1.get() as i128, t, p_len_sq) as i64) };
    let p_clip = (clip(tv[1]), clip(tv[2]));
    let n_clip = (line_project(n, p_clip.0), line_project(n, p_clip.1));
    Some((p_clip, n_clip))
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    #[test]
    fn dp_coupled_net_name_matches_every_upstream_convention() {
        assert_eq!(dp_coupled_net_name("USB_DP"), Some("USB_DN".to_string()));
        assert_eq!(dp_coupled_net_name("USB_DN"), Some("USB_DP".to_string()));
        assert_eq!(dp_coupled_net_name("CLK+"), Some("CLK-".to_string()));
        assert_eq!(dp_coupled_net_name("CLK-"), Some("CLK+".to_string()));
        assert_eq!(dp_coupled_net_name("LVDS_P0"), Some("LVDS_N0".to_string()), "trailing digits after the suffix letter must survive");
        assert_eq!(dp_coupled_net_name("LVDS_N12"), Some("LVDS_P12".to_string()));
        assert_eq!(dp_coupled_net_name("GND"), None, "no recognized suffix at all");
        assert_eq!(dp_coupled_net_name(""), None);
        assert_eq!(dp_coupled_net_name("P"), Some("N".to_string()), "the whole name can be just the suffix letter");
    }

    #[test]
    fn the_polarity_is_matchdpsuffixs_return_value() {
        assert_eq!((dp_net_polarity("USB_DP"), dp_net_polarity("USB_DN")), (1, -1));
        assert_eq!((dp_net_polarity("/D+"), dp_net_polarity("/D-")), (1, -1));
        assert_eq!((dp_net_polarity("LVDS_P0"), dp_net_polarity("LVDS_N12")), (1, -1), "digits and underscores after the letter are skipped");
        assert_eq!((dp_net_polarity("GND"), dp_net_polarity(""), dp_net_polarity("SDA")), (0, 0, 0));
    }

    #[test]
    fn offset_polyline_keeps_a_straight_line_straight() {
        let centerline = vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)];
        let offset = offset_polyline(&centerline, 100_000);
        // E direction: perpendicular (dy,-dx)/len = (0,-1) for a +100 offset -- shifts "up" (negative y).
        assert_eq!(offset, vec![p(Nm::ZERO, mm(-0.1)), p(mm(1.0), mm(-0.1))]);
    }

    #[test]
    fn offset_polyline_miters_a_corner_instead_of_gapping_it() {
        // A 90-degree corner (E then S).
        let centerline = vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO), p(mm(1.0), mm(1.0))];
        let offset = offset_polyline(&centerline, 100_000);
        assert_eq!(offset.len(), 3, "still exactly one corner, not a gap/overlap at the join");
        let (a, b, c) = (offset[0], offset[1], offset[2]);
        assert_eq!(a.y, b.y, "first offset leg stays horizontal");
        assert_eq!(b.x, c.x, "second offset leg stays vertical");
    }

    #[test]
    fn a_gap_vector_is_the_shortest_that_doubles_to_the_length() {
        // along a diagonal: (a, a) doubled is 2a * sqrt 2 = 299_999.4 for a = 106_066, which `EuclideanNorm` rounds to 300_000
        let v = make_gap_vector((mm(0.3), mm(0.3)), mm(0.3));
        assert_eq!(v, (Nm::new(106_066), Nm::new(106_066)));
        assert!(vnorm((v.0 * 2, v.1 * 2)) >= mm(0.3) && vnorm((v.0 * 2 - Nm::new(2), v.1 * 2 - Nm::new(2))) < mm(0.3));
        assert_eq!(make_gap_vector((Nm::ZERO, Nm::ZERO), mm(0.3)), (Nm::ZERO, Nm::ZERO));
        assert_eq!(make_gap_vector((Nm::new(-5000), Nm::ZERO), mm(0.3)), (mm(-0.15), Nm::ZERO));
        assert_eq!(make_gap_vector((Nm::new(7000), Nm::ZERO), Nm::ZERO), (Nm::ZERO, Nm::ZERO));
    }

    #[test]
    fn two_pads_on_a_row_get_the_gateways_straight_out_and_diagonal_from_between_them() {
        // pitch 300, pads 1000 apart on a row: KiCad's `BuildGeneric` gives the midpoint exits (priority 2: far apart), the
        // side-by exits, and the diagonal/straight crossings
        let mut gws = DpGateways::new(mm(0.3));
        gws.build_generic(p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO), true, false);
        assert!(!gws.gateways.is_empty());
        assert!(gws.gateways.iter().all(|g| g.has_entry_lines), "build_entries gave every one its entry lines");
        let mid = gws.gateways.iter().find(|g| g.priority == 2 && g.allowed_entry_angles == ANG_RIGHT).expect("the midpoint exit");
        // centred between the pads. KiCad builds it from `makeGapVector( p0_n - p0_p, m_gap / 2 )`, so its anchors are half a pitch
        // apart, not a pitch: `checkGap` refuses every pair of lines that starts there (kept as it is)
        assert_eq!((mid.anchor_p.x, mid.anchor_n.x), (mm(0.425), mm(0.575)));
        assert_eq!(mid.anchor_p.y, mid.anchor_n.y);
        // every straight-straight crossing is one pad's horizontal with the other's vertical: priority 10 (pads far apart)
        assert!(gws.gateways.iter().any(|g| g.priority == 10));
        // every other gateway's two anchors are at least the gap (less rounding) apart
        for g in gws.gateways.iter().filter(|g| g.allowed_entry_angles != ANG_RIGHT) {
            assert!(vnorm(vsub(g.anchor_p, g.anchor_n)) >= Nm::from_raw_shim(300_000 - 3000), "{g:?}");
        }
    }

    #[test]
    fn via_mode_leaves_out_the_midpoint_and_mixed_gateways() {
        let mut plain = DpGateways::new(mm(0.3));
        plain.build_generic(p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(0.6)), true, false);
        let mut vias = DpGateways::new(mm(0.3));
        vias.build_generic(p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(0.6)), true, true);
        assert!(vias.gateways.len() < plain.gateways.len());
        assert!(vias.gateways.iter().all(|g| g.priority >= 10), "only the crossings of the lines through the two vias");
    }

    #[test]
    fn the_cursor_gets_eight_gateways_a_gap_apart() {
        let mut gws = DpGateways::new(mm(0.3));
        gws.set_fit_vias(false, Nm::ZERO, None);
        gws.build_for_cursor(p(mm(5.0), mm(5.0)));
        assert_eq!(gws.gateways.len(), 8, "four diagonal pairs and four straight ones");
        for g in &gws.gateways {
            let mid = midpoint(g.anchor_p, g.anchor_n);
            assert_eq!(mid, p(mm(5.0), mm(5.0)), "centred on the cursor");
            assert!(vnorm(vsub(g.anchor_p, g.anchor_n)) >= mm(0.3), "a pitch apart");
        }
        // with vias: the pitch is the via gap plus a via
        let mut v = DpGateways::new(mm(0.3));
        v.set_fit_vias(true, mm(0.6), Some(mm(0.25)));
        v.build_for_cursor(p(mm(5.0), mm(5.0)));
        assert!(!v.gateways.is_empty());
        assert!(v.gateways.iter().all(|g| g.has_entry_lines));
    }

    #[test]
    fn filtering_by_orientation_drops_the_gateways_that_line_up_with_the_reference() {
        let mut gws = DpGateways::new(mm(0.3));
        gws.set_fit_vias(false, Nm::ZERO, None);
        gws.build_for_cursor(p(Nm::ZERO, Nm::ZERO));
        // pairs lined up straight or reversed with a heading north: the vertical offsets (ANG_STRAIGHT | ANG_HALF_FULL)
        gws.filter_by_orientation(ANG_STRAIGHT | ANG_HALF_FULL, Direction45::N);
        assert_eq!(gws.gateways.len(), 6);
        for g in &gws.gateways {
            let d = direction_of(vsub(g.anchor_p, g.anchor_n));
            assert!(d != Direction45::N && d != Direction45::S);
        }
    }

    #[test]
    fn a_straight_pair_between_two_pad_pairs_keeps_the_gap_the_whole_way() {
        // two pads at the start, 600 apart, and two at the end: pitch 300
        let (start, end) = (DpPrimitivePair::from_anchors(p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(0.6))), DpPrimitivePair::from_anchors(p(mm(10.0), Nm::ZERO), p(mm(10.0), mm(0.6))));
        let mut entry = DpGateways::new(mm(0.3));
        entry.build_from_primitive_pair(&start, false);
        let mut target = DpGateways::new(mm(0.3));
        target.build_from_primitive_pair(&end, false);
        let mut dp = DiffPair::default();
        assert!(entry.fit_gateways(&target, false, &mut dp));
        assert_eq!((dp.p.first(), dp.n.first()), (Some(&p(Nm::ZERO, Nm::ZERO)), Some(&p(Nm::ZERO, mm(0.6)))));
        assert_eq!((dp.p.last(), dp.n.last()), (Some(&p(mm(10.0), Nm::ZERO)), Some(&p(mm(10.0), mm(0.6)))));
        // every segment of P keeps the pitch from every segment of N
        assert!(check_gap(&dp.p, &dp.n, mm(0.3)));
        assert!(!chains_intersect(&dp.p, &dp.n));
        // the two lines are coupled for most of their length once they run at the pitch
        dp.width = mm(0.1);
        dp.set_gap(mm(0.2));
        assert!(dp.coupled_length() > 6_000_000.0, "coupled {}", dp.coupled_length());
    }

    #[test]
    fn the_gap_check_is_the_pitch_less_100_nm() {
        let p_line = [p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)];
        assert!(check_gap(&p_line, &[p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.3))], mm(0.3)));
        assert!(check_gap(&p_line, &[p(Nm::ZERO, Nm::new(299_900)), p(mm(1.0), Nm::new(299_900))], mm(0.3)), "100 nm of slack");
        assert!(!check_gap(&p_line, &[p(Nm::ZERO, Nm::new(299_899)), p(mm(1.0), Nm::new(299_899))], mm(0.3)));
        assert!(!check_gap(&p_line, &[p(mm(0.5), mm(0.1)), p(mm(0.5), mm(-0.1))], mm(0.3)), "crossing");
    }

    #[test]
    fn connection_angles_are_checked_per_line_against_the_allowed_mask() {
        let mut a = DiffPair::default();
        a.p = vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)];
        a.n = vec![p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.3))];
        let mut straight = DiffPair::default();
        straight.p = vec![p(mm(1.0), Nm::ZERO), p(mm(2.0), Nm::ZERO)];
        straight.n = vec![p(mm(1.0), mm(0.3)), p(mm(2.0), mm(0.3))];
        let mut obtuse = DiffPair::default();
        obtuse.p = vec![p(mm(1.0), Nm::ZERO), p(mm(2.0), mm(1.0))];
        obtuse.n = vec![p(mm(1.0), mm(0.3)), p(mm(2.0), mm(1.3))];
        let mut right = DiffPair::default();
        right.p = vec![p(mm(1.0), Nm::ZERO), p(mm(1.0), mm(1.0))];
        right.n = vec![p(mm(1.0), mm(0.3)), p(mm(1.0), mm(1.3))];
        assert!(a.check_connection_angle(&straight, ANG_STRAIGHT));
        assert!(!a.check_connection_angle(&straight, ANG_OBTUSE));
        assert!(a.check_connection_angle(&obtuse, ANG_OBTUSE));
        assert!(!a.check_connection_angle(&right, ANG_STRAIGHT | ANG_OBTUSE));
        assert!(a.check_connection_angle(&right, ANG_RIGHT));
        // a line with no segment always passes
        assert!(a.check_connection_angle(&DiffPair::default(), 0));
    }

    #[test]
    fn two_parallel_tracks_point_the_pair_along_themselves_and_two_pads_across() {
        let seg = |net: &str, a: Point, b: Point| Item::Segment(Segment { net: crate::item::net_of(net), layer: 0, a, b, width: mm(0.1), source_track: None, locked: false });
        let tracks = DpPrimitivePair::new(seg("P", p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)), seg("N", p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.3))));
        let (mid, dir) = tracks.cursor_orientation(p(mm(5.0), mm(5.0)));
        assert_eq!(mid, p(mm(1.0), mm(0.15)));
        assert_eq!(dir, (mm(0.3), Nm::ZERO), "the heading of the tracks, as long as the pair is wide");
        // not parallel: perpendicular to the line through the two ends, on the cursor's side
        let skew = DpPrimitivePair::new(seg("P", p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)), seg("N", p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.6))));
        let (_, dir) = skew.cursor_orientation(p(mm(5.0), Nm::ZERO));
        assert_eq!(dir, (mm(0.6), Nm::ZERO), "perpendicular to the line through the two ends (0, -600), turned toward the cursor");
        let (_, away) = skew.cursor_orientation(p(mm(-5.0), Nm::ZERO));
        assert_eq!(away, (mm(-0.6), Nm::ZERO));
        let pads = DpPrimitivePair::from_anchors(p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(0.3)));
        assert_eq!(pads.cursor_orientation(p(mm(5.0), Nm::ZERO)).1, (Nm::ZERO, Nm::ZERO), "no items, no orientation");
    }

    #[test]
    fn the_ending_primitives_of_a_pair_are_its_last_segments_with_the_anchors_at_their_ends() {
        let mut dp = DiffPair::default();
        dp.p = vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO), p(mm(2.0), mm(1.0))];
        dp.n = vec![p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.3)), p(mm(2.0), mm(1.3))];
        dp.set_nets(crate::item::net_of("P"), crate::item::net_of("N"));
        dp.width = mm(0.1);
        let e = dp.ending_primitives();
        assert_eq!((e.anchor_p, e.anchor_n), (p(mm(2.0), mm(1.0)), p(mm(2.0), mm(1.3))));
        assert!(e.directional());
        assert_eq!(e.dir_p(), Direction45::SE, "the heading the pair goes on in");
    }

    #[test]
    fn coupled_length_counts_only_the_stretches_beside_each_other_at_the_gap() {
        let mut dp = DiffPair::default();
        dp.width = mm(0.1);
        dp.set_gap(mm(0.2));
        // P along y = 0, N along y = 300 for x in 0..1000 (centre pitch 300 = width 100 + gap 200), then N bends away
        dp.p = vec![p(Nm::ZERO, Nm::ZERO), p(mm(2.0), Nm::ZERO)];
        dp.n = vec![p(Nm::ZERO, mm(0.3)), p(mm(1.0), mm(0.3)), p(mm(2.0), mm(1.3))];
        let coupled = dp.coupled_length();
        assert!((coupled - 1_000_000.0).abs() <= 1.0, "{coupled}");
        // the lengths: P 2000, N 1000 + 1414 = 2414
        assert!((dp.skew() - (1_000_000.0 - 1_000_000.0 * std::f64::consts::SQRT_2)).abs() <= 1.0, "{}", dp.skew());
    }
}
