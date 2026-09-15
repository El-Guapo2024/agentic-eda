//! Copper pours: the plane a real board carries its ground on.
//!
//! A poured net is not track-routed. Its pads reach each other through a
//! sheet of copper flooded across one layer, and the router's only job is
//! to answer the question that makes the claim honest: does that sheet
//! actually reach every pad?
//!
//! The answer is computed on the same occupancy grid the rest of the
//! router uses, so a pour is checked against the model the cross-check
//! gate already holds to the geometry gates. The flood is deliberately
//! *conservative*: a cell joins the plane only if the net could legally
//! put a track there, and a real filler lays copper into gaps thinner
//! than a track. Under-reporting reach costs a stitching via we did not
//! strictly need; over-reporting would report a connected plane on a
//! board with an isolated pad, which is the failure this module exists
//! to make impossible.

use crate::grid::{Grid, Occ};
use eda_model::ir::{Point, Track, Um, Via, Zone};
use eda_model::Pour;
use std::collections::VecDeque;

/// A pad of the poured net that the plane has to pick up.
pub struct PourPad {
    pub refpin: String,
    /// The poured net's name, for own-pad tests along a stub.
    pub refpin_net: String,
    pub at: Point,
    /// Copper layers the pad exists on.
    pub layers: Vec<u8>,
    /// Candidate cells the pad's copper covers.
    pub cells: Vec<(i64, i64)>,
    /// Where a stub should leave the pad: the pad's own grid cell centre
    /// when that still lands inside the copper (keeping the track on
    /// grid), else the pad centre, because connecting beats being tidy.
    pub start: Point,
}

pub struct PourResult {
    pub zone: Zone,
    /// Stitching vias dropped to pull an off-layer pad down to the plane.
    pub vias: Vec<Via>,
    /// Stubs joining a pad to its stitching via, when the via could not
    /// sit on the pad's own copper.
    pub tracks: Vec<Track>,
    /// Pads the plane could not reach, as "REF.PIN".
    pub unreached: Vec<String>,
    /// Where each reached pad enters the plane, in cells on the poured
    /// layer: its own copper for a pad already on that layer, else the
    /// cell its stitching via punches through. `verify` re-tests exactly
    /// these points once the signals are down.
    pub entries: Vec<(String, (i64, i64))>,
}

/// Cells on `layer` the poured net may occupy, as a flood-filled component
/// map. `None` means the cell is not pourable at all.
fn components(grid: &Grid, layer: u8, net_id: u32) -> (Vec<i32>, usize) {
    let w = grid.cells_x;
    let h = grid.cells_y;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let mut comp = vec![-1i32; (w * h) as usize];
    let mut n = 0usize;
    for sy in 0..h {
        for sx in 0..w {
            if comp[idx(sx, sy)] != -1 {
                continue;
            }
            if !grid.in_outline(sx, sy) || !grid.passable_as_id(sx, sy, layer, net_id, Occ::Track) {
                continue;
            }
            let id = n as i32;
            n += 1;
            let mut q = VecDeque::from([(sx, sy)]);
            comp[idx(sx, sy)] = id;
            while let Some((x, y)) = q.pop_front() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let i = idx(nx, ny);
                    if comp[i] != -1 {
                        continue;
                    }
                    if !grid.in_outline(nx, ny) || !grid.passable_as_id(nx, ny, layer, net_id, Occ::Track) {
                        continue;
                    }
                    comp[i] = id;
                    q.push_back((nx, ny));
                }
            }
        }
    }
    (comp, n)
}

