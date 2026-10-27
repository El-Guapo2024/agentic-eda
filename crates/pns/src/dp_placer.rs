//! Port of `PNS::DIFF_PAIR_PLACER` (`pcbnew/router/pns_diff_pair_placer.{h,cpp}`): the interactive router for a differential pair --
//! both nets at once, from a pair of pads, vias or track ends to wherever the cursor goes, in the router's mode.
//!
//! The geometry (gateways, the pair, the coupled length) is [`crate::diff_pair`]; this is the session:
//!
//! - [`DiffPairPlacer::start`] is `Start` with `FindDpPrimitivePair` ([`find_dp_primitive_pair`]): the item under the cursor and the
//!   nearest item of the coupled net of the same kind (and layers) are the pair's start (`DP_PRIMITIVE_PAIR`); a pair of tracks
//!   has to be at its end (`getDanglingAnchor`).
//! - [`DiffPairPlacer::move_to`] is `Move`: `routeHead` fits the start's gateways (`gwsEntry.BuildFromPrimitivePair`) to the end's
//!   (`BuildFromPrimitivePair` of the coupled pair under the cursor, else `BuildForCursor`, the cursor first pushed away from what
//!   the pair's circle would sit on, `propagateDpHeadForces`), then the mode decides what happens to an obstacle: `rhMarkObstacles`
//!   only reports it, `rhWalkOnly` walks the pair around it ([`DiffPairPlacer::try_walk_dp`]: one line is walked around the obstacle,
//!   the other is shoved to the gap from it), `rhShoveOnly` walks the pair around the pads and shoves the tracks and vias, both lines
//!   as heads of one shove.
//! - [`DiffPairPlacer::fix_route`] is `FixRoute`: the pair is added to the world, the next leg starts from where it ended
//!   (`EndingPrimitives`) and `m_initialDiagonal` carries the posture on. A pair with vias (`V`, [`DiffPairPlacer::toggle_via`]) ends in
//!   two vias a `viaGap()` apart and goes on on the other layer from them (`SetLayer`).
//!
//! **Springback** (`SHOVE`'s `m_nodeStack`, [`crate::springback`]): in Shove mode every `Move` starts its shove from the newest frame the
//! new heads still press on (`reduceSpringback`, both lines' heads asked about together, so a frame one line still presses on stays for
//! the other too) and pushes the world the shove leaves (`pushSpringback`), so a track or via the pair pushed stays pushed while the pair
//! still presses on it, is pushed on from where it was left, and springs back as the cursor draws away. What the code routes, walks and
//! shoves against is that frame (`m_currentNode`); a fix then hands it to the commit, and `FixRoute` makes a fresh `SHOVE` over the
//! committed world (`m_shove = std::make_unique<SHOVE>( m_world, .. )` before `CommitPlacement`), so the stack restarts empty at every fix
//! and no frame is ever locked. Backspace, which KiCad's pair placer does not implement (see below), clears the stack with the leg.
//!
//! What differs from KiCad, on purpose:
//!
//! - (Not a difference: `tryWalkDp` ends with `if( bestScore > 0.0 ) {..; return true;}`, which is always true -- when no attempt worked,
//!   `best` is an empty `DIFF_PAIR` and the walk is called a success with an empty pair; [`DiffPairPlacer::try_walk_dp`] does the same.
//!   Of the attempts that work it keeps the one of the lowest `cl + |skew| * 3`, the *shortest* coupled length.)
//! - **Four attempts are two.** `tryWalkDp` tries `attempt` 0..=3 with `pfirst = attempt & 1` and a winding `attempt & 2` that
//!   `attemptWalk` never reads; attempts 2 and 3 repeat 0 and 1.
//! - **Backspace** (`UnfixRoute`, which `DIFF_PAIR_PLACER` does not implement) takes back the last fixed leg: the world, the posture and
//!   the pair it went on from are put back as they were before it.
//! - The sizes are `ImportSizes`' ([`DpSizes::resolve`]): the width at least the board's minimum track width, the gap and the via gap
//!   the class's pair gap, at least the board's minimum clearance.

use crate::diff_pair::{direction_of, line_project, vnorm, vsub, DiffPair, DpGateways, DpPrimitivePair, PairLine, ANG_HALF_FULL, ANG_STRAIGHT};
use crate::direction45::Direction45;
use crate::item::{net_of, same_net, Item, ItemId, Kind, Net, Via};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::line_placer::{FixOutcome, SNAP_NM};
use crate::node::{kind_mask, Node, QueryOpts};
use crate::settings::{Mode, RoutingSettings};
use crate::shove::{DisplacedLine, DisplacedVia, ShoveHead};
use crate::springback::{HeadSet, Springback};
use crate::walkaround::{WalkPolicy, WalkStatus, Walker};
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use std::collections::{BTreeMap, HashSet};

// ------------------------------------------------------------------------------------------------
// sizes
// ------------------------------------------------------------------------------------------------

/// The parts of `SIZES_SETTINGS` a pair uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpSizes {
    /// `DiffPairWidth`.
    pub width: Nm,
    /// `DiffPairGap`: edge to edge.
    pub gap: Nm,
    /// `DiffPairViaGap`: edge to edge between the two vias.
    pub via_gap: Nm,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    /// `GetDiffPairHoleToHole`.
    pub hole_to_hole: Nm,
}

impl DpSizes {
    /// `PNS_KICAD_IFACE_BASE::ImportSizes` for a pair, from the net class of the P net (the nets of a pair are in one class), in net-class
    /// mode: the width starts at the board's minimum track width and takes the larger of that and the class's (`std::max( diffPairWidth,
    /// constraint.m_Value.Opt() )`); the gap **and the via gap** start at the board's minimum clearance (`int diffPairGap =
    /// bds.m_MinClearance; int diffPairViaGap = bds.m_MinClearance;`) and both take the `DIFF_PAIR_GAP` constraint, the class's pair gap
    /// (`diffPairViaGap = std::max( diffPairViaGap, constraint.m_Value.Opt() )`) -- the class's own via gap is not read. The clearance
    /// between the two nets plays no part (findings p5 F4-1: it raised a 0.1 mm pair gap to a class clearance of 0.5 mm).
    pub fn resolve(rules: &BoardRules, net_p: &str, _net_n: &str) -> Self {
        let gap = rules.diff_pair_gap_of(net_p).max(rules.min_clearance_nm);
        DpSizes {
            width: rules.diff_pair_width_of(net_p).max(rules.track_width_min_nm),
            gap,
            via_gap: gap,
            via_diameter: rules.via_diameter_of(net_p),
            via_drill: rules.via_drill_of(net_p),
            hole_to_hole: rules.hole_to_hole_min_nm,
        }
    }

    /// `DIFF_PAIR_PLACER::viaGap`.
    pub fn via_gap_total(&self) -> Nm {
        self.via_gap.max(self.hole_to_hole + self.via_drill - self.via_diameter)
    }

    /// `DIFF_PAIR_PLACER::gap`: the pitch of the two lines, centre to centre.
    pub fn pitch(&self) -> Nm {
        self.gap + self.width
    }
}

// ------------------------------------------------------------------------------------------------
// FindDpPrimitivePair
// ------------------------------------------------------------------------------------------------

/// `DpNetPair`'s refusal.
const NO_COMPLEMENT: &str = "Unable to find complementary differential pair nets. Make sure the names of the nets belonging to a differential pair end with either N/P or +/-.";
/// `FindDpPrimitivePair`'s refusal when the item has no dangling end.
const NOT_AT_THE_END: &str = "Can't find a suitable starting point.  If starting from an existing differential pair make sure you are at the end.";

/// `getDanglingAnchor`: where a pair can start on an item -- a pad's or via's centre, or the end of a track nothing else joins.
pub fn dangling_anchor(node: &Node, item: &Item) -> Option<Point> {
    match item {
        Item::Via(_) | Item::Solid(_) => item.anchors().first().copied(),
        Item::Segment(_) | Item::Arc(_) => {
            // `getDanglingAnchor`: `ARC_T` is `Arc().GetP0()` / `GetP1()` like a segment's ends
            let (a, b) = item.track_ends()?;
            let lonely = |p: Point| node.joint_at(p, item.net()).is_some_and(|j| j.links.len() == 1);
            if lonely(a) {
                Some(a)
            } else if lonely(b) {
                Some(b)
            } else {
                None
            }
        }
    }
}

/// What [`find_dp_primitive_pair`] found: the pair, and the items in the world it is made of.
#[derive(Debug, Clone)]
pub struct FoundPair {
    pub pair: DpPrimitivePair,
    pub id_p: ItemId,
    pub id_n: ItemId,
}

/// `DIFF_PAIR_PLACER::FindDpPrimitivePair`: the item `item_id` and the item of the same kind, on the coupled net, whose dangling anchor
/// is nearest to its own (a pad or via has to be on the same layers too). The pair is P first, whichever of the two was asked for
/// (`DpNetPair`: the net named `+` or `P` is P).
pub fn find_dp_primitive_pair(node: &Node, item_id: ItemId) -> Result<FoundPair, String> {
    let item = node.get(item_id).ok_or_else(|| NO_COMPLEMENT.to_string())?;
    let ref_net = item.net().clone();
    let ref_name = ref_net.as_deref().ok_or_else(|| NO_COMPLEMENT.to_string())?;
    let polarity = crate::diff_pair::dp_net_polarity(ref_name);
    let coupled_name = crate::diff_pair::dp_coupled_net_name(ref_name).filter(|_| polarity != 0).ok_or_else(|| NO_COMPLEMENT.to_string())?;
    let coupled_net = net_of(&coupled_name);

    let ref_anchor = dangling_anchor(node, item).ok_or_else(|| NOT_AT_THE_END.to_string())?;

    // `AllItemsInNet( coupledNet )`, nearest first; a tie goes to the lower id so that the result does not depend on the table's order
    let mut best: Option<(Nm, ItemId, Point)> = None;
    for (id, other) in node.iter() {
        if !same_net(other.net(), &coupled_net) || other.kind() != item.kind() {
            continue;
        }
        let Some(anchor) = dangling_anchor(node, other) else { continue };
        if matches!(item.kind(), Kind::Solid | Kind::Via) && other.layers() != item.layers() {
            continue; // `shapeMatches`
        }
        let dist = vnorm(vsub(anchor, ref_anchor));
        if best.is_none_or(|(d, bid, _)| (dist, id) < (d, bid)) {
            best = Some((dist, id, anchor));
        }
    }
    let Some((_, other_id, other_anchor)) = best else {
        return Err(format!("Can't find a suitable starting point for coupled net \"{coupled_name}\"."));
    };
    let other = node.get(other_id).cloned().expect("found in the node");
    let (mut pair, id_p, id_n);
    if polarity != 1 {
        // `refNet != netP`: the item found is P
        pair = DpPrimitivePair::new(other, item.clone());
        pair.anchor_p = other_anchor;
        pair.anchor_n = ref_anchor;
        (id_p, id_n) = (other_id, item_id);
    } else {
        pair = DpPrimitivePair::new(item.clone(), other);
        pair.anchor_p = ref_anchor;
        pair.anchor_n = other_anchor;
        (id_p, id_n) = (item_id, other_id);
    }
    Ok(FoundPair { pair, id_p, id_n })
}

