//! The pure part of putting one footprint in the place of another (`PCB_EDIT_FRAME::ExchangeFootprint`, `pcbnew/pcb_edit_frame.cpp` at KiCad 8303b2ad):
//! where the new footprint goes so that its pads land where the old ones were, and which text of the old footprint is which of the new one.
//!
//! * [`pad_alignment`] is `ComputeFootprintShift` (`pcbnew/footprint_utils.cpp`) over `ORTHO_ITEM_REALIGNER::GetTransform` (`common/item_realignment.cpp`): the
//!   pads whose number is unique in both footprints are the points; the new footprint is turned by a multiple of 90 degrees and moved so that the most of them
//!   coincide (or, when no shift fits more than half of them, so that the mean distance is least).
//! * [`match_texts`] is `matchItemsBySimilarity` for the texts of the two footprints (`EDA_TEXT::Similarity`: 0.9 for a different look, 0.9 for another place,
//!   times the Levenshtein ratio of the two strings).
//! * [`carry_text`] and [`carry_layout`] are `processTextItem`: a text of the old footprint put over its match in the new one, taking from the old what the
//!   dialog's "update/reset" options leave alone.
//!
//! Frames and signs. A footprint's pads, texts and graphics are in the frame of `Pad::at`: the footprint as seen from the top, un-rotated. `FootprintInstance::rot`
//! turns clockwise on the screen and a bottom-side footprint mirrors x first ([`crate::footprint::to_board`]). KiCad's `GetFPRelativePosition` is the board
//! offset turned back by the orientation, i.e. the same frame mirrored for a bottom-side footprint, and its orientation is counter-clockwise: `rot` is the
//! opposite of it. [`PadAlignment`] says the shift in the first and the turn in the second, as KiCad does; [`PadAlignment::apply`] puts it on a pose.

use crate::footprint::Footprint;
use crate::fp_art::FpText;
use crate::fp_edit::FieldLayout;
use crate::ir::{FootprintInstance, Millideg, Point, Side, Nm};
use std::collections::BTreeMap;

/// `epsilon` of `ORTHO_ITEM_REALIGNER::GetTransform`: 10 nm, as nm.
const EPSILON_NM: f64 = 10.0;

/// What `ComputeFootprintShift` finds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadAlignment {
    /// How far the new footprint's position moves, in the footprint frame (the frame of `Pad::at`, mirrored in x for a bottom-side footprint), nm.
    pub translation: (f64, f64),
    /// How much the footprint's orientation is turned, counter-clockwise, in millidegrees: 0, 90000, 180000 or 270000.
    pub rotation_ccw: i64,
}

impl PadAlignment {
    /// `pose` with the shift put on it: `position += GetRotated( translation, orientation )`, `orientation += rotation`. `rot` turns the other way
    /// (clockwise), so the turn is taken from it.
    pub fn apply(&self, pose: &FootprintInstance) -> FootprintInstance {
        let rad = (pose.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
        let (sin, cos) = rad.sin_cos();
        let (tx, ty) = self.translation;
        let at = Point { x: pose.at.x + Nm::kiround(tx * cos - ty * sin), y: pose.at.y + Nm::kiround(tx * sin + ty * cos) };
        FootprintInstance { at, rot: (pose.rot as i64 - self.rotation_ccw).rem_euclid(360_000) as Millideg, ..pose.clone() }
    }
}

/// The pads of `fp` whose number nothing else in it has, each at its position in the footprint frame (`getUniquelyNumberedPads`): the points a shift is worked out from.
fn uniquely_numbered_pads(fp: &Footprint, side: Side) -> BTreeMap<&str, (f64, f64)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &fp.pads {
        *seen.entry(p.number.as_str()).or_default() += 1;
    }
    let mirror = if side == Side::Bottom { -1.0 } else { 1.0 };
    fp.pads.iter().filter(|p| seen[p.number.as_str()] == 1).map(|p| (p.number.as_str(), (p.at.0.as_f64() * mirror, p.at.1.as_f64()))).collect()
}

