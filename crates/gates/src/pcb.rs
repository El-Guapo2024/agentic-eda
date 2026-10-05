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
use eda_model::{CheckResult, ConstraintModel, PlacementRule};
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
pub struct Rect(pub Um, pub Um, pub Um, pub Um);

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

// ------------------------------------------------------------ lint adapters
//
// `eda-lint`'s findings come back as the `CheckResult` shape this module has
// always returned, so nothing downstream (the repair loop, the build
// placer's per-step gate loop, existing tests) has to change. The gates
// KiCad owns (courtyard overlap, copper-to-edge clearance) are in
// `crate::kicad`, answered by kicad-cli.

/// One [`eda_lint::Finding`] as the `CheckResult` this module has always
/// produced for `check_name`. `location` joins every referenced item's id
/// (this module's own "A/B" pair convention); a lint finding is always a
/// hard `Fail`, like every gate here. `fix`, when present, becomes
/// `detail.suggest` -- the field `eda::print_checks` reads back out -- plus
/// the mover/toward/distance a repair pass acts on.
fn from_lint(f: &eda_lint::Finding, check_name: &str) -> CheckResult {
    let location = f.items.iter().map(|it| it.id.as_str()).collect::<Vec<_>>().join("/");
    let mut cr = CheckResult { check: check_name.to_string(), status: eda_model::CheckStatus::Fail, location: Some(location), hint: Some(f.description.clone()), detail: None };
    if let Some(fix) = &f.fix {
        cr = cr.with_detail(serde_json::json!({
            "mover": fix.mover, "toward": fix.toward, "distance_to_close_um": fix.distance_to_close_um,
            "suggest": fix.suggested_command,
        }));
    }
    cr
}

/// The lint findings named `key`, translated to `check_name`, with a single
/// `CheckResult::pass(check_name)` appended when there are none.
fn lint_check(findings: &[eda_lint::Finding], key: &str, check_name: &str, out: &mut Vec<CheckResult>) {
    let mut any = false;
    for f in findings.iter().filter(|f| f.check == key) {
        any = true;
        out.push(from_lint(f, check_name));
    }
    if !any {
        out.push(CheckResult::pass(check_name));
    }
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

    // Refdes labels over neighbouring courtyards and the proximity rules are
    // `eda-lint`'s. Courtyard overlap and pad-to-edge clearance are KiCad's
    // (`crate::kicad`): they need a kicad-cli run, which does not belong in
    // a gate the placer runs on every step.
    let lint = eda_lint::placement::check(design, model);
    lint_check(&lint, "placement_refdes_clear", "placement_refdes_clear", &mut out);
    lint_check(&lint, "placement_proximity", "placement_proximity", &mut out);

    // Separation: the repulsive mirror. Thermal, noise coupling and
    // high-voltage clearance all want parts *apart*, and the failure
    // carries the rule's stated reason because a separation rule is
    // usually a judgement call rather than a datasheet number.
    let mut sep_ok = true;
    for rule in &model.placement_rules {
        if let PlacementRule::Separation { a, b, min_mm, reason } = rule {
            if let (Some((ra, _)), Some((rb, _))) = (courtyards.get(a), courtyards.get(b)) {
                let d = ra.gap(rb) / 1000.0;
                if d < *min_mm {
                    sep_ok = false;
                    let under = *min_mm - d;
                    out.push(
                        CheckResult::fail("placement_separation", format!("{a}/{b}"), format!("{d:.2} mm apart, rule wants at least {min_mm} mm"))
                            .with_detail(serde_json::json!({
                                "pair": [a, b], "gap_mm": (d * 100.0).round() / 100.0, "min_mm": min_mm,
                                "under_mm": (under * 100.0).round() / 100.0, "reason": reason,
                                "suggest": "the placer pulls these together to shorten wire; give one of them somewhere else to be, or relax min_mm if the separation was a guess",
                            })),
                    );
                }
            }
        }
    }
    if sep_ok {
        out.push(CheckResult::pass("placement_separation"));
    }
    out.extend(placement_region(design, model));
    // Locality: connectors on the edge, board used, decoupling close, nets
    // compact, no crossed stubs. One gate, so no caller can forget them.
    out.extend(locality_with(design, model, &lint));
    out
}