/// Pour `spec` over `grid` and report what it reaches.
///
/// `pads` are the poured net's pads; `outline` is the board edge, which
/// the zone polygon follows -- the filler clips it to the real clearance,
/// and our own reach test is what decides whether that is good enough.
pub fn pour(
    grid: &Grid,
    spec: &Pour,
    layer: u8,
    pads: &[PourPad],
    outline: &[Point],
    via_drill: Um,
    via_diameter: Um,
    layers: &[String],
) -> PourResult {
    let net_id = grid.net_id_of(&spec.net);
    let (comp, ncomp) = components(grid, layer, net_id);
    let w = grid.cells_x;
    let idx = |x: i64, y: i64| (y * w + x) as usize;

    // The plane is the largest pourable component. Smaller ones are
    // islands the filler would drop; treating them as plane would be the
    // lie this module is built to prevent.
    let mut size = vec![0usize; ncomp];
    for &c in &comp {
        if c >= 0 {
            size[c as usize] += 1;
        }
    }
    let body = (0..ncomp).max_by_key(|&i| size[i]).map(|i| i as i32);

    let mut vias = Vec::new();
    let mut tracks = Vec::new();
    let mut unreached = Vec::new();
    let mut entries = Vec::new();
    for p in pads {
        // A pad already on the poured layer, sitting in the plane's own
        // component, is connected by the copper itself.
        if p.layers.contains(&layer) {
            if let Some(&cell) = p.cells.iter().find(|&&(x, y)| {
                grid.in_bounds(x, y) && body.is_some_and(|b| comp[idx(x, y)] == b)
            }) {
                entries.push((p.refpin.clone(), cell));
                continue;
            }
        }
        // Otherwise it needs a stitching via down to the plane, and the
        // via has to be *on* the pad or joined to it by a stub. A via
        // merely near the pad connects nothing.
        let pad_layer = p.layers.first().copied().unwrap_or(0);
        match stitch(grid, &comp, body, net_id, p, pad_layer) {
            Some((at, stub)) => {
                entries.push((p.refpin.clone(), grid.to_cell(at)));
                vias.push(Via {
                    net: spec.net.clone(),
                    at,
                    drill: via_drill,
                    diameter: via_diameter,
                    from_layer: layers[pad_layer as usize].clone(),
                    to_layer: spec.layer.clone(),
                });
                if stub.len() >= 2 {
                    tracks.push(Track {
                        net: spec.net.clone(),
                        pins: vec![p.refpin.clone()],
                        layer: layers[pad_layer as usize].clone(),
                        width: grid.net_track_half_of(&spec.net) * 2,
                        pts: stub,
                    });
                }
            }
            None => {
                if std::env::var_os("EDA_POUR_DEBUG").is_some() {
                    eprintln!("pour {}: {} unreachable -- {}", spec.net, p.refpin, why(grid, &comp, body, net_id, p, pad_layer));
                }
                unreached.push(p.refpin.clone())
            }
        }
    }

    PourResult {
        zone: Zone { net: spec.net.clone(), layer: spec.layer.clone(), outline: outline.to_vec() },
        vias,
        tracks,
        unreached,
        entries,
    }
}

/// A stitching via into the plane, with the stub that reaches it.
///
/// Searched within `pour_stitch_reach_um` of the pad rather than over the
/// whole board: a via found across the board is a track by another name.
///
/// The stub is routed with the router's own A*, not with a hand-rolled
/// shape. Straight-and-one-corner was the first cut, and the pour
/// diagnostic showed what it cost -- on L4, four to six hundred cells per
/// failing pad were sitting on the plane, legal for a via, and rejected
/// only because no straight or L-shaped stub happened to reach them. The
/// stub is a tiny routing problem; the router already solves those.
fn stitch(
    grid: &Grid,
    comp: &[i32],
    body: Option<i32>,
    net_id: u32,
    p: &PourPad,
    pad_layer: u8,
) -> Option<(Point, Vec<Point>)> {
    let b = body?;
    let w = grid.cells_x;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let (cx, cy) = grid.to_cell(p.start);
    let max_r = (grid.tuning.pour_stitch_reach_um / grid.grid_um).max(2);

    // Via-in-pad is a fab and assembly defect even on the via's own net
    // -- solder wicks down the barrel -- so a stitching via never sits on
    // pad copper, its own included. Silkscreen is a hard gate too
    // (`routing_over_refdes`), and the via's copper has to be legal on
    // every layer it punches through, not just the plane it lands on.
    let mut goals: std::collections::HashSet<(i64, i64, u8)> = std::collections::HashSet::new();
    for dy in -max_r..=max_r {
        for dx in -max_r..=max_r {
            let (x, y) = (cx + dx, cy + dy);
            if !grid.in_bounds(x, y)
                || !grid.in_outline(x, y)
                || comp[idx(x, y)] != b
                || grid.pad_within(x, y, pad_layer, grid.via_pad_radius_cells())
                || (0..grid.num_layers).any(|l| grid.is_soft_via(x, y, l as u8))
                || !(0..grid.num_layers).all(|l| grid.passable_as_id(x, y, l as u8, net_id, Occ::Via))
            {
                continue;
            }
            goals.insert((x, y, pad_layer));
        }
    }
    if goals.is_empty() {
        return None;
    }
    let h_targets: Vec<(i64, i64)> = goals.iter().map(|&(x, y, _)| (x, y)).collect();
    let path = crate::astar::route_to_any(grid, &p.refpin_net, &[(cx, cy, pad_layer)], &goals, &h_targets)?;
    let (vx, vy, _) = *path.last()?;
    // A stub that changes layer would plant a second via nobody asked
    // for. Goals are all on the pad's own layer, so a flat path is what
    // A* should return; reject anything else rather than emit geometry
    // this function does not model.
    if path.iter().any(|&(_, _, l)| l != pad_layer) {
        return None;
    }
    let at = grid.to_point(vx, vy);
    Some((at, polyline(grid, &path)))
}