/// `Snap90Degrees`: the angle, normalized to `[0, 360)`, taken to the nearest of 0, 90, 180 and 270 (315 and over go to 0).
fn snap_90(deg: f64) -> i64 {
    let a = deg.rem_euclid(360.0);
    if a < 45.0 {
        0
    } else if a < 135.0 {
        90
    } else if a < 225.0 {
        180
    } else if a < 315.0 {
        270
    } else {
        0
    }
}

/// `RotatePoint( pt, { 0, 0 }, angle )` for a multiple of 90 degrees: KiCad's positive angle turns (x, y) to (y, -x).
fn rotate_kicad(p: (f64, f64), quarter_ccw: i64) -> (f64, f64) {
    match quarter_ccw.rem_euclid(4) {
        0 => p,
        1 => (p.1, -p.0),
        2 => (-p.0, -p.1),
        _ => (-p.1, p.0),
    }
}

/// `ORTHO_ITEM_REALIGNER::GetTransform( aPtsA, aPtsB )`: the turn (degrees, a multiple of 90) and the translation that bring the points B onto the points A.
fn realign(a: &[(f64, f64)], b: &[(f64, f64)]) -> Option<(i64, (f64, f64))> {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len();
    let norm = |v: (f64, f64)| v.0.hypot(v.1);
    let angle = |v: (f64, f64)| v.1.atan2(v.0).to_degrees();
    // a baseline: two points that are not coincident in either set
    let mut best_rotation: Option<f64> = None;
    'outer: for first in 0..n {
        for second in first..n {
            let base_a = (a[second].0 - a[first].0, a[second].1 - a[first].1);
            let base_b = (b[second].0 - b[first].0, b[second].1 - b[first].1);
            if norm(base_a) < EPSILON_NM || norm(base_b) < EPSILON_NM {
                continue;
            }
            best_rotation = Some(angle(base_b) - angle(base_a));
            break 'outer;
        }
    }
    let rotation = snap_90(best_rotation?);
    let quarter = rotation / 90;
    let rotated: Vec<(f64, f64)> = b.iter().map(|p| rotate_kicad(*p, quarter)).collect();
    // The translation that makes the most points coincide, when that is more than half of them; "less than half is no good because that can happen if you widen a DIP".
    let mut best: Option<(usize, usize)> = None; // (index, matches)
    for k in 0..n {
        let t = (a[k].0 - rotated[k].0, a[k].1 - rotated[k].1);
        let matches = (0..n).filter(|&i| norm((a[i].0 - (rotated[i].0 + t.0), a[i].1 - (rotated[i].1 + t.1))) < EPSILON_NM).count();
        if best.is_none_or(|(_, m)| matches > m) {
            best = Some((k, matches));
        }
    }
    if let Some((k, matches)) = best {
        if matches > n / 2 {
            return Some((rotation, (a[k].0 - rotated[k].0, a[k].1 - rotated[k].1)));
        }
    }
    // Otherwise the mean displacement, which is the translation of least squared error (VECTOR2I's division truncates).
    let (mut sx, mut sy) = (0.0, 0.0);
    for i in 0..n {
        sx += a[i].0 - rotated[i].0;
        sy += a[i].1 - rotated[i].1;
    }
    Some((rotation, ((sx / n as f64).trunc(), (sy / n as f64).trunc())))
}

/// `ComputeFootprintShift( aExisting, aNew )`: where the new footprint goes so that its pads are where the existing footprint's were. `side` is the side the
/// footprint is on (the frame of a bottom-side footprint is mirrored). `None` when the two share no pad number of their own, or all those pads lie on one point.
pub fn pad_alignment(existing: &Footprint, new: &Footprint, side: Side) -> Option<PadAlignment> {
    let (old_pads, new_pads) = (uniquely_numbered_pads(existing, side), uniquely_numbered_pads(new, side));
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for (number, at) in &old_pads {
        if let Some(to) = new_pads.get(number) {
            a.push(*at);
            b.push(*to);
        }
    }
    let (rotation, translation) = realign(&a, &b)?;
    Some(PadAlignment { translation, rotation_ccw: rotation * 1000 })
}

