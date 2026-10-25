//! What a length tuning leaves on the board: the tracks that take the place of the stretch of line that was tuned -- or that put the line back
//! straight, when a pattern is removed or about to be tuned again (`PCB_TUNING_PATTERN::EditFinish`, `Remove` and `resetToBaseline`).
//!
//! A placer ([`crate::meander_placer`], [`crate::dp_meander_placer`]) cuts the line it tunes in three: what comes before the pattern, the
//! stretch the pattern is on, what comes after, and replaces the middle one with a meander. KiCad commits the whole new line, a track for each
//! of its segments and arcs. The IR keeps tracks as polylines, and a line is often several of them (a track of an imported board is one
//! segment), so this commits only what was touched: the tracks that have a segment of the stretch are replaced, the runs of them outside it
//! are handed back, and the replacement runs from the first touched track's start to the last one's end -- the line up to the pattern, the
//! pattern (a polyline for each run of straight segments, an arc for each arc), the line after it. The pieces that are the pattern are
//! flagged: they become the members of its [`TuningPattern`](eda_model::tuning::TuningPattern) (`EditFinish` adds the new tracks inside the
//! pattern's outline to the generator).
//!
//! Micrometres on the way out: the chains are nanometres (see [`crate::kigeom`]) and a vertex is rounded to the board's grid. The same cut
//! point is the end of one piece and the start of the next, so they still meet.

use eda_model::units::Nm;
use crate::chain::{distance, ShapeLineChain};
use crate::item::{Item, ItemId};
use crate::line::Line;
use crate::meander_placer::{chain_of, Tuned};
use crate::node::Node;
use eda_model::ir::Point;
use std::collections::{BTreeSet, HashSet};

/// How far apart two points that should be one are allowed to be before they are two (nanometres): the rounding of a meander's shapes.
const SNAP_NM: Nm = Nm::new(3);

/// One track of the new line (micrometres).
#[derive(Debug, Clone, PartialEq)]
pub enum PieceShape {
    /// A run of straight segments.
    Poly(Vec<Point>),
    /// An arc, by its two ends and a point on it.
    Arc { start: Point, mid: Point, end: Point },
}

/// A new track, and whether it belongs to the tuning pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub shape: PieceShape,
    pub member: bool,
}

/// What one tuned line becomes on the board.
#[derive(Debug, Clone, Default)]
pub struct LineEdit {
    /// The tracks the edit replaces: the ones with a segment in the stretch.
    pub remove_track_ids: Vec<String>,
    /// What stays of those tracks, as the world has it (consecutive segments that join end to end are one line): outside the stretch.
    pub keep: Vec<Line>,
    /// The new tracks, in order along the line.
    pub pieces: Vec<Piece>,
}

/// The chains one after the other, each starting where the one before ended (a few nanometres off is the rounding of a meander: the start of the
/// next is moved onto the end of the one before). The segment count after each of them.
fn join(parts: &[&ShapeLineChain]) -> (ShapeLineChain, Vec<usize>) {
    let mut out = ShapeLineChain::new();
    let mut ends = Vec::new();
    for part in parts {
        if part.point_count() == 0 {
            ends.push(out.segment_count());
            continue;
        }
        if out.point_count() > 0 {
            let (last, first) = (out.c_last_point(), part.c_point(0));
            if last != first && distance(last, first) <= SNAP_NM {
                let mut moved = (*part).clone();
                moved.set_point(0, last);
                out.append_chain(&moved);
                ends.push(out.segment_count());
                continue;
            }
        }
        out.append_chain(part);
        ends.push(out.segment_count());
    }
    (out, ends)
}

/// The part of `chain` from the point `p` (on it) to its end.
fn tail_from(chain: &ShapeLineChain, p: Point) -> Option<ShapeLineChain> {
    let mut c = chain.clone();
    let i = c.split_point(p, true);
    (i >= 0).then(|| c.slice(i, -1))
}

/// The part of `chain` from its start to the point `p` (on it).
fn head_to(chain: &ShapeLineChain, p: Point) -> Option<ShapeLineChain> {
    let mut c = chain.clone();
    let i = c.split_point(p, true);
    (i >= 0).then(|| c.slice(0, i))
}

