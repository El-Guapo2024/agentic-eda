//! A verb set for building a board a step at a time.
//!
//! The placer we have takes one shot: `initial()` dumps a hundred parts
//! into shelf rows in reading order, the annealer scrambles them for
//! ninety seconds, and the gates deliver a verdict at the end. Nothing in
//! between is addressable. If the result is wrong there is no decision to
//! revisit, only a seed to change.
//!
//! This is the other shape. A board is a value you issue commands
//! against, each command is small enough to judge, and the gates run
//! after every one. An engineer does not place a hundred parts at once;
//! they put the regulator down, ring it with its capacitors, and check as
//! they go.
//!
//! # Commands never carry coordinates
//!
//! Every command names parts and directions, never micrometres:
//! `Place { part: "C20", anchor: "U11", side: North }`, not `at (67000,
//! 51200)`. Resolving that to an exact snapped pose is geometry's job.
//!
//! This is not a stylistic choice. It was measured. Asked to pick the
//! most stretched net from raw coordinates on real boards from this
//! project, the evaluation model scored 12 of 23 against a 1-in-6 random
//! baseline -- better than chance, but its confidence was *anti*
//! calibrated: it averaged 0.345 when wrong against 0.273 when right, and
//! anchored on one answer for ten of twenty-three trials regardless of
//! the board. Asked which part belongs next to an IC given what the IC
//! is, it scored 4 of 4 at 0.96 confidence, and 32 of 32 on settled
//! layout questions including deliberately counterintuitive ones.
//!
//! So the split is: the model chooses *what* and *which neighbour*, which
//! it is good at and our gates cannot express; arithmetic decides *where*,
//! exactly, for free, and identically every run.
//!
//! # Every command is checked, and a bad one is refused
//!
//! Commands hard-fail rather than doing something approximate: placing a
//! part twice, anchoring to a part that is not down yet, naming a part
//! the model has never heard of. A command that silently did nothing
//! would be indistinguishable from one that worked.

use eda_model::footprint::{placed_courtyard, placed_keepout};
use eda_model::ir::{Design, FootprintInstance, LabelSide, Point, Side, Um};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use std::collections::BTreeSet;

/// Which way from the anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    North,
    South,
    East,
    West,
}

impl Dir {
    /// The four of them, for enumerating candidates.
    pub const ALL: [Dir; 4] = [Dir::North, Dir::East, Dir::South, Dir::West];

    /// Unit step in board coordinates. Y grows downward, as everywhere
    /// else in this codebase, so north is negative y.
    fn delta(self) -> (i64, i64) {
        match self {
            Dir::North => (0, -1),
            Dir::South => (0, 1),
            Dir::East => (1, 0),
            Dir::West => (-1, 0),
        }
    }

    /// The name a decision model sees and returns.
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::North => "north",
            Dir::South => "south",
            Dir::East => "east",
            Dir::West => "west",
        }
    }
}

/// A ninth of the board, for putting the first part down.
///
/// Construction has to start somewhere and every later part is anchored
/// to one already placed, so the seed part needs a way to land that is
/// not relative to anything. Naming a region rather than a coordinate
/// keeps the command set free of micrometres: a decision model can pick
/// "centre" for an MCU without doing arithmetic it is bad at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    NorthWest,
    North,
    NorthEast,
    West,
    Centre,
    East,
    SouthWest,
    South,
    SouthEast,
}

impl Region {
    pub const ALL: [Region; 9] = [
        Region::NorthWest, Region::North, Region::NorthEast,
        Region::West, Region::Centre, Region::East,
        Region::SouthWest, Region::South, Region::SouthEast,
    ];

    /// Centre of this region as a fraction of the board, x then y.
    fn fractions(self) -> (f64, f64) {
        let third = |n: f64| (n + 0.5) / 3.0;
        let (cx, cy) = match self {
            Region::NorthWest => (0.0, 0.0),
            Region::North => (1.0, 0.0),
            Region::NorthEast => (2.0, 0.0),
            Region::West => (0.0, 1.0),
            Region::Centre => (1.0, 1.0),
            Region::East => (2.0, 1.0),
            Region::SouthWest => (0.0, 2.0),
            Region::South => (1.0, 2.0),
            Region::SouthEast => (2.0, 2.0),
        };
        (third(cx), third(cy))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Region::NorthWest => "north_west", Region::North => "north", Region::NorthEast => "north_east",
            Region::West => "west", Region::Centre => "centre", Region::East => "east",
            Region::SouthWest => "south_west", Region::South => "south", Region::SouthEast => "south_east",
        }
    }
}