// --------------------------------------------------------------------------------------------------------------------------------------- texts

/// `EDA_TEXT::Levenshtein`: 1 minus the edit distance over the longer length; 0 when either string is empty.
pub fn levenshtein_ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (m, n) = (a.len(), b.len());
    if m == 0 || n == 0 {
        return 0.0;
    }
    let mut distance = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in distance.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=n {
        distance[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            distance[i][j] = if a[i - 1] == b[j - 1] { distance[i - 1][j - 1] } else { distance[i - 1][j].min(distance[i][j - 1]).min(distance[i - 1][j - 1]) + 1 };
        }
    }
    1.0 - distance[m][n] as f64 / m.max(n) as f64
}

/// The attributes `EDA_TEXT_ATTRIBUTES` compares: size, thickness, justification, style, mirroring and angle.
fn same_attributes(a: &FieldLayout, b: &FieldLayout) -> bool {
    (a.size, a.thickness, a.halign, a.valign, a.bold, a.italic, a.mirror, a.keep_upright, a.knockout, a.angle) == (b.size, b.thickness, b.halign, b.valign, b.bold, b.italic, b.mirror, b.keep_upright, b.knockout, b.angle)
}

/// `EDA_TEXT::Similarity`.
pub fn text_similarity(old: &FpText, new: &FpText) -> f64 {
    let mut similarity = 1.0;
    if !same_attributes(&old.layout, &new.layout) {
        similarity *= 0.9;
    }
    if old.layout.at != new.layout.at {
        similarity *= 0.9;
    }
    similarity * levenshtein_ratio(&old.text, &new.text)
}

/// `matchItemsBySimilarity` for texts: the pairs (index in `old`, index in `new`) taken best score first, none twice; a pair that scores nothing is not a pair.
pub fn match_texts(old: &[FpText], new: &[FpText]) -> Vec<(usize, usize)> {
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i, o) in old.iter().enumerate() {
        for (j, n) in new.iter().enumerate() {
            let score = text_similarity(o, n);
            if score > 0.0 {
                candidates.push((score, i, j));
            }
        }
    }
    candidates.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let (mut used_old, mut used_new) = (vec![false; old.len()], vec![false; new.len()]);
    let mut pairs = Vec::new();
    for (_, i, j) in candidates {
        if used_old[i] || used_new[j] {
            continue;
        }
        used_old[i] = true;
        used_new[j] = true;
        pairs.push((i, j));
    }
    pairs
}

/// The dialog's "update/reset" options that bear on a text (`m_resetTextItemLayers`, `Effects`, `Positions`, `Content`): when one is on, the text keeps what the
/// new footprint has; when it is off, it takes what the old footprint's text had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextReset {
    pub layers: bool,
    pub effects: bool,
    pub positions: bool,
    pub content: bool,
}

/// `processTextItem` for a field's layout: `old` (a text of the footprint at `old_pose`) over `new` (its match in the footprint now at `new_pose`).
/// Without `positions` the text stays where it was on the board and keeps its absolute angle.
pub fn carry_layout(old: &FieldLayout, new: &mut FieldLayout, old_pose: &FootprintInstance, new_pose: &FootprintInstance, reset: &TextReset) {
    if !reset.layers {
        new.layer = old.layer.clone();
        new.visible = old.visible;
    }
    if !reset.effects {
        new.size = old.size;
        new.thickness = old.thickness;
        new.halign = old.halign;
        new.valign = old.valign;
        new.mirror = old.mirror;
        new.bold = old.bold;
        new.italic = old.italic;
        new.keep_upright = old.keep_upright;
        new.knockout = old.knockout;
    }
    if !reset.positions {
        // `rotatedShift = GetRotated( aSrc.GetFPRelativePosition() - aPosShift, -aAngleShift )`: the text stays where it was on the board
        new.set_from_board(new_pose, old.board_position(old_pose), old.board_angle(old_pose));
    }
}