// ------------------------------------------------------------------------------------------------
// the session
// ------------------------------------------------------------------------------------------------

/// A leg of the route that has been fixed: the two lines on one layer, and the two vias they end in (a leg that ends in vias is the
/// last on its layer).
#[derive(Debug, Clone)]
pub struct DpLeg {
    pub layer: i32,
    pub width: Nm,
    pub p: Vec<Point>,
    pub n: Vec<Point>,
    pub vias: Option<(Via, Via)>,
}

/// Everything a finished route puts on the board.
#[derive(Debug, Clone)]
pub struct DpCommit {
    pub net_p: Net,
    pub net_n: Net,
    pub legs: Vec<DpLeg>,
    /// The tracks a shove moved, by IR track id: their replacement lines (none: the track is removed).
    pub displaced_tracks: Vec<(String, Vec<Line>)>,
    /// The vias a shove moved, by IR via id.
    pub displaced_vias: Vec<(String, Point)>,
}

/// A via, as the preview draws it.
#[derive(Debug, Clone)]
pub struct PreviewVia {
    pub net: Net,
    pub pos: Point,
    pub diameter: Nm,
    pub drill: Nm,
    pub layers: LayerRange,
}

impl From<&Via> for PreviewVia {
    fn from(v: &Via) -> Self {
        PreviewVia { net: v.net.clone(), pos: v.pos, diameter: v.diameter, drill: v.drill, layers: v.layers }
    }
}

/// The live pair: what the last `Move` left, and what has been fixed so far.
#[derive(Debug, Clone)]
pub struct DiffPairPreview {
    pub layer: i32,
    pub width: Nm,
    pub gap: Nm,
    /// The fixed legs of P (`a`) and N (`b`), one line per leg, each on its own layer.
    pub runs_a: Vec<Line>,
    pub runs_b: Vec<Line>,
    /// The head: the two lines from where the fixed legs end to the cursor, walked or shoved per the mode.
    pub head_a: Line,
    pub head_b: Line,
    /// The pair does not fit as drawn (`!m_fitOk`): a line collides, or the mode could not resolve it.
    pub colliding: bool,
    /// Both lines end on a coupled pair of pads, vias or track ends (`m_snapOnTarget`): fixing here finishes the route.
    pub snapped_end: bool,
    /// The vias the head ends in, if a pair of vias is being placed (`V`): P's and N's.
    pub vias: Vec<PreviewVia>,
    /// The vias of the fixed legs.
    pub fixed_vias: Vec<PreviewVia>,
    /// `Mode::Shove`: the tracks and vias the head would push aside.
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
    pub placing_via: bool,
}

/// What `Start` and `Move` and `FixRoute` need to undo a fix: the state before it.
#[derive(Debug, Clone)]
struct Snapshot {
    world: Node,
    legs: usize,
    own_items: Vec<ItemId>,
    prev_pair: Option<DpPrimitivePair>,
    start: DpPrimitivePair,
    start_diagonal: bool,
    initial_diagonal: bool,
    current_layer: i32,
    displaced_tracks: BTreeMap<String, Vec<Line>>,
    displaced_vias: BTreeMap<String, Point>,
}

/// `PNS::DIFF_PAIR_PLACER`.
pub struct DiffPairPlacer {
    pub net_p: Net,
    pub net_n: Net,
    pub sizes: DpSizes,
    /// `m_start`, the pair the route began on -- or, after a layer change, the vias it went on from.
    start: DpPrimitivePair,
    /// `m_prevPair`: what the last fix ended on, which the next head starts from.
    prev_pair: Option<DpPrimitivePair>,
    /// The ids of the two items the route started on and of those it has fixed: a head does not end on them.
    start_items: Vec<ItemId>,
    own_items: Vec<ItemId>,
    /// All layers of the board, for an end item while a via is placed.
    all_layers: LayerRange,
    /// The session's copy of the world: the router's, with the legs fixed so far (`m_world` after `CommitPlacement`) and whatever the
    /// shoves of those legs moved.
    world: Node,
    legs: Vec<DpLeg>,
    history: Vec<Snapshot>,
    /// What shoves moved, by IR id, the latest position of each (as [`crate::line_placer::LinePlacer`] keeps them).
    displaced_tracks: BTreeMap<String, Vec<Line>>,
    displaced_vias: BTreeMap<String, Point>,
    /// `m_startDiagonal` / `m_initialDiagonal`: the posture of the head, and the one the next leg starts with.
    start_diagonal: bool,
    initial_diagonal: bool,
    /// `m_currentLayer`.
    current_layer: i32,
    /// `m_placingVia`, and the layer the route goes on to after the vias (`switchLayerOnViaPlacement`).
    placing_via: bool,
    via_to_layer: Option<i32>,
    // ---- what the last Move left
    /// `m_currentTrace`, `m_currentTraceOk`, `m_fitOk`, `m_snapOnTarget`.
    current_trace: DiffPair,
    current_trace_ok: bool,
    fit_ok: bool,
    snap_on_target: bool,
    current_end_item: Option<ItemId>,
    /// `m_currentNode` / `m_lastNode`: the world the last shove left, if it shoved.
    last_node: Option<Node>,
    shoved_lines: Vec<DisplacedLine>,
    shoved_vias: Vec<DisplacedVia>,
    /// `m_shove`'s springback stack (`m_nodeStack`): a frame per Move that shoved, over the world the last fix committed, and no locked
    /// frames (a fix makes a fresh `SHOVE`).
    springback: Springback,
    /// `m_idle`: the route is finished.
    pub idle: bool,
    pub placement_correct: bool,
}

impl DiffPairPlacer {
    /// `DIFF_PAIR_PLACER::Start`, and what `ROUTER::isStartingPointRoutable` asks of a pair first: the item under the cursor (`start_item`)
    /// and its coupled partner ([`find_dp_primitive_pair`]), the gap at the start of a pair of tracks matching the configured one, and
    /// the lines at the start not colliding with anything (at the board's minimum width if not at the pair's own). `layer` is the layer
    /// the route starts on.
    pub fn start(world: &Node, rules: &BoardRules, settings: &RoutingSettings, start_item: ItemId, layer: i32, all_layers: LayerRange) -> Result<Self, String> {
        let found = find_dp_primitive_pair(world, start_item)?;
        let (Some(prim_p), Some(prim_n)) = (found.pair.prim_p.clone(), found.pair.prim_n.clone()) else { return Err(NO_COMPLEMENT.into()) };
        let (net_p, net_n) = (prim_p.net().clone(), prim_n.net().clone());
        let (name_p, name_n) = (net_p.as_deref().unwrap_or(""), net_n.as_deref().unwrap_or(""));
        let mut sizes = DpSizes::resolve(rules, name_p, name_n);

        // `isStartingPointRoutable` (`if( Settings().AllowDRCViolations() ) return true;` first)
        if !settings.allow_drc_violations() {
            if sizes.gap < rules.min_clearance_nm {
                return Err("Diff pair gap is less than board minimum clearance.".into());
            }
        }
        // starting from tracks: the width they have (`inheritTrackWidth`)
        if let (Item::Segment(sp), Item::Segment(_)) = (&prim_p, &prim_n) {
            sizes.width = sp.width;
        }
        if !settings.allow_drc_violations() {
            // ... and the gap they are at has to be the configured one (to 10 %)
            if let (Item::Segment(_), Item::Segment(_)) = (&prim_p, &prim_n) {
                let actual_gap = vnorm(vsub(found.pair.anchor_p, found.pair.anchor_n));
                let configured = sizes.gap + sizes.width;
                if (actual_gap - configured).abs() > configured / 10 {
                    return Err("The differential pair gap at the start point does not match the configured gap. This can occur in neckdown areas where tracks have narrower width and spacing. Adjust the differential pair settings or start from a location with the correct gap.".into());
                }
            }
            // the two lines, as dots, collide with nothing: "If the only reason we collide is track width; it's better to allow the user to
            // start anyway and just highlight the resulting collisions, so they can change width later."
            let dummy_collides = |width: Nm| {
                [(found.pair.anchor_n, &net_n), (found.pair.anchor_p, &net_p)].iter().any(|(at, net)| {
                    let pts = [*at, *at];
                    PairLine { pts: &pts, net: *net, layer, width, via: None }.hits_node(world, rules, kind_mask::ANY)
                })
            };
            if dummy_collides(sizes.width) && dummy_collides(rules.track_width_min_nm) {
                return Err("The routing start point violates DRC.".into());
            }
        }

        Ok(DiffPairPlacer {
            net_p: net_p.clone(),
            net_n: net_n.clone(),
            sizes,
            start: found.pair,
            prev_pair: None,
            start_items: vec![found.id_p, found.id_n],
            own_items: Vec::new(),
            all_layers,
            world: world.branch(),
            legs: Vec::new(),
            history: Vec::new(),
            displaced_tracks: BTreeMap::new(),
            displaced_vias: BTreeMap::new(),
            start_diagonal: false,
            initial_diagonal: false,
            current_layer: layer,
            placing_via: false,
            via_to_layer: None,
            current_trace: DiffPair { net_p, net_n, ..DiffPair::default() },
            current_trace_ok: false,
            fit_ok: false,
            snap_on_target: false,
            current_end_item: None,
            last_node: None,
            shoved_lines: Vec::new(),
            shoved_vias: Vec::new(),
            springback: Springback::new(),
            idle: false,
            placement_correct: false,
        })
    }

    pub fn current_layer(&self) -> i32 {
        self.current_layer
    }

    pub fn is_placing_via(&self) -> bool {
        self.placing_via
    }

    /// `UpdateSizes` from the Differential Pair Dimensions dialog: the width, the gap and the via gap of the next `Move`.
    pub fn set_dimensions(&mut self, width: Option<Nm>, gap: Option<Nm>, via_gap: Option<Nm>) {
        if let Some(w) = width.filter(|w| *w > Nm::ZERO) {
            self.sizes.width = w;
        }
        if let Some(g) = gap.filter(|g| *g > Nm::ZERO) {
            self.sizes.gap = g;
        }
        if let Some(g) = via_gap.filter(|g| *g > Nm::ZERO) {
            self.sizes.via_gap = g;
        }
        self.current_trace.width = self.sizes.width;
        self.current_trace.set_gap(self.sizes.gap);
    }

    /// `DIFF_PAIR_PLACER::FlipPosture`.
    pub fn flip_posture(&mut self) {
        self.start_diagonal = !self.start_diagonal;
    }

