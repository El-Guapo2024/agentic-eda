//! Port of `PNS::DIFF_PAIR_PLACER` (`pcbnew/router/pns_diff_pair_placer.
//! {h,cpp}`), gap #7's task item 6: route two parallel, gap-matched lines
//! (a differential pair) from one click-drag session, keyed `6`.
//!
//! ## Scope -- significantly narrower than upstream, by necessity
//!
//! Upstream's `DIFF_PAIR_PLACER` routes a genuine new `PNS::ITEM` kind
//! (`DIFF_PAIR_T`, decision #3-adjacent in `PARITY.md`: not modeled at
//! all in this port's `Item` enum) with its own dedicated hull/collision/
//! shove/walkaround machinery, so the *pair itself* is a first-class
//! obstacle-resolution unit (nudging both lines together around
//! something in the way). Building that whole second collision model was
//! out of proportion to this task's remaining scope, so this port takes
//! a structurally simpler approach instead:
//!
//! 1. Build one **spine** centerline with the exact same direct-45-trace
//!    logic [`crate::line_placer::LinePlacer`] uses
//!    (`Direction45::build_initial_trace`) -- no walkaround, no shove.
//! 2. Derive the two actual lines by offsetting the spine perpendicular
//!    by `(gap + width) / 2` on each side ([`offset_polyline`]), snapping
//!    their very first (and, on a real finish, last) point to the real
//!    pad each one is actually supposed to land on.
//! 3. Collision-check each offset line independently against the board;
//!    if *either* collides, the whole preview is flagged colliding --
//!    the same user-facing contract as [`crate::settings::Mode::
//!    MarkObstacles`] (report, don't resolve). **There is no shove or
//!    walkaround for a diff pair in this port** -- a documented gap, not
//!    an oversight; see `PARITY.md`.
//! 4. No via/layer-switch mid-pair-route either (single layer only this
//!    session) -- `PARITY.md` tracks this too.
//!
//! This is a real, usable feature for the common case this task actually
//! asks for (lay a clean, gap-matched pair along an open path, same as a
//! single track in `MarkObstacles` mode already does today), just not a
//! collision-resolving one yet.
//!
//! ## Pair detection and sizing
//!
//! [`dp_coupled_net_name`] is `BOARD::MatchDpSuffix` ported verbatim
//! (`pcbnew/board.cpp`): walk a net name backward past any trailing
//! digits/underscores, and if the next character is `+`/`-`/`P`/`N`,
//! swap it for its complement (`+`<->`-`, `P`<->`N`), preserving
//! whatever digits/underscores followed it (`"LVDS_P0"` ->
//! `"LVDS_N0"`). Gap/width come from `BoardRules::diff_pair_gap_of`/
//! `diff_pair_width_of` (`crates/model/src/lib.rs`) -- the net-class
//! fields the task brief points at, falling back to upstream's own
//! `SIZES_SETTINGS` hardcoded defaults (125um/180um) when a board has no
//! diff-pair-specific net class, which -- per this workspace's own
//! example boards -- is every example board today.

