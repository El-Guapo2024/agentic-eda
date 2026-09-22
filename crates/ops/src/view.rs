//! The board as a language-action model sees it.
//!
//! One JSON object per decision: what is on the board, how the parts sit
//! relative to each other, and what is currently wrong with it. The
//! action taken against this view is a [`crate::Cmd`], and the reward is
//! the change in gate failures. Observation, action, reward -- the three
//! things a language-action model needs, in the three places this crate
//! already had them.
//!
//! # Why relations and not coordinates
//!
//! A part's position is a number, and handing a model numbers invites it
//! to do arithmetic it is bad at. Measured on geometry questions of
//! exactly that shape, the evaluation model scored 12 of 23, with
//! confidence that went *up* when it was wrong -- and those numbers are
//! ones our gates already compute exactly and instantly.
//!
//! So the view carries relations, which is also how a layout engineer
//! thinks: not "37400 µm" but "3.2 mm north-east of U2, same block,
//! shares AIN1_FILT". Distances are included where a rule turns on one,
//! already reduced to the thing that matters -- how far over the limit
//! it is. The model is asked what our gates cannot express: which
//! neighbour is the right one, where a section belongs, what to move.
//!
//! # Why no image
//!
//! Everything a render would convey about *this* problem can be said:
//! region occupancy names the dead corners, block extents name a
//! scattered section. A render adds pixels the model must interpret to
//! recover numbers we already have. If a judgement turns up that this
//! encoding provably cannot express, that is the moment to add one --
//! measured, not assumed.

use crate::Board;
use eda_model::{CheckResult, ConstraintModel};
use std::collections::{BTreeMap, BTreeSet};
use serde_json::{json, Value};

/// Compass direction from one part to another, as a person would say it.
fn bearing(dx: i64, dy: i64) -> &'static str {
    // Screen coordinates: +y is south.
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax * 3 < ay {
        return if dy < 0 { "north" } else { "south" };
    }
    if ay * 3 < ax {
        return if dx < 0 { "west" } else { "east" };
    }
    match (dx < 0, dy < 0) {
        (true, true) => "north-west",
        (false, true) => "north-east",
        (true, false) => "south-west",
        (false, false) => "south-east",
    }
}

fn mm(um: i64) -> f64 {
    (um as f64 / 100.0).round() / 10.0
}

/// How much of each ninth of the board is covered, so dead space is
/// visible without a picture.
fn regions(board: &Board, model: &ConstraintModel) -> Result<Value, Vec<CheckResult>> {
    let d = board.design();
    let pl = d.placement.as_ref().ok_or_else(|| {
        vec![CheckResult::fail("view_no_placement", "board", "the design has no placement section, so there is no board to describe")]
    })?;
    let bounds = outline_bounds(&pl.outline)?;
    let (x0, y0, x1, y1) = bounds;
    let (w, h) = (((x1 - x0) / 3).max(1), ((y1 - y0) / 3).max(1));
    let names = [
        ["north-west", "north", "north-east"],
        ["west", "centre", "east"],
        ["south-west", "south", "south-east"],
    ];
    let mut area = [[0f64; 3]; 3];
    let mut held: [[BTreeSet<String>; 3]; 3] = Default::default();
    for f in &pl.footprints {
        // A footprint that cannot be read is a failure, never a skip: a
        // part silently missing from the observation is a board the
        // model was never shown.
        let part = model.part(&f.id).ok_or_else(|| {
            vec![CheckResult::fail("view_unknown_part", &f.id, "placed footprint names a part that is not in the model")]
        })?;
        let c = eda_model::footprint::placed_courtyard(model, part, f).ok_or_else(|| {
            vec![CheckResult::fail("view_no_courtyard", &f.id, "this part resolves to no footprint, so its area is unknown")]
        })?;
        let col = (((f.at.x - x0) / w).clamp(0, 2)) as usize;
        let row = (((f.at.y - y0) / h).clamp(0, 2)) as usize;
        area[row][col] += ((c.2 - c.0) as f64) * ((c.3 - c.1) as f64);
        held[row][col].insert(f.id.clone());
    }
    let cell = (w as f64) * (h as f64);
    let mut out = Vec::new();
    for r in 0..3 {
        for c in 0..3 {
            out.push(json!({
                "region": names[r][c],
                "occupancy_pct": ((area[r][c] / cell) * 100.0).round(),
                "parts": held[r][c].iter().cloned().collect::<Vec<_>>(),
            }));
        }
    }
    Ok(json!(out))
}

/// The rectangle an outline spans, or a failure.
fn outline_bounds(outline: &[eda_model::ir::Point]) -> Result<(i64, i64, i64, i64), Vec<CheckResult>> {
    if outline.len() < 3 {
        return Err(vec![CheckResult::fail(
            "view_no_outline",
            "board",
            "the board outline has fewer than three points, so it encloses nothing",
        )]);
    }
    Ok((
        outline.iter().map(|p| p.x).min().expect("outline is non-empty"),
        outline.iter().map(|p| p.y).min().expect("outline is non-empty"),
        outline.iter().map(|p| p.x).max().expect("outline is non-empty"),
        outline.iter().map(|p| p.y).max().expect("outline is non-empty"),
    ))
}

