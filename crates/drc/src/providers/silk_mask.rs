//! Ported from `pcbnew/drc/drc_test_provider_silk_clearance.cpp`
//! (`DRC_TEST_PROVIDER_SILK_CLEARANCE`): silkscreen against silkscreen
//! (`DRCE_SILK_CLEARANCE`, "silk_overlap") and -- while the board's solder
//! mask minimum web width is 0, the only time this provider (rather than
//! `drc_test_provider_solder_mask.cpp`'s whole-board mask polygon, see
//! `solder_mask.rs`) is responsible -- silkscreen against individual
//! mask-layer items (`DRCE_SILK_MASK_CLEARANCE`, "silk_over_copper").
//!
//! Mirrors the C++ structure:
//! - the silk tree holds every item on `F.SilkS`/`B.SilkS` (graphics, text,
//!   and *pads that list a silk layer*), the target tree every item on the
//!   matching mask layer plus the silk layer itself (the other layer pairs
//!   -- paste, courtyard, fab, adhesive, copper, edge cuts -- never produce
//!   a violation: `EvalRules( SILK_CLEARANCE_CONSTRAINT, .., layer )`
//!   finds no constraint on a non-silk layer, so `minClearance` stays -1);
//! - pairs are visited per layer pair in `layerPairs` order (silk/silk
//!   first), each unordered item pair collides at most once
//!   (`QueryCollidingPairs`' `collidingCompounds`);
//! - a pair is skipped when the *target* shape does not touch the board
//!   outline polygon, when both are `PCB_SHAPE`s of the same footprint (or
//!   both board-level), and a mask-layer pair is only tested at all when
//!   `m_SolderMaskMinWidth <= 0` (`checkIndividualMaskItems`), with
//!   `minClearance = max( constraint, 0 )`.
//!
//! Text is laid out by `stroke_font::kicad_text_segments` (a port of
//! `FONT::getLinePositions` + `STROKE_FONT::GetTextAsGlyphs`); markup and
//! italics are drawn literally.
//!
//! Generated: `DRCE_SILK_CLEARANCE`, `DRCE_SILK_MASK_CLEARANCE`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::Shape;
use crate::providers::solder_mask::{build_items, collide, item_ref, pad_ref, shapes_bbox, Item, Kind, BACK, FRONT};
use crate::rtree::DrcRTree;
use eda_model::ir::Um;
use eda_model::BoardRules;
use std::collections::{HashMap, HashSet};

const ERROR_LIMIT: usize = 199;

pub(crate) type Key = (u8, usize);

/// One entry of the silk or target tree: an item and its compound shape.
pub(crate) struct Ent {
    /// Identity of the owning `BOARD_ITEM` (a pad silk item and the same
    /// pad's mask entry share one, so they never collide with each other).
    pub(crate) key: Key,
    pub(crate) fp: Option<usize>,
    /// A `PCB_SHAPE` (`Type() == PCB_SHAPE_T`).
    pub(crate) is_shape: bool,
    pub(crate) shapes: Vec<Shape>,
    pub(crate) bbox: (Um, Um, Um, Um),
    pub(crate) refitem: DrcRefItem,
}

const K_PAD: u8 = 0;
const K_VIA: u8 = 1;
const K_TRACK: u8 = 2;
const K_MASK_GFX: u8 = 3;
const K_SILK: u8 = 4;

fn ent_from_item(board: &DrcBoard, it: &Item) -> Ent {
    let key = match it.kind {
        Kind::Pad => (K_PAD, it.idx),
        Kind::Via => (K_VIA, it.idx),
        Kind::Track => (K_TRACK, it.idx),
        Kind::Graphic => (K_MASK_GFX, it.idx),
    };
    Ent { key, fp: it.fp, is_shape: it.kind == Kind::Graphic && !it.is_pad, shapes: it.shapes.clone(), bbox: it.bbox, refitem: item_ref(board, it) }
}

