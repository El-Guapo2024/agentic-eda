//! Constraint model — the SOURCE OF TRUTH for a design.
//! Coordinates never appear here; they are derived by the engine.
//! JSON internally, YAML at LLM-facing edges.

pub mod board;
pub mod floorplan;
pub mod footprint;
pub mod ir;

pub use footprint::{Footprint, Pad, PadKind, PadShape};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintModel {
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub nets: Vec<Net>,
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    #[serde(default)]
    pub placement_rules: Vec<PlacementRule>,
    #[serde(default)]
    pub stackup: Option<Stackup>,
    #[serde(default)]
    pub impedance_targets: Vec<ImpedanceTarget>,
    /// Explicit footprint definitions; anything not listed here falls to
    /// the built-in package library (`footprint::builtin`).
    #[serde(default)]
    pub footprints: Vec<Footprint>,
    /// Board rules consumed by the placer, router and routing gates.
    #[serde(default)]
    pub board: BoardRules,
    /// What the solver may change on its own when the rules as written do
    /// not route (see `eda solve`). Absent = nothing: the board is built
    /// exactly as specified or fails.
    #[serde(default)]
    pub allow: Allowances,
    /// Stage choices and placement tuning (see [`SolverSettings`]); router
    /// tuning lives in `board.tuning`.
    #[serde(default)]
    pub solver: SolverSettings,
}

/// Design-rule latitude granted to the solver. Every field is an upper or
/// lower bound the user is willing to accept; the solver walks the
/// cheapest-first ladder of variants inside these bounds and takes the
/// first one that passes every gate, logging each attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Allowances {
    /// Most copper layers the solver may go to (2 or 4). None = as written.
    #[serde(default)]
    pub max_layers: Option<usize>,
    /// Narrowest track the solver may fall back to, µm. None = as written.
    #[serde(default)]
    pub min_track: Option<ir::Um>,
    /// Smallest clearance the solver may fall back to, µm. None = as written.
    #[serde(default)]
    pub min_clearance: Option<ir::Um>,
    /// Placers the solver may try, in preference order (`anneal`, `cypress`).
    /// Empty = only the one given on the command line.
    #[serde(default)]
    pub placers: Vec<String>,
}

/// Physical design rules. Integer µm throughout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardRules {
    /// Routing grid pitch.
    #[serde(default = "d_grid")]
    pub grid: ir::Um,
    #[serde(default = "d_track")]
    pub track_width: ir::Um,
    #[serde(default = "d_clearance")]
    pub clearance: ir::Um,
    #[serde(default = "d_via_drill")]
    pub via_drill: ir::Um,
    #[serde(default = "d_via_dia")]
    pub via_diameter: ir::Um,
    /// Copper layer names, outer first: `["F.Cu", "B.Cu"]`.
    #[serde(default = "d_layers")]
    pub layers: Vec<String>,
    /// Per-class track width and routing order. A board without classes
    /// routes every net at `track_width` in one undifferentiated pass,
    /// which is not how anyone lays out a board: power wants copper, and
    /// wants it before the signals have taken the channels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_classes: Vec<NetClass>,
    /// Board outline override, µm. When absent the placer sizes a rectangle.
    #[serde(default)]
    pub outline: Option<Vec<ir::Point>>,
    /// Refdes silkscreen font size override, µm. None = 1/40 of the shorter
    /// board side, clamped to 600..=1000 (KiCad's default text is 1 mm; on
    /// a 100 mm board the unclamped rule gave 2.5 mm labels that dwarfed
    /// every passive and sealed their pads).
    #[serde(default)]
    pub refdes_font_um: Option<ir::Um>,
    /// Copper pours. A poured net is not track-routed at all: its pads
    /// reach each other through the plane, which is how every real board
    /// carries GND. Declaring one is a claim the gates then have to check
    /// -- an unreachable pad is a hard fail, not a silent island.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pours: Vec<Pour>,
    /// Router tuning. Every value has a default; all are settable here.
    #[serde(default)]
    pub tuning: RoutingTuning,
}

/// One copper plane: a net flooded across a whole layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pour {
    pub net: String,
    /// Stackup layer name ("B.Cu").
    pub layer: String,
}

