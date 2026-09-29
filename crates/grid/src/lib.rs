//! `eda-grid` — the board as an occupancy grid, for the checks that ask
//! where copper could still go: whether every pad can leave the pocket
//! placement left it in (`preflight`), and whether a routed pour still
//! reaches every pad of its net (`check_pours`). The same legality rules
//! the routing gates hold copper to -- pads, the strips between adjacent
//! SMD pads, refdes labels, the board edge -- rasterised at the board's
//! routing grid. Routing itself is `eda-freeroute`'s.

pub mod grid;
pub mod pads;
pub mod pour;

use eda_model::ir::{Design, Point, Track, Via};
use eda_model::{CheckResult, ConstraintModel};
use grid::{Grid, Occ};
use std::collections::{HashMap, HashSet, VecDeque};

/// The model's board rules, under the name this crate's API has always
/// used.
pub use eda_model::BoardRules as RouteRules;

/// A "REF.PIN"-addressed pad and its net (or a synthetic private net for
/// unassigned pins, so they still act as physical obstacles).
struct PadInfo {
    pt: Point,
    size: (i64, i64),
    /// Full board-space pad geometry (shape-aware containment).
    geom: eda_model::footprint::PlacedPad,
    net: String,
    /// Copper layers this pad exists on.
    layers: Vec<u8>,
}

/// Grid cells whose centre lies strictly inside the pad's copper, so a
/// track ending on any of them is guaranteed to touch the pad. Computed
/// from the real rectangle: a pad centre is rarely grid-aligned.
fn pad_interior_cells(grid: &Grid, pad: &PadInfo, rules: &RouteRules) -> Vec<(i64, i64)> {
    let (cx, cy) = grid.to_cell(pad.pt);
    let hx = (pad.size.0 / 2 + rules.grid) / rules.grid;
    let hy = (pad.size.1 / 2 + rules.grid) / rules.grid;
    // A track ending on a goal cell must keep its whole copper width
    // inside the pad (KiCad connectivity wants the end point inside the
    // pad shape; rounded corners and circles have less copper than their
    // bounding box). The centre cell is always included: on tiny pads the
    // margin may exceed the pad, and the centre is the best we can do.
    let margin = (rules.track_width as f64) / 2.0;
    let mut out = vec![(cx, cy)];
    for dx in -hx..=hx {
        for dy in -hy..=hy {
            let p = grid.to_point(cx + dx, cy + dy);
            if (dx, dy) != (0, 0) && pad.geom.contains_with_margin(p, margin) {
                out.push((cx + dx, cy + dy));
            }
        }
    }
    out
}

/// Grid cells a pad's copper is rasterised over: every cell whose own
/// square intersects the pad rectangle (the pad edge never lies more
/// than half a cell past an occupied cell centre — the invariant `Grid`'s
/// clearance arithmetic relies on), plus the centre cell.
fn pad_cells(grid: &Grid, pad: &PadInfo) -> Vec<(i64, i64)> {
    let (x0, y0) = grid.to_cell(Point { x: pad.pt.x - pad.size.0 / 2, y: pad.pt.y - pad.size.1 / 2 });
    let (x1, y1) = grid.to_cell(Point { x: pad.pt.x + pad.size.0 / 2, y: pad.pt.y + pad.size.1 / 2 });
    let mut out = vec![grid.to_cell(pad.pt)];
    for gx in x0..=x1 {
        for gy in y0..=y1 {
            out.push((gx, gy));
        }
    }
    out
}

