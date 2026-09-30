//! Board geometry shared by the placers: sizing the outline to the parts
//! and shaving whatever rim they leave empty.
//!
//! This lives here rather than beside either placer because both need it
//! and neither may depend on the other. It was written for the Cypress
//! path; the anneal path needs it for exactly the same reason, since a
//! board left at whatever outline the intent declared ships two to three
//! times larger than it has to be.

use crate::footprint::placed_keepout;
use crate::ir::{Design, Point, Um};
use crate::ConstraintModel;
use std::collections::BTreeSet;

/// Margin, µm, kept between the outermost part and a trimmed edge.
pub const TRIM_MARGIN_UM: Um = 1500;

/// Courtyard-to-edge gap, µm, at which a fixed (edge) connector is pinned.
/// Under the placement gate's `EDGE_CONNECTOR_MAX_GAP_UM` (1500) with room
/// for the board's copper edge clearance (500) and a track beside the pads.
pub const EDGE_PIN_GAP_UM: Um = 700;

/// Sum of every part's keep-out area (courtyard + label), µm².
pub fn keepout_area(design: &Design, model: &ConstraintModel) -> f64 {
    let Some(pl) = design.placement.as_ref() else { return 0.0 };
    pl.footprints.iter().filter_map(|f| model.part(&f.id).and_then(|p| placed_keepout(model, &pl.outline, p, f))).map(|k| ((k.2 - k.0) as f64) * ((k.3 - k.1) as f64)).sum()
}

/// Cut blank bands off the sides of a rectangular outline that hold no
/// pinned connector, keeping [`TRIM_MARGIN_UM`] beyond the outermost
/// keep-out. Parts keep their board coordinates (shifted when a min side
/// moves). `None` when nothing is worth trimming (< 2 mm).
/// Shave blank bands off any board edge that holds no pinned connector,
/// returning `None` when there is nothing to shave. Board geometry, not a
/// Cypress detail: the anneal path calls it through `eda::trim_empty_edges`
/// because fitting the board before placing only gets part of the way, and
/// whatever the placer left empty at the rim is board nobody pays for
/// twice.
pub fn trim_empty_edges(design: &Design, model: &ConstraintModel, pinned: &BTreeSet<String>) -> Option<Design> {
    let pl = design.placement.as_ref()?;
    if pl.outline.len() != 4 {
        return None;
    }
    let (x0, y0, x1, y1) = (
        pl.outline.iter().map(|p| p.x).min()?,
        pl.outline.iter().map(|p| p.y).min()?,
        pl.outline.iter().map(|p| p.x).max()?,
        pl.outline.iter().map(|p| p.y).max()?,
    );
    if !pl.outline.iter().all(|p| (p.x == x0 || p.x == x1) && (p.y == y0 || p.y == y1)) {
        return None;
    }
    let kos: Vec<(bool, (i64, i64, i64, i64))> = pl
        .footprints
        .iter()
        .filter_map(|f| Some((pinned.contains(&f.id), placed_keepout(model, &pl.outline, model.part(&f.id)?, f)?)))
        .collect();
    let used = (
        kos.iter().map(|(_, k)| k.0).min()?,
        kos.iter().map(|(_, k)| k.1).min()?,
        kos.iter().map(|(_, k)| k.2).max()?,
        kos.iter().map(|(_, k)| k.3).max()?,
    );
    // An edge with a pinned connector on it is not trimmed.
    let near = |k: (i64, i64, i64, i64), edge: u8| match edge {
        0 => k.0 - x0 < 2 * EDGE_PIN_GAP_UM,
        1 => x1 - k.2 < 2 * EDGE_PIN_GAP_UM,
        2 => k.1 - y0 < 2 * EDGE_PIN_GAP_UM,
        _ => y1 - k.3 < 2 * EDGE_PIN_GAP_UM,
    };
    let held = |edge: u8| kos.iter().any(|(p, k)| *p && near(*k, edge));
    let mut n = (x0, y0, x1, y1);
    if !held(0) && used.0 - x0 > TRIM_MARGIN_UM + 2000 { n.0 = used.0 - TRIM_MARGIN_UM; }
    if !held(1) && x1 - used.2 > TRIM_MARGIN_UM + 2000 { n.2 = used.2 + TRIM_MARGIN_UM; }
    if !held(2) && used.1 - y0 > TRIM_MARGIN_UM + 2000 { n.1 = used.1 - TRIM_MARGIN_UM; }
    if !held(3) && y1 - used.3 > TRIM_MARGIN_UM + 2000 { n.3 = used.3 + TRIM_MARGIN_UM; }
    if n == (x0, y0, x1, y1) {
        return None;
    }
    // Whole millimetres, origin back at the old min corner.
    let w = ((n.2 - n.0) as f64 / 1000.0).ceil() as i64 * 1000;
    let h = ((n.3 - n.1) as f64 / 1000.0).ceil() as i64 * 1000;
    let (dx, dy) = (n.0 - x0, n.1 - y0);
    let mut out = design.clone();
    let p = out.placement.as_mut()?;
    p.outline = vec![Point { x: x0, y: y0 }, Point { x: x0 + w, y: y0 }, Point { x: x0 + w, y: y0 + h }, Point { x: x0, y: y0 + h }];
    for f in &mut p.footprints {
        f.at = Point { x: f.at.x - dx, y: f.at.y - dy };
    }
    eprintln!("cypress: trimmed blank edges, board {}x{} -> {}x{} mm", (x1 - x0) / 1000, (y1 - y0) / 1000, w / 1000, h / 1000);
    Some(out)
}