/// A placement built under a floorplan must respect it. The plan gave each
/// module a rectangle; a part that has drifted out of its block means the
/// placement and the floorplan disagree, and shipping either one alone is
/// not an answer. A part the floorplan left free is not judged here --
/// the plan never claimed to know where it goes.
///
/// A design with no recorded modules is not floorplanned and this gate has
/// nothing to say about it.
pub fn placement_region(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let Some(pl) = design.placement.as_ref() else { return Vec::new() };
    if pl.modules.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut ok = true;
    for m in &pl.modules {
        for id in &m.refs {
            let Some(fp) = pl.footprints.iter().find(|f| &f.id == id) else {
                out.push(CheckResult::fail("placement_region", format!("{}/{id}", m.name), "the floorplan assigns this part to a module but the placement does not contain it"));
                ok = false;
                continue;
            };
            let Some(part) = model.part(id) else { continue };
            let Some(c) = placed_courtyard(model, part, fp) else { continue };
            let outx = ((m.rect.0 - c.0).max(0) + (c.2 - m.rect.2).max(0)) as f64 / 1000.0;
            let outy = ((m.rect.1 - c.1).max(0) + (c.3 - m.rect.3).max(0)) as f64 / 1000.0;
            if outx > 0.0 || outy > 0.0 {
                ok = false;
                out.push(
                    CheckResult::fail(
                        "placement_region",
                        format!("{}/{id}", m.name),
                        format!("courtyard escapes its module region by {outx:.2} mm in x, {outy:.2} mm in y"),
                    )
                    .with_detail(serde_json::json!({
                        "module": m.name, "part": id, "rect_um": [m.rect.0, m.rect.1, m.rect.2, m.rect.3],
                        "courtyard_um": [c.0, c.1, c.2, c.3],
                        "out_x_mm": (outx * 100.0).round() / 100.0, "out_y_mm": (outy * 100.0).round() / 100.0,
                        "suggest": "far miss: the block is too small for its parts or too tightly boxed by its neighbours. Re-run the floorplan under another seed, or raise floorplan fill headroom",
                    })),
                );
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("placement_region"));
    }
    out
}

/// Placement quality gate: catches layouts that pass the hard geometric
/// checks but are locally bad in ways that should still fail the run.
///
/// * `placement_isolation` — a part whose *only* connection to the rest of
///   the board is a single point-to-point (2-pin) net must sit reasonably
///   close to that one neighbour; parked far across the board is a
///   regression even though it is legal.
/// * `placement_edge_connector` — a part flagged in `model` as an edge
///   connector (its reference or value contains "connector"/"conn" and it
///   has a through-hole pad, i.e. headers/jacks meant for cable access)
///   must have at least one courtyard edge on the board outline's bounding
///   box, not stranded in the interior.
pub fn check_placement_locality(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    locality_with(design, model, &eda_lint::placement::check(design, model))
}

/// [`check_placement_locality`] over lint findings the caller already has.
fn locality_with(design: &Design, model: &ConstraintModel, lint: &[eda_lint::Finding]) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let Some(pl) = design.placement.as_ref() else {
        out.push(CheckResult::fail("placement_isolation", "design", "design has no placement section"));
        return out;
    };
    if pl.outline.len() < 3 {
        // Not a silent skip. Every distance in this gate is scaled by the
        // board diagonal, and an absent outline made that diagonal zero --
        // so the gate returned no checks at all: no pass, no fail, just a
        // hole where a reward signal should be.
        out.push(CheckResult::fail(
            "placement_outline",
            "design.placement.outline",
            "outline needs at least 3 points; every locality check here is measured against the board diagonal",
        ));
        return out;
    }
    let mut courtyards: BTreeMap<String, Rect> = BTreeMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        if let Some((x0, y0, x1, y1)) = placed_courtyard(model, part, fp) {
            courtyards.insert(fp.id.clone(), Rect(x0, y0, x1, y1));
        }
    }
    let bb = (
        pl.outline.iter().map(|p| p.x).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.x).max().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).max().unwrap_or(0),
    );
    let board_diag = (((bb.2 - bb.0) as f64).powi(2) + ((bb.3 - bb.1) as f64).powi(2)).sqrt();
    // Allow a generous multiple of the two courtyards' own extents before
    // calling a point-to-point net "far apart" — small parts on a huge
    // board can legitimately sit a good fraction of the diagonal apart, but
    // never further than the whole diagonal.
    let threshold = (board_diag * 0.75).max(20_000.0);

    let mut isolation_ok = true;
    for net in &model.nets {
        if net.pins.len() != 2 {
            continue;
        }
        let refs: Vec<&str> = net.pins.iter().filter_map(|p| p.split_once('.').map(|(r, _)| r)).collect();
        if refs.len() != 2 || refs[0] == refs[1] {
            continue;
        }
        let (Some(ra), Some(rb)) = (courtyards.get(refs[0]), courtyards.get(refs[1])) else { continue };
        let d = ra.gap(rb);
        if d > threshold {
            isolation_ok = false;
            out.push(CheckResult::fail(
                "placement_isolation",
                format!("{}/{}", refs[0], refs[1]),
                format!("only connection is net {:?}, but courtyards are {:.0} µm apart (board diagonal {:.0} µm)", net.name, d, board_diag),
            ));
        }
    }
    if isolation_ok {
        out.push(CheckResult::pass("placement_isolation"));
    }

    // Edge-connector placement, board use, decoupling distance, net
    // compactness and crossing stubs are `eda-lint`'s -- none of them looks
    // at copper, so they cost nothing on a routed design.
    let _ = (&courtyards, bb); // still built above for `placement_isolation`
    lint_check(lint, "placement_edge_connector", "placement_edge_connector", &mut out);
    lint_check(lint, "placement_board_use", "placement_board_use", &mut out);
    lint_check(lint, "placement_decoupling", "placement_decoupling", &mut out);
    lint_check(lint, "placement_net_compactness", "placement_net_compactness", &mut out);
    lint_check(lint, "placement_stub_crossings", "placement_stub_crossings", &mut out);
    out
}