impl BoardRules {
    /// Refdes font for this board: the override, else 1/40 of the shorter
    /// rendered side (outline bbox + 2 mm margin), clamped to 600..=1000 µm.
    pub fn refdes_font(&self, outline: &[ir::Point]) -> ir::Um {
        if let Some(f) = self.refdes_font_um {
            return f;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (ir::Um::MAX, ir::Um::MAX, ir::Um::MIN, ir::Um::MIN);
        for p in outline {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if outline.is_empty() {
            return 600;
        }
        let m = 2000;
        let (vw, vh) = (x1 - x0 + 2 * m, y1 - y0 + 2 * m);
        (vw.min(vh) / 40).clamp(600, 1000)
    }
}

/// Every router constant, with the corpus-tuned value as default. Costs are
/// in A* steps at the reference 254 µm grid (the router rescales them to
/// the board's grid); distances in µm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RoutingTuning {
    /// `negotiated` (PathFinder, default) or `sequential` (rip-up & reroute).
    pub router: String,
    /// Cost of a via in reference cells (~7.6 mm of track at 30).
    pub via_cost_cells: i64,
    /// Extra cost per direction change.
    pub bend_cost: i64,
    /// Copper-to-board-edge clearance (KiCad's default 0.5 mm).
    pub edge_clearance_um: ir::Um,
    /// Same-footprint SMD pad gaps narrower than this are hard-blocked.
    pub between_pads_max_gap_um: ir::Um,
    /// Penalty per cell under a refdes label (labels are hard keep-outs;
    /// this only prices the relaxed diagnostics passes).
    pub refdes_penalty: u8,
    /// Soft escape lane outside fine-pitch pad rows: reach and per-cell price.
    pub escape_lane_um: ir::Um,
    pub escape_lane_penalty: u8,
    /// Cells a pad's free pocket must reach (or a via site / own pad) to
    /// pass the placement preflight.
    pub preflight_reach_cells: usize,
    /// Sequential router: rip-up rounds per net, victims per round, history
    /// bump per round, A* expansion cap at the reference grid.
    pub seq_max_rounds: u32,
    pub seq_max_victims: usize,
    pub seq_hist_bump: u8,
    pub seq_max_expansions: usize,
    /// Negotiated router: iteration cap, present-cost factor start /
    /// growth / ceiling, minimum history increment.
    pub nc_max_iters: usize,
    pub nc_pres_fac_0: f64,
    pub nc_pres_fac_mult: f64,
    pub nc_pres_fac_max: f64,
    pub nc_hist_inc: u16,
    /// Wall-clock budget for the negotiated router, seconds. When it runs
    /// out the run fails with route_congestion_unresolved and its hotspots
    /// instead of grinding through every iteration (L4 took 16-24 min to
    /// fail). 0 = no budget.
    pub nc_max_wall_s: f64,
    /// Stop when the overused-cell count has not improved for this many
    /// iterations. Off by default (0): PathFinder convergence is not
    /// monotone (L4 seed 0 sat at 26-166 overused cells for 12+ iterations
    /// and still converged before 40), so the wall budget is the honest
    /// limit. Set it for fast exploratory batches.
    pub nc_stall_iters: usize,
    /// Overused-cell count at or below which the negotiator stops ripping
    /// up the whole board each pass and rips up only the nets actually in
    /// conflict. 0 disables focusing and always rips up everything.
    pub nc_focus_cells: usize,
    /// After an iteration that ends worse than the best seen, restore the
    /// best board and keep the history learned from the bad pass. Turns
    /// the negotiation from a random walk into a monotone search.
    pub nc_rollback: bool,
    /// Extra A* step cost on a layer carrying a copper pour. Signals are
    /// discouraged from crossing the plane, not forbidden: a track there
    /// cuts the plane into islands, but sealing the layer outright would
    /// strand nets that have nowhere else to go. 0 disables.
    ///
    /// Kept small deliberately. At 3 (4x the base step) a ten-cell run
    /// across the plane cost more than a via, the signals crowded onto the
    /// remaining layer, and L4 went from routing clean to 14 unresolved
    /// conflicts against its whole wall budget. At 1 the router still
    /// prefers another layer wherever one will do.
    pub pour_layer_penalty: u8,
    /// How far from a pad a stitching via may sit, µm. The via has to
    /// clear every pad's copper and every silkscreen label, both of which
    /// are hard gates, so on a dense board the first legal site is not
    /// next door. Too short a leash reports a reachable pad unreachable.
    pub pour_stitch_reach_um: ir::Um,
    /// Reserve a corridor on the poured layer joining every point a pad
    /// meets the plane, and keep other nets out of it.
    ///
    /// Without this, plane connectivity is whatever the signals happen to
    /// leave behind, and `routing_pour_cut_off` reports the damage after
    /// the fact. With it, the connection cannot be severed: the corridor
    /// is the net's own copper as far as the negotiator is concerned.
    ///
    /// The cost is real -- that copper is routing space the signals do not
    /// get -- but so is the physics. A layer carrying a plane is not a
    /// signal layer with a plane drawn on top of it.
    pub pour_reserve_skeleton: bool,
}
impl Default for RoutingTuning {
    fn default() -> Self {
        RoutingTuning {
            router: "negotiated".into(),
            via_cost_cells: 30,
            bend_cost: 2,
            edge_clearance_um: 500,
            between_pads_max_gap_um: 2000,
            refdes_penalty: 8,
            escape_lane_um: 1200,
            escape_lane_penalty: 6,
            preflight_reach_cells: 2000,
            seq_max_rounds: 6,
            seq_max_victims: 6,
            seq_hist_bump: 4,
            seq_max_expansions: 400_000,
            nc_max_iters: 40,
            nc_pres_fac_0: 0.5,
            nc_pres_fac_mult: 1.6,
            nc_pres_fac_max: 2000.0,
            nc_hist_inc: 2,
            nc_max_wall_s: 900.0,
            nc_stall_iters: 0,
            nc_focus_cells: 8,
            nc_rollback: true,
            pour_layer_penalty: 1,
            pour_stitch_reach_um: 3000,
            pour_reserve_skeleton: true,
        }
    }
}

/// Stage choices and placement tuning the intent may set. Defaults apply
/// when absent; the CLI flags override when given explicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SolverSettings {
    /// `anneal` (default) or `cypress`.
    pub placer: String,
    /// Anneal placer: keep-apart margin around courtyards (routing channel
    /// space), moves per part, placement snap grid, all µm / counts.
    pub place_spacing_um: ir::Um,
    pub place_moves_per_part: usize,
    pub place_snap_um: ir::Um,
    /// Cypress: weight of the synthetic proximity-rule nets.
    pub cypress_proximity_weight: f64,
    /// Cypress: shrink a rectangular `board.outline` (treated as the
    /// largest allowed board) until the parts' keep-out area is this
    /// fraction of the board, so a small design does not float in a big
    /// blank rectangle. 0 = keep the outline as written.
    pub fit_board_utilization: f64,
    /// How many placement seeds one rung of the solver's ladder may try
    /// before escalating. Placement is the cheap stage to repeat and most
    /// of its failures are near misses that a different arrangement
    /// clears; escalating re-places anyway, under coarser settings. 1 =
    /// the old single-shot behaviour.
    pub place_attempts: usize,
    /// Cut the board into functional modules and give each one a region
    /// before placing any part (see [`floorplan`]). This is how a human
    /// team lays out a board; on a hundred-part design it turns one large
    /// placement into a dozen small ones. Off by default: it changes the
    /// shape of every placement, so an intent opts in.
    #[serde(default)]
    pub floorplan: bool,
}
impl Default for SolverSettings {
    fn default() -> Self {
        SolverSettings { placer: "anneal".into(), place_spacing_um: 600, place_moves_per_part: 4000, place_snap_um: 100, cypress_proximity_weight: 50.0, fit_board_utilization: 0.25, place_attempts: 3, floorplan: false }
    }
}

