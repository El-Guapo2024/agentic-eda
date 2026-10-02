//! Differential-pair length tuning -- keys `8` (`pcbnew.LengthTuner.TuneDiffPair`) and `9`
//! (`pcbnew.LengthTuner.TuneDiffPairSkew`), the two siblings of the single-track tuner in
//! [`crate::meander`] (key `7`).
//!
//! * **`8`, `PNS::DP_MEANDER_PLACER`** (`pns_dp_meander_placer.cpp`): the clicked line's coupled partner is found
//!   (`TOPOLOGY::AssembleDiffPair`: the track of the complementary net -- [`dp_coupled_net_name`], i.e.
//!   `BOARD::MatchDpSuffix` -- that runs alongside), one meander is built along the *midline* of the coupled
//!   segments (`baselineSegment`) and each line is that shape shifted sideways by half the pair's pitch
//!   (`(gap + width) / 2`, `SetBaselineOffset`). The pair's length is `max( P, N )` over the whole nets
//!   (`origPathLength`), and the meander lengthens it to the target (`tuneLineLength`). The two replaced tracks
//!   come out as one `CommitRoute`.
//! * **`9`, `PNS::MEANDER_SKEW_PLACER`** (`pns_meander_skew_placer.cpp`): not a pair operation at all -- it is the
//!   single-line meander placer lengthening the *selected* line until its length equals the partner's
//!   (`m_coupledLength`) plus the target skew (`doMove( aP, aEndItem, m_coupledLength + m_settings.m_targetSkew.
//!   Opt(), ... )`). Skew is `own - coupled`.
//!
//! Scope, the same as [`crate::meander`] (see there for why this is dialog-driven rather than a live mouse
//! session): the tuned stretch is a **single straight axis-aligned run per line** -- both lines of the pair
//! being one 2-point track each, side by side on one layer -- and the lengths are the whole nets' routed copper
//! (`lineLength` over `AssembleTuningPath`, without pad-to-die lengths or via barrels, which this model has no
//! data for). The pure geometry lives here so it is testable without a board on disk; `crates/cli/src/
//! tune_api.rs` is the collision check, the JSON and the commit.

use crate::diff_pair::dp_coupled_net_name;
use crate::meander::{generate_dp_meander, generate_meander, polyline_length, simplify_collinear};
use eda_model::ir::{Point, Track, Um};

/// `DP_MEANDER_PLACER::Start`'s own failure reason for a track whose net has no complement.
pub const DP_NAME_HINT: &str = "Unable to find complementary differential pair net for length tuning. Make sure the names of the nets belonging to a differential pair end with either _N/_P or +/-.";

/// `MEANDER_SKEW_PLACER::Start`'s.
pub const SKEW_NAME_HINT: &str = "Unable to find complementary differential pair net for skew tuning. Make sure the names of the nets belonging to a differential pair end with either _N/_P or +/-.";

const SINGLE_STRAIGHT_HINT: &str = "select a straight track with no corners or vias -- this port's length tuner doesn't yet handle a multi-segment run";

/// Routed copper of `net`: every track on every layer (`lineLength` over the assembled tuning path).
pub fn net_length(tracks: &[Track], net: &str) -> Um {
    tracks.iter().filter(|t| t.net == net).map(|t| polyline_length(&t.pts)).sum()
}

/// The direction a straight run points, as a unit step along +x or +y, with the sideways (`n = (uy, -ux)`, the
/// `offset_polyline` sign convention) and along coordinates it implies. Everything below works in those two
/// coordinates so a horizontal and a vertical pair share one code path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Axis {
    ux: Um,
    uy: Um,
}

impl Axis {
    /// `Some` for a track that is exactly one axis-aligned segment.
    fn of(t: &Track) -> Option<Axis> {
        let [a, b] = t.pts[..] else { return None };
        match (a.x == b.x, a.y == b.y) {
            (false, true) => Some(Axis { ux: 1, uy: 0 }),
            (true, false) => Some(Axis { ux: 0, uy: 1 }),
            _ => None,
        }
    }
    fn along(self, p: Point) -> Um {
        p.x * self.ux + p.y * self.uy
    }
    fn lat(self, p: Point) -> Um {
        p.x * self.uy - p.y * self.ux
    }
    fn point(self, along: Um, lat: Um) -> Point {
        Point { x: self.ux * along + self.uy * lat, y: self.uy * along - self.ux * lat }
    }
}