/// `processTextItem` for a footprint text: [`carry_layout`], and the string when "update/reset text content" is off.
pub fn carry_text(old: &FpText, new: &mut FpText, old_pose: &FootprintInstance, new_pose: &FootprintInstance, reset: &TextReset) {
    if !reset.content {
        new.text = old.text.clone();
    }
    if !reset.effects {
        new.face = old.face.clone();
        new.line_spacing_permille = old.line_spacing_permille;
    }
    carry_layout(&old.layout, &mut new.layout, old_pose, new_pose, reset);
}

/// The old text kept in the new footprint when nothing in the new footprint matches it ("remove text items if not in library footprint" is off): the
/// same text, left where it was on the board.
pub fn keep_text(old: &FpText, old_pose: &FootprintInstance, new_pose: &FootprintInstance) -> FpText {
    let mut kept = old.clone();
    kept.layout.set_from_board(new_pose, old.layout.board_position(old_pose), old.layout.board_angle(old_pose));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::{Pad, PadKind, PadShape};
    use crate::ir::LabelSide;

    fn pad(number: &str, x: Nm, y: Nm) -> Pad {
        Pad { number: number.into(), at: (x, y), size: (Nm::new(600_000), Nm::new(600_000)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    fn fp(pads: Vec<Pad>) -> Footprint {
        Footprint { name: "X".into(), pads, ..Default::default() }
    }

    fn pose(x: Nm, y: Nm, rot: Millideg, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x, y }, rot, side, label: LabelSide::default() }
    }

    /// Where the pad `number` of `fp` is on the board with the footprint at `pose`.
    fn on_board(fp: &Footprint, number: &str, pose: &FootprintInstance) -> Point {
        let p = fp.pads.iter().find(|p| p.number == number).unwrap();
        crate::footprint::to_board(pose, p.at)
    }

    #[test]
    fn a_new_footprint_with_another_origin_is_moved_so_its_pads_stay_where_they_were() {
        // the old footprint has its origin at the middle of the two pads; the new one has it on pad 1
        let old = fp(vec![pad("1", Nm::new(-1_000_000), Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO)]);
        let new = fp(vec![pad("1", Nm::ZERO, Nm::ZERO), pad("2", Nm::new(2_000_000), Nm::ZERO)]);
        for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (270_000, Side::Top), (0, Side::Bottom), (90_000, Side::Bottom), (180_000, Side::Bottom)] {
            let was = pose(Nm::new(10_000_000), Nm::new(20_000_000), rot, side);
            let shift = pad_alignment(&old, &new, side).expect("two pads to go by");
            assert_eq!(shift.rotation_ccw, 0);
            let now = shift.apply(&was);
            for number in ["1", "2"] {
                assert_eq!(on_board(&new, number, &now), on_board(&old, number, &was), "pad {number}, {side:?} turned {rot}");
            }
        }
    }

    #[test]
    fn a_new_footprint_drawn_turned_a_quarter_is_turned_back_and_its_pads_land_on_the_old_ones() {
        // old: a horizontal pair; new: the same pair drawn vertically (a part library that has its 2-pin parts upright)
        let old = fp(vec![pad("1", Nm::new(-1_000_000), Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO)]);
        let new = fp(vec![pad("1", Nm::ZERO, Nm::new(-1_000_000)), pad("2", Nm::ZERO, Nm::new(1_000_000))]);
        for side in [Side::Top, Side::Bottom] {
            for rot in [0, 90_000, 135_000] {
                let was = pose(Nm::new(5_000_000), Nm::new(7_000_000), rot, side);
                let shift = pad_alignment(&old, &new, side).unwrap();
                assert_eq!(shift.rotation_ccw.rem_euclid(180_000), 90_000, "a quarter turn one way or the other: {shift:?}");
                let now = shift.apply(&was);
                for number in ["1", "2"] {
                    let (a, b) = (on_board(&new, number, &now), on_board(&old, number, &was));
                    assert!((a.x - b.x).abs() <= Nm::new(2) && (a.y - b.y).abs() <= Nm::new(2), "pad {number} {side:?} {rot}: {a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_pad_that_is_not_alone_with_its_number_is_not_a_point_to_go_by() {
        // pad "1" twice in the old footprint: only "2" and "3" are points, and they are enough
        let old = fp(vec![pad("1", Nm::new(-3_000_000), Nm::ZERO), pad("1", Nm::new(-2_000_000), Nm::ZERO), pad("2", Nm::ZERO, Nm::ZERO), pad("3", Nm::new(2_000_000), Nm::ZERO)]);
        let new = fp(vec![pad("1", Nm::new(100_000), Nm::new(100_000)), pad("2", Nm::new(1_000_000), Nm::ZERO), pad("3", Nm::new(3_000_000), Nm::ZERO)]);
        let shift = pad_alignment(&old, &new, Side::Top).unwrap();
        assert_eq!(shift.translation, (-1_000_000.0, 0.0));
        // nothing in common, or one point alone: nothing to turn by
        assert!(pad_alignment(&fp(vec![pad("1", Nm::ZERO, Nm::ZERO)]), &fp(vec![pad("A", Nm::new(5000), Nm::new(5000))]), Side::Top).is_none());
        assert!(pad_alignment(&fp(vec![pad("1", Nm::ZERO, Nm::ZERO)]), &fp(vec![pad("1", Nm::new(5000), Nm::new(5000))]), Side::Top).is_none(), "a single point has no baseline");
    }

    #[test]
    fn the_pads_that_still_match_decide_when_they_are_most_and_the_mean_decides_when_they_are_not() {
        // four pads, three of them where they were: the outlier is ignored
        let old = fp(vec![pad("1", Nm::ZERO, Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO), pad("3", Nm::new(2_000_000), Nm::ZERO), pad("4", Nm::new(3_000_000), Nm::ZERO)]);
        let moved = fp(vec![pad("1", Nm::ZERO, Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO), pad("3", Nm::new(2_000_000), Nm::ZERO), pad("4", Nm::new(3_600_000), Nm::ZERO)]);
        assert_eq!(pad_alignment(&old, &moved, Side::Top).unwrap().translation, (0.0, 0.0), "3 of 4 coincide");
        // a DIP widened by 2 mm: two of four coincide, which is not more than half ("that can happen if you widen a DIP"): the mean displacement
        let dip = fp(vec![pad("1", Nm::new(-1_000_000), Nm::new(-1_000_000)), pad("2", Nm::new(-1_000_000), Nm::new(1_000_000)), pad("3", Nm::new(1_000_000), Nm::new(1_000_000)), pad("4", Nm::new(1_000_000), Nm::new(-1_000_000))]);
        let wide = fp(vec![pad("1", Nm::new(-1_000_000), Nm::new(-1_000_000)), pad("2", Nm::new(-1_000_000), Nm::new(1_000_000)), pad("3", Nm::new(3_000_000), Nm::new(1_000_000)), pad("4", Nm::new(3_000_000), Nm::new(-1_000_000))]);
        assert_eq!(pad_alignment(&dip, &wide, Side::Top).unwrap().translation, (-1_000_000.0, 0.0), "(0 + 0 - 2000 - 2000) / 4");
        // pads spread out by a fifth: no shift keeps more than one, the mean is (-200 - 400 - 600) / 4
        let spread = fp(vec![pad("1", Nm::ZERO, Nm::ZERO), pad("2", Nm::new(1_200_000), Nm::ZERO), pad("3", Nm::new(2_400_000), Nm::ZERO), pad("4", Nm::new(3_600_000), Nm::ZERO)]);
        assert_eq!(pad_alignment(&old, &spread, Side::Top).unwrap().translation, (-300_000.0, 0.0));
    }

    #[test]
    fn the_snap_goes_to_the_nearest_quarter_turn() {
        assert_eq!([snap_90(0.0), snap_90(44.9), snap_90(45.0), snap_90(134.9), snap_90(135.0), snap_90(224.9), snap_90(225.0), snap_90(314.9), snap_90(315.0), snap_90(-90.0)], [0, 0, 90, 90, 180, 180, 270, 270, 0, 270]);
    }

    #[test]
    fn levenshtein_is_one_minus_the_distance_over_the_longer() {
        assert_eq!(levenshtein_ratio("REF**", "REF**"), 1.0);
        assert_eq!(levenshtein_ratio("", "x"), 0.0);
        assert!((levenshtein_ratio("kitten", "sitting") - (1.0 - 3.0 / 7.0)).abs() < 1e-12);
    }

    fn text(s: &str, x: Nm) -> FpText {
        FpText::new(s, Point { x, y: Nm::ZERO }, "F.Fab")
    }

    #[test]
    fn texts_are_matched_best_first_and_none_twice() {
        let old = vec![text("${REFERENCE}", Nm::ZERO), text("note", Nm::new(5_000_000)), text("zzz", Nm::ZERO)];
        let new = vec![text("note", Nm::new(5_000_000)), text("${REFERENCE}", Nm::ZERO)];
        let mut pairs = match_texts(&old, &new);
        pairs.sort();
        assert_eq!(pairs, [(0, 1), (1, 0)], "'zzz' scores nothing against either and stays alone");
    }

    #[test]
    fn a_text_the_options_leave_alone_stays_where_it_was_on_the_board() {
        let (old_pose, new_pose) = (pose(Nm::new(10_000_000), Nm::new(10_000_000), 0, Side::Top), pose(Nm::new(10_500_000), Nm::new(10_000_000), 90_000, Side::Top));
        let mut old = text("hello", Nm::new(1_000_000));
        old.layout.size = (Nm::new(800_000), Nm::new(800_000));
        old.layout.layer = "F.SilkS".into();
        let mut new = text("hi", Nm::ZERO);
        carry_text(&old, &mut new, &old_pose, &new_pose, &TextReset { layers: true, effects: true, positions: false, content: true });
        assert_eq!(new.text, "hi", "content reset: the new footprint's");
        assert_eq!(new.layout.size, (Nm::new(1_000_000), Nm::new(1_000_000)), "effects reset");
        assert_eq!(new.layout.layer, "F.Fab", "layers reset");
        assert_eq!(new.layout.board_position(&new_pose), old.layout.board_position(&old_pose), "the position is the old one on the board");
        assert_eq!(new.layout.board_angle(&new_pose), old.layout.board_angle(&old_pose));
        // nothing reset: everything is the old text's
        let mut new = text("hi", Nm::ZERO);
        carry_text(&old, &mut new, &old_pose, &new_pose, &TextReset { layers: false, effects: false, positions: false, content: false });
        assert_eq!((new.text.as_str(), new.layout.size, new.layout.layer.as_str()), ("hello", (Nm::new(800_000), Nm::new(800_000)), "F.SilkS"));
        // position reset: the new footprint's own place
        let mut new = text("hi", Nm::new(123_000));
        carry_text(&old, &mut new, &old_pose, &new_pose, &TextReset { layers: true, effects: true, positions: true, content: true });
        assert_eq!(new.layout.at, Point { x: Nm::new(123_000), y: Nm::ZERO });
    }

    #[test]
    fn a_text_nothing_matches_is_kept_where_it_was_on_the_board() {
        let (old_pose, new_pose) = (pose(Nm::new(10_000_000), Nm::new(10_000_000), 0, Side::Top), pose(Nm::new(12_000_000), Nm::new(9_000_000), 180_000, Side::Top));
        let old = text("keep me", Nm::new(1_000_000));
        let kept = keep_text(&old, &old_pose, &new_pose);
        assert_eq!(kept.layout.board_position(&new_pose), old.layout.board_position(&old_pose));
        assert_eq!(kept.text, "keep me");
    }
}