pub use eda_lint::placement::{
    BOARD_USE_MAX_IMBALANCE, BOARD_USE_MIN_DENSITY_FRACTION, BOARD_USE_MIN_SPAN, DECOUPLING_MAX_GAP_UM, EDGE_CONNECTOR_MAX_GAP_UM,
    NET_COMPACTNESS_FLOOR_UM, NET_COMPACTNESS_MAX_MEMBERS, NET_COMPACTNESS_MAX_RATIO, STUB_CROSSING_MAX_RATIO,
};

/// See [`eda_model::is_free_two_pin`].
pub fn is_free_two_pin(part: &eda_model::Part) -> bool {
    eda_model::is_free_two_pin(part)
}

/// See [`eda_model::footprint::is_edge_connector`].
pub fn is_edge_connector(part: &eda_model::Part) -> bool {
    eda_model::footprint::is_edge_connector(part)
}

/// See [`eda_model::footprint::edge_connector_gap`].
pub fn edge_connector_gap(r: Rect, bb: (Um, Um, Um, Um)) -> Um {
    eda_model::footprint::edge_connector_gap((r.0, r.1, r.2, r.3), bb)
}

/// See [`eda_model::decoupling_pairs`].
pub fn decoupling_pairs(model: &ConstraintModel) -> Vec<(String, String)> {
    eda_model::decoupling_pairs(model)
}

// --------------------------------------------------------------- routing

#[derive(Debug, Clone)]
struct PadItem {
    refpin: String,
    net: String,
    rect: Rect,
    /// Shape-aware geometry for connectivity (KiCad semantics: the track
    /// end must lie in the pad copper); clearance keeps the conservative
    /// bounding rect.
    geom: eda_model::footprint::PlacedPad,
    layers: Vec<String>,
}

/// Distance from a segment's centreline to the pad's copper (0 = touches).
fn seg_pad_dist(a: Point, b: Point, pad: &PadItem) -> f64 {
    let g = &pad.geom;
    let center = g.center;
    if g.is_round() {
        let rmin = (g.size.0.min(g.size.1) / 2) as f64;
        (seg_point_dist(a, b, center) - rmin).max(0.0)
    } else {
        let r = g.corner_radius();
        let inset = Rect(
            (center.x as f64 - g.size.0 as f64 / 2.0 + r).round() as Um,
            (center.y as f64 - g.size.1 as f64 / 2.0 + r).round() as Um,
            (center.x as f64 + g.size.0 as f64 / 2.0 - r).round() as Um,
            (center.y as f64 + g.size.1 as f64 / 2.0 - r).round() as Um,
        );
        (seg_rect_dist(a, b, &inset) - r).max(0.0)
    }
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
    // A board with no stackup is not a two-layer board, and guessing one
    // here put every SMD pad on an invented layer -- after which the
    // clearance gate compares copper that shares a name with nothing.
    if rules.layers.is_empty() {
        out.push(CheckResult::fail(
            "routing_stackup",
            "board.layers",
            "board declares no copper layers; there is no such thing as a default stackup",
        ));
        return out;
    }
    let outer_top = rules.layers[0].clone();
    let outer_bot = rules.layers[rules.layers.len() - 1].clone();

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
            pads.push(PadItem { refpin, net, rect: Rect::centered(g.center, g.size), geom: g.clone(), layers });
        }
    }
    if fp_ok {
        out.push(CheckResult::pass("routing_footprint"));
    }

    // Track width against the net class is `eda-lint`'s (below); outline
    // containment has no KiCad equivalent and stays exactly as it was.
    let mut outline_ok = true;
    for (i, t) in rt.tracks.iter().enumerate() {
        for p in &t.pts {
            if !point_in_polygon(*p, &pl.outline) && !on_boundary(*p, &pl.outline) {
                outline_ok = false;
                out.push(CheckResult::fail("routing_within_outline", format!("{}#{i}", t.net), format!("track point ({},{}) outside outline", p.x, p.y)));
            }
        }
    }
    for v in &rt.vias {
        if !point_in_polygon(v.at, &pl.outline) {
            outline_ok = false;
            out.push(CheckResult::fail("routing_within_outline", &v.net, format!("via ({},{}) outside outline", v.at.x, v.at.y)));
        }
    }
    if outline_ok {
        out.push(CheckResult::pass("routing_within_outline"));
    }

    let lint = eda_lint::routing::check(design, model);
    lint_check(&lint, "routing_track_width", "routing_track_width", &mut out);

    // Connectivity: nodes = pads, track vertices, vias.
    check_connectivity(rt, &pads, model, rules.track_width, &mut out);

    // Clearance: this module's own flat copper-to-copper check against
    // `rules.clearance` -- the same model `crates/freeroute` routes by, so
    // the router and this gate can only disagree by a bug, which is what
    // `eda::route_checked` exists to catch. KiCad's own `clearance` and
    // `hole_clearance` (the latter stricter: a hole needs more room than a
    // plain copper gap) are kicad-cli's, in the DRC dialog.
    check_clearance(rt, &pads, rules.clearance, &mut out);

    // Workmanship: things a human reviewer sends back even when DRC is
    // clean (pass-through pads, via-in-pad, threading between SMD pads,
    // copper under a refdes label).
    check_workmanship(design, model, rt, &pads, &outer_top, &outer_bot, &mut out);

    out
}