fn along_range(t: &Track, axis: Axis) -> (Um, Um) {
    let (a, b) = (axis.along(t.pts[0]), axis.along(t.pts[1]));
    (a.min(b), a.max(b))
}

/// The partner line: a one-segment track of `partner_net` on the same layer, parallel to `a`, beside it (not on
/// top of it) and overlapping it along the run -- the nearest such, then the longest overlap.
fn find_partner<'a>(tracks: &'a [Track], a: &Track, axis: Axis, partner_net: &str) -> Option<&'a Track> {
    let (alo, ahi) = along_range(a, axis);
    let lat_a = axis.lat(a.pts[0]);
    tracks
        .iter()
        .filter(|t| t.id != a.id && t.net == partner_net && t.layer == a.layer && Axis::of(t) == Some(axis))
        .filter_map(|t| {
            let (lo, hi) = along_range(t, axis);
            let overlap = ahi.min(hi) - alo.max(lo);
            let sep = (axis.lat(t.pts[0]) - lat_a).abs();
            (overlap > 0 && sep > 0).then_some((sep, -overlap, t))
        })
        .min_by_key(|&(sep, neg_overlap, _)| (sep, neg_overlap))
        .map(|(_, _, t)| t)
}

/// `tuned` replaces the middle of the straight track `t`; the stubs on either side are kept and the whole line is
/// returned running the way `t`'s own points did, so whatever the track's ends were joined to still is.
fn stitch(t: &Track, axis: Axis, tuned: &[Point]) -> Vec<Point> {
    let forward = axis.along(t.pts[0]) <= axis.along(t.pts[1]);
    let (lo, hi) = if forward { (t.pts[0], t.pts[1]) } else { (t.pts[1], t.pts[0]) };
    let mut v = Vec::with_capacity(tuned.len() + 2);
    v.push(lo);
    v.extend_from_slice(tuned);
    v.push(hi);
    let mut v = simplify_collinear(v);
    if !forward {
        v.reverse();
    }
    v
}

/// What `8` computes: the two lines to put in place of the clicked track and its partner.
#[derive(Debug, Clone)]
pub struct DpTune {
    pub partner_id: String,
    pub partner_net: String,
    /// New points of the clicked line and of its partner, each running the way the original track's points did.
    pub a_pts: Vec<Point>,
    pub b_pts: Vec<Point>,
    /// Centre-to-centre distance of the two lines (the pair's `gap + width`).
    pub pitch: Um,
    /// Whole-net routed lengths of the clicked line's net / the partner's, before and after.
    pub a_before: Um,
    pub b_before: Um,
    pub a_after: Um,
    pub b_after: Um,
}

impl DpTune {
    /// `origPathLength`: the pair is as long as its longer line.
    pub fn pair_length_before(&self) -> Um {
        self.a_before.max(self.b_before)
    }
    pub fn pair_length_after(&self) -> Um {
        self.a_after.max(self.b_after)
    }
    /// Clicked line minus partner.
    pub fn skew_before(&self) -> Um {
        self.a_before - self.b_before
    }
    pub fn skew_after(&self) -> Um {
        self.a_after - self.b_after
    }
}