/// A chain as the pieces of track it is made of: a polyline for each run of straight segments of one kind and an arc for each arc (micrometres;
/// segments `members` are the ones from `member_from` up to, not including, `member_to`).
fn pieces_of(chain: &ShapeLineChain, member_from: usize, member_to: usize, members: bool) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut run: Vec<Point> = Vec::new();
    let mut run_member = false;
    let flush = |run: &mut Vec<Point>, member: bool, out: &mut Vec<Piece>| {
        let mut pts: Vec<Point> = Vec::new();
        for q in run.drain(..) {
            let q = q;
            if pts.last() != Some(&q) {
                pts.push(q);
            }
        }
        if pts.len() >= 2 {
            out.push(Piece { shape: PieceShape::Poly(pts), member });
        }
    };
    let mut i = 0;
    while i < chain.segment_count() {
        let member = members && i >= member_from && i < member_to;
        if chain.is_arc_segment(i) {
            let ai = chain.arc_index(i);
            let mut j = i;
            while j < chain.segment_count() && chain.is_arc_segment(j) && chain.arc_index(j) == ai {
                j += 1;
            }
            flush(&mut run, run_member, &mut out);
            let arc = chain.arc(ai as usize);
            let (start, mid, end) = (arc.start, arc.mid, arc.end);
            if start != end && start != mid && mid != end {
                out.push(Piece { shape: PieceShape::Arc { start, mid, end }, member });
            } else {
                out.push(Piece { shape: PieceShape::Poly(vec![start, end]), member });
            }
            i = j;
            continue;
        }
        if run.is_empty() {
            run.push(chain.c_point(i));
            run_member = member;
        } else if member != run_member {
            flush(&mut run, run_member, &mut out);
            run.push(chain.c_point(i));
            run_member = member;
        }
        run.push(chain.c_point(i + 1));
        i += 1;
    }
    flush(&mut run, run_member, &mut out);
    out
}