use crate::direction45::{CornerMode, Direction45};
use crate::item::{net_of, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::optimizer::intersect_lines;
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;

/// `BOARD::MatchDpSuffix`: the complementary net name for a differential
/// pair, by KiCad's own naming convention, or `None` if `net_name` doesn't
/// end (after any trailing digits/underscores) in a recognized suffix
/// character. See this module's header comment for the exact algorithm
/// and worked examples.
pub fn dp_coupled_net_name(net_name: &str) -> Option<String> {
    let chars: Vec<char> = net_name.chars().collect();
    let mut count = 0usize;
    let mut complement: Option<char> = None;
    for &ch in chars.iter().rev() {
        count += 1;
        if ch.is_ascii_digit() || ch == '_' {
            continue;
        }
        complement = match ch {
            '+' => Some('-'),
            '-' => Some('+'),
            'N' => Some('P'),
            'P' => Some('N'),
            _ => None,
        };
        break;
    }
    let complement = complement?;
    let len = chars.len();
    if count == 0 || count > len {
        return None;
    }
    let prefix: String = chars[..len - count].iter().collect();
    let suffix: String = chars[(len - count + 1)..].iter().collect();
    Some(format!("{prefix}{complement}{suffix}"))
}

/// Offsets a 45-degree polyline perpendicular by `offset` (signed: positive
/// rotates each segment's direction -90 degrees -- clockwise, since board
/// +y is down -- negative the other way), re-joining consecutive offset
/// segments at their own line-line meeting point ([`intersect_lines`])
/// rather than just translating every vertex by the same amount, which is
/// what actually keeps corners mitered instead of gapped/overlapping --
/// the standard "parallel curve of a polyline" construction. A run of
/// exactly-collinear segments (e.g. either side of a point that isn't a
/// real corner) degenerates `intersect_lines` to `None`; the shared
/// endpoint is used directly in that case, which is exact for collinear
/// input (the two offset segments are themselves exactly collinear too).
pub fn offset_polyline(centerline: &[Point], offset: i64) -> Vec<Point> {
    if centerline.len() < 2 {
        return centerline.to_vec();
    }
    let mut offset_segs: Vec<(Point, Point)> = Vec::with_capacity(centerline.len() - 1);
    for w in centerline.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            continue; // a zero-length leg contributes no direction to offset by; skip it.
        }
        let (nx, ny) = (dy / len, -dx / len);
        let (ox, oy) = ((nx * offset as f64).round() as Um, (ny * offset as f64).round() as Um);
        offset_segs.push((Point { x: a.x + ox, y: a.y + oy }, Point { x: b.x + ox, y: b.y + oy }));
    }
    let Some(&(first_a, _)) = offset_segs.first() else { return centerline.to_vec() };
    let mut out = vec![first_a];
    for pair in offset_segs.windows(2) {
        let (s1a, s1b) = pair[0];
        let (s2a, s2b) = pair[1];
        out.push(intersect_lines(s1a, s1b, s2a, s2b).unwrap_or(s1b));
    }
    out.push(offset_segs.last().unwrap().1);
    out
}

fn midpoint(a: Point, b: Point) -> Point {
    Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
}

/// Which perpendicular side (of `spine_head`'s own first real segment's
/// direction) `net_a` is on, as `+1.0`/`-1.0` -- resolved from the pads'
/// own separation vector (`pad_b - pad_a`) the first time a
/// non-degenerate head exists (a zero-length head has no direction to
/// measure that separation against yet, so this returns a provisional
/// `1.0` without the caller locking anything in). Once a real `fix`/
/// `finish` locks a value in (`DiffPairPlacer::a_sign`), callers pass it
/// back in as `locked` and get it back unchanged -- the two lines must
/// never swap sides mid-route.
fn resolve_sign(locked: Option<f64>, pad_a: Point, pad_b: Point, spine_head: &[Point]) -> f64 {
    if let Some(s) = locked {
        return s;
    }
    spine_head
        .windows(2)
        .find(|w| w[0] != w[1])
        .map(|w| {
            let (dx, dy) = ((w[1].x - w[0].x) as f64, (w[1].y - w[0].y) as f64);
            let (perp_x, perp_y) = (dy, -dx); // same -90-degree rotation `offset_polyline` uses for a positive offset.
            let (lx, ly) = ((pad_b.x - pad_a.x) as f64, (pad_b.y - pad_a.y) as f64);
            if perp_x * lx + perp_y * ly > 0.0 {
                -1.0 // perpendicular already points toward pad_b -- net_a goes the other way.
            } else {
                1.0
            }
        })
        .unwrap_or(1.0)
}

/// The live preview: both lines' current head, plus whatever either has
/// already fixed this session.
#[derive(Debug, Clone)]
pub struct DiffPairPreview {
    pub layer: i32,
    pub width: Um,
    pub runs_a: Vec<Line>,
    pub runs_b: Vec<Line>,
    pub head_a: Line,
    pub head_b: Line,
    pub colliding: bool,
    /// Both lines found a same-net anchor near the cursor at once --
    /// finishing now would land exactly on a real coupled pad pair.
    pub snapped_end: bool,
    /// [`resolve_sign`]'s own result for this preview -- `fix`/`finish`
    /// reuse it to lock `DiffPairPlacer::a_sign` in without recomputing
    /// the spine trace a second time.
    resolved_sign: f64,
}