/// Smallest facing-edge gap between two SMD pads of the same footprint
/// (adjacent pins on the same package), µm; `i64::MAX` when the board has
/// no multi-pad SMD footprint. Decides whether the grid needs refining to
/// see a fine-pitch part's escapes at all.
fn smallest_same_footprint_gap(pads: &HashMap<String, PadInfo>) -> i64 {
    let mut by_fp: HashMap<&str, Vec<&PadInfo>> = HashMap::new();
    for (refpin, p) in pads {
        if p.geom.through_hole {
            continue;
        }
        let r = refpin.rsplit_once('.').map(|(r, _)| r).unwrap_or(refpin);
        by_fp.entry(r).or_default().push(p);
    }
    let mut min_gap = i64::MAX;
    for fpads in by_fp.values() {
        for i in 0..fpads.len() {
            for j in i + 1..fpads.len() {
                let (a, b) = (fpads[i], fpads[j]);
                let ra = (a.pt.x - a.size.0 / 2, a.pt.y - a.size.1 / 2, a.pt.x + a.size.0 / 2, a.pt.y + a.size.1 / 2);
                let rb = (b.pt.x - b.size.0 / 2, b.pt.y - b.size.1 / 2, b.pt.x + b.size.0 / 2, b.pt.y + b.size.1 / 2);
                let (oy0, oy1) = (ra.1.max(rb.1), ra.3.min(rb.3));
                let (ox0, ox1) = (ra.0.max(rb.0), ra.2.min(rb.2));
                let gap = if oy1 > oy0 && (ra.2 <= rb.0 || rb.2 <= ra.0) {
                    if ra.2 <= rb.0 { rb.0 - ra.2 } else { ra.0 - rb.2 }
                } else if ox1 > ox0 && (ra.3 <= rb.1 || rb.3 <= ra.1) {
                    if ra.3 <= rb.1 { rb.1 - ra.3 } else { ra.1 - rb.3 }
                } else {
                    continue;
                };
                min_gap = min_gap.min(gap);
            }
        }
    }
    min_gap
}