    /// `DIFF_PAIR_PLACER::ToggleVia`: the head ends in a pair of vias (or stops doing so). `to_layer` is the layer the route goes on to
    /// after them; without one, the other end of the board.
    pub fn toggle_via(&mut self, enabled: bool, diameter: Option<Nm>, drill: Option<Nm>, to_layer: Option<i32>) {
        self.placing_via = enabled;
        if enabled {
            if let Some(d) = diameter.filter(|d| *d > Nm::ZERO) {
                self.sizes.via_diameter = d;
            }
            if let Some(d) = drill.filter(|d| *d > Nm::ZERO) {
                self.sizes.via_drill = d;
            }
            self.via_to_layer = to_layer.or_else(|| Some(if self.current_layer == self.all_layers.start() { self.all_layers.end() } else { self.all_layers.start() }));
        } else {
            self.via_to_layer = None;
        }
    }

    /// `makeVia`: a via of the pair's size on the layers between the current one and the one the route goes on to.
    fn make_via(&self, pos: Point, net: &Net) -> Via {
        let to = self.via_to_layer.unwrap_or(self.current_layer);
        Via { net: net.clone(), layers: LayerRange::new(self.current_layer, to), pos, diameter: self.sizes.via_diameter, drill: self.sizes.via_drill, source_via: None, locked: false, is_virtual: false }
    }