/// One step a caller can take against a board.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Cmd {
    /// Put an unplaced `part` immediately beside an already-placed
    /// `anchor`, on `side` of it. The pose is the nearest snapped,
    /// collision-free one on that side.
    Place { part: String, anchor: String, side: Dir },
    /// Put an unplaced `part` flush against a board edge, `fraction` of
    /// the way along it (0.0 = start, 1.0 = end).
    PlaceEdge { part: String, edge: Dir, fraction: f64 },
    /// Put an unplaced `part` in a named region of the board. The way a
    /// board starts: the main IC lands, everything else hangs off it.
    PlaceRegion { part: String, region: Region },
    /// Shift a placed part by whole snap steps.
    Nudge { part: String, dir: Dir, steps: u32 },
    /// Turn a placed part by quarter turns.
    Rotate { part: String, quarter_turns: u8 },
    /// Exchange the positions of two placed parts.
    Swap { a: String, b: String },
    /// Take a placed part off the board, so a different choice can be
    /// made. Backtracking is a first-class move: a constructive placer
    /// that cannot undo paints itself into a corner at part eighty.
    Rip { part: String },
}

impl Cmd {
    /// The parts this command touches, for logging and credit assignment.
    pub fn subjects(&self) -> Vec<&str> {
        match self {
            Cmd::Place { part, anchor, .. } => vec![part, anchor],
            Cmd::PlaceEdge { part, .. }
            | Cmd::PlaceRegion { part, .. }
            | Cmd::Nudge { part, .. }
            | Cmd::Rotate { part, .. }
            | Cmd::Rip { part } => vec![part],
            Cmd::Swap { a, b } => vec![a, b],
        }
    }
}

/// A board under construction.
pub struct Board<'a> {
    model: &'a ConstraintModel,
    design: Design,
    snap: Um,
    /// Minimum gap left between courtyards when resolving a `Place`.
    spacing: Um,
}

/// What applying a command did to the gate verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub before: usize,
    pub after: usize,
}

impl Outcome {
    pub fn improved(&self) -> bool {
        self.after < self.before
    }
    /// A move that changes nothing about the verdict.
    ///
    /// These matter. The failures this project cannot repair need two or
    /// three parts moved together, and the first of those moves fixes
    /// nothing on its own -- a strict-improvement rule forbids exactly
    /// the sequence that works. A caller doing multi-step repair must be
    /// able to accept a plateau.
    pub fn level(&self) -> bool {
        self.after == self.before
    }
}

impl<'a> Board<'a> {
    /// An empty board with the given outline.
    pub fn new(design: Design, model: &'a ConstraintModel, snap: Um, spacing: Um) -> Self {
        Board { model, design, snap, spacing }
    }

    pub fn design(&self) -> &Design {
        &self.design
    }

    pub fn into_design(self) -> Design {
        self.design
    }

    fn placement(&self) -> &eda_model::ir::PlacementSection {
        self.design.placement.as_ref().expect("a Board always carries a placement section")
    }

    /// Refdes of everything currently on the board.
    pub fn placed(&self) -> BTreeSet<String> {
        self.placement().footprints.iter().map(|f| f.id.clone()).collect()
    }

    /// Parts not yet placed that have at least one placed neighbour --
    /// the legal next steps.
    ///
    /// A part with no placed neighbour is not offered: placing it would
    /// be a guess about a region of the board nothing has claimed yet,
    /// and the whole point of building outward is that each step is
    /// anchored to one already taken.
    pub fn frontier(&self) -> Vec<String> {
        let placed = self.placed();
        let mut out: Vec<String> = Vec::new();
        for p in &self.model.parts {
            if placed.contains(&p.reference) {
                continue;
            }
            if self.neighbours_of(&p.reference).iter().any(|n| placed.contains(n)) {
                out.push(p.reference.clone());
            }
        }
        out.sort();
        out
    }

    /// Parts sharing a small net or a proximity rule with `r`.
    ///
    /// Power and ground are excluded by the net-size cut: a 40-pin GND
    /// net makes every part everyone's neighbour and the frontier stops
    /// meaning anything.
    pub fn neighbours_of(&self, r: &str) -> BTreeSet<String> {
        const FANOUT_LIMIT: usize = 6;
        let mut out = BTreeSet::new();
        for n in &self.model.nets {
            let parts: BTreeSet<&str> = n.pins.iter().map(|p| p.split('.').next().unwrap_or(p)).collect();
            if parts.len() > FANOUT_LIMIT || !parts.contains(r) {
                continue;
            }
            out.extend(parts.into_iter().filter(|p| *p != r).map(String::from));
        }
        for rule in &self.model.placement_rules {
            if let eda_model::PlacementRule::Proximity { a, b, .. } = rule {
                if a == r {
                    out.insert(b.clone());
                } else if b == r {
                    out.insert(a.clone());
                }
            }
        }
        out
    }