/// `8`: meander the pair `track_id` belongs to until the longer net reaches `target_length`.
pub fn tune_diff_pair(tracks: &[Track], track_id: &str, amplitude: Um, spacing: Um, target_length: Um, flip: bool) -> Result<DpTune, String> {
    let a = tracks.iter().find(|t| t.id == track_id).ok_or("no such track")?;
    let axis = Axis::of(a).ok_or(SINGLE_STRAIGHT_HINT)?;
    let partner_net = dp_coupled_net_name(&a.net).ok_or(DP_NAME_HINT)?;
    let b = find_partner(tracks, a, axis, &partner_net).ok_or_else(|| format!("no straight run of {partner_net} on {} lies alongside this track -- tune a pair where both lines run side by side", a.layer))?;

    let (alo, ahi) = along_range(a, axis);
    let (blo, bhi) = along_range(b, axis);
    let (s0, s1) = (alo.max(blo), ahi.min(bhi));
    let (lat_a, lat_b) = (axis.lat(a.pts[0]), axis.lat(b.pts[0]));
    // `baselineSegment`: the midline of the coupled stretch; each line sits `(gap + width) / 2` off it
    let lat_m = lat_a + (lat_b - lat_a) / 2;
    let (off_a, off_b) = (lat_a - lat_m, lat_b - lat_m);

    let (a_before, b_before) = (net_length(tracks, &a.net), net_length(tracks, &partner_net));
    let pair_before = a_before.max(b_before);
    if target_length <= pair_before {
        return Err(format!("the target length must be longer than the pair's current length ({pair_before} um)"));
    }
    // both lines are the same length across the stretch, so the longer net's elongation is what the stretch adds
    let want = (s1 - s0) + (target_length - pair_before);
    let dp = generate_dp_meander(axis.point(s0, lat_m), axis.point(s1, lat_m), off_a, off_b, amplitude, spacing, want, flip).map_err(|e| e.message().to_string())?;

    let a_pts = stitch(a, axis, &dp.a);
    let b_pts = stitch(b, axis, &dp.b);
    let a_after = a_before - polyline_length(&a.pts) + polyline_length(&a_pts);
    let b_after = b_before - polyline_length(&b.pts) + polyline_length(&b_pts);
    Ok(DpTune { partner_id: b.id.clone(), partner_net, a_pts, b_pts, pitch: (lat_a - lat_b).abs(), a_before, b_before, a_after, b_after })
}

/// What `9` computes: the one line to put in place of the clicked track.
#[derive(Debug, Clone)]
pub struct SkewTune {
    pub partner_net: String,
    pub pts: Vec<Point>,
    /// The partner net's routed length (`m_coupledLength`).
    pub coupled_length: Um,
    /// The clicked net's routed length before / after.
    pub own_before: Um,
    pub own_after: Um,
}

impl SkewTune {
    /// `CurrentSkew`: this line minus the partner.
    pub fn skew_before(&self) -> Um {
        self.own_before - self.coupled_length
    }
    pub fn skew_after(&self) -> Um {
        self.own_after - self.coupled_length
    }
}