    /// The end item of a cursor (`TOOL_BASE::updateEndItem`, `pickSingleItem` for each net of the pair): a pad or via of either net near
    /// it, else a track end, on the layer being routed (any while vias are placed). Not the items the route started on or has fixed.
    fn pick_end_item(&self, at: Point) -> Option<ItemId> {
        let layers = if self.placing_via { self.all_layers } else { LayerRange::single(self.current_layer) };
        let own: HashSet<ItemId> = self.start_items.iter().chain(self.own_items.iter()).copied().collect();
        let mut best: Option<((u8, Nm, ItemId), ItemId)> = None;
        for (id, item) in self.world.iter() {
            if own.contains(&id) || !item.layers().overlaps(&layers) {
                continue;
            }
            if !(same_net(item.net(), &self.net_p) || same_net(item.net(), &self.net_n)) {
                continue;
            }
            // pads and vias take priority over tracks (`prioritized[]`); then the nearest
            let class = if matches!(item, Item::Segment(_)) { 1u8 } else { 0 };
            let dist = item.anchors().iter().map(|a| vnorm(vsub(*a, at))).min();
            let Some(dist) = dist.filter(|d| *d <= SNAP_NM) else { continue };
            let key = (class, dist, id);
            if best.as_ref().is_none_or(|(k, _)| key < *k) {
                best = Some((key, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// `DIFF_PAIR_PLACER::propagateDpHeadForces`: the cursor, pushed off what the circle round the end of the pair (or of its vias) would
    /// sit on, so that the end gateways are where there is room. `None` when the cursor cannot be moved clear. Not in Highlight
    /// collisions mode; in Shove mode off pads alone (the rest is the shove's to move). `node` is `m_currentNode`: in Shove mode the
    /// springback frame the move starts from.
    fn propagate_dp_head_forces(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Option<Point> {
        let mut virt = if self.placing_via {
            let mut v = self.make_via(p, &None);
            v.diameter = self.sizes.via_gap_total() + 2 * v.diameter;
            v
        } else {
            let mut v = self.make_via(p, &None);
            v.layers = LayerRange::single(self.current_layer);
            v.diameter = self.sizes.gap + 2 * self.sizes.width;
            v
        };
        let mask = match settings.mode {
            Mode::MarkObstacles => return Some(p),
            Mode::Walkaround => kind_mask::ANY,
            Mode::Shove => kind_mask::SOLID,
        };

        // "Note: this code is lifted from VIA::PushoutForce and then optimized for this use case and to check proper clearances to the
        // diff pair line."
        const MAX_ITER: usize = 40;
        let mut iter = 0;
        let mut collided = false;
        let mut force: crate::diff_pair::Vec2 = (Nm::ZERO, Nm::ZERO);
        let mut handled: HashSet<ItemId> = HashSet::new();
        let norm_sq = |v: crate::diff_pair::Vec2| v.0.area(v.0) + v.1.area(v.1);
        while iter < MAX_ITER {
            // `m_currentNode->QueryColliding( &virtHead, ..)`: its copper and its hole
            let opts = QueryOpts { kind_mask: mask, filter: None, use_epsilon: true, box_hull: false, head_hole: None };
            let mut obstacles = node.via_colliding(&virt, rules, &[], opts);
            obstacles.sort_by_key(|o| o.id);
            let Some(obs) = obstacles.first() else { break };
            if handled.contains(&obs.id) {
                break;
            }
            let Some(item) = node.get(obs.id) else { break };
            // `m_currentNode->GetClearance( obs->m_item, &m_currentTrace.PLine(), false )`
            let clearance = node.clearance_to(rules, crate::node::Side::track(&self.net_p, self.current_layer), item);
            let mut layer_force = (Nm::ZERO, Nm::ZERO);
            collided = false;
            // `RelevantShapeLayers`: the layers both occupy (shapes are the same on every layer here)
            if item.layers().overlaps(&virt.layers) {
                if let Some(f) = crate::shove::item_circle_mtv(item, virt.pos, virt.diameter / 2, clearance) {
                    collided = true;
                    layer_force = f;
                }
                if norm_sq(layer_force) > norm_sq(force) {
                    force = layer_force;
                }
            }
            if collided {
                virt.pos = Point { x: virt.pos.x + force.0, y: virt.pos.y + force.1 };
            }
            handled.insert(obs.id);
            iter += 1;
        }
        let succeeded = !collided || iter != MAX_ITER;
        succeeded.then_some(Point { x: p.x + force.0, y: p.y + force.1 })
    }

    /// `DIFF_PAIR_PLACER::routeHead`: fit the gateways of where the pair is coming from to those of where it is going -- the coupled
    /// pair of pads, vias or track ends under the cursor, or the cursor -- and take the best pair of lines between them. `false` only
    /// when nothing fits and nothing ever did. `node` is `m_currentNode`, where the end item and the pushout forces are looked for:
    /// `Some` for a springback frame (Shove mode), `None` for the session's world (the other modes).
    fn route_head(&mut self, node: Option<&Node>, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        let world = node.unwrap_or(&self.world);
        self.fit_ok = false;
        let pitch = self.sizes.pitch();
        let mut gws_entry = DpGateways::new(pitch);
        let mut gws_target = DpGateways::new(pitch);
        if self.prev_pair.is_none() {
            self.prev_pair = Some(self.start.clone());
        }
        let prev = self.prev_pair.clone().expect("set above");
        gws_entry.build_from_primitive_pair(&prev, self.start_diagonal);

        let target = self.current_end_item.and_then(|id| find_dp_primitive_pair(world, id).ok());
        if let Some(target) = target {
            gws_target.build_from_primitive_pair(&target.pair, self.start_diagonal);
            self.snap_on_target = true;
        } else {
            let Some(fp) = self.propagate_dp_head_forces(world, rules, settings, p) else { return false };
            let (midp, dir_v) = prev.cursor_orientation(fp);
            let fp_proj = line_project((midp, Point { x: midp.x + dir_v.0, y: midp.y + dir_v.1 }), fp);
            // "compute 'leader point' distance from the cursor (project cursor position on the extension of the starting segment pair
            // of the DP)"
            let lead_dist = vnorm(vsub(fp_proj, fp));
            gws_target.set_fit_vias(self.placing_via, self.sizes.via_diameter, Some(self.sizes.via_gap_total()));
            if lead_dist > (self.sizes.gap + self.sizes.width) / 2 {
                // "far from the initial segment extension line -> allow a 45-degree obtuse turn"
                gws_target.build_for_cursor(fp);
            } else {
                // "close to the initial segment extension line -> keep straight part only, project as close as possible to the cursor."
                gws_target.build_for_cursor(fp_proj);
                gws_target.filter_by_orientation(ANG_STRAIGHT | ANG_HALF_FULL, direction_of(dir_v));
            }
            self.snap_on_target = false;
        }

        self.current_trace.set_gap(pitch);
        self.current_trace.layer = self.current_layer;
        let result = gws_entry.fit_gateways(&gws_target, self.start_diagonal, &mut self.current_trace);
        if result {
            self.current_trace_ok = true;
            self.current_trace.set_nets(self.net_p.clone(), self.net_n.clone());
            self.current_trace.width = self.sizes.width;
            self.current_trace.set_gap(self.sizes.gap);
            if self.placing_via {
                let last_p = *self.current_trace.p.last().expect("a fitted pair has points");
                let last_n = *self.current_trace.n.last().expect("a fitted pair has points");
                let (via_p, via_n) = (self.make_via(last_p, &self.net_p), self.make_via(last_n, &self.net_n));
                self.current_trace.append_vias(via_p, via_n);
            } else {
                self.current_trace.remove_vias();
            }
            return true;
        }
        self.current_trace_ok && !self.current_trace.is_empty()
    }

    // ---------------------------------------------------------------- the modes

    /// `rhMarkObstacles`: the pair as fitted; it does not fit if either line collides.
    fn rh_mark_obstacles(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        if !self.route_head(None, rules, settings, p) {
            return false;
        }
        let hit_p = self.current_trace.p_pair_line().hits_node(&self.world, rules, kind_mask::ANY);
        let hit_n = self.current_trace.n_pair_line().hits_node(&self.world, rules, kind_mask::ANY);
        self.fit_ok = !(hit_p || hit_n);
        self.fit_ok
    }

    /// `rhWalkOnly`: the pair walked around everything in its way.
    fn rh_walk_only(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        if !self.route_head(None, rules, settings, p) {
            return false;
        }
        let mut pair = self.current_trace.clone();
        self.fit_ok = self.try_walk_dp(&self.world, rules, settings, &mut pair, false);
        if self.fit_ok {
            self.current_trace = pair;
        }
        self.fit_ok
    }

    /// `DIFF_PAIR_PLACER::CurrentNode` for this Move: the newest springback frame's world against the session's (and the legs fixed in
    /// it), or that world itself when no shove of this leg has pushed a frame.
    fn frame_node(&self) -> Node {
        let (frame, _) = self.springback.top();
        if frame.tracks.is_empty() && frame.vias.is_empty() {
            return self.world.clone();
        }
        frame.apply(&self.world)
    }

    /// `rhShoveOnly`: the pair walked around the pads, then both lines are heads of one shove that moves the tracks and vias in the way.
    /// The move starts from the springback frame the new heads no longer press on (`m_currentNode = m_shove->CurrentNode()`, which
    /// `SHOVE::Run`'s `reduceSpringback` leaves as the newest frame they do), and the world its shove leaves is the frame `FixRoute`
    /// commits.
    fn rh_shove_only(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        // `m_currentNode = m_shove->CurrentNode()` at the top of `rhShoveOnly`, and `Move`'s `m_lastNode = m_currentNode->Branch()`
        let frame = self.frame_node();
        self.last_node = Some(frame.clone());
        // what is shown when the head cannot be routed (`routeHead` fails: `propagateDpHeadForces` runs out of steps; `tryWalkDp` always
        // returns true): `Move`'s `m_lastNode = latestNode->Branch()` of that frame, whatever `route()` returned, so the tracks and vias
        // the newest frame pushed stay pushed on screen (round-2 R-6)
        let (top, _) = self.springback.top();
        self.shoved_lines = top.displaced_lines(&self.world);
        self.shoved_vias = top.displaced_vias();
        let ok = self.route_head(Some(&frame), rules, settings, p);
        self.fit_ok = false;
        if !ok {
            return false;
        }
        let mut pair = self.current_trace.clone();
        if !self.try_walk_dp(&frame, rules, settings, &mut pair, true) {
            return false;
        }
        self.current_trace = pair;
        let (p_line, n_line) = (self.current_trace.p_line(), self.current_trace.n_line());
        let heads = vec![
            ShoveHead { line: p_line.clone(), via: self.current_trace.via_p.clone() },
            ShoveHead { line: n_line.clone(), via: self.current_trace.via_n.clone() },
        ];
        // `SHOVE::Run`: "Pop NODEs containing previous shoves which are no longer necessary" (`reduceSpringback( aHeadSet )`), the head
        // set holding both lines -- a frame one of them still presses on stays for the other too
        let head_sets = [
            HeadSet { pts: &self.current_trace.p, net: &self.net_p, layer: self.current_layer, width: self.sizes.width, via: self.current_trace.via_p.as_ref() },
            HeadSet { pts: &self.current_trace.n, net: &self.net_n, layer: self.current_layer, width: self.sizes.width, via: self.current_trace.via_n.as_ref() },
        ];
        let (parent, prior_area) = self.springback.reduce_all(&self.world, &head_sets, rules);
        let mut shove_node = parent.apply(&self.world);
        // `SHOVE::Run`: "m_currentNode->LockJoint( head.CPoint( 0 ), &head, true ); if( !head.EndsWithVia() ) m_currentNode->LockJoint(
        // head.CLastPoint(), &head, true )" -- a line of either head's net the shove pushes (the route's own tail) ends there
        for (line, via) in [(&p_line, &self.current_trace.via_p), (&n_line, &self.current_trace.via_n)] {
            if let Some(&start) = line.pts.first() {
                shove_node.lock_joint(start, &line.net);
            }
            if let (None, Some(&end)) = (via, line.pts.last()) {
                shove_node.lock_joint(end, &line.net);
            }
        }
        let mut world = parent;
        match crate::shove::shove_heads_after(&shove_node, heads, rules, settings, prior_area) {
            Some(out) => {
                // "Update m_currentTrace with the shoved shapes so FixRoute() commits correct geometry"
                let (hp, hn) = (out.heads[0].clone(), out.heads[1].clone());
                self.current_trace.set_shape(hp, hn, false);
                let (hit_p, hit_n) = (self.current_trace.p_pair_line().hits_node(&out.world, rules, kind_mask::ANY), self.current_trace.n_pair_line().hits_node(&out.world, rules, kind_mask::ANY));
                world.merge_shove(&out.displaced_lines, &out.displaced_vias);
                // `pushSpringback( m_currentNode, m_affectedArea )`: the world this shove left is the newest frame
                self.springback.push(&self.world, world.clone(), out.area);
                self.last_node = Some(out.world);
                if !hit_p && !hit_n {
                    self.fit_ok = true;
                }
            }
            // "bring back previous state": `m_currentNode = parent`, the frame the reduce left, and `m_lastNode` branches from it
            None => self.last_node = Some(shove_node),
        }
        // what the frames have moved, as the preview draws it: the world the shove left, or the one the reduce left on failure
        self.shoved_lines = world.displaced_lines(&self.world);
        self.shoved_vias = world.displaced_vias();
        self.fit_ok
    }

    /// `DIFF_PAIR_PLACER::route`.
    fn route(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        match settings.mode {
            Mode::MarkObstacles => self.rh_mark_obstacles(rules, settings, p),
            Mode::Walkaround => self.rh_walk_only(rules, settings, p),
            Mode::Shove => self.rh_shove_only(rules, settings, p),
        }
    }

    /// `DIFF_PAIR_PLACER::Move`: re-route the head to the cursor at `p`.
    pub fn move_to(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> bool {
        if self.idle {
            return false;
        }
        self.current_end_item = self.pick_end_item(p);
        self.fit_ok = false;
        self.last_node = None;
        self.shoved_lines.clear();
        self.shoved_vias.clear();
        self.route(rules, settings, p)
    }

    // ---------------------------------------------------------------- walking a pair

    /// `walkaround.Route( line )` for one line of the pair: the shortest way around everything of the kinds in `mask`, or `None` if there
    /// is none (`ST_DONE` is the only success).
    fn walk_line(node: &Node, rules: &BoardRules, settings: &RoutingSettings, line: PairLine, mask: u8) -> Option<Vec<Point>> {
        if line.pts.len() < 2 {
            return None;
        }
        let mut walker = Walker::new(node, rules);
        walker.corner_mode = settings.corner_mode;
        walker.item_mask = mask;
        walker.iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
        walker.set_allowed_policies(&[WalkPolicy::Shortest]);
        walker.set_end_via(line.via.cloned());
        let out = walker.route(line.net, line.layer, line.width, line.pts);
        (out.status_of(WalkPolicy::Shortest) == WalkStatus::Done).then(|| out.line_of(WalkPolicy::Shortest).to_vec())
    }

    /// `DIFF_PAIR_PLACER::attemptWalk`: walk one line of the pair around what it collides with, shove the other to the gap from it, and
    /// if that one now collides walk it and shove the first -- up to three rounds. `None` if a walk or a shove fails or the rounds run out.
    fn attempt_walk(node: &Node, rules: &BoardRules, settings: &RoutingSettings, current: &DiffPair, p_first: bool, solids_only: bool) -> Option<DiffPair> {
        let mask = if solids_only { kind_mask::SOLID } else { kind_mask::ANY };
        let mut cur = current.clone();
        let mut current_is_p = p_first;
        let mut iter = 0;
        loop {
            // the line to walk (`preWalk`) and the one to shove (`preShove`), with what they are made of
            let (walk_net, shove_net) = if current_is_p { (cur.net_p.clone(), cur.net_n.clone()) } else { (cur.net_n.clone(), cur.net_p.clone()) };
            let (walk_via, shove_via) = if current_is_p { (cur.via_p.clone(), cur.via_n.clone()) } else { (cur.via_n.clone(), cur.via_p.clone()) };
            let (walk_pts, shove_pts) = if current_is_p { (cur.p.clone(), cur.n.clone()) } else { (cur.n.clone(), cur.p.clone()) };
            let pre_walk = PairLine { pts: &walk_pts, net: &walk_net, layer: cur.layer, width: cur.width, via: walk_via.as_ref() };
            let pre_shove = PairLine { pts: &shove_pts, net: &shove_net, layer: cur.layer, width: cur.width, via: shove_via.as_ref() };
            if !pre_walk.hits_node(node, rules, mask) {
                current_is_p = !current_is_p;
                if !pre_shove.hits_node(node, rules, mask) {
                    break;
                }
            } else {
                let post_walk_pts = Self::walk_line(node, rules, settings, pre_walk, mask)?;
                let post_walk = Line::from_points(walk_net.clone(), cur.layer, cur.width, post_walk_pts);
                let pre_shove_line = Line::from_points(shove_net.clone(), cur.layer, cur.width, shove_pts.clone());
                // `shove.ForceClearance( true, cur.Gap() - 2 * PNS_HULL_MARGIN )`: the margin (`PNS_HULL_MARGIN` 10 nm, `pns_line.h`) takes a hair off
                // the gap so that a line already at the gap is not pushed
                let forced = cur.gap - 2 * crate::hull::PNS_HULL_MARGIN;
                let post_shove = crate::shove::shove_obstacle_line(node, rules, settings, &post_walk, walk_via.as_ref(), &pre_shove_line, shove_via.as_ref(), Some(forced))?;
                let (mut post_walk, mut post_shove) = (post_walk, post_shove);
                post_walk.simplify();
                post_shove.simplify();
                let swap = !current_is_p;
                cur.set_shape(post_walk.pts.clone(), post_shove.pts.clone(), swap);
                current_is_p = !current_is_p;
                let shoved_hits = PairLine { pts: &post_shove.pts, net: &shove_net, layer: cur.layer, width: cur.width, via: shove_via.as_ref() }.hits_node(node, rules, mask);
                if !shoved_hits {
                    break;
                }
                iter += 1;
            }
            if iter >= 3 {
                break;
            }
        }
        if iter == 3 {
            return None;
        }
        let mut walked = current.clone();
        walked.set_shape(cur.p, cur.n, false);
        Some(walked)
    }

    /// `DIFF_PAIR_PLACER::tryWalkDp`: the better of the walks starting with P and with N -- the lowest `1 + coupled length + 3 |skew|`
    /// (`double score = cl + fabs( skew ) * 3.0; if( score < bestScore )`), so the *shortest* coupled length -- then optimized ([`crate::optimizer::optimize_pair`]). When neither walk works, KiCad's `if( bestScore > 0.0 )` is true all the same (the score
    /// starts at 1e14 and nothing lowers it): `aPair.SetShape( best )` takes the default `DIFF_PAIR` -- two empty lines -- and the function says `true`, so
    /// a pair that cannot be walked disappears and has no head to show. (Walk around then calls that a fit, and fixing it places nothing; Shove runs
    /// a shove with two empty heads, which fails.) The function never returns `false`; the result is kept for the callers' shape.
    fn try_walk_dp(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, pair: &mut DiffPair, solids_only: bool) -> bool {
        let mut best: Option<DiffPair> = None;
        let mut best_score = 100_000_000_000_000.0f64;
        // KiCad tries four attempts, `pfirst = attempt & 1` and a winding it never reads: the third and fourth repeat the first two
        for p_first in [false, true] {
            if let Some(p) = Self::attempt_walk(node, rules, settings, pair, p_first, solids_only) {
                let cl = 1.0 + p.coupled_length();
                let skew = p.skew();
                let score = cl + skew.abs() * 3.0;
                if score < best_score {
                    best_score = score;
                    best = Some(p);
                }
            }
        }
        // "DIFF_PAIR best;" -- empty, unless an attempt beat the score
        let best = best.unwrap_or_default();
        pair.set_shape_from(&best);
        crate::optimizer::optimize_pair(node, rules, pair);
        true
    }

    // ---------------------------------------------------------------- fixing

    /// What `Move` left, for the UI.
    pub fn preview(&self) -> DiffPairPreview {
        let line_of = |pts: &[Point], net: &Net| Line::from_points(net.clone(), self.current_layer, self.sizes.width, pts.to_vec());
        let mut vias = Vec::new();
        if let (Some(vp), Some(vn)) = (&self.current_trace.via_p, &self.current_trace.via_n) {
            vias = vec![PreviewVia::from(vp), PreviewVia::from(vn)];
        }
        let fixed_vias = self.legs.iter().filter_map(|l| l.vias.as_ref()).flat_map(|(a, b)| [PreviewVia::from(a), PreviewVia::from(b)]).collect();
        let leg_lines = |p: bool| self.legs.iter().map(|l| Line::from_points(if p { self.net_p.clone() } else { self.net_n.clone() }, l.layer, l.width, if p { l.p.clone() } else { l.n.clone() })).collect::<Vec<_>>();
        // what the head shoves now, and what the legs fixed so far shoved (a fix puts the shoved track in the world, where it stays pushed
        // until the commit): the head's answer wins for a track both have moved
        let mut displaced_lines = self.shoved_lines.clone();
        let moving: HashSet<&str> = self.shoved_lines.iter().filter_map(|d| d.source_track.as_deref()).collect();
        for (id, lines) in &self.displaced_tracks {
            if !moving.contains(id.as_str()) {
                displaced_lines.extend(lines.iter().filter(|l| l.point_count() >= 2).map(|l| DisplacedLine { source_track: Some(id.clone()), line: l.clone() }));
            }
        }
        let mut displaced_vias = self.shoved_vias.clone();
        for (id, pos) in &self.displaced_vias {
            if !self.shoved_vias.iter().any(|d| &d.source_via == id) {
                displaced_vias.push(DisplacedVia { source_via: id.clone(), pos: *pos });
            }
        }
        DiffPairPreview {
            layer: self.current_layer,
            width: self.sizes.width,
            gap: self.sizes.gap,
            runs_a: leg_lines(true),
            runs_b: leg_lines(false),
            head_a: line_of(&self.current_trace.p, &self.net_p),
            head_b: line_of(&self.current_trace.n, &self.net_n),
            colliding: !self.fit_ok,
            snapped_end: self.snap_on_target,
            vias,
            fixed_vias,
            displaced_lines,
            displaced_vias,
            placing_via: self.placing_via,
        }
    }

    /// The tracks and vias a shove moved, merged into what the route has displaced so far: the latest position of each wins.
    fn absorb_displacement(&mut self) {
        let mut fresh: BTreeMap<String, Vec<Line>> = BTreeMap::new();
        for d in &self.shoved_lines {
            if let Some(id) = &d.source_track {
                let e = fresh.entry(id.clone()).or_default();
                if d.line.point_count() >= 2 {
                    e.push(d.line.clone());
                }
            }
        }
        for (id, ls) in fresh {
            self.displaced_tracks.insert(id, ls);
        }
        for d in &self.shoved_vias {
            self.displaced_vias.insert(d.source_via.clone(), d.pos);
        }
    }

    /// `DIFF_PAIR_PLACER::FixRoute( aP, aEndItem, aForceFinish )`: the head becomes a leg of the route. Not when it does not fit (and
    /// violations are not allowed) or has no segments: `None`. A pair that ended on a coupled pair of pads or vias, or `force_finish`,
    /// is finished; otherwise the next leg goes on from where this ended (`EndingPrimitives`), and with vias on the other layer.
    pub fn fix_route(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point, force_finish: bool) -> Option<FixOutcome> {
        if self.idle {
            return None;
        }
        self.move_to(rules, settings, p);
        self.fix_head(settings, force_finish)
    }

    /// [`Self::fix_route`] for the head the last `Move` left.
    fn fix_head(&mut self, settings: &RoutingSettings, force_finish: bool) -> Option<FixOutcome> {
        if !self.fit_ok && !settings.allow_drc_violations() {
            return None;
        }
        if crate::diff_pair::segment_count(&self.current_trace.p) < 1 || crate::diff_pair::segment_count(&self.current_trace.n) < 1 {
            return None;
        }
        let was = Snapshot {
            world: self.world.clone(),
            legs: self.legs.len(),
            own_items: self.own_items.clone(),
            prev_pair: self.prev_pair.clone(),
            start: self.start.clone(),
            start_diagonal: self.start_diagonal,
            initial_diagonal: self.initial_diagonal,
            current_layer: self.current_layer,
            displaced_tracks: self.displaced_tracks.clone(),
            displaced_vias: self.displaced_vias.clone(),
        };
        if crate::diff_pair::segment_count(&self.current_trace.p) > 1 {
            let t = &self.current_trace.p;
            self.initial_diagonal = !Direction45::from_seg(t[t.len() - 3], t[t.len() - 2]).is_diagonal();
        }
        // "avoid fixing the last segment" unless the route ends here
        if !self.snap_on_target && !self.current_trace.ends_with_vias() && !force_finish && !settings.fix_all_segments {
            let (mut new_p, mut new_n) = (self.current_trace.p.clone(), self.current_trace.n.clone());
            if new_p.len() > 2 && new_n.len() > 2 {
                new_p.pop();
                new_n.pop();
            }
            self.current_trace.set_shape(new_p, new_n, false);
        }

        // the world the head left (shoved, or as it was), with the pair in it
        let mut node = self.last_node.take().unwrap_or_else(|| self.world.clone());
        let trace = self.current_trace.clone();
        let vias = match (&trace.via_p, &trace.via_n) {
            (Some(a), Some(b)) => Some((a.clone(), b.clone())),
            _ => None,
        };
        if let Some((a, b)) = &vias {
            self.own_items.push(node.add(Item::Via(a.clone())));
            self.own_items.push(node.add(Item::Via(b.clone())));
        }
        self.own_items.extend(node.add_line(&trace.p_line(), None, false));
        self.own_items.extend(node.add_line(&trace.n_line(), None, false));
        self.world = node;
        self.absorb_displacement();
        self.shoved_lines.clear();
        self.shoved_vias.clear();
        // `m_shove = std::make_unique<SHOVE>( m_world, Router() )`: the stack restarts empty over the world the fix commits
        self.springback = Springback::new();
        self.history.push(was);

        self.legs.push(DpLeg { layer: self.current_layer, width: trace.width, p: trace.p.clone(), n: trace.n.clone(), vias: vias.clone() });
        let ending = trace.ending_primitives();
        self.prev_pair = Some(ending);
        self.placement_correct = true;

        let real_end = self.snap_on_target;
        if let Some((a, _)) = &vias {
            // `switchLayerOnViaPlacement` -> `SetLayer`: on from the vias, on the other layer
            let _ = a;
            self.current_layer = self.via_to_layer.unwrap_or(self.current_layer);
            self.start = self.prev_pair.clone().expect("just set");
            self.placing_via = false;
            self.via_to_layer = None;
        }
        // `initPlacement`
        self.start_diagonal = self.initial_diagonal;
        self.current_end_item = None;
        self.current_trace.remove_vias();
        self.current_trace.p.clear();
        self.current_trace.n.clear();
        self.snap_on_target = false;
        self.fit_ok = false;
        if real_end || force_finish {
            self.idle = true;
        }
        Some(FixOutcome::Fixed { real_end })
    }

    /// `FixRoute( .., aForceFinish = true )` and `CommitPlacement`: fix the head and finish the route, or just finish it if the last fix
    /// already did. `None` if the head does not fit.
    pub fn finish(&mut self, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Option<DpCommit> {
        if !self.idle {
            // a head with nothing in it is not a leg (finishing right after the start places nothing)
            self.move_to(rules, settings, p);
            let has_head = crate::diff_pair::segment_count(&self.current_trace.p) >= 1 && crate::diff_pair::segment_count(&self.current_trace.n) >= 1;
            if has_head {
                self.fix_head(settings, true)?;
            } else {
                self.idle = true;
            }
        }
        Some(self.commit())
    }

    /// Everything the route has fixed, to put on the board.
    pub fn commit(&self) -> DpCommit {
        DpCommit {
            net_p: self.net_p.clone(),
            net_n: self.net_n.clone(),
            legs: self.legs.clone(),
            displaced_tracks: self.displaced_tracks.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            displaced_vias: self.displaced_vias.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        }
    }

    /// Backspace: take back the last fixed leg (see this module's header).
    pub fn undo_last_segment(&mut self) -> bool {
        let Some(was) = self.history.pop() else { return false };
        self.world = was.world;
        self.legs.truncate(was.legs);
        self.own_items = was.own_items;
        self.prev_pair = was.prev_pair;
        self.start = was.start;
        self.start_diagonal = was.start_diagonal;
        self.initial_diagonal = was.initial_diagonal;
        self.current_layer = was.current_layer;
        self.displaced_tracks = was.displaced_tracks;
        self.displaced_vias = was.displaced_vias;
        self.idle = false;
        self.placing_via = false;
        self.via_to_layer = None;
        self.current_trace.p.clear();
        self.current_trace.n.clear();
        self.current_trace.remove_vias();
        self.last_node = None;
        // the frames of the undone leg's moves go with it: the fix that committed it had started the stack afresh
        self.springback = Springback::new();
        true
    }

    /// `DIFF_PAIR_PLACER::CurrentStart`: where the next leg starts on the P line -- the anchor of the pair the last fix ended on, or the pair the route
    /// started on.
    pub fn current_start_p(&self) -> Point {
        self.prev_pair.as_ref().unwrap_or(&self.start).anchor_p
    }

    /// The legs fixed so far.
    pub fn legs(&self) -> &[DpLeg] {
        &self.legs
    }

    /// The session's world, for tests: the router's with the legs in it.
    pub fn world(&self) -> &Node {
        &self.world
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_drc::kimath::Shape;
    use crate::item::{Segment, Solid};

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    /// Clearance 200, a USB class with a 200-wide pair at a gap of 250 (pitch 450).
    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\nnet_classes:\n  - name: usb\n    nets: [\"USB_*\"]\n    diff_pair_width: 200000\n    diff_pair_gap: 250000\n").unwrap()
    }

    #[allow(dead_code)]
    fn pad(node: &mut Node, net: &str, at: Point, r: Nm) -> ItemId {
        node.add(Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: at, shape: Shape::Circle { c: at, r }, source: format!("{net}@{},{}", at.x, at.y), edge: false, rule: 0, anchor_points: Vec::new() }))
    }

    fn smd_pad(node: &mut Node, net: &str, at: Point, r: Nm) -> ItemId {
        node.add(Item::Solid(Solid { net: net_of(net), layers: LayerRange::single(0), pos: at, shape: Shape::Circle { c: at, r }, source: format!("{net}@{},{}", at.x, at.y), edge: false, rule: 0, anchor_points: Vec::new() }))
    }

    fn track(node: &mut Node, net: &str, a: Point, b: Point, width: Nm) -> ItemId {
        node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width, source_track: Some((format!("trk_{net}"), 0)), locked: false }))
    }

    fn mode(m: Mode) -> RoutingSettings {
        RoutingSettings { mode: m, ..RoutingSettings::default() }
    }

    /// Two SMD pad pairs 10 mm apart: USB_DP/USB_DN at x = 0 and x = 10000, 1000 apart in y.
    fn two_pad_pairs() -> (Node, ItemId) {
        let mut node = Node::new();
        let start = smd_pad(&mut node, "USB_DP", p(Nm::ZERO, Nm::ZERO), mm(0.25));
        smd_pad(&mut node, "USB_DN", p(Nm::ZERO, mm(1.0)), mm(0.25));
        smd_pad(&mut node, "USB_DP", p(mm(10.0), Nm::ZERO), mm(0.25));
        smd_pad(&mut node, "USB_DN", p(mm(10.0), mm(1.0)), mm(0.25));
        (node, start)
    }

    fn placer(node: &Node, start: ItemId) -> DiffPairPlacer {
        DiffPairPlacer::start(node, &rules(), &RoutingSettings::default(), start, 0, LayerRange::new(0, 1)).expect("starts")
    }

    #[test]
    fn the_pair_starts_on_the_item_and_its_coupled_partner_whichever_net_was_clicked() {
        let (node, dp_pad) = two_pad_pairs();
        let found = find_dp_primitive_pair(&node, dp_pad).unwrap();
        assert_eq!((found.pair.anchor_p, found.pair.anchor_n), (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(1.0))), "P is the net named P, N the one named N");
        // clicking the N pad gives the same pair, P first
        let dn_pad = node.iter().find(|(_, it)| matches!(it, Item::Solid(s) if s.pos == p(Nm::ZERO, mm(1.0)))).map(|(id, _)| id).unwrap();
        let again = find_dp_primitive_pair(&node, dn_pad).unwrap();
        assert_eq!((again.pair.anchor_p, again.pair.anchor_n, again.id_p, again.id_n), (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(1.0)), found.id_p, found.id_n));
        // the nearest of the coupled net is the partner: a farther USB_DN pad does not matter
        assert_eq!(found.id_n, dn_pad);
    }

    #[test]
    fn a_net_that_is_not_half_of_a_pair_is_refused_with_kicads_message() {
        let mut node = Node::new();
        let id = smd_pad(&mut node, "GND", p(Nm::ZERO, Nm::ZERO), mm(0.25));
        assert!(find_dp_primitive_pair(&node, id).unwrap_err().contains("Unable to find complementary differential pair nets"));
        let lone = smd_pad(&mut node, "USB_DP", p(mm(5.0), Nm::ZERO), mm(0.25));
        assert!(find_dp_primitive_pair(&node, lone).unwrap_err().contains("coupled net \"USB_DN\""), "no USB_DN anywhere");
    }

    #[test]
    fn a_track_can_start_a_pair_only_at_its_dangling_end() {
        let mut node = Node::new();
        let t_p = track(&mut node, "USB_DP", p(Nm::ZERO, Nm::ZERO), p(mm(2.0), Nm::ZERO), mm(0.15));
        let t_n = track(&mut node, "USB_DN", p(Nm::ZERO, mm(0.4)), p(mm(2.0), mm(0.4)), mm(0.15));
        // joints in the middle of both: the pair is two segments a line, and only their outer ends are free
        let (p1, p2) = node.split_segment(t_p, p(mm(1.0), Nm::ZERO)).unwrap();
        let (n1, n2) = node.split_segment(t_n, p(mm(1.0), mm(0.4))).unwrap();
        let found = find_dp_primitive_pair(&node, p2).expect("the end of P and the nearest free end of N");
        assert_eq!((found.pair.anchor_p, found.pair.anchor_n), (p(mm(2.0), Nm::ZERO), p(mm(2.0), mm(0.4))));
        assert!(found.pair.directional());
        assert_eq!((found.id_p, found.id_n), (p2, n2));
        // from the other end: the first halves
        let other = find_dp_primitive_pair(&node, n1).unwrap();
        assert_eq!((other.pair.anchor_p, other.pair.anchor_n), (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(0.4))));
        let _ = p1;
        // a segment with no free end -- one a third track meets at the far end too -- is no start
        node.add(Item::Segment(Segment { net: net_of("USB_DP"), layer: 0, a: p(mm(1.0), Nm::ZERO), b: p(mm(1.0), mm(-0.5)), width: mm(0.15), source_track: None, locked: false }));
        assert!(dangling_anchor(&node, node.get(p2).unwrap()).is_some(), "the far end is still free");
        node.add(Item::Segment(Segment { net: net_of("USB_DP"), layer: 0, a: p(mm(2.0), Nm::ZERO), b: p(mm(2.5), Nm::ZERO), width: mm(0.15), source_track: None, locked: false }));
        assert!(find_dp_primitive_pair(&node, p2).unwrap_err().contains("make sure you are at the end"), "P is joined at both ends now");
    }

