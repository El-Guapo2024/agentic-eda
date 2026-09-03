//! T3 gates for the physical stages: placement and routing.
//!
//! These are exact-geometry judges, deliberately independent of how the
//! placer/router reached their answer (no grid, no occupancy map): copper
//! edge-to-edge distances against `BoardRules::clearance`, union-find
//! connectivity over real pad rectangles, courtyard overlap and outline
//! containment. The generators are free to change; this is what has to
//! hold.

use eda_model::footprint::{placed_courtyard, placed_pads};
use eda_model::ir::{Design, Point, Side, Track, Um, Via};
use eda_model::{CheckResult, CheckStatus, ConstraintModel, PlacementRule};
use std::collections::{BTreeMap, HashMap};

// ------------------------------------------------------------- geometry

fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i].x, poly[i].y);
        let (xj, yj) = (poly[j].x, poly[j].y);
        if (yi > p.y) != (yj > p.y) {
            let x_int = xi as f64 + ((p.y - yi) as f64) * ((xj - xi) as f64) / ((yj - yi) as f64);
            if (p.x as f64) < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn seg_point_dist(a: Point, b: Point, p: Point) -> f64 {
    let (ax, ay, bx, by, px, py) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, p.x as f64, p.y as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn orient(a: Point, b: Point, c: Point) -> i128 {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let o1 = orient(a, b, c).signum();
    let o2 = orient(a, b, d).signum();
    let o3 = orient(c, d, a).signum();
    let o4 = orient(c, d, b).signum();
    if o1 != o2 && o3 != o4 {
        return true;
    }
    let on = |p: Point, q: Point, r: Point| {
        orient(p, q, r) == 0 && r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
    };
    on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b)
}

fn seg_seg_dist(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    seg_point_dist(a, b, c).min(seg_point_dist(a, b, d)).min(seg_point_dist(c, d, a)).min(seg_point_dist(c, d, b))
}

/// Axis-aligned rectangle (min_x, min_y, max_x, max_y).
#[derive(Debug, Clone, Copy)]
struct Rect(Um, Um, Um, Um);

impl Rect {
    fn centered(c: Point, size: (Um, Um)) -> Self {
        Rect(c.x - size.0 / 2, c.y - size.1 / 2, c.x + size.0 / 2, c.y + size.1 / 2)
    }
    fn corners(&self) -> [Point; 4] {
        [Point { x: self.0, y: self.1 }, Point { x: self.2, y: self.1 }, Point { x: self.2, y: self.3 }, Point { x: self.0, y: self.3 }]
    }
    fn contains(&self, p: Point) -> bool {
        p.x >= self.0 && p.x <= self.2 && p.y >= self.1 && p.y <= self.3
    }
    fn gap(&self, o: &Rect) -> f64 {
        let dx = (self.0 - o.2).max(o.0 - self.2).max(0) as f64;
        let dy = (self.1 - o.3).max(o.1 - self.3).max(0) as f64;
        (dx * dx + dy * dy).sqrt()
    }
    fn overlap_area(&self, o: &Rect) -> i128 {
        let w = (self.2.min(o.2) - self.0.max(o.0)).max(0) as i128;
        let h = (self.3.min(o.3) - self.1.max(o.1)).max(0) as i128;
        w * h
    }
}

fn seg_rect_dist(a: Point, b: Point, r: &Rect) -> f64 {
    if r.contains(a) || r.contains(b) {
        return 0.0;
    }
    let c = r.corners();
    let mut best = f64::MAX;
    for i in 0..4 {
        best = best.min(seg_seg_dist(a, b, c[i], c[(i + 1) % 4]));
    }
    best
}

// ------------------------------------------------------------- placement

/// Placement gate: every model part placed exactly once, every placed part
/// has real footprint geometry, courtyards inside the outline and not
/// overlapping (per side), proximity rules honoured.
pub fn check_placement(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let Some(pl) = design.placement.as_ref() else {
        out.push(CheckResult::fail("placement_present", "design", "design has no placement section"));
        return out;
    };
    if pl.outline.len() < 3 {
        out.push(CheckResult::fail("placement_outline", "design.placement.outline", "outline needs at least 3 points"));
        return out;
    }

    // Coverage.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for fp in &pl.footprints {
        *seen.entry(fp.id.as_str()).or_default() += 1;
    }
    let mut coverage_ok = true;
    for part in &model.parts {
        match seen.get(part.reference.as_str()) {
            None => {
                coverage_ok = false;
                out.push(CheckResult::fail("placement_coverage", &part.reference, "part is not placed"));
            }
            Some(n) if *n > 1 => {
                coverage_ok = false;
                out.push(CheckResult::fail("placement_coverage", &part.reference, format!("part placed {n} times")));
            }
            _ => {}
        }
    }
    for fp in &pl.footprints {
        if model.part(&fp.id).is_none() {
            coverage_ok = false;
            out.push(CheckResult::fail("placement_coverage", &fp.id, "placed footprint has no part in the model"));
        }
    }
    if coverage_ok {
        out.push(CheckResult::pass("placement_coverage"));
    }

    // Courtyards.
    let mut courtyards: BTreeMap<String, (Rect, Side)> = BTreeMap::new();
    let mut fp_ok = true;
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        match placed_courtyard(model, part, fp) {
            Some((x0, y0, x1, y1)) => {
                courtyards.insert(fp.id.clone(), (Rect(x0, y0, x1, y1), fp.side));
            }
            None => {
                fp_ok = false;
                out.push(CheckResult::fail(
                    "placement_footprint",
                    &fp.id,
                    format!("no footprint geometry (footprint={:?}, package={:?})", part.footprint, part.package),
                ));
            }
        }
    }
    if fp_ok {
        out.push(CheckResult::pass("placement_footprint"));
    }

    let mut inside_ok = true;
    for (id, (r, _)) in &courtyards {
        if !r.corners().iter().all(|c| point_in_polygon(*c, &pl.outline)) {
            inside_ok = false;
            out.push(CheckResult::fail("placement_within_outline", id, "courtyard extends outside the board outline"));
        }
    }
    if inside_ok {
        out.push(CheckResult::pass("placement_within_outline"));
    }

    let mut overlap_ok = true;
    let ids: Vec<&String> = courtyards.keys().collect();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (ra, sa) = courtyards[ids[i]];
            let (rb, sb) = courtyards[ids[j]];
            if sa == sb && ra.overlap_area(&rb) > 0 {
                overlap_ok = false;
                out.push(CheckResult::fail(
                    "placement_courtyard_overlap",
                    format!("{}/{}", ids[i], ids[j]),
                    format!("courtyards overlap by {} µm²", ra.overlap_area(&rb)),
                ));
            }
        }
    }
    if overlap_ok {
        out.push(CheckResult::pass("placement_courtyard_overlap"));
    }

    // Proximity is courtyard edge-to-edge: "C1 within 5 mm of U1" means
    // the gap between their bodies, not between their centres (which a
    // large package could never satisfy).
    let mut rules_ok = true;
    for rule in &model.placement_rules {
        if let PlacementRule::Proximity { a, b, max_mm } = rule {
            if let (Some((ra, _)), Some((rb, _))) = (courtyards.get(a), courtyards.get(b)) {
                let d = ra.gap(rb) / 1000.0;
                if d > *max_mm {
                    rules_ok = false;
                    out.push(CheckResult::fail(
                        "placement_proximity",
                        format!("{a}/{b}"),
                        format!("{d:.2} mm apart, rule allows {max_mm} mm"),
                    ));
                }
            }
        }
    }
    if rules_ok {
        out.push(CheckResult::pass("placement_proximity"));
    }
    out
}

