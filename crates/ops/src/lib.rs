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

use eda_model::footprint::{placed_courtyard, placed_keepout, placed_pads};
use eda_model::ir::{
    Design, DrawingsSection, FootprintInstance, LabelKind, LabelSide, Millideg, NetLabel, NoConnect, Point, PowerSymbol, RoutingSection, SchematicSection, Shape, Side, SymbolInstance, Text,
    TextJustify, Track, Um, Via, Wire, Zone,
};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    /// Put a part at an explicit board position, µm.
    ///
    /// The other place verbs say *where* symbolically -- a region, an
    /// edge, beside a neighbour -- which is how a layout engineer
    /// speaks and cannot be off by a rounding error. This one is for
    /// when the position is actually known: a part being restored, a
    /// board being replayed, or a caller that has already decided.
    PlaceAt { part: String, x: Um, y: Um },
    /// Move a placed part to a board position, µm, keeping its rotation
    /// and side: what dragging it does. Like `Nudge`, the gates judge
    /// where it lands.
    MoveTo { part: String, x: Um, y: Um },
    Nudge { part: String, dir: Dir, steps: u32 },
    /// Turn a placed part by quarter turns.
    Rotate { part: String, quarter_turns: u8 },
    /// Exchange the positions of two placed parts.
    Swap { a: String, b: String },
    /// Take a placed part off the board, so a different choice can be
    /// made. Backtracking is a first-class move: a constructive placer
    /// that cannot undo paints itself into a corner at part eighty.
    Rip { part: String },
    /// Turn a placed part over to the other side of the board. Pad
    /// positions mirror because `footprint::to_board` reads `side`; the
    /// courtyard does not move (it is symmetric about the origin), so
    /// nothing else about the placement needs rechecking.
    ///
    /// KiCad keeps a part's tracks attached when it moves (ratsnest
    /// aside); this codebase does not model that yet, so a `Flip`, like
    /// every other part edit, clears the board's routing (see
    /// `Cmd::clears_routing` and the CLI's `step`). Deferred, not fixed
    /// here -- see the report.
    Flip { part: String },

    /// Add a hand-drawn copper track. `net` and `layer` are refused if
    /// they do not name a real net / a real copper layer; whether the
    /// track's *path* is any good (wrong net touched, clearance violated)
    /// is left to the routing gates, exactly as it is for the router's own
    /// output -- see the report.
    AddTrack { net: String, layer: String, width: Um, pts: Vec<Point> },
    /// Remove a track by id.
    DeleteTrack { id: String },
    /// Widen or narrow a track in place. Its id, net, layer and path do
    /// not change.
    SetTrackWidth { id: String, width: Um },

    /// Add a via.
    AddVia { net: String, x: Um, y: Um, drill: Um, diameter: Um, from_layer: String, to_layer: String },
    /// Remove a via by id.
    DeleteVia { id: String },
    /// Move a via to a new position. Its id, net, drill, diameter and
    /// layer span do not change.
    MoveVia { id: String, x: Um, y: Um },

    /// Add a copper pour.
    AddZone { net: String, layer: String, outline: Vec<Point> },
    /// Remove a zone by id.
    DeleteZone { id: String },

    /// Add a graphic shape (silkscreen art, fab-layer outlines, ...). The
    /// `id` field of `shape`, if the caller sent one, is ignored -- ids are
    /// assigned here, the same deterministic way as everywhere else.
    AddShape { shape: Shape },
    /// Remove a shape by id.
    DeleteShape { id: String },
    /// Translate a shape by `(dx, dy)` -- what dragging it does. A shape
    /// has no single position the way a via or a text does (its geometry
    /// is two or more points), so the move is a delta, not a destination.
    MoveShape { id: String, dx: Um, dy: Um },

    /// Add free text.
    AddText { text: Text },
    /// Replace a text's content and styling in place. Its id and position
    /// do not change (see `MoveText` for that).
    EditText { id: String, content: String, angle: Millideg, layer: String, size_um: Um, stroke_width: Um, justify: TextJustify, mirror: bool },
    /// Remove a text by id.
    DeleteText { id: String },
    /// Move a text to a new position, keeping its id and styling.
    MoveText { id: String, x: Um, y: Um },

    /// Copy existing tracks/vias/zones/shapes/texts named by id, in
    /// place (same position, fresh ids) -- the studio's Cmd+D. Footprints
    /// are deliberately not supported: duplicating one would add a part
    /// instance the intent/BOM does not have, which needs a human
    /// decision this verb cannot make on its own (see the report). An id
    /// naming a footprint, or anything else this can't duplicate, is
    /// simply not among the ones found -- only refused if NONE of the
    /// given ids match anything duplicable.
    Duplicate { ids: Vec<String> },
    /// Insert fresh copies of whole tracks/vias/zones/shapes/texts
    /// (ids ignored and reassigned, same as `AddShape`/`AddText`) --
    /// the studio's Cmd+V. Unlike `Duplicate` (which looks up existing
    /// board items by id), the pasted items' full data travels with the
    /// command itself, so paste still works after the original was
    /// deleted, or even against a different board than the one they were
    /// copied from.
    PasteItems {
        #[serde(default)]
        tracks: Vec<Track>,
        #[serde(default)]
        vias: Vec<Via>,
        #[serde(default)]
        zones: Vec<Zone>,
        #[serde(default)]
        shapes: Vec<Shape>,
        #[serde(default)]
        texts: Vec<Text>,
    },

    /// Move/rotate one or more placed parts by an exact cartesian offset
    /// and angle -- pcbnew's "Move Exactly..." (Shift+M) dialog.
    /// `rotate_millideg` adds to each part's own orientation regardless
    /// of `pivot`; `pivot` is `None` to rotate a part in place around its
    /// own (already-translated) anchor -- a pure spin, no further
    /// position change, exactly like `Rotate` -- or `Some(point)` to
    /// rotate around a shared point instead (dialog's "selection center"/
    /// "local coordinates origin" anchor choices), which the caller
    /// resolves to an actual board point since this crate has no
    /// selection/UI-origin concept of its own.
    MoveExact { parts: Vec<String>, dx: Um, dy: Um, rotate_millideg: i64, pivot: Option<Point> },

    // ---------------------------------------------------------- eeschema
    //
    // Mirrors the PCB verbs above one-for-one where the shape allows
    // (MoveSymbol/RotateSymbol ~ MoveTo/Rotate), schematic-specific where
    // it does not. Every one of these is a `Domain::Schematic` command
    // (see `Cmd::domain`) -- `step_quiet` reconciles `design.schematic`'s
    // own connectivity (and, through it, `design.nets`) after any of them
    // lands, so a wire/label edit's effect on "what's on what net" is
    // never a separate step the caller has to remember to take.
    /// `M`: move a symbol to an absolute sheet position, keeping rotation/
    /// mirror. A wire's endpoint is a bare coordinate, not a reference to
    /// the pin it happens to land on (see `Wire::pts`), so moving the
    /// symbol out from under it is *exactly* what "breaks the connection"
    /// means here -- no separate bookkeeping, the next connectivity pass
    /// just finds nothing at that point any more.
    MoveSymbol { id: String, x: Um, y: Um },
    /// `G`: move a symbol AND drag along the endpoint of every wire
    /// presently attached to one of its pins (KiCad's rubber-band) --
    /// `attached_wire_endpoints` names which (wire index, endpoint index)
    /// pairs the caller found glued to this symbol before the drag
    /// started, so this moves exactly those points by the same delta
    /// rather than re-deriving attachment from the new position.
    DragSymbol { id: String, x: Um, y: Um, attached_wire_endpoints: Vec<(usize, usize)> },
    /// `R`: quarter turns, same convention as `Rotate`.
    RotateSymbol { id: String, quarter_turns: u8 },
    /// `X` ("Mirror Horizontally", KiCad's `SYM_MIRROR_Y` / negate-X):
    /// toggles `SymbolInstance::mirrored`, the one axis this project's
    /// schematic IR currently models (confirmed against
    /// `transform_local_point`: `mirrored` already negates X, matching
    /// this hotkey exactly). `Y` ("Mirror Vertically" / negate-Y) has no
    /// field to toggle yet -- see `PARITY-sch.md`'s eeschema row for that
    /// gap; left unwired rather than overloading this one flag with a
    /// second meaning.
    MirrorSymbol { id: String },
    /// `Del` on a symbol: removes the instance (and its synthesized
    /// `Part`, if `reconcile_schematic` had added one for a symbol with no
    /// intent counterpart) from the sheet. Matches real eeschema: wires
    /// that landed on this symbol's pins are left exactly where they are,
    /// now dangling -- ERC's existing dangling-wire checks are what
    /// surfaces that, not a cascade delete.
    DeleteSymbol { id: String },

    /// `W`: add a hand-drawn wire. `net`/`pins` are left for the next
    /// connectivity reconciliation to fill in (same as a freshly-imported
    /// `.kicad_sch`'s wires start out) -- the caller only knows geometry.
    /// Junction dots are not a stored item at all (eeschema's own
    /// `(junction ...)` record is purely cosmetic -- a T-intersection
    /// reconciles into one net with or without the dot, see
    /// `eda_kicad::sch_import::reconcile`'s own doc): the renderer draws
    /// one wherever 3+ wire endpoints/segments meet, computed fresh from
    /// `wires` every paint, same as a real schematic's junction dots are
    /// implied by geometry, not placed by hand in this port.
    AddWire { pts: Vec<Point> },
    /// Backspace mid-draw is purely a frontend undo of the in-progress
    /// polyline (never reaches the backend at all); this is `Del` on an
    /// already-committed wire, by its `Wire::id`.
    DeleteWire { id: String },
    /// `Q`: no-connect flag at a point (normally a pin's own position --
    /// the caller resolves that, this just records the flag). `pin` is
    /// left blank for the next reconciliation to fill in, same as
    /// `AddWire`'s `net`/`pins`.
    AddNoConnect { at: Point },
    DeleteNoConnect { id: String },

    /// `L`/`Ctrl+L`/`H`: place a net label. `kind`/`shape` follow
    /// `eda_model::ir::LabelKind`'s own vocabulary (local has no shape).
    AddLabel { net: String, at: Point, kind: LabelKind },
    DeleteLabel { id: String },
    /// `P`: place a power symbol on a pin.
    AddPowerSymbol { lib_id: String, at: Point, rot_millideg: Millideg, net: String, pin: String },
    DeletePowerSymbol { id: String },

    /// `A`: place a new symbol instance from the library -- the one verb
    /// here that can grow the part list itself (`reconcile_schematic`
    /// synthesizes a `Part` for an `id` with no intent counterpart), so it
    /// shows up unplaced on the PCB tab exactly like any other part
    /// `eda board new` never heard of.
    AddSymbol { id: String, lib_id: String, at: Point, rot_millideg: Millideg, value: String, footprint: String },

    /// `Ctrl+A` (Annotate): assign reference designators to every
    /// not-yet-annotated symbol (an `id` this project synthesizes as
    /// `"U?1"`/`"U?2"`/... when `AddSymbol` is given a blank prefix+number
    /// -- see `sch_drawing_tools`' own symbol-chooser flow), in `order`
    /// (top-to-bottom sheet position, left-to-right as the tiebreak,
    /// matching `dialog_annotate.cpp`'s default). `reset_existing` mirrors
    /// the dialog's "Clear and re-annotate" vs "Keep existing" modes.
    Annotate { reset_existing: bool },
}