/// Label box of a refdes as the routing judge renders it (centred above
/// the courtyard, sans-serif at `fs`): the box a track must stay out of
/// for the label to remain readable. Mirrors `eda_judge::render_board_svg`.
pub fn refdes_box(model: &ConstraintModel, pl: &eda_model::ir::PlacementSection, fp: &eda_model::ir::FootprintInstance) -> Option<(Rect, Side)> {
    let part = model.part(&fp.id)?;
    let (x0, y0, x1, y1) = eda_model::footprint::placed_refdes_box(model, &pl.outline, part, fp)?;
    Some((Rect(x0, y0, x1, y1), fp.side))
}

/// Exact-geometry workmanship gates over routed copper. Every one has a
/// hard numeric threshold of zero: a single instance is what a reviewer
/// sends back.
///
/// - `routing_pass_through_pad`: a track segment whose centreline crosses
///   a pad of its own net without the polyline starting or ending inside
///   that pad (the pad is a stepping stone, not a terminal). Other-net
///   pads are already caught by `routing_clearance`.
/// - `routing_via_in_pad`: a via whose copper overlaps any pad on a layer
///   the pad exists on. Via-in-pad needs filled/capped vias; a plain
///   two-layer board wicks solder down the barrel.
/// - `routing_between_smd_pads`: a track that properly crosses the line
///   joining two SMD pads of one footprint whose copper gap is under
///   `BETWEEN_PADS_MAX_GAP` — i.e. runs under the component body between
///   its pads. Through-hole pin rows are exempt: routing between header
///   pins is standard practice.
/// - `routing_over_refdes`: a track on a part's own side crossing the
///   refdes label box the judge renders above the courtyard.
fn check_workmanship(
    design: &Design,
    model: &ConstraintModel,
    rt: &eda_model::ir::RoutingSection,
    pads: &[PadItem],
    outer_top: &str,
    outer_bot: &str,
    out: &mut Vec<CheckResult>,
) {
    const BETWEEN_PADS_MAX_GAP: Um = 2000;
    if design.placement.is_none() {
        return;
    }
    // Pass-through pads.
    let mut n_pass = 0usize;
    for (i, t) in rt.tracks.iter().enumerate() {
        if t.pts.len() < 2 {
            continue;
        }
        let (first, last) = (t.pts[0], t.pts[t.pts.len() - 1]);
        for p in pads {
            if p.net != t.net || !p.layers.contains(&t.layer) {
                continue;
            }
            if p.geom.contains(first) || p.geom.contains(last) {
                continue;
            }
            let crosses = t.pts.windows(2).any(|w| seg_pad_dist(w[0], w[1], p) == 0.0);
            if crosses {
                if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
                    eprintln!("gate: pass-through {}#{i} pts {:?} through pad {} at {:?} size {:?}", t.net, t.pts, p.refpin, p.geom.center, p.geom.size);
                }
                n_pass += 1;
                out.push(CheckResult::fail("routing_pass_through_pad", format!("{}#{i}/{}", t.net, p.refpin), "track runs through a pad of its own net without terminating on it (max 0)"));
            }
        }
    }
    if n_pass == 0 {
        out.push(CheckResult::pass("routing_pass_through_pad"));
    }

    // Via in pad.
    let mut n_vip = 0usize;
    for (vi, v) in rt.vias.iter().enumerate() {
        let vr = (v.diameter / 2) as f64;
        for p in pads {
            let overlap = p.rect.gap(&Rect::centered(v.at, (0, 0))) - vr;
            if overlap < 0.0 {
                n_vip += 1;
                out.push(CheckResult::fail("routing_via_in_pad", format!("via:{}#{vi}/{}", v.net, p.refpin), format!("via copper overlaps pad by {:.0} µm (max 0)", -overlap)));
            }
        }
    }
    if n_vip == 0 {
        out.push(CheckResult::pass("routing_via_in_pad"));
    }

    // Between SMD pads of one footprint.
    let mut n_between = 0usize;
    let mut by_fp: BTreeMap<&str, Vec<&PadItem>> = BTreeMap::new();
    for p in pads {
        if p.geom.through_hole {
            continue;
        }
        let r = p.refpin.rsplit_once('.').map(|(r, _)| r).unwrap_or(&p.refpin);
        by_fp.entry(r).or_default().push(p);
    }
    for (_, fpads) in &by_fp {
        for i in 0..fpads.len() {
            for j in i + 1..fpads.len() {
                let (a, b) = (fpads[i], fpads[j]);
                if (a.rect.gap(&b.rect) as Um) > BETWEEN_PADS_MAX_GAP {
                    continue;
                }
                // Adjacent pairs only: pins 1 and 3 of a row have pin 2
                // between them, and that is a pad, not a gap.
                let via_other = fpads.iter().enumerate().any(|(k, o)| k != i && k != j && seg_rect_dist(a.geom.center, b.geom.center, &o.rect) == 0.0);
                if via_other {
                    continue;
                }
                let layer = &a.layers[0];
                for (ti, t) in rt.tracks.iter().enumerate() {
                    if &t.layer != layer {
                        continue;
                    }
                    // A track on either pad's own net is that pad's
                    // connection, and it may leave along the pad edge that
                    // faces its neighbour (L3 seed 1: I2C_SCL exiting U3.3
                    // westward grazed the U3.2-U3.3 gap line at the pad
                    // boundary). Threading is a *foreign* net's business.
                    if t.net == a.net || t.net == b.net {
                        continue;
                    }
                    let hit = t.pts.windows(2).any(|w| {
                        let o1 = orient(a.geom.center, b.geom.center, w[0]).signum();
                        let o2 = orient(a.geom.center, b.geom.center, w[1]).signum();
                        let o3 = orient(w[0], w[1], a.geom.center).signum();
                        let o4 = orient(w[0], w[1], b.geom.center).signum();
                        // Proper crossing of the centre-centre line, at a
                        // point outside both pads' copper.
                        if !(o1 != o2 && o1 != 0 && o2 != 0 && o3 != o4) {
                            return false;
                        }
                        let x = seg_seg_cross(a.geom.center, b.geom.center, w[0], w[1]);
                        !(a.geom.contains(x) || b.geom.contains(x))
                    });
                    if hit {
                        n_between += 1;
                        out.push(CheckResult::fail("routing_between_smd_pads", format!("{}#{ti}/{}-{}", t.net, a.refpin, b.refpin), "track threads the gap between two SMD pads of one footprint (max 0)"));
                    }
                }
            }
        }
    }
    if n_between == 0 {
        out.push(CheckResult::pass("routing_between_smd_pads"));
    }

    // Copper-to-board-edge clearance is KiCad's (`crate::kicad`:
    // `routing_edge_clearance`).

    // Refdes label boxes: this gate's own box (`refdes_box`), the label the
    // judge renders above the courtyard -- a workmanship check on what a
    // reviewer sees. KiCad's silk checks (`silk_over_copper`, `silk_overlap`)
    // are kicad-cli's, in the DRC dialog.
    let pl = design.placement.as_ref().expect("checked at function entry");
    let mut n_refdes = 0usize;
    for fp in &pl.footprints {
        let Some((bx, side)) = refdes_box(model, pl, fp) else { continue };
        let layer = if side == Side::Top { outer_top } else { outer_bot };
        for (ti, t) in rt.tracks.iter().enumerate() {
            if t.layer != layer {
                continue;
            }
            let half = (t.width / 2) as f64;
            if t.pts.windows(2).any(|w| seg_rect_dist(w[0], w[1], &bx) - half < 0.0) {
                n_refdes += 1;
                out.push(CheckResult::fail("routing_over_refdes", format!("{}#{ti}/{}", t.net, fp.id), "track runs under the refdes label (max 0)"));
            }
        }
        for (vi, v) in rt.vias.iter().enumerate() {
            if bx.gap(&Rect::centered(v.at, (0, 0))) - ((v.diameter / 2) as f64) < 0.0 {
                n_refdes += 1;
                out.push(CheckResult::fail("routing_over_refdes", format!("via:{}#{vi}/{}", v.net, fp.id), "via sits under the refdes label (max 0)"));
            }
        }
    }
    if n_refdes == 0 {
        out.push(CheckResult::pass("routing_over_refdes"));
    }
}