/// What `line` (a line of `world`, assembled from the segment a placer started on) becomes when the stretch `t` describes is replaced by
/// `t.tuned`. `members` says whether the new stretch is a tuning pattern (a meander) or only the line, put back. `None` when the cut does not
/// fit the line (nothing was cut, or the line is not made of the world's segments).
pub fn line_edit(world: &Node, line: &Line, t: &Tuned, members: bool) -> Option<LineEdit> {
    let origin = chain_of(line);
    if origin.segment_count() == 0 || line.segment_ids.len() != origin.segment_count() || t.tuned.point_count() < 2 {
        return None;
    }

    // The segments of the line the stretch runs along: the one every segment of the raw cut lies on (found by its middle).
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    for j in 0..t.raw.segment_count() {
        let s = t.raw.c_segment(j);
        if s.a == s.b {
            continue;
        }
        // the midpoint, halved toward minus infinity
        let half = |v: Nm| Nm::new(v.get().div_euclid(2));
        let mid = Point { x: half(s.a.x + s.b.x), y: half(s.a.y + s.b.y) };
        let k = (0..origin.segment_count()).min_by_key(|&k| origin.c_segment(k).squared_distance(mid))?;
        touched.insert(k);
    }
    let (&lo, &hi) = (touched.iter().next()?, touched.iter().next_back()?);
    let (v_lo, v_hi) = (origin.c_point(lo), origin.c_point(hi + 1));

    // The touched run's two ends: the one on the side of the pattern's start is the one `pre` reaches back to.
    let lo_in_pre = t.pre.point_on_edge_nm(v_lo, SNAP_NM);
    let hi_in_pre = t.pre.point_on_edge_nm(v_hi, SNAP_NM);
    let (first, last) = match (lo_in_pre, hi_in_pre) {
        (true, false) => (v_lo, v_hi),
        (false, true) => (v_hi, v_lo),
        _ => return None,
    };
    let head = tail_from(&t.pre, first)?;
    let tail = head_to(&t.post, last)?;
    let (replacement, ends) = join(&[&head, &t.tuned, &tail]);
    let pieces = pieces_of(&replacement, ends[0], ends[1], members);
    if pieces.is_empty() {
        return None;
    }

    // The tracks that own the touched segments, and what is left of them.
    let covered: HashSet<ItemId> = line.segment_ids[lo..=hi].iter().copied().collect();
    let mut remove_track_ids: Vec<String> = Vec::new();
    for id in &covered {
        if let Some(track) = world.get(*id).and_then(Item::source_track_id) {
            if !remove_track_ids.iter().any(|t| t == track) {
                remove_track_ids.push(track.to_string());
            }
        }
    }
    remove_track_ids.sort();
    let keep: Vec<Line> = remove_track_ids.iter().flat_map(|id| world.uncovered_track_runs(id, &covered)).collect();
    Some(LineEdit { remove_track_ids, keep, pieces })
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Segment};
    use crate::meander::MeanderSettings;
    use crate::meander_placer::MeanderPlacer;
    use crate::tuning_path::LengthEnv;
    use eda_model::BoardRules;

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn env() -> LengthEnv {
        let mut rules = BoardRules::default();
        rules.layers = vec!["F.Cu".into(), "B.Cu".into()];
        rules.clearance = mm(0.2);
        LengthEnv { stackup: eda_model::rules::default_stackup(2, mm(1.6)), rules }
    }

    fn settings(target_nm: i64) -> MeanderSettings {
        let mut s = MeanderSettings::default();
        s.set_target_length(target_nm);
        s.keep_endpoints = true;
        s
    }

    fn seg(w: &mut Node, track: &str, k: usize, a: Point, b: Point) -> ItemId {
        w.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a, b, width: mm(0.2), source_track: Some((track.into(), k)), locked: false }))
    }

    fn poly_points(e: &LineEdit) -> Vec<Point> {
        let mut out: Vec<Point> = Vec::new();
        for piece in &e.pieces {
            match &piece.shape {
                PieceShape::Poly(pts) => out.extend(pts.iter().copied()),
                PieceShape::Arc { start, end, .. } => out.extend([*start, *end]),
            }
        }
        out.dedup();
        out
    }

    /// The pieces form one connected line: each starts where the one before ended.
    fn assert_connected(e: &LineEdit) {
        let ends = |s: &PieceShape| match s {
            PieceShape::Poly(pts) => (pts[0], *pts.last().unwrap()),
            PieceShape::Arc { start, end, .. } => (*start, *end),
        };
        for w in e.pieces.windows(2) {
            assert_eq!(ends(&w[0].shape).1, ends(&w[1].shape).0, "{:?} then {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn a_straight_track_tuned_in_its_middle_becomes_the_line_before_the_meander_and_after() {
        let mut w = Node::new();
        let s = seg(&mut w, "trk", 0, p(0, 0), p(40_000_000, 0));
        let mut placer = MeanderPlacer::start(&w, &env(), s, p(5_000_000, 0), settings(44_000_000)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        let line = placer.origin_line().clone();
        let e = line_edit(&w, &line, &placer.tuned(), true).expect("an edit");
        assert_eq!(e.remove_track_ids, vec!["trk".to_string()]);
        assert!(e.keep.is_empty(), "the one segment of the track was touched");
        assert_connected(&e);
        let first = &e.pieces[0];
        let last = e.pieces.last().unwrap();
        assert_eq!(first.shape, PieceShape::Poly(vec![p(0, 0), p(5_000_000, 0)]));
        assert_eq!(last.shape, PieceShape::Poly(vec![p(35_000_000, 0), p(40_000_000, 0)]));
        assert!(!first.member && !last.member);
        let members: Vec<&Piece> = e.pieces.iter().filter(|q| q.member).collect();
        assert!(members.len() > 10 && members.len() == e.pieces.len() - 2, "the meander is every piece between: {} of {}", members.len(), e.pieces.len());
        assert!(members.iter().any(|q| matches!(q.shape, PieceShape::Arc { .. })), "rounded corners are arcs");
    }

    #[test]
    fn only_the_tracks_the_pattern_touches_are_replaced_and_the_runs_outside_it_stay() {
        let mut w = Node::new();
        // a line of three tracks, the middle one a polyline of three segments (a vertex at 12, 14 and 16 mm)
        seg(&mut w, "a", 0, p(0, 0), p(10_000_000, 0));
        seg(&mut w, "b", 0, p(10_000_000, 0), p(12_000_000, 0));
        let clicked = seg(&mut w, "b", 1, p(12_000_000, 0), p(14_000_000, 0));
        seg(&mut w, "b", 2, p(14_000_000, 0), p(30_000_000, 0));
        seg(&mut w, "c", 0, p(30_000_000, 0), p(40_000_000, 0));
        let mut placer = MeanderPlacer::start(&w, &env(), clicked, p(13_000_000, 0), settings(0)).unwrap();
        // the pattern runs from 13 mm to 22 mm: segment 1 (12..14) and segment 2 (14..30) of track b are touched; track a and c are not
        placer.update_settings({
            let mut s = settings(0);
            s.set_target_length((placer.orig_path_length() + mm(3.0)).get());
            s
        });
        assert!(placer.do_move(p(22_000_000, 0)));
        let line = placer.origin_line().clone();
        let e = line_edit(&w, &line, &placer.tuned(), true).expect("an edit");
        assert_eq!(e.remove_track_ids, vec!["b".to_string()], "only b has a segment in the stretch");
        // b's first segment (10..12) is outside the stretch and is handed back
        assert_eq!(e.keep.len(), 1, "{:?}", e.keep);
        assert_eq!(e.keep[0].pts, vec![p(10_000_000, 0), p(12_000_000, 0)]);
        assert_connected(&e);
        // the replacement runs from where the first touched segment starts (12 mm) to where the last one ends (30 mm)
        let pts = poly_points(&e);
        assert_eq!(pts[0], p(12_000_000, 0));
        assert_eq!(*pts.last().unwrap(), p(30_000_000, 0));
        assert!(e.pieces.iter().any(|q| q.member) && !e.pieces[0].member && !e.pieces.last().unwrap().member);
    }

    #[test]
    fn a_pattern_dragged_back_along_the_line_is_replaced_the_same_way() {
        let mut w = Node::new();
        let s = seg(&mut w, "a", 0, p(0, 0), p(10_000_000, 0));
        seg(&mut w, "b", 0, p(10_000_000, 0), p(40_000_000, 0));
        // started at 30 mm and taken back to 20 mm: the cut comes in the other order, and the pieces run from the start of the pattern outwards
        let mut placer = MeanderPlacer::start(&w, &env(), s, p(30_000_000, 0), settings(0)).unwrap();
        placer.update_settings({
            let mut st = settings(0);
            st.set_target_length((placer.orig_path_length() + mm(2.0)).get());
            st
        });
        assert!(placer.do_move(p(20_000_000, 0)));
        let line = placer.origin_line().clone();
        let e = line_edit(&w, &line, &placer.tuned(), true).expect("an edit");
        assert_eq!(e.remove_track_ids, vec!["b".to_string()]);
        assert!(e.keep.is_empty());
        assert_connected(&e);
        let pts = poly_points(&e);
        let (lo, hi) = (pts.iter().map(|q| q.x).min().unwrap(), pts.iter().map(|q| q.x).max().unwrap());
        assert_eq!((lo, hi), (mm(10.0), mm(40.0)), "b is replaced whole: its start to its end");
        assert!(e.pieces.iter().any(|q| q.member));
    }

    #[test]
    fn a_line_put_back_has_no_members() {
        let mut w = Node::new();
        let s = seg(&mut w, "trk", 0, p(0, 0), p(40_000_000, 0));
        let mut placer = MeanderPlacer::start(&w, &env(), s, p(5_000_000, 0), settings(44_000_000)).unwrap();
        assert!(placer.do_move(p(35_000_000, 0)));
        let line = placer.origin_line().clone();
        let mut t = placer.tuned();
        // the stretch back to straight: the baseline from the pattern's start to its end
        t.tuned = ShapeLineChain::from_points(&[p(5_000_000, 0), p(35_000_000, 0)]);
        let e = line_edit(&w, &line, &t, false).expect("an edit");
        assert!(e.pieces.iter().all(|q| !q.member));
        assert_eq!(poly_points(&e), vec![p(0, 0), p(5_000_000, 0), p(35_000_000, 0), p(40_000_000, 0)]);
    }

    #[test]
    fn nothing_cut_is_no_edit() {
        let mut w = Node::new();
        let s = seg(&mut w, "trk", 0, p(0, 0), p(40_000_000, 0));
        let placer = MeanderPlacer::start(&w, &env(), s, p(5_000_000, 0), settings(44_000_000)).unwrap();
        let line = placer.origin_line().clone();
        assert!(line_edit(&w, &line, &Tuned::default(), true).is_none());
    }
}