/// A cell path as board points, keeping only the corners.
fn polyline(grid: &Grid, path: &[(i64, i64, u8)]) -> Vec<Point> {
    if path.len() < 2 {
        return Vec::new();
    }
    let mut out = vec![grid.to_point(path[0].0, path[0].1)];
    for i in 1..path.len() - 1 {
        let (a, b, c) = (path[i - 1], path[i], path[i + 1]);
        let turns = (b.0 - a.0, b.1 - a.1) != (c.0 - b.0, c.1 - b.1);
        if turns {
            out.push(grid.to_point(b.0, b.1));
        }
    }
    out.push(grid.to_point(path[path.len() - 1].0, path[path.len() - 1].1));
    out
}

/// Re-check a planned pour against the board the router actually built.
///
/// The plan runs before the signals route, so its reachability answer is
/// about an empty board. Signals laid afterwards can cut a pad off from
/// the plane -- KiCad's own DRC found exactly that on three L4 connector
/// pads that our gates were calling connected. This is the check that
/// makes the plan's promise true at the end rather than at the start.
///
/// Every pad, stub and stitching via of the poured net is its own copper,
/// so on a board where the plan held they all flood into one component
/// with the plane. A pad outside it is floating.
pub fn verify(grid: &Grid, spec: &Pour, layer: u8, entries: &[(String, (i64, i64))]) -> Vec<String> {
    let net_id = grid.net_id_of(&spec.net);
    let (comp, ncomp) = components(grid, layer, net_id);
    let mut size = vec![0usize; ncomp];
    for &c in &comp {
        if c >= 0 {
            size[c as usize] += 1;
        }
    }
    let Some(body) = (0..ncomp).max_by_key(|&i| size[i]).map(|i| i as i32) else {
        return entries.iter().map(|(r, _)| r.clone()).collect();
    };
    let w = grid.cells_x;
    let mut out: Vec<String> = entries
        .iter()
        .filter(|(_, (x, y))| !grid.in_bounds(*x, *y) || comp[(y * w + x) as usize] != body)
        .map(|(r, _)| r.clone())
        .collect();
    out.sort();
    out
}