/// Which editor a `Cmd` belongs to -- `eeschema`'s `design.schematic`, or
/// everything pcbnew touches (`placement`/`routing`/`drawings`). Two uses:
/// `crates/cli/src/board.rs`'s undo/redo scopes a stack entry by this (see
/// its own doc comment on why pressing Ctrl+Z on the Schematic tab used to
/// silently undo a PCB edit -- GAPS.md #15), and `step_quiet` only runs
/// `reconcile_schematic` after a `Schematic` command, so a PCB-only board
/// (nothing in this project's own test/parity corpus has ever executed a
/// schematic `Cmd`) never pays for or risks that pass at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Pcb,
    Schematic,
}

impl Cmd {
    /// Which editor this command belongs to -- see [`Domain`].
    pub fn domain(&self) -> Domain {
        match self {
            Cmd::MoveSymbol { .. }
            | Cmd::DragSymbol { .. }
            | Cmd::RotateSymbol { .. }
            | Cmd::MirrorSymbol { .. }
            | Cmd::DeleteSymbol { .. }
            | Cmd::AddWire { .. }
            | Cmd::DeleteWire { .. }
            | Cmd::AddNoConnect { .. }
            | Cmd::DeleteNoConnect { .. }
            | Cmd::AddLabel { .. }
            | Cmd::DeleteLabel { .. }
            | Cmd::AddPowerSymbol { .. }
            | Cmd::DeletePowerSymbol { .. }
            | Cmd::AddSymbol { .. }
            | Cmd::Annotate { .. } => Domain::Schematic,
            _ => Domain::Pcb,
        }
    }
}