pub struct DiffPairPlacer {
    pub net_a: Net,
    pub net_b: Net,
    pub width: Um,
    pub gap: Um,
    pub layer: i32,
    /// `net_a`/`net_b`'s own real starting anchors -- every `a`/`b` line's
    /// very first point snaps exactly to these, never to a geometrically
    /// offset point, so the pair always actually reaches the pads it
    /// started from regardless of their exact spacing relative to
    /// `width + gap` (see the module doc comment's step 2).
    pad_a: Point,
    pad_b: Point,
    /// The spine's own reference start -- `midpoint(pad_a, pad_b)`,
    /// advanced to the last fixed leg's own spine endpoint as the session
    /// progresses (see `fixed_spine_start`).
    spine_origin: Point,
    direction: Direction45,
    /// See [`resolve_sign`]'s own doc comment.
    a_sign: Option<f64>,
    runs: Vec<(Line, Line)>,
    pub idle: bool,
    pub placement_correct: bool,
}

impl DiffPairPlacer {
    /// `DIFF_PAIR_PLACER::Start`, with the pair-finding half of upstream's
    /// own `FindDpPrimitivePair`: `clicked_pos`/`clicked_net` is whatever
    /// the user actually clicked (`Router::start_diff_pair` resolves this
    /// the same way `Router::start` resolves a single-track start); this
    /// finds the complementary net via [`dp_coupled_net_name`] and the
    /// real board anchor nearest `clicked_pos` on it (the common real-
    /// world layout: a diff pair's two pads sit right next to each other
    /// on the same footprint/connector). `None` if `clicked_net` has no
    /// recognized DP suffix, or if nothing on the complementary net
    /// exists anywhere on the board to pair with.
    pub fn start(node: &Node, clicked_pos: Point, clicked_net: &Net, layer: i32, rules: &BoardRules) -> Option<Self> {
        let net_a_name = clicked_net.as_deref()?;
        let other_name = dp_coupled_net_name(net_a_name)?;
        let net_b = net_of(&other_name);
        // A generous search radius -- diff-pair pads are typically well
        // under a few mm apart, but this isn't itself a clearance-bearing
        // distance, just "how far to look for the other half of the
        // pair", so erring wide is harmless.
        const PAIR_SEARCH_UM: Um = 20_000;
        let (_, pad_b) = node.nearest_anchor(clicked_pos, LayerRange::single(layer), PAIR_SEARCH_UM, Some(&net_b))?;
        let width = rules.diff_pair_width_of(net_a_name);
        let gap = rules.diff_pair_gap_of(net_a_name);
        Some(DiffPairPlacer {
            net_a: clicked_net.clone(),
            net_b,
            width,
            gap,
            layer,
            pad_a: clicked_pos,
            pad_b,
            spine_origin: midpoint(clicked_pos, pad_b),
            direction: Direction45::N,
            a_sign: None,
            runs: Vec::new(),
            idle: false,
            placement_correct: false,
        })
    }

    fn fixed_spine_start(&self) -> Point {
        self.runs.last().map(|(a, b)| midpoint(a.last().unwrap_or(self.pad_a), b.last().unwrap_or(self.pad_b))).unwrap_or(self.spine_origin)
    }

    fn fixed_a_end(&self) -> Point {
        self.runs.last().and_then(|(a, _)| a.last()).unwrap_or(self.pad_a)
    }

    fn fixed_b_end(&self) -> Point {
        self.runs.last().and_then(|(_, b)| b.last()).unwrap_or(self.pad_b)
    }

    fn half_total(&self) -> Um {
        (self.gap + self.width) / 2
    }

