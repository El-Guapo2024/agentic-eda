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

use eda_model::footprint::{placed_courtyard, placed_keepout, placed_pads, Footprint};
use eda_model::ir::{
    Design, DrawingsSection, ErcExclusion, FillMode, FootprintAttributes, FootprintInstance, FootprintLibrarySection, IslandRemovalMode, LabelKind, LabelSide, LibraryFootprint, LibraryPad, Millideg, NetLabel, NoConnect, PadConnection, Point, PowerSymbol, RoutingSection, SchematicSection, SchematicText, Shape, Side, SymbolInstance, Text, TextJustify, Track, Um, Via, ViaPreset, Wire, Zone,
};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// `dialog_annotate.cpp`'s own "Order Options": which coordinate breaks
/// ties first when two symbols would otherwise land in the same spot in
/// the sort. `YThenX` ("sort by Y position") is KiCad's own default and
/// what this project's `annotate` already did before this field existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotateOrder {
    #[default]
    YThenX,
    XThenY,
}

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

/// `Cmd::EditTracksAndVias`'s track-width field: either leave every
/// matched track at its own net class's width (`BoardRules::width_of`,
/// falling back to the board default), or set them all to one explicit
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SizeSpec {
    NetClass,
    Value { um: Um },
}

/// Same idea as [`SizeSpec`], for a via's diameter+drill pair (which
/// always travel together -- there is no meaningful "set the diameter but
/// leave the drill" on its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViaSizeSpec {
    NetClass,
    Value { diameter: Um, drill: Um },
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