/// Intersection point of two properly crossing segments (rounded to µm).
fn seg_seg_cross(a: Point, b: Point, c: Point, d: Point) -> Point {
    let (ax, ay, bx, by, cx, cy, dx, dy) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64, d.x as f64, d.y as f64);
    let den = (bx - ax) * (dy - cy) - (by - ay) * (dx - cx);
    if den == 0.0 {
        return a;
    }
    let t = ((cx - ax) * (dy - cy) - (cy - ay) * (dx - cx)) / den;
    Point { x: (ax + t * (bx - ax)).round() as Um, y: (ay + t * (by - ay)).round() as Um }
}

/// Routing *quality* gate (loop-3): exact-geometry judge over track/via
/// output, independent of `check_routing`'s legality checks. Two axes:
///
/// - `routing_detour_ratio`: for a two-pin net, `total track length /
///   Manhattan airline distance between its pads`. A router with sane
///   costs never needs much more than the Manhattan airline on an open
///   board; a large ratio means the router looped, zig-zagged, or
///   took a long way around an obstacle it should have gone straight
///   past. Threshold 2.5x is generous headroom over the corpus's worst
///   observed ratio (~1.35x on `mcu_board_30plus`, a 30+ part board) while
///   still catching a genuinely bad detour.
/// - `routing_unnecessary_via`: a net's via count should never exceed its
///   pin count (a spanning tree has `pins - 1` edges, and even a single
///   edge legitimately needs one via *pair* — down to dodge an obstacle
///   on the pad's own layer, back up to land on it — so `pins` vias is
///   already generous headroom; observed on the corpus's most congested
///   board, `mcu_board_30plus`, where several two-pin nets route a
///   straight, zero-bend line except for exactly one via-pair around a
///   blocking track. More vias than that means the router is thrashing
///   layers rather than making one deliberate dodge.
pub fn check_routing_quality(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    const MAX_DETOUR_RATIO: f64 = 2.5;
    let mut out = Vec::new();
    let (Some(pl), Some(rt)) = (design.placement.as_ref(), design.routing.as_ref()) else {
        return out;
    };

    // Pad centre per "REF.PIN", for the airline distance.
    let mut pad_at: HashMap<String, Point> = HashMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(geoms) = placed_pads(model, part, fp) else { continue };
        for g in geoms {
            pad_at.insert(format!("{}.{}", fp.id, g.number), g.center);
        }
    }

    let mut detour_ok = true;
    let mut via_ok = true;
    let mut bend_ok = true;
    for net in &model.nets {
        let tracks: Vec<&Track> = rt.tracks.iter().filter(|t| t.net == net.name).collect();
        if tracks.is_empty() {
            continue;
        }
        let via_count = rt.vias.iter().filter(|v| v.net == net.name).count();
        let pin_count = net.pins.len();
        if pin_count >= 2 && via_count > pin_count {
            via_ok = false;
            out.push(CheckResult::fail(
                "routing_unnecessary_via",
                &net.name,
                format!("{via_count} via(s) on a {pin_count}-pin net; expected at most {pin_count} (one via-pair per spanning-tree edge)"),
            ));
        }

        // Bend count: interior vertices of every track polyline on the
        // net (the router already collinear-merges each polyline, so any
        // interior vertex left is a real direction change). Budget is
        // `2 + 2*pins` — generous enough for a spanning tree with a
        // couple of unavoidable doglegs per edge, but tight enough to
        // catch a router that's actually zig-zagging.
        let bend_count: usize = tracks.iter().map(|t| t.pts.len().saturating_sub(2)).sum();
        let bend_budget = 2 + 2 * pin_count;
        if bend_count > bend_budget {
            bend_ok = false;
            out.push(CheckResult::fail(
                "routing_bend_count",
                &net.name,
                format!("{bend_count} bend(s) on a {pin_count}-pin net; expected at most {bend_budget} (2 + 2*pins)"),
            ));
        }

        if pin_count != 2 {
            continue;
        }
        let (Some(&a), Some(&b)) = (pad_at.get(&net.pins[0]), pad_at.get(&net.pins[1])) else { continue };
        let airline = (a.x - b.x).abs() + (a.y - b.y).abs();
        if airline == 0 {
            continue;
        }
        let length: i64 = tracks.iter().map(|t| polyline_len(&t.pts)).sum();
        let ratio = length as f64 / airline as f64;
        if ratio > MAX_DETOUR_RATIO {
            detour_ok = false;
            out.push(CheckResult::fail(
                "routing_detour_ratio",
                &net.name,
                format!("track length {length}µm is {ratio:.2}x the {airline}µm airline distance (max {MAX_DETOUR_RATIO}x)"),
            ));
        }
    }
    if via_ok {
        out.push(CheckResult::pass("routing_unnecessary_via"));
    }
    if bend_ok {
        out.push(CheckResult::pass("routing_bend_count"));
    }
    if detour_ok {
        out.push(CheckResult::pass("routing_detour_ratio"));
    }
    out
}