/// Hard-block the gap strip between two SMD pads of one footprint whose
/// copper is closer than `between_pads_max_gap_um`: the strip lies under the
/// component body, and a track threading it (any net, the pads' own
/// included) is what a reviewer sends back. Through-hole pin rows are
/// exempt — routing between header pins is standard practice.
fn block_pad_gaps(grid: &mut Grid, pads: &HashMap<String, PadInfo>) {
    let mut by_fp: HashMap<&str, Vec<&PadInfo>> = HashMap::new();
    for (refpin, p) in pads {
        if p.geom.through_hole {
            continue;
        }
        let r = refpin.rsplit_once('.').map(|(r, _)| r).unwrap_or(refpin);
        by_fp.entry(r).or_default().push(p);
    }
    for fpads in by_fp.values() {
        for i in 0..fpads.len() {
            for j in i + 1..fpads.len() {
                let (a, b) = (fpads[i], fpads[j]);
                let ra = (a.pt.x - a.size.0 / 2, a.pt.y - a.size.1 / 2, a.pt.x + a.size.0 / 2, a.pt.y + a.size.1 / 2);
                let rb = (b.pt.x - b.size.0 / 2, b.pt.y - b.size.1 / 2, b.pt.x + b.size.0 / 2, b.pt.y + b.size.1 / 2);
                let (ox0, ox1) = (ra.0.max(rb.0), ra.2.min(rb.2));
                let (oy0, oy1) = (ra.1.max(rb.1), ra.3.min(rb.3));
                // Strip between the facing edges, limited to the pads'
                // shared extent along the other axis.
                let strip = if oy1 > oy0 && (ra.2 <= rb.0 || rb.2 <= ra.0) {
                    let (gx0, gx1) = if ra.2 <= rb.0 { (ra.2, rb.0) } else { (rb.2, ra.0) };
                    if gx1 - gx0 > grid.tuning.between_pads_max_gap_um { continue }
                    (gx0, oy0, gx1, oy1)
                } else if ox1 > ox0 && (ra.3 <= rb.1 || rb.3 <= ra.1) {
                    let (gy0, gy1) = if ra.3 <= rb.1 { (ra.3, rb.1) } else { (rb.3, ra.1) };
                    if gy1 - gy0 > grid.tuning.between_pads_max_gap_um { continue }
                    (ox0, gy0, ox1, gy1)
                } else {
                    continue;
                };
                // Only adjacent pairs: a strip that crosses a third pad of
                // the footprint (pins 1 and 3 of a row) is not a gap.
                let crosses_other = fpads.iter().enumerate().any(|(k, o)| {
                    k != i && k != j && {
                        let ro = (o.pt.x - o.size.0 / 2, o.pt.y - o.size.1 / 2, o.pt.x + o.size.0 / 2, o.pt.y + o.size.1 / 2);
                        ro.0 < strip.2 && ro.2 > strip.0 && ro.1 < strip.3 && ro.3 > strip.1
                    }
                });
                if crosses_other {
                    continue;
                }
                let (c0x, c0y) = grid.to_cell(Point { x: strip.0, y: strip.1 });
                let (c1x, c1y) = grid.to_cell(Point { x: strip.2, y: strip.3 });
                for cx in c0x..=c1x {
                    for cy in c0y..=c1y {
                        let p = grid.to_point(cx, cy);
                        if p.x > strip.0 && p.x < strip.2 && p.y > strip.1 && p.y < strip.3 {
                            for &l in &a.layers {
                                grid.set_blocked(cx, cy, l, true);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Mark the cells under each refdes label as a keep-out for tracks and
/// vias. Silkscreen text over
/// copper is legal, but the label becomes unreadable; the judge renders
/// each refdes centred just above its courtyard on the part's own side
/// (see `eda_judge::render_board_svg` and `eda_gates::pcb::refdes_box`,
/// which this mirrors).
fn mark_refdes_boxes(grid: &mut Grid, placement: &eda_model::ir::PlacementSection, model: &ConstraintModel, num_layers: usize) {
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(bx) = eda_model::footprint::placed_refdes_box(model, &placement.outline, part, fp) else { continue };
        let layer = match fp.side {
            eda_model::ir::Side::Top => 0u8,
            eda_model::ir::Side::Bottom => (num_layers - 1) as u8,
        };
        // Two separate dilations, matching the two shapes the gate itself
        // checks against the label box: a track's centreline (dilated by
        // its own half-width) and a via's copper (dilated by the via
        // radius). Using the via radius for both — as a single keep-out
        // used to — over-blocks the track case by up to a via-vs-track
        // width difference on every side, which is enough to wall off a
        // pad on a dense board where a label's soft box sits close to a
        // neighbour's escape route with no other legal cell to spare.
        let dt = grid.track_half_um;
        let dv = grid.via_half_um;
        let (t0x, t0y) = grid.to_cell(Point { x: bx.0 - dt, y: bx.1 - dt });
        let (t1x, t1y) = grid.to_cell(Point { x: bx.2 + dt, y: bx.3 + dt });
        for gx in t0x..=t1x {
            for gy in t0y..=t1y {
                grid.set_soft(gx, gy, layer);
            }
        }
        let (v0x, v0y) = grid.to_cell(Point { x: bx.0 - dv, y: bx.1 - dv });
        let (v1x, v1y) = grid.to_cell(Point { x: bx.2 + dv, y: bx.3 + dv });
        for gx in v0x..=v1x {
            for gy in v0y..=v1y {
                grid.set_soft_via(gx, gy, layer);
            }
        }
    }
}

/// The routing obstacle map for a placement: every pad registered with
/// exact geometry, the between-pad strips hard-blocked, the refdes label
/// keep-outs marked, on the effective (possibly refined) grid.
pub(crate) struct ObstacleMap {
    pub grid: Grid,
    pub pads: HashMap<String, PadInfo>,
    pub rules: RouteRules,
    pub num_layers: usize,
}

pub(crate) fn obstacle_map(placement: &eda_model::ir::PlacementSection, model: &ConstraintModel, rules: &RouteRules) -> Result<ObstacleMap, Vec<CheckResult>> {
    let dbg = std::env::var_os("EDA_ROUTE_DEBUG").is_some();
    let t_start = std::time::Instant::now();
    // --- 1. pad geometry -----------------------------------------------
    let num_layers = rules.layers.len();
    let mut pads: HashMap<String, PadInfo> = HashMap::new();
    let mut precondition: Vec<CheckResult> = Vec::new();
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else {
            precondition.push(CheckResult::fail("route_precondition", &fp.id, "placed footprint has no part in the model"));
            continue;
        };
        // SMD pads exist only on their side's outer copper layer; through
        // hole pads exist on every layer.
        let side_layer = match fp.side {
            eda_model::ir::Side::Top => 0u8,
            eda_model::ir::Side::Bottom => (num_layers - 1) as u8,
        };
        let Some(geoms) = pads::pad_positions(model, part, fp) else {
            precondition.push(CheckResult::fail(
                "route_precondition",
                &fp.id,
                format!(
                    "no footprint geometry for part (footprint={:?}, package={:?}); define it in `footprints` or use a built-in package name",
                    part.footprint, part.package
                ),
            ));
            continue;
        };
        for g in geoms {
            let refpin = format!("{}.{}", fp.id, g.number);
            let net = model
                .nets
                .iter()
                .find(|n| n.pins.iter().any(|p| p == &refpin))
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("__unassigned__{refpin}"));
            let layers = if g.through_hole { (0..num_layers as u8).collect() } else { vec![side_layer] };
            pads.insert(refpin, PadInfo { pt: g.center, size: g.size, geom: g.clone(), net, layers });
        }
    }
    if !precondition.is_empty() {
        return Err(precondition);
    }

    // The configured grid is a design-rule pitch, not necessarily a
    // resolution a pad can be seen to escape a fine-pitch part on: a TSSOP at a
    // 650 um pin pitch, say, has no grid cell landing on the centre line
    // between two adjacent pads at a 254 um grid, and the pad-to-pad gap
    // (pitch minus pad size) is regularly smaller than one such cell, so
    // the router sees a wall around the part instead of the real,
    // routable gaps beside it. Halving the grid (still coarser than most
    // clearances, so correctness is untouched) gives escape routing a
    // real shot at those gaps without the cost of an arbitrary GCD-of-
    // pitches grid, which for typical pitch/grid combinations (654 vs
    // 254, gcd 2) would be absurdly fine.
    //
    // This is only worth the (substantial — up to ~4x cells, ~4x search
    // cost) resolution increase when the board actually has a fine-pitch
    // part: scan same-footprint adjacent SMD pad pairs (the same notion
    // `block_pad_gaps` uses below) for the smallest facing-edge gap, and
    // only refine the grid when that gap wouldn't fit a single configured
    // cell. Boards without such a part keep the coarse grid and its
    // established performance.
    let min_pad_gap = smallest_same_footprint_gap(&pads);
    let mut eff_rules = rules.clone();
    if rules.grid > 130 && min_pad_gap < rules.grid {
        eff_rules.grid = rules.grid / 2;
    }
    if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
        eprintln!("route: min_pad_gap={min_pad_gap} eff_grid={}", eff_rules.grid);
    }
    let rules = &eff_rules;

    // --- 2. grid & obstacle map -----------------------------------------
    if dbg { eprintln!("route: t pads {:?}", t_start.elapsed()); }
    let mut grid = Grid::with_widths(placement.outline.clone(), rules.grid, rules.clearance, rules.track_width, rules.via_diameter, num_layers);
    // Declare class widths before any copper is stamped: each distinct
    // width gets its own clearance summary, and summaries only build
    // correctly from empty.
    for net in &model.nets {
        let w = rules.width_of(&net.name);
        if w != rules.track_width {
            grid.set_net_width(&net.name, w);
        }
    }
    grid.tuning = rules.tuning.clone();
    for pad in pads.values() {
        // Rasterise the pad area from its real rectangle: every cell whose
        // own square intersects the copper (the cell nearest a corner is
        // the cell whose square contains it), so the pad edge never lies
        // more than half a cell past an occupied cell centre — the
        // invariant `Grid`'s clearance arithmetic relies on. Centring on
        // the rounded centre cell instead can miss a row when the pad
        // centre is off-grid.
        let cells = pad_cells(&grid, pad);
        for &l in &pad.layers {
            grid.add_pad(&pad.net, l, &pad.geom, &cells);
        }
    }
    block_pad_gaps(&mut grid, &pads);
    mark_refdes_boxes(&mut grid, placement, model, num_layers);
    Ok(ObstacleMap { grid, pads, rules: eff_rules, num_layers })
}

/// Placement-stage routability preflight, with the router's own legality
/// rules and obstacle map (pads, between-pad strips, label keep-outs, board
/// edge) and no tracks yet: every pad must be able to leave its pocket. A
/// pad passes when a flood over passable track cells on one of its layers
/// reaches open space (`REACH_OPEN` cells), a legal via site (so the net
/// can change layer), or a pad of its own net. Otherwise the pocket is
/// closed by placement alone and no rip-up can ever open it (l2 with the
/// Cypress placement: decoupling caps legalised flush against U2's pin
/// column) — `placement_pad_reach` fails and the placement goes back.
pub fn preflight(design: &Design, model: &ConstraintModel, rules: &RouteRules) -> Vec<CheckResult> {
    let Some(placement) = design.placement.as_ref() else {
        return vec![CheckResult::fail("placement_pad_reach", "design", "no placement section")];
    };
    if placement.outline.len() < 3 {
        return vec![CheckResult::fail("placement_pad_reach", "design.placement.outline", "board outline needs at least 3 points")];
    }
    let ObstacleMap { mut grid, pads, rules: eff_rules, num_layers } = match obstacle_map(placement, model, rules) {
        Ok(m) => m,
        Err(f) => return f,
    };
    let rules = &eff_rules;
    grid.soft_active = true;
    grid.soft_via_active = true;
    let mut out = Vec::new();
    let mut refs: Vec<&String> = pads.keys().collect();
    refs.sort();
    for refpin in refs {
        let pad = &pads[refpin];
        if pad.net.starts_with("__unassigned__") {
            continue;
        }
        let mut reached = false;
        let mut best_pocket = usize::MAX;
        // Cells of the net's *other* pads (any layer): reaching one is an
        // exit. The previous test (`is_pad_of && not interior`) matched
        // this pad's own outer rasterised cells and passed every sealed pad
        // (l4: D5.1 boxed in by four neighbours' labels, preflight green).
        let other_pad_cells: HashSet<(i64, i64, u8)> = pads
            .iter()
            .filter(|(r, p)| *r != refpin && p.net == pad.net)
            .flat_map(|(_, p)| {
                let cells = pad_cells(&grid, p);
                p.layers.iter().flat_map(move |&l| cells.clone().into_iter().map(move |(x, y)| (x, y, l))).collect::<Vec<_>>()
            })
            .collect();
        for &l in &pad.layers {
            let mut seen: HashSet<(i64, i64)> = HashSet::new();
            let mut queue: VecDeque<(i64, i64)> = VecDeque::new();
            for c in pad_interior_cells(&grid, pad, rules) {
                if seen.insert(c) {
                    queue.push_back(c);
                }
            }
            let mut exit = false;
            while let Some((cx, cy)) = queue.pop_front() {
                if seen.len() >= grid.tuning.preflight_reach_cells {
                    exit = true;
                    break;
                }
                // A legal via site: via copper clear here on this layer and
                // on some other layer, and not over any pad.
                if num_layers > 1
                    && !grid.via_near_pad(cx, cy, l)
                    && grid.passable_as(cx, cy, l, &pad.net, Occ::Via)
                    && (0..num_layers as u8).any(|o| o != l && !grid.via_near_pad(cx, cy, o) && grid.passable_as(cx, cy, o, &pad.net, Occ::Via))
                {
                    exit = true;
                    break;
                }
                // Another pad of the same net.
                if other_pad_cells.contains(&(cx, cy, l)) {
                    exit = true;
                    break;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let n = (cx + dx, cy + dy);
                    if !seen.contains(&n) && grid.passable_as(n.0, n.1, l, &pad.net, Occ::Track) {
                        seen.insert(n);
                        queue.push_back(n);
                    }
                }
            }
            if exit {
                reached = true;
                break;
            }
            best_pocket = best_pocket.min(seen.len());
        }
        if std::env::var("EDA_PREFLIGHT_PAD").ok().as_deref() == Some(refpin.as_str()) {
            eprintln!("preflight: {refpin} net={} layers={:?} reached={reached} smallest_pocket={best_pocket} cells", pad.net, pad.layers);
            let (cx, cy) = grid.to_cell(pad.pt);
            eprintln!("{}", grid.dump_why(cx, cy, pad.layers[0], &pad.net, 14));
        }
        if !reached {
            let um = best_pocket as f64 * (rules.grid as f64 / 1000.0).powi(2);
            out.push(CheckResult::fail(
                "placement_pad_reach",
                refpin,
                format!("pad is sealed in by placement: its free pocket is {best_pocket} cells (~{um:.1} mm²) with no via site and no same-net pad reachable"),
            ));
        }
    }
    if out.is_empty() {
        out.push(CheckResult::pass("placement_pad_reach"));
    }
    out
}

/// Whether each pour of a routed design still reaches every pad of its
/// net, once every track and via of the design has carved the plane: from
/// the plane's body through the net's own copper -- its tracks, its vias,
/// its pads -- on any layer. The check this router makes of its own
/// routing (`routing_pour_cut_off`), for a design routed some other way,
/// whose net may reach the plane by tracks as well as by stitching vias.
/// As conservative as the router's own flood: the plane is only where a
/// track of its net could lie.
pub fn check_pours(design: &Design, model: &ConstraintModel, rules: &RouteRules) -> Vec<CheckResult> {
    let (Some(placement), Some(routing)) = (&design.placement, &design.routing) else { return Vec::new() };
    if rules.pours.is_empty() {
        return Vec::new();
    }
    let ObstacleMap { mut grid, pads, rules, .. } = match obstacle_map(placement, model, rules) {
        Ok(m) => m,
        Err(f) => return f,
    };
    stamp_routed(&mut grid, &routing.tracks, &routing.vias, &rules);
    let mut fails = Vec::new();
    for spec in &rules.pours {
        let Some(layer) = rules.layers.iter().position(|l| *l == spec.layer).map(|l| l as u8) else {
            fails.push(CheckResult::fail("routing_pour_layer", &spec.net, format!("pour names layer {}, which is not in the board stackup {:?}", spec.layer, rules.layers)));
            continue;
        };
        // The net's own copper, cell by cell and layer by layer, and where
        // it passes between layers: its vias and its through-hole pads.
        let net_pads: Vec<(&String, &PadInfo)> = pads.iter().filter(|(_, p)| p.net == spec.net).collect();
        let mut own: HashSet<(i64, i64, u8)> = grid.routed_cells_of(&spec.net).into_iter().collect();
        let mut through: HashSet<(i64, i64)> = routing.vias.iter().filter(|v| v.net == spec.net).map(|v| grid.to_cell(v.at)).collect();
        for (_, p) in &net_pads {
            let cells = pad_cells(&grid, p);
            for &l in &p.layers {
                own.extend(cells.iter().map(|&(x, y)| (x, y, l)));
            }
            if p.layers.len() > 1 {
                through.extend(cells.iter().copied());
            }
        }
        // Flood from the plane's body through it.
        let mut seen: HashSet<(i64, i64, u8)> = HashSet::new();
        let mut queue: VecDeque<(i64, i64, u8)> = VecDeque::new();
        for (x, y) in pour::body_cells(&grid, spec, layer) {
            if seen.insert((x, y, layer)) {
                queue.push_back((x, y, layer));
            }
        }
        while let Some((x, y, l)) = queue.pop_front() {
            // Eight ways: a 45-degree track steps corner to corner.
            let mut next: Vec<(i64, i64, u8)> = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)].iter().map(|&(dx, dy)| (x + dx, y + dy, l)).collect();
            if through.contains(&(x, y)) {
                next.extend((0..grid.num_layers as u8).map(|m| (x, y, m)));
            }
            for n in next {
                if own.contains(&n) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        let mut cut: Vec<String> = net_pads
            .iter()
            .filter(|(_, p)| !pad_cells(&grid, p).iter().any(|&(x, y)| p.layers.iter().any(|&l| seen.contains(&(x, y, l)))))
            .map(|(r, _)| (*r).clone())
            .collect();
        cut.sort();
        if !cut.is_empty() {
            fails.push(CheckResult::fail(
                "routing_pour_cut_off",
                &spec.net,
                format!(
                    "the {} pour on {} does not reach {} of the {} pads of its net, by the plane or by the net's own copper, once the board's copper has cut it: {}. \
                     The copper that isolated them has to move, or the plane needs a path back; a pad the plane does not reach is floating.",
                    spec.net,
                    spec.layer,
                    cut.len(),
                    net_pads.len(),
                    cut.join(", ")
                ),
            ));
        }
    }
    fails
}

/// Replay routed tracks and vias into the grid's occupancy.
///
/// The pour check asks the grid about free space, and needs the board as
/// it was routed, not as placement left it.
fn stamp_routed(grid: &mut Grid, tracks: &[Track], vias: &[Via], rules: &RouteRules) {
    let layer_of = |name: &str| rules.layers.iter().position(|l| l == name).map(|i| i as u8);
    for t in tracks {
        let Some(layer) = layer_of(&t.layer) else { continue };
        for pair in t.pts.windows(2) {
            let (ax, ay) = grid.to_cell(pair[0]);
            let (bx, by) = grid.to_cell(pair[1]);
            let n = (bx - ax).abs().max((by - ay).abs());
            for i in 0..=n {
                // Segments are grid-aligned or diagonal; interpolating on
                // the longer axis walks every cell either way.
                let (x, y) = if n == 0 {
                    (ax, ay)
                } else {
                    (ax + (bx - ax) * i / n, ay + (by - ay) * i / n)
                };
                grid.set(x, y, layer, &t.net, Occ::Track);
            }
        }
    }
    for v in vias {
        let (x, y) = grid.to_cell(v.at);
        for l in 0..grid.num_layers {
            grid.set(x, y, l as u8, &v.net, Occ::Via);
        }
    }
}