/// Max courtyard gap, µm, from a decoupling capacitor to the IC it
/// decouples (shares both of its nets with). The placement gate judges it;
/// the placers pull toward it.
pub const DECOUPLING_MAX_GAP_UM: ir::Um = 3000;

/// A part the placer can freely move or swap to uncross a stub: two pins,
/// not a connector or an IC.
pub fn is_free_two_pin(part: &Part) -> bool {
    part.pins.len() == 2 && !part.reference.starts_with('J') && !part.reference.starts_with('U')
}

/// Decoupling pairs: a 2-pin `C…` part between a power net and a ground
/// net (judged by the pin kinds on those nets), and a `U…` part that has
/// pins on both. A cap from a signal net to ground is a filter, not
/// decoupling. Shared by the gate and the placers.
pub fn decoupling_pairs(model: &ConstraintModel) -> Vec<(String, String)> {
    let net_of_pin: std::collections::HashMap<&str, &str> =
        model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let kind_of_pin: std::collections::HashMap<String, PinKind> =
        model.parts.iter().flat_map(|p| p.pins.iter().map(move |pin| (format!("{}.{}", p.reference, pin.number), pin.kind))).collect();
    let net_kind = |name: &str| -> (bool, bool) {
        let Some(n) = model.nets.iter().find(|n| n.name == name) else { return (false, false) };
        let kinds: Vec<PinKind> = n.pins.iter().filter_map(|p| kind_of_pin.get(p).copied()).collect();
        (kinds.contains(&PinKind::Power), kinds.contains(&PinKind::Ground))
    };
    let mut pairs = Vec::new();
    for c in &model.parts {
        if !c.reference.starts_with('C') || c.pins.len() != 2 {
            continue;
        }
        let nets: Vec<&str> = c.pins.iter().filter_map(|p| net_of_pin.get(format!("{}.{}", c.reference, p.number).as_str()).copied()).collect();
        if nets.len() != 2 || nets[0] == nets[1] {
            continue;
        }
        let (k0, k1) = (net_kind(nets[0]), net_kind(nets[1]));
        let rail_to_ground = (k0.0 && k1.1) || (k1.0 && k0.1);
        if !rail_to_ground {
            continue;
        }
        for u in &model.parts {
            if !u.reference.starts_with('U') {
                continue;
            }
            let has = |net: &str| u.pins.iter().any(|p| net_of_pin.get(format!("{}.{}", u.reference, p.number).as_str()) == Some(&net));
            if has(nets[0]) && has(nets[1]) {
                pairs.push((c.reference.clone(), u.reference.clone()));
            }
        }
    }
    pairs
}
fn d_grid() -> ir::Um { 254 }
fn d_track() -> ir::Um { 200 }
fn d_clearance() -> ir::Um { 200 }
fn d_via_drill() -> ir::Um { 300 }
fn d_via_dia() -> ir::Um { 600 }
fn d_layers() -> Vec<String> { vec!["F.Cu".into(), "B.Cu".into()] }
impl BoardRules {
    /// The class owning `net`, if any: first match wins.
    pub fn class_of(&self, net: &str) -> Option<&NetClass> {
        self.net_classes.iter().find(|c| c.matches(net))
    }