fn polyline_len(pts: &[Point]) -> i64 {
    pts.windows(2).map(|w| (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs()).sum()
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

        // Node ids: pads 0..P, then one per (track, vertex), then vias,
        // then one per pour.
        let mut n = net_pads.len();
        let mut tv_base = Vec::with_capacity(tracks.len());
        for t in &tracks {
            tv_base.push(n);
            n += t.pts.len();
        }
        let via_base = n;
        n += vias.len();
        // ... then one node per copper pour on this net: the plane itself
        // is a conductor, and pads and vias join the net through it.
        let zone_count = rt.zones.iter().filter(|z| z.net == net.name).count();
        n += zone_count;
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
                    // KiCad connects a track to a pad when the track's
                    // end point lies inside the pad copper; a mere graze
                    // of the track's width does not count.
                    let touches = pad.geom.contains(a) || pad.geom.contains(b);
                    let _ = (half, seg_pad_dist(a, b, pad));
                    if touches {
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
        // Track-track joins (KiCad anchor rule): an END POINT of one
        // track lies within the other track's copper on the same layer.
        // Mere crossings do not connect.
        for ti in 0..tracks.len() {
            for tj in 0..tracks.len() {
                if ti == tj || tracks[ti].layer != tracks[tj].layer {
                    continue;
                }
                let half = (tracks[tj].width / 2) as f64;
                let ends = [(0usize, tracks[ti].pts.first()), (tracks[ti].pts.len().saturating_sub(1), tracks[ti].pts.last())];
                for (k, end) in ends {
                    let Some(&end) = end else { continue };
                    for m in 0..tracks[tj].pts.len().saturating_sub(1) {
                        if seg_point_dist(tracks[tj].pts[m], tracks[tj].pts[m + 1], end) <= half {
                            uf.union(tv_base[ti] + k, tv_base[tj] + m);
                            break;
                        }
                    }
                }
            }
        }

        // Copper pours. A plane is a conductor like any other: a pad on
        // the poured layer, or a via landing in it, joins the net through
        // the copper rather than through a track.
        //
        // This is the *optimistic* half of the pour model -- it assumes
        // the plane is one piece. The conservative half lives in the
        // router (`routing_pour_unreachable`), which flood-fills the real
        // free area and fails when the plane is cut into islands. Neither
        // subsumes the other: drop the router's check and this gate would
        // pass a board whose plane is confetti.
        let zones: Vec<&eda_model::ir::Zone> = rt.zones.iter().filter(|z| z.net == net.name).collect();
        let zone_base = via_base + vias.len();
        for (zi, z) in zones.iter().enumerate() {
            for (pi, pad) in net_pads.iter().enumerate() {
                if pad.layers.iter().any(|l| *l == z.layer) && point_in_polygon(Point { x: (pad.rect.0 + pad.rect.2) / 2, y: (pad.rect.1 + pad.rect.3) / 2 }, &z.outline) {
                    uf.union(pi, zone_base + zi);
                }
            }
            for (vi, v) in vias.iter().enumerate() {
                let spans = v.from_layer == z.layer || v.to_layer == z.layer;
                if spans && point_in_polygon(v.at, &z.outline) {
                    uf.union(via_base + vi, zone_base + zi);
                }
            }
        }

        let root = uf.find(0);
        if std::env::var_os("EDA_GATE_DEBUG").is_some() {
            for (pi, p) in net_pads.iter().enumerate() {
                eprintln!("gate {}: pad {} rect {:?} layers {:?} comp {}", net.name, p.refpin, p.rect, p.layers, uf.find(pi));
            }
            for (ti, t) in tracks.iter().enumerate() {
                eprintln!("gate {}: track {ti} {} {:?} comp {}", net.name, t.layer, t.pts, uf.find(tv_base[ti]));
            }
            for (vi, v) in vias.iter().enumerate() {
                eprintln!("gate {}: via {vi} {:?} comp {}", net.name, v.at, uf.find(via_base + vi));
            }
        }
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

/// Plain copper-to-copper clearance between different nets (tracks, vias,
/// pads -- exact edge-to-edge against one flat `clearance` floor,
/// `rules.clearance`): the router's own model, checked independently. See
/// `check_routing`'s call site.
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
    use eda_model::CheckStatus;

    #[test]
    fn seg_seg_distance_basics() {
        let p = |x, y| Point { x, y };
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(5, -5), p(5, 5)), 0.0);
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(0, 3), p(10, 3)), 3.0);
        assert_eq!(seg_seg_dist(p(0, 0), p(10, 0), p(13, 4), p(20, 4)), 5.0);
    }

    // ---------------------------------------------- workmanship gates
    use eda_model::ir::{FootprintInstance, PlacementSection, Provenance, RoutingSection};
    use eda_model::{Net, Part, Pin, PinKind};

    fn wpart(reference: &str, package: &str) -> Part {
        Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some(package.into()),
            footprint: None,
            pins: (1..=2).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(),
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        }
    }

    /// Two 0603s side by side on a 20 mm board: R1 at (5000,5000) has
    /// pads at x=4175/5825, C1 at (10000,5000) at x=9175/10825. Net A is
    /// R1.2 + C1.1 + C1.2, net B is R1.1 alone.
    fn wfixture(rt: RoutingSection) -> (Design, ConstraintModel) {
        let model = ConstraintModel {
            parts: vec![wpart("R1", "0603"), wpart("C1", "0603")],
            nets: vec![
                Net { name: "A".into(), pins: vec!["R1.2".into(), "C1.1".into(), "C1.2".into()] },
                Net { name: "B".into(), pins: vec!["R1.1".into()] },
            ],
            ..Default::default()
        };
        let outline = vec![Point { x: 0, y: 0 }, Point { x: 20000, y: 0 }, Point { x: 20000, y: 20000 }, Point { x: 0, y: 20000 }];
        let fpi = |id: &str, x| FootprintInstance { id: id.into(), at: Point { x, y: 5000 }, rot: 0, side: Side::Top, label: Default::default() };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection { outline, footprints: vec![fpi("R1", 5000), fpi("C1", 10000)], modules: Vec::new() }),
            routing: Some(rt),
            drawings: None,
        };
        (design, model)
    }

    fn track(net: &str, pts: &[(Um, Um)]) -> Track {
        Track { id: String::new(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: pts.iter().map(|&(x, y)| Point { x, y }).collect(), arc_mid_offset: None }
    }

    fn fails(design: &Design, model: &ConstraintModel, check: &str) -> Vec<CheckResult> {
        check_routing(design, model).into_iter().filter(|c| c.check == check && c.status == CheckStatus::Fail).collect()
    }

    /// Net A routed cleanly: R1.2 -> C1.1 straight, C1.1 -> C1.2 around
    /// the top of the C1 body.
    fn clean_routing() -> RoutingSection {
        RoutingSection {
            tracks: vec![track("A", &[(5825, 5000), (9175, 5000)]), track("A", &[(9175, 5000), (9175, 3500), (10825, 3500), (10825, 5000)])],
            vias: vec![],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }
    }

    #[test]
    fn a_board_with_no_stackup_fails_instead_of_being_given_one() {
        // Guessing F.Cu/B.Cu put every SMD pad on an invented layer, after
        // which the clearance gate compares copper that shares a name with
        // nothing on the board -- and reports it clean.
        let (design, mut model) = wfixture(clean_routing());
        model.board.layers.clear();
        let out = check_routing(&design, &model);
        assert!(
            out.iter().any(|c| c.check == "routing_stackup" && c.status == CheckStatus::Fail),
            "expected routing_stackup to fail, got {:?}",
            out.iter().map(|c| (&c.check, &c.status)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn workmanship_gates_pass_on_clean_routing() {
        let (d, m) = wfixture(clean_routing());
        for check in ["routing_pass_through_pad", "routing_via_in_pad", "routing_between_smd_pads", "routing_over_refdes"] {
            assert!(fails(&d, &m, check).is_empty(), "{check} should pass: {:?}", fails(&d, &m, check));
        }
    }

    #[test]
    fn pass_through_pad_fails_when_track_crosses_own_net_pad() {
        // R1.2 -> C1.2 straight through C1.1 (same net) without stopping.
        let rt = RoutingSection { tracks: vec![track("A", &[(5825, 5000), (10825, 5000)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };
        let (d, m) = wfixture(rt);
        let f = fails(&d, &m, "routing_pass_through_pad");
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].location.as_deref().unwrap().contains("C1.1"));
    }

    #[test]
    fn via_in_pad_fails_when_via_overlaps_pad() {
        let mut rt = clean_routing();
        rt.vias.push(Via { id: String::new(), net: "A".into(), at: Point { x: 9175, y: 5000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let (d, m) = wfixture(rt);
        assert_eq!(fails(&d, &m, "routing_via_in_pad").len(), 1);
        // Just outside the pad copper (pad top edge at 5000-475): clean.
        rt = clean_routing();
        rt.vias.push(Via { id: String::new(), net: "A".into(), at: Point { x: 9175, y: 3800 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let (d, m) = wfixture(rt);
        assert!(fails(&d, &m, "routing_via_in_pad").is_empty());
    }

    #[test]
    fn between_smd_pads_fails_when_foreign_track_threads_the_gap() {
        // Net Z (on neither pad) vertical at x=5000: through the gap
        // between R1.1 and R1.2 (pad edges at 4575 and 5425, so clearance
        // is fine). The same track on net B, R1.1's own net, is that pad's
        // connection and must not count.
        let mut rt = clean_routing();
        rt.tracks.push(track("Z", &[(5000, 2000), (5000, 8000)]));
        let (d, m) = wfixture(rt);
        assert!(fails(&d, &m, "routing_clearance").is_empty());
        assert_eq!(fails(&d, &m, "routing_between_smd_pads").len(), 1);
        let mut rt = clean_routing();
        rt.tracks.push(track("B", &[(5000, 2000), (5000, 8000)]));
        let (d, m) = wfixture(rt);
        assert!(fails(&d, &m, "routing_between_smd_pads").is_empty());
    }

    #[test]
    fn over_refdes_fails_when_track_runs_under_the_label() {
        let (d0, m) = wfixture(clean_routing());
        let pl = d0.placement.as_ref().unwrap();
        let (bx, _) = refdes_box(&m, pl, &pl.footprints[1]).unwrap();
        let y = (bx.1 + bx.3) / 2;
        let mut rt = clean_routing();
        rt.tracks.push(track("B", &[(7000, y), (13000, y)]));
        let (d, m) = wfixture(rt);
        assert_eq!(fails(&d, &m, "routing_over_refdes").len(), 1);
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