    /// `DIFF_PAIR_PLACER::Move`: the live preview at `p`, direct-45-traced
    /// from the spine's current end (no walkaround/shove -- see the
    /// module doc comment). Pure -- never mutates `self`, not even
    /// `a_sign` (see [`resolve_sign`]'s own doc comment on how a caller
    /// that wants to actually lock it in reuses `resolved_sign`).
    pub fn preview(&self, node: &Node, rules: &BoardRules, p: Point) -> DiffPairPreview {
        let spine_start = self.fixed_spine_start();
        let spine_head = if spine_start == p { vec![spine_start] } else { self.direction.build_initial_trace(spine_start, p, false, CornerMode::Mitered45) };
        let sign = resolve_sign(self.a_sign, self.pad_a, self.pad_b, &spine_head);
        let half = self.half_total();

        let mut a_pts = offset_polyline(&spine_head, (sign * half as f64).round() as i64);
        let mut b_pts = offset_polyline(&spine_head, (-sign * half as f64).round() as i64);
        if let Some(first) = a_pts.first_mut() {
            *first = self.fixed_a_end();
        }
        if let Some(first) = b_pts.first_mut() {
            *first = self.fixed_b_end();
        }

        // End-of-route snapping: both lines must find a same-net anchor
        // near the cursor's own offset endpoint at once for this to count
        // as a real, pair-complete end (mirrors `LinePlacer::preview`'s
        // single-net `nearest_anchor` snap).
        let layers = LayerRange::single(self.layer);
        let snap_a = a_pts.last().and_then(|&q| node.nearest_anchor(q, layers, crate::line_placer::SNAP_UM, Some(&self.net_a)));
        let snap_b = b_pts.last().and_then(|&q| node.nearest_anchor(q, layers, crate::line_placer::SNAP_UM, Some(&self.net_b)));
        let snapped_end = snap_a.is_some() && snap_b.is_some();
        if snapped_end {
            if let (Some(last), Some((_, pos))) = (a_pts.last_mut(), snap_a) {
                *last = pos;
            }
            if let (Some(last), Some((_, pos))) = (b_pts.last_mut(), snap_b) {
                *last = pos;
            }
        }

        let colliding = line_collides(node, &a_pts, self.layer, self.width, &self.net_a, rules) || line_collides(node, &b_pts, self.layer, self.width, &self.net_b, rules);

        DiffPairPreview {
            layer: self.layer,
            width: self.width,
            runs_a: self.runs.iter().map(|(a, _)| a.clone()).collect(),
            runs_b: self.runs.iter().map(|(_, b)| b.clone()).collect(),
            head_a: Line::from_points(self.net_a.clone(), self.layer, self.width, a_pts),
            head_b: Line::from_points(self.net_b.clone(), self.layer, self.width, b_pts),
            colliding,
            snapped_end,
            resolved_sign: sign,
        }
    }

    /// `DIFF_PAIR_PLACER::FixRoute` for an intermediate click. `None`
    /// (refused) if either line still collides -- this port's pair
    /// routing has no `MarkObstacles`-style "commit anyway" override
    /// (upstream's own `AllowDRCViolations` is single-track-only in this
    /// port too, see `line_placer.rs`'s matching guard). `Some(real_end)`
    /// on success.
    pub fn fix(&mut self, node: &Node, rules: &BoardRules, p: Point) -> Option<bool> {
        let preview = self.preview(node, rules, p);
        if preview.colliding {
            return None;
        }
        self.a_sign = Some(preview.resolved_sign);
        if preview.head_a.point_count() < 2 {
            // Zero movement: only a legitimate "finish" if it's onto a real anchor pair.
            return if preview.snapped_end { Some(true) } else { None };
        }
        self.direction = preview.head_a.pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap_or(self.direction);
        let real_end = preview.snapped_end;
        self.runs.push((preview.head_a, preview.head_b));
        self.placement_correct = true;
        Some(real_end)
    }

    /// `DIFF_PAIR_PLACER::UnfixRoute` (Backspace).
    pub fn undo_last_segment(&mut self) -> bool {
        let Some(popped) = self.runs.pop() else { return false };
        self.direction = self
            .runs
            .last()
            .and_then(|(a, _)| a.pts.windows(2).rev().find(|w| w[0] != w[1]))
            .map(|w| Direction45::from_seg(w[0], w[1]))
            .unwrap_or(self.direction);
        let _ = popped;
        true
    }