    /// Track width `net` must be routed at.
    pub fn width_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.track_width).unwrap_or(self.track_width)
    }

    /// Widest track any net on this board can take. The router's clearance
    /// summaries are precomputed for one querying width, so they are built
    /// at this one: conservative for narrow nets, correct for every net.
    pub fn widest_track(&self) -> ir::Um {
        self.net_classes.iter().filter_map(|c| c.track_width).fold(self.track_width, ir::Um::max)
    }

    /// Routing order key: higher priority first. Power before signals.
    pub fn priority_of(&self, net: &str) -> i32 {
        self.class_of(net).map_or(0, |c| c.priority)
    }
}

impl Default for BoardRules {
    fn default() -> Self {
        BoardRules { grid: d_grid(), track_width: d_track(), clearance: d_clearance(), via_drill: d_via_drill(), via_diameter: d_via_dia(), layers: d_layers(), net_classes: Vec::new(), outline: None, refdes_font_um: None, pours: Vec::new(), tuning: RoutingTuning::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// Reference designator, unique: "U1", "C3".
    pub reference: String,
    #[serde(default)]
    pub mpn: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    /// KiCad footprint id, e.g. "Capacitor_SMD:C_0402_1005Metric".
    #[serde(default)]
    pub footprint: Option<String>,
    #[serde(default)]
    pub pins: Vec<Pin>,
    /// Cable comes in from outside: the part must sit on a board edge
    /// (`placement_edge_connector`). Unset = decided from the value/mpn
    /// (USB, jack, terminal block, receptacle…); a bare `J` header is
    /// *not* an edge part by itself — in real boards most `J` headers are
    /// interior (programming, jumpers), so say `edge: true` for the ones
    /// a cable plugs into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub number: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: PinKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinKind {
    Power,
    Ground,
    #[default]
    Signal,
    Passive,
    Nc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub name: String,
    /// Pin refs as "U1.3".
    #[serde(default)]
    pub pins: Vec<String>,
}

/// The atomic unit for both schematic and PCB.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub anchor: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlacementRule {
    /// Two parts must end up within `max_mm` of each other, measured
    /// courtyard gap to courtyard gap.
    Proximity {
        a: String,
        b: String,
        max_mm: f64,
        /// Why the rule exists, in the author's own words. Carried into
        /// the gate's failure detail, so a rule can be judged without
        /// reading whatever produced it. A rule proposed from circuit
        /// intuition rather than a datasheet says so here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Two parts must end up at least `min_mm` apart. The mirror of
    /// `Proximity`, and the reason it exists: heat, noise coupling and
    /// high-voltage clearance are all repulsive, and until this variant
    /// the intent could only ever ask for parts to be *closer*. Note that
    /// the Cypress placer cannot honour it -- proximity reaches Cypress as
    /// a synthetic weighted net, and a net can only pull -- so a design
    /// carrying separation rules is an anneal-placer design until that
    /// changes. The gate fails either way rather than ignoring the rule.
    Separation {
        a: String,
        b: String,
        min_mm: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Keepout { zone: String, refs: Vec<String> },
    ThermalGroup { refs: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stackup {
    pub layers: Vec<StackupLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackupLayer {
    pub name: String,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub thickness_mm: Option<f64>,
}

/// A group of nets that route alike. Matched by glob over the net name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetClass {
    pub name: String,
    /// Globs over net names: `["GND", "VBAT*", "3V3"]`. First class whose
    /// pattern matches a net owns it, so order the list most-specific first.
    pub nets: Vec<String>,
    /// Track width for this class. Absent = the board default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_width: Option<ir::Um>,
    /// Routed before lower numbers. Power belongs first: it needs copper,
    /// it sets the return paths, and it is the hardest thing to squeeze in
    /// once signal nets have taken the channels. Default 0; signals sit at
    /// 0 and power is given a higher number in the intent.
    #[serde(default)]
    pub priority: i32,
}

impl NetClass {
    /// Whether `net` belongs to this class. `*` is the only wildcard and
    /// matches any run of characters, which covers the way power nets are
    /// actually named (`VBAT_RAW`, `VBAT_FUSED`).
    pub fn matches(&self, net: &str) -> bool {
        self.nets.iter().any(|pat| glob_match(pat, net))
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpedanceTarget {
    /// Glob over net names, e.g. "USB_D*".
    pub net_pattern: String,
    pub ohms: f64,
    #[serde(default)]
    pub tolerance_pct: Option<f64>,
}

// ---------------------------------------------------------------- check shape
/// Every checker — schema, ERC, metrics, critic — returns this one shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub status: CheckStatus,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
    /// Machine-readable failure detail (what blocks, where, which knob
    /// would change it). Absent for passes and for checks that have not
    /// been upgraded yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

impl CheckResult {
    pub fn fail(check: &str, location: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { check: check.into(), status: CheckStatus::Fail,
               location: Some(location.into()), hint: Some(hint.into()), detail: None }
    }
    pub fn pass(check: &str) -> Self {
        Self { check: check.into(), status: CheckStatus::Pass, location: None, hint: None, detail: None }
    }
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl ConstraintModel {
    pub fn part(&self, reference: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.reference == reference)
    }
    /// Resolve the physical footprint for a part: explicit model
    /// definitions first (by `Part::footprint` name, then by `package`),
    /// then the built-in library. `None` means the part is not physically
    /// realisable and every physical stage must fail on it.
    pub fn footprint_of(&self, part: &Part) -> Option<Footprint> {
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(fp) = self.footprints.iter().find(|f| f.name == key) {
                return Some(fp.clone());
            }
            let norm = footprint::normalize_name(key);
            if let Some(fp) = self.footprints.iter().find(|f| footprint::normalize_name(&f.name) == norm) {
                return Some(fp.clone());
            }
        }
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(fp) = footprint::builtin(key) {
                return Some(fp);
            }
        }
        None
    }
    /// Simple glob match ('*' wildcard) over net names.
    pub fn nets_matching(&self, pattern: &str) -> Vec<&Net> {
        self.nets.iter().filter(|n| glob_match(pattern, &n.name)).collect()
    }
}

pub fn glob_match(pattern: &str, s: &str) -> bool {
    // '*' matches any run (incl. empty); everything else literal.
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], s) || (!s.is_empty() && rec(p, &s[1..])),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    rec(pattern.as_bytes(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob() {
        assert!(glob_match("SPI_*", "SPI_MOSI"));
        assert!(glob_match("VBUS", "VBUS"));
        assert!(!glob_match("SPI_*", "I2C_SCL"));
        assert!(glob_match("*", "anything"));
    }
    #[test]
    fn roundtrip() {
        let m = ConstraintModel::default();
        let j = serde_json::to_string(&m).unwrap();
        let _: ConstraintModel = serde_json::from_str(&j).unwrap();
    }
}