fn d_unit_one() -> u32 {
    1
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
    /// Footprint Properties' reference-designator "Text Placement" field
    /// (`pcb_properties_panel.cpp`'s silkscreen label side) -- which side
    /// of the courtyard the refdes label sits on. Does not move or
    /// resize anything else about the part, so unlike `Flip`/`Rotate`/
    /// `MoveTo` this does not clear the board's routing.
    SetLabelSide { part: String, side: LabelSide },

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
    /// `dialog_track_via_properties.cpp`'s editable via fields (GAPS.md
    /// #11): pad diameter and drill. Net, position and layer span are
    /// unchanged (KiCad does not let you re-net or re-span an existing via
    /// from this dialog either -- that is what delete-and-redraw is for).
    EditVia { id: String, diameter: Um, drill: Um },
    /// `BOARD_DESIGN_SETTINGS::m_TrackWidthList`, replaced wholesale --
    /// the studio's Board Setup "Track Widths & Vias" panel has no
    /// per-entry add/remove Cmd of its own, it just resubmits the whole
    /// list on every change, same spirit as `PasteItems`. The board's own
    /// default (`BoardRules::track_width`) is not part of this list -- a
    /// consumer always offers it as the implicit first entry.
    SetTrackWidthPresets { widths: Vec<Um> },
    /// Same idea as `SetTrackWidthPresets`, for `m_ViaSizeList`.
    SetViaPresets { presets: Vec<ViaPreset> },

    /// `dialog_global_edit_tracks_and_vias.cpp`'s "Apply and Close": bulk-
    /// set width (tracks/arcs) and/or diameter+drill (vias) and/or layer
    /// (tracks only) on every named id, as one atomic undo step. `ids` is
    /// already filtered by the caller (net/net-class/layer/width/
    /// "selected only" are UI concepts this crate has no model of, same
    /// split every other global-edit-style dialog here uses -- see
    /// `CleanupOptions`/`cleanup.rs`). `track_width`/`via_size` are `None`
    /// to leave that property alone (source's `INDETERMINATE_ACTION`);
    /// `Some(SizeSpec::NetClass)` resolves each item's *own* net's class
    /// at apply time (`BoardRules::width_of`/`via_diameter_of`/
    /// `via_drill_of`), so a mixed-net-class selection lands each item on
    /// its own class's value from one command, matching
    /// `SetTrackSegmentWidth`'s per-item resolution rather than a single
    /// shared value. This model has only one via "type" (no through/
    /// micro/blind/buried distinction, no padstack/annular-ring/IPC4761
    /// protection-feature concept), so those parts of the real dialog
    /// have no field here at all.
    EditTracksAndVias {
        ids: Vec<String>,
        #[serde(default)]
        track_width: Option<SizeSpec>,
        #[serde(default)]
        via_size: Option<ViaSizeSpec>,
        #[serde(default)]
        layer: Option<String>,
    },

    /// Add a copper pour.
    AddZone { net: String, layer: String, outline: Vec<Point> },
    /// Remove a zone by id.
    DeleteZone { id: String },
    /// `pcb_point_editor.cpp`'s zone-outline editing (drag a corner, add/
    /// remove one) -- this app has no live point-by-point drag state on
    /// the backend, so the whole edited outline is sent at once, same
    /// "replace wholesale" shape `EditZone`'s settings already use. Net/
    /// layer/settings untouched.
    SetZoneOutline { id: String, outline: Vec<Point> },
    /// `dialog_copper_zones.cpp`'s "OK": replace a zone's net/layer and
    /// every `ZONE_SETTINGS` field at once -- KiCad has no concept of
    /// editing just one field of the panel, the whole thing commits
    /// together. The outline is untouched (see the point editor, GAPS.md
    /// #1's zone item, for corner-level edits).
    EditZone {
        id: String,
        net: String,
        layer: String,
        clearance: Um,
        min_thickness: Um,
        thermal_gap: Um,
        thermal_spoke_width: Um,
        pad_connection: PadConnection,
        priority: u32,
        island_removal_mode: IslandRemovalMode,
        min_island_area: i64,
        fill_mode: FillMode,
        hatch_thickness: Um,
        hatch_gap: Um,
        hatch_orientation_mdeg: Millideg,
        hatch_smoothing_level: i32,
        hatch_smoothing_value: f64,
        hatch_hole_min_area: f64,
        hatch_border_algorithm: i32,
    },

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
    /// `dialog_pcb_shape_properties.cpp`'s editable fields (GAPS.md #11):
    /// layer, line width, and filled/unfilled. Geometry itself has no
    /// dialog field to edit in source either (dragging the shape's own
    /// points is the only way) -- left for a future point editor.
    EditShape { id: String, layer: String, stroke_width: Um, filled: bool },

    /// Add free text.
    AddText { text: Text },
    /// Replace a text's content and styling in place. Its id and position
    /// do not change (see `MoveText` for that).
    EditText { id: String, content: String, angle: Millideg, layer: String, size_um: Um, stroke_width: Um, justify: TextJustify, mirror: bool },
    /// Remove a text by id.
    DeleteText { id: String },
    /// Move a text to a new position, keeping its id and styling.
    MoveText { id: String, x: Um, y: Um },
    /// `dialog_global_edit_text_and_graphics.cpp`'s "Apply and Close",
    /// scoped to this model's two free-standing board drawing kinds
    /// (`Shape`/`Text` -- no footprint reference/value fields, dimensions,
    /// tables or barcodes exist as editable board items here, see
    /// PARITY-pcb.md section 2). Every field `None` leaves that property
    /// alone (source's `INDETERMINATE_ACTION`); `shape_ids`/`text_ids` are
    /// already filtered by the caller, same "no UI-filter concept in this
    /// crate" split `EditTracksAndVias` uses. This model has no "layer
    /// default values" to reset to (no `BOARD_DESIGN_SETTINGS::
    /// m_LineThickness`/`m_TextSize` per-layer-class arrays), so unlike
    /// source there is only the one "specified values" mode.
    EditTextAndGraphics {
        #[serde(default)]
        shape_ids: Vec<String>,
        #[serde(default)]
        text_ids: Vec<String>,
        #[serde(default)]
        layer: Option<String>,
        /// Shapes' `stroke_width`.
        #[serde(default)]
        line_width: Option<Um>,
        /// Texts' `size_um` (applied to both the width and height this
        /// model draws text at -- see `Text::size_um`'s own doc).
        #[serde(default)]
        text_size: Option<Um>,
        /// Texts' `stroke_width`.
        #[serde(default)]
        text_thickness: Option<Um>,
    },

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

    /// Finish an interactive router session (`crates/pns`'s `Router::
    /// finish`, gap #7): remove whatever existing tracks/vias the session's
    /// push-and-shove displaced, then add the session's final geometry
    /// (its own new runs plus every displaced item's updated shape) --
    /// all as one undo step, the same way `PasteItems` is one step for a
    /// pure add. Unknown ids in `remove_track_ids`/`remove_via_ids` are
    /// tolerated (not an error): the router names ids from its own
    /// internal bookkeeping of what it touched, not from user input, and a
    /// route that didn't actually displace anything legitimately sends
    /// none.
    CommitRoute {
        #[serde(default)]
        remove_track_ids: Vec<String>,
        #[serde(default)]
        remove_via_ids: Vec<String>,
        #[serde(default)]
        tracks: Vec<Track>,
        #[serde(default)]
        vias: Vec<Via>,
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
    ///
    /// `unit`: which placed instance of `id` to move, when a multi-unit
    /// part has more than one on the sheet (`None` -- every existing
    /// caller, since this field postdates multi-unit support -- resolves
    /// to "the only instance" when `id` names exactly one, and is refused
    /// as ambiguous otherwise; see `Board::find_symbol_unit`'s own doc).
    /// Same convention on every other per-instance verb below
    /// (`DragSymbol`/`RotateSymbol`/`MirrorSymbol`/`MirrorSymbolVertical`/
    /// `DeleteSymbol`) -- `EditSymbolFields`/`RenameSymbol` deliberately do
    /// *not* take one, since Reference/Value/Footprint/Datasheet are
    /// part-wide, not per-unit (`ONE part, one footprint, several placed
    /// units` -- every unit shares them).
    MoveSymbol { id: String, x: Um, y: Um, #[serde(default)] unit: Option<u32> },
    /// `G`: move a symbol AND drag along the endpoint of every wire
    /// presently attached to one of its pins (KiCad's rubber-band) --
    /// `attached_wire_endpoints` names which (wire index, endpoint index)
    /// pairs the caller found glued to this symbol before the drag
    /// started, so this moves exactly those points by the same delta
    /// rather than re-deriving attachment from the new position. `unit`:
    /// see `MoveSymbol`'s own doc.
    DragSymbol { id: String, x: Um, y: Um, attached_wire_endpoints: Vec<(usize, usize)>, #[serde(default)] unit: Option<u32> },
    /// `R`: quarter turns, same convention as `Rotate`. `unit`: see
    /// `MoveSymbol`'s own doc.
    RotateSymbol { id: String, quarter_turns: u8, #[serde(default)] unit: Option<u32> },
    /// `X` ("Mirror Horizontally", KiCad's `SYM_MIRROR_Y` / negate-X):
    /// toggles `SymbolInstance::mirrored` (confirmed against
    /// `transform_local_point`: `mirrored` negates X, matching this
    /// hotkey exactly). `unit`: see `MoveSymbol`'s own doc.
    MirrorSymbol { id: String, #[serde(default)] unit: Option<u32> },
    /// `Y` ("Mirror Vertically", KiCad's `SYM_MIRROR_X`): toggles
    /// `SymbolInstance::mirror_y`, clearing `mirrored` if it was set --
    /// KiCad's own symbols never carry both mirror flags at once (there
    /// are only 3 mirror states: none, X, Y -- see `transform.ts`'s own
    /// header comment on the frontend, ported from `sch_symbol.cpp::
    /// SetOrientation`), so toggling one axis on always means toggling
    /// the other off, same as a real `SetOrientation` call replaces the
    /// whole orientation rather than adding a flag. `unit`: see
    /// `MoveSymbol`'s own doc.
    MirrorSymbolVertical { id: String, #[serde(default)] unit: Option<u32> },
    /// `Del` on a symbol: removes one placed instance -- all of it when
    /// `id` names a single-unit part (and its synthesized `Part`, if
    /// `reconcile_schematic` had added one for a symbol with no intent
    /// counterpart), or just that one unit of a multi-unit part, leaving
    /// its siblings on the sheet (matches real eeschema: deleting unit B of
    /// a quad gate does not remove units A/C/D). Matches real eeschema in
    /// the single-unit case too: wires that landed on this symbol's pins
    /// are left exactly where they are, now dangling -- ERC's existing
    /// dangling-wire checks are what surfaces that, not a cascade delete.
    /// `unit`: see `MoveSymbol`'s own doc.
    DeleteSymbol { id: String, #[serde(default)] unit: Option<u32> },

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
    /// `bus: true` (GAPS.md #20 -- the `B` tool) draws a bus wire
    /// (`Wire::bus`) instead of a plain one; `#[serde(default)]` so a
    /// `design.json`/client written before this field existed keeps
    /// drawing plain wires exactly as before.
    AddWire {
        pts: Vec<Point>,
        #[serde(default)]
        bus: bool,
    },
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
    /// GAPS.md #20: a bus entry stub at `at`, reaching to `at + size` --
    /// `size` is the caller's choice (the frontend defaults to KiCad's own
    /// ±100mil/±2540um diagonal, picking the sign from which side of the
    /// bus wire was clicked), not fixed here, so an imported file's own
    /// exact stored size and a freshly-drawn one share the same verb.
    AddBusEntry { at: Point, size: Point },
    DeleteBusEntry { id: String },

    /// `dialog_erc.cpp`'s own "Exclude this violation" (right-click a
    /// finding, or the dialog's own Exclude button): accepts one ERC
    /// finding by its own `(check, location)` key -- the same shape
    /// `eda_kicad::erc::Exclusions` already keys on, so `erc_json` can
    /// build one directly from the persisted list with no translation.
    /// Refused if `location` is empty (nothing to key on -- the same
    /// findings `Exclusions` itself can never exclude); adding one
    /// already excluded is a harmless no-op, not an error.
    AddErcExclusion { check: String, location: String },
    /// Un-exclude (the dialog's own "Ignored Tests" tab, "Remove"):
    /// refused if no exclusion with this exact key exists.
    DeleteErcExclusion { check: String, location: String },

    /// `L`/`Ctrl+L`/`H`: place a net label. `kind`/`shape` follow
    /// `eda_model::ir::LabelKind`'s own vocabulary (local has no shape).
    AddLabel { net: String, at: Point, kind: LabelKind },
    DeleteLabel { id: String },
    /// `T`: place free-standing text -- see `SchematicText`'s own doc for
    /// why this is a separate verb from `AddText` (the PCB one, a
    /// different shape entirely: layer/stroke/justify/mirror, none of
    /// which a schematic has).
    AddSchText { content: String, at: Point, angle_millideg: Millideg, size_um: Um },
    DeleteSchText { id: String },
    /// `P`: place a power symbol on a pin.
    AddPowerSymbol { lib_id: String, at: Point, rot_millideg: Millideg, net: String, pin: String },
    DeletePowerSymbol { id: String },

    /// `A`: place a new symbol instance from the library -- the one verb
    /// here that can grow the part list itself (`reconcile_schematic`
    /// synthesizes a `Part` for an `id` with no intent counterpart), so it
    /// shows up unplaced on the PCB tab exactly like any other part
    /// `eda board new` never heard of.
    ///
    /// `unit`: which unit of a multi-unit symbol this placement is (1 when
    /// omitted -- every placed symbol before this field existed, and every
    /// single-unit part, is unit 1). Refused if `(id, unit)` is already on
    /// the sheet -- placing unit 2 of an *already-annotated* reference
    /// (e.g. `id: "U1", unit: 2` once "U1" unit 1 already exists) is the
    /// supported way to add another unit of an existing part; placing a
    /// second not-yet-annotated unit under the same placeholder id (e.g.
    /// two different `"U?"` parts each wanting more than one unit placed
    /// before `Annotate` ever runs) is a known, narrower gap -- `Annotate`
    /// still groups purely by placeholder text, not by a placement
    /// session, so two distinct unannotated multi-unit parts sharing the
    /// literal text `"U?"` would be mis-numbered as one; placing additional
    /// units of an already-numbered reference is unaffected by this.
    AddSymbol { id: String, lib_id: String, at: Point, rot_millideg: Millideg, value: String, footprint: String, #[serde(default = "d_unit_one")] unit: u32 },

    /// `E` (Properties -- Value/Footprint/Datasheet only; see below for
    /// `U`'s own reference rename) and `V`/`F` (`sch_edit_tool.cpp::
    /// EditField`'s quick single-field edits): any of `value`/
    /// `footprint`/`datasheet` left `None` is unchanged, so a single
    /// quick-edit doesn't have to resend the other two just to leave them
    /// alone. Deliberately does *not* touch `ConstraintModel::Part` (not
    /// persisted across requests anyway, see `board::load`) -- same
    /// "the schematic instance's own copy wins when set" precedent
    /// `SymbolInstance::value`'s own doc already established, so the
    /// Schematic tab reflects the edit immediately regardless.
    EditSymbolFields { id: String, value: Option<String>, footprint: Option<String>, datasheet: Option<String> },

    /// `U` (`editReference`): rename a symbol's own reference designator,
    /// cascading the change through every `"REF.PIN"` string this sheet's
    /// wires/power-symbols/no-connects hold (so a wire that named
    /// `"R1.2"` still resolves after `R1` becomes `R5`). Refused if
    /// `new_id` is blank or already names another symbol on this sheet.
    /// Deliberately does *not* retarget `design.placement`'s own
    /// `FootprintInstance` (the PCB side has no rename concept at all) or
    /// touch intent.yaml (read-only to every verb in this file) -- the
    /// next `reconcile_schematic` pass simply synthesizes a fresh `Part`
    /// for the new id, same mechanism `AddSymbol`'s own doc describes,
    /// while the old reference's PCB footprint (if any) is left exactly
    /// where it was, now with no matching schematic symbol. A real
    /// "rename and keep the PCB placement" is future work -- see
    /// PARITY-sch.md.
    RenameSymbol { id: String, new_id: String },

    /// `Ctrl+A` (Annotate): assign reference designators to every
    /// not-yet-annotated symbol (an `id` this project synthesizes as
    /// `"U?1"`/`"U?2"`/... when `AddSymbol` is given a blank prefix+number
    /// -- see `sch_drawing_tools`' own symbol-chooser flow), sorted by
    /// `order` (`dialog_annotate.cpp`'s "Order Options", default top to
    /// bottom). `reset_existing` mirrors the dialog's "Clear and
    /// re-annotate" vs "Keep existing" modes. `ids`, when given, is
    /// `dialog_annotate.cpp`'s own "Selection" scope -- only symbols named
    /// here are reset/renumbered, everything else on the sheet is left
    /// exactly as it is; `None` is the dialog's "Schematic"/"Sheet" scope
    /// (this project's IR has no sheet hierarchy to tell those two apart,
    /// so there's only one "whole sheet" scope here, not source's three).
    Annotate {
        reset_existing: bool,
        #[serde(default)]
        order: AnnotateOrder,
        #[serde(default)]
        ids: Option<Vec<String>>,
    },

    // -------------------------------------------------- footprint editor
    //
    // GAPS.md #8. A `Domain::FootprintEditor` command set (see `Cmd::
    // domain`): every verb here targets `design.footprint_library`, never
    // `design.placement`/`routing` directly -- editing a footprint
    // definition never touches anything already on the board on its own.
    // `UpdateFootprintOnBoard` is the one explicit exception, and even
    // that only flips a flag `crate::board::load`'s overlay reads (see
    // `LibraryFootprint::published`'s own doc in eda-model) rather than
    // reaching into `design.placement` itself.
    /// Open a footprint for editing, creating `design.footprint_library`'s
    /// entry for `name` the first time -- from an already-resolved engine
    /// footprint (a loaded `.kicad_mod` library file, an intent-declared
    /// one, or the builtin table) when `name` resolves to one, else a
    /// brand-new empty footprint (KiCad's "New Footprint" and "Edit
    /// Footprint" are the same verb here: naming something nothing has
    /// defined yet just starts blank). A no-op, not an error, if this
    /// footprint is already in the library -- re-opening never resets
    /// in-progress edits.
    OpenFootprintForEdit { name: String },
    /// Remove a footprint definition from the project library entirely.
    /// Never touches a board instance naming it (same "explicit, not
    /// automatic" rule `UpdateFootprintOnBoard` documents) -- an instance
    /// left pointing at a now-undefined name simply falls back to the
    /// builtin table/intent resolution, exactly as if this editor had
    /// never touched it.
    DeleteLibraryFootprint { name: String },
    /// `dialog_footprint_properties_fp_editor.cpp`'s General tab -- the
    /// whole panel commits together, same shape as `EditZone`.
    EditFootprintProperties {
        name: String,
        description: String,
        keywords: String,
        attributes: FootprintAttributes,
        reference_visible: bool,
        value_visible: bool,
        /// `(model "...")` 3D model path; `None` clears it.
        model: Option<String>,
    },
    /// `PCB_ACTIONS::setAnchor` ("Place Footprint Anchor"): `at` (in the
    /// footprint's current local frame) becomes the new origin -- every
    /// pad/graphic/text is translated by `-at` so it does, matching
    /// `FOOTPRINT::MoveAnchorPosition`'s own "rewrite every child's local
    /// coordinate" behavior. This editor has no board position for the
    /// anchor to stay fixed relative to the way source's board-pulled
    /// footprint does (see PARITY-fpedit.md).
    SetFootprintAnchor { name: String, at: Point },
    /// KiCad's "Update Footprint from Library", kept an explicit action
    /// (see this section's own doc) -- every board instance naming `name`
    /// starts resolving its pads/courtyard from this library entry from
    /// here on (`crate::board::load`'s overlay, gated on `published`).
    UpdateFootprintOnBoard { name: String },

    /// Place a pad (`pad_tool.cpp`'s `PlacePad`). `pad.id`, if the caller
    /// sent one, is ignored -- ids are assigned here, same as `AddShape`.
    AddPad { footprint: String, pad: LibraryPad },
    /// Move a pad to an absolute position in the footprint's local frame
    /// -- a pad has one center point, like a via or text (see `MoveVia`/
    /// `MoveText`), not `MoveShape`'s delta form.
    MovePad { footprint: String, id: String, x: Um, y: Um },
    /// Quarter turns about the pad's own center, same convention as
    /// `Cmd::Rotate`. KiCad's own pad rotation step is the generic
    /// `EDIT_TOOL`'s configurable `PCBNEW_SETTINGS::m_RotationAngle`
    /// (defaults to 90 degrees); this app's R/Shift+R convention
    /// everywhere else is a fixed quarter turn, kept here too rather than
    /// adding a second rotation-step setting just for pads.
    RotatePad { footprint: String, id: String, quarter_turns: u8 },
    DeletePad { footprint: String, id: String },
    /// Pad Properties dialog's OK: replace every field of one pad at once
    /// (same "whole panel commits together" shape as `EditZone`). `id` on
    /// `pad`, if the caller sent one, is ignored -- the pad keeps the id
    /// this Cmd's own `id` field names.
    EditPad { footprint: String, id: String, pad: LibraryPad },
    /// `pad_tool.cpp`'s "Push Pad Properties": copy shape/size/drill/
    /// layers/overrides (never number, position, or rotation) from
    /// `source_pad_id` to every *other* pad in the same footprint, each
    /// `filter_*` flag restricting the targets to pads whose current
    /// value of that one property already matches the source (an unset
    /// filter applies to every pad regardless of that property) --
    /// source's own four independent filter checkboxes.
    PushPadProperties {
        footprint: String,
        source_pad_id: String,
        filter_shape: bool,
        filter_orientation: bool,
        filter_layers: bool,
        filter_type: bool,
    },
    /// `pad_tool.cpp`'s "Renumber Pads" (`DIALOG_ENUM_PADS`), simplified:
    /// source numbers pads in click/drag order; this orders them by
    /// position (top-to-bottom, then left-to-right -- reading order,
    /// matching `Annotate`'s own sort) since this editor has no
    /// interactive click-sequence tool of its own (see PARITY-fpedit.md).
    /// `prefix` + (`start` + `step` * index).
    RenumberPads { footprint: String, start: u32, prefix: String, step: u32 },

    /// Free-standing graphics on a footprint's own silkscreen/fab/
    /// courtyard layers, in its local frame -- same `Shape` type and the
    /// same id-assignment/validation rules as the PCB tab's `AddShape`.
    AddFootprintGraphic { footprint: String, shape: Shape },
    DeleteFootprintGraphic { footprint: String, id: String },
    MoveFootprintGraphic { footprint: String, id: String, dx: Um, dy: Um },
    EditFootprintGraphic { footprint: String, id: String, layer: String, stroke_width: Um, filled: bool },
    /// Free-standing text on a footprint, same `Text` type as the PCB
    /// tab's `AddText`.
    AddFootprintText { footprint: String, text: Text },
    EditFootprintText { footprint: String, id: String, content: String, angle: Millideg, layer: String, size_um: Um, stroke_width: Um, justify: TextJustify, mirror: bool },
    DeleteFootprintText { footprint: String, id: String },
    MoveFootprintText { footprint: String, id: String, x: Um, y: Um },
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
    /// GAPS.md #8's Footprint Editor tab: `design.footprint_library`'s own
    /// undo/redo scope, independent of whatever the PCB/Schematic tabs are
    /// doing (so Ctrl+Z there never undoes a board edit, or vice versa --
    /// same reasoning `Schematic` already got its own scope for, GAPS.md
    /// #15).
    FootprintEditor,
}

impl Cmd {
    /// Which editor this command belongs to -- see [`Domain`].
    pub fn domain(&self) -> Domain {
        match self {
            Cmd::MoveSymbol { .. }
            | Cmd::DragSymbol { .. }
            | Cmd::RotateSymbol { .. }
            | Cmd::MirrorSymbol { .. }
            | Cmd::MirrorSymbolVertical { .. }
            | Cmd::DeleteSymbol { .. }
            | Cmd::AddWire { .. }
            | Cmd::DeleteWire { .. }
            | Cmd::AddNoConnect { .. }
            | Cmd::DeleteNoConnect { .. }
            | Cmd::AddBusEntry { .. }
            | Cmd::DeleteBusEntry { .. }
            | Cmd::AddErcExclusion { .. }
            | Cmd::DeleteErcExclusion { .. }
            | Cmd::AddLabel { .. }
            | Cmd::DeleteLabel { .. }
            | Cmd::AddSchText { .. }
            | Cmd::DeleteSchText { .. }
            | Cmd::AddPowerSymbol { .. }
            | Cmd::DeletePowerSymbol { .. }
            | Cmd::AddSymbol { .. }
            | Cmd::EditSymbolFields { .. }
            | Cmd::RenameSymbol { .. }
            | Cmd::Annotate { .. } => Domain::Schematic,
            Cmd::OpenFootprintForEdit { .. }
            | Cmd::DeleteLibraryFootprint { .. }
            | Cmd::EditFootprintProperties { .. }
            | Cmd::SetFootprintAnchor { .. }
            | Cmd::UpdateFootprintOnBoard { .. }
            | Cmd::AddPad { .. }
            | Cmd::MovePad { .. }
            | Cmd::RotatePad { .. }
            | Cmd::DeletePad { .. }
            | Cmd::EditPad { .. }
            | Cmd::PushPadProperties { .. }
            | Cmd::RenumberPads { .. }
            | Cmd::AddFootprintGraphic { .. }
            | Cmd::DeleteFootprintGraphic { .. }
            | Cmd::MoveFootprintGraphic { .. }
            | Cmd::EditFootprintGraphic { .. }
            | Cmd::AddFootprintText { .. }
            | Cmd::EditFootprintText { .. }
            | Cmd::DeleteFootprintText { .. }
            | Cmd::MoveFootprintText { .. } => Domain::FootprintEditor,
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
            | Cmd::Flip { part }
            | Cmd::SetLabelSide { part, .. } => vec![part],
            Cmd::Swap { a, b } => vec![a, b],
            Cmd::AddTrack { net, .. } | Cmd::AddVia { net, .. } | Cmd::AddZone { net, .. } => vec![net.as_str()],
            Cmd::DeleteTrack { id }
            | Cmd::SetTrackWidth { id, .. }
            | Cmd::DeleteVia { id }
            | Cmd::MoveVia { id, .. }
            | Cmd::EditVia { id, .. }
            | Cmd::DeleteZone { id }
            | Cmd::EditZone { id, .. }
            | Cmd::SetZoneOutline { id, .. }
            | Cmd::DeleteShape { id }
            | Cmd::MoveShape { id, .. }
            | Cmd::EditShape { id, .. }
            | Cmd::EditText { id, .. }
            | Cmd::DeleteText { id }
            | Cmd::MoveText { id, .. } => vec![id.as_str()],
            Cmd::AddShape { shape } => vec![shape.layer()],
            Cmd::AddText { text } => vec![text.content.as_str()],
            Cmd::Duplicate { ids } => ids.iter().map(String::as_str).collect(),
            Cmd::PasteItems { .. } => vec!["paste"],
            Cmd::CommitRoute { .. } => vec!["route"],
            Cmd::MoveExact { parts, .. } => parts.iter().map(String::as_str).collect(),
            Cmd::SetTrackWidthPresets { .. } => vec!["track_width_presets"],
            Cmd::SetViaPresets { .. } => vec!["via_presets"],
            Cmd::EditTracksAndVias { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::EditTextAndGraphics { shape_ids, text_ids, .. } => shape_ids.iter().chain(text_ids.iter()).map(String::as_str).collect(),

            Cmd::MoveSymbol { id, .. }
            | Cmd::DragSymbol { id, .. }
            | Cmd::RotateSymbol { id, .. }
            | Cmd::MirrorSymbol { id, .. }
            | Cmd::MirrorSymbolVertical { id, .. }
            | Cmd::DeleteSymbol { id, .. }
            | Cmd::AddSymbol { id, .. }
            | Cmd::EditSymbolFields { id, .. }
            | Cmd::RenameSymbol { id, .. } => vec![id],
            Cmd::AddWire { .. } => vec!["wire"],
            Cmd::DeleteWire { id } | Cmd::DeleteNoConnect { id } | Cmd::DeleteLabel { id } | Cmd::DeletePowerSymbol { id } | Cmd::DeleteSchText { id } | Cmd::DeleteBusEntry { id } => vec![id],
            Cmd::AddNoConnect { .. } => vec!["no_connect"],
            Cmd::AddBusEntry { .. } => vec!["bus_entry"],
            Cmd::AddErcExclusion { location, .. } | Cmd::DeleteErcExclusion { location, .. } => vec![location.as_str()],
            Cmd::AddLabel { net, .. } => vec![net.as_str()],
            Cmd::AddSchText { content, .. } => vec![content.as_str()],
            Cmd::AddPowerSymbol { net, .. } => vec![net.as_str()],
            Cmd::Annotate { .. } => vec!["annotate"],

            Cmd::OpenFootprintForEdit { name } | Cmd::DeleteLibraryFootprint { name } | Cmd::EditFootprintProperties { name, .. } | Cmd::SetFootprintAnchor { name, .. } | Cmd::UpdateFootprintOnBoard { name } => {
                vec![name.as_str()]
            }
            Cmd::AddPad { footprint, .. }
            | Cmd::MovePad { footprint, .. }
            | Cmd::RotatePad { footprint, .. }
            | Cmd::DeletePad { footprint, .. }
            | Cmd::EditPad { footprint, .. }
            | Cmd::PushPadProperties { footprint, .. }
            | Cmd::RenumberPads { footprint, .. }
            | Cmd::AddFootprintGraphic { footprint, .. }
            | Cmd::DeleteFootprintGraphic { footprint, .. }
            | Cmd::MoveFootprintGraphic { footprint, .. }
            | Cmd::EditFootprintGraphic { footprint, .. }
            | Cmd::AddFootprintText { footprint, .. }
            | Cmd::EditFootprintText { footprint, .. }
            | Cmd::DeleteFootprintText { footprint, .. }
            | Cmd::MoveFootprintText { footprint, .. } => vec![footprint.as_str()],
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
            Cmd::SetLabelSide { part, side } => self.set_label_side(part, *side),

            Cmd::AddTrack { net, layer, width, pts } => self.add_track(net, layer, *width, pts),
            Cmd::DeleteTrack { id } => self.delete_track(id),
            Cmd::SetTrackWidth { id, width } => self.set_track_width(id, *width),

            Cmd::AddVia { net, x, y, drill, diameter, from_layer, to_layer } => self.add_via(net, *x, *y, *drill, *diameter, from_layer, to_layer),
            Cmd::DeleteVia { id } => self.delete_via(id),
            Cmd::MoveVia { id, x, y } => self.move_via(id, *x, *y),
            Cmd::EditVia { id, diameter, drill } => self.edit_via(id, *diameter, *drill),
            Cmd::SetTrackWidthPresets { widths } => self.set_track_width_presets(widths),
            Cmd::SetViaPresets { presets } => self.set_via_presets(presets),
            Cmd::EditTracksAndVias { ids, track_width, via_size, layer } => self.edit_tracks_and_vias(ids, track_width.as_ref(), via_size.as_ref(), layer.as_deref()),

            Cmd::AddZone { net, layer, outline } => self.add_zone(net, layer, outline),
            Cmd::DeleteZone { id } => self.delete_zone(id),
            Cmd::SetZoneOutline { id, outline } => self.set_zone_outline(id, outline),
            Cmd::EditZone {
                id,
                net,
                layer,
                clearance,
                min_thickness,
                thermal_gap,
                thermal_spoke_width,
                pad_connection,
                priority,
                island_removal_mode,
                min_island_area,
                fill_mode,
                hatch_thickness,
                hatch_gap,
                hatch_orientation_mdeg,
                hatch_smoothing_level,
                hatch_smoothing_value,
                hatch_hole_min_area,
                hatch_border_algorithm,
            } => self.edit_zone(
                id,
                net,
                layer,
                *clearance,
                *min_thickness,
                *thermal_gap,
                *thermal_spoke_width,
                *pad_connection,
                *priority,
                *island_removal_mode,
                *min_island_area,
                *fill_mode,
                *hatch_thickness,
                *hatch_gap,
                *hatch_orientation_mdeg,
                *hatch_smoothing_level,
                *hatch_smoothing_value,
                *hatch_hole_min_area,
                *hatch_border_algorithm,
            ),

            Cmd::AddShape { shape } => self.add_shape(shape.clone()),
            Cmd::DeleteShape { id } => self.delete_shape(id),
            Cmd::MoveShape { id, dx, dy } => self.move_shape(id, *dx, *dy),
            Cmd::EditShape { id, layer, stroke_width, filled } => self.edit_shape(id, layer, *stroke_width, *filled),

            Cmd::AddText { text } => self.add_text(text.clone()),
            Cmd::EditText { id, content, angle, layer, size_um, stroke_width, justify, mirror } => {
                self.edit_text(id, content.clone(), *angle, layer.clone(), *size_um, *stroke_width, *justify, *mirror)
            }
            Cmd::DeleteText { id } => self.delete_text(id),
            Cmd::MoveText { id, x, y } => self.move_text(id, *x, *y),
            Cmd::EditTextAndGraphics { shape_ids, text_ids, layer, line_width, text_size, text_thickness } => {
                self.edit_text_and_graphics(shape_ids, text_ids, layer.as_deref(), *line_width, *text_size, *text_thickness)
            }

            Cmd::Duplicate { ids } => self.duplicate_items(ids),
            Cmd::PasteItems { tracks, vias, zones, shapes, texts } => self.insert_copies(tracks.clone(), vias.clone(), zones.clone(), shapes.clone(), texts.clone()),
            Cmd::CommitRoute { remove_track_ids, remove_via_ids, tracks, vias } => self.commit_route(remove_track_ids, remove_via_ids, tracks.clone(), vias.clone()),
            Cmd::MoveExact { parts, dx, dy, rotate_millideg, pivot } => self.move_exact(parts, *dx, *dy, *rotate_millideg, *pivot),

            Cmd::MoveSymbol { id, x, y, unit } => self.move_symbol(id, *x, *y, *unit),
            Cmd::DragSymbol { id, x, y, attached_wire_endpoints, unit } => self.drag_symbol(id, *x, *y, attached_wire_endpoints, *unit),
            Cmd::RotateSymbol { id, quarter_turns, unit } => self.rotate_symbol(id, *quarter_turns, *unit),
            Cmd::MirrorSymbol { id, unit } => self.mirror_symbol(id, *unit),
            Cmd::MirrorSymbolVertical { id, unit } => self.mirror_symbol_vertical(id, *unit),
            Cmd::DeleteSymbol { id, unit } => self.delete_symbol(id, *unit),
            Cmd::AddWire { pts, bus } => self.add_wire(pts.clone(), *bus),
            Cmd::DeleteWire { id } => self.delete_wire(id),
            Cmd::AddNoConnect { at } => self.add_no_connect(*at),
            Cmd::AddBusEntry { at, size } => self.add_bus_entry(*at, *size),
            Cmd::DeleteBusEntry { id } => self.delete_bus_entry(id),
            Cmd::DeleteNoConnect { id } => self.delete_no_connect(id),
            Cmd::AddErcExclusion { check, location } => self.add_erc_exclusion(check, location),
            Cmd::DeleteErcExclusion { check, location } => self.delete_erc_exclusion(check, location),
            Cmd::AddLabel { net, at, kind } => self.add_label(net, *at, kind.clone()),
            Cmd::DeleteLabel { id } => self.delete_label(id),
            Cmd::AddSchText { content, at, angle_millideg, size_um } => self.add_sch_text(content, *at, *angle_millideg, *size_um),
            Cmd::DeleteSchText { id } => self.delete_sch_text(id),
            Cmd::AddPowerSymbol { lib_id, at, rot_millideg, net, pin } => self.add_power_symbol(lib_id, *at, *rot_millideg, net, pin),
            Cmd::DeletePowerSymbol { id } => self.delete_power_symbol(id),
            Cmd::AddSymbol { id, lib_id, at, rot_millideg, value, footprint, unit } => self.add_symbol(id, lib_id, *at, *rot_millideg, value, footprint, *unit),
            Cmd::EditSymbolFields { id, value, footprint, datasheet } => self.edit_symbol_fields(id, value.as_deref(), footprint.as_deref(), datasheet.as_deref()),
            Cmd::RenameSymbol { id, new_id } => self.rename_symbol(id, new_id),
            Cmd::Annotate { reset_existing, order, ids } => self.annotate(*reset_existing, *order, ids.as_deref()),

            Cmd::OpenFootprintForEdit { name } => self.open_footprint_for_edit(name),
            Cmd::DeleteLibraryFootprint { name } => self.delete_library_footprint(name),
            Cmd::EditFootprintProperties { name, description, keywords, attributes, reference_visible, value_visible, model } => {
                self.edit_footprint_properties(name, description.clone(), keywords.clone(), *attributes, *reference_visible, *value_visible, model.clone())
            }
            Cmd::SetFootprintAnchor { name, at } => self.set_footprint_anchor(name, *at),
            Cmd::UpdateFootprintOnBoard { name } => self.update_footprint_on_board(name),

            Cmd::AddPad { footprint, pad } => self.add_pad(footprint, pad.clone()),
            Cmd::MovePad { footprint, id, x, y } => self.move_pad(footprint, id, *x, *y),
            Cmd::RotatePad { footprint, id, quarter_turns } => self.rotate_pad(footprint, id, *quarter_turns),
            Cmd::DeletePad { footprint, id } => self.delete_pad(footprint, id),
            Cmd::EditPad { footprint, id, pad } => self.edit_pad(footprint, id, pad.clone()),
            Cmd::PushPadProperties { footprint, source_pad_id, filter_shape, filter_orientation, filter_layers, filter_type } => {
                self.push_pad_properties(footprint, source_pad_id, *filter_shape, *filter_orientation, *filter_layers, *filter_type)
            }
            Cmd::RenumberPads { footprint, start, prefix, step } => self.renumber_pads(footprint, *start, prefix, *step),

            Cmd::AddFootprintGraphic { footprint, shape } => self.add_footprint_graphic(footprint, shape.clone()),
            Cmd::DeleteFootprintGraphic { footprint, id } => self.delete_footprint_graphic(footprint, id),
            Cmd::MoveFootprintGraphic { footprint, id, dx, dy } => self.move_footprint_graphic(footprint, id, *dx, *dy),
            Cmd::EditFootprintGraphic { footprint, id, layer, stroke_width, filled } => self.edit_footprint_graphic(footprint, id, layer, *stroke_width, *filled),
            Cmd::AddFootprintText { footprint, text } => self.add_footprint_text(footprint, text.clone()),
            Cmd::EditFootprintText { footprint, id, content, angle, layer, size_um, stroke_width, justify, mirror } => {
                self.edit_footprint_text(footprint, id, content.clone(), *angle, layer.clone(), *size_um, *stroke_width, *justify, *mirror)
            }
            Cmd::DeleteFootprintText { footprint, id } => self.delete_footprint_text(footprint, id),
            Cmd::MoveFootprintText { footprint, id, x, y } => self.move_footprint_text(footprint, id, *x, *y),
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

    fn set_label_side(&mut self, part: &str, side: LabelSide) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        let fps = &mut self.design.placement.as_mut().unwrap().footprints;
        let i = fps.iter().position(|f| f.id == part).expect("caller checked the part is placed");
        fps[i].label = side;
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
        self.design.routing.get_or_insert_with(|| RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] })
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

    fn edit_via(&mut self, id: &str, diameter: Um, drill: Um) -> Result<(), Vec<CheckResult>> {
        if drill <= 0 || diameter <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_via", id, "via drill and diameter must be positive")]);
        }
        if drill >= diameter {
            return Err(vec![CheckResult::fail("ops_bad_via", id, format!("drill {drill} um is not smaller than the {diameter} um pad, so the via has no annular ring"))]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "the board has no routing yet")])?;
        let v = rt.vias.iter_mut().find(|v| v.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "no via with this id")])?;
        v.diameter = diameter;
        v.drill = drill;
        Ok(())
    }

    /// Board Setup > Track Widths & Vias' "Track Widths" list -- a plain
    /// whole-list replace (see `Cmd::SetTrackWidthPresets`'s own doc).
    fn set_track_width_presets(&mut self, widths: &[Um]) -> Result<(), Vec<CheckResult>> {
        if widths.iter().any(|w| *w <= 0) {
            return Err(vec![CheckResult::fail("ops_bad_track", "track_width_presets", "every preset width must be positive")]);
        }
        self.routing_mut().track_width_presets = widths.to_vec();
        Ok(())
    }

    /// Same panel's "Via Sizes" list.
    fn set_via_presets(&mut self, presets: &[ViaPreset]) -> Result<(), Vec<CheckResult>> {
        for p in presets {
            if p.drill <= 0 || p.diameter <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_via", "via_presets", "via drill and diameter must be positive")]);
            }
            if p.drill >= p.diameter {
                return Err(vec![CheckResult::fail("ops_bad_via", "via_presets", "drill must be smaller than diameter")]);
            }
        }
        self.routing_mut().via_presets = presets.to_vec();
        Ok(())
    }

    /// `Cmd::EditTracksAndVias` -- see that variant's own doc. Unknown ids
    /// among `ids` are tolerated (silently match nothing), same convention
    /// `CommitRoute`'s own doc already established for a caller-computed
    /// id list; refused only when `ids` itself is empty (nothing named at
    /// all, almost certainly a caller bug) or an explicit `Value` is out
    /// of range.
    fn edit_tracks_and_vias(&mut self, ids: &[String], track_width: Option<&SizeSpec>, via_size: Option<&ViaSizeSpec>, layer: Option<&str>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_global_edit", "edit_tracks_and_vias", "no items given")]);
        }
        if let Some(l) = layer {
            self.known_layer(l)?;
        }
        if let Some(SizeSpec::Value { um }) = track_width {
            if *um <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_track", "edit_tracks_and_vias", "track width must be positive")]);
            }
        }
        if let Some(ViaSizeSpec::Value { diameter, drill }) = via_size {
            if *drill <= 0 || *diameter <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_via", "edit_tracks_and_vias", "via drill and diameter must be positive")]);
            }
            if *drill >= *diameter {
                return Err(vec![CheckResult::fail("ops_bad_via", "edit_tracks_and_vias", "drill must be smaller than diameter")]);
            }
        }

        let id_set: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        let model = self.model;
        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut().filter(|t| id_set.contains(t.id.as_str())) {
                if let Some(spec) = track_width {
                    t.width = match spec {
                        SizeSpec::NetClass => model.board.width_of(&t.net),
                        SizeSpec::Value { um } => *um,
                    };
                }
                if let Some(l) = layer {
                    t.layer = l.to_string();
                }
            }
            for v in rt.vias.iter_mut().filter(|v| id_set.contains(v.id.as_str())) {
                if let Some(spec) = via_size {
                    match spec {
                        ViaSizeSpec::NetClass => {
                            v.diameter = model.board.via_diameter_of(&v.net);
                            v.drill = model.board.via_drill_of(&v.net);
                        }
                        ViaSizeSpec::Value { diameter, drill } => {
                            v.diameter = *diameter;
                            v.drill = *drill;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// `Cmd::EditTextAndGraphics` -- see that variant's own doc. Same
    /// unknown-id/empty-input tolerance as `edit_tracks_and_vias`.
    fn edit_text_and_graphics(&mut self, shape_ids: &[String], text_ids: &[String], layer: Option<&str>, line_width: Option<Um>, text_size: Option<Um>, text_thickness: Option<Um>) -> Result<(), Vec<CheckResult>> {
        if shape_ids.is_empty() && text_ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_global_edit", "edit_text_and_graphics", "no items given")]);
        }
        // Not `known_layer` -- that validates against the board's *copper*
        // layer list only, and silkscreen/fab/edge layers (where a shape
        // or text normally lives) are deliberately not on it. Same "just
        // non-empty" convention `edit_shape`/`edit_text` already use.
        if matches!(layer, Some(l) if l.is_empty()) {
            return Err(vec![CheckResult::fail("ops_bad_shape", "edit_text_and_graphics", "a layer, if given, cannot be empty")]);
        }
        if let Some(w) = line_width {
            if w <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_shape", "edit_text_and_graphics", "line width must be positive")]);
            }
        }
        if let Some(s) = text_size {
            if s <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_text", "edit_text_and_graphics", "text size must be positive")]);
            }
        }
        if let Some(t) = text_thickness {
            if t <= 0 {
                return Err(vec![CheckResult::fail("ops_bad_text", "edit_text_and_graphics", "text thickness must be positive")]);
            }
        }

        let shape_set: BTreeSet<&str> = shape_ids.iter().map(String::as_str).collect();
        let text_set: BTreeSet<&str> = text_ids.iter().map(String::as_str).collect();
        if let Some(dr) = self.design.drawings.as_mut() {
            for s in dr.shapes.iter_mut().filter(|s| shape_set.contains(s.id())) {
                if let Some(l) = layer {
                    s.set_layer(l.to_string());
                }
                if let Some(w) = line_width {
                    s.set_stroke_width(w);
                }
            }
            for t in dr.texts.iter_mut().filter(|t| text_set.contains(t.id.as_str())) {
                if let Some(l) = layer {
                    t.layer = l.to_string();
                }
                if let Some(s) = text_size {
                    t.size_um = s;
                }
                if let Some(th) = text_thickness {
                    t.stroke_width = th;
                }
            }
        }
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

    /// `pcb_point_editor.cpp`'s zone corner drag/add/remove -- see
    /// `Cmd::SetZoneOutline`'s own doc on why the whole outline is sent.
    fn set_zone_outline(&mut self, id: &str, outline: &[Point]) -> Result<(), Vec<CheckResult>> {
        if outline.len() < 3 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "a zone outline needs at least three points")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")])?;
        z.outline = outline.to_vec();
        Ok(())
    }

    /// `dialog_copper_zones.cpp`'s "OK": see `Cmd::EditZone`'s own doc.
    /// Outline untouched; every other `ZONE_SETTINGS` field replaced
    /// wholesale, same convention `edit_text` already uses for a full
    /// properties-dialog commit.
    #[allow(clippy::too_many_arguments)]
    fn edit_zone(
        &mut self,
        id: &str,
        net: &str,
        layer: &str,
        clearance: Um,
        min_thickness: Um,
        thermal_gap: Um,
        thermal_spoke_width: Um,
        pad_connection: PadConnection,
        priority: u32,
        island_removal_mode: IslandRemovalMode,
        min_island_area: i64,
        fill_mode: FillMode,
        hatch_thickness: Um,
        hatch_gap: Um,
        hatch_orientation_mdeg: Millideg,
        hatch_smoothing_level: i32,
        hatch_smoothing_value: f64,
        hatch_hole_min_area: f64,
        hatch_border_algorithm: i32,
    ) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(layer)?;
        if clearance < 0 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "clearance cannot be negative")]);
        }
        if min_thickness <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "minimum width must be positive")]);
        }
        if thermal_spoke_width < min_thickness {
            // panel_zone_properties.cpp's AcceptOptions(): "Thermal spoke
            // width cannot be smaller than the minimum width."
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "thermal spoke width cannot be smaller than the minimum width")]);
        }
        if fill_mode == FillMode::HatchPattern && (hatch_thickness < min_thickness || hatch_gap < min_thickness) {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "hatch thickness and gap must be at least the minimum width")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")])?;
        z.net = net.into();
        z.layer = layer.into();
        z.clearance = clearance;
        z.min_thickness = min_thickness;
        z.thermal_gap = thermal_gap;
        z.thermal_spoke_width = thermal_spoke_width;
        z.pad_connection = pad_connection;
        z.priority = priority;
        z.island_removal_mode = island_removal_mode;
        z.min_island_area = min_island_area;
        z.fill_mode = fill_mode;
        z.hatch_thickness = hatch_thickness;
        z.hatch_gap = hatch_gap;
        z.hatch_orientation_mdeg = hatch_orientation_mdeg;
        z.hatch_smoothing_level = hatch_smoothing_level;
        z.hatch_smoothing_value = hatch_smoothing_value;
        z.hatch_hole_min_area = hatch_hole_min_area;
        z.hatch_border_algorithm = hatch_border_algorithm;
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

    fn edit_shape(&mut self, id: &str, layer: &str, stroke_width: Um, filled: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "a shape needs a layer")]);
        }
        if stroke_width <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "line width must be positive")]);
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "the board has no drawings yet")])?;
        let s = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])?;
        s.set_layer(layer.into());
        s.set_stroke_width(stroke_width);
        s.set_filled(filled);
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

    /// `Cmd::CommitRoute`: see that variant's own doc comment. Validates
    /// each incoming track/via the same way `add_track`/`add_via` would
    /// (unlike `insert_copies`, whose items are always copies of something
    /// already on the board and so already known-good) -- this router's
    /// output is new geometry, not a copy, so the same guard applies.
    fn commit_route(&mut self, remove_track_ids: &[String], remove_via_ids: &[String], mut tracks: Vec<Track>, mut vias: Vec<Via>) -> Result<(), Vec<CheckResult>> {
        for t in &tracks {
            self.known_net(&t.net)?;
            self.known_layer(&t.layer)?;
            if t.width <= 0 || t.pts.len() < 2 {
                return Err(vec![CheckResult::fail("ops_bad_track", &t.net, "a committed route track needs a positive width and at least two points")]);
            }
        }
        for v in &vias {
            self.known_net(&v.net)?;
            self.known_layer(&v.from_layer)?;
            self.known_layer(&v.to_layer)?;
            if v.drill <= 0 || v.diameter <= 0 || v.drill >= v.diameter {
                return Err(vec![CheckResult::fail("ops_bad_via", &v.net, "a committed route via needs a drill smaller than its diameter, both positive")]);
            }
        }
        if !remove_track_ids.is_empty() || !remove_via_ids.is_empty() {
            if let Some(rt) = self.design.routing.as_mut() {
                rt.tracks.retain(|t| !remove_track_ids.iter().any(|id| id == &t.id));
                rt.vias.retain(|v| !remove_via_ids.iter().any(|id| id == &v.id));
            }
        }
        for t in &mut tracks {
            t.id.clear();
        }
        for v in &mut vias {
            v.id.clear();
        }
        if !tracks.is_empty() || !vias.is_empty() {
            let rt = self.routing_mut();
            rt.tracks.append(&mut tracks);
            rt.vias.append(&mut vias);
            rt.assign_missing_ids();
            self.sort_routing();
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
        self.design.schematic.get_or_insert_with(|| SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], imported_from_kicad: false, title_block: None, sheets: vec![], instance_overrides: vec![] })
    }

    fn find_symbol(&self, id: &str) -> Result<&SymbolInstance, Vec<CheckResult>> {
        self.schematic()?.symbols.iter().find(|s| s.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol instance with this reference")])
    }

    /// Resolve `(id, unit)` to exactly one placed instance's index into
    /// `schematic().symbols`, for the per-instance verbs below (move/drag/
    /// rotate/mirror/delete -- see `Cmd::MoveSymbol`'s own doc on why those,
    /// specifically, need a `unit` to disambiguate and
    /// `EditSymbolFields`/`RenameSymbol` do not). `unit: None` means "the
    /// only instance with this id" -- every caller that predates multi-unit
    /// support, and every single-unit part today, so this is a no-op change
    /// for them: there is exactly one match, and it's returned the same as
    /// `find_symbol` always did. `unit: None` with *more than one* placed
    /// instance (a multi-unit reference) is refused as ambiguous rather
    /// than silently guessing one -- the caller must say which unit.
    /// Returning an index (not a reference) sidesteps the borrow-checker
    /// shape every call site would otherwise hit wanting to resolve this
    /// immutably and then mutate `schematic_mut()` right after.
    fn find_symbol_unit_index(&self, id: &str, unit: Option<u32>) -> Result<usize, Vec<CheckResult>> {
        let sch = self.schematic()?;
        if let Some(u) = unit {
            return sch
                .symbols
                .iter()
                .position(|s| s.id == id && s.unit == u)
                .ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", id, format!("no symbol instance with reference '{id}' unit {u}"))]);
        }
        let matches: Vec<usize> = sch.symbols.iter().enumerate().filter(|(_, s)| s.id == id).map(|(i, _)| i).collect();
        match matches.len() {
            0 => Err(vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol instance with this reference")]),
            1 => Ok(matches[0]),
            n => Err(vec![CheckResult::fail("ops_ambiguous_symbol", id, format!("reference '{id}' has {n} placed units -- specify which one"))]),
        }
    }

    fn move_symbol(&mut self, id: &str, x: Um, y: Um, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        self.schematic_mut()?.symbols[i].at = Point { x, y };
        Ok(())
    }

    /// `G`: move a symbol and drag along the endpoint of every wire the
    /// caller found attached to one of its pins before the drag started
    /// (`(wire index, point index)` pairs into `wires`/`wires[i].pts`) --
    /// see `Cmd::DragSymbol`'s own doc for why resolving "attached" is the
    /// caller's job, not this method's.
    fn drag_symbol(&mut self, id: &str, x: Um, y: Um, attached: &[(usize, usize)], unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        let before = self.schematic()?.symbols[i].at;
        let (dx, dy) = (x - before.x, y - before.y);
        let sch = self.schematic_mut()?;
        for &(wi, pi) in attached {
            if let Some(p) = sch.wires.get_mut(wi).and_then(|w| w.pts.get_mut(pi)) {
                p.x += dx;
                p.y += dy;
            }
        }
        sch.symbols[i].at = Point { x, y };
        Ok(())
    }

    /// `R`/Shift+`R`: quarter turns, same sign convention as `Rotate`
    /// (positive = CCW, matching `sch_edit_tool.cpp`'s own 'R' default).
    fn rotate_symbol(&mut self, id: &str, quarter_turns: u8, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        let sch = self.schematic_mut()?;
        let s = &mut sch.symbols[i];
        let add = (quarter_turns as i64 % 4) * 90_000;
        s.rot = (s.rot as i64 + add).rem_euclid(360_000) as Millideg;
        Ok(())
    }

    /// `X` ("Mirror Horizontally").
    fn mirror_symbol(&mut self, id: &str, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        let sch = self.schematic_mut()?;
        let s = &mut sch.symbols[i];
        s.mirrored = !s.mirrored;
        if s.mirrored {
            s.mirror_y = false; // see Cmd::MirrorSymbolVertical's doc -- never both at once
        }
        Ok(())
    }

    /// `Y` ("Mirror Vertically") -- see `Cmd::MirrorSymbolVertical`'s doc.
    fn mirror_symbol_vertical(&mut self, id: &str, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        let sch = self.schematic_mut()?;
        let s = &mut sch.symbols[i];
        s.mirror_y = !s.mirror_y;
        if s.mirror_y {
            s.mirrored = false;
        }
        Ok(())
    }

    /// `Del` on a symbol: removes just the one placed `(id, unit)` instance
    /// (see `Cmd::DeleteSymbol`'s own doc) -- a multi-unit reference's other
    /// units are untouched.
    fn delete_symbol(&mut self, id: &str, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        self.schematic_mut()?.symbols.remove(i);
        Ok(())
    }

    fn add_wire(&mut self, pts: Vec<Point>, bus: bool) -> Result<(), Vec<CheckResult>> {
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_wire", "wire", "a wire needs at least two points")]);
        }
        self.schematic_mut_or_create().wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus });
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

    fn add_bus_entry(&mut self, at: Point, size: Point) -> Result<(), Vec<CheckResult>> {
        self.schematic_mut_or_create().bus_entries.push(eda_model::ir::BusEntry { id: String::new(), at, size });
        Ok(())
    }

    fn delete_bus_entry(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.bus_entries.len();
        sch.bus_entries.retain(|be| be.id != id);
        if sch.bus_entries.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_bus_entry", id, "no bus entry with this id")]);
        }
        Ok(())
    }

    /// `dialog_erc.cpp`'s "Exclude this violation" -- see `Cmd::AddErcExclusion`'s own doc.
    fn add_erc_exclusion(&mut self, check: &str, location: &str) -> Result<(), Vec<CheckResult>> {
        if location.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_erc_exclusion", check, "this finding has no location to key an exclusion on")]);
        }
        let sch = self.schematic_mut_or_create();
        if !sch.erc_exclusions.iter().any(|e| e.check == check && e.location == location) {
            sch.erc_exclusions.push(ErcExclusion { check: check.into(), location: location.into() });
            sch.erc_exclusions.sort();
        }
        Ok(())
    }

    fn delete_erc_exclusion(&mut self, check: &str, location: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.erc_exclusions.len();
        sch.erc_exclusions.retain(|e| !(e.check == check && e.location == location));
        if sch.erc_exclusions.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_erc_exclusion", location, "no exclusion with this check+location exists")]);
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

    /// `T`: place free-standing text. Unlike `add_label`, an empty
    /// `content` isn't refused here -- the frontend dialog itself requires
    /// non-blank text before it will even submit (same convention
    /// `TextDialog.tsx` already uses for the PCB side's own `add_text`),
    /// so a backend-level refusal would never actually trigger in
    /// practice; this keeps the verb itself simple.
    fn add_sch_text(&mut self, content: &str, at: Point, angle: Millideg, size_um: Um) -> Result<(), Vec<CheckResult>> {
        self.schematic_mut_or_create().texts.push(SchematicText { id: String::new(), content: content.into(), at, angle, size_um });
        Ok(())
    }

    fn delete_sch_text(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.texts.len();
        sch.texts.retain(|t| t.id != id);
        if sch.texts.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_sch_text", id, "no text with this id")]);
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
    /// `Annotate` to number later) -- this only refuses an exact duplicate
    /// of `(id, unit)` already on the sheet, so placing unit 2 of an
    /// already-placed multi-unit reference (same `id`, a different `unit`)
    /// is exactly how another unit of an existing part gets added -- see
    /// `Cmd::AddSymbol`'s own doc.
    #[allow(clippy::too_many_arguments)]
    fn add_symbol(&mut self, id: &str, lib_id: &str, at: Point, rot: Millideg, value: &str, footprint: &str, unit: u32) -> Result<(), Vec<CheckResult>> {
        if id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", "symbol", "a symbol needs a reference designator")]);
        }
        let sch = self.schematic_mut_or_create();
        if sch.symbols.iter().any(|s| s.id == id && s.unit == unit) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", id, format!("reference '{id}' already has unit {unit} on the sheet"))]);
        }
        sch.symbols.push(SymbolInstance { id: id.into(), at, rot, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: value.into(), footprint: footprint.into(), datasheet: String::new() });
        Ok(())
    }

    /// `E`/`V`/`F`: see `Cmd::EditSymbolFields`'s own doc. Applies to
    /// *every* placed unit sharing `id` at once -- Reference/Value/
    /// Footprint/Datasheet are part-wide, not per-unit, so an edit here can
    /// never itself introduce a `unit_value_mismatch`/`different_unit_
    /// footprint` ERC finding the way a hand-edited `.kicad_sch` can.
    fn edit_symbol_fields(&mut self, id: &str, value: Option<&str>, footprint: Option<&str>, datasheet: Option<&str>) -> Result<(), Vec<CheckResult>> {
        self.find_symbol(id)?;
        let sch = self.schematic_mut()?;
        for s in sch.symbols.iter_mut().filter(|s| s.id == id) {
            if let Some(v) = value {
                s.value = v.to_string();
            }
            if let Some(f) = footprint {
                s.footprint = f.to_string();
            }
            if let Some(d) = datasheet {
                s.datasheet = d.to_string();
            }
        }
        Ok(())
    }

    /// `U`: see `Cmd::RenameSymbol`'s own doc. Renames *every* placed unit
    /// sharing `id` together, in one step -- a multi-unit reference's units
    /// are the same physical part, so they always carry the same reference
    /// text; renaming only one of them would desync it from its siblings.
    fn rename_symbol(&mut self, id: &str, new_id: &str) -> Result<(), Vec<CheckResult>> {
        if new_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", id, "a symbol needs a reference designator")]);
        }
        self.find_symbol(id)?;
        if new_id != id && self.schematic()?.symbols.iter().any(|s| s.id == new_id) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", new_id, "a symbol with this reference is already on the sheet")]);
        }
        if new_id == id {
            return Ok(()); // renaming to the same id is a no-op, not an error
        }
        let sch = self.schematic_mut()?;
        let old_prefix = format!("{id}.");
        let new_prefix = format!("{new_id}.");
        for w in &mut sch.wires {
            for p in &mut w.pins {
                if let Some(rest) = p.strip_prefix(&old_prefix) {
                    *p = format!("{new_prefix}{rest}");
                }
            }
        }
        for ps in &mut sch.power_symbols {
            if let Some(rest) = ps.pin.strip_prefix(&old_prefix) {
                ps.pin = format!("{new_prefix}{rest}");
            }
        }
        for nc in &mut sch.no_connects {
            if let Some(rest) = nc.pin.strip_prefix(&old_prefix) {
                nc.pin = format!("{new_prefix}{rest}");
            }
        }
        for s in sch.symbols.iter_mut().filter(|s| s.id == id) {
            s.id = new_id.to_string();
        }
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
    fn annotate(&mut self, reset_existing: bool, order: AnnotateOrder, ids: Option<&[String]>) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        // Scope snapshot, by index, taken before any id mutates: `ids`
        // names symbols by the id the caller/selection saw them under,
        // which `reset_existing` below is about to change for exactly the
        // ones in scope -- re-checking membership against the *new* id
        // after that would wrongly drop them back out of scope.
        let in_scope: Vec<bool> = sch.symbols.iter().map(|s| ids.is_none_or(|list| list.iter().any(|want| want == &s.id))).collect();

        if reset_existing {
            for (i, s) in sch.symbols.iter_mut().enumerate() {
                if !in_scope[i] {
                    continue;
                }
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
        let mut to_number: Vec<usize> = (0..sch.symbols.len()).filter(|&i| sch.symbols[i].id.ends_with('?') && in_scope[i]).collect();
        to_number.sort_by(|&a, &b| {
            let (ya, xa) = (sch.symbols[a].at.y, sch.symbols[a].at.x);
            let (yb, xb) = (sch.symbols[b].at.y, sch.symbols[b].at.x);
            match order {
                AnnotateOrder::YThenX => (ya, xa).cmp(&(yb, xb)),
                AnnotateOrder::XThenY => (xa, ya).cmp(&(xb, yb)),
            }
        });
        for i in to_number {
            let prefix = sch.symbols[i].id.trim_end_matches('?').to_string();
            let prefix = if prefix.is_empty() { "U".to_string() } else { prefix };
            let n = next.entry(prefix.clone()).or_insert(0);
            *n += 1;
            sch.symbols[i].id = format!("{prefix}{n}");
        }
        Ok(())
    }

    // -------------------------------------------------- footprint editor
    //
    // GAPS.md #8. See `Cmd`'s own "footprint editor" section for what each
    // verb means; these are its `Board::apply` targets, following the same
    // shape every PCB-domain verb above already does (validate first,
    // mutate `self.design.footprint_library` second, never the reverse).

    /// `design.footprint_library`, creating an empty one on first use --
    /// same pattern as `routing_mut`/`drawings_mut`.
    fn footprint_library_mut(&mut self) -> &mut FootprintLibrarySection {
        self.design.footprint_library.get_or_insert_with(FootprintLibrarySection::default)
    }

    fn library_footprint_mut(&mut self, name: &str) -> Result<&mut LibraryFootprint, Vec<CheckResult>> {
        self.design
            .footprint_library
            .as_mut()
            .and_then(|l| l.by_name_mut(name))
            .ok_or_else(|| vec![CheckResult::fail("ops_unknown_footprint", name, "this footprint has not been opened in the Footprint Editor yet")])
    }

    /// The same two-tier lookup `ConstraintModel::footprint_of` does
    /// (explicit list by exact/normalized name, then the builtin table),
    /// keyed by a bare name instead of a `Part` -- this editor opens a
    /// footprint by name directly, with no board instance necessarily
    /// involved yet. Does not stamp a builtin's 3D model path the way
    /// `footprint_of` does (that needs a part reference letter to pick
    /// the R/C/D library, which a bare name has no equivalent of); the
    /// Footprint Properties dialog's own 3D Models field covers it.
    fn resolve_named_footprint(&self, name: &str) -> Option<Footprint> {
        if let Some(fp) = self.model.footprints.iter().find(|f| f.name == name) {
            return Some(fp.clone());
        }
        let norm = eda_model::footprint::normalize_name(name);
        if let Some(fp) = self.model.footprints.iter().find(|f| eda_model::footprint::normalize_name(&f.name) == norm) {
            return Some(fp.clone());
        }
        eda_model::footprint::builtin(name)
    }

    fn open_footprint_for_edit(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        if name.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_footprint", "footprint", "a footprint needs a name")]);
        }
        if self.design.footprint_library.as_ref().and_then(|l| l.by_name(name)).is_some() {
            return Ok(()); // already open -- never resets in-progress edits
        }
        let mut lib_fp = match self.resolve_named_footprint(name) {
            Some(fp) => LibraryFootprint::from_engine_footprint(&fp),
            None => LibraryFootprint::new_empty(name),
        };
        lib_fp.assign_missing_ids();
        self.footprint_library_mut().footprints.push(lib_fp);
        Ok(())
    }

    fn delete_library_footprint(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        let lib = self
            .design
            .footprint_library
            .as_mut()
            .ok_or_else(|| vec![CheckResult::fail("ops_unknown_footprint", name, "the project footprint library is empty")])?;
        let before = lib.footprints.len();
        lib.footprints.retain(|f| f.name != name);
        if lib.footprints.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_footprint", name, "no footprint with this name in the library")]);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_footprint_properties(
        &mut self,
        name: &str,
        description: String,
        keywords: String,
        attributes: FootprintAttributes,
        reference_visible: bool,
        value_visible: bool,
        model: Option<String>,
    ) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(name)?;
        fp.description = description;
        fp.keywords = keywords;
        fp.attributes = attributes;
        fp.reference_visible = reference_visible;
        fp.value_visible = value_visible;
        fp.model = model;
        Ok(())
    }

    fn set_footprint_anchor(&mut self, name: &str, at: Point) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(name)?;
        let (dx, dy) = (-at.x, -at.y);
        for p in &mut fp.pads {
            p.at = Point { x: p.at.x + dx, y: p.at.y + dy };
        }
        for g in &mut fp.graphics {
            g.translate(dx, dy);
        }
        for t in &mut fp.texts {
            t.at = Point { x: t.at.x + dx, y: t.at.y + dy };
        }
        Ok(())
    }

    fn update_footprint_on_board(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        self.library_footprint_mut(name)?.published = true;
        Ok(())
    }

    /// `pad.size` must be positive and, for a through-hole/non-plated
    /// kind, the drill must actually leave an annular ring -- reuses
    /// `Footprint::validate`'s own rules (the same ones a placed board
    /// footprint is checked against) on a one-pad probe footprint, rather
    /// than re-deriving a second copy of those rules here.
    fn validate_pad(pad: &LibraryPad) -> Result<(), Vec<CheckResult>> {
        if pad.size.0 <= 0 || pad.size.1 <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_pad", &pad.number, "pad size must be positive")]);
        }
        let probe = Footprint { name: "probe".into(), pads: vec![pad.to_engine_pad()], courtyard: None, model: None };
        let errs = probe.validate();
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }

    fn find_pad_mut<'b>(fp: &'b mut LibraryFootprint, id: &str) -> Result<&'b mut LibraryPad, Vec<CheckResult>> {
        fp.pads.iter_mut().find(|p| p.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_pad", id, "no pad with this id")])
    }

    fn add_pad(&mut self, footprint: &str, mut pad: LibraryPad) -> Result<(), Vec<CheckResult>> {
        pad.id = String::new(); // ids are ours to assign, never the caller's
        Self::validate_pad(&pad)?;
        let fp = self.library_footprint_mut(footprint)?;
        fp.pads.push(pad);
        fp.assign_missing_ids();
        Ok(())
    }

    fn move_pad(&mut self, footprint: &str, id: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        Self::find_pad_mut(fp, id)?.at = Point { x, y };
        Ok(())
    }

    fn rotate_pad(&mut self, footprint: &str, id: &str, quarter_turns: u8) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let pad = Self::find_pad_mut(fp, id)?;
        let delta = (quarter_turns as u32 % 4) * 90_000;
        pad.rot = (pad.rot + delta) % 360_000;
        Ok(())
    }

    fn delete_pad(&mut self, footprint: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let before = fp.pads.len();
        fp.pads.retain(|p| p.id != id);
        if fp.pads.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_pad", id, "no pad with this id")]);
        }
        Ok(())
    }

    fn edit_pad(&mut self, footprint: &str, id: &str, mut pad: LibraryPad) -> Result<(), Vec<CheckResult>> {
        pad.id = id.to_string();
        Self::validate_pad(&pad)?;
        let fp = self.library_footprint_mut(footprint)?;
        let slot = Self::find_pad_mut(fp, id)?;
        *slot = pad;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_pad_properties(&mut self, footprint: &str, source_pad_id: &str, filter_shape: bool, filter_orientation: bool, filter_layers: bool, filter_type: bool) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let src = fp
            .pads
            .iter()
            .find(|p| p.id == source_pad_id)
            .cloned()
            .ok_or_else(|| vec![CheckResult::fail("ops_unknown_pad", source_pad_id, "no pad with this id")])?;
        for p in fp.pads.iter_mut().filter(|p| p.id != source_pad_id) {
            if filter_shape && p.shape != src.shape {
                continue;
            }
            if filter_orientation && p.rot != src.rot {
                continue;
            }
            if filter_layers && p.layers != src.layers {
                continue;
            }
            if filter_type && p.kind != src.kind {
                continue;
            }
            p.size = src.size;
            p.offset = src.offset;
            p.shape = src.shape;
            p.drill = src.drill;
            p.drill_slot = src.drill_slot;
            p.roundrect_ratio = src.roundrect_ratio;
            p.trapezoid_delta = src.trapezoid_delta;
            p.chamfer_ratio = src.chamfer_ratio;
            p.chamfer_corners = src.chamfer_corners;
            p.layers = src.layers.clone();
            p.clearance_override = src.clearance_override;
            p.thermal_gap_override = src.thermal_gap_override;
            p.thermal_spoke_width_override = src.thermal_spoke_width_override;
        }
        Ok(())
    }

    fn renumber_pads(&mut self, footprint: &str, start: u32, prefix: &str, step: u32) -> Result<(), Vec<CheckResult>> {
        if step == 0 {
            return Err(vec![CheckResult::fail("ops_bad_renumber", footprint, "step must be at least 1")]);
        }
        let fp = self.library_footprint_mut(footprint)?;
        let mut order: Vec<usize> = (0..fp.pads.len()).collect();
        // Reading order: top-to-bottom (y), then left-to-right (x) -- see
        // `Cmd::RenumberPads`'s own doc on why this substitutes for
        // source's click/drag-order tool.
        order.sort_by(|&a, &b| (fp.pads[a].at.y, fp.pads[a].at.x).cmp(&(fp.pads[b].at.y, fp.pads[b].at.x)));
        for (i, idx) in order.into_iter().enumerate() {
            fp.pads[idx].number = format!("{prefix}{}", start + i as u32 * step);
        }
        Ok(())
    }

    fn add_footprint_graphic(&mut self, footprint: &str, mut shape: Shape) -> Result<(), Vec<CheckResult>> {
        if shape.layer().is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", footprint, "a shape needs a layer")]);
        }
        if let Shape::Polygon { pts, .. } = &shape {
            if pts.len() < 3 {
                return Err(vec![CheckResult::fail("ops_bad_shape", footprint, "a polygon needs at least three points")]);
            }
        }
        shape.set_id(String::new());
        let fp = self.library_footprint_mut(footprint)?;
        fp.graphics.push(shape);
        fp.assign_missing_ids();
        Ok(())
    }

    fn delete_footprint_graphic(&mut self, footprint: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let before = fp.graphics.len();
        fp.graphics.retain(|s| s.id() != id);
        if fp.graphics.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")]);
        }
        Ok(())
    }

    fn find_footprint_graphic_mut<'b>(fp: &'b mut LibraryFootprint, id: &str) -> Result<&'b mut Shape, Vec<CheckResult>> {
        fp.graphics.iter_mut().find(|s| s.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])
    }

    fn move_footprint_graphic(&mut self, footprint: &str, id: &str, dx: Um, dy: Um) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        Self::find_footprint_graphic_mut(fp, id)?.translate(dx, dy);
        Ok(())
    }

    fn edit_footprint_graphic(&mut self, footprint: &str, id: &str, layer: &str, stroke_width: Um, filled: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "a shape needs a layer")]);
        }
        if stroke_width <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "line width must be positive")]);
        }
        let fp = self.library_footprint_mut(footprint)?;
        let s = Self::find_footprint_graphic_mut(fp, id)?;
        s.set_layer(layer.into());
        s.set_stroke_width(stroke_width);
        s.set_filled(filled);
        Ok(())
    }

    fn add_footprint_text(&mut self, footprint: &str, mut text: Text) -> Result<(), Vec<CheckResult>> {
        if text.layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", footprint, "text needs a layer")]);
        }
        if text.size_um <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_text", footprint, "text size must be positive")]);
        }
        text.id = String::new();
        let fp = self.library_footprint_mut(footprint)?;
        fp.texts.push(text);
        fp.assign_missing_ids();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_footprint_text(
        &mut self,
        footprint: &str,
        id: &str,
        content: String,
        angle: Millideg,
        layer: String,
        size_um: Um,
        stroke_width: Um,
        justify: TextJustify,
        mirror: bool,
    ) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text needs a layer")]);
        }
        if size_um <= 0 {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text size must be positive")]);
        }
        let fp = self.library_footprint_mut(footprint)?;
        let t = fp.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.content = content;
        t.angle = angle;
        t.layer = layer;
        t.size_um = size_um;
        t.stroke_width = stroke_width;
        t.justify = justify;
        t.mirror = mirror;
        Ok(())
    }

    fn delete_footprint_text(&mut self, footprint: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let before = fp.texts.len();
        fp.texts.retain(|t| t.id != id);
        if fp.texts.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")]);
        }
        Ok(())
    }

    fn move_footprint_text(&mut self, footprint: &str, id: &str, x: Um, y: Um) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let t = fp.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.at = Point { x, y };
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