/// Every failure, read as something to do about it rather than a verdict.
fn problems(board: &Board, model: &ConstraintModel) -> Value {
    let fixes = crate::repair::fixes_for(board, model);
    let mut out: Vec<Value> = fixes
        .iter()
        .map(|f| {
            json!({
                "check": f.check,
                "move": f.mover,
                "toward": f.toward,
                "apart_mm": mm(f.apart_um),
                "allowed_mm": mm(f.allowed_um),
                "close_by_mm": mm(f.close_um()),
                "actionable": true,
            })
        })
        .collect();
    // Everything the repair pass cannot read is still shown, marked as
    // such: a model that is only told about solvable problems will think
    // the board is better than it is.
    for c in crate::repair::unactionable(board) {
        out.push(json!({
            "check": c.check,
            "where": c.location,
            "detail": c.detail,
            "actionable": false,
        }));
    }
    json!(out)
}

/// The board as one JSON object, or a failure saying why it cannot be
/// described.
///
/// Strict on purpose. Every fallback this used to take -- a missing
/// outline read as zero, an unresolvable footprint skipped, an absent
/// placement returned as an empty list -- produced a view that looked
/// complete and described a board that did not exist. A decision layer
/// cannot tell a quiet omission from a real absence, and neither can a
/// training set.
pub fn view(board: &Board, model: &ConstraintModel) -> Result<Value, Vec<CheckResult>> {
    let placed = board.placed();
    let d = board.design();
    let pl = d.placement.as_ref().ok_or_else(|| {
        vec![CheckResult::fail("view_no_placement", "board", "the design has no placement section, so there is no board to describe")]
    })?;
    let (bx0, by0, bx1, by1) = outline_bounds(&pl.outline)?;
    let (modules, free) = eda_model::floorplan::partition(model);
    let mut block_of: BTreeMap<String, String> = BTreeMap::new();
    for m in &modules {
        for r in &m.refs {
            block_of.insert(r.clone(), m.name.clone());
        }
    }

    let pinned: BTreeSet<String> = model
        .parts
        .iter()
        .filter(|p| eda_model::footprint::is_edge_connector(p))
        .map(|p| p.reference.clone())
        .collect();

    let mut parts = Vec::new();
    for p in &model.parts {
        let r = &p.reference;
        let fp = pl.footprints.iter().find(|f| f.id == *r);
        // Nearest placed neighbours, by relation rather than position.
        let mut near: Vec<Value> = Vec::new();
        if let Some(fp) = fp {
            let mut cand: Vec<(i64, &eda_model::ir::FootprintInstance)> = pl
                .footprints
                .iter()
                .filter(|o| o.id != *r)
                .map(|o| {
                    let (dx, dy) = (o.at.x - fp.at.x, o.at.y - fp.at.y);
                    (((dx * dx + dy * dy) as f64).sqrt() as i64, o)
                })
                .collect();
            cand.sort_by_key(|(d, _)| *d);
            for (dist, o) in cand.into_iter().take(4) {
                let shared: Vec<&str> = model
                    .nets
                    .iter()
                    .filter(|n| {
                        let ps: Vec<&str> = n.pins.iter().map(|x| x.split('.').next().unwrap_or(x)).collect();
                        ps.contains(&r.as_str()) && ps.contains(&o.id.as_str())
                    })
                    .map(|n| n.name.as_str())
                    .collect();
                near.push(json!({
                    "part": o.id,
                    "bearing": bearing(o.at.x - fp.at.x, o.at.y - fp.at.y),
                    "distance_mm": mm(dist),
                    "same_block": block_of.get(&o.id) == block_of.get(r),
                    "shared_nets": shared,
                }));
            }
        }
        parts.push(json!({
            "ref": r,
            "value": p.value,
            "mpn": p.mpn,
            "package": p.package,
            "block": block_of.get(r),
            "placed": placed.contains(r),
            "pinned_to_edge": pinned.contains(r),
            "rotation_deg": fp.map(|f| f.rot / 1000),
            "near": near,
        }));
    }

    let problems_v = problems(board, model);
    let actionable = problems_v.as_array().map(|a| a.iter().filter(|p| p["actionable"] == json!(true)).count()).unwrap_or(0);
    let blocked = problems_v.as_array().map(|a| a.len()).unwrap_or(0) - actionable;
    // One line saying where this board stands, so a reader does not have
    // to derive it from three other fields.
    let status = if placed.len() < model.parts.len() {
        "placing"
    } else if board.failures() == 0 {
        "clean"
    } else if actionable > 0 {
        "repairable"
    } else {
        "stuck"
    };
    Ok(json!({
        "status": status,
        "board": {
            "width_mm": mm(bx1 - bx0),
            "height_mm": mm(by1 - by0),
            "placed": placed.len(),
            "total": model.parts.len(),
            "failures": board.failures(),
            "actionable": actionable,
            "blocked": blocked,
        },
        "blocks": modules.iter().map(|m| json!({ "name": m.name, "parts": m.refs })).collect::<Vec<_>>(),
        "unassigned": free,
        "regions": regions(board, model)?,
        "problems": problems_v,
        "parts": parts,
    }))
}