/// Shrink a rectangular outline (axis-aligned, 4 corners) about its
/// min corner so the parts' keep-out area is `util` of the board, keeping
/// the aspect ratio and never growing it. `scale_floor` bounds the shrink
/// from below (1.0 = as written). Non-rectangular outlines are kept.
/// Shrink a rectangular outline until the parts' keep-out area is `util`
/// of it, keeping the aspect ratio and never growing it, then park every
/// part at the new centre for the placer to spread.
///
/// This is board geometry, not a Cypress detail; it lives here because
/// that is where it was written, and the anneal path calls it through
/// `eda::fit_outline` for exactly the same reason Cypress does: a board
/// left at whatever outline the intent declared ships two to three times
/// larger than it needs to be, and board area is money.
pub fn fit_outline(design: &Design, model: &ConstraintModel, util: f64, scale_floor: f64) -> Design {
    let mut out = design.clone();
    let Some(pl) = out.placement.as_mut() else { return out };
    if util <= 0.0 || pl.outline.len() != 4 {
        return out;
    }
    let (x0, y0, x1, y1) = (
        pl.outline.iter().map(|p| p.x).min().unwrap(),
        pl.outline.iter().map(|p| p.y).min().unwrap(),
        pl.outline.iter().map(|p| p.x).max().unwrap(),
        pl.outline.iter().map(|p| p.y).max().unwrap(),
    );
    let rect = pl.outline.iter().all(|p| (p.x == x0 || p.x == x1) && (p.y == y0 || p.y == y1));
    if !rect {
        return out;
    }
    let area = ((x1 - x0) as f64) * ((y1 - y0) as f64);
    let want = keepout_area(design, model) / util;
    let scale = (want / area).sqrt().max(scale_floor).min(1.0);
    if scale >= 0.999 {
        return out;
    }
    // Round the new size up to whole millimetres.
    let w = ((((x1 - x0) as f64) * scale / 1000.0).ceil() * 1000.0) as i64;
    let h = ((((y1 - y0) as f64) * scale / 1000.0).ceil() * 1000.0) as i64;
    pl.outline = vec![Point { x: x0, y: y0 }, Point { x: x0 + w, y: y0 }, Point { x: x0 + w, y: y0 + h }, Point { x: x0, y: y0 + h }];
    // Parts placed on the old outline start at the new centre; Cypress
    // spreads them.
    for f in &mut pl.footprints {
        f.at = Point { x: x0 + w / 2, y: y0 + h / 2 };
    }
    out
}


/// The area every part's courtyard needs, before anything is placed.
///
/// [`keepout_area`] reads the *placed* footprints, so it answers zero for
/// a board that has not been laid out yet -- which makes it useless for
/// deciding how big that board should be. Trying to size a board with it
/// collapsed every outline to the scale floor, because an empty
/// placement genuinely has no area.
///
/// This asks the model instead: each part resolves to a footprint, and a
/// footprint knows its courtyard about the origin whether or not it has
/// a position. Rotation is ignored -- a part's area does not change when
/// it turns, and which way each part will face is not known yet.
pub fn intrinsic_part_area(model: &ConstraintModel) -> f64 {
    model
        .parts
        .iter()
        .filter_map(|p| model.footprint_of(p))
        .map(|fp| {
            let (hw, hh) = fp.courtyard_half();
            ((hw * 2) as f64) * ((hh * 2) as f64)
        })
        .sum()
}