impl Cmd {
    /// The parts this command touches, for logging and credit assignment.
    pub fn subjects(&self) -> Vec<&str> {
        match self {
            Cmd::Place { part, anchor, .. } => vec![part, anchor],
            Cmd::PlaceEdge { part, .. }
            | Cmd::PlaceRegion { part, .. }
            | Cmd::PlaceAt { part, .. }
            | Cmd::MoveTo { part, .. }
            | Cmd::Nudge { part, .. }
            | Cmd::Rotate { part, .. }
            | Cmd::Rip { part }
            | Cmd::Flip { part } => vec![part],
            Cmd::Swap { a, b } => vec![a, b],
            Cmd::AddTrack { net, .. } | Cmd::AddVia { net, .. } | Cmd::AddZone { net, .. } => vec![net.as_str()],
            Cmd::DeleteTrack { id }
            | Cmd::SetTrackWidth { id, .. }
            | Cmd::DeleteVia { id }
            | Cmd::MoveVia { id, .. }
            | Cmd::DeleteZone { id }
            | Cmd::DeleteShape { id }
            | Cmd::MoveShape { id, .. }
            | Cmd::EditText { id, .. }
            | Cmd::DeleteText { id }
            | Cmd::MoveText { id, .. } => vec![id.as_str()],
            Cmd::AddShape { shape } => vec![shape.layer()],
            Cmd::AddText { text } => vec![text.content.as_str()],
            Cmd::Duplicate { ids } => ids.iter().map(String::as_str).collect(),
            Cmd::PasteItems { .. } => vec!["paste"],
            Cmd::MoveExact { parts, .. } => parts.iter().map(String::as_str).collect(),

            Cmd::MoveSymbol { id, .. } | Cmd::DragSymbol { id, .. } | Cmd::RotateSymbol { id, .. } | Cmd::MirrorSymbol { id } | Cmd::DeleteSymbol { id } | Cmd::AddSymbol { id, .. } => vec![id],
            Cmd::AddWire { .. } => vec!["wire"],
            Cmd::DeleteWire { id } | Cmd::DeleteNoConnect { id } | Cmd::DeleteLabel { id } | Cmd::DeletePowerSymbol { id } => vec![id],
            Cmd::AddNoConnect { .. } => vec!["no_connect"],
            Cmd::AddLabel { net, .. } => vec![net.as_str()],
            Cmd::AddPowerSymbol { net, .. } => vec![net.as_str()],
            Cmd::Annotate { .. } => vec!["annotate"],
        }
    }

    /// Whether this command can leave the board's routing stale. True for
    /// anything that moves, flips or removes a part -- exactly the
    /// commands the CLI's `step` already cleared routing for -- and false
    /// for every copper/drawing command, which by construction only adds,
    /// deletes or restyles the copper/graphics themselves and never moves
    /// a part out from under them.
    pub fn clears_routing(&self) -> bool {
        matches!(
            self,
            Cmd::Place { .. }
                | Cmd::PlaceEdge { .. }
                | Cmd::PlaceRegion { .. }
                | Cmd::PlaceAt { .. }
                | Cmd::MoveTo { .. }
                | Cmd::Nudge { .. }
                | Cmd::Rotate { .. }
                | Cmd::Swap { .. }
                | Cmd::Rip { .. }
                | Cmd::Flip { .. }
                | Cmd::MoveExact { .. }
        )
    }
}

/// A board under construction.
pub struct Board<'a> {
    model: &'a ConstraintModel,
    design: Design,
    snap: Um,
    /// Minimum gap left between courtyards when resolving a `Place`.
    spacing: Um,
    /// [`unruled_decoupling_pairs`] both ways round, computed once: every
    /// neighbour query reads it.
    decoupling: Arc<BTreeMap<String, BTreeSet<String>>>,
}