    /// `DIFF_PAIR_PLACER::FlipPosture` (the `/` key) -- same spine-posture
    /// flip as `LinePlacer::flip_posture`; never touches `a_sign` (which
    /// side of the spine each net is on is a property of the pads
    /// themselves, not the routing posture).
    pub fn flip_posture(&mut self) {
        self.direction = self.direction.right();
    }

    /// `DIFF_PAIR_PLACER::FixRoute` with `aForceFinish`: commit the final
    /// head (refusing a collision the same way [`Self::fix`] does) and
    /// return every leg this session placed, as `(a_runs, b_runs)`.
    pub fn finish(&mut self, node: &Node, rules: &BoardRules, p: Point) -> Option<(Vec<Line>, Vec<Line>)> {
        let preview = self.preview(node, rules, p);
        if preview.colliding {
            return None;
        }
        self.a_sign = Some(preview.resolved_sign);
        if preview.head_a.point_count() >= 2 {
            self.runs.push((preview.head_a, preview.head_b));
        }
        self.idle = true;
        self.placement_correct = !self.runs.is_empty();
        Some((self.runs.iter().map(|(a, _)| a.clone()).collect(), self.runs.iter().map(|(_, b)| b.clone()).collect()))
    }
}

fn line_collides(node: &Node, pts: &[Point], layer: i32, width: Um, net: &Net, rules: &BoardRules) -> bool {
    let layers = LayerRange::single(layer);
    pts.windows(2).any(|w| {
        let shape = Shape::Stadium { a: w[0], b: w[1], r: width.max(1) / 2 };
        node.first_colliding(&shape, net, layers, rules, &[]).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Item;
    use crate::item::Segment;
    use crate::item::Solid;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn dp_coupled_net_name_matches_every_upstream_convention() {
        assert_eq!(dp_coupled_net_name("USB_DP"), Some("USB_DN".to_string()));
        assert_eq!(dp_coupled_net_name("USB_DN"), Some("USB_DP".to_string()));
        assert_eq!(dp_coupled_net_name("CLK+"), Some("CLK-".to_string()));
        assert_eq!(dp_coupled_net_name("CLK-"), Some("CLK+".to_string()));
        assert_eq!(dp_coupled_net_name("LVDS_P0"), Some("LVDS_N0".to_string()), "trailing digits after the suffix letter must survive");
        assert_eq!(dp_coupled_net_name("LVDS_N12"), Some("LVDS_P12".to_string()));
        assert_eq!(dp_coupled_net_name("GND"), None, "no recognized suffix at all");
        assert_eq!(dp_coupled_net_name(""), None);
        assert_eq!(dp_coupled_net_name("P"), Some("N".to_string()), "the whole name can be just the suffix letter");
    }

    #[test]
    fn offset_polyline_keeps_a_straight_line_straight() {
        let centerline = vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }];
        let offset = offset_polyline(&centerline, 100);
        // E direction: perpendicular (dy,-dx)/len = (0,-1) for a +100 offset -- shifts "up" (negative y).
        assert_eq!(offset, vec![Point { x: 0, y: -100 }, Point { x: 1000, y: -100 }]);
    }

    #[test]
    fn offset_polyline_miters_a_corner_instead_of_gapping_it() {
        // A 90-degree corner (E then S).
        let centerline = vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }];
        let offset = offset_polyline(&centerline, 100);
        assert_eq!(offset.len(), 3, "still exactly one corner, not a gap/overlap at the join");
        let (a, b, c) = (offset[0], offset[1], offset[2]);
        assert_eq!(a.y, b.y, "first offset leg stays horizontal");
        assert_eq!(b.x, c.x, "second offset leg stays vertical");
    }

    #[test]
    fn starts_from_a_pad_and_finds_the_coupled_pad_by_suffix() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 200 }, source: "J1.1".into(), edge: false }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 500 }, shape: Shape::Circle { c: Point { x: 0, y: 500 }, r: 200 }, source: "J1.2".into(), edge: false }));
        let rules = rules();
        let placer = DiffPairPlacer::start(&node, Point { x: 0, y: 0 }, &net_of("USB_DP"), 0, &rules).expect("must find the coupled pad");
        assert_eq!(placer.pad_a, Point { x: 0, y: 0 });
        assert_eq!(placer.pad_b, Point { x: 0, y: 500 });
        assert_eq!(placer.width, 125, "no diff-pair net class -- upstream's own SIZES_SETTINGS default");
        assert_eq!(placer.gap, 180);
    }

    #[test]
    fn refuses_a_net_with_no_recognized_suffix() {
        let node = Node::new();
        let rules = rules();
        assert!(DiffPairPlacer::start(&node, Point { x: 0, y: 0 }, &net_of("GND"), 0, &rules).is_none());
    }

    #[test]
    fn refuses_when_nothing_exists_on_the_coupled_net() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 200 }, source: "J1.1".into(), edge: false }));
        let rules = rules();
        assert!(DiffPairPlacer::start(&node, Point { x: 0, y: 0 }, &net_of("USB_DP"), 0, &rules).is_none(), "no USB_DN anywhere on the board to pair with");
    }

    #[test]
    fn routes_a_straight_pair_and_finishes_onto_the_coupled_destination_pads() {
        let mut node = Node::new();
        // Pad separation (900) picked so SNAP_UM (500) reaches both pads
        // from one click near their shared midpoint, while still clearing
        // the *other* net's own pads by more than track half-width (62)
        // + board clearance (200) + the 50-radius pad itself.
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 50 }, source: "J1.1".into(), edge: false }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 900 }, shape: Shape::Circle { c: Point { x: 0, y: 900 }, r: 50 }, source: "J1.2".into(), edge: false }));
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::new(0, 1), pos: Point { x: 10_000, y: 0 }, shape: Shape::Circle { c: Point { x: 10_000, y: 0 }, r: 50 }, source: "U1.1".into(), edge: false }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::new(0, 1), pos: Point { x: 10_000, y: 900 }, shape: Shape::Circle { c: Point { x: 10_000, y: 900 }, r: 50 }, source: "U1.2".into(), edge: false }));
        let rules = rules();
        let mut placer = DiffPairPlacer::start(&node, Point { x: 0, y: 0 }, &net_of("USB_DP"), 0, &rules).unwrap();
        // The pair's own shared midpoint -- within SNAP_UM of both U1 pads at once.
        let target = Point { x: 10_000, y: 450 };
        let preview = placer.preview(&node, &rules, target);
        assert!(!preview.colliding);
        assert!(preview.snapped_end, "the cursor sits within range of both of U1's own pads at once");
        let (runs_a, runs_b) = placer.finish(&node, &rules, target).expect("collision-free finish must succeed");
        assert_eq!(runs_a.len(), 1);
        assert_eq!(runs_b.len(), 1);
        assert_eq!(runs_a[0].first(), Some(Point { x: 0, y: 0 }), "the A line must start exactly on its own real pad");
        assert_eq!(runs_a[0].last(), Some(Point { x: 10_000, y: 0 }), "and end exactly on its own real destination pad");
        assert_eq!(runs_b[0].first(), Some(Point { x: 0, y: 900 }));
        assert_eq!(runs_b[0].last(), Some(Point { x: 10_000, y: 900 }));
    }

    #[test]
    fn a_pad_directly_on_one_lines_path_is_reported_as_colliding() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 200 }, source: "J1.1".into(), edge: false }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 300 }, shape: Shape::Circle { c: Point { x: 0, y: 300 }, r: 200 }, source: "J1.2".into(), edge: false }));
        // Sits right on the A line's own straight path (y=0), a totally
        // different net -- this port's diff pair has no shove/walkaround
        // (module doc comment), so this must simply be flagged, not routed
        // around.
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkB".into(), 0)), locked: false }));
        let rules = rules();
        let placer = DiffPairPlacer::start(&node, Point { x: 0, y: 0 }, &net_of("USB_DP"), 0, &rules).unwrap();
        let preview = placer.preview(&node, &rules, Point { x: 5000, y: 0 });
        assert!(preview.colliding);
    }
}