    /// Gate failures on what is placed so far.
    pub fn checks(&self) -> Vec<CheckResult> {
        eda_gates::check_placement_partial(&self.design, self.model)
    }

    /// How many gates are failing right now.
    pub fn failures(&self) -> usize {
        self.checks().iter().filter(|c| matches!(c.status, CheckStatus::Fail)).count()
    }

    /// Apply a command, or refuse it.
    pub fn apply(&mut self, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            Cmd::Place { part, anchor, side } => self.place_beside(part, anchor, *side),
            Cmd::PlaceEdge { part, edge, fraction } => self.place_on_edge(part, *edge, *fraction),
            Cmd::PlaceRegion { part, region } => self.place_in_region(part, *region),
            Cmd::Nudge { part, dir, steps } => self.nudge(part, *dir, *steps),
            Cmd::Rotate { part, quarter_turns } => self.rotate(part, *quarter_turns),
            Cmd::Swap { a, b } => self.swap(a, b),
            Cmd::Rip { part } => self.rip(part),
        }
    }

    /// Apply a command and keep it only if the gates allow.
    ///
    /// `accept_level` decides whether a move that changes nothing is
    /// kept. Multi-part repairs need it; greedy construction does not.
    pub fn try_apply(&mut self, cmd: &Cmd, accept_level: bool) -> Result<Outcome, Vec<CheckResult>> {
        let before = self.failures();
        let saved = self.design.clone();
        self.apply(cmd)?;
        let after = self.failures();
        let outcome = Outcome { before, after };
        let keep = outcome.improved() || (accept_level && outcome.level());
        if !keep {
            self.design = saved;
        }
        Ok(outcome)
    }

    // ---------------------------------------------------------- lookups

    fn part(&self, r: &str) -> Result<&eda_model::Part, Vec<CheckResult>> {
        self.model
            .part(r)
            .ok_or_else(|| vec![CheckResult::fail("ops_unknown_part", r, "no part with this reference exists in the model")])
    }

    fn pose_of(&self, r: &str) -> Option<&FootprintInstance> {
        self.placement().footprints.iter().find(|f| f.id == r)
    }

    fn require_placed(&self, r: &str) -> Result<FootprintInstance, Vec<CheckResult>> {
        self.pose_of(r)
            .cloned()
            .ok_or_else(|| vec![CheckResult::fail("ops_not_placed", r, "this part is not on the board, so it cannot be moved or used as an anchor")])
    }

    fn require_unplaced(&self, r: &str) -> Result<(), Vec<CheckResult>> {
        if self.pose_of(r).is_some() {
            return Err(vec![CheckResult::fail("ops_already_placed", r, "this part is already on the board; rip it first to place it somewhere else")]);
        }
        Ok(())
    }

    /// Courtyard the part would have at this pose.
    fn courtyard_at(&self, r: &str, fp: &FootprintInstance) -> Result<(Um, Um, Um, Um), Vec<CheckResult>> {
        let p = self.part(r)?;
        placed_courtyard(self.model, p, fp)
            .ok_or_else(|| vec![CheckResult::fail("ops_no_footprint", r, "the part has no resolvable footprint, so it has no courtyard to place")])
    }

    /// Keepout of a placed part, for collision tests.
    fn keepout_of(&self, fp: &FootprintInstance) -> Option<(Um, Um, Um, Um)> {
        let p = self.model.part(&fp.id)?;
        placed_keepout(self.model, &self.placement().outline, p, fp)
    }

    /// Does this trial rectangle hit anything already down?
    fn collides(&self, r: &str, rect: (Um, Um, Um, Um)) -> bool {
        self.placement().footprints.iter().filter(|f| f.id != r).any(|f| {
            self.keepout_of(f).map(|k| overlaps(rect, k)).unwrap_or(false)
        })
    }

    fn board_bbox(&self) -> (Um, Um, Um, Um) {
        let o = &self.placement().outline;
        (
            o.iter().map(|p| p.x).min().unwrap_or(0),
            o.iter().map(|p| p.y).min().unwrap_or(0),
            o.iter().map(|p| p.x).max().unwrap_or(0),
            o.iter().map(|p| p.y).max().unwrap_or(0),
        )
    }

    // --------------------------------------------------------- commands

    /// Resolve `Place` to an exact pose.
    ///
    /// Start flush against the anchor on the named side, then slide along
    /// that side -- alternating either way from centred -- until nothing
    /// collides. Sliding rather than giving up matters: the spot directly
    /// beside an IC is usually taken by the last capacitor placed there,
    /// and the next one along is the answer a person would reach for.
    fn place_beside(&mut self, part: &str, anchor: &str, side: Dir) -> Result<(), Vec<CheckResult>> {
        self.require_unplaced(part)?;
        let anchor_fp = self.require_placed(anchor)?;
        let anchor_keep = self
            .keepout_of(&anchor_fp)
            .ok_or_else(|| vec![CheckResult::fail("ops_no_footprint", anchor, "the anchor has no resolvable footprint")])?;

        // The part's own extent, at rotation 0, measured about its origin.
        let probe = FootprintInstance { id: part.into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let c = self.courtyard_at(part, &probe)?;
        let (half_w, half_h) = ((c.2 - c.0) / 2, (c.3 - c.1) / 2);

        let (ax, ay) = ((anchor_keep.0 + anchor_keep.2) / 2, (anchor_keep.1 + anchor_keep.3) / 2);
        let (dx, dy) = side.delta();
        // Distance from anchor centre to the part centre when their
        // rectangles just touch, plus the spacing we owe.
        let stand_off = if dx != 0 {
            (anchor_keep.2 - anchor_keep.0) / 2 + half_w + self.spacing
        } else {
            (anchor_keep.3 - anchor_keep.1) / 2 + half_h + self.spacing
        };
        let base = (ax + dx * stand_off, ay + dy * stand_off);

        // Slide perpendicular to the placement direction.
        let (sx, sy) = if dx != 0 { (0, 1) } else { (1, 0) };
        let bb = self.board_bbox();
        for step in 0..MAX_SLIDE_STEPS {
            for sign in [1i64, -1] {
                if step == 0 && sign < 0 {
                    continue; // centred is one position, not two
                }
                let off = sign * step as i64 * self.snap;
                let at = Point { x: snap(base.0 + sx * off, self.snap), y: snap(base.1 + sy * off, self.snap) };
                let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
                let Ok(cr) = self.courtyard_at(part, &fp) else { continue };
                if cr.0 < bb.0 || cr.1 < bb.1 || cr.2 > bb.2 || cr.3 > bb.3 {
                    continue;
                }
                if self.collides(part, cr) {
                    continue;
                }
                self.design.placement.as_mut().unwrap().footprints.push(fp);
                self.sort_footprints();
                return Ok(());
            }
        }
        Err(vec![CheckResult::fail(
            "ops_no_room",
            format!("{part}/{anchor}"),
            format!("no collision-free pose {} of {anchor} within {MAX_SLIDE_STEPS} snap steps; rip a neighbour or choose another side", side.as_str()),
        )])
    }

    /// Resolve `PlaceRegion` to a pose, spiralling outward from the
    /// region's centre until nothing collides. The spiral matters when a
    /// caller seeds several parts into the same region.
    fn place_in_region(&mut self, part: &str, region: Region) -> Result<(), Vec<CheckResult>> {
        self.require_unplaced(part)?;
        let bb = self.board_bbox();
        let (fx, fy) = region.fractions();
        let centre = (
            bb.0 + ((bb.2 - bb.0) as f64 * fx) as Um,
            bb.1 + ((bb.3 - bb.1) as f64 * fy) as Um,
        );
        for ring in 0..MAX_SLIDE_STEPS {
            let r = ring as i64 * self.snap;
            let offsets: Vec<(i64, i64)> = if ring == 0 {
                vec![(0, 0)]
            } else {
                let mut v = Vec::new();
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    v.push((dx * r, dy * r));
                }
                v
            };
            for (ox, oy) in offsets {
                let at = Point { x: snap(centre.0 + ox, self.snap), y: snap(centre.1 + oy, self.snap) };
                let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
                let Ok(cr) = self.courtyard_at(part, &fp) else { continue };
                if cr.0 < bb.0 || cr.1 < bb.1 || cr.2 > bb.2 || cr.3 > bb.3 {
                    continue;
                }
                if self.collides(part, cr) {
                    continue;
                }
                self.design.placement.as_mut().unwrap().footprints.push(fp);
                self.sort_footprints();
                return Ok(());
            }
        }
        Err(vec![CheckResult::fail(
            "ops_no_room",
            part,
            format!("no collision-free pose in the {} region within {MAX_SLIDE_STEPS} snap steps", region.as_str()),
        )])
    }

    fn place_on_edge(&mut self, part: &str, edge: Dir, fraction: f64) -> Result<(), Vec<CheckResult>> {
        self.require_unplaced(part)?;
        if !(0.0..=1.0).contains(&fraction) {
            return Err(vec![CheckResult::fail("ops_bad_fraction", part, format!("fraction {fraction} is outside 0.0..=1.0"))]);
        }
        let bb = self.board_bbox();
        let probe = FootprintInstance { id: part.into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let c = self.courtyard_at(part, &probe)?;
        let (half_w, half_h) = ((c.2 - c.0) / 2, (c.3 - c.1) / 2);

        let along = |lo: Um, hi: Um, half: Um| -> Um {
            let usable = (hi - half) - (lo + half);
            lo + half + (usable as f64 * fraction) as Um
        };
        let at = match edge {
            Dir::North => Point { x: along(bb.0, bb.2, half_w), y: bb.1 + half_h },
            Dir::South => Point { x: along(bb.0, bb.2, half_w), y: bb.3 - half_h },
            Dir::West => Point { x: bb.0 + half_w, y: along(bb.1, bb.3, half_h) },
            Dir::East => Point { x: bb.2 - half_w, y: along(bb.1, bb.3, half_h) },
        };
        let at = Point { x: snap(at.x, self.snap), y: snap(at.y, self.snap) };
        let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
        let cr = self.courtyard_at(part, &fp)?;
        if self.collides(part, cr) {
            return Err(vec![CheckResult::fail(
                "ops_no_room",
                part,
                format!("the {} edge is occupied at fraction {fraction}", edge.as_str()),
            )]);
        }
        self.design.placement.as_mut().unwrap().footprints.push(fp);
        self.sort_footprints();
        Ok(())
    }

    fn nudge(&mut self, part: &str, dir: Dir, steps: u32) -> Result<(), Vec<CheckResult>> {
        let fp = self.require_placed(part)?;
        let (dx, dy) = dir.delta();
        let d = steps as i64 * self.snap;
        self.set_pose(part, Point { x: fp.at.x + dx * d, y: fp.at.y + dy * d }, fp.rot)
    }

    fn rotate(&mut self, part: &str, quarter_turns: u8) -> Result<(), Vec<CheckResult>> {
        let fp = self.require_placed(part)?;
        let rot = (fp.rot + quarter_turns as u32 * 90_000) % 360_000;
        self.set_pose(part, fp.at, rot)
    }

    fn swap(&mut self, a: &str, b: &str) -> Result<(), Vec<CheckResult>> {
        if a == b {
            return Err(vec![CheckResult::fail("ops_swap_self", a, "a part cannot be swapped with itself")]);
        }
        let fa = self.require_placed(a)?;
        let fb = self.require_placed(b)?;
        self.set_pose(a, fb.at, fa.rot)?;
        self.set_pose(b, fa.at, fb.rot)
    }

    fn rip(&mut self, part: &str) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        self.design.placement.as_mut().unwrap().footprints.retain(|f| f.id != part);
        Ok(())
    }

    fn set_pose(&mut self, part: &str, at: Point, rot: u32) -> Result<(), Vec<CheckResult>> {
        let fps = &mut self.design.placement.as_mut().unwrap().footprints;
        let i = fps.iter().position(|f| f.id == part).expect("caller checked the part is placed");
        fps[i].at = Point { x: snap(at.x, self.snap), y: snap(at.y, self.snap) };
        fps[i].rot = rot;
        Ok(())
    }

    /// Keep the footprint list ordered by refdes.
    ///
    /// The exporters and the design hash both depend on it, so a board
    /// built by commands must serialise the same as one the annealer
    /// produced or two identical layouts would compare unequal.
    fn sort_footprints(&mut self) {
        self.design.placement.as_mut().unwrap().footprints.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

/// How far a `Place` will slide along the anchor before giving up.
const MAX_SLIDE_STEPS: u32 = 60;

fn snap(v: Um, to: Um) -> Um {
    if to <= 0 {
        return v;
    }
    ((v as f64 / to as f64).round() as i64) * to
}

fn overlaps(a: (Um, Um, Um, Um), b: (Um, Um, Um, Um)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

#[cfg(test)]
mod tests;