/// Decoupling pairs `(cap, ic)` that no proximity rule already places --
/// exactly the caps the decoupling gate judges, since a cap the intent
/// ties to a part with a rule is judged by that rule instead.
pub fn unruled_decoupling_pairs(model: &ConstraintModel) -> Vec<(String, String)> {
    let ruled = |c: &str| {
        model.placement_rules.iter().any(|r| matches!(r, eda_model::PlacementRule::Proximity { a, b, .. } if a == c || b == c))
    };
    eda_model::decoupling_pairs(model).into_iter().filter(|(c, _)| !ruled(c)).collect()
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
        let mut decoupling: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (cap, ic) in unruled_decoupling_pairs(model) {
            decoupling.entry(cap.clone()).or_default().insert(ic.clone());
            decoupling.entry(ic).or_default().insert(cap);
        }
        Board { model, design, snap, spacing, decoupling: Arc::new(decoupling) }
    }

    pub fn design(&self) -> &Design {
        &self.design
    }

    pub fn model(&self) -> &'a ConstraintModel {
        self.model
    }

    /// A copy to try a command against without committing to it.
    ///
    /// A chooser has to see what each candidate would actually do --
    /// predicting it from a second model of the geometry is how the
    /// optimiser and the gates drifted apart everywhere else in this
    /// project.
    pub fn fork(&self) -> Board<'a> {
        Board {
            model: self.model,
            design: self.design.clone(),
            snap: self.snap,
            spacing: self.spacing,
            decoupling: Arc::clone(&self.decoupling),
        }
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

    /// The frontier, restricted to a set of parts.
    ///
    /// Used to finish one functional block before starting the next. A
    /// block placed as a unit stays together; a board built by taking
    /// whatever part happens to be adjacent lets a regulator's inductor
    /// drift away from its own capacitors, because "adjacent to
    /// something already down" says nothing about belonging to the same
    /// circuit.
    pub fn frontier_within(&self, allowed: &BTreeSet<String>) -> Vec<String> {
        self.frontier().into_iter().filter(|p| allowed.contains(p)).collect()
    }

    /// Parts sharing a small net, a proximity rule or a decoupling pair
    /// with `r`.
    ///
    /// Power and ground are excluded by the net-size cut: a 40-pin GND
    /// net makes every part everyone's neighbour and the frontier stops
    /// meaning anything. Which is why decoupling pairs are added back by
    /// name: a decoupling cap sits on nothing but a rail and ground, so
    /// the cut left it with no neighbour at all -- never on the frontier,
    /// never anchored to its IC, just seeded wherever a region had room.
    /// On mcu_board_30plus that put all twelve 7 to 13 mm from the MCU.
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
        if let Some(partners) = self.decoupling.get(r) {
            out.extend(partners.iter().cloned());
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

    /// The search's own ranking key for this board state: `(fail count,
    /// warn count)`. Used instead of a bare `failures()` wherever a
    /// constructive search picks the best of several candidates, so a
    /// tie on fail count still prefers the candidate with fewer/lesser
    /// warnings before falling through to wirelength and a stable id
    /// (see `crates/ops/src/build.rs`'s `best_pose`/`best_pose_within`/
    /// `Greedy::choose_part`/`best_edge_pose`) -- rather than whatever
    /// incidental order the underlying check engine happened to return
    /// its results in. That stability matters specifically because
    /// `check_placement_partial` is re-run on a narrowed, *partial*
    /// design at every step: two different (but equally valid)
    /// implementations of the same check can disagree by a warning or
    /// two on an unfinished board without either being wrong, and a
    /// search that only compared fail counts could tip a close call
    /// differently depending on which implementation was plugged in.
    /// Computed from one `checks()` call, not two, so this costs nothing
    /// extra over the old `failures()` at these hot call sites.
    pub fn search_rank(&self) -> (usize, usize) {
        let checks = self.checks();
        let fails = checks.iter().filter(|c| matches!(c.status, CheckStatus::Fail)).count();
        let warns = checks.iter().filter(|c| matches!(c.status, CheckStatus::Warn)).count();
        (fails, warns)
    }

    /// Apply a command, or refuse it.
    pub fn apply(&mut self, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            Cmd::Place { part, anchor, side } => self.place_beside(part, anchor, *side),
            Cmd::PlaceEdge { part, edge, fraction } => self.place_on_edge(part, *edge, *fraction),
            Cmd::PlaceRegion { part, region } => self.place_in_region(part, *region),
            Cmd::PlaceAt { part, x, y } => self.place_at(part, *x, *y),
            Cmd::MoveTo { part, x, y } => {
                let fp = self.require_placed(part)?;
                self.set_pose(part, Point { x: *x, y: *y }, fp.rot)
            }
            Cmd::Nudge { part, dir, steps } => self.nudge(part, *dir, *steps),
            Cmd::Rotate { part, quarter_turns } => self.rotate(part, *quarter_turns),
            Cmd::Swap { a, b } => self.swap(a, b),
            Cmd::Rip { part } => self.rip(part),
            Cmd::Flip { part } => self.flip(part),

            Cmd::AddTrack { net, layer, width, pts } => self.add_track(net, layer, *width, pts),
            Cmd::DeleteTrack { id } => self.delete_track(id),
            Cmd::SetTrackWidth { id, width } => self.set_track_width(id, *width),

            Cmd::AddVia { net, x, y, drill, diameter, from_layer, to_layer } => self.add_via(net, *x, *y, *drill, *diameter, from_layer, to_layer),
            Cmd::DeleteVia { id } => self.delete_via(id),
            Cmd::MoveVia { id, x, y } => self.move_via(id, *x, *y),

            Cmd::AddZone { net, layer, outline } => self.add_zone(net, layer, outline),
            Cmd::DeleteZone { id } => self.delete_zone(id),

            Cmd::AddShape { shape } => self.add_shape(shape.clone()),
            Cmd::DeleteShape { id } => self.delete_shape(id),
            Cmd::MoveShape { id, dx, dy } => self.move_shape(id, *dx, *dy),

            Cmd::AddText { text } => self.add_text(text.clone()),
            Cmd::EditText { id, content, angle, layer, size_um, stroke_width, justify, mirror } => {
                self.edit_text(id, content.clone(), *angle, layer.clone(), *size_um, *stroke_width, *justify, *mirror)
            }
            Cmd::DeleteText { id } => self.delete_text(id),
            Cmd::MoveText { id, x, y } => self.move_text(id, *x, *y),

            Cmd::Duplicate { ids } => self.duplicate_items(ids),
            Cmd::PasteItems { tracks, vias, zones, shapes, texts } => self.insert_copies(tracks.clone(), vias.clone(), zones.clone(), shapes.clone(), texts.clone()),
            Cmd::MoveExact { parts, dx, dy, rotate_millideg, pivot } => self.move_exact(parts, *dx, *dy, *rotate_millideg, *pivot),

            Cmd::MoveSymbol { id, x, y } => self.move_symbol(id, *x, *y),
            Cmd::DragSymbol { id, x, y, attached_wire_endpoints } => self.drag_symbol(id, *x, *y, attached_wire_endpoints),
            Cmd::RotateSymbol { id, quarter_turns } => self.rotate_symbol(id, *quarter_turns),
            Cmd::MirrorSymbol { id } => self.mirror_symbol(id),
            Cmd::DeleteSymbol { id } => self.delete_symbol(id),
            Cmd::AddWire { pts } => self.add_wire(pts.clone()),
            Cmd::DeleteWire { id } => self.delete_wire(id),
            Cmd::AddNoConnect { at } => self.add_no_connect(*at),
            Cmd::DeleteNoConnect { id } => self.delete_no_connect(id),
            Cmd::AddLabel { net, at, kind } => self.add_label(net, *at, kind.clone()),
            Cmd::DeleteLabel { id } => self.delete_label(id),
            Cmd::AddPowerSymbol { lib_id, at, rot_millideg, net, pin } => self.add_power_symbol(lib_id, *at, *rot_millideg, net, pin),
            Cmd::DeletePowerSymbol { id } => self.delete_power_symbol(id),
            Cmd::AddSymbol { id, lib_id, at, rot_millideg, value, footprint } => self.add_symbol(id, lib_id, *at, *rot_millideg, value, footprint),
            Cmd::Annotate { reset_existing } => self.annotate(*reset_existing),
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

    /// The rectangle a candidate must actually keep clear: its courtyard
    /// *plus* its refdes label box.
    ///
    /// Testing a bare courtyard against the neighbours' keepouts is
    /// asymmetric, and the asymmetry is exactly the size of a refdes
    /// label -- which is why the first real build produced six
    /// `placement_refdes_clear` failures on a board the annealer clears.
    /// The gate judges label against courtyard, so the placer has to as
    /// well.
    fn keepout_at(&self, r: &str, fp: &FootprintInstance) -> Result<(Um, Um, Um, Um), Vec<CheckResult>> {
        let p = self.part(r)?;
        placed_keepout(self.model, &self.placement().outline, p, fp)
            .ok_or_else(|| vec![CheckResult::fail("ops_no_footprint", r, "the part has no resolvable footprint, so it has no keepout to place")])
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
                if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
                    continue; // a corner on the boundary is not inside it
                }
                let Ok(kr) = self.keepout_at(part, &fp) else { continue };
                if self.collides(part, kr) {
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
                if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
                    continue; // a corner on the boundary is not inside it
                }
                let Ok(kr) = self.keepout_at(part, &fp) else { continue };
                if self.collides(part, kr) {
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
        // Turn the part so its long side runs *along* the edge.
        //
        // Without this a 40mm sixteen-pin header asked for the west edge
        // is laid across the board instead of down it: its left side
        // does touch the edge, but it juts 40mm into a 60mm board and
        // fails `placement_edge_connector` anyway. The annealer's own
        // seeding has always done this; leaving it out here was worth
        // one gate failure on L1 and no clue as to why.
        let flat = FootprintInstance { id: part.into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let c0 = self.courtyard_at(part, &flat)?;
        let wider_than_tall = (c0.2 - c0.0) >= (c0.3 - c0.1);
        let horizontal_edge = matches!(edge, Dir::North | Dir::South);
        let rot: u32 = if horizontal_edge == wider_than_tall { 0 } else { 90_000 };

        let probe = FootprintInstance { id: part.into(), at: Point { x: 0, y: 0 }, rot, side: Side::Top, label: LabelSide::Above };
        let c = self.courtyard_at(part, &probe)?;
        let (half_w, half_h) = ((c.2 - c.0) / 2, (c.3 - c.1) / 2);

        let along = |lo: Um, hi: Um, half: Um| -> Um {
            let usable = (hi - half) - (lo + half);
            lo + half + (usable as f64 * fraction) as Um
        };
        // Sit one snap step inside the edge, not exactly on it.
        //
        // `placement_within_outline` tests the courtyard's corners with
        // a point-in-polygon, and a corner lying exactly on the boundary
        // is not inside it. Placed flush, a connector's courtyard ends
        // at x == the board's right edge and fails, which is what put
        // seven of L3's ten failures there. The annealer never hits this
        // because legalisation nudges everything inward by a snap. One
        // step is far inside the 1500µm an edge connector is allowed to
        // sit from its edge, so this costs nothing.
        //
        // And far enough that the pads' copper keeps the edge clearance:
        // KiCad measures copper to Edge.Cuts, and a header's pads sit a
        // few tenths inside its courtyard -- 0.35 mm on L1, where 0.5 mm
        // is the rule.
        let step = self.snap.max(1);
        let clearance = self.model.board.tuning.copper_edge_clearance();
        let pads = placed_pads(self.model, self.part(part)?, &probe).unwrap_or_default();
        let copper = pads.iter().fold((Um::MAX, Um::MAX, Um::MIN, Um::MIN), |r, p| {
            (r.0.min(p.center.x - p.size.0 / 2), r.1.min(p.center.y - p.size.1 / 2), r.2.max(p.center.x + p.size.0 / 2), r.3.max(p.center.y + p.size.1 / 2))
        });
        // Per side, how much further in than its courtyard the part must
        // sit for that: whole snap steps, plus one for the snapping below,
        // and nothing where the pads clear it anyway. The ends count too:
        // a row put near a corner has its end pad by the other edge.
        let room = |gap: Um| -> Um {
            let short = clearance - gap;
            if short <= 0 { 0 } else { (short + step - 1) / step * step + step }
        };
        let (w, n, e, s) = if pads.is_empty() { (0, 0, 0, 0) } else { (room(copper.0 - c.0), room(copper.1 - c.1), room(c.2 - copper.2), room(c.3 - copper.3)) };
        let at = match edge {
            Dir::North => Point { x: along(bb.0 + w, bb.2 - e, half_w), y: bb.1 + half_h + step.max(n) },
            Dir::South => Point { x: along(bb.0 + w, bb.2 - e, half_w), y: bb.3 - half_h - step.max(s) },
            Dir::West => Point { x: bb.0 + half_w + step.max(w), y: along(bb.1 + n, bb.3 - s, half_h) },
            Dir::East => Point { x: bb.2 - half_w - step.max(e), y: along(bb.1 + n, bb.3 - s, half_h) },
        };
        let at = Point { x: snap(at.x, self.snap), y: snap(at.y, self.snap) };
        let fp = FootprintInstance { id: part.into(), at, rot, side: Side::Top, label: LabelSide::Above };
        // A part longer than the edge it was given does not fit on it,
        // and saying so is the whole point. The other two resolvers
        // already bound themselves to the outline; this one did not, so
        // a 40.3mm header went onto a 40mm edge and failed
        // `placement_within_outline` instead of being refused here.
        let cy = self.courtyard_at(part, &fp)?;
        if cy.0 < bb.0 || cy.1 < bb.1 || cy.2 > bb.2 || cy.3 > bb.3 {
            return Err(vec![CheckResult::fail(
                "ops_no_room",
                part,
                format!(
                    "does not fit the {} edge: it needs {}x{} µm and the board is {}x{}",
                    edge.as_str(),
                    cy.2 - cy.0,
                    cy.3 - cy.1,
                    bb.2 - bb.0,
                    bb.3 - bb.1
                ),
            )]);
        }
        let cr = self.keepout_at(part, &fp)?;
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

    /// Place a part at a given point, refusing it if it will not fit.
    ///
    /// Unlike the symbolic verbs this does not search: a caller naming a
    /// position is taken at their word, and a position that collides or
    /// leaves the outline is refused rather than quietly slid somewhere
    /// nearby. A command that silently does something other than what it
    /// says cannot be learned from.
    fn place_at(&mut self, part: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        self.require_unplaced(part)?;
        let bb = self.board_bbox();
        let at = Point { x: snap(x, self.snap), y: snap(y, self.snap) };
        let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
        let cr = self.courtyard_at(part, &fp)?;
        if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
            return Err(vec![CheckResult::fail(
                "ops_outside_board",
                part,
                "that position puts the part's courtyard on or past the board edge",
            )]);
        }
        let kr = self.keepout_at(part, &fp)?;
        if self.collides(part, kr) {
            return Err(vec![CheckResult::fail(
                "ops_occupied",
                part,
                "another part's keepout already covers that position",
            )]);
        }
        self.design.placement.as_mut().expect("board has a placement").footprints.push(fp);
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

    // ---------------------------------------------------------- flip

    fn flip(&mut self, part: &str) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        let fps = &mut self.design.placement.as_mut().unwrap().footprints;
        let i = fps.iter().position(|f| f.id == part).expect("caller checked the part is placed");
        fps[i].side = if fps[i].side == Side::Top { Side::Bottom } else { Side::Top };
        Ok(())
    }

    // --------------------------------------------------------- copper

    fn known_net(&self, net: &str) -> Result<(), Vec<CheckResult>> {
        if self.model.nets.iter().any(|n| n.name == net) {
            Ok(())
        } else {
            Err(vec![CheckResult::fail("ops_unknown_net", net, "no net with this name exists in the model")])
        }
    }

    fn known_layer(&self, layer: &str) -> Result<(), Vec<CheckResult>> {
        if self.model.board.layers.iter().any(|l| l == layer) {
            Ok(())
        } else {
            Err(vec![CheckResult::fail(
                "ops_unknown_layer",
                layer,
                format!("layer is not one of the board's copper layers {:?}", self.model.board.layers),
            )])
        }
    }

    /// The board's routing section, creating an empty one if this is the
    /// first hand-added piece of copper. Unlike a part edit, adding copper
    /// must never clear an existing routing section -- see
    /// `Cmd::clears_routing`.
    fn routing_mut(&mut self) -> &mut RoutingSection {
        self.design.routing.get_or_insert_with(|| RoutingSection { tracks: vec![], vias: vec![], zones: vec![] })
    }

    /// The board's drawings section, creating an empty one on first use.
    fn drawings_mut(&mut self) -> &mut DrawingsSection {
        self.design.drawings.get_or_insert_with(DrawingsSection::default)
    }

    /// Keep tracks and vias in the same canonical order the router and
    /// `Design::canonical_bytes` use, so a hand-edited board serialises
    /// the same way a freshly routed one does.
    fn sort_routing(&mut self) {
        if let Some(rt) = self.design.routing.as_mut() {
            rt.tracks.sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
            rt.vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
        }
    }

    fn add_track(&mut self, net: &str, layer: &str, width: Um, pts: &[Point]) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(layer)?;
        if width <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_track", net, "track width must be positive")]);
        }
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_track", net, "a track needs at least two points")]);
        }
        let rt = self.routing_mut();
        rt.tracks.push(Track { id: String::new(), net: net.into(), pins: vec![], layer: layer.into(), width, pts: pts.to_vec() });
        rt.assign_missing_ids();
        self.sort_routing();
        Ok(())
    }

    fn delete_track(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "the board has no routing yet")])?;
        let before = rt.tracks.len();
        rt.tracks.retain(|t| t.id != id);
        if rt.tracks.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_track", id, "no track with this id")]);
        }
        Ok(())
    }

    fn set_track_width(&mut self, id: &str, width: Um) -> Result<(), Vec<CheckResult>> {
        if width <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_track", id, "track width must be positive")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "the board has no routing yet")])?;
        let t = rt.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "no track with this id")])?;
        t.width = width;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_via(&mut self, net: &str, x: Um, y: Um, drill: Um, diameter: Um, from_layer: &str, to_layer: &str) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(from_layer)?;
        self.known_layer(to_layer)?;
        if from_layer == to_layer {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "a via needs two different layers to span")]);
        }
        if drill <= 0 || diameter <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "via drill and diameter must be positive")]);
        }
        if drill >= diameter {
            return Err(vec![CheckResult::fail(
                "ops_bad_via",
                net,
                format!("drill {drill} um is not smaller than the {diameter} um pad, so the via has no annular ring"),
            )]);
        }
        let rt = self.routing_mut();
        rt.vias.push(Via { id: String::new(), net: net.into(), at: Point { x, y }, drill, diameter, from_layer: from_layer.into(), to_layer: to_layer.into() });
        rt.assign_missing_ids();
        self.sort_routing();
        Ok(())
    }

    fn delete_via(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "the board has no routing yet")])?;
        let before = rt.vias.len();
        rt.vias.retain(|v| v.id != id);
        if rt.vias.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_via", id, "no via with this id")]);
        }
        Ok(())
    }

    fn move_via(&mut self, id: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "the board has no routing yet")])?;
        let v = rt.vias.iter_mut().find(|v| v.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "no via with this id")])?;
        v.at = Point { x, y };
        Ok(())
    }

    fn add_zone(&mut self, net: &str, layer: &str, outline: &[Point]) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(layer)?;
        if outline.len() < 3 {
            return Err(vec![CheckResult::fail("ops_bad_zone", net, "a zone outline needs at least three points")]);
        }
        let rt = self.routing_mut();
        rt.zones.push(Zone { id: String::new(), net: net.into(), layer: layer.into(), outline: outline.to_vec(), ..Default::default() });
        rt.assign_missing_ids();
        Ok(())
    }

    fn delete_zone(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let before = rt.zones.len();
        rt.zones.retain(|z| z.id != id);
        if rt.zones.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")]);
        }
        Ok(())
    }

    // -------------------------------------------------------- drawings

    fn add_shape(&mut self, mut shape: Shape) -> Result<(), Vec<CheckResult>> {
        if shape.layer().is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", "shape", "a shape needs a layer")]);
        }
        if let Shape::Polygon { pts, .. } = &shape {
            if pts.len() < 3 {
                return Err(vec![CheckResult::fail("ops_bad_shape", "shape", "a polygon needs at least three points")]);
            }
        }
        shape.set_id(String::new()); // ids are ours to assign, never the caller's
        let dr = self.drawings_mut();
        dr.shapes.push(shape);
        dr.assign_missing_ids();
        Ok(())
    }

    fn delete_shape(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "the board has no drawings yet")])?;
        let before = dr.shapes.len();
        dr.shapes.retain(|s| s.id() != id);
        if dr.shapes.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")]);
        }
        Ok(())
    }

    fn move_shape(&mut self, id: &str, dx: Um, dy: Um) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "the board has no drawings yet")])?;
        let s = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])?;
        s.translate(dx, dy);
        Ok(())
    }

    fn add_text(&mut self, mut text: Text) -> Result<(), Vec<CheckResult>> {
        if text.layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", "text", "text needs a layer")]);
        }
        if text.size_um <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_text", "text", "text size must be positive")]);
        }
        text.id = String::new();
        let dr = self.drawings_mut();
        dr.texts.push(text);
        dr.assign_missing_ids();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_text(&mut self, id: &str, content: String, angle: Millideg, layer: String, size_um: Um, stroke_width: Um, justify: TextJustify, mirror: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text needs a layer")]);
        }
        if size_um <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text size must be positive")]);
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let t = dr.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.content = content;
        t.angle = angle;
        t.layer = layer;
        t.size_um = size_um;
        t.stroke_width = stroke_width;
        t.justify = justify;
        t.mirror = mirror;
        Ok(())
    }

    fn delete_text(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let before = dr.texts.len();
        dr.texts.retain(|t| t.id != id);
        if dr.texts.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")]);
        }
        Ok(())
    }

    fn move_text(&mut self, id: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let t = dr.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.at = Point { x, y };
        Ok(())
    }

    // ------------------------------------------------- duplicate / paste

    /// `Cmd::Duplicate`: resolve each id against whichever collection
    /// actually has it (a track, via, zone, shape or text -- never a
    /// footprint, which has no match in any of these and so is simply
    /// skipped, not specially detected) and hand the found copies to
    /// `insert_copies`.
    fn duplicate_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_duplicate", "duplicate", "no ids given")]);
        }

        let mut tracks = Vec::new();
        let mut vias = Vec::new();
        let mut zones = Vec::new();
        if let Some(rt) = self.design.routing.as_ref() {
            tracks.extend(rt.tracks.iter().filter(|t| ids.iter().any(|id| id == &t.id)).cloned());
            vias.extend(rt.vias.iter().filter(|v| ids.iter().any(|id| id == &v.id)).cloned());
            zones.extend(rt.zones.iter().filter(|z| ids.iter().any(|id| id == &z.id)).cloned());
        }
        let mut shapes = Vec::new();
        let mut texts = Vec::new();
        if let Some(dr) = self.design.drawings.as_ref() {
            shapes.extend(dr.shapes.iter().filter(|s| ids.iter().any(|id| id == s.id())).cloned());
            texts.extend(dr.texts.iter().filter(|t| ids.iter().any(|id| id == &t.id)).cloned());
        }

        if tracks.is_empty() && vias.is_empty() && zones.is_empty() && shapes.is_empty() && texts.is_empty() {
            return Err(vec![CheckResult::fail(
                "ops_unknown_duplicate",
                "duplicate",
                "none of the given ids name a track, via, zone, shape or text (footprints cannot be duplicated this way)",
            )]);
        }

        self.insert_copies(tracks, vias, zones, shapes, texts)
    }

    /// Shared by `Duplicate` (copies resolved from existing ids) and
    /// `PasteItems` (copies that travelled with the command): blank every
    /// incoming id -- never trust a caller's or a stale copy's id, same
    /// rule `add_shape`/`add_track`/etc. already follow -- insert, and
    /// assign fresh deterministic ones the same way a brand new item
    /// would get.
    fn insert_copies(&mut self, mut tracks: Vec<Track>, mut vias: Vec<Via>, mut zones: Vec<Zone>, mut shapes: Vec<Shape>, mut texts: Vec<Text>) -> Result<(), Vec<CheckResult>> {
        for t in &mut tracks {
            t.id.clear();
        }
        for v in &mut vias {
            v.id.clear();
        }
        for z in &mut zones {
            z.id.clear();
        }
        for s in &mut shapes {
            s.set_id(String::new());
        }
        for t in &mut texts {
            t.id.clear();
        }

        if !tracks.is_empty() || !vias.is_empty() || !zones.is_empty() {
            let rt = self.routing_mut();
            rt.tracks.append(&mut tracks);
            rt.vias.append(&mut vias);
            rt.zones.append(&mut zones);
            rt.assign_missing_ids();
            self.sort_routing();
        }
        if !shapes.is_empty() || !texts.is_empty() {
            let dr = self.drawings_mut();
            dr.shapes.append(&mut shapes);
            dr.texts.append(&mut texts);
            dr.assign_missing_ids();
        }
        Ok(())
    }

    // ----------------------------------------------------- move exact

    /// `Cmd::MoveExact`: translate by `(dx, dy)`, then rotate by
    /// `rotate_millideg` around `pivot` (or, if `None`, around the part's
    /// own just-translated anchor -- a pure in-place spin, since rotating
    /// a point about itself cannot move it). Every part moves/rotates by
    /// the SAME translation and angle, matching source's dialog (one
    /// shared delta/angle for the whole selection); each part's `pivot`,
    /// when given, is still the one shared point (the caller resolves
    /// "selection center" to a single coordinate before sending this).
    fn move_exact(&mut self, parts: &[String], dx: Um, dy: Um, rotate_millideg: i64, pivot: Option<Point>) -> Result<(), Vec<CheckResult>> {
        if parts.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_move_exact", "move_exact", "no parts given")]);
        }
        // Validate every part is placed before moving any of them, so a
        // bad name among several refuses cleanly instead of leaving a
        // partial edit applied.
        for part in parts {
            self.require_placed(part)?;
        }
        for part in parts {
            let fp = self.require_placed(part)?;
            let moved = Point { x: fp.at.x + dx, y: fp.at.y + dy };
            let final_pos = match pivot {
                None => moved,
                Some(p) => rotate_point_about(moved, p, rotate_millideg),
            };
            let final_rot = (fp.rot as i64 + rotate_millideg).rem_euclid(360_000) as u32;
            self.set_pose(part, final_pos, final_rot)?;
        }
        Ok(())
    }

    // ---------------------------------------------------------- eeschema

    fn schematic(&self) -> Result<&SchematicSection, Vec<CheckResult>> {
        self.design.schematic.as_ref().ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])
    }

    fn schematic_mut(&mut self) -> Result<&mut SchematicSection, Vec<CheckResult>> {
        self.design.schematic.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])
    }

    /// Creates an empty schematic section on first use -- `AddSymbol`/
    /// `AddWire`/`AddLabel`/`AddNoConnect` are the only verbs allowed to
    /// start one from nothing (a brand-new design, or an intent whose own
    /// `derive_schematic` never ran); everything else targets an id that
    /// can only already exist inside a section that is already there.
    fn schematic_mut_or_create(&mut self) -> &mut SchematicSection {
        self.design.schematic.get_or_insert_with(|| SchematicSection { symbols: vec![], wires: vec![], labels: vec![], power_symbols: vec![], no_connects: vec![], title_block: None, sheets: vec![] })
    }

    fn find_symbol(&self, id: &str) -> Result<&SymbolInstance, Vec<CheckResult>> {
        self.schematic()?.symbols.iter().find(|s| s.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol instance with this reference")])
    }

    fn move_symbol(&mut self, id: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        self.find_symbol(id)?;
        let sch = self.schematic_mut()?;
        sch.symbols.iter_mut().find(|s| s.id == id).expect("checked above").at = Point { x, y };
        Ok(())
    }

    /// `G`: move a symbol and drag along the endpoint of every wire the
    /// caller found attached to one of its pins before the drag started
    /// (`(wire index, point index)` pairs into `wires`/`wires[i].pts`) --
    /// see `Cmd::DragSymbol`'s own doc for why resolving "attached" is the
    /// caller's job, not this method's.
    fn drag_symbol(&mut self, id: &str, x: Um, y: Um, attached: &[(usize, usize)]) -> Result<(), Vec<CheckResult>> {
        let before = self.find_symbol(id)?.at;
        let (dx, dy) = (x - before.x, y - before.y);
        let sch = self.schematic_mut()?;
        for &(wi, pi) in attached {
            if let Some(p) = sch.wires.get_mut(wi).and_then(|w| w.pts.get_mut(pi)) {
                p.x += dx;
                p.y += dy;
            }
        }
        sch.symbols.iter_mut().find(|s| s.id == id).expect("find_symbol just found it").at = Point { x, y };
        Ok(())
    }

    /// `R`/Shift+`R`: quarter turns, same sign convention as `Rotate`
    /// (positive = CCW, matching `sch_edit_tool.cpp`'s own 'R' default).
    fn rotate_symbol(&mut self, id: &str, quarter_turns: u8) -> Result<(), Vec<CheckResult>> {
        self.find_symbol(id)?;
        let sch = self.schematic_mut()?;
        let s = sch.symbols.iter_mut().find(|s| s.id == id).expect("checked above");
        let add = (quarter_turns as i64 % 4) * 90_000;
        s.rot = (s.rot as i64 + add).rem_euclid(360_000) as Millideg;
        Ok(())
    }

    /// `X` ("Mirror Horizontally") -- see `Cmd::MirrorSymbol`'s doc on why
    /// this is the one axis the IR models.
    fn mirror_symbol(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        self.find_symbol(id)?;
        let sch = self.schematic_mut()?;
        let s = sch.symbols.iter_mut().find(|s| s.id == id).expect("checked above");
        s.mirrored = !s.mirrored;
        Ok(())
    }

    fn delete_symbol(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.symbols.len();
        sch.symbols.retain(|s| s.id != id);
        if sch.symbols.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol instance with this reference")]);
        }
        Ok(())
    }

    fn add_wire(&mut self, pts: Vec<Point>) -> Result<(), Vec<CheckResult>> {
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_wire", "wire", "a wire needs at least two points")]);
        }
        self.schematic_mut_or_create().wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts });
        Ok(())
    }

    fn delete_wire(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.wires.len();
        sch.wires.retain(|w| w.id != id);
        if sch.wires.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_wire", id, "no wire with this id")]);
        }
        Ok(())
    }

    fn add_no_connect(&mut self, at: Point) -> Result<(), Vec<CheckResult>> {
        self.schematic_mut_or_create().no_connects.push(NoConnect { id: String::new(), at, pin: String::new() });
        Ok(())
    }

    fn delete_no_connect(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.no_connects.len();
        sch.no_connects.retain(|nc| nc.id != id);
        if sch.no_connects.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_no_connect", id, "no no-connect flag with this id")]);
        }
        Ok(())
    }

    fn add_label(&mut self, net: &str, at: Point, kind: LabelKind) -> Result<(), Vec<CheckResult>> {
        if net.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_label", "label", "a label needs a net name")]);
        }
        self.schematic_mut_or_create().labels.push(NetLabel { id: String::new(), net: net.into(), at, kind });
        Ok(())
    }

    fn delete_label(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.labels.len();
        sch.labels.retain(|l| l.id != id);
        if sch.labels.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_label", id, "no label with this id")]);
        }
        Ok(())
    }

    /// `P`: place a power symbol. `id` follows KiCad's own synthetic
    /// `#PWR01`/`#PWR02`/... numbering (`PowerSymbol::id`'s own doc).
    fn add_power_symbol(&mut self, lib_id: &str, at: Point, rot: Millideg, net: &str, pin: &str) -> Result<(), Vec<CheckResult>> {
        if net.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_power_symbol", "power_symbol", "a power symbol needs a net name")]);
        }
        let sch = self.schematic_mut_or_create();
        let mut n = sch.power_symbols.len() as u32 + 1;
        let mut id = format!("#PWR{n:02}");
        while sch.power_symbols.iter().any(|p| p.id == id) {
            n += 1;
            id = format!("#PWR{n:02}");
        }
        sch.power_symbols.push(PowerSymbol { id, lib_id: lib_id.into(), at, rot, net: net.into(), pin: pin.into() });
        Ok(())
    }

    fn delete_power_symbol(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.power_symbols.len();
        sch.power_symbols.retain(|p| p.id != id);
        if sch.power_symbols.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_power_symbol", id, "no power symbol with this id")]);
        }
        Ok(())
    }

    /// `A`: place a new symbol instance. The caller names `id` (a real
    /// reference like "R12", or a KiCad-style placeholder like "U?" for
    /// `Annotate` to number later) -- this only refuses an exact
    /// duplicate of an id already on the sheet.
    #[allow(clippy::too_many_arguments)]
    fn add_symbol(&mut self, id: &str, lib_id: &str, at: Point, rot: Millideg, value: &str, footprint: &str) -> Result<(), Vec<CheckResult>> {
        if id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", "symbol", "a symbol needs a reference designator")]);
        }
        let sch = self.schematic_mut_or_create();
        if sch.symbols.iter().any(|s| s.id == id) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", id, "a symbol with this reference is already on the sheet")]);
        }
        sch.symbols.push(SymbolInstance { id: id.into(), at, rot, mirrored: false, lib_id: lib_id.into(), unit: 1, value: value.into(), footprint: footprint.into(), datasheet: String::new() });
        Ok(())
    }

    /// `Ctrl+A` (Annotate), `dialog_annotate.cpp`'s default mode
    /// (`INCREMENTAL_BY_REF`, sort "top to bottom" -- by Y then X):
    /// assigns the next free number, per reference-letter prefix, to
    /// every symbol whose id is blank or ends in `?` (this project's own
    /// placeholder for "not yet annotated", e.g. `AddSymbol`'s "U?").
    /// `reset_existing` first strips every symbol's id back to
    /// "<prefix>?" (dialog's "Reset existing annotations"), so the whole
    /// sheet renumbers from scratch instead of only filling gaps.
    fn annotate(&mut self, reset_existing: bool) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if reset_existing {
            for s in &mut sch.symbols {
                let prefix: String = s.id.chars().take_while(|c| c.is_alphabetic()).collect();
                if !prefix.is_empty() {
                    s.id = format!("{prefix}?");
                }
            }
        }
        let mut next: BTreeMap<String, u32> = BTreeMap::new();
        for s in &sch.symbols {
            if s.id.ends_with('?') {
                continue;
            }
            let prefix: String = s.id.chars().take_while(|c| c.is_alphabetic()).collect();
            let num: u32 = s.id[prefix.len()..].parse().unwrap_or(0);
            let e = next.entry(prefix).or_insert(0);
            *e = (*e).max(num);
        }
        let mut order: Vec<usize> = (0..sch.symbols.len()).filter(|&i| sch.symbols[i].id.ends_with('?')).collect();
        order.sort_by(|&a, &b| (sch.symbols[a].at.y, sch.symbols[a].at.x).cmp(&(sch.symbols[b].at.y, sch.symbols[b].at.x)));
        for i in order {
            let prefix = sch.symbols[i].id.trim_end_matches('?').to_string();
            let prefix = if prefix.is_empty() { "U".to_string() } else { prefix };
            let n = next.entry(prefix.clone()).or_insert(0);
            *n += 1;
            sch.symbols[i].id = format!("{prefix}{n}");
        }
        Ok(())
    }
}

/// Rotate `pt` by `angle_millideg` around `pivot` -- the same matrix
/// `crates/model/src/footprint.rs`'s `to_board` applies to a pad's local
/// offset (no Y-axis flip either side), so a part moved this way lands
/// exactly where its own rendering will subsequently draw it.
fn rotate_point_about(pt: Point, pivot: Point, angle_millideg: i64) -> Point {
    let rad = (angle_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let dx = (pt.x - pivot.x) as f64;
    let dy = (pt.y - pivot.y) as f64;
    Point { x: pivot.x + (dx * cos - dy * sin).round() as Um, y: pivot.y + (dx * sin + dy * cos).round() as Um }
}

/// How far a `Place` will slide along the anchor before giving up.
///
/// In snap steps, so 400 is 40mm at the default 100µm grid -- most of a
/// board's width. The loop stops at the board edge anyway, so this is
/// only a bound against spinning, not a policy. It was 60 (6mm) until a
/// keepout started including the refdes label box, at which point six
/// capacitors along one side of a SOIC-8 no longer fitted in the slide
/// range and the sixth was refused.
const MAX_SLIDE_STEPS: u32 = 400;

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

pub mod build;
pub mod repair;
pub mod view;
pub mod episode;
pub mod flash;