// --------------------------------------------------------------- routing

#[derive(Debug, Clone)]
struct PadItem {
    refpin: String,
    net: String,
    rect: Rect,
    layers: Vec<String>,
}

struct Uf(Vec<usize>);
impl Uf {
    fn new(n: usize) -> Self {
        Uf((0..n).collect())
    }
    fn find(&mut self, x: usize) -> usize {
        if self.0[x] != x {
            let r = self.find(self.0[x]);
            self.0[x] = r;
        }
        self.0[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

/// Routing gate: connectivity per net, copper clearance between different
/// nets (tracks, vias, pads — exact edge-to-edge), outline containment,
/// minimum track width, plus an informational off-grid warning.
pub fn check_routing(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let rules = &model.board;
    let mut out = Vec::new();
    let (Some(pl), Some(rt)) = (design.placement.as_ref(), design.routing.as_ref()) else {
        out.push(CheckResult::fail("routing_present", "design", "design needs both placement and routing sections"));
        return out;
    };
    let outer_top = rules.layers.first().cloned().unwrap_or_else(|| "F.Cu".into());
    let outer_bot = rules.layers.last().cloned().unwrap_or_else(|| "B.Cu".into());

    // Pads.
    let mut pads: Vec<PadItem> = Vec::new();
    let mut fp_ok = true;
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(geoms) = placed_pads(model, part, fp) else {
            fp_ok = false;
            out.push(CheckResult::fail("routing_footprint", &fp.id, "no footprint geometry"));
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
            let layers = if g.through_hole {
                rules.layers.clone()
            } else {
                vec![if fp.side == Side::Top { outer_top.clone() } else { outer_bot.clone() }]
            };
            pads.push(PadItem { refpin, net, rect: Rect::centered(g.center, g.size), layers });
        }
    }
    if fp_ok {
        out.push(CheckResult::pass("routing_footprint"));
    }

    // Track width & outline & grid.
    let mut width_ok = true;
    let mut outline_ok = true;
    let mut offgrid = 0usize;
    for (i, t) in rt.tracks.iter().enumerate() {
        if t.width < rules.track_width {
            width_ok = false;
            out.push(CheckResult::fail("routing_track_width", format!("{}#{i}", t.net), format!("width {} < min {}", t.width, rules.track_width)));
        }
        for p in &t.pts {
            if !point_in_polygon(*p, &pl.outline) && !on_boundary(*p, &pl.outline) {
                outline_ok = false;
                out.push(CheckResult::fail("routing_within_outline", format!("{}#{i}", t.net), format!("track point ({},{}) outside outline", p.x, p.y)));
            }
            if p.x.rem_euclid(rules.grid) != 0 || p.y.rem_euclid(rules.grid) != 0 {
                offgrid += 1;
            }
        }
    }
    for v in &rt.vias {
        if !point_in_polygon(v.at, &pl.outline) {
            outline_ok = false;
            out.push(CheckResult::fail("routing_within_outline", &v.net, format!("via ({},{}) outside outline", v.at.x, v.at.y)));
        }
    }
    if width_ok {
        out.push(CheckResult::pass("routing_track_width"));
    }
    if outline_ok {
        out.push(CheckResult::pass("routing_within_outline"));
    }
    out.push(CheckResult {
        check: "routing_offgrid_points".into(),
        status: if offgrid == 0 { CheckStatus::Pass } else { CheckStatus::Warn },
        location: None,
        hint: Some(format!("{offgrid} track vertices off the {}µm grid", rules.grid)),
    });

    // Connectivity: nodes = pads, track vertices, vias.
    check_connectivity(rt, &pads, model, rules.track_width, &mut out);

    // Clearance.
    check_clearance(rt, &pads, rules.clearance, &mut out);

    out
}

fn on_boundary(p: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    (0..n).any(|i| {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        orient(a, b, p) == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    })
}

fn check_connectivity(rt: &eda_model::ir::RoutingSection, pads: &[PadItem], model: &ConstraintModel, track_w: Um, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for net in &model.nets {
        let net_pads: Vec<&PadItem> = pads.iter().filter(|p| p.net == net.name).collect();
        if net_pads.len() < 2 {
            continue;
        }
        let tracks: Vec<&Track> = rt.tracks.iter().filter(|t| t.net == net.name).collect();
        let vias: Vec<&Via> = rt.vias.iter().filter(|v| v.net == net.name).collect();

        // Node ids: pads 0..P, then one per (track, vertex), then vias.
        let mut n = net_pads.len();
        let mut tv_base = Vec::with_capacity(tracks.len());
        for t in &tracks {
            tv_base.push(n);
            n += t.pts.len();
        }
        let via_base = n;
        n += vias.len();
        let mut uf = Uf::new(n);

        // Track vertices chain.
        for (ti, t) in tracks.iter().enumerate() {
            for k in 1..t.pts.len() {
                uf.union(tv_base[ti] + k - 1, tv_base[ti] + k);
            }
        }
        // Vertex-vertex on the same layer at the same point; vertex-via at
        // the via point (any layer the via spans).
        let mut by_point: HashMap<(Um, Um), Vec<(usize, String)>> = HashMap::new();
        for (ti, t) in tracks.iter().enumerate() {
            for (k, p) in t.pts.iter().enumerate() {
                by_point.entry((p.x, p.y)).or_default().push((tv_base[ti] + k, t.layer.clone()));
            }
        }
        for (vi, v) in vias.iter().enumerate() {
            if let Some(list) = by_point.get(&(v.at.x, v.at.y)) {
                for (node, _) in list {
                    uf.union(via_base + vi, *node);
                }
            }
        }
        for list in by_point.values() {
            for i in 0..list.len() {
                for j in i + 1..list.len() {
                    if list[i].1 == list[j].1 {
                        uf.union(list[i].0, list[j].0);
                    }
                }
            }
        }
        // Pad-track: any segment on a pad layer whose copper touches the pad.
        for (pi, pad) in net_pads.iter().enumerate() {
            for (ti, t) in tracks.iter().enumerate() {
                if !pad.layers.contains(&t.layer) {
                    continue;
                }
                let half = (t.width.max(track_w) / 2) as f64;
                for k in 0..t.pts.len() {
                    let a = t.pts[k];
                    let b = if k + 1 < t.pts.len() { t.pts[k + 1] } else { a };
                    if seg_rect_dist(a, b, &pad.rect) <= half {
                        uf.union(pi, tv_base[ti] + k);
                        break;
                    }
                }
            }
            for (vi, v) in vias.iter().enumerate() {
                if pad.rect.gap(&Rect::centered(v.at, (v.diameter, v.diameter))) <= 0.0 {
                    uf.union(pi, via_base + vi);
                }
            }
        }
        // Segment-segment touching on same layer (T-junctions).
        for ti in 0..tracks.len() {
            for tj in ti + 1..tracks.len() {
                if tracks[ti].layer != tracks[tj].layer {
                    continue;
                }
                let half = ((tracks[ti].width + tracks[tj].width) / 2) as f64;
                'outer: for k in 0..tracks[ti].pts.len().saturating_sub(1) {
                    for m in 0..tracks[tj].pts.len().saturating_sub(1) {
                        if seg_seg_dist(tracks[ti].pts[k], tracks[ti].pts[k + 1], tracks[tj].pts[m], tracks[tj].pts[m + 1]) <= half {
                            uf.union(tv_base[ti] + k, tv_base[tj] + m);
                            break 'outer;
                        }
                    }
                }
            }
        }

        let root = uf.find(0);
        let disconnected: Vec<&str> = net_pads.iter().enumerate().filter(|(i, _)| uf.find(*i) != root).map(|(_, p)| p.refpin.as_str()).collect();
        if !disconnected.is_empty() {
            ok = false;
            out.push(CheckResult::fail(
                "routing_connectivity",
                &net.name,
                format!("pads not connected to {}: {}", net_pads[0].refpin, disconnected.join(", ")),
            ));
        }
    }
    if ok {
        out.push(CheckResult::pass("routing_connectivity"));
    }
}

fn check_clearance(rt: &eda_model::ir::RoutingSection, pads: &[PadItem], clearance: Um, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    let cl = clearance as f64;
    let mut fail = |a: String, b: String, gap: f64| {
        ok = false;
        out.push(CheckResult::fail("routing_clearance", format!("{a}/{b}"), format!("copper gap {gap:.0} µm < {clearance} µm")));
    };

    // Segments with their half-width.
    struct Seg<'a> {
        net: &'a str,
        layer: &'a str,
        a: Point,
        b: Point,
        half: f64,
        id: String,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for (i, t) in rt.tracks.iter().enumerate() {
        for k in 0..t.pts.len().saturating_sub(1) {
            segs.push(Seg { net: &t.net, layer: &t.layer, a: t.pts[k], b: t.pts[k + 1], half: (t.width / 2) as f64, id: format!("{}#{i}", t.net) });
        }
    }

    // seg-seg
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let (s, o) = (&segs[i], &segs[j]);
            if s.net == o.net || s.layer != o.layer {
                continue;
            }
            let gap = seg_seg_dist(s.a, s.b, o.a, o.b) - s.half - o.half;
            if gap < cl {
                fail(s.id.clone(), o.id.clone(), gap);
            }
        }
    }
    // seg-pad
    for s in &segs {
        for p in pads {
            if s.net == p.net || !p.layers.iter().any(|l| l == s.layer) {
                continue;
            }
            let gap = seg_rect_dist(s.a, s.b, &p.rect) - s.half;
            if gap < cl {
                fail(s.id.clone(), p.refpin.clone(), gap);
            }
        }
    }
    // via-seg, via-pad, via-via (vias span all layers in v1)
    for (vi, v) in rt.vias.iter().enumerate() {
        let vr = (v.diameter / 2) as f64;
        let vid = format!("via:{}#{vi}", v.net);
        for s in &segs {
            if s.net == v.net {
                continue;
            }
            let gap = seg_point_dist(s.a, s.b, v.at) - s.half - vr;
            if gap < cl {
                fail(vid.clone(), s.id.clone(), gap);
            }
        }
        for p in pads {
            if p.net == v.net {
                continue;
            }
            let gap = p.rect.gap(&Rect::centered(v.at, (0, 0))) - vr;
            if gap < cl {
                fail(vid.clone(), p.refpin.clone(), gap);
            }
        }
        for (wi, w) in rt.vias.iter().enumerate().skip(vi + 1) {
            if w.net == v.net {
                continue;
            }
            let d = (((v.at.x - w.at.x) as f64).powi(2) + ((v.at.y - w.at.y) as f64).powi(2)).sqrt();
            let gap = d - vr - (w.diameter / 2) as f64;
            if gap < cl {
                fail(vid.clone(), format!("via:{}#{wi}", w.net), gap);
            }
        }
    }
    // pad-pad (different nets)
    for i in 0..pads.len() {
        for j in i + 1..pads.len() {
            let (p, q) = (&pads[i], &pads[j]);
            if p.net == q.net || !p.layers.iter().any(|l| q.layers.contains(l)) {
                continue;
            }
            let gap = p.rect.gap(&q.rect);
            if gap < cl {
                fail(p.refpin.clone(), q.refpin.clone(), gap);
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("routing_clearance"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_seg_distance_basics() {
        let p = |x, y| Point { x, y };
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(5, -5), p(5, 5)), 0.0);
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(0, 3), p(10, 3)), 3.0);
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(13, 4), p(20, 4)), 5.0);
    }

    #[test]
    fn rect_gap_and_seg_rect() {
        let p = |x, y| Point { x, y };
        let r = Rect(0, 0, 10, 10);
        assert_eq!(r.gap(&Rect(13, 0, 20, 10)), 3.0);
        assert_eq!(seg_rect_dist(p(-5, 5), p(-2, 5), &r), 2.0);
        assert_eq!(seg_rect_dist(p(-5, 5), p(5, 5), &r), 0.0);
    }
}