/// Why no stitching via could be placed for `p`: a tally of what each
/// criterion rejected over the whole search area.
///
/// "Unreachable" on its own sends you looking at the plane when the real
/// answer is usually a silkscreen label sitting where the via wanted to
/// go -- the cheapest thing on the board to move, and invisible without
/// this breakdown.
fn why(grid: &Grid, comp: &[i32], body: Option<i32>, net_id: u32, p: &PourPad, pad_layer: u8) -> String {
    let Some(b) = body else { return "the pour has no body at all: nothing on this layer is pourable".into() };
    let w = grid.cells_x;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let (cx, cy) = grid.to_cell(p.at);
    let max_r = (grid.tuning.pour_stitch_reach_um / grid.grid_um).max(2);
    let (mut off_plane, mut in_pad, mut under_silk, mut blocked, mut no_stub) = (0, 0, 0, 0, 0);
    for dy in -max_r..=max_r {
        for dx in -max_r..=max_r {
            let (x, y) = (cx + dx, cy + dy);
            if !grid.in_bounds(x, y) || !grid.in_outline(x, y) {
                continue;
            }
            if comp[idx(x, y)] != b {
                off_plane += 1;
            } else if grid.pad_within(x, y, pad_layer, grid.via_pad_radius_cells()) {
                in_pad += 1;
            } else if (0..grid.num_layers).any(|l| grid.is_soft_via(x, y, l as u8)) {
                under_silk += 1;
            } else if !(0..grid.num_layers).all(|l| grid.passable_as_id(x, y, l as u8, net_id, Occ::Via)) {
                blocked += 1;
            } else {
                no_stub += 1;
            }
        }
    }
    format!(
        "within {max_r} cells: {off_plane} not on the plane, {in_pad} too close to pad copper, \
         {under_silk} under a silkscreen label, {blocked} blocked by other copper, {no_stub} reachable but with no legal stub"
    )
}

/// Cells that must stay clear of other nets so the plane can reach every
/// pad: a shortest-path tree on the poured layer joining all the entry
/// points.
///
/// This is the difference between hoping the signals leave the plane
/// intact and requiring it. A penalty ring around each entry only
/// perturbed the routing -- it fixed two L4 pads and broke a third. A
/// reserved corridor cannot be severed, because the negotiator sees the
/// net's own copper there and routes around it.
///
/// Nothing is emitted. The corridor is grid occupancy only: keep it free
/// of other nets and the filler lays plane copper along it by itself,
/// which is exactly what a human does when leaving a channel for a pour.
/// Emitting tracks would litter the board with GND stubs that the fill
/// makes redundant.
///
/// One BFS does the whole job. At plan time the plane is a single
/// component, so a breadth-first wave from the first entry reaches all of
/// them; walking each entry's parent chain back until it meets copper
/// already reserved yields a tree whose shared prefixes merge for free.
pub fn skeleton(grid: &Grid, spec: &Pour, layer: u8, entries: &[(String, (i64, i64))]) -> Vec<(i64, i64)> {
    if entries.len() < 2 {
        return Vec::new();
    }
    let net_id = grid.net_id_of(&spec.net);
    let w = grid.cells_x;
    let h = grid.cells_y;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let pourable = |x: i64, y: i64| {
        grid.in_bounds(x, y) && grid.in_outline(x, y) && grid.passable_as_id(x, y, layer, net_id, Occ::Track)
    };

    // -1 unvisited, -2 root; otherwise the index of the cell we came from.
    let mut parent = vec![-1i64; (w * h) as usize];
    let root = entries[0].1;
    if !pourable(root.0, root.1) {
        return Vec::new();
    }
    parent[idx(root.0, root.1)] = -2;
    let mut q = VecDeque::from([root]);
    while let Some((x, y)) = q.pop_front() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if !pourable(nx, ny) || parent[idx(nx, ny)] != -1 {
                continue;
            }
            parent[idx(nx, ny)] = idx(x, y) as i64;
            q.push_back((nx, ny));
        }
    }

    let mut reserved = vec![false; (w * h) as usize];
    reserved[idx(root.0, root.1)] = true;
    let mut out = vec![root];
    for (_, e) in entries.iter().skip(1) {
        if !grid.in_bounds(e.0, e.1) || parent[idx(e.0, e.1)] == -1 {
            // Unreachable at plan time; `pour` already failed this pad.
            continue;
        }
        let mut cur = idx(e.0, e.1);
        while !reserved[cur] {
            reserved[cur] = true;
            out.push(((cur as i64) % w, (cur as i64) / w));
            match parent[cur] {
                -2 => break,
                p => cur = p as usize,
            }
        }
    }
    out
}