/// A board sized for the parts it must hold, at `density`, keeping the
/// aspect ratio of `like`.
///
/// Sizes in both directions: a board too large for its contents fails
/// `placement_board_use` for being mostly empty, and one too small
/// leaves the placer with nowhere to put anything. Rounded up to whole
/// millimetres, anchored at `like`'s origin.
///
/// `None` when there is nothing to size for, or `like` is not a
/// rectangle -- a shaped outline is a deliberate decision and is left
/// alone.
pub fn sized_for_parts(model: &ConstraintModel, like: &[Point], density: f64) -> Option<Vec<Point>> {
    if density <= 0.0 || like.len() != 4 {
        return None;
    }
    let (x0, y0) = (like.iter().map(|p| p.x).min()?, like.iter().map(|p| p.y).min()?);
    let (x1, y1) = (like.iter().map(|p| p.x).max()?, like.iter().map(|p| p.y).max()?);
    if !like.iter().all(|p| (p.x == x0 || p.x == x1) && (p.y == y0 || p.y == y1)) {
        return None;
    }
    let (w, h) = ((x1 - x0) as f64, (y1 - y0) as f64);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let want = intrinsic_part_area(model) / density;
    if want <= 0.0 {
        return None;
    }
    let scale = (want / (w * h)).sqrt();
    let nw = ((w * scale / 1000.0).ceil() * 1000.0) as i64;
    let nh = ((h * scale / 1000.0).ceil() * 1000.0) as i64;
    if nw <= 0 || nh <= 0 {
        return None;
    }
    Some(vec![
        Point { x: x0, y: y0 },
        Point { x: x0 + nw, y: y0 },
        Point { x: x0 + nw, y: y0 + nh },
        Point { x: x0, y: y0 + nh },
    ])
}

/// A board for an intent that declares no outline: a square at the origin
/// sized for the parts at `density`, as [`sized_for_parts`] would size a
/// declared one, and never narrower than the widest part plus a
/// millimetre -- sized by area alone, a board holding one long header
/// could come out shorter than the header.
///
/// [`fit_outline`] cannot do this: it shrinks an outline that exists, and
/// handed a placement with none it returns the placement unchanged.
pub fn square_for_parts(model: &ConstraintModel, density: f64) -> Option<Vec<Point>> {
    let unit = [Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }, Point { x: 0, y: 1000 }];
    let sized = sized_for_parts(model, &unit, density)?;
    let widest = model
        .parts
        .iter()
        .filter_map(|p| model.footprint_of(p))
        .map(|fp| {
            let (hw, hh) = fp.courtyard_half();
            2 * hw.max(hh)
        })
        .max()
        .unwrap_or(0);
    let side = sized[1].x.max((widest + 1000 + 999) / 1000 * 1000);
    Some(vec![Point { x: 0, y: 0 }, Point { x: side, y: 0 }, Point { x: side, y: side }, Point { x: 0, y: side }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Part, Pin, PinKind};

    fn model(packages: &[&str]) -> ConstraintModel {
        let parts = packages
            .iter()
            .enumerate()
            .map(|(i, pkg)| Part {
                reference: format!("U{i}"),
                mpn: None,
                value: None,
                package: Some((*pkg).into()),
                footprint: Some((*pkg).into()),
                pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }],
                body_um: None,
                edge: None,
            })
            .collect();
        ConstraintModel { parts, ..Default::default() }
    }

    #[test]
    fn a_board_is_fitted_for_parts_when_the_intent_has_no_outline() {
        let m = model(&["0603", "0603", "SOT-23", "SOIC-8"]);
        // The shrink-only fitter has nothing to shrink.
        let empty = Design {
            schema: 1,
            provenance: crate::ir::Provenance { engine_version: "0".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(crate::ir::PlacementSection { outline: Vec::new(), footprints: Vec::new(), modules: Vec::new() }),
            routing: None,
        };
        assert!(fit_outline(&empty, &m, 0.25, 0.0).placement.unwrap().outline.is_empty());

        let o = square_for_parts(&m, 0.25).expect("a square");
        let side = o[1].x;
        assert_eq!(o, vec![Point { x: 0, y: 0 }, Point { x: side, y: 0 }, Point { x: side, y: side }, Point { x: 0, y: side }]);
        assert_eq!(side % 1000, 0, "whole millimetres");
        // The smallest whole-millimetre square the parts fill no more than a quarter of.
        let area = intrinsic_part_area(&m);
        assert!(area / (side as f64).powi(2) <= 0.25, "{side} um is too small");
        assert!(area / ((side - 1000) as f64).powi(2) > 0.25, "{side} um is bigger than it needs to be");
    }

    #[test]
    fn a_fitted_square_holds_its_widest_part() {
        // One 1x10 header: 25.4 mm long, too little area to need that side.
        let m = model(&["PINHEADER-10"]);
        let o = square_for_parts(&m, 0.25).expect("a square");
        let (hw, hh) = m.footprint_of(&m.parts[0]).unwrap().courtyard_half();
        assert!(o[1].x >= 2 * hw.max(hh) + 1000, "side {} for a part {} long", o[1].x, 2 * hw.max(hh));
    }

    #[test]
    fn nothing_to_size_from_is_none() {
        assert_eq!(square_for_parts(&ConstraintModel::default(), 0.25), None);
    }
}