/// Every item on a silkscreen layer, per side (`addToSilkTree`): graphics,
/// text, and pads whose layer set lists the silk layer.
pub(crate) fn silk_ents(board: &DrcBoard) -> [Vec<Ent>; 2] {
    let mut silk: [Vec<Ent>; 2] = [Vec::new(), Vec::new()];
    for (i, p) in board.pads.iter().enumerate() {
        let Some(pm) = board.mask.pads.get(i) else { continue };
        for (side, name) in [(FRONT, "F.SilkS"), (BACK, "B.SilkS")] {
            if pm.layers.iter().any(|l| l == name) {
                let shapes = vec![p.copper.clone()];
                silk[side].push(Ent { key: (K_PAD, i), fp: pm.fp, is_shape: false, bbox: shapes_bbox(&shapes), shapes, refitem: pad_ref(board, i) });
            }
        }
    }
    for (i, s) in board.mask.silk.iter().enumerate() {
        let side = if s.layer == "F.SilkS" {
            FRONT
        } else if s.layer == "B.SilkS" {
            BACK
        } else {
            continue;
        };
        if s.shapes.is_empty() {
            continue;
        }
        silk[side].push(Ent { key: (K_SILK, i), fp: s.fp, is_shape: s.is_shape, bbox: shapes_bbox(&s.shapes), shapes: s.shapes.clone(), refitem: DrcRefItem { description: s.desc.clone(), pos: (s.pos.x, s.pos.y), id: s.id.clone() } });
    }
    silk
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out: Vec<DrcViolation> = Vec::new();
    let silk_clearance = constraints::silk_clearance_min(rules);
    // `checkIndividualMaskItems`: with a minimum web width the whole-board mask
    // polygon test in `solder_mask.rs` takes over.
    let check_individual_mask_items = board.mask.rules.min_width_um <= 0;
    let outline: Option<Shape> = if board.outline.len() >= 3 { Some(Shape::Polygon { pts: board.outline.clone() }) } else { None };
    let outline_bbox = outline.as_ref().map(|o| o.bbox(0));

    // ---- silk tree (per side) ----
    let silk = silk_ents(board);

    // ---- target tree: mask-layer items ----
    let mut net_ids: HashMap<String, i32> = HashMap::new();
    let (items, _fps) = build_items(board, &mut net_ids);
    let mut targets: [Vec<Ent>; 2] = [Vec::new(), Vec::new()];
    for it in &items {
        // A tented via, a track, a copper-only pad: not on a mask layer.
        for side in [FRONT, BACK] {
            if it.mask[side] && !it.shapes.is_empty() {
                targets[side].push(ent_from_item(board, it));
            }
        }
    }

    let mut colliding: HashSet<(Key, Key)> = HashSet::new();
    let (mut n_silk, mut n_mask) = (0usize, 0usize);

    for side in [FRONT, BACK] {
        // Layer pair (silk, silk), then (silk, mask).
        for pair in 0..2 {
            let tgt: &Vec<Ent> = if pair == 0 { &silk[side] } else { &targets[side] };
            let mut tree = DrcRTree::new(2000);
            for (i, t) in tgt.iter().enumerate() {
                tree.insert(i, t.bbox);
            }
            let inflate = silk_clearance.max(0);
            for r in &silk[side] {
                if n_silk >= ERROR_LIMIT && n_mask >= ERROR_LIMIT {
                    break;
                }
                let q = (r.bbox.0 - inflate, r.bbox.1 - inflate, r.bbox.2 + inflate, r.bbox.3 + inflate);
                let mut cand = tree.query(q);
                cand.sort_unstable();
                for ti in cand {
                    let t = &tgt[ti];
                    // don't collide items against themselves
                    if t.key == r.key {
                        continue;
                    }
                    let unordered = if r.key <= t.key { (r.key, t.key) } else { (t.key, r.key) };
                    if colliding.contains(&unordered) {
                        continue;
                    }
                    let (error_type, min_clearance): (ErrorType, Um) = if pair == 0 {
                        (ErrorType::SilkOverlap, silk_clearance)
                    } else if check_individual_mask_items {
                        // `constraint` is null on a mask layer (-1); `max( -1, 0 )`.
                        (ErrorType::SilkOverCopper, 0)
                    } else {
                        continue;
                    };
                    if min_clearance < 0 {
                        continue;
                    }
                    if (error_type == ErrorType::SilkOverlap && n_silk >= ERROR_LIMIT) || (error_type == ErrorType::SilkOverCopper && n_mask >= ERROR_LIMIT) {
                        continue;
                    }
                    // Graphics are often compound shapes so ignore collisions between
                    // shapes in a single footprint or on the board (both parent
                    // footprints will be nullptr).
                    if r.is_shape && t.is_shape && r.fp == t.fp {
                        continue;
                    }
                    // Only the part of the target that touches the board outline counts.
                    let test_shapes: Vec<Shape> = match (&outline, outline_bbox) {
                        (Some(o), Some(ob)) => {
                            if t.bbox.2 < ob.0 || ob.2 < t.bbox.0 || t.bbox.3 < ob.1 || ob.3 < t.bbox.1 {
                                continue;
                            }
                            let kept: Vec<Shape> = t.shapes.iter().filter(|s| s.collides(o, 0).is_some()).cloned().collect();
                            if kept.is_empty() {
                                continue;
                            }
                            kept
                        }
                        _ => t.shapes.clone(),
                    };
                    let Some((actual, _pos)) = collide(&r.shapes, &test_shapes, min_clearance) else { continue };
                    let detail = if min_clearance > 0 { format!("(clearance {}; actual {})", format_um(min_clearance), format_um(actual)) } else { String::new() };
                    out.push(DrcViolation::new(error_type, detail, vec![r.refitem.clone(), t.refitem.clone()]));
                    colliding.insert(unordered);
                    if error_type == ErrorType::SilkOverlap {
                        n_silk += 1;
                    } else {
                        n_mask += 1;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{DrcPadMask, DrcSilk};
    use eda_model::ir::Point;
    use eda_model::{PadKind, PadShape};

    fn pad(id: &str, x: i64) -> crate::board::DrcPad {
        crate::board::DrcPad {
            id: id.into(),
            footprint_ref: "U1".into(),
            number: "1".into(),
            net: None,
            center: Point { x, y: 0 },
            side: eda_model::ir::Side::Top,
            kind: PadKind::Smd,
            layers: vec!["F.Cu".into()],
            copper: Shape::Rect { x0: x - 250, y0: -250, x1: x + 250, y1: 250 },
            hole: None,
            drill_round: None,
            drill_slot: None,
        }
    }

    fn board() -> DrcBoard {
        DrcBoard { copper_graphics: vec![], layers: vec!["F.Cu".into(), "B.Cu".into()], outline: vec![], pads: vec![], tracks: vec![], vias: vec![], zones: vec![], keepouts: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![], mask: Default::default() }
    }

    fn silk_line(id: &str, a: (i64, i64), b: (i64, i64)) -> DrcSilk {
        DrcSilk { fp: None, id: id.into(), desc: id.into(), pos: Point { x: a.0, y: a.1 }, layer: "F.SilkS".into(), shapes: vec![Shape::Stadium { a: Point { x: a.0, y: a.1 }, b: Point { x: b.0, y: b.1 }, r: 50 }], is_shape: false }
    }

    #[test]
    fn silk_over_an_exposed_pad_is_silk_over_copper() {
        let mut b = board();
        b.pads = vec![pad("U1.1", 0)];
        b.mask.pads = vec![DrcPadMask { fp: None, layers: vec![], margin: None, tent_front: None, tent_back: None, pin_type: String::new(), shape: PadShape::Rect, size: (500, 500) }];
        b.mask.silk = vec![silk_line("s1", (-1000, 0), (1000, 0))];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.iter().filter(|v| v.error_type == "silk_over_copper").count(), 1, "{v:#?}");
    }

    #[test]
    fn silk_over_a_track_is_not_reported() {
        // `drc_test_provider_silk_clearance.cpp` only pairs silk with mask-layer items:
        // routed copper (a track) is not one, however much silk sits on top of it.
        let mut b = board();
        b.tracks = vec![crate::board::DrcTrackSeg { id: "t1".into(), net: None, layer: "F.Cu".into(), width: 200, a: Point { x: -1000, y: 0 }, b: Point { x: 1000, y: 0 }, arc_mid: None }];
        b.mask.silk = vec![silk_line("s1", (-1000, 0), (1000, 0))];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn crossing_silk_items_overlap_once() {
        let mut b = board();
        b.mask.silk = vec![silk_line("s1", (-1000, 0), (1000, 0)), silk_line("s2", (0, -1000), (0, 1000))];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.iter().filter(|v| v.error_type == "silk_overlap").count(), 1, "{v:#?}");
    }

    #[test]
    fn a_minimum_web_width_hands_mask_testing_to_the_solder_mask_provider() {
        let mut b = board();
        b.pads = vec![pad("U1.1", 0)];
        b.mask.rules.min_width_um = 100;
        b.mask.silk = vec![silk_line("s1", (-1000, 0), (1000, 0))];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }
}