    #[test]
    fn a_pair_between_two_pad_pairs_is_coupled_and_finishes_on_them() {
        let (node, start) = two_pad_pairs();
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        assert_eq!(dp.sizes.pitch(), mm(0.45));
        // the cursor on the far pair's P pad: the end item is found, the pair snaps onto both pads
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)));
        let pv = dp.preview();
        assert!(!pv.colliding && pv.snapped_end, "{pv:?}");
        assert_eq!((pv.head_a.first(), pv.head_a.last()), (Some(p(Nm::ZERO, Nm::ZERO)), Some(p(mm(10.0), Nm::ZERO))));
        assert_eq!((pv.head_b.first(), pv.head_b.last()), (Some(p(Nm::ZERO, mm(1.0))), Some(p(mm(10.0), mm(1.0)))));
        // the two lines run side by side at the pitch for most of the way
        let mut trace = dp.current_trace.clone();
        trace.width = mm(0.2);
        trace.set_gap(mm(0.25));
        assert!(trace.coupled_length() > 8_000_000.0, "coupled {}", trace.coupled_length());
        // fixing it there finishes the route
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(10.0), Nm::ZERO), false), Some(FixOutcome::Fixed { real_end: true })));
        assert!(dp.idle);
        let commit = dp.finish(&rules, &settings, p(mm(10.0), Nm::ZERO)).expect("already finished");
        assert_eq!(commit.legs.len(), 1);
        assert_eq!((commit.legs[0].p.first(), commit.legs[0].p.last()), (Some(&p(Nm::ZERO, Nm::ZERO)), Some(&p(mm(10.0), Nm::ZERO))));
    }

    /// The least edge-to-edge distance between the lines of the head (a gap, not a pitch).
    fn min_gap(a: &[Point], wa: Nm, b: &[Point], wb: Nm) -> f64 {
        let mut best = f64::MAX;
        for sa in a.windows(2) {
            for sb in b.windows(2) {
                best = best.min(Shape::Stadium { a: sa[0], b: sa[1], r: wa / 2 }.gap_to(&Shape::Stadium { a: sb[0], b: sb[1], r: wb / 2 }));
            }
        }
        best
    }

    /// The least gap between a line of `width` along `pts` and an item.
    fn gap_to_item(pts: &[Point], width: Nm, item: &Item) -> f64 {
        pts.windows(2).map(|w| Shape::Stadium { a: w[0], b: w[1], r: width / 2 }.gap_to(&item.shape(item.layers().start()))).fold(f64::MAX, f64::min)
    }

    /// The pair keeps its gap: no two points of the lines closer than the gap less a micrometre of rounding, and over most of its
    /// length the lines run beside each other at it (`DIFF_PAIR::CoupledLength`).
    fn assert_coupled(pv: &DiffPairPreview, min_share: f64) {
        let gap = min_gap(&pv.head_a.pts, pv.width, &pv.head_b.pts, pv.width);
        assert!(gap >= pv.gap.as_f64() - 100.0, "the lines come within {gap} of each other, the gap is {}\n P {:?}\n N {:?}", pv.gap, pv.head_a.pts, pv.head_b.pts);
        let mut dp = DiffPair { p: pv.head_a.pts.clone(), n: pv.head_b.pts.clone(), width: pv.width, ..DiffPair::default() };
        dp.set_gap(pv.gap);
        let share = dp.coupled_length() / dp.total_length();
        assert!(share >= min_share, "only {:.0} % of the pair is coupled\n P {:?}\n N {:?}", share * 100.0, pv.head_a.pts, pv.head_b.pts);
    }

    #[test]
    fn marking_obstacles_only_reports_a_pad_in_the_way() {
        let (mut node, start) = two_pad_pairs();
        pad(&mut node, "GND", p(mm(5.0), mm(-0.1)), mm(0.4));
        let (rules, settings) = (rules(), mode(Mode::MarkObstacles));
        let mut dp = placer(&node, start);
        assert!(!dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)));
        let pv = dp.preview();
        assert!(pv.colliding, "the straight pair runs into the pad");
        assert_eq!(pv.head_a.pts.len(), 4, "and is drawn as it is, not walked");
        assert!(dp.fix_route(&rules, &settings, p(mm(10.0), Nm::ZERO), false).is_none(), "a violation is not fixed ...");
        let allowed = RoutingSettings { can_violate_drc: true, ..settings };
        assert!(dp.fix_route(&rules, &allowed, p(mm(10.0), Nm::ZERO), false).is_some(), "... unless violations are allowed");
    }

    #[test]
    fn a_pair_walks_around_a_pad_together_and_keeps_its_gap() {
        // a pad just outside the P line (y = 300): the pair has to give way to it, both lines on the same side
        let (mut node, start) = two_pad_pairs();
        let gnd = pad(&mut node, "GND", p(mm(5.0), mm(-0.1)), mm(0.4));
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)), "a way around exists");
        let pv = dp.preview();
        assert!(!pv.colliding && pv.snapped_end);
        assert_eq!((pv.head_a.first(), pv.head_a.last()), (Some(p(Nm::ZERO, Nm::ZERO)), Some(p(mm(10.0), Nm::ZERO))), "ends where it began");
        assert_eq!((pv.head_b.first(), pv.head_b.last()), (Some(p(Nm::ZERO, mm(1.0))), Some(p(mm(10.0), mm(1.0)))));
        let item = node.get(gnd).unwrap().clone();
        for (line, name) in [(&pv.head_a, "P"), (&pv.head_b, "N")] {
            let gap = gap_to_item(&line.pts, pv.width, &item);
            assert!(gap >= 200_000.0 - 500.0, "{name} comes within {gap} of the pad: {:?}", line.pts);
        }
        assert_coupled(&pv, 0.85);
        // it really did bend: more than the two fan-out corners at each end
        assert!(pv.head_a.pts.len() > 4 && pv.head_b.pts.len() > 4);
        // the pair as a whole moved, not one line: both bump away from the pad (down, y grows)
        let y_max = |l: &Line| l.pts.iter().map(|q| q.y).max().unwrap();
        assert!(y_max(&pv.head_a) >= mm(0.5) && y_max(&pv.head_b) >= mm(0.9), "P {:?} N {:?}", pv.head_a.pts, pv.head_b.pts);
    }

    /// `tryWalkDp`: when neither walk works, KiCad's `if( bestScore > 0.0 )` is true all the same and the pair takes the default `DIFF_PAIR`, two
    /// empty lines: a pair that cannot be walked has no head to show. Walk around calls it a fit and fixing it places nothing; Shove runs a shove with
    /// two empty heads, which fails.
    #[test]
    fn a_pair_that_cannot_be_walked_loses_its_head_as_kicads_trywalkdp_empties_it() {
        let (mut node, start) = two_pad_pairs();
        pad(&mut node, "GND", p(mm(5.0), mm(-0.1)), mm(0.4));
        let rules = rules();
        // one step of walking is not enough to get round the pad
        let impatient = |m: Mode| RoutingSettings { walkaround_iteration_limit: 1, ..mode(m) };
        let mut dp = placer(&node, start);
        let walk = impatient(Mode::Walkaround);
        assert!(dp.move_to(&rules, &walk, p(mm(10.0), Nm::ZERO)), "KiCad's tryWalkDp says true, and `m_fitOk` follows it");
        let pv = dp.preview();
        assert!(pv.head_a.pts.is_empty() && pv.head_b.pts.is_empty(), "the pair is emptied: {pv:?}");
        assert!(!pv.colliding, "and not flagged");
        assert!(dp.fix_route(&rules, &walk, p(mm(10.0), Nm::ZERO), false).is_none(), "an empty head is not a leg");
        assert!(dp.legs().is_empty());
        // the same cursor with patience: the pair is back
        assert!(dp.move_to(&rules, &mode(Mode::Walkaround), p(mm(10.0), Nm::ZERO)));
        assert!(dp.preview().head_a.pts.len() > 2);
        // Shove: two empty heads fail the shove, so the pair does not fit
        let mut dp = placer(&node, start);
        let shove = impatient(Mode::Shove);
        assert!(!dp.move_to(&rules, &shove, p(mm(10.0), Nm::ZERO)));
        let pv = dp.preview();
        assert!(pv.head_a.pts.is_empty() && pv.colliding, "{pv:?}");
    }

    #[test]
    fn a_pair_shoves_a_track_and_keeps_its_gap() {
        // a GND track in a U whose bottom (y = 150) dips into the pair's lane
        let (mut node, start) = two_pad_pairs();
        for (i, w) in [p(mm(3.0), mm(-1.5)), p(mm(3.0), mm(0.15)), p(mm(7.0), mm(0.15)), p(mm(7.0), mm(-1.5))].windows(2).enumerate() {
            node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some(("u".into(), i)), locked: false }));
        }
        let (rules, settings) = (rules(), mode(Mode::Shove));
        let mut dp = placer(&node, start);
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)), "the track can be pushed");
        let pv = dp.preview();
        assert!(!pv.colliding && pv.snapped_end);
        // the pair itself did not give way: it is the straight pair of the free-space case
        assert_eq!(pv.head_a.pts, vec![p(Nm::ZERO, Nm::ZERO), p(Nm::new(274_999), Nm::new(274_999)), p(Nm::new(9_725_001), Nm::new(274_999)), p(mm(10.0), Nm::ZERO)]);
        assert_eq!(pv.head_b.pts, vec![p(Nm::ZERO, mm(1.0)), p(Nm::new(274_999), Nm::new(725_001)), p(Nm::new(9_725_001), Nm::new(725_001)), p(mm(10.0), mm(1.0))]);
        assert_coupled(&pv, 0.9);
        // the track gave way: it is one displaced IR track, replaced by a line that keeps the clearance from both lines
        assert_eq!(pv.displaced_lines.len(), 1, "{:?}", pv.displaced_lines);
        assert_eq!(pv.displaced_lines[0].source_track.as_deref(), Some("u"));
        let moved = &pv.displaced_lines[0].line;
        assert_eq!((moved.first(), moved.last()), (Some(p(mm(3.0), mm(-1.5))), Some(p(mm(7.0), mm(-1.5)))), "its ends stayed");
        for (line, name) in [(&pv.head_a, "P"), (&pv.head_b, "N")] {
            let gap = min_gap(&line.pts, pv.width, &moved.pts, mm(0.2));
            assert!(gap >= 199_000.0, "{name} is {gap} from the shoved track\n {:?}", moved.pts);
        }
        // fixing and finishing carries it into the commit
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(10.0), Nm::ZERO), false), Some(FixOutcome::Fixed { real_end: true })));
        let commit = dp.finish(&rules, &settings, p(mm(10.0), Nm::ZERO)).unwrap();
        assert_eq!(commit.displaced_tracks.len(), 1);
        assert_eq!(commit.displaced_tracks[0].0, "u");
        assert_eq!(commit.displaced_tracks[0].1.len(), 1);
    }

    #[test]
    fn a_fixed_leg_keeps_showing_the_track_it_shoved() {
        // a GND track in a short U whose bottom dips into the pair's lane only between x = 3000 and 4000
        let (mut node, start) = two_pad_pairs();
        for (i, w) in [p(mm(3.0), mm(-1.5)), p(mm(3.0), mm(0.15)), p(mm(4.0), mm(0.15)), p(mm(4.0), mm(-1.5))].windows(2).enumerate() {
            node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some(("u".into(), i)), locked: false }));
        }
        let (rules, settings) = (rules(), mode(Mode::Shove));
        let mut dp = placer(&node, start);
        assert!(dp.move_to(&rules, &settings, p(mm(5.0), mm(0.5))));
        let shoved = dp.preview().displaced_lines.iter().map(|d| (d.source_track.clone(), d.line.pts.clone())).collect::<Vec<_>>();
        assert_eq!(shoved.len(), 1, "{shoved:?}");
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(5.0), mm(0.5)), false), Some(FixOutcome::Fixed { real_end: false })));
        // the next head is on from there and clear of the track: it pushes nothing, the leg before it has moved the track, and it is still drawn pushed, once
        assert!(dp.move_to(&rules, &settings, p(mm(9.0), mm(0.5))));
        assert!(dp.shoved_lines.is_empty(), "this head pushes nothing itself: the track is in the world where the fixed leg left it");
        let pv = dp.preview();
        assert_eq!(pv.displaced_lines.len(), 1, "{:?}", pv.displaced_lines);
        assert_eq!(pv.displaced_lines[0].source_track.as_deref(), Some("u"));
        assert_eq!(pv.displaced_lines[0].line.pts, shoved[0].1, "the pushed track is where the fixed leg left it");
        // and what the commit takes is the same track
        let commit = dp.finish(&rules, &settings, p(mm(9.0), mm(0.5))).unwrap();
        assert_eq!(commit.displaced_tracks.len(), 1);
        assert_eq!((commit.displaced_tracks[0].0.as_str(), commit.displaced_tracks[0].1[0].pts.clone()), ("u", shoved[0].1.clone()));
    }

    /// The pair's one `SHOVE` lives for the whole leg (`rhShoveOnly`: `m_currentNode = m_shove->CurrentNode()`), so a via a move
    /// pushed stays pushed -- where that frame left it, not where it started -- as the cursor goes on, until a fix commits the leg and
    /// the stack starts over (`FixRoute`: a fresh `SHOVE` over `m_world`). Without the stack every move re-derives from the world the
    /// last fix left, so the via is back where it started the moment the head is off it; a fixed leg's shove is
    /// `a_fixed_leg_keeps_showing_the_track_it_shoved`'s.
    #[test]
    fn a_via_the_pair_pushed_stays_pushed_until_the_leg_is_fixed() {
        let (mut node, start) = two_pad_pairs();
        // a GND via just above the N line's lane, in the pair's way to the target pads
        node.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(mm(5.0), mm(1.0)), diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaG".into()), locked: false, is_virtual: false }));
        // and, out of the way, a GND pad with a row of small ones after it: `propagateDpHeadForces` pushes the cursor out of the big one
        // (1.515 mm, the largest force of the loop, which every later step adds again) into each small one in turn, until its 40 steps
        // run out still colliding -- so `routeHead` fails
        let (ax, ay) = (mm(0.0), mm(-30.0));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(ax, ay), shape: Shape::Circle { c: p(ax, ay), r: mm(1.0) }, source: "U9.0".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        for k in 1..=45i64 {
            let c = p(ax + mm(0.01) + Nm::new(1_515_000) * k + mm(0.3), ay);
            node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r: mm(0.05) }, source: format!("U9.{k}"), edge: false, rule: 0, anchor_points: Vec::new() }));
        }
        let (rules, settings) = (rules(), mode(Mode::Shove));
        let mut dp = placer(&node, start);
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)), "the via can be pushed");
        let pushed = dp.preview().displaced_vias.clone();
        assert_eq!(pushed.len(), 1, "{pushed:?}");
        assert_eq!(pushed[0].source_via, "viaG");
        assert_ne!(pushed[0].pos, p(mm(5.0), mm(1.0)), "the via is pushed off the N line");
        // round-2 R-6: the cursor on the big pad, where the head cannot be routed: `rhShoveOnly` returns before its shove, and what is shown
        // is still the newest frame (`m_lastNode = latestNode->Branch()`), with the via pushed
        assert!(!dp.move_to(&rules, &settings, p(ax + mm(0.01), ay)), "the head cannot be routed there");
        let pv = dp.preview();
        assert_eq!(pv.displaced_vias.iter().map(|v| (v.source_via.clone(), v.pos)).collect::<Vec<_>>(), vec![(pushed[0].source_via.clone(), pushed[0].pos)], "the pushed via stays pushed: {pv:?}");
        // the cursor draws back, so the head is nowhere near the via: the frame the first position pushed is still the one the leg's
        // shove starts from, and the via stays where that frame left it
        assert!(dp.move_to(&rules, &settings, p(mm(2.0), mm(-2.0))));
        let pv = dp.preview();
        assert!(!pv.colliding, "{pv:?}");
        assert_eq!(pv.displaced_vias.len(), 1, "the via stays pushed: {pv:?}");
        assert_eq!((pv.displaced_vias[0].source_via.clone(), pv.displaced_vias[0].pos), (pushed[0].source_via.clone(), pushed[0].pos));
        // a fix commits the leg's frame, with the via where the frame left it
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(2.0), mm(-2.0)), false), Some(FixOutcome::Fixed { real_end: false })));
        let commit = dp.finish(&rules, &settings, p(mm(2.0), mm(-2.0))).unwrap();
        assert_eq!(commit.displaced_vias, vec![("viaG".to_string(), pushed[0].pos)]);
    }

    #[test]
    fn the_same_track_is_walked_around_in_walk_around_mode() {
        let (mut node, start) = two_pad_pairs();
        for (i, w) in [p(mm(3.0), mm(-1.5)), p(mm(3.0), mm(0.15)), p(mm(7.0), mm(0.15)), p(mm(7.0), mm(-1.5))].windows(2).enumerate() {
            node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some(("u".into(), i)), locked: false }));
        }
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)));
        let pv = dp.preview();
        assert!(!pv.colliding && pv.displaced_lines.is_empty(), "nothing is moved");
        assert_coupled(&pv, 0.85);
        for (line, name) in [(&pv.head_a, "P"), (&pv.head_b, "N")] {
            for w in [(p(mm(3.0), mm(-1.5)), p(mm(3.0), mm(0.15))), (p(mm(3.0), mm(0.15)), p(mm(7.0), mm(0.15))), (p(mm(7.0), mm(0.15)), p(mm(7.0), mm(-1.5)))] {
                let gap = min_gap(&line.pts, pv.width, &[w.0, w.1], mm(0.2));
                assert!(gap >= 199_000.0, "{name} is {gap} from the track: {:?}", line.pts);
            }
        }
    }

    #[test]
    fn a_pad_pair_at_45_degrees_fans_out_diagonally() {
        // rectangular pads (1.0 x 0.6 mm) on a diagonal: (0,0) and (700,700). The gateways with entry lines -- the two lines leave the
        // pads side by side, along the pads' long side -- are only built for pads that line up like this.
        let mut node = Node::new();
        let rect = |c: Point| Shape::Rect { x0: c.x - mm(0.5), y0: c.y - mm(0.3), x1: c.x + mm(0.5), y1: c.y + mm(0.3) };
        let start = node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::single(0), pos: p(Nm::ZERO, Nm::ZERO), shape: rect(p(Nm::ZERO, Nm::ZERO)), source: "J1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::single(0), pos: p(mm(0.7), mm(0.7)), shape: rect(p(mm(0.7), mm(0.7))), source: "J1.2".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        let found = find_dp_primitive_pair(&node, start).unwrap();
        let mut gws = DpGateways::new(mm(0.4));
        gws.build_from_primitive_pair(&found.pair, false);
        let fans: Vec<&crate::diff_pair::DpGateway> = gws.gateways.iter().filter(|g| g.priority >= 99).collect();
        assert_eq!(fans.len(), 4, "two fan-outs (ortho and diagonal distance) each way: {:?}", gws.gateways.iter().map(|g| g.priority).collect::<Vec<_>>());
        assert!(fans.iter().all(|g| g.has_entry_lines && g.entry_p[0] == p(Nm::ZERO, Nm::ZERO) && g.entry_n[0] == p(mm(0.7), mm(0.7))), "the entry lines start on the pads");
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        // to the right of both pads: the lines leave the pads and run side by side
        assert!(dp.move_to(&rules, &settings, p(mm(7.0), mm(0.35))));
        let pv = dp.preview();
        assert!(!pv.colliding);
        assert_eq!((pv.head_a.first(), pv.head_b.first()), (Some(p(Nm::ZERO, Nm::ZERO)), Some(p(mm(0.7), mm(0.7)))), "from the pad centres");
        assert_coupled(&pv, 0.7);
        // every segment is horizontal, vertical or diagonal
        for line in [&pv.head_a, &pv.head_b] {
            for w in line.pts.windows(2) {
                let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
                assert!(dx == Nm::ZERO || dy == Nm::ZERO || dx == dy, "{:?}", line.pts);
            }
        }
    }

    #[test]
    fn a_pair_of_vias_is_placed_a_via_gap_apart_and_the_route_goes_on_on_the_other_layer() {
        let (mut node, start) = two_pad_pairs();
        // the start pads are SMD on F.Cu; the far pair is on B.Cu
        let far: Vec<ItemId> = node.iter().filter(|(_, it)| matches!(it, Item::Solid(s) if s.pos.x == mm(10.0))).map(|(id, _)| id).collect();
        for id in far {
            node.remove(id);
        }
        node.add(Item::Solid(Solid { net: net_of("USB_DP"), layers: LayerRange::single(1), pos: p(mm(10.0), Nm::ZERO), shape: Shape::Circle { c: p(mm(10.0), Nm::ZERO), r: mm(0.25) }, source: "far P".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        node.add(Item::Solid(Solid { net: net_of("USB_DN"), layers: LayerRange::single(1), pos: p(mm(10.0), mm(1.0)), shape: Shape::Circle { c: p(mm(10.0), mm(1.0)), r: mm(0.25) }, source: "far N".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        dp.toggle_via(true, None, None, Some(1));
        assert!(dp.is_placing_via());
        assert!(dp.move_to(&rules, &settings, p(mm(5.0), mm(0.5))));
        let pv = dp.preview();
        assert!(!pv.colliding, "{pv:?}");
        assert_eq!(pv.vias.len(), 2, "one via per net");
        let (via_p, via_n) = (&pv.vias[0], &pv.vias[1]);
        assert_eq!((via_p.net.as_deref(), via_n.net.as_deref()), (Some("USB_DP"), Some("USB_DN")));
        assert_eq!(via_p.layers, LayerRange::new(0, 1), "a through via");
        // `viaGap()`: the vias are the via gap (the larger of the pair's 250 and the hole-to-hole spacing) apart, edge to edge
        let centres = vnorm(vsub(via_p.pos, via_n.pos));
        let wanted = dp.sizes.via_gap_total() + dp.sizes.via_diameter;
        assert!(centres >= wanted - Nm::new(2000) && centres <= wanted + Nm::new(4000), "centres {centres}, wanted about {wanted}");
        assert_eq!(pv.head_a.last(), Some(via_p.pos), "each line ends in its via");
        assert_eq!(pv.head_b.last(), Some(via_n.pos));
        // fix: the pair of vias is placed and the route goes on from them, on B.Cu
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(5.0), mm(0.5)), false), Some(FixOutcome::Fixed { real_end: false })));
        assert_eq!(dp.current_layer(), 1);
        assert!(!dp.is_placing_via());
        assert_eq!(dp.legs().len(), 1);
        assert!(dp.legs()[0].vias.is_some());
        assert!(dp.move_to(&rules, &settings, p(mm(10.0), Nm::ZERO)), "on to the far pads");
        let pv = dp.preview();
        assert_eq!(pv.layer, 1);
        assert!(pv.snapped_end && !pv.colliding);
        assert_eq!(pv.head_a.first(), Some(via_p.pos), "from the via");
        assert_eq!(pv.fixed_vias.len(), 2);
        let commit = dp.finish(&rules, &settings, p(mm(10.0), Nm::ZERO)).unwrap();
        assert_eq!(commit.legs.len(), 2);
        assert_eq!((commit.legs[0].layer, commit.legs[1].layer), (0, 1));
        assert_eq!(commit.legs[1].p.last(), Some(&p(mm(10.0), Nm::ZERO)));
    }

    #[test]
    fn a_pair_of_vias_is_pushed_off_a_pad_in_walk_around_mode() {
        let (mut node, start) = two_pad_pairs();
        pad(&mut node, "GND", p(mm(5.0), mm(0.5)), mm(0.3));
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        dp.toggle_via(true, None, None, Some(1));
        // the cursor is just beside the pad's centre: `propagateDpHeadForces` moves the pair's end off it
        assert!(dp.move_to(&rules, &settings, p(mm(5.0), mm(0.7))));
        let pv = dp.preview();
        assert!(!pv.colliding, "{pv:?}");
        assert_eq!(pv.vias.len(), 2);
        let gnd = node.iter().find(|(_, it)| matches!(it, Item::Solid(s) if s.net.as_deref() == Some("GND"))).map(|(_, it)| it.clone()).unwrap();
        for v in &pv.vias {
            let gap = Shape::Circle { c: v.pos, r: v.diameter / 2 }.gap_to(&gnd.shape(0));
            assert!(gap >= 199_000.0, "a via is {gap} from the pad");
        }
    }

    #[test]
    fn backspace_takes_the_last_leg_back() {
        let (node, start) = two_pad_pairs();
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        assert!(!dp.undo_last_segment(), "nothing to take back yet");
        dp.move_to(&rules, &settings, p(mm(4.0), mm(0.5)));
        assert!(matches!(dp.fix_route(&rules, &settings, p(mm(4.0), mm(0.5)), false), Some(FixOutcome::Fixed { real_end: false })));
        assert_eq!(dp.legs().len(), 1);
        let first_end = dp.legs()[0].p.last().copied().unwrap();
        dp.move_to(&rules, &settings, p(mm(6.0), mm(0.7)));
        assert!(dp.fix_route(&rules, &settings, p(mm(6.0), mm(0.7)), false).is_some());
        assert_eq!(dp.legs().len(), 2);
        assert_eq!(dp.legs()[1].p.first(), Some(&first_end), "the second leg goes on from the first");
        assert!(dp.undo_last_segment());
        assert_eq!(dp.legs().len(), 1);
        assert!(dp.undo_last_segment());
        assert!(dp.legs().is_empty() && !dp.undo_last_segment());
        // and the route can be started over from the pads
        dp.move_to(&rules, &settings, p(mm(4.0), mm(0.5)));
        assert_eq!(dp.preview().head_a.first(), Some(p(Nm::ZERO, Nm::ZERO)));
    }

    #[test]
    fn flipping_the_posture_chooses_the_other_of_two_fits() {
        // the cursor is up and a little to the right of the pads: the pair can leave straight up and turn at the end, or leave
        // sideways and turn up at once (`FitGateways` gives the posture it prefers a bonus over the other one)
        let (node, start) = two_pad_pairs();
        let (rules, settings) = (rules(), mode(Mode::Walkaround));
        let mut dp = placer(&node, start);
        let target = p(mm(1.0), mm(-2.0));
        assert!(dp.move_to(&rules, &settings, target));
        let before = dp.preview().head_a.pts.clone();
        dp.flip_posture();
        assert!(dp.move_to(&rules, &settings, target));
        let after = dp.preview().head_a.pts.clone();
        assert_ne!(before, after, "the other posture is another route to the same place");
        assert_eq!(before.first(), after.first());
        // flipping again is the first one again
        dp.flip_posture();
        dp.move_to(&rules, &settings, target);
        assert_eq!(dp.preview().head_a.pts, before);
    }

    /// findings p5 F4-1: `ImportSizes` takes the gap and the via gap from the `DIFF_PAIR_GAP` constraint (the class's pair gap), floored at
    /// the board's minimum clearance; the clearance between the two nets and the class's via gap play no part.
    #[test]
    fn dp_sizes_take_the_gap_and_the_via_gap_from_the_pair_gap() {
        let mut rules: BoardRules = serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap();
        rules.net_classes.push(eda_model::NetClass { name: "usb".into(), nets: vec!["USB_*".into()], track_width: None, clearance: Some(mm(0.25)), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: Some(mm(0.2)), diff_pair_via_gap: Some(mm(0.5)), priority: 0 });
        let sizes = DpSizes::resolve(&rules, "USB_P", "USB_N");
        assert_eq!((sizes.gap, sizes.via_gap), (mm(0.2), mm(0.2)), "not the class clearance (250), not the class via gap (500)");
        rules.min_clearance_nm = mm(0.3);
        let sizes = DpSizes::resolve(&rules, "USB_P", "USB_N");
        assert_eq!((sizes.gap, sizes.via_gap), (mm(0.3), mm(0.3)), "floored at the board's minimum clearance");
        // no class: KiCad's Default class sizes, the width at least the board minimum
        let plain: BoardRules = serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap();
        let sizes = DpSizes::resolve(&plain, "A_P", "A_N");
        assert_eq!((sizes.width, sizes.gap), (mm(0.2), mm(0.25)));
        let (node, start) = two_pad_pairs();
        let mut dp = DiffPairPlacer::start(&node, &plain, &RoutingSettings::default(), start, 0, LayerRange::new(0, 1)).unwrap();
        dp.set_dimensions(None, Some(mm(0.15)), None);
        assert_eq!(dp.sizes.gap, mm(0.15), "typed in: kept");
    }
}