/// `9`: lengthen the clicked line until its net is `target_skew` longer than the partner net (0: equal length).
pub fn tune_skew(tracks: &[Track], track_id: &str, amplitude: Um, spacing: Um, target_skew: Um, flip: bool) -> Result<SkewTune, String> {
    let a = tracks.iter().find(|t| t.id == track_id).ok_or("no such track")?;
    let [p0, p1] = a.pts[..] else { return Err(SINGLE_STRAIGHT_HINT.into()) };
    let partner_net = dp_coupled_net_name(&a.net).ok_or(SKEW_NAME_HINT)?;
    let coupled_length = net_length(tracks, &partner_net);
    if coupled_length == 0 {
        return Err(format!("{partner_net} has no routed copper yet, so there is no length to match"));
    }
    let own_before = net_length(tracks, &a.net);
    let extra = coupled_length + target_skew - own_before;
    if extra <= 0 {
        return Err(format!("{} is already {} um long against {partner_net}'s {coupled_length} um -- skew tuning only lengthens the selected line, select the shorter one", a.net, own_before));
    }
    let segment = polyline_length(&a.pts);
    let meander = generate_meander(p0, p1, amplitude, spacing, segment + extra, flip)
        .ok_or("can't reach that length -- the amplitude/spacing leave no room on this short or non-axis-aligned a run")?;
    let own_after = own_before - segment + meander.achieved_length;
    Ok(SkewTune { partner_net, pts: meander.pts, coupled_length, own_before, own_after })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    fn track(id: &str, net: &str, layer: &str, pts: &[Point]) -> Track {
        Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: 150, pts: pts.to_vec(), arc_mid_offset: None }
    }

    /// A 20 mm horizontal pair, 300 um apart (USB_P above, USB_N below), plus a stub of other copper on USB_N.
    fn pair() -> Vec<Track> {
        vec![
            track("p1", "USB_P", "F.Cu", &[p(0, 9_850), p(20_000, 9_850)]),
            track("n1", "USB_N", "F.Cu", &[p(0, 10_150), p(20_000, 10_150)]),
        ]
    }

    #[test]
    fn net_length_adds_up_every_track_of_the_net_on_every_layer() {
        let mut t = pair();
        t.push(track("n2", "USB_N", "B.Cu", &[p(0, 0), p(0, 500)]));
        assert_eq!(net_length(&t, "USB_P"), 20_000);
        assert_eq!(net_length(&t, "USB_N"), 20_500);
        assert_eq!(net_length(&t, "GND"), 0);
    }

    #[test]
    fn diff_pair_tuning_meanders_both_lines_in_step_and_keeps_their_ends_where_they_were() {
        let t = pair();
        let tune = tune_diff_pair(&t, "p1", 400, 800, 23_000, false).expect("room");
        assert_eq!(tune.partner_id, "n1");
        assert_eq!(tune.partner_net, "USB_N");
        assert_eq!(tune.pitch, 300);
        assert_eq!((tune.a_pts.first(), tune.a_pts.last()), (Some(&p(0, 9_850)), Some(&p(20_000, 9_850))));
        assert_eq!((tune.b_pts.first(), tune.b_pts.last()), (Some(&p(0, 10_150)), Some(&p(20_000, 10_150))));
        assert!((tune.pair_length_after() - 23_000).abs() <= 3, "pair is now {}", tune.pair_length_after());
        assert!(tune.skew_after().abs() < 450, "the miters trade only a little length between the lines: {}", tune.skew_after());
        assert_eq!(tune.pair_length_before(), 20_000);
        assert!(tune.a_pts.len() > 10 && tune.b_pts.len() > 10, "both lines actually meander");
    }

    #[test]
    fn diff_pair_tuning_works_from_either_line_and_on_a_vertical_pair_with_reversed_points() {
        let t = vec![
            // stored bottom-to-top / top-to-bottom: the results must keep each track's own direction
            track("a", "CLK+", "F.Cu", &[p(5_000, 20_000), p(5_000, 0)]),
            track("b", "CLK-", "F.Cu", &[p(5_300, 0), p(5_300, 20_000)]),
        ];
        let tune = tune_diff_pair(&t, "b", 400, 800, 23_000, false).expect("vertical pair");
        assert_eq!(tune.partner_id, "a");
        assert_eq!((tune.a_pts.first(), tune.a_pts.last()), (Some(&p(5_300, 0)), Some(&p(5_300, 20_000))), "the clicked track b keeps its own direction");
        assert_eq!((tune.b_pts.first(), tune.b_pts.last()), (Some(&p(5_000, 20_000)), Some(&p(5_000, 0))), "the partner a keeps its own");
        assert_eq!(tune.pitch, 300);
        assert!((tune.pair_length_after() - 23_000).abs() <= 3);
    }

    #[test]
    fn diff_pair_tuning_only_meanders_the_stretch_both_lines_share_and_keeps_the_stubs() {
        let t = vec![
            track("p", "D_P", "F.Cu", &[p(0, 0), p(20_000, 0)]),
            // the partner starts 5 mm in and stops 3 mm short
            track("n", "D_N", "F.Cu", &[p(5_000, 300), p(17_000, 300)]),
        ];
        let tune = tune_diff_pair(&t, "p", 400, 800, 22_000, false).expect("room in the 12 mm they share");
        assert_eq!((tune.a_pts.first(), tune.a_pts.last()), (Some(&p(0, 0)), Some(&p(20_000, 0))));
        assert_eq!((tune.b_pts.first(), tune.b_pts.last()), (Some(&p(5_000, 300)), Some(&p(17_000, 300))));
        // the unshared 5 mm of D_P stays one straight run (and its 3 mm tail too)
        assert_eq!(&tune.a_pts[..2], &[p(0, 0), p(tune.a_pts[1].x, 0)]);
        assert!(tune.a_pts[1].x >= 5_000 - 1, "the stub reaches the start of the shared stretch: {:?}", tune.a_pts[1]);
        assert!((tune.a_after - 22_000).abs() <= 3, "D_P is the longer line: {}", tune.a_after);
        assert_eq!(tune.pair_length_before(), 20_000, "D_N is only 12 mm long");
    }

    #[test]
    fn diff_pair_tuning_falls_as_close_as_the_run_allows_when_the_target_is_out_of_reach() {
        // 5 mm more than the 12 mm shared stretch can take at 400 um amplitude / 800 um spacing (7 bumps, +2.3 mm)
        let t = vec![track("p", "D_P", "F.Cu", &[p(0, 0), p(20_000, 0)]), track("n", "D_N", "F.Cu", &[p(5_000, 300), p(17_000, 300)])];
        let tune = tune_diff_pair(&t, "p", 400, 800, 25_000, false).expect("best effort");
        assert!((22_000..25_000).contains(&tune.pair_length_after()), "{}", tune.pair_length_after());
    }

    #[test]
    fn diff_pair_tuning_explains_each_way_it_cannot_run() {
        let t = pair();
        assert_eq!(tune_diff_pair(&t, "nope", 400, 800, 23_000, false).unwrap_err(), "no such track");
        // not a pair name
        let lone = vec![track("x", "VCC", "F.Cu", &[p(0, 0), p(10_000, 0)])];
        assert_eq!(tune_diff_pair(&lone, "x", 400, 800, 12_000, false).unwrap_err(), DP_NAME_HINT);
        // partner on another layer
        let other_layer = vec![track("p", "D_P", "F.Cu", &[p(0, 0), p(10_000, 0)]), track("n", "D_N", "B.Cu", &[p(0, 300), p(10_000, 300)])];
        assert!(tune_diff_pair(&other_layer, "p", 400, 800, 12_000, false).unwrap_err().contains("lies alongside"));
        // not straight
        let bent = vec![track("p", "D_P", "F.Cu", &[p(0, 0), p(5_000, 0), p(5_000, 5_000)])];
        assert!(tune_diff_pair(&bent, "p", 400, 800, 12_000, false).unwrap_err().contains("straight track"));
        // target not longer than the pair
        assert!(tune_diff_pair(&t, "p1", 400, 800, 20_000, false).unwrap_err().contains("must be longer"));
    }

    #[test]
    fn skew_tuning_lengthens_the_selected_line_to_the_partners_length() {
        // USB_P is 20 mm, USB_N 20 mm + 3 mm of other copper: P is 3 mm short
        let mut t = pair();
        t.push(track("n2", "USB_N", "F.Cu", &[p(0, 0), p(3_000, 0)]));
        let tune = tune_skew(&t, "p1", 400, 800, 0, false).expect("room");
        assert_eq!(tune.partner_net, "USB_N");
        assert_eq!(tune.coupled_length, 23_000);
        assert_eq!(tune.skew_before(), -3_000);
        assert!(tune.skew_after().abs() <= 3, "skew after {}", tune.skew_after());
        assert_eq!((tune.pts.first(), tune.pts.last()), (Some(&p(0, 9_850)), Some(&p(20_000, 9_850))));
    }

    #[test]
    fn skew_tuning_can_aim_for_a_requested_skew() {
        let t = pair();
        // lengthen P so it ends up 1.5 mm *longer* than N
        let tune = tune_skew(&t, "p1", 400, 800, 1_500, false).expect("room");
        assert!((tune.skew_after() - 1_500).abs() <= 3, "skew after {}", tune.skew_after());
    }

    #[test]
    fn skew_tuning_refuses_a_line_that_is_already_long_enough_and_other_non_starters() {
        let t = pair();
        // N is not shorter than P -> tuning N has nothing to add
        let err = tune_skew(&t, "n1", 400, 800, 0, false).unwrap_err();
        assert!(err.contains("only lengthens the selected line"), "{err}");
        let lone = vec![track("x", "VCC", "F.Cu", &[p(0, 0), p(10_000, 0)])];
        assert_eq!(tune_skew(&lone, "x", 400, 800, 0, false).unwrap_err(), SKEW_NAME_HINT);
        let no_partner_copper = vec![track("p", "D_P", "F.Cu", &[p(0, 0), p(10_000, 0)])];
        assert!(tune_skew(&no_partner_copper, "p", 400, 800, 0, false).unwrap_err().contains("no routed copper"));
        let bent = vec![track("p", "D_P", "F.Cu", &[p(0, 0), p(5_000, 0), p(5_000, 5_000)]), track("n", "D_N", "F.Cu", &[p(0, 300), p(20_000, 300)])];
        assert!(tune_skew(&bent, "p", 400, 800, 0, false).unwrap_err().contains("straight track"));
    }
}
