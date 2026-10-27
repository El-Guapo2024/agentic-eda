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
    Design, Dimension, DimensionSettings, DrawingsSection, DrcExclusion, DrcExclusionKey, ErcExclusion, FillMode, FootprintAttributes, FootprintInstance, FootprintLibrarySection, IslandRemovalMode, Junction, LabelKind, LabelSide, LibraryFill, LibraryFootprint, LibraryPad, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, Millideg, NetLabel, NoConnect, PadConnection, Point, PowerSymbol, RoutingSection, SchLine, SchematicSection, SchematicText, Shape, Side, SheetInstance, SymbolInstance, SymbolLibrarySection, TeardropSettings, Text, TextJustify, Track, Nm, Via, ViaPreset, Wire, Zone,
};
#[cfg(test)]
use eda_model::ir::Group;
use eda_model::rules::{Constraints, MaskPaste, NetClassSettings, StackupSettings, TextGraphicsDefaults};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
pub use pcb_edit::BooleanOp;

pub mod board_setup;
mod review;
mod simulator;
pub mod library_editors;
mod array;
pub use array::ArrayGeometry;
mod array_numbering;
pub use array_numbering::{alphabetic_from_index, ArrayAxis, AxisSpec, NumberingAxes, NumberingType, PadNumberProvider, PadNumbering};
mod fp_array;
mod pad_edit;
mod unused_pads;
mod pad_nets;
mod fp_lib_fields;
mod sym_derive;
mod fix_shapes;
mod fp_edit;
mod fp_models;
mod import_foreign;
pub use import_foreign::imported_intent_file;
mod import_graphics;
pub use import_graphics::{board_items as graphics_board_items, GraphicsFormat, ImportGraphics};
mod import_graphics_sch;
pub use import_graphics_sch::{sch_items as graphics_sch_items, symbol_items as graphics_symbol_items, ImportSchGraphics, ImportSymbolGraphics};
mod fp_exchange;
pub use fp_exchange::{board_footprint, footprint_name_of, names_footprint_of, pending_footprint_changes, BoardFootprint, ExchangeOptions, FootprintChange};
mod library_place;
pub mod microwave;
mod outline;
mod part_rename;
mod reannotate;
mod pcb_global_edit;
mod pcb_groups;
mod sch_groups;
mod pcb_paste;
mod specctra;
mod pcb_paste_special;
mod design_blocks;
mod multichannel;
mod multichannel_copy;
mod multichannel_facts;
mod multichannel_geom;
pub use multichannel::{AreaKind, AreaPick, MultichannelCmd, PlacementSources, PotentialArea, RepeatOptions, RepeatPlan, TargetCompat};
pub mod embedded;
pub use embedded::{CollectedModel, EmbeddedCmd, EmbeddedOwner};
pub mod pcb_items;
pub mod kicad10_items;
pub mod drill_chart;
mod pcb_props;
mod pcb_table_builders;
mod pcb_tables;
pub use pcb_tables::TableCmd;
mod pcb_transform;
mod tuning;
mod zone_overrides;
pub mod zone_priority;
pub mod rule_editor;
pub use rule_editor::DrcRuleAction;
mod sch_annotate;
mod sch_clipboard;
pub mod sch_control;
mod sch_sheets;
mod sheet_units;
mod sheets;
pub use sch_annotate::{AnnotateMessage, AnnotateOptions, AnnotateReport, AnnotateScope};
pub use sch_sheets::{unique_sheet_name, SheetFileFinding};
pub use sch_groups::groupable_ids;
pub use pcb_paste_special::PasteMode;
pub use pcb_transform::{flip_layer, FlipDirection};

/// `symbol_editor_pin_tool.cpp`'s three "Push Pin ..." context-menu items
/// (`PushPinLength`/`PushPinNameSize`/`PushPinNumberSize`), folded into one
/// Cmd with a field selector rather than three near-identical variants --
/// see `Cmd::PushPinProperty`'s own doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushPinField {
    Length,
    NameSize,
    NumberSize,
}

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
    Value {
        #[serde(alias = "um")]
        nm: Nm,
    },
}

/// Same idea as [`SizeSpec`], for a via's diameter+drill pair (which
/// always travel together -- there is no meaningful "set the diameter but
/// leave the drill" on its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViaSizeSpec {
    NetClass,
    Value { diameter: Nm, drill: Nm },
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

fn d_true() -> bool {
    true
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
    PlaceAt { part: String, x: Nm, y: Nm },
    /// Move a placed part to a board position, µm, keeping its rotation
    /// and side: what dragging it does. Like `Nudge`, the gates judge
    /// where it lands.
    MoveTo { part: String, x: Nm, y: Nm },
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
    /// Footprint Properties' "Zone connection" and "Clearance" (`FOOTPRINT::SetLocalZoneConnection`, `SetLocalClearance`) on a
    /// placed footprint: how zones connect to every pad of it that does not set its own, and the clearance they keep from
    /// them. `None` inherits (the zone's connection, the rules' clearance). A whole-panel commit; see [`zone_overrides`].
    SetFootprintZoneConnection {
        part: String,
        #[serde(default)]
        zone_connection: Option<PadConnection>,
        #[serde(default)]
        clearance: Option<Nm>,
    },
    /// Pad Properties' zone fields on the pads numbered `pad` of a placed footprint: "Pad connection" (`PAD::SetLocalZoneConnection`:
    /// solid, thermal relief, none, or `None` for "From parent footprint"), "Relief gap", "Spoke width" and "Spoke angle" (millidegrees,
    /// KiCad's sign convention), and the pad's own "Clearance". A whole-panel commit: every field is the pad's own, `None` inherits.
    SetPadZoneOverrides {
        part: String,
        pad: String,
        #[serde(default)]
        zone_connection: Option<PadConnection>,
        #[serde(default)]
        thermal_gap: Option<Nm>,
        #[serde(default)]
        thermal_spoke_width: Option<Nm>,
        #[serde(default)]
        thermal_spoke_angle_mdeg: Option<Millideg>,
        #[serde(default)]
        clearance: Option<Nm>,
    },

    /// Add a hand-drawn copper track. `net` and `layer` are refused if
    /// they do not name a real net / a real copper layer; whether the
    /// track's *path* is any good (wrong net touched, clearance violated)
    /// is left to the routing gates, exactly as it is for the router's own
    /// output -- see the report.
    AddTrack { net: String, layer: String, width: Nm, pts: Vec<Point> },
    /// Remove a track by id.
    DeleteTrack { id: String },
    /// Widen or narrow a track in place. Its id, net, layer and path do
    /// not change.
    SetTrackWidth { id: String, width: Nm },

    /// Add a via.
    AddVia { net: String, x: Nm, y: Nm, drill: Nm, diameter: Nm, from_layer: String, to_layer: String },
    /// Remove a via by id.
    DeleteVia { id: String },
    /// Move a via to a new position. Its id, net, drill, diameter and
    /// layer span do not change.
    MoveVia { id: String, x: Nm, y: Nm },
    /// `dialog_track_via_properties.cpp`'s editable via fields (GAPS.md
    /// #11): pad diameter and drill. Net, position and layer span are
    /// unchanged (KiCad does not let you re-net or re-span an existing via
    /// from this dialog either -- that is what delete-and-redraw is for).
    EditVia { id: String, diameter: Nm, drill: Nm },
    /// `BOARD_DESIGN_SETTINGS::m_TrackWidthList`, replaced wholesale --
    /// the studio's Board Setup "Track Widths & Vias" panel has no
    /// per-entry add/remove Cmd of its own, it just resubmits the whole
    /// list on every change, same spirit as `PasteItems`. The board's own
    /// default (`BoardRules::track_width`) is not part of this list -- a
    /// consumer always offers it as the implicit first entry.
    SetTrackWidthPresets { widths: Vec<Nm> },
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
    /// layer/settings untouched. `holes`, when given, replaces the zone's holes too (a mirror moves the whole polygon set, `ZONE::Mirror`: `m_Poly->Mirror`);
    /// absent leaves them as they are (a corner edit of the outline).
    SetZoneOutline {
        id: String,
        outline: Vec<Point>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holes: Option<Vec<Vec<Point>>>,
    },
    /// `dialog_copper_zones.cpp`'s "OK": replace a zone's net/layer and
    /// every `ZONE_SETTINGS` field at once -- KiCad has no concept of
    /// editing just one field of the panel, the whole thing commits
    /// together. The outline is untouched (see the point editor, GAPS.md
    /// #1's zone item, for corner-level edits).
    EditZone {
        id: String,
        net: String,
        layer: String,
        /// `ZONE_SETTINGS::m_Layers`: the layers the zone is on (the dialog's layer list), `layer` being the first. Absent leaves a zone on several
        /// layers as it is when `layer` is one of them, and puts any other zone on `layer` alone. A zone with no copper layer in its set is a non-copper
        /// zone (it has no net).
        #[serde(default)]
        layers: Option<Vec<String>>,
        clearance: Nm,
        min_thickness: Nm,
        thermal_gap: Nm,
        thermal_spoke_width: Nm,
        pad_connection: PadConnection,
        priority: u32,
        island_removal_mode: IslandRemovalMode,
        min_island_area: i64,
        fill_mode: FillMode,
        hatch_thickness: Nm,
        hatch_gap: Nm,
        hatch_orientation_mdeg: Millideg,
        hatch_smoothing_level: i32,
        hatch_smoothing_value: f64,
        hatch_hole_min_area: f64,
        hatch_border_algorithm: i32,
        /// Task item 3: `ZONE::GetIsRuleArea()` plus its five `DoNotAllow*`
        /// keepout flags -- see `eda_model::ir::Zone`'s own doc. Carried on
        /// this same `Cmd` (not a separate one) because real KiCad edits
        /// them through the identical "zone properties" dialog, just
        /// showing a different panel.
        #[serde(default)]
        is_rule_area: bool,
        #[serde(default)]
        keepout_tracks: bool,
        #[serde(default)]
        keepout_vias: bool,
        #[serde(default)]
        keepout_pads: bool,
        #[serde(default)]
        keepout_copper_pour: bool,
        #[serde(default)]
        keepout_footprints: bool,
        /// `ZONE_SETTINGS::m_cornerSmoothingType` and `m_cornerRadius` ("Outline smoothing" and its size): the outline's corners
        /// are chamfered or filleted before the fill (`ZONE::BuildSmoothedPoly`). `None` leaves what the zone has.
        #[serde(default)]
        smoothing: Option<eda_model::ir::ZoneSmoothing>,
        #[serde(default)]
        corner_radius: Option<Nm>,
        /// The rule area's Placement tab (`DIALOG_RULE_AREA_PROPERTIES`: No placement, or the sheet, component class or group whose footprints the area stands
        /// for; [`eda_model::multichannel::PlacementArea`]). `None` leaves what the zone has.
        #[serde(default)]
        placement: Option<eda_model::multichannel::PlacementArea>,
    },

    /// Task item 4: Board Setup > Teardrops' settings page -- whole-struct
    /// replace, same "no per-field Cmd" spirit `SetTrackWidthPresets`/
    /// `SetViaPresets` already use.
    SetTeardropSettings { settings: TeardropSettings },
    /// "Add Teardrops" (the dialog's own apply button, not a per-item
    /// tool): regenerate the board's whole teardrop set from the current
    /// settings, tracks, vias and pads -- `eda_connectivity::teardrop::
    /// generate_teardrops`. Replaces (not appends to) whatever this
    /// command's own previous run left behind, so running it twice in a
    /// row is idempotent; a user-drawn zone is never touched (only zones
    /// with `Zone::teardrop == true` are replaced).
    AddAllTeardrops,
    /// "Remove All Teardrops": drop every zone with `Zone::teardrop ==
    /// true`. Does not disable the feature (`TeardropSettings::enabled`
    /// is untouched) -- same split upstream's own dialog buttons have.
    RemoveAllTeardrops,

    // ------------------------------------------------------------ groups
    //
    // `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp`: implemented in pcb_groups.rs. Groups nest -- a
    // member id may be another group's.
    /// Ctrl+G: create a new group from `ids` (parts, tracks, vias, zones, shapes, texts, dimensions, groups). Refused with fewer
    /// than 2 ids -- a one-item "group" is meaningless, matching source's own `ACTIONS::group.Enable( selectionCount >= 2 )`.
    /// A named id that is itself a group becomes a member of the new one (it nests). Each id leaves the group it was in; when
    /// they all shared one, the new group takes their place in it.
    Group { ids: Vec<String> },
    /// Ctrl+Shift+G: dissolve every named group, releasing its members (which stay on the board, exactly where they are, just no
    /// longer grouped, nor in the group above the dissolved one either -- `GROUP_TOOL::Ungroup`'s `RemoveAll()`). An id that
    /// does not name a group is silently skipped, not refused -- the dialog's own context menu only ever sends real group ids,
    /// but a stale id from a since-changed board should not abort the rest of the batch.
    Ungroup { ids: Vec<String> },
    /// `ACTIONS::addToGroup`: add `ids` to an existing group, pulling each one out of whatever other group it already belonged to
    /// first (an item is only ever in one group at a time). A group is refused as a member of itself or of a group below it.
    AddToGroup { group_id: String, ids: Vec<String> },
    /// `ACTIONS::removeFromGroup`: remove `ids` from whatever group each currently belongs to (a no-op for one that isn't in any
    /// group). A group left with fewer than 2 members is dissolved entirely -- `GROUP_TOOL::RemoveFromGroup`'s own
    /// `if (group->GetItems().size() < 2) group->RemoveAll()` rule, ported exactly.
    RemoveFromGroup { ids: Vec<String> },
    /// `DIALOG_GROUP_PROPERTIES::TransferDataFromWindow` (`ACTIONS::groupProperties`, Group Properties...): rename the group `id` and
    /// make its members exactly `member_ids` -- each one pulled out of whatever other group it was in (an item is in one group at a
    /// time), the group's own previous members that are not listed released (an emptied group goes). The name is `PCB_GROUP::SetName`; the dialog's "Library link:" box is
    /// `design_block` (`Some("")` clears the design block link, `Some(text)` sets it as `LIB_ID::Parse( text, true )` reads it, `None` leaves it); its Locked box is a
    /// `set_locked` sent with it in one batch. A member may be a group, but not this group or one that holds it.
    EditGroup {
        id: String,
        name: String,
        member_ids: Vec<String>,
        #[serde(default)]
        design_block: Option<String>,
    },
    /// What a successful Save Selection as Design Block / Update Design Block from Selection does to the board (`PCB_EDIT_FRAME::SaveSelectionAsDesignBlock`,
    /// `UpdateDesignBlockFromSelection`): `ids` -- the selection, a group included -- are linked to `block` (`Lib:Name`). A lone group with no link takes it ("Set Group Design
    /// Block Link"); anything else goes in a new group named after the block and linked to it ("Group Items"; it may hold one item), each item leaving the group it was in.
    LinkDesignBlock { ids: Vec<String>, block: String },
    /// Place Design Block (`pcbnew.InteractiveDrawing.placeDesignBlock`, `PCB_CONTROL::AppendDesignBlock` -> `AppendBoard`): the block's board file -- `text`, a board file's
    /// text, the same text Append Board reads -- put on the board with the file's origin (the point Append Board holds it by, the studio's server finds it) at `at`. The
    /// references the file has are kept (`AppendBoard` never renumbers: "Don't reannotate dupes on Append Board"). With `as_group` and at least two items at its top, what
    /// came in is put in one group named `group_name` (the block's name when that is empty) and linked to `block` (`Lib:Name`): `placeAsGroup`, `SetDesignBlockLibId`. One undo
    /// step. See `design_blocks.rs`.
    PlaceDesignBlock {
        text: String,
        at: Point,
        block: String,
        #[serde(default)]
        as_group: bool,
        #[serde(default)]
        group_name: String,
    },

    // ------------------------------------------------------------ arrays
    //
    // Task item 6: `pcbnew.Array.createArray` (Ctrl+T) --
    // `pcbnew/tools/array_tool.cpp`'s `ARRAY_TOOL::CreateArray`.
    /// `pcbnew.Array.createArray` (Ctrl+T), `ARRAY_TOOL::CreateArray` -- see [`array`]. `arrange: false` (the dialog's default,
    /// "Duplicate selection") makes a copy of the whole selection for every point of the array but one -- footprints, tracks, vias,
    /// zones, graphics, text, dimensions and groups -- and moves the selection to the other. `arrange: true` ("Arrange selection") makes
    /// nothing and puts the selected items, in the order given, on the points.
    CreateArray {
        ids: Vec<String>,
        geometry: ArrayGeometry,
        #[serde(default)]
        arrange: bool,
        /// "Assign unique reference designators" (the dialog's default, `ShouldReannotateFootprints`): each copy of a footprint takes the
        /// next free number. False is "Keep original reference designators": the copies show the original's text.
        #[serde(default = "d_true")]
        reannotate: bool,
    },

    // ------------------------------------------------------- dimensions
    //
    // Task item 7: `pcbnew/pcb_dimension.{h,cpp}` -- see `eda_model::ir::
    // Dimension`'s own doc for the storage shape and
    // `eda_connectivity::dimension` for the geometry/text this crate
    // never duplicates. `id` on `dimension`, if the caller sent one, is
    // ignored, same as every other "whole struct" add/edit Cmd.
    AddDimension { dimension: Dimension },
    DeleteDimension { id: String },
    /// Translate both feature points by `(dx, dy)` -- what dragging a
    /// dimension does (`PCB_DIMENSION_BASE::Move`).
    MoveDimension { id: String, dx: Nm, dy: Nm },
    /// Properties dialog's OK: replace every field at once (kind,
    /// feature points included) -- same "whole panel commits together"
    /// shape `EditZone`/`EditPad` already have. `id` on `dimension` is
    /// ignored the same way.
    EditDimension { id: String, dimension: Dimension },
    /// Board Setup > Dimension Properties: whole-struct replace, applied
    /// to new dimensions from then on -- never retroactively, matching
    /// source's own `StyleFromSettings` being called once, at creation.
    SetDimensionSettings { settings: DimensionSettings },

    // ------------------------------------------------------- Board Setup
    //
    // `pcbnew/dialogs/dialog_board_setup.cpp`'s pages: each stores one page of the dialog in the design
    // (`eda_model::rules::RulesOverlay`), which the loader lays over the intent's rules -- see `board_setup`.
    // Whole page in, whole page replaced, validated like the page's own panel.
    /// Design Rules > Net Classes: the Default class's sizes and the classes with their net-name patterns.
    SetNetClasses { settings: NetClassSettings },
    /// Design Rules > Constraints: the board-wide minimums (clearance, track width, annular ring, holes, edge, ...).
    SetConstraints { constraints: Constraints },
    /// Board Stackup > Solder Mask/Paste.
    SetMaskPaste { settings: MaskPaste },
    /// Text & Graphics > Defaults.
    SetTextGraphicsDefaults { settings: TextGraphicsDefaults },
    /// Board Stackup > Physical Stackup: the copper layer count and the layers' thickness.
    SetStackup { settings: StackupSettings },
    /// Design Rules > Violation Severity: DRC settings key -> `error` | `warning` | `ignore`.
    SetRuleSeverities { severities: BTreeMap<String, String> },
    /// Design Rules > Custom Rules: the text of the board's `.kicad_dru`.
    SetCustomRules { text: String },
    /// The DRC Rule Editor (`pcbnew.DRETool.drcRuleEditor`): Save, Delete, Move Up or Move Down on one rule of the `.kicad_dru`, the file written again as
    /// the editor writes it (`rule_editor.rs`). `entry` is the rule's place in the editor's list and `name` its name there; `rule` is what Save sends.
    EditDrcRule {
        action: DrcRuleAction,
        #[serde(default)]
        entry: Option<usize>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        rule: Option<eda_drc::rule_editor::RuleEdit>,
    },
    /// Board Stackup > Board Editor Layers (`panel_setup_layers.cpp`): the non-copper layers the board has (the technical ones and User.1 to User.45), the
    /// names the user gave its layers and their types. The whole page; refused with the layer's name when a name or type is out of the panel's rules or
    /// a layer is turned off while items are on it.
    SetBoardLayers { settings: eda_model::layers::LayerSetup },
    /// Text & Graphics > Formatting (`panel_setup_formatting.cpp`): the dash and gap ratios of dashed lines and the "apply board defaults to footprint .." boxes.
    SetFormatting { settings: eda_model::setup_pages::FormattingSettings },
    /// Board Stackup > Zone Hatch Offsets (`panel_setup_zone_hatch_offsets.cpp`): the whole page, copper layer -> the offset of a hatched zone's pattern (a layer left out has none).
    SetZoneHatchOffsets { offsets: eda_model::setup_pages::HatchOffsets },
    /// Design Rules > Length-tuning Patterns (`panel_setup_tuning_patterns.cpp`): what a tuning pattern of each kind starts from.
    SetTuningPatterns { settings: eda_model::setup_pages::TuningPatterns },
    /// Design Rules > Tuning Profiles (`panel_setup_tuning_profiles.cpp`): the profiles and the profile each net class names.
    SetTuningProfiles { settings: eda_model::setup_pages::TuningProfiles },
    /// Text Variables (`panel_text_variables.cpp`): the project's variables, name -> value, the whole set. `${NAME}` in a text, a field or the title
    /// block shows the value, and the derived `.kicad_pro` carries them (`text_variables`) so kicad-cli's plots show the same.
    SetTextVariables { variables: BTreeMap<String, String> },

    // ------------------------------------------------------- DRC and ERC review
    //
    // `pcbnew/dialogs/dialog_drc.cpp` and `eeschema/dialogs/dialog_erc.cpp`: waiving a violation, and the schematic's per-check
    // severities (`common/dialogs/panel_setup_severities.cpp`). See `review`.
    /// "Exclude this violation", "Exclude with comment...", "Edit exclusion comment..." and "Exclude all violations of ..." (and Exclude
    /// Marker) on the board: waive the DRC violations `exclusions` name, as one step. An exclusion whose `(check, items)` is waived already
    /// is replaced by this one (that edits its comment). Refused when `exclusions` is empty or one names no check or no item.
    AddDrcExclusions { exclusions: Vec<DrcExclusion> },
    /// "Remove exclusion for this violation": put the violations `exclusions` name back in the report. Refused when none of them is waived.
    DeleteDrcExclusions { exclusions: Vec<DrcExclusionKey> },
    /// Delete Marker on an excluded marker and Delete All Markers with "Errors, Warnings and Exclusions" (`DIALOG_DRC::OnDeleteOneClick`,
    /// `OnDeleteAllClick`): the markers go, and `BOARD::RecordDRCExclusions` -- which the next run and the next save make -- keeps the exclusions of the
    /// markers that are left, so with all of them gone no exclusion is. (One marker's is `DeleteDrcExclusions`.) Refused when none is listed.
    ClearDrcExclusions,
    /// Schematic Setup > Violation Severity: the ERC checks reported at a severity other than their default, by settings key
    /// (`eda_model::erc_checks`) -> `error` | `warning` | `ignore`. Whole table in, whole table replaced.
    SetErcSeverities { severities: BTreeMap<String, String> },

    /// Task item 8: `GLOBAL_EDIT_TOOL::SwapLayers`/`DIALOG_SWAP_LAYERS`.
    /// `mapping` is "move items on this layer to that one" pairs (a
    /// layer absent from `mapping` is left untouched, matching source's
    /// own per-row grid where every row defaults to itself). Applies to
    /// every track/zone/shape/text/dimension whose own `layer` matches a
    /// key, and to a blind/buried via's layer pair (a through via --
    /// spanning both outer layers -- is source's own skipped
    /// `if (via->GetViaType() == VIATYPE::THROUGH) continue;` case);
    /// footprints aren't touched either, same as
    /// source (a footprint's own "layer" is just `Side`, not a copper
    /// layer this swaps). Each destination layer must already be one of
    /// the board's own copper layers.
    SwapLayers { mapping: Vec<(String, String)> },

    /// `BOARD_EDITOR_CONTROL::modifyLockSelected` (`L`, "Toggle Lock"):
    /// set (`locked: true`) or clear (`false`) `BOARD_ITEM::SetLocked` on
    /// every id -- a placed part's reference, or a track/via/zone/shape/text
    /// id. The caller resolves source's TOGGLE mode (any selected item
    /// already locked -> unlock all, else lock all) before sending this, so
    /// one Cmd is one `commit.Push( "Lock" / "Unlock" )`. An unknown id
    /// refuses the whole Cmd (nothing partially applied).
    SetLocked { ids: Vec<String>, locked: bool },
    /// `EDIT_TOOL::Swap` (`Alt+S`) for footprints: `parts` is in selection
    /// order and, exactly like source's `for i in 0..n-1: swap(sorted[i],
    /// sorted[i+1])`, the result is a cyclic shift of the whole pose
    /// (position, orientation AND board side) -- the first part ends on the
    /// second's pose, ..., the last on the first's. Two parts is a plain
    /// pose swap. One Cmd so the whole chain is one undo step.
    SwapChain { parts: Vec<String> },
    /// `EDIT_TOOL::BooleanPolygons` (Merge / Subtract / Intersect Polygons,
    /// `item_modification_routine.cpp`): `ids` are rectangles, circles and
    /// polygons in the order the routine takes them (the first is the base
    /// and the donor of layer/width/fill). The consumed shapes are deleted
    /// and the result added as polygons -- see [`pcb_edit::boolean_shapes`].
    BooleanShapes { operation: BooleanOp, ids: Vec<String> },
    /// `ZONE_CREATE_HELPER::performZoneCutout` (`Shift+C`, "Add a Zone
    /// Cutout"): subtract the closed polygon `cutout` from zone `id`'s
    /// outline (`SHAPE_POLY_SET::BooleanSubtract`) and replace the zone by
    /// one copy of itself per resulting main outline, same settings, fill
    /// dropped (`UnFill`; fills are always derived here). This IR's zone has
    /// one outline and no holes, so a cutout lying wholly inside the zone --
    /// which source keeps as a real hole -- is stored fractured (the
    /// hole joined to the outline by a zero-width slit, `SHAPE_POLY_SET::
    /// Fracture`): the same copper area, so fills and DRC agree.
    ZoneCutout { id: String, cutout: Vec<Point> },
    /// `pcbnew.EditorControl.zoneMerge` (`BOARD_EDITOR_CONTROL::ZoneMerge`): the zones among `ids` that touch the
    /// first one (same net, kind and layer) joined into it -- see [`board_control::merge_zones`].
    MergeZones { ids: Vec<String> },
    /// `pcbnew.EditorControl.zonePriority{MoveToTop,Raise,Lower,MoveToBottom}`: where zone `id` sits in the fill
    /// order among the zones it overlaps -- see [`board_control::set_zone_priority`].
    SetZonePriority { id: String, to: board_control::ZonePriorityMove },
    /// `pcbnew.EditorControl.drillOrigin` / `drillResetOrigin`: the drill/place file origin
    /// (`DrawingsSection::aux_origin`); `None` is the reset to (0, 0).
    SetAuxOrigin { at: Option<Point> },
    /// `common.Control.gridSetOrigin` / `gridResetOrigin` / `editGridOrigin` (`PCB_CONTROL::DoSetGridOrigin`): the point the editing grid is anchored
    /// at (`DrawingsSection::grid_origin`); `None` is the reset to (0, 0). The placement snap of the verbs below follows it.
    SetGridOrigin { at: Option<Point> },
    /// `common.Control.pageSettings` on the board (`BOARD_EDITOR_CONTROL::PageSettings`): the board's paper and title block, set
    /// together as one undo step -- see [`page_settings::set_board_page`]. Refused when nothing changes.
    SetBoardPage { page: eda_model::page::PageSettings, title_block: eda_model::ir::TitleBlock },
    /// `common.Control.pageSettings` on the schematic (`SCH_EDITOR_CONTROL::PageSetup`): the same for the schematic -- see
    /// [`page_settings::set_schematic_page`].
    SetSchematicPage { page: eda_model::page::PageSettings, title_block: eda_model::ir::TitleBlock },
    /// `pcbnew.Control.repairBoard` (`BOARD_EDITOR_CONTROL::RepairBoard`): see [`board_control::repair_board`].
    /// Refused when there is nothing to repair, so no empty undo step is pushed.
    RepairBoard,
    /// The board items the IR gained with GAPS.md item 24 (package P16): text boxes, reference images, points and the hatch fill of shapes -- one verb family, see
    /// [`pcb_items::PcbItemCmd`]. On the wire: `{"op": "pcb_item", "verb": "add_text_box", "text_box": {..}}`.
    PcbItem(pcb_items::PcbItemCmd),
    /// The files embedded in a board, a schematic, a footprint or a symbol (GAPS.md items 24 and 30): the Embedded Files panel's Add, Remove and Embed fonts, the tool actions
    /// `common.Embed.embededFile` and `removeFile`, and Collect and Embed 3D Models -- one verb family, see [`embedded::EmbeddedCmd`]. On the wire:
    /// `{"op": "embedded", "verb": "add", "owner": {"kind": "board"}, "name": "part.step", "data": "<base64>"}`.
    Embedded(embedded::EmbeddedCmd),
    /// The microwave tools (`pcbnew.MicrowaveTool.*`, GAPS.md item 9, package P19): a gap, a stub, an arc stub, a polygonal shape or an inductor as a footprint of its own, one verb family, see
    /// [`microwave::MicrowaveCmd`]. On the wire: `{"op": "microwave", "verb": "gap", "at": {..}, "size_nm": 500000, "track_width_nm": 250000}`.
    Microwave(microwave::MicrowaveCmd),
    /// The multichannel tool (GAPS.md items 24 and 32, package P24): Generate Placement Rule Areas, Repeat Layout and Apply Design Block Layout -- one verb family, see
    /// [`multichannel::MultichannelCmd`]. On the wire: `{"op": "multichannel", "verb": "repeat_layout", "reference": "zone_..", "targets": [..]}`.
    Multichannel(multichannel::MultichannelCmd),
    /// File > Import > Specctra Session... (`pcbnew.EditorControl.importSpecctraSession`, `PCB_EDIT_FRAME::ImportSpecctraSession`, `SPECCTRA_DB::FromSESSION`): the wires and
    /// vias of a Specctra session file (`session`, its text; `name`, the file it came from, for messages) in place of the board's tracks and vias that are not locked, and
    /// the footprints moved where its placement puts them. One undo step; see [`specctra`](crate::specctra). On the wire: `{"op": "specctra", "session": "(session ..)"}`.
    Specctra {
        session: String,
        #[serde(default)]
        name: String,
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
    MoveShape { id: String, dx: Nm, dy: Nm },
    /// `dialog_pcb_shape_properties.cpp`'s editable fields (GAPS.md #11):
    /// layer, line width, and filled/unfilled. Geometry itself has no
    /// dialog field to edit in source either (dragging the shape's own
    /// points is the only way) -- left for a future point editor.
    EditShape { id: String, layer: String, stroke_width: Nm, filled: bool },

    /// Add free text.
    AddText { text: Text },
    /// Replace a text's content and styling in place. Its id and position
    /// do not change (see `MoveText` for that).
    EditText { id: String, content: String, angle: Millideg, layer: String, size_nm: Nm, stroke_width: Nm, justify: TextJustify, mirror: bool },
    /// Remove a text by id.
    DeleteText { id: String },
    /// Move a text to a new position, keeping its id and styling.
    MoveText { id: String, x: Nm, y: Nm },
    /// `dialog_global_edit_text_and_graphics.cpp`'s "Apply and Close", scoped to what this model has of the dialog's items: free-standing board
    /// graphics (`Shape`), board text (`Text`) and board dimensions -- no footprint reference/value fields, tables or barcodes exist as editable
    /// board items here, see PARITY-pcb.md section 2. `shape_ids`/`text_ids`/`dimension_ids` are already filtered by the caller (scope, layer,
    /// "selected only" are UI concepts, same split `EditTracksAndVias` uses). "Set to specified values:" is the fields below, each `None` leaving
    /// that property alone (source's `INDETERMINATE_ACTION`); `layer_defaults` is "Set to layer default values:" (`StyleFromSettings`, the
    /// Board Setup > Text & Graphics defaults), which ignores the fields below. See `pcb_global_edit.rs`.
    EditTextAndGraphics {
        #[serde(default)]
        shape_ids: Vec<String>,
        #[serde(default)]
        text_ids: Vec<String>,
        #[serde(default)]
        dimension_ids: Vec<String>,
        #[serde(default)]
        layer_defaults: bool,
        #[serde(default)]
        layer: Option<String>,
        /// Shapes' and dimensions' `stroke_width`.
        #[serde(default)]
        line_width: Option<Nm>,
        /// Texts' `size_nm` (applied to both the width and height this
        /// model draws text at -- see `Text::size_nm`'s own doc) and a
        /// dimension's `text_size_nm`.
        #[serde(default)]
        text_size: Option<Nm>,
        /// Texts' `stroke_width` and a dimension's `text_thickness_nm`.
        #[serde(default)]
        text_thickness: Option<Nm>,
    },
    /// `pcbnew.GlobalEdit.removeUnusedPads` (Remove Unused Pads...), `DIALOG_UNUSED_PAD_LAYERS::updatePadsAndVias` -- see `unused_pads.rs`: the plated
    /// through-hole pads (`pads`) and the vias (`vias`) the dialog picked stop having copper on the layers nothing connects them on (`remove` with `keep_outside`:
    /// "Keep outside layers" -- the pad keeps its front and back ring), or keep every layer again (`remove: false`, "Restore All Layers"). `selected` is "Selected only":
    /// the ids of the selection (a footprint, a pad `REF.NUM`, a via); `None` takes the whole board. One undo step; refused when nothing qualifies.
    RemoveUnusedPads {
        #[serde(default)]
        selected: Option<Vec<String>>,
        #[serde(default)]
        vias: bool,
        #[serde(default)]
        pads: bool,
        #[serde(default)]
        keep_outside: bool,
        remove: bool,
    },
    /// `pcbnew.InteractiveEdit.swapPadNets` (Swap Pad Nets), `EDIT_TOOL::SwapPadNets` -- see `pad_nets.rs`: the nets of the pads `ids` (two or more, in selection order) rotate,
    /// the pad `i` taking the net of the pad `i + 1`, and the tracks, vias and pads the copper connected to them reaches follow. `connected_pads` is the answer to "N unselected pad(s)
    /// are connected to these nets": `false` ignores them, `true` swaps them too; left out, such a swap is refused with `ops_swap_unselected_pads` (the pads in its `detail`)
    /// so that the studio can ask. One undo step.
    SwapPadNets {
        ids: Vec<String>,
        #[serde(default)]
        connected_pads: Option<bool>,
    },
    /// `pcbnew.InteractiveEdit.swapGateNets` (Swap Gate Nets), `EDIT_TOOL::SwapGateNets`: the pads `ids` are in two or more units of one multi-gate footprint (`FOOTPRINT::GetUnitInfo`),
    /// whose nets rotate unit to unit, position by position -- or in one unit, which swaps with the unit called `target`. `connected_pads` as for [`Cmd::SwapPadNets`].
    SwapGateNets {
        ids: Vec<String>,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        connected_pads: Option<bool>,
    },

    /// `EDIT_TOOL::Duplicate` (the studio's Cmd+D): exact copies of the named items in place, under fresh ids -- placed footprints,
    /// tracks, vias, zones, shapes, texts, dimensions and groups (a group with copies of its members). A copied footprint is a new
    /// part (`DrawingsSection::board_parts`) under the next free reference, with its pads on the same nets; a copy of a member of a
    /// group joins that group. An id that names nothing duplicable is skipped; the command is refused only if NONE of the given ids
    /// match anything.
    Duplicate { ids: Vec<String> },
    /// `PCB_CONTROL::Paste` of KiCad's own clipboard text (`pcbnew/kicad_clipboard.cpp`, [`eda_kicad::parse_pcb_clipboard`]): the
    /// items it holds -- footprints, tracks, vias, zones, graphics, text, dimensions and groups -- put on the board with the
    /// clipboard's origin (the copy's reference point) at `at`. The clipboard travels with the command, so a paste works from a
    /// copy made on another board or in KiCad itself, and one undo step takes the whole paste back. See [`pcb_paste`] for how
    /// nets, layers, groups and footprints land.
    PasteClipboard { text: String, at: Point },
    /// `PCB_CONTROL::Paste` from Paste Special... (`DIALOG_PASTE_SPECIAL`): [`Cmd::PasteClipboard`] with the dialog's two answers. `mode` says what is
    /// done with the reference designators of the pasted footprints (the default, [`PasteMode::Keep`], is what a plain Paste does: the reference the
    /// clipboard has stays, even when another footprint has it); `clear_nets` puts the pasted copper and pads on no net instead of mapping their nets
    /// by name. See [`pcb_paste_special`].
    PasteSpecial {
        text: String,
        at: Point,
        #[serde(default)]
        mode: PasteMode,
        #[serde(default)]
        clear_nets: bool,
    },
    /// `BOARD_EDITOR_CONTROL::PlaceFootprint` (`A`): put a footprint of a library on the board at `at`, as a footprint of its own -- a
    /// mounting hole, a fiducial, a test point, a connector nobody drew a symbol for (`DrawingsSection::board_parts`, the same kind of
    /// part a Duplicate or a Paste makes) -- unplaced pads on no net, top side, not turned. `footprint` is `Lib:Name`; `definition`
    /// is its pads, graphics and courtyard, carried with the command (like a paste) so the board needs no library to be redrawn: the
    /// studio's server fills it in from the installed KiCad libraries when it is left out, and it is not needed when the board's model
    /// already resolves the name. `reference` empty takes the next free number of the footprint's usual prefix (`H1` for a
    /// `MountingHole`, `TP1` for a `TestPoint`, ...), `value` empty the footprint's own name. One undo step.
    PlaceFootprint {
        footprint: String,
        at: Point,
        #[serde(default)]
        reference: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        definition: Option<LibraryFootprint>,
    },
    /// File > Import > Non-KiCad Board File... (`pcbnew.EditorControl.openNonKicadBoard`, `BOARD_EDITOR_CONTROL::OpenNonKicadBoard`): the open board is replaced by the one
    /// kicad-cli made of another EDA format's file -- `text`, the `.kicad_pcb` it wrote; `name`, the file it came from, is for messages. Placement, routing and drawings are the
    /// file's, and so are its parts, nets and rules (the server writes them as the model `DrawingsSection::intent_file` names); the schematic stays. One undo step, and an
    /// undo brings the old board back whole. See [`import_foreign`].
    ReplaceBoard {
        text: String,
        #[serde(default)]
        name: String,
        /// The dialog's "Keep KiCad layer names" (`m_ImportKeepKiCadLayerNames`): the layers show their standard names, not the names the other application gave them.
        #[serde(default)]
        keep_kicad_layer_names: bool,
        /// The `.kicad_pro` `kicad-cli import` wrote for the board: the design rules and the net classes the importer set, which the server puts in the board's model.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        /// The custom DRC rules (`.kicad_dru`) the importer wrote beside the board or the file it read (`eda_kicad_engine::foreign::imported_rules`), which the server
        /// sets as the board's custom rules.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rules: Option<String>,
    },
    /// File > Import > Non-KiCad Schematic File... (`eeschema.EditorControl.importNonKicadSchematic`, `SCH_EDIT_FRAME::OnImportProject`): the schematic -- the root sheet and every
    /// sheet under it -- is replaced by the one kicad-cli made of another EDA format's file (`content`, read by `eda_kicad::load_sheet_file`; `name` is the file it came from).
    /// The board stays. One undo step. See [`import_foreign`].
    ReplaceSchematic {
        content: eda_model::sheet_file::SheetFile,
        #[serde(default)]
        name: String,
    },
    /// `DRAWING_TOOL::PlaceImportedGraphics` (File > Import > Graphics..., `pcbnew.InteractiveDrawing.placeImportedGraphics`): a DXF or SVG file as
    /// the board's own lines, arcs, circles, polygons, curves and text on one layer, in one undo step -- see [`import_graphics`] for what is mapped
    /// and how. The file travels with the command.
    ImportGraphics(ImportGraphics),
    /// `SCH_DRAWING_TOOLS::ImportGraphics` (File > Import > Graphics..., `eeschema.EditorControl.importGraphics`): a DXF or SVG file as graphic lines, shapes
    /// and text of the sheet in view, in one undo step -- see [`import_graphics_sch`].
    ImportSchGraphics(ImportSchGraphics),
    /// `SYMBOL_EDITOR_DRAWING_TOOLS::ImportGraphics`: the same into the symbol `lib_id` of the Symbol Editor.
    ImportSymbolGraphics(ImportSymbolGraphics),
    /// Change Footprint(s) (`pcbnew.GlobalEdit.changeFootprint(s)`), Update Footprint(s) from Library (`updateFootprint(s)`) and the footprint a symbol's Footprint field names
    /// (`BOARD_NETLIST_UPDATER::replaceFootprint`): put the parts `references` on `footprint`, or -- `footprint` absent -- each on the library copy of the footprint it is
    /// on. The pose, lock and group stay; the pads, their nets by number, the texts, the attributes, the overrides and the models follow the dialog's `options`
    /// (`PCB_EDIT_FRAME::ExchangeFootprint`; see `fp_exchange.rs`). The part's assignment (`DrawingsSection::footprint_assignments`) and, in a change, its symbol's Footprint
    /// field are set; one undo step on both editors. `library` is the library copy of each footprint the command needs (`Lib:Name` -> footprint), which the studio's
    /// server fills in from the library tables when it is left out: a command has to replay and undo the same on a machine without the libraries.
    ExchangeFootprints {
        references: Vec<String>,
        #[serde(default)]
        footprint: Option<String>,
        #[serde(default)]
        options: ExchangeOptions,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        library: BTreeMap<String, Footprint>,
    },
    /// Geographical Reannotate (`pcbnew.ReannotateTool.ShowReannotateDialog`, `DIALOG_BOARD_REANNOTATE`): the footprints on the board renumbered by where they are, in the order,
    /// scope, prefixes and start numbers of `options` (`eda_model::reannotate`); `selection` is the footprints the "Selection" scope names (the studio's selection). Refused when two
    /// footprints would be left with one reference. The symbols take the new references in the same step (what "Update Schematic from PCB" would have them take) and the board's
    /// parts move with them, so there is no part, pose or route under an old reference; one undo step on both editors.
    ReannotateBoard {
        #[serde(default)]
        options: eda_model::reannotate::ReannotateOptions,
        #[serde(default)]
        selection: Vec<String>,
    },
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
        /// The indices into `tracks` of the new tracks that are locked: a locked track a route started or ended in the middle of is cut in two, and both halves are as
        /// locked as it was (`PNS_KICAD_IFACE::AddItem`: "`if( aItem->IsLocked() ) newBoardItem->SetLocked( true )`").
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        lock_tracks: Vec<usize>,
        /// The new vias of `vias` that are not through vias, as (index into `vias`, type): a blind/buried or micro via the router placed
        /// (`ROUTER_TOOL::handleLayerSwitch`: `sizes.SetViaType( viaType )`; `PNS_KICAD_IFACE::AddItem`: "`via_board->SetViaType( via->ViaType() )`"), or a
        /// displaced via that keeps its type. They go into `DrawingsSection::via_extras`, in the same step.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        via_kinds: Vec<(usize, eda_model::via_extra::ViaKind)>,
    },

    /// What the length tuner (`crates/pns`'s `MeanderPlacer`, `GENERATOR_TOOL::GenEditAction`) makes, as one undo step: remove the tracks of
    /// the line it tuned (`remove_track_ids`, the old pattern's members among them) and the pattern `remove_pattern`, add `tracks`, and add
    /// `pattern` with the tracks of `member_indices` (indices into `tracks`) as its members -- see [`eda_model::tuning::TuningPattern`]. A new
    /// pattern has an empty id; re-tuning one removes it and adds the new record under the same id; removing a pattern is `pattern: None` with the
    /// baseline put back as `tracks`. Unknown track ids are tolerated, as `CommitRoute`'s are.
    CommitTuning {
        #[serde(default)]
        pattern: Option<eda_model::tuning::TuningPattern>,
        #[serde(default)]
        remove_pattern: Option<String>,
        #[serde(default)]
        remove_track_ids: Vec<String>,
        #[serde(default)]
        tracks: Vec<Track>,
        #[serde(default)]
        member_indices: Vec<usize>,
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
    MoveExact { parts: Vec<String>, dx: Nm, dy: Nm, rotate_millideg: i64, pivot: Option<Point> },

    // --------------------------------------------- move, rotate, flip: any item
    //
    // `EDIT_TOOL::Move` / `Rotate` / `Flip` (`pcbnew/tools/edit_tool.cpp`) for every kind of item at once, as one undo
    // step each -- see [`pcb_transform`] for what each item class does with the transform. `ids` are placed parts'
    // references and track, via, zone, shape, text, dimension and group ids, mixed freely; a group stands for its
    // members. Which items a selection holds, which of them a lock keeps out, and about which point a turn or a flip
    // happens are the tool's rules, which the studio applies before it sends one of these
    // (`web/studio/src/kicad-port/pcbTransform.ts`).
    /// Translate every item by `(dx, dy)`. Tracks keep their ids and shapes, a footprint lands on the placement grid.
    MoveItems { ids: Vec<String>, dx: Nm, dy: Nm },
    /// Turn every item about `pivot` by `angle_millideg`, positive clockwise on the screen (the sense of `MoveExact`'s
    /// `rotate_millideg`). Each item turns about the one shared point, and its own orientation follows: a footprint's
    /// `rot`, a text's and a dimension's text angle.
    RotateItems { ids: Vec<String>, pivot: Point, angle_millideg: i64 },
    /// `Change Side / Flip`: mirror every item across the axis through `pivot` and put it on the other side of the
    /// board -- a footprint's side, a track's, zone's, shape's, text's and dimension's layer (`::FlipLayer`), a blind or
    /// buried via's layer pair.
    FlipItems { ids: Vec<String>, pivot: Point, direction: FlipDirection },
    /// `EDIT_TOOL::Mirror` of tracks and arcs (`PCB_TRACK::Mirror`, `PCB_ARC::Mirror`: `MIRROR( m_Start, aCentre, aFlipDirection )` and the end, and an
    /// arc's mid point): mirrored across the axis through `pivot`, in place -- the id, the layer, the net and the group stay.
    MirrorTracks { ids: Vec<String>, pivot: Point, direction: FlipDirection },

    // ------------------------------------------------- Properties panel (see [`pcb_props`])
    //
    // The four properties of the grid that no verb above sets. A grid edit is one `Batch` of these and the others.
    /// "Start X" / "Start Y" / "End X" / "End Y" of a track or an arc (`PCB_TRACK::SetStart`, `SetEnd`): the first and/or last point
    /// moves; a field left out stays. An arc keeps its mid point.
    EditTrack {
        id: String,
        #[serde(default)]
        start: Option<Point>,
        #[serde(default)]
        end: Option<Point>,
    },
    /// "Net" (`BOARD_CONNECTED_ITEM::SetNetCode`) of every named track, via or zone. Only a zone may be given "" (no net).
    SetItemNet { ids: Vec<String>, net: String },
    /// "Name" of a zone (`ZONE::SetZoneName`).
    SetZoneName { id: String, name: String },
    /// A shape's new geometry (and whatever else of it changed), in place: the id and the lock stay. The panel's "Start X", "Radius",
    /// "Width" and the like (`EDA_SHAPE` setters) send the whole shape.
    ReplaceShape { id: String, shape: Shape },

    // ------------------------------------------- footprints as editable objects (see [`fp_edit`])
    /// `DIALOG_FOOTPRINT_PROPERTIES::TransferDataFromWindow`: what is given replaces what the footprint `part` has -- the Reference
    /// field's layout, the Value field's, the user fields (the whole list), the attributes (type, board only, exclude from position files
    /// and BOM, DNP, exempt from the courtyard requirement). What is left out stays. DNP and Exclude from BOM also reach the schematic
    /// symbol, so the BOM kicad-cli writes from it agrees with the board. One undo step.
    EditBoardFootprint {
        part: String,
        #[serde(default)]
        reference: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default)]
        value: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default)]
        fields: Option<Vec<eda_model::fp_edit::UserField>>,
        #[serde(default)]
        attrs: Option<eda_model::fp_edit::FootprintAttrs>,
    },
    /// One field of a footprint on its own (the Properties panel's rows of a `PCB_FIELD`): the Reference, the Value or the user field `name` gets
    /// `layout` (its position, size, layer, ...) and/or, for a user field, `text`. The Reference and Value texts come from the schematic and
    /// are not edited here. Unlike `EditBoardFootprint` it leaves every other field alone, so edits of two fields of one footprint compose.
    EditBoardField {
        part: String,
        name: String,
        #[serde(default)]
        layout: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default)]
        text: Option<String>,
    },
    /// `DIALOG_PAD_PROPERTIES::TransferDataFromWindow` for a pad on the board (the footprint editor's own is `EditPad`): the pad of `part` that `edit` names (its number, and which of the pads that
    /// share it) gets `edit` as its changes -- shape, size, hole, offset, corner radius, clearance and solder mask and paste margins. An
    /// edit that changes nothing puts the pad back as the library has it.
    EditBoardPad { part: String, edit: eda_model::fp_edit::PadEdit },
    /// The 3D Models tab of Footprint Properties (`PANEL_FP_PROPERTIES_3D_MODEL`, [`fp_models`]): the footprint `part`'s whole list of `(model ...)` entries becomes `models`, in
    /// order -- the dialog's OK, and what Migrate 3D Models sends. An empty list is a footprint with no model. A list that comes to what the library gives takes the edit away.
    /// Refused for a file name that is empty or illegal (`FILENAME_RESOLVER::ValidateFileName`) and for numbers the preview panel does not take; one undo step.
    SetBoardModels { part: String, models: Vec<eda_model::footprint::Model3d> },
    /// "Add 3D model" (`OnAdd3DModel`): `model` joins the footprint's list at the end, or before position `at` (0-based). One undo step.
    AddBoardModel {
        part: String,
        model: eda_model::footprint::Model3d,
        #[serde(default)]
        at: Option<usize>,
    },
    /// "Remove 3D model" (`OnRemove3DModel`): the model at `index` (0-based) leaves the list. Refused for a position the list has none at.
    RemoveBoardModel { part: String, index: usize },
    /// Put the model at `from` where `to` is (0-based), the others keeping their order; refused for a position the list has none at.
    MoveBoardModel { part: String, from: usize, to: usize },
    /// Migrate 3D Models (`DIALOG_MIGRATE_3D_MODELS::OnReplaceClick`): every model of every footprint on the board whose file name is a key of `replacements` takes the name it maps to --
    /// the placement stays -- as the 3D Models tab's lists, one undo step ("Migrate 3D model references"). Refused for a new name the tab would not take, for no replacement at all
    /// and when no footprint has a model of those names.
    MigrateBoardModels { replacements: BTreeMap<String, String> },
    /// The file name, offset, scale, rotation, opacity and Show cell of the model at `index` (0-based): what is left out stays. The numbers are KiCad's file's -- millimetres and
    /// degrees, the rotation clockwise as the file stores it (the dialog shows it the other way round).
    EditBoardModel {
        part: String,
        index: usize,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        offset: Option<[f64; 3]>,
        #[serde(default)]
        scale: Option<[f64; 3]>,
        #[serde(default)]
        rotate: Option<[f64; 3]>,
        #[serde(default)]
        opacity: Option<f64>,
        #[serde(default)]
        show: Option<bool>,
    },

    /// Apply `cmds` in order as ONE command: one undo step, one activity
    /// entry, all-or-nothing (the first refused sub-command restores the
    /// board to how it was before the batch and its refusal is returned).
    /// KiCad commits a whole multi-item operation (rotate N items, delete
    /// N items, flip N footprints) with a single `BOARD_COMMIT::Push`;
    /// without this the studio sent one `/api/cmd` -- and so one undo
    /// snapshot -- per item. The batch's undo scope (`Cmd::domain`) is its
    /// first sub-command's, so a batch should stay within one editor.
    Batch {
        #[serde(default)]
        cmds: Vec<Cmd>,
    },

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
    MoveSymbol { id: String, x: Nm, y: Nm, #[serde(default)] unit: Option<u32> },
    /// `G`: move a symbol AND drag along the endpoint of every wire
    /// presently attached to one of its pins (KiCad's rubber-band) --
    /// `attached_wire_endpoints` names which (wire index, endpoint index)
    /// pairs the caller found glued to this symbol before the drag
    /// started, so this moves exactly those points by the same delta
    /// rather than re-deriving attachment from the new position. `unit`:
    /// see `MoveSymbol`'s own doc.
    DragSymbol { id: String, x: Nm, y: Nm, attached_wire_endpoints: Vec<(usize, usize)>, #[serde(default)] unit: Option<u32> },
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

    /// `J` (`eeschema.InteractiveDrawing.placeJunction`, `SCH_DRAWING_TOOLS::SingleClickPlace` ->
    /// `SCH_LINE_WIRE_BUS_TOOL::AddJunction`): an explicit junction at `at` -- the item that joins wires
    /// which merely cross (see `eda_model::ir::Junction`). The caller has already checked the point is
    /// joinable (`IsExplicitJunctionAllowed`: three or more directions meet there); this verb only refuses a
    /// point that already has one (a click on an existing junction does nothing in source either).
    AddJunction { at: Point },
    DeleteJunction { id: String },
    /// `I` (`eeschema.InteractiveDrawingLineWireBus.drawLines`, `SCH_LINE_WIRE_BUS_TOOL::DrawSegments` on
    /// `LAYER_NOTES`): a graphic polyline of at least two points, decoration only (see `eda_model::ir::SchLine`).
    /// `width_nm` 0 is the default line width.
    AddSchLine {
        pts: Vec<Point>,
        #[serde(default)]
        width_nm: Nm,
    },
    DeleteSchLine { id: String },
    /// `S` (`eeschema.InteractiveDrawing.drawSheet`, `SCH_DRAWING_TOOLS::DrawSheet`): place a hierarchical sheet
    /// symbol of `size` at `at`, showing the screen in `file` (a bare `*.kicad_sch` name; `.kicad_sch` is added
    /// when missing). A `file` no sheet uses yet gets a new, empty screen; one that exists is shared
    /// (`EditSheetProperties`' "use the existing file" case). Refused for an empty or already-used sheet name
    /// ("A sheet named ... already exists"), a name with a path separator, or a non-positive size.
    AddSheet { name: String, file: String, at: Point, size: (Nm, Nm) },
    /// Import Sheet... and Draw Sheet from File (`eeschema.InteractiveDrawing.importSheet` / `drawSheetFromFile`, `SCH_DRAWING_TOOLS::ImportSheet` /
    /// `DrawSheet`, with `SCH_EDIT_FRAME::LoadSheetFromFile`): put the schematic file `source` in the design as a sheet -- a sheet symbol `name` of `size`
    /// at `at` showing a copy of the file under `file` (a bare name; made unique when the design has one of that name), and a copy of the screen of each sheet
    /// inside it -- or, with `as_sheet` off, put its contents on the sheet in view (carried to `at`). The symbols are numbered anew, in the `order` and with the
    /// numbering `options` say, unless `keep_annotations` is on, which renumbers only the references the design already holds. Refused for a file that
    /// places itself, symbols that name no library, a name already used on the sheet.
    ///
    /// The verb reads no file: the studio's server reads `source` into `content` (`eda_kicad::load_sheet_file`) before the command runs, so that it replays
    /// and undoes the same anywhere.
    ImportSheet {
        source: String,
        #[serde(default)]
        content: Option<eda_model::sheet_file::SheetFile>,
        #[serde(default = "d_true")]
        as_sheet: bool,
        #[serde(default)]
        name: String,
        #[serde(default)]
        file: String,
        at: Point,
        #[serde(default)]
        size: (Nm, Nm),
        #[serde(default)]
        keep_annotations: bool,
        #[serde(default)]
        order: AnnotateOrder,
        #[serde(default)]
        options: AnnotateOptions,
        /// Place as group (`placeAsGroup`): what comes in -- the file's items, or with `block` the sheet -- is put in a group of its own, named `group_name` (the file's name,
        /// or the block's, when that is left empty) and, for a block, linked to it. A sheet placed from a file alone is not grouped (`DrawSheet` groups only a design block's).
        #[serde(default)]
        as_group: bool,
        #[serde(default)]
        group_name: String,
        /// The design block (`Lib:Name`) the file is the schematic of: placing a block is `SCH_DRAWING_TOOLS::ImportSheet` with `placingDesignBlock`, so the group it makes is
        /// linked to the block (`SetDesignBlockLibId`) and the power symbols are numbered anew too. Empty for a plain file.
        #[serde(default)]
        block: String,
    },
    /// Alt+S (`eeschema.InteractiveEdit.swap`, `SCH_EDIT_TOOL::Swap`): exchange the positions of two schematic
    /// items -- symbols (placed or power), labels and free texts, found by id -- and, for two symbols of the
    /// same library symbol, their orientations. A selection of more than two is a Batch of these in selection
    /// order, which `Swap`'s own loop (`sorted[i]` with `sorted[i + 1]`) turns into a rotation of the positions.
    SwapSchItems { a: String, b: String },
    /// Paste (`common.Interactive.paste`, Ctrl+V), Paste Special (Ctrl+Shift+V) and Duplicate (Ctrl+D) on the schematic
    /// (`SCH_EDITOR_CONTROL::Paste`): add a fragment of items -- what `eda_kicad::parse_clipboard` read from KiCad's clipboard format, or from
    /// the copy of this very design -- to the sheet in view, moved by `(dx, dy)` (where the cursor carried it). The fragment's library symbols
    /// are published into the project when the design has none by that name; the symbols are numbered per `mode` and unique across every
    /// sheet; wires, labels, texts, junctions, no-connects, bus entries, lines and drawn graphics come in with new ids and without locks. One
    /// command, so one undo step. Refused for an empty fragment.
    PasteSch {
        fragment: eda_model::sch_clipboard::SchFragment,
        #[serde(default)]
        dx: Nm,
        #[serde(default)]
        dy: Nm,
        #[serde(default)]
        mode: eda_model::sch_clipboard::PasteMode,
    },
    /// The schematic editor's other edit and drawing tools (lock, break, convert text type, shapes, sheet pins,
    /// ...), one verb family -- see [`sch_edit::SchCmd`]. On the wire: `{"op": "sch_edit", "verb": "...", ...}`.
    SchEdit(sch_edit::SchCmd),
    /// Move, Drag, Rotate, Mirror and Align to Grid for every kind of schematic item -- one verb family, one undo step per command, on the
    /// sheet in view (`OnSheet`) -- see [`sch_move::SchMoveCmd`]. On the wire: `{"op": "sch_move", "verb": "drag", "ids": [...], ...}`.
    SchMove(sch_move::SchMoveCmd),
    /// The board's tables (`PCB_TABLE`): Draw Table's commit, the Table Properties and Table Cell Properties dialogs, add and delete rows and columns, merge and unmerge
    /// cells -- one verb family, see [`pcb_tables::TableCmd`]. On the wire: `{"op": "table", "verb": "add_row", "id": "tbl_..", ..}`. Move, turn, flip, delete, lock and
    /// group a table with the verbs that do the same for any item (`move_items`, `rotate_items`, `flip_items`, `set_locked`, `group`).
    Table(pcb_tables::TableCmd),
    /// The same table verbs on the tables of the sheet in view (`SchExtras::tables`): Draw Table, Table Properties and Table Cell Properties, Add / Delete Row(s) and
    /// Column(s), Merge and Unmerge Cells -- `eda_model::table`'s edits, one copy for both editors (`EDIT_TABLE_TOOL_BASE`). On the wire:
    /// `{"op": "sch_table", "verb": "add_row", "id": "tbl_..", "row": 0}`. A table of a sheet has no layer, and no stackup or characteristics to build.
    SchTable(pcb_tables::TableCmd),
    /// Run `cmd` on the sheet at `sheet` instead of the root: the root-to-here list of `SheetInstance::id`s joined by `/`
    /// (what `GET /api/schematic?sheet=` takes; empty is the root). KiCad edits whichever sheet is open
    /// (`SCH_EDIT_FRAME::GetCurrentSheet`); every schematic verb here acts on one screen, and this names which. A sheet that
    /// is placed more than once shows one screen, so the edit shows in every placement (`SCH_SCREEN` is shared). Refused when the
    /// path names no sheet, or a sheet whose content was never read. One undo step, like the command it carries.
    OnSheet { sheet: String, cmd: Box<Cmd> },
    /// Run `cmd` with the board editor in the group `group` (the one the selection tool has entered): whatever the command adds -- a drawn shape, a
    /// text, a dimension, a routed track, a pasted or duplicated item, a footprint placed -- joins the group when it is in none yet
    /// (`BOARD_COMMIT::Push`: `enteredGroup->AddItem( boardItem )`; `PCB_CONTROL::placeBoardItems` does it for a paste). The command is applied as
    /// itself, one undo step; a group that is gone is not joined. See [`pcb_groups`].
    InGroup { group: String, cmd: Box<Cmd> },
    /// Tools > Reorganize into Module Sheets: turn the current flat schematic into one sheet per functional module (an IC with the
    /// passives that serve it, repeated channels, each connector) with a sheet symbol for each on the root, nets that cross modules
    /// as sheet pins and power as power symbols. Every symbol keeps its reference, value, footprint and fields; only places change,
    /// and the nets stay the nets. Refused when the schematic already has sheets, holds a multi-unit part, or is one module.
    ReorganizeSheets,

    /// `dialog_erc.cpp`'s own "Exclude this violation" (right-click a
    /// finding, or the dialog's own Exclude button): accepts one ERC
    /// finding by its own `(check, location)` key -- the key the studio
    /// applies to kicad-cli's ERC report, so the persisted list needs no
    /// translation.
    /// Refused if `location` is empty (nothing to key on -- the same
    /// findings `Exclusions` itself can never exclude). One already excluded
    /// is replaced, which is how "Edit exclusion comment..." changes the
    /// comment: `comment` is `SCH_MARKER::GetComment` ("Exclude with
    /// comment..."), `marker` the marker as the report said it
    /// (`ErcViolation.marker`; none when it could not) and none leaves the
    /// marker the exclusion had.
    AddErcExclusion {
        check: String,
        location: String,
        #[serde(default)]
        comment: String,
        #[serde(default)]
        marker: Option<eda_model::ir::ErcMarker>,
    },
    /// Un-exclude (the dialog's own "Ignored Tests" tab, "Remove"; Delete
    /// Marker on an excluded marker, which is what makes KiCad forget it --
    /// `RecordERCExclusions` records the markers that are left):
    /// refused if no exclusion with this exact key exists. `sheet`, the sheet path of the finding's marker (`ErcMarker::sheet`), when given keeps the
    /// exclusions of the other placements of a screen two sheets show (their items have one id, and their markers each their own sheet).
    DeleteErcExclusion {
        check: String,
        location: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sheet: Option<Vec<String>>,
    },
    /// Schematic Setup > Formatting > Connection grid spacing (`panel_setup_formatting.cpp`): the grid, in mils, that ERC expects wire ends and pins on
    /// (`SCHEMATIC_SETTINGS::m_ConnectionGridSize`). From `MIN_CONNECTION_GRID_MILS` (25) to 10000; 50, KiCad's default, is stored as none.
    SetConnectionGrid { mils: u32 },
    /// Schematic Setup > Formatting > Inter-sheet References (`panel_setup_formatting.cpp`): whether a global label shows the pages its net is on, its own page among
    /// them, abbreviated, and the prefix and suffix (`SCHEMATIC_SETTINGS::m_IntersheetRefs*`). KiCad's defaults are stored as none. One undo step.
    SetIntersheetRefs { settings: eda_model::sch_extras::IntersheetRefs },
    /// The SPICE simulator's settings and workbook (`crate::simulator`): `ngspice` is the `.kicad_pro`'s `schematic.ngspice`
    /// (`NGSPICE_SETTINGS`, none for KiCad's defaults), `workbook` the text of the `.wbk` Save Workbook writes (`SIMULATOR_FRAME_UI::SaveWorkbook`),
    /// none for no workbook. Both are replaced as sent; refused when nothing changes.
    SetSimulator {
        #[serde(default)]
        ngspice: Option<eda_model::sch_extras::NgspiceSettings>,
        #[serde(default)]
        workbook: Option<String>,
    },
    /// Delete All Markers with "Errors, Warnings and Exclusions"
    /// (`DIALOG_ERC::OnDeleteAllClick`): every marker goes, the excluded ones
    /// too, and KiCad records the markers that are left, so no exclusion
    /// is. Refused when none is listed.
    ClearErcExclusions,

    /// `L`/`Ctrl+L`/`H`: place a net label. `kind`/`shape` follow
    /// `eda_model::ir::LabelKind`'s own vocabulary (local has no shape).
    AddLabel { net: String, at: Point, kind: LabelKind },
    DeleteLabel { id: String },
    /// `T`: place free-standing text -- see `SchematicText`'s own doc for
    /// why this is a separate verb from `AddText` (the PCB one, a
    /// different shape entirely: layer/stroke/justify/mirror, none of
    /// which a schematic has).
    AddSchText { content: String, at: Point, angle_millideg: Millideg, size_nm: Nm },
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
    /// `SCH_SCREEN::AddLibSymbol`: keep a copy of a library symbol's definition with the schematic (KiCad's `lib_symbols` cache), so an instance
    /// that names it draws, exports and passes ERC without the library it came from -- what placing a symbol of an installed library
    /// does first (`SCH_DRAWING_TOOLS::PlaceSymbol` -> `SCH_SCREEN::AddLibSymbol`). Stored as a published entry of the project symbol library,
    /// the one place a definition the intent does not hold is kept (a paste does the same). A no-op when the design already has a definition
    /// under that `lib_id` -- the one in the schematic wins, as in `SCH_EDITOR_CONTROL::Paste`. The studio's server wraps an `AddSymbol`
    /// of an installed symbol and this in one command, so adding the symbol and keeping its definition are one undo step.
    EmbedLibSymbol { symbol: LibrarySymbol },

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
    /// `new_id` is blank or already names another symbol on this sheet,
    /// another sheet, or a footprint of the board.
    ///
    /// The board's part goes with the symbol, in the same step
    /// ([`Board::rename_part`], `part_rename.rs`): its pose, the pads its
    /// routing ends on, its net pins, its fields and edits, its zone
    /// overrides, its lock and its group are under the new reference, and the
    /// intent's part is recorded in `DrawingsSection::renamed_parts` for the
    /// loader to lay over the intent (which no verb edits). One undo step for
    /// both halves ([`Cmd::spans_both_editors`]).
    RenameSymbol { id: String, new_id: String },

    /// Symbol Fields Table "OK/Apply" (`dialog_symbol_fields_table.cpp`'s
    /// `TransferDataFromWindow` -> `FIELDS_EDITOR_GRID_DATA_MODEL::ApplyData`):
    /// the whole batch of staged cell edits and column adds / renames /
    /// removals as ONE verb, so one Ctrl+Z undoes the entire Apply (the
    /// source commits them in one `SCH_COMMIT`). Applied in the order
    /// remove, rename, add, edits (edits address the final names); refused
    /// as a whole -- nothing applied -- if any part is invalid (Reference
    /// edit, unknown symbol, duplicate or mandatory field name). See
    /// [`fields_table::apply_field_changes`].
    SetSymbolFields {
        #[serde(default)]
        edits: Vec<fields_table::FieldEdit>,
        #[serde(default)]
        add_fields: Vec<String>,
        #[serde(default)]
        rename_fields: Vec<fields_table::FieldRename>,
        #[serde(default)]
        remove_fields: Vec<String>,
    },

    /// Find and Replace's "Replace" / "Replace All"
    /// (`SCH_FIND_REPLACE_TOOL::ReplaceAndFindNext` / `ReplaceAll`):
    /// replaces `search.find` with `search.replace` in every matching
    /// searchable item (`EDA_ITEM::Replace`) as one undo step
    /// (`commit.Push( "Find and Replace All" )`). `items` restricts it to
    /// the given [`search::Hit::key`]s -- one key is "Replace" on the
    /// current match; `None` is "Replace All". Replacing a Reference
    /// renames the symbol (cascading like `RenameSymbol`) and only happens
    /// with `search.replace_references`. Refused when nothing matched.
    ///
    /// `all_sheets` is `SCH_FIND_REPLACE_TOOL::ReplaceAll` with the Find dialog's "Search the current sheet only" off: every sheet of the
    /// hierarchy is replaced in (`items` is not used then: a key names an item of one sheet), as one undo step.
    ReplaceText {
        search: search::SearchData,
        #[serde(default)]
        items: Option<Vec<String>>,
        #[serde(default)]
        all_sheets: bool,
    },

    /// `panel_setup_pinmap.cpp::changeErrorLevel`: set one pin-map cell to
    /// `level` (0 = OK, 1 = warning, 2 = error) and its mirror -- the
    /// panel only has the lower triangle and always writes both
    /// `SetPinMapValue( y, x )` and `( x, y )`. `a`/`b` are
    /// `ELECTRICAL_PINTYPE` indexes (0..12). Materializes the default map
    /// into `SchematicSection::erc_pin_map` first when it was absent.
    SetErcPinMapCell { a: usize, b: usize, level: u8 },
    /// `ERC_SETTINGS::ResetPinMap` ("Reset to Defaults"): back to KiCad's
    /// default map -- stored as an absent `erc_pin_map`.
    ResetErcPinMap,

    /// `Ctrl+A` (Annotate): assign reference designators to every
    /// not-yet-annotated symbol (an `id` this project synthesizes as
    /// `"U?1"`/`"U?2"`/... when `AddSymbol` is given a blank prefix+number
    /// -- see `sch_drawing_tools`' own symbol-chooser flow), sorted by
    /// `order` (`dialog_annotate.cpp`'s "Order Options", default top to
    /// bottom). `reset_existing` mirrors the dialog's "Clear and
    /// re-annotate" vs "Keep existing" modes. `ids`, when given, is
    /// `dialog_annotate.cpp`'s own "Selection" scope -- only symbols named
    /// here are reset/renumbered, everything else is left exactly as it
    /// is; `None` is the sheet in view.
    ///
    /// The rest of `DIALOG_ANNOTATE` -- scope (entire schematic, the sheet in view, the selection), the numbering algorithm and start
    /// number, "recursive", "regroup units" -- is `options` (`sch_annotate.rs`); an absent `options` is the dialog before it had them.
    Annotate {
        reset_existing: bool,
        #[serde(default)]
        order: AnnotateOrder,
        #[serde(default)]
        ids: Option<Vec<String>>,
        #[serde(default)]
        options: AnnotateOptions,
    },

    // ----------------------------------------- schematic control (sch_control.rs)
    /// `eeschema.EditorControl.setDNP` / `setExcludeFromBOM` / `setExcludeFromBoard` / `setExcludeFromSimulation`
    /// (`SCH_EDIT_TOOL::SetAttribute`): sets the given attributes (`None` leaves one alone) on every placed unit of every
    /// reference in `ids`. Refused for an empty `ids` or a reference that is not on the sheet.
    SetSymbolAttrs {
        ids: Vec<String>,
        #[serde(default)]
        dnp: Option<bool>,
        #[serde(default)]
        exclude_from_bom: Option<bool>,
        #[serde(default)]
        exclude_from_board: Option<bool>,
        #[serde(default)]
        exclude_from_sim: Option<bool>,
    },
    /// `eeschema.EditorControl.editPageNumber` (`SCH_EDIT_TOOL::EditPageNumber`): the page number of the placement `sheet` (a
    /// `SheetInstance::id`). Letters and digits only; empty puts the sheet back to its place in the hierarchy.
    SetSheetPage { sheet: String, page: String },
    /// `eeschema.Interactive.increment*` (`SCH_TOOL_BASE::Increment`): the new text of a label (its net name) or a free text, by id.
    SetSchItemText { id: String, text: String },
    /// `eeschema.EditorControl.editSymbolLibraryLinks` (`DIALOG_EDIT_SYMBOLS_LIBID`): every symbol linked to a `from` library id is linked to
    /// the paired `to` id instead (`update_fields`: the Datasheet follows the library symbol). Refused for an invalid or unknown `to`.
    SetSymbolLibIds {
        changes: Vec<(String, String)>,
        #[serde(default)]
        update_fields: bool,
    },
    /// `eeschema.EditorControl.incrementAnnotations` (`SCH_EDITOR_CONTROL::IncrementAnnotations`): every reference with the start
    /// reference's letters and a number at least its own moves by `increment`.
    IncrementAnnotations { start: String, increment: i32 },

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
    /// `pcbnew.ModuleEditor.newFootprint` (Ctrl+N, `FOOTPRINT_EDITOR_CONTROL::
    /// NewFootprint` -> `PCB_BASE_FRAME::CreateNewFootprint`): a brand-new
    /// empty footprint called `name` with KiCad's own defaults for one --
    /// `int footprintAttrs = FP_SMD;` (the library it would be saved into
    /// offers no other footprint to infer them from here). Unlike
    /// [`Cmd::OpenFootprintForEdit`] (which re-opens whatever `name` already
    /// resolves to) this REFUSES a name something already defines -- in the
    /// project library, the builtin table, an intent declaration or a
    /// loaded `.kicad_mod` -- because `CreateNewFootprint` makes the name
    /// unique first (`Untitled`, `Untitled_1`, ...): the caller picks that
    /// name (the footprint list it needs is the same one the editor's
    /// picker reads).
    NewFootprint { name: String },
    /// Remove a footprint definition from the project library entirely.
    /// Never touches a board instance naming it (same "explicit, not
    /// automatic" rule `UpdateFootprintOnBoard` documents) -- an instance
    /// left pointing at a now-undefined name simply falls back to the
    /// builtin table/intent resolution, exactly as if this editor had
    /// never touched it.
    DeleteLibraryFootprint { name: String },
    /// Store a whole footprint in the project library under `footprint.name`: the one verb behind
    /// `FOOTPRINT_EDITOR_CONTROL`'s Duplicate, Paste (the copy lands under `<name>_copy`) and Import,
    /// and `SaveFootprintAs`. The caller picks a free name first (`DuplicateFootprint`'s `name_1`,
    /// `name_2`, ...); this refuses a name that is taken -- in the project library, or by anything the
    /// model resolves -- unless `overwrite` (the dialog's "Overwrite" button). Item ids of the stored
    /// copy are assigned here, never taken from the caller. A new entry starts unpublished; an
    /// overwritten one keeps following the board if it already did.
    PutLibraryFootprint {
        footprint: LibraryFootprint,
        #[serde(default)]
        overwrite: bool,
    },
    /// `FOOTPRINT_EDITOR_CONTROL::RenameFootprint`: `name` (a project-library entry) becomes
    /// `new_name`. Refused when `new_name` is taken unless `overwrite`. Never touches a board instance
    /// naming the old name (same "explicit, not automatic" rule as [`Cmd::DeleteLibraryFootprint`]).
    RenameLibraryFootprint {
        name: String,
        new_name: String,
        #[serde(default)]
        overwrite: bool,
    },
    /// `FOOTPRINT_EDITOR_CONTROL::RepairFootprint`: give every pad/graphic/text that repeats another
    /// item's id a fresh one. (A missing id is also filled in.)
    RepairFootprint { name: String },
    /// `dialog_footprint_properties_fp_editor.cpp`'s General tab -- the
    /// whole panel commits together, same shape as `EditZone`.
    EditFootprintProperties {
        name: String,
        description: String,
        keywords: String,
        attributes: FootprintAttributes,
        reference_visible: bool,
        value_visible: bool,
        /// `(model "...")` 3D model path; `None` clears it. The path of the one model of an older entry: a command that sends the footprint's
        /// whole list ([`Cmd::EditFootprintProperties::models3d`]) has it follow the list's first model.
        model: Option<String>,
        /// The 3D Models tab (`PANEL_FP_PROPERTIES_3D_MODEL`): the footprint's whole list of models, in order, each with its offset, scale, rotation, opacity and Show
        /// switch ([`fp_models`]); an empty list is a footprint with no model. Refused for an empty or illegal file name and for numbers the preview panel does not
        /// take. Left out, the footprint keeps the list it has -- unless `model` names another path than its first model's, which is then its one model, as it was
        /// before the list existed.
        #[serde(default)]
        models3d: Option<Vec<eda_model::footprint::Model3d>>,
        /// The Clearances tab's "Zone connection" (`FOOTPRINT::SetLocalZoneConnection`); `None` is "inherited". Part of the
        /// whole panel, so a command that leaves it out clears it.
        #[serde(default)]
        zone_connection: Option<PadConnection>,
        /// The General tab's fields grid (`PCB_FIELDS_GRID_TABLE`, [`fp_lib_fields`]): the Reference's and the Value's layout (the text they show in its `text`) and the user fields in
        /// order, each with its layout. Left out, the footprint keeps what it has. The Show check boxes are `reference_visible` and `value_visible` and each field's `visible`.
        #[serde(default)]
        reference: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default)]
        value: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default)]
        fields: Option<Vec<eda_model::ir::FootprintField>>,
        /// The grid's third and fourth mandatory rows, the Datasheet and the Description fields, each with its text in the layout's `text`. Left out, the footprint keeps
        /// what it has.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        datasheet: Option<eda_model::fp_edit::FieldLayout>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description_field: Option<eda_model::fp_edit::FieldLayout>,
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
    MovePad { footprint: String, id: String, x: Nm, y: Nm },
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
    /// `PAD_TOOL::EnumeratePads`' commit ("Renumber Pads", click each pad in the order it should be
    /// numbered): `numbers` is every `(pad id, new number)` pair the click sequence produced, applied
    /// together -- one undo step -- or not at all when an id is unknown. A pad that is not named keeps
    /// its number.
    SetPadNumbers { footprint: String, numbers: Vec<(String, String)> },
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
    MoveFootprintGraphic { footprint: String, id: String, dx: Nm, dy: Nm },
    EditFootprintGraphic { footprint: String, id: String, layer: String, stroke_width: Nm, filled: bool },
    /// Free-standing text on a footprint, same `Text` type as the PCB
    /// tab's `AddText`.
    AddFootprintText { footprint: String, text: Text },
    EditFootprintText { footprint: String, id: String, content: String, angle: Millideg, layer: String, size_nm: Nm, stroke_width: Nm, justify: TextJustify, mirror: bool },
    DeleteFootprintText { footprint: String, id: String },
    MoveFootprintText { footprint: String, id: String, x: Nm, y: Nm },
    /// `pcbnew.Array.createArray` (Ctrl+T) in the Footprint Editor, `ARRAY_TOOL::CreateArray` with `m_isFootprintEditor` -- see [`fp_array`]: a grid or a
    /// circular array of the pads, graphics and texts `ids` name, in one undo step. `arrange` puts the selected items on the points instead of copying them
    /// (and numbers nothing); `numbering`, when given, renumbers the pads of the array ([`PadNumbering`]: "Renumber pads" with its scheme, direction and
    /// start), and left out the copies keep the number of their source.
    CreateFootprintArray {
        footprint: String,
        ids: Vec<String>,
        geometry: ArrayGeometry,
        #[serde(default)]
        arrange: bool,
        #[serde(default)]
        numbering: Option<PadNumbering>,
    },
    /// `pcbnew.PadTool.explodePad` (Edit Pad as Graphic Shapes, Ctrl+E), `PAD_TOOL::EditPad` with a pad selected -- see [`pad_edit`]: a custom pad's basic shapes
    /// become free graphics of the footprint on the pad's layer, the pad is left as its anchor and the footprint is in pad edit mode
    /// ([`eda_model::ir::PadEditMode`]), until the pad is recombined. One undo step. Refused while a pad is being edited.
    ExplodePad { footprint: String, id: String },
    /// `pcbnew.PadTool.recombinePad` (Finish Pad Edit, Ctrl+E), `PAD_TOOL::EditPad` with a pad being edited -- see [`pad_edit`]: every graphic on the pad's layer
    /// that touches the pad goes back into it as a basic shape. One undo step. Refused when no pad is being edited.
    RecombinePad { footprint: String },

    // ----------------------------------------------------- symbol editor
    //
    // The Symbol Editor tab's own `Domain::SymbolEditor` command set --
    // every verb here targets `design.symbol_library`, mirroring the
    // footprint-editor section above field-for-field where the two
    // editors share a shape (open/delete/edit-properties/update-on-board)
    // and diverging where a symbol's own content differs (pins instead of
    // pads, no per-item "layer", positions in mm not um -- a library
    // symbol's own native unit, see `eda_model::symbol`'s doc).
    /// Open a symbol for editing, creating `design.symbol_library`'s entry
    /// for `lib_id` the first time -- from an already-resolved `LibSymbol`
    /// (a real library file already folded into `ConstraintModel::symbols`,
    /// the builtin table, or an already-placed instance's own resolution)
    /// when `lib_id` resolves to one, else a brand-new empty symbol
    /// (KiCad's "New Symbol" and "Edit Symbol" on an unresolvable name are
    /// the same verb here, same precedent `OpenFootprintForEdit`'s own doc
    /// sets). A no-op, not an error, if already open.
    OpenSymbolForEdit { lib_id: String },
    /// Ctrl+N in the Symbol Editor (`eeschema.SymbolLibraryControl.newSymbol`, `SYMBOL_EDIT_FRAME::CreateNewSymbol`):
    /// a brand-new, empty symbol in the project library. Unlike `OpenSymbolForEdit` (which opens whatever
    /// resolves, falling back to a blank symbol) this *refuses* a `lib_id` that is already taken -- in the
    /// project library or resolvable to a library/builtin symbol -- the way `DIALOG_LIB_NEW_SYMBOL` refuses
    /// a name the library already has.
    NewSymbol { lib_id: String },
    /// Remove a symbol definition from the project library entirely.
    /// Never touches a placed instance naming it -- same "explicit, not
    /// automatic" rule `DeleteLibraryFootprint` documents.
    DeleteLibrarySymbol { lib_id: String },
    /// Store a whole symbol in the project library under `symbol.lib_id`: the one verb behind
    /// `SYMBOL_EDIT_FRAME::DuplicateSymbol` (Duplicate and Paste), `ImportSymbol` and
    /// `saveSymbolCopyAs`. Same contract as [`Cmd::PutLibraryFootprint`]: the caller picks a free name
    /// (`ensureUniqueName`'s `name_1`, `name_2`, ...), a taken one is refused unless `overwrite`, item
    /// ids are assigned here.
    PutLibrarySymbol {
        symbol: LibrarySymbol,
        #[serde(default)]
        overwrite: bool,
    },
    /// `SYMBOL_EDITOR_CONTROL::RenameSymbol`: `lib_id` (a project-library entry) becomes `new_lib_id`.
    /// Never touches a placed instance naming the old id (undo of this editor restores only the symbol
    /// library, so nothing else may change with it).
    RenameLibrarySymbol {
        lib_id: String,
        new_lib_id: String,
        #[serde(default)]
        overwrite: bool,
    },
    /// `SYMBOL_EDITOR_DRAWING_TOOLS::PlaceAnchor` ("Move Symbol Anchor"): `at` (mm, the symbol's own
    /// frame) becomes the new origin -- `symbol->Move( -cursorPos )`, every pin and graphic of every unit
    /// and body style shifts by `-at`.
    SetSymbolAnchor { lib_id: String, at: eda_model::symbol::SPoint },
    /// `dialog_lib_symbol_properties.cpp`'s General + Units&&Body Styles
    /// tabs -- the whole panel commits together, same shape as
    /// `EditFootprintProperties`.
    #[allow(clippy::too_many_arguments)]
    EditSymbolProperties {
        lib_id: String,
        reference_prefix: String,
        description: String,
        keywords: String,
        datasheet: String,
        power: bool,
        in_bom: bool,
        on_board: bool,
        pin_numbers_hidden: bool,
        pin_names_hidden: bool,
        pin_name_offset_mm: f64,
        unit_count: u32,
        has_alternate_body_style: bool,
        footprint_filters: Vec<String>,
        /// The rest of the panel (`TransferDataFromWindow`), each left as the symbol has it when absent: "All units are interchangeable" unchecked
        /// (`LockUnits( GetUnitCount() > 1 && !interchangeable )`), "Define as local power symbol" (with "Define as power symbol"), "Exclude from simulation",
        /// "Exclude from position files".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        units_locked: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        power_local: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclude_from_sim: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclude_from_pos_files: Option<bool>,
    },
    /// KiCad's "Update Symbol from Library": every schematic symbol/
    /// power-symbol instance naming `lib_id` starts resolving its
    /// graphics/pins from here (`crate::board::load`'s overlay, gated on
    /// `published`) -- same explicit, user-triggered switch
    /// `UpdateFootprintOnBoard` already is.
    UpdateSymbolOnBoard { lib_id: String },

    /// Place a pin (`symbol_editor_pin_tool.cpp`'s `CreatePin`). `pin.id`,
    /// if the caller sent one, is ignored -- ids are assigned here, same
    /// as `AddPad`.
    AddSymbolPin { lib_id: String, pin: LibrarySymbolPin },
    /// Move a pin to an absolute position in the symbol's own mm frame.
    MoveSymbolPin { lib_id: String, id: String, x: f64, y: f64 },
    DeleteSymbolPin { lib_id: String, id: String },
    /// Pin Properties dialog's OK: replace every field of one pin at once
    /// (same "whole panel commits together" shape as `EditPad`). `id` on
    /// `pin`, if the caller sent one, is ignored.
    EditSymbolPin { lib_id: String, id: String, pin: LibrarySymbolPin },
    /// `symbol_editor_pin_tool.cpp`'s "Push Pin Length"/"Push Pin Name
    /// Size"/"Push Pin Number Size" -- source's own three context-menu
    /// items, each copying exactly one field from the one selected
    /// (source) pin to every *other* pin on the symbol unconditionally;
    /// folded into one Cmd with a field selector rather than three
    /// near-identical variants.
    ///
    /// `body_style` is `m_frame->GetBodyStyle()` -- the body style the editor is showing. Source's
    /// length push reaches only pins that are shared by every style (`0`) or belong to that one
    /// (`!pin->GetBodyStyle() || pin->GetBodyStyle() == m_frame->GetBodyStyle()`); `None` means the
    /// source pin's own style (what this verb did before the field existed).
    PushPinProperty {
        lib_id: String,
        source_pin_id: String,
        field: PushPinField,
        #[serde(default)]
        body_style: Option<u32>,
    },

    /// Free-standing graphics on a symbol's own unit/body-style, in its
    /// local mm frame -- same shape/validation spirit as the PCB tab's
    /// `AddShape`/the footprint editor's `AddFootprintGraphic`.
    AddSymbolGraphic { lib_id: String, graphic: LibrarySymbolGraphic },
    DeleteSymbolGraphic { lib_id: String, id: String },
    MoveSymbolGraphic { lib_id: String, id: String, dx_mm: f64, dy_mm: f64 },
    /// Shape Properties' editable fields (stroke width, fill) -- geometry
    /// has no dialog field to edit either, only by dragging its own
    /// points (no point editor, same gap `EditShape`'s own doc notes for
    /// the PCB side). `unit` and `body_style` are the dialog's "Common to all units in symbol" and "Common to all body styles"
    /// (`DIALOG_SHAPE_PROPERTIES::TransferDataFromWindow`: 0 for a box that is checked, the unit and the body style the editor is
    /// showing for one that is not); left out they stay as they are. A negative `stroke_mm` is the border switched off, kept as -0.0001 (`STROKE_PARAMS` width -1).
    EditSymbolGraphic {
        lib_id: String,
        id: String,
        stroke_mm: f64,
        fill: LibraryFill,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body_style: Option<u32>,
    },
    /// `LibrarySymbolGraphic::Text`'s own fields (content/angle/size) --
    /// split from `EditSymbolGraphic` the same way the footprint editor's
    /// `EditFootprintText` is split from `EditFootprintGraphic` (a
    /// different field set, not a shape). Position is edited through the
    /// generic `MoveSymbolGraphic` (text is still one `LibrarySymbolGraphic`
    /// entry, addressed by the same `id`).
    EditSymbolText { lib_id: String, id: String, text: String, angle_deg: f64, size_mm: f64 },
    /// The properties dialog of a library graphic that has more to it than a stroke and a fill (`DIALOG_TEXT_PROPERTIES` of a text box: the text, its size, bold and italic, its
    /// alignment and way, the border, the fill, the unit and the body style): `graphic` takes the place of the one with `id` -- the same kind, the same place in the list, the id kept.
    ReplaceSymbolGraphic { lib_id: String, id: String, graphic: LibrarySymbolGraphic },

    // ------------------------------------------------------- derived symbols
    //
    // `(extends "Parent")`: a symbol that inherits its parent's drawing and overrides its fields -- `eda_model::sym_fields`, `crates/ops/src/sym_derive.rs`.
    /// `eeschema.SymbolLibraryControl.deriveFromExistingSymbol` (`DIALOG_LIB_NEW_SYMBOL` with a parent, `LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol`): `lib_id`, a new symbol in
    /// `parent`'s library, derived from `parent` (a project-library entry). The five mandatory fields are copied from the parent (the Footprint and the Datasheet emptied unless
    /// kept); `transfer_user_fields` copies its user fields too (their text emptied unless `keep_content_user_fields`). Refused when the name is taken unless `overwrite`.
    DeriveSymbol {
        parent: String,
        lib_id: String,
        #[serde(default)]
        keep_footprint: bool,
        #[serde(default)]
        keep_datasheet: bool,
        #[serde(default)]
        transfer_user_fields: bool,
        #[serde(default)]
        keep_content_user_fields: bool,
        #[serde(default)]
        overwrite: bool,
    },
    /// `eeschema.SymbolLibraryControl.flattenSymbol` (`SYMBOL_EDITOR_CONTROL::FlattenSymbol`): the derived symbol `lib_id` becomes the flat symbol it stands for -- the root's
    /// drawing and everything it inherits, no parent. Refused for a symbol that is not derived.
    FlattenSymbol { lib_id: String },
    /// `eeschema.SymbolLibraryControl.updateSymbolFields` (`DIALOG_UPDATE_SYMBOL_FIELDS`): the fields `fields` (by name) of the derived symbol `lib_id` follow its parent's (flattened):
    /// the text, the visibility, the sizes and styles and the places, as each box asks; `remove_extras` takes away a field the parent lacks and `reset_empty` lets an empty
    /// parent field empty the symbol's. Refused for a symbol that is not derived ("Symbol is not derived from another symbol.").
    UpdateSymbolFields {
        lib_id: String,
        fields: Vec<String>,
        #[serde(default)]
        remove_extras: bool,
        #[serde(default)]
        reset_empty: bool,
        #[serde(default = "d_true")]
        reset_text: bool,
        #[serde(default = "d_true")]
        reset_visibility: bool,
        #[serde(default = "d_true")]
        reset_effects: bool,
        #[serde(default = "d_true")]
        reset_positions: bool,
    },
    /// `DIALOG_LIB_SYMBOL_PROPERTIES`' "Derive from" choice: the derived symbol `lib_id` extends `parent` instead. The parent must be a symbol of the same library, not the symbol
    /// itself and not one derived from it (that would make a cycle).
    SetSymbolParent { lib_id: String, parent: String },
    /// The Symbol Properties dialog's fields grid (`DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataFromWindow`): the symbol's fields, all at once -- the five mandatory ones and the user
    /// fields with their text, place, size, alignment and visibility. A user field needs a name that is not taken (nor one of the `ki_` properties, which are the symbol's keywords
    /// and footprint filters); one with neither a name nor a text is dropped.
    EditLibrarySymbolFields { lib_id: String, fields: Vec<eda_model::sym_fields::LibrarySymbolField> },
}

/// A schematic with nothing on it: what the first schematic verb creates, and the screen a new
/// hierarchical sheet starts with.
fn empty_schematic_section() -> SchematicSection {
    SchematicSection {
        symbols: vec![],
        wires: vec![],
        labels: vec![],
        texts: vec![],
        power_symbols: vec![],
        no_connects: vec![],
        bus_entries: vec![],
        junctions: vec![],
        lines: vec![], extras: Default::default(),
        erc_exclusions: vec![],
        erc_pin_map: None,
        user_fields: Default::default(),
        field_layout: Default::default(),
        imported_from_kicad: false,
        title_block: None,
        sheets: vec![],
        instance_overrides: vec![],
    }
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
    /// The Symbol Editor tab: `design.symbol_library`'s own undo/redo
    /// scope, same independence `FootprintEditor`'s own doc explains.
    SymbolEditor,
}

impl Cmd {
    /// Which editor this command belongs to -- see [`Domain`].
    pub fn domain(&self) -> Domain {
        match self {
            Cmd::Batch { cmds } => cmds.first().map_or(Domain::Pcb, Cmd::domain),
            Cmd::OnSheet { cmd, .. } | Cmd::InGroup { cmd, .. } => cmd.domain(),
            Cmd::ReorganizeSheets => Domain::Schematic,
            Cmd::Embedded(c) => c.domain(),
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
            | Cmd::AddJunction { .. }
            | Cmd::DeleteJunction { .. }
            | Cmd::AddSchLine { .. }
            | Cmd::ImportSchGraphics(_)
            | Cmd::DeleteSchLine { .. }
            | Cmd::AddSheet { .. }
            | Cmd::ImportSheet { .. }
            | Cmd::ReplaceSchematic { .. }
            | Cmd::SwapSchItems { .. }
            | Cmd::PasteSch { .. }
            | Cmd::SchEdit(_)
            | Cmd::SchTable(_)
            | Cmd::SchMove(_)
            | Cmd::AddErcExclusion { .. }
            | Cmd::DeleteErcExclusion { .. }
            | Cmd::ClearErcExclusions
            | Cmd::SetConnectionGrid { .. }
            | Cmd::SetIntersheetRefs { .. }
            | Cmd::SetSimulator { .. }
            | Cmd::AddLabel { .. }
            | Cmd::DeleteLabel { .. }
            | Cmd::AddSchText { .. }
            | Cmd::DeleteSchText { .. }
            | Cmd::AddPowerSymbol { .. }
            | Cmd::DeletePowerSymbol { .. }
            | Cmd::AddSymbol { .. }
            | Cmd::EmbedLibSymbol { .. }
            | Cmd::EditSymbolFields { .. }
            | Cmd::RenameSymbol { .. }
            | Cmd::SetSymbolFields { .. }
            | Cmd::ReplaceText { .. }
            | Cmd::SetErcPinMapCell { .. }
            | Cmd::ResetErcPinMap
            | Cmd::SetErcSeverities { .. }
            | Cmd::SetSchematicPage { .. }
            | Cmd::Annotate { .. }
            | Cmd::SetSymbolAttrs { .. }
            | Cmd::SetSheetPage { .. }
            | Cmd::SetSchItemText { .. }
            | Cmd::SetSymbolLibIds { .. }
            | Cmd::IncrementAnnotations { .. } => Domain::Schematic,
            Cmd::OpenFootprintForEdit { .. }
            | Cmd::NewFootprint { .. }
            | Cmd::DeleteLibraryFootprint { .. }
            | Cmd::PutLibraryFootprint { .. }
            | Cmd::RenameLibraryFootprint { .. }
            | Cmd::RepairFootprint { .. }
            | Cmd::EditFootprintProperties { .. }
            | Cmd::SetFootprintAnchor { .. }
            | Cmd::UpdateFootprintOnBoard { .. }
            | Cmd::AddPad { .. }
            | Cmd::MovePad { .. }
            | Cmd::RotatePad { .. }
            | Cmd::DeletePad { .. }
            | Cmd::EditPad { .. }
            | Cmd::PushPadProperties { .. }
            | Cmd::SetPadNumbers { .. }
            | Cmd::RenumberPads { .. }
            | Cmd::AddFootprintGraphic { .. }
            | Cmd::DeleteFootprintGraphic { .. }
            | Cmd::MoveFootprintGraphic { .. }
            | Cmd::EditFootprintGraphic { .. }
            | Cmd::AddFootprintText { .. }
            | Cmd::EditFootprintText { .. }
            | Cmd::DeleteFootprintText { .. }
            | Cmd::MoveFootprintText { .. }
            | Cmd::CreateFootprintArray { .. }
            | Cmd::ExplodePad { .. }
            | Cmd::RecombinePad { .. } => Domain::FootprintEditor,
            Cmd::OpenSymbolForEdit { .. }
            | Cmd::NewSymbol { .. }
            | Cmd::DeleteLibrarySymbol { .. }
            | Cmd::PutLibrarySymbol { .. }
            | Cmd::RenameLibrarySymbol { .. }
            | Cmd::SetSymbolAnchor { .. }
            | Cmd::EditSymbolProperties { .. }
            | Cmd::UpdateSymbolOnBoard { .. }
            | Cmd::AddSymbolPin { .. }
            | Cmd::MoveSymbolPin { .. }
            | Cmd::DeleteSymbolPin { .. }
            | Cmd::EditSymbolPin { .. }
            | Cmd::PushPinProperty { .. }
            | Cmd::AddSymbolGraphic { .. }
            | Cmd::ImportSymbolGraphics(_)
            | Cmd::DeleteSymbolGraphic { .. }
            | Cmd::MoveSymbolGraphic { .. }
            | Cmd::EditSymbolGraphic { .. }
            | Cmd::EditSymbolText { .. }
            | Cmd::ReplaceSymbolGraphic { .. }
            | Cmd::DeriveSymbol { .. }
            | Cmd::FlattenSymbol { .. }
            | Cmd::UpdateSymbolFields { .. }
            | Cmd::SetSymbolParent { .. }
            | Cmd::EditLibrarySymbolFields { .. } => Domain::SymbolEditor,
            _ => Domain::Pcb,
        }
    }
}

impl Cmd {
    /// Can this schematic command change what is connected? A symbol that only changes place or turns (`M`, `G`, `R`, `X`, `Y`) is layout: it
    /// never rewrites the net list. `design.nets` is the electrical intent -- the one netlist the PCB and every export read -- and the drawing is
    /// its picture, which kicad-cli's ERC judges (a wire left behind, a pin off its wire); a tidy-up must not turn a board's nets into whatever the
    /// drawing happens to show. Every other edit is read from the drawing, before and after (`crates/cli/src/board.rs` `reconcile_schematic`).
    pub fn edits_connectivity(&self) -> bool {
        match self {
            Cmd::Batch { cmds } => cmds.iter().any(Cmd::edits_connectivity),
            Cmd::OnSheet { cmd, .. } | Cmd::InGroup { cmd, .. } => cmd.edits_connectivity(),
            // A severity table and the findings waived are about the report, not the drawing.
            Cmd::SetErcSeverities { .. } | Cmd::AddErcExclusion { .. } | Cmd::DeleteErcExclusion { .. } | Cmd::ClearErcExclusions | Cmd::SetConnectionGrid { .. } | Cmd::SetIntersheetRefs { .. } | Cmd::SetSimulator { .. } => false,
            Cmd::MoveSymbol { .. } | Cmd::DragSymbol { .. } | Cmd::RotateSymbol { .. } | Cmd::MirrorSymbol { .. } | Cmd::MirrorSymbolVertical { .. } => false,
            Cmd::SchMove(c) => c.edits_connectivity(),
            Cmd::SchEdit(c) => c.edits_connectivity(),
            // A table is drawn text and lines: nothing it covers is connected to it.
            Cmd::SchTable(_) => false,
            // A file kept inside the schematic is no part of the drawing.
            Cmd::Embedded(_) => false,
            // A definition kept with the schematic changes no connection.
            Cmd::EmbedLibSymbol { .. } => false,
            // Drawn lines, shapes and text are not wires: nothing they cross is connected to them.
            Cmd::ImportSchGraphics(_) => false,
            _ => true,
        }
    }
}

impl Cmd {
    /// Is this File > Import > Non-KiCad Board / Schematic (`ReplaceBoard`, `ReplaceSchematic`)? An import is judged by nobody: the file is what it is, so the gates
    /// that refuse a move that makes the board worse do not refuse it, and what it replaces is not traced against -- the server reads the new drawing on its own.
    pub fn replaces_board(&self) -> bool {
        matches!(self, Cmd::ReplaceBoard { .. })
    }

    /// Is this File > Import > Specctra Session (`Cmd::Specctra`)? The routing is what the outside router made of the board: the gates that refuse a move that makes the
    /// board worse do not judge it, as they do not judge an import of a board.
    pub fn imports_session(&self) -> bool {
        matches!(self, Cmd::Specctra { .. })
    }

    pub fn replaces_schematic(&self) -> bool {
        matches!(self, Cmd::ReplaceSchematic { .. })
    }

    /// Does this command only add placement rule areas (and the groups round them)? Such a command moves nothing and routes nothing, so the gates that refuse a move that makes the
    /// board worse have no say in it: what they find after it is the board's own, which the first zone of a board with no routing yet starts to count (`routing_connectivity` judges
    /// a board that has a routing section, and a zone is held in it).
    pub fn adds_only_rule_areas(&self) -> bool {
        matches!(self, Cmd::Multichannel(MultichannelCmd::GenerateRuleAreas { .. }))
    }
}

impl Cmd {
    /// Does this command change both editors' halves of the design in one step? A reference rename moves the schematic's symbol and the board's part together
    /// (`Board::rename_part`), so one Ctrl+Z, from either tab, has to take both back: the history keeps such a step as a whole-design step
    /// (`crates/cli/src/board.rs`, the `both` tag), where an edit of one editor is undone from that editor's tab alone (`Cmd::domain`).
    pub fn spans_both_editors(&self) -> bool {
        match self {
            Cmd::Batch { cmds } => cmds.iter().any(Cmd::spans_both_editors),
            Cmd::OnSheet { cmd, .. } | Cmd::InGroup { cmd, .. } => cmd.spans_both_editors(),
            Cmd::RenameSymbol { .. } | Cmd::Annotate { .. } | Cmd::IncrementAnnotations { .. } => true,
            // the board's footprint and the symbol's Footprint field are put right together
            Cmd::ExchangeFootprints { .. } => true,
            // the footprints' references and the symbols' go together
            Cmd::ReannotateBoard { .. } => true,
            // a board that comes in has its own nets: the netlist the schematic's drawing had set goes (`design.nets`)
            Cmd::ReplaceBoard { .. } => true,
            // a swap of nets moves pins between the nets of `design.nets` -- the netlist the schematic's half of the design owns -- and the nets of the board's copper with them
            Cmd::SwapPadNets { .. } | Cmd::SwapGateNets { .. } => true,
            // Replace All renames a reference when it is asked to look at them
            Cmd::ReplaceText { search, .. } => search.replace_references,
            // Field Properties of a Reference field renames the symbol
            Cmd::SchEdit(sch_edit::SchCmd::EditField { id, text: Some(_), .. }) => id.ends_with(":Reference"),
            _ => false,
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
            | Cmd::SetLabelSide { part, .. }
            | Cmd::SetFootprintZoneConnection { part, .. }
            | Cmd::SetPadZoneOverrides { part, .. } => vec![part],
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
            Cmd::PasteItems { .. } | Cmd::PasteClipboard { .. } | Cmd::PasteSpecial { .. } => vec!["paste"],
            Cmd::PlaceFootprint { footprint, .. } => vec![footprint.as_str()],
            Cmd::ImportGraphics(req) => vec![req.layer.as_str()],
            Cmd::ImportSchGraphics(_) => vec!["sch_graphics"],
            Cmd::ImportSymbolGraphics(req) => vec![req.lib_id.as_str()],
            Cmd::ExchangeFootprints { references, .. } => references.iter().map(String::as_str).collect(),
            Cmd::ReannotateBoard { .. } => vec!["reannotate"],
            Cmd::CommitRoute { .. } => vec!["route"],
            Cmd::CommitTuning { pattern, remove_pattern, .. } => remove_pattern.iter().map(String::as_str).chain(pattern.iter().map(|p| p.layer.as_str())).collect(),
            Cmd::Batch { cmds } => cmds.iter().flat_map(Cmd::subjects).collect(),
            Cmd::OnSheet { cmd, .. } | Cmd::InGroup { cmd, .. } => cmd.subjects(),
            Cmd::ReorganizeSheets => vec!["sheets"],
            Cmd::MoveExact { parts, .. } => parts.iter().map(String::as_str).collect(),
            Cmd::MoveItems { ids, .. } | Cmd::RotateItems { ids, .. } | Cmd::FlipItems { ids, .. } | Cmd::MirrorTracks { ids, .. } | Cmd::SetItemNet { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::EditTrack { id, .. } | Cmd::SetZoneName { id, .. } | Cmd::ReplaceShape { id, .. } => vec![id.as_str()],
            Cmd::EditBoardFootprint { part, .. } | Cmd::EditBoardField { part, .. } | Cmd::EditBoardPad { part, .. } => vec![part.as_str()],
            Cmd::SetBoardModels { part, .. } | Cmd::AddBoardModel { part, .. } | Cmd::RemoveBoardModel { part, .. } | Cmd::MoveBoardModel { part, .. } | Cmd::EditBoardModel { part, .. } => vec![part.as_str()],
            Cmd::MigrateBoardModels { replacements } => replacements.keys().map(String::as_str).collect(),
            Cmd::SetTrackWidthPresets { .. } => vec!["track_width_presets"],
            Cmd::SetViaPresets { .. } => vec!["via_presets"],
            Cmd::EditTracksAndVias { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::EditTextAndGraphics { shape_ids, text_ids, dimension_ids, .. } => shape_ids.iter().chain(text_ids.iter()).chain(dimension_ids.iter()).map(String::as_str).collect(),
            Cmd::RemoveUnusedPads { selected, .. } => selected.iter().flatten().map(String::as_str).collect(),
            Cmd::SwapPadNets { ids, .. } | Cmd::SwapGateNets { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::SetTeardropSettings { .. } => vec!["teardrop_settings"],
            Cmd::AddAllTeardrops | Cmd::RemoveAllTeardrops => vec!["teardrops"],
            Cmd::Group { ids } | Cmd::Ungroup { ids } | Cmd::RemoveFromGroup { ids } => ids.iter().map(String::as_str).collect(),
            Cmd::AddToGroup { group_id, ids } => std::iter::once(group_id.as_str()).chain(ids.iter().map(String::as_str)).collect(),
            Cmd::EditGroup { id, member_ids, .. } => std::iter::once(id.as_str()).chain(member_ids.iter().map(String::as_str)).collect(),
            Cmd::LinkDesignBlock { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::PlaceDesignBlock { block, .. } => vec![block.as_str()],
            Cmd::CreateArray { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::AddDimension { .. } => vec!["dimension"],
            Cmd::DeleteDimension { id } | Cmd::MoveDimension { id, .. } | Cmd::EditDimension { id, .. } => vec![id.as_str()],
            Cmd::SetDimensionSettings { .. } => vec!["dimension_settings"],
            Cmd::SetNetClasses { .. } => vec!["net_classes"],
            Cmd::SetConstraints { .. } => vec!["constraints"],
            Cmd::SetMaskPaste { .. } => vec!["mask_paste"],
            Cmd::SetTextGraphicsDefaults { .. } => vec!["text_graphics"],
            Cmd::SetStackup { .. } => vec!["stackup"],
            Cmd::SetRuleSeverities { .. } => vec!["severities"],
            Cmd::SetCustomRules { .. } => vec!["custom_rules"],
            Cmd::EditDrcRule { .. } => vec!["custom_rules"],
            Cmd::SetTextVariables { .. } => vec!["text_variables"],
            Cmd::SetBoardLayers { .. } => vec!["board_layers"],
            Cmd::SetFormatting { .. } => vec!["formatting"],
            Cmd::SetZoneHatchOffsets { .. } => vec!["zone_hatch_offsets"],
            Cmd::SetTuningPatterns { .. } => vec!["tuning_patterns"],
            Cmd::SetTuningProfiles { .. } => vec!["tuning_profiles"],
            Cmd::AddDrcExclusions { exclusions } => exclusions.iter().map(|e| e.check.as_str()).collect(),
            Cmd::DeleteDrcExclusions { exclusions } => exclusions.iter().map(|e| e.check.as_str()).collect(),
            Cmd::ClearDrcExclusions => vec!["drc_exclusions"],
            Cmd::SetErcSeverities { .. } => vec!["erc_severities"],
            Cmd::SwapLayers { .. } => vec!["swap_layers"],
            Cmd::SetLocked { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::SwapChain { parts } => parts.iter().map(String::as_str).collect(),
            Cmd::BooleanShapes { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::ZoneCutout { id, .. } => vec![id.as_str()],
            Cmd::MergeZones { ids } => ids.iter().map(String::as_str).collect(),
            Cmd::SetZonePriority { id, .. } => vec![id.as_str()],
            Cmd::SetAuxOrigin { .. } => vec!["aux_origin"],
            Cmd::SetGridOrigin { .. } => vec!["grid_origin"],
            Cmd::SetBoardPage { .. } | Cmd::SetSchematicPage { .. } => vec!["page"],
            Cmd::RepairBoard => vec!["repair_board"],

            Cmd::MoveSymbol { id, .. }
            | Cmd::DragSymbol { id, .. }
            | Cmd::RotateSymbol { id, .. }
            | Cmd::MirrorSymbol { id, .. }
            | Cmd::MirrorSymbolVertical { id, .. }
            | Cmd::DeleteSymbol { id, .. }
            | Cmd::AddSymbol { id, .. }
            | Cmd::EditSymbolFields { id, .. }
            | Cmd::RenameSymbol { id, .. } => vec![id],
            Cmd::EmbedLibSymbol { symbol } => vec![symbol.lib_id.as_str()],
            Cmd::AddWire { .. } => vec!["wire"],
            Cmd::DeleteWire { id }
            | Cmd::DeleteNoConnect { id }
            | Cmd::DeleteLabel { id }
            | Cmd::DeletePowerSymbol { id }
            | Cmd::DeleteSchText { id }
            | Cmd::DeleteBusEntry { id }
            | Cmd::DeleteJunction { id }
            | Cmd::DeleteSchLine { id } => vec![id],
            Cmd::AddNoConnect { .. } => vec!["no_connect"],
            Cmd::AddBusEntry { .. } => vec!["bus_entry"],
            Cmd::AddJunction { .. } => vec!["junction"],
            Cmd::AddSchLine { .. } => vec!["sch_line"],
            Cmd::AddSheet { name, .. } => vec![name.as_str()],
            Cmd::ImportSheet { source, .. } => vec![source.as_str()],
            Cmd::ReplaceBoard { name, .. } | Cmd::ReplaceSchematic { name, .. } => vec![name.as_str()],
            Cmd::SwapSchItems { a, b } => vec![a, b],
            Cmd::PasteSch { .. } => vec!["paste"],
            Cmd::SchEdit(c) => c.ids(),
            Cmd::SchMove(c) => c.ids(),
            Cmd::PcbItem(c) => c.ids(),
            Cmd::Embedded(c) => c.ids(),
            Cmd::Microwave(c) => c.ids(),
            Cmd::Specctra { name, .. } => vec![name.as_str()],
            Cmd::Multichannel(c) => c.ids(),
            Cmd::Table(c) | Cmd::SchTable(c) => c.ids(),
            Cmd::AddErcExclusion { location, .. } | Cmd::DeleteErcExclusion { location, .. } => vec![location.as_str()],
            Cmd::ClearErcExclusions => vec!["erc_exclusions"],
            Cmd::SetConnectionGrid { .. } => vec!["connection_grid"],
            Cmd::SetIntersheetRefs { .. } => vec!["intersheet_refs"],
            Cmd::SetSimulator { .. } => vec!["simulator"],
            Cmd::AddLabel { net, .. } => vec![net.as_str()],
            Cmd::AddSchText { content, .. } => vec![content.as_str()],
            Cmd::AddPowerSymbol { net, .. } => vec![net.as_str()],
            Cmd::Annotate { .. } => vec!["annotate"],
            Cmd::SetSymbolAttrs { ids, .. } => ids.iter().map(String::as_str).collect(),
            Cmd::SetSheetPage { sheet, .. } => vec![sheet.as_str()],
            Cmd::SetSchItemText { id, .. } => vec![id.as_str()],
            Cmd::SetSymbolLibIds { .. } => vec!["symbol_lib_ids"],
            Cmd::IncrementAnnotations { start, .. } => vec![start.as_str()],
            Cmd::SetSymbolFields { .. } => vec!["symbol_fields"],
            Cmd::ReplaceText { .. } => vec!["replace_text"],
            Cmd::SetErcPinMapCell { .. } | Cmd::ResetErcPinMap => vec!["erc_pin_map"],

            Cmd::PutLibraryFootprint { footprint, .. } => vec![footprint.name.as_str()],
            Cmd::RenameLibraryFootprint { name, .. } | Cmd::RepairFootprint { name } => vec![name.as_str()],
            Cmd::OpenFootprintForEdit { name }
            | Cmd::NewFootprint { name }
            | Cmd::DeleteLibraryFootprint { name }
            | Cmd::EditFootprintProperties { name, .. }
            | Cmd::SetFootprintAnchor { name, .. }
            | Cmd::UpdateFootprintOnBoard { name } => {
                vec![name.as_str()]
            }
            Cmd::AddPad { footprint, .. }
            | Cmd::MovePad { footprint, .. }
            | Cmd::RotatePad { footprint, .. }
            | Cmd::DeletePad { footprint, .. }
            | Cmd::EditPad { footprint, .. }
            | Cmd::PushPadProperties { footprint, .. }
            | Cmd::SetPadNumbers { footprint, .. }
            | Cmd::RenumberPads { footprint, .. }
            | Cmd::AddFootprintGraphic { footprint, .. }
            | Cmd::DeleteFootprintGraphic { footprint, .. }
            | Cmd::MoveFootprintGraphic { footprint, .. }
            | Cmd::EditFootprintGraphic { footprint, .. }
            | Cmd::AddFootprintText { footprint, .. }
            | Cmd::EditFootprintText { footprint, .. }
            | Cmd::DeleteFootprintText { footprint, .. }
            | Cmd::MoveFootprintText { footprint, .. }
            | Cmd::CreateFootprintArray { footprint, .. }
            | Cmd::ExplodePad { footprint, .. }
            | Cmd::RecombinePad { footprint } => vec![footprint.as_str()],

            Cmd::PutLibrarySymbol { symbol, .. } => vec![symbol.lib_id.as_str()],
            Cmd::RenameLibrarySymbol { lib_id, .. } | Cmd::SetSymbolAnchor { lib_id, .. } => vec![lib_id.as_str()],
            Cmd::OpenSymbolForEdit { lib_id } | Cmd::NewSymbol { lib_id } | Cmd::DeleteLibrarySymbol { lib_id } | Cmd::EditSymbolProperties { lib_id, .. } | Cmd::UpdateSymbolOnBoard { lib_id } => vec![lib_id.as_str()],
            Cmd::AddSymbolPin { lib_id, .. }
            | Cmd::MoveSymbolPin { lib_id, .. }
            | Cmd::DeleteSymbolPin { lib_id, .. }
            | Cmd::EditSymbolPin { lib_id, .. }
            | Cmd::PushPinProperty { lib_id, .. }
            | Cmd::AddSymbolGraphic { lib_id, .. }
            | Cmd::DeleteSymbolGraphic { lib_id, .. }
            | Cmd::MoveSymbolGraphic { lib_id, .. }
            | Cmd::EditSymbolGraphic { lib_id, .. }
            | Cmd::EditSymbolText { lib_id, .. }
            | Cmd::ReplaceSymbolGraphic { lib_id, .. }
            | Cmd::FlattenSymbol { lib_id }
            | Cmd::UpdateSymbolFields { lib_id, .. }
            | Cmd::SetSymbolParent { lib_id, .. }
            | Cmd::EditLibrarySymbolFields { lib_id, .. } => vec![lib_id.as_str()],
            Cmd::DeriveSymbol { lib_id, parent, .. } => vec![lib_id.as_str(), parent.as_str()],
        }
    }

    /// Whether this command can leave the board's routing stale. True for
    /// anything that moves, flips or removes a part -- exactly the
    /// commands the CLI's `step` already cleared routing for -- and false
    /// for every copper/drawing command, which by construction only adds,
    /// deletes or restyles the copper/graphics themselves and never moves
    /// a part out from under them.
    pub fn clears_routing(&self) -> bool {
        if let Cmd::Batch { cmds } = self {
            return cmds.iter().any(Cmd::clears_routing);
        }
        if let Cmd::InGroup { cmd, .. } = self {
            return cmd.clears_routing();
        }
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
                | Cmd::SwapChain { .. }
                | Cmd::Rip { .. }
                | Cmd::Flip { .. }
                | Cmd::MoveExact { .. }
                // `arrange: true` can reposition a placed part; `false`
                // never touches one (see `Cmd::CreateArray`'s own doc) but
                // this classification is per-variant, not per-call, so it
                // errs the same safe direction `MoveExact` already does.
                | Cmd::CreateArray { .. }
        )
    }

    /// [`Cmd::clears_routing`], told what the board holds. The edit tools never clear the routing: `EDIT_TOOL::Move`, `Rotate`, `Flip`,
    /// `DeleteItems` and the array tool act on the items they were given and leave the copper that was routed to a footprint's pads where it is
    /// (it becomes unconnected, and the ratsnest says so). So the commands that move, turn, flip or take away *any* kind of item
    /// ([`Cmd::MoveItems`], [`Cmd::RotateItems`], [`Cmd::FlipItems`], [`Cmd::Rip`], [`Cmd::CreateArray`]) answer `false` here, whatever the
    /// footprints they name have routed to them; the older verbs of the CLI and the agents (`move_to`, `rotate`, `flip`, `place_at`, ...) keep their
    /// answer, because a placement made by hand there is followed by a new routing. `board` is the board as it was before the command.
    pub fn clears_routing_in(&self, board: &Board<'_>) -> bool {
        match self {
            Cmd::Batch { cmds } => cmds.iter().any(|c| c.clears_routing_in(board)),
            Cmd::InGroup { cmd, .. } => cmd.clears_routing_in(board),
            Cmd::MoveItems { .. } | Cmd::RotateItems { .. } | Cmd::FlipItems { .. } | Cmd::MirrorTracks { .. } | Cmd::Rip { .. } | Cmd::CreateArray { .. } => false,
            other => other.clears_routing(),
        }
    }
}

/// A board under construction.
pub struct Board<'a> {
    model: &'a ConstraintModel,
    design: Design,
    snap: Nm,
    /// Minimum gap left between courtyards when resolving a `Place`.
    spacing: Nm,
    /// [`unruled_decoupling_pairs`] both ways round, computed once: every
    /// neighbour query reads it.
    decoupling: Arc<BTreeMap<String, BTreeSet<String>>>,
    /// The screen the schematic verbs act on: the `Design::sheet_contents` key of the sheet [`Cmd::OnSheet`] named, or `None` for
    /// the root (`design.schematic`) -- KiCad's "current sheet". Only ever set while one `OnSheet` runs.
    focus: Option<String>,
    /// The intent's own nets, when the caller supplied them (`Board::with_intent_nets`): lets Reorganize give a net its declared
    /// name back. `None` for every board built without them.
    intent_nets: Option<Vec<eda_model::Net>>,
    /// What the net-collision monitor drew for the last Move or Drag this board applied (`sch_net_collision.rs`): the studio's move preview reads it with the sheet.
    net_collision: Option<sch_net_collision::NetCollision>,
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
    pub fn new(design: Design, model: &'a ConstraintModel, snap: Nm, spacing: Nm) -> Self {
        let mut decoupling: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (cap, ic) in unruled_decoupling_pairs(model) {
            decoupling.entry(cap.clone()).or_default().insert(ic.clone());
            decoupling.entry(ic).or_default().insert(cap);
        }
        Board { model, design, snap, spacing, decoupling: Arc::new(decoupling), focus: None, intent_nets: None, net_collision: None }
    }

    /// The net-collision overlay of the last Move or Drag applied (see `sch_net_collision.rs`); `None` when none was.
    pub fn net_collision(&self) -> Option<&sch_net_collision::NetCollision> {
        self.net_collision.as_ref()
    }

    pub fn design(&self) -> &Design {
        &self.design
    }

    /// `BOARD_DESIGN_SETTINGS::GetGridOrigin()`: where the editing grid is anchored, (0, 0) unless the board has set one.
    fn grid_origin(&self) -> Point {
        self.design.drawings.as_ref().and_then(|d| d.grid_origin).unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO })
    }

    /// A point on the placement grid: whole snap steps from the grid origin, not from (0, 0), so a position the editor snapped to the grid it
    /// draws (`origin + n * grid`) is the position the verb stores.
    fn snap_point(&self, x: Nm, y: Nm) -> Point {
        let o = self.grid_origin();
        Point { x: snap(x - o.x, self.snap) + o.x, y: snap(y - o.y, self.snap) + o.y }
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
            focus: self.focus.clone(),
            intent_nets: self.intent_nets.clone(),
            net_collision: None,
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

    /// Apply a command, or refuse it. A group keeps only the members that are still on the board afterwards (`pcb_groups.rs`).
    pub fn apply(&mut self, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        let groups_before = self.group_universe();
        let sch_groups_before = self.sch_group_universe();
        let patterns_before = self.tuning_universe();
        // A board brought in whole (`ReplaceBoard`) comes with its own summary, made with its own model, which this board's model is not.
        let edge_before = if matches!(cmd, Cmd::ReplaceBoard { .. }) { None } else { outline::snapshot(&self.design, self.model) };
        let shared_before = self.shared_screens_before();
        self.apply_cmd(cmd)?;
        self.tidy_shared_screens(shared_before);
        self.tidy_groups(groups_before);
        self.tidy_sch_groups(sch_groups_before);
        self.tidy_tuning_patterns(patterns_before);
        outline::refresh_if_changed(&mut self.design, self.model, edge_before);
        Ok(())
    }

    fn apply_cmd(&mut self, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
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
            Cmd::SetFootprintZoneConnection { part, zone_connection, clearance } => self.set_footprint_zone_connection(part, *zone_connection, *clearance),
            Cmd::SetPadZoneOverrides { part, pad, zone_connection, thermal_gap, thermal_spoke_width, thermal_spoke_angle_mdeg, clearance } => self.set_pad_zone_overrides(
                part,
                pad,
                eda_model::ir::PadZoneOverride { number: pad.clone(), clearance: *clearance, zone_connection: *zone_connection, thermal_gap: *thermal_gap, thermal_spoke_width: *thermal_spoke_width, thermal_spoke_angle_mdeg: *thermal_spoke_angle_mdeg },
            ),

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
            Cmd::SetTeardropSettings { settings } => self.set_teardrop_settings(*settings),
            Cmd::AddAllTeardrops => self.add_all_teardrops(),
            Cmd::RemoveAllTeardrops => self.remove_all_teardrops(),
            Cmd::Group { ids } => self.group_items(ids),
            Cmd::Ungroup { ids } => self.ungroup_items(ids),
            Cmd::AddToGroup { group_id, ids } => self.add_to_group(group_id, ids),
            Cmd::RemoveFromGroup { ids } => self.remove_from_group(ids),
            Cmd::EditGroup { id, name, member_ids, design_block } => self.edit_group(id, name, member_ids, design_block.as_deref()),
            Cmd::LinkDesignBlock { ids, block } => self.link_design_block_items(ids, block),
            Cmd::PlaceDesignBlock { text, at, block, as_group, group_name } => self.place_design_block(text, *at, block, *as_group, group_name),
            Cmd::CreateArray { ids, geometry, arrange, reannotate } => self.create_array(ids, geometry, *arrange, *reannotate),
            Cmd::AddDimension { dimension } => self.add_dimension(dimension.clone()),
            Cmd::DeleteDimension { id } => self.delete_dimension(id),
            Cmd::MoveDimension { id, dx, dy } => self.move_dimension(id, *dx, *dy),
            Cmd::EditDimension { id, dimension } => self.edit_dimension(id, dimension.clone()),
            Cmd::SetDimensionSettings { settings } => self.set_dimension_settings(*settings),
            Cmd::SetNetClasses { settings } => board_setup::set_net_classes(&mut self.design, settings),
            Cmd::SetConstraints { constraints } => board_setup::set_constraints(&mut self.design, constraints),
            Cmd::SetMaskPaste { settings } => board_setup::set_mask_paste(&mut self.design, settings),
            Cmd::SetTextGraphicsDefaults { settings } => board_setup::set_text_graphics(&mut self.design, settings),
            Cmd::SetStackup { settings } => board_setup::set_stackup(&mut self.design, self.model, settings),
            Cmd::SetRuleSeverities { severities } => board_setup::set_severities(&mut self.design, severities),
            Cmd::AddDrcExclusions { exclusions } => review::add_drc_exclusions(&mut self.design, exclusions),
            Cmd::DeleteDrcExclusions { exclusions } => review::delete_drc_exclusions(&mut self.design, exclusions),
            Cmd::ClearDrcExclusions => review::clear_drc_exclusions(&mut self.design),
            Cmd::SetErcSeverities { severities } => review::set_erc_severities(&mut self.design, severities),
            Cmd::SetConnectionGrid { mils } => review::set_connection_grid(&mut self.design, *mils),
            Cmd::SetIntersheetRefs { settings } => review::set_intersheet_refs(&mut self.design, settings),
            Cmd::SetSimulator { ngspice, workbook } => simulator::set_simulator(&mut self.design, ngspice.as_ref(), workbook.as_deref()),
            Cmd::SetCustomRules { text } => board_setup::set_custom_rules(&mut self.design, text),
            Cmd::EditDrcRule { action, entry, name, rule } => rule_editor::edit_drc_rule(&mut self.design, self.model, *action, *entry, name, rule.as_ref()),
            Cmd::SetTextVariables { variables } => board_setup::set_text_variables(&mut self.design, variables),
            Cmd::SetBoardLayers { settings } => board_setup::set_board_layers(&mut self.design, self.model, settings),
            Cmd::SetFormatting { settings } => board_setup::set_formatting(&mut self.design, settings),
            Cmd::SetZoneHatchOffsets { offsets } => board_setup::set_zone_hatch_offsets(&mut self.design, self.model, offsets),
            Cmd::SetTuningPatterns { settings } => board_setup::set_tuning_patterns(&mut self.design, settings),
            Cmd::SetTuningProfiles { settings } => board_setup::set_tuning_profiles(&mut self.design, self.model, settings),
            Cmd::SwapLayers { mapping } => self.swap_layers(mapping),
            Cmd::SetLocked { ids, locked } => self.set_locked(ids, *locked),
            Cmd::SwapChain { parts } => self.swap_chain(parts),
            Cmd::BooleanShapes { operation, ids } => pcb_edit::boolean_shapes(self.drawings_mut(), *operation, ids),
            Cmd::ZoneCutout { id, cutout } => self.zone_cutout(id, cutout),
            Cmd::MergeZones { ids } => board_control::merge_zones(&mut self.design, ids).map(|_| ()),
            Cmd::SetZonePriority { id, to } => board_control::set_zone_priority(&mut self.design, id, *to),
            Cmd::SetAuxOrigin { at } => board_control::set_aux_origin(&mut self.design, *at),
            Cmd::SetGridOrigin { at } => board_control::set_grid_origin(&mut self.design, *at),
            Cmd::SetBoardPage { page, title_block } => page_settings::set_board_page(&mut self.design, page, title_block),
            Cmd::SetSchematicPage { page, title_block } => page_settings::set_schematic_page(&mut self.design, page, title_block),
            Cmd::RepairBoard => {
                let report = board_control::repair_board(&mut self.design, &self.model.nets);
                if report.repaired == 0 {
                    return Err(vec![CheckResult::fail("ops_repair_board", "board", "No board problems found.")]);
                }
                Ok(())
            }

            Cmd::AddZone { net, layer, outline } => self.add_zone(net, layer, outline),
            Cmd::DeleteZone { id } => self.delete_zone(id),
            Cmd::SetZoneOutline { id, outline, holes } => self.set_zone_outline(id, outline, holes.as_deref()),
            Cmd::EditZone {
                id,
                net,
                layer,
                layers,
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
                is_rule_area,
                keepout_tracks,
                keepout_vias,
                keepout_pads,
                keepout_copper_pour,
                keepout_footprints,
                smoothing,
                corner_radius,
                placement,
            } => self.edit_zone(
                id,
                net,
                layer,
                layers.as_deref(),
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
                *is_rule_area,
                *keepout_tracks,
                *keepout_vias,
                *keepout_pads,
                *keepout_copper_pour,
                *keepout_footprints,
                *smoothing,
                *corner_radius,
                placement.as_ref(),
            ),

            Cmd::AddShape { shape } => self.add_shape(shape.clone()),
            Cmd::DeleteShape { id } => self.delete_shape(id),
            Cmd::MoveShape { id, dx, dy } => self.move_shape(id, *dx, *dy),
            Cmd::EditShape { id, layer, stroke_width, filled } => self.edit_shape(id, layer, *stroke_width, *filled),

            Cmd::AddText { text } => self.add_text(text.clone()),
            Cmd::EditText { id, content, angle, layer, size_nm, stroke_width, justify, mirror } => {
                self.edit_text(id, content.clone(), *angle, layer.clone(), *size_nm, *stroke_width, *justify, *mirror)
            }
            Cmd::DeleteText { id } => self.delete_text(id),
            Cmd::MoveText { id, x, y } => self.move_text(id, *x, *y),
            Cmd::RemoveUnusedPads { selected, vias, pads, keep_outside, remove } => self.remove_unused_pads(&unused_pads::UnusedPadLayers { selected: selected.as_deref(), vias: *vias, pads: *pads, keep_outside: *keep_outside, remove: *remove }),
            Cmd::SwapPadNets { ids, connected_pads } => self.swap_pad_nets(ids, *connected_pads),
            Cmd::SwapGateNets { ids, target, connected_pads } => self.swap_gate_nets(ids, target.as_deref(), *connected_pads),
            Cmd::EditTextAndGraphics { shape_ids, text_ids, dimension_ids, layer_defaults, layer, line_width, text_size, text_thickness } => {
                self.edit_text_and_graphics(&pcb_global_edit::GlobalTextEdit { shape_ids, text_ids, dimension_ids, layer_defaults: *layer_defaults, layer: layer.as_deref(), line_width: *line_width, text_size: *text_size, text_thickness: *text_thickness })
            }

            Cmd::Duplicate { ids } => self.duplicate_all(ids),
            Cmd::PasteClipboard { text, at } => self.paste_clipboard(text, *at),
            Cmd::ImportGraphics(req) => self.import_graphics(req),
            Cmd::ImportSchGraphics(req) => self.import_sch_graphics(req),
            Cmd::ImportSymbolGraphics(req) => self.import_symbol_graphics(req),
            Cmd::PasteSpecial { text, at, mode, clear_nets } => self.paste_special(text, *at, *mode, *clear_nets),
            Cmd::PlaceFootprint { footprint, at, reference, value, definition } => self.place_footprint(footprint, *at, reference, value, definition.as_ref()),
            Cmd::ExchangeFootprints { references, footprint, options, library } => self.exchange_footprints(references, footprint.as_deref(), options, library),
            Cmd::ReannotateBoard { options, selection } => self.reannotate_board(options, selection),
            Cmd::PasteItems { tracks, vias, zones, shapes, texts } => self.insert_copies(tracks.clone(), vias.clone(), zones.clone(), shapes.clone(), texts.clone()),
            Cmd::CommitRoute { remove_track_ids, remove_via_ids, tracks, vias, lock_tracks, via_kinds } => self.commit_route(remove_track_ids, remove_via_ids, tracks.clone(), vias.clone(), lock_tracks, via_kinds),
            Cmd::CommitTuning { pattern, remove_pattern, remove_track_ids, tracks, member_indices } => self.commit_tuning(pattern.as_ref(), remove_pattern.as_deref(), remove_track_ids, tracks.clone(), member_indices),
            Cmd::MoveExact { parts, dx, dy, rotate_millideg, pivot } => self.move_exact(parts, *dx, *dy, *rotate_millideg, *pivot),
            Cmd::MoveItems { ids, dx, dy } => self.move_items(ids, *dx, *dy),
            Cmd::RotateItems { ids, pivot, angle_millideg } => self.rotate_items(ids, *pivot, *angle_millideg),
            Cmd::FlipItems { ids, pivot, direction } => self.flip_items(ids, *pivot, *direction),
            Cmd::MirrorTracks { ids, pivot, direction } => self.mirror_tracks(ids, *pivot, *direction),
            Cmd::EditBoardFootprint { part, reference, value, fields, attrs } => self.edit_board_footprint(part, reference.as_ref(), value.as_ref(), fields.as_deref(), attrs.as_ref()),
            Cmd::EditBoardField { part, name, layout, text } => self.edit_board_field(part, name, layout.as_ref(), text.as_deref()),
            Cmd::EditBoardPad { part, edit } => self.edit_board_pad(part, edit),
            Cmd::MigrateBoardModels { replacements } => self.migrate_board_models(replacements),
            Cmd::SetBoardModels { part, models } => self.set_board_models(part, models),
            Cmd::AddBoardModel { part, model, at } => self.add_board_model(part, model, *at),
            Cmd::RemoveBoardModel { part, index } => self.remove_board_model(part, *index),
            Cmd::MoveBoardModel { part, from, to } => self.move_board_model(part, *from, *to),
            Cmd::EditBoardModel { part, index, path, offset, scale, rotate, opacity, show } => {
                self.edit_board_model(part, *index, &eda_model::fp_models::ModelPatch { path: path.clone(), offset: *offset, scale: *scale, rotate: *rotate, opacity: *opacity, show: *show })
            }
            Cmd::EditTrack { id, start, end } => self.edit_track(id, *start, *end),
            Cmd::SetItemNet { ids, net } => self.set_item_net(ids, net),
            Cmd::SetZoneName { id, name } => self.set_zone_name(id, name),
            Cmd::ReplaceShape { id, shape } => self.replace_shape(id, shape.clone()),
            Cmd::Batch { cmds } => {
                let saved = self.design.clone();
                for c in cmds {
                    if let Err(e) = self.apply(c) {
                        self.design = saved;
                        return Err(e);
                    }
                }
                Ok(())
            }

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
            Cmd::AddJunction { at } => self.add_junction(*at),
            Cmd::DeleteJunction { id } => self.delete_junction(id),
            Cmd::AddSchLine { pts, width_nm } => self.add_sch_line(pts.clone(), *width_nm),
            Cmd::DeleteSchLine { id } => self.delete_sch_line(id),
            Cmd::AddSheet { name, file, at, size } => self.add_sheet(name, file, *at, *size),
            Cmd::ImportSheet { source, content, as_sheet, name, file, at, size, keep_annotations, order, options, as_group, group_name, block } => match content {
                Some(content) => self.import_sheet(content, *as_sheet, name, file, *at, *size, *keep_annotations, *order, options, &design_blocks::Placement { source, as_group: *as_group, group_name, block }),
                None => Err(vec![CheckResult::fail("ops_sheet_file_not_read", source, format!("the file '{source}' was not read, so there is nothing to import"))]),
            },
            Cmd::ReplaceSchematic { content, name } => self.replace_schematic(content, name),
            Cmd::ReplaceBoard { text, name, keep_kicad_layer_names, project, rules } => self.replace_board(text, name, *keep_kicad_layer_names, project.as_deref(), rules.as_deref()),
            Cmd::SwapSchItems { a, b } => self.swap_sch_items(a, b),
            Cmd::PasteSch { fragment, dx, dy, mode } => self.paste_sch(fragment, *dx, *dy, *mode).map(|_| ()),
            Cmd::SchEdit(c) => self.apply_sch_edit(c),
            Cmd::SchMove(c) => self.apply_sch_move(c),
            Cmd::PcbItem(c) => self.apply_pcb_item(c),
            Cmd::Embedded(c) => self.apply_embedded(c),
            Cmd::Microwave(c) => self.apply_microwave(c),
            Cmd::Specctra { session, name } => self.import_specctra_session(session, name),
            Cmd::Multichannel(c) => self.apply_multichannel(c),
            Cmd::Table(c) => self.apply_table(c),
            Cmd::SchTable(c) => self.apply_sch_table(c),
            Cmd::OnSheet { sheet, cmd } => self.on_sheet(sheet, cmd),
            Cmd::InGroup { group, cmd } => self.in_group(group, cmd),
            Cmd::ReorganizeSheets => self.reorganize_sheets(),
            Cmd::DeleteNoConnect { id } => self.delete_no_connect(id),
            Cmd::AddErcExclusion { check, location, comment, marker } => self.add_erc_exclusion(check, location, comment, marker),
            Cmd::DeleteErcExclusion { check, location, sheet } => self.delete_erc_exclusion(check, location, sheet.as_deref()),
            Cmd::ClearErcExclusions => self.clear_erc_exclusions(),
            Cmd::AddLabel { net, at, kind } => self.add_label(net, *at, kind.clone()),
            Cmd::DeleteLabel { id } => self.delete_label(id),
            Cmd::AddSchText { content, at, angle_millideg, size_nm } => self.add_sch_text(content, *at, *angle_millideg, *size_nm),
            Cmd::DeleteSchText { id } => self.delete_sch_text(id),
            Cmd::AddPowerSymbol { lib_id, at, rot_millideg, net, pin } => self.add_power_symbol(lib_id, *at, *rot_millideg, net, pin),
            Cmd::DeletePowerSymbol { id } => self.delete_power_symbol(id),
            Cmd::AddSymbol { id, lib_id, at, rot_millideg, value, footprint, unit } => self.add_symbol(id, lib_id, *at, *rot_millideg, value, footprint, *unit),
            Cmd::EmbedLibSymbol { symbol } => self.embed_lib_symbol(symbol),
            Cmd::EditSymbolFields { id, value, footprint, datasheet } => self.edit_symbol_fields(id, value.as_deref(), footprint.as_deref(), datasheet.as_deref()),
            Cmd::RenameSymbol { id, new_id } => self.rename_symbol(id, new_id),
            Cmd::SetSymbolAttrs { ids, dnp, exclude_from_bom, exclude_from_board, exclude_from_sim } => self.set_symbol_attrs(ids, *dnp, *exclude_from_bom, *exclude_from_board, *exclude_from_sim),
            Cmd::SetSheetPage { sheet, page } => self.set_sheet_page(sheet, page),
            Cmd::SetSchItemText { id, text } => self.set_sch_item_text(id, text),
            Cmd::SetSymbolLibIds { changes, update_fields } => self.set_symbol_lib_ids(changes, *update_fields),
            Cmd::IncrementAnnotations { start, increment } => self.increment_annotations(start, *increment),
            Cmd::SetSymbolFields { edits, add_fields, rename_fields, remove_fields } => self.set_symbol_fields(edits, add_fields, rename_fields, remove_fields),
            Cmd::ReplaceText { search, items, all_sheets } => self.replace_text(search, items.as_deref(), *all_sheets),
            Cmd::SetErcPinMapCell { a, b, level } => self.set_erc_pin_map_cell(*a, *b, *level),
            Cmd::ResetErcPinMap => self.reset_erc_pin_map(),
            Cmd::Annotate { reset_existing, order, ids, options } => self.annotate(*reset_existing, *order, ids.as_deref(), options),

            Cmd::OpenFootprintForEdit { name } => self.open_footprint_for_edit(name),
            Cmd::NewFootprint { name } => self.new_footprint(name),
            Cmd::NewSymbol { lib_id } => self.new_symbol(lib_id),
            Cmd::DeleteLibraryFootprint { name } => self.delete_library_footprint(name),
            Cmd::PutLibraryFootprint { footprint, overwrite } => self.put_library_footprint(footprint, *overwrite),
            Cmd::RenameLibraryFootprint { name, new_name, overwrite } => self.rename_library_footprint(name, new_name, *overwrite),
            Cmd::RepairFootprint { name } => self.repair_footprint(name),
            Cmd::EditFootprintProperties { name, description, keywords, attributes, reference_visible, value_visible, model, models3d, zone_connection, reference, value, fields, datasheet, description_field } => {
                // the grid is judged first: a row that is wrong refuses the panel before anything of it is taken
                self.check_library_fields(name, reference.as_ref(), value.as_ref(), fields.as_deref())?;
                self.check_library_info_fields(name, datasheet.as_ref(), description_field.as_ref())?;
                self.edit_footprint_properties(name, description.clone(), keywords.clone(), *attributes, *reference_visible, *value_visible, model.clone(), models3d.as_deref(), *zone_connection)?;
                self.apply_library_fields(name, reference.as_ref(), value.as_ref(), fields.as_deref())?;
                self.apply_library_info_fields(name, datasheet.as_ref(), description_field.as_ref())
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
            Cmd::SetPadNumbers { footprint, numbers } => self.set_pad_numbers(footprint, numbers),
            Cmd::RenumberPads { footprint, start, prefix, step } => self.renumber_pads(footprint, *start, prefix, *step),
            Cmd::ExplodePad { footprint, id } => self.explode_pad(footprint, id),
            Cmd::RecombinePad { footprint } => self.recombine_pad(footprint),

            Cmd::AddFootprintGraphic { footprint, shape } => self.add_footprint_graphic(footprint, shape.clone()),
            Cmd::DeleteFootprintGraphic { footprint, id } => self.delete_footprint_graphic(footprint, id),
            Cmd::MoveFootprintGraphic { footprint, id, dx, dy } => self.move_footprint_graphic(footprint, id, *dx, *dy),
            Cmd::EditFootprintGraphic { footprint, id, layer, stroke_width, filled } => self.edit_footprint_graphic(footprint, id, layer, *stroke_width, *filled),
            Cmd::AddFootprintText { footprint, text } => self.add_footprint_text(footprint, text.clone()),
            Cmd::EditFootprintText { footprint, id, content, angle, layer, size_nm, stroke_width, justify, mirror } => {
                self.edit_footprint_text(footprint, id, content.clone(), *angle, layer.clone(), *size_nm, *stroke_width, *justify, *mirror)
            }
            Cmd::DeleteFootprintText { footprint, id } => self.delete_footprint_text(footprint, id),
            Cmd::MoveFootprintText { footprint, id, x, y } => self.move_footprint_text(footprint, id, *x, *y),
            Cmd::CreateFootprintArray { footprint, ids, geometry, arrange, numbering } => self.create_footprint_array(footprint, ids, geometry, *arrange, numbering.as_ref()),
            Cmd::DeriveSymbol { parent, lib_id, keep_footprint, keep_datasheet, transfer_user_fields, keep_content_user_fields, overwrite } => {
                self.derive_symbol_verb(parent, lib_id, eda_model::sym_fields::DeriveOptions { keep_footprint: *keep_footprint, keep_datasheet: *keep_datasheet, transfer_user_fields: *transfer_user_fields, keep_content_user_fields: *keep_content_user_fields }, *overwrite)
            }
            Cmd::FlattenSymbol { lib_id } => self.flatten_symbol_verb(lib_id),
            Cmd::UpdateSymbolFields { lib_id, fields, remove_extras, reset_empty, reset_text, reset_visibility, reset_effects, reset_positions } => self.update_symbol_fields_verb(
                lib_id,
                eda_model::sym_fields::UpdateFieldsOptions {
                    fields: fields.iter().cloned().collect(),
                    remove_extras: *remove_extras,
                    reset_empty: *reset_empty,
                    reset_text: *reset_text,
                    reset_visibility: *reset_visibility,
                    reset_effects: *reset_effects,
                    reset_positions: *reset_positions,
                },
            ),
            Cmd::SetSymbolParent { lib_id, parent } => self.set_symbol_parent_verb(lib_id, parent),
            Cmd::EditLibrarySymbolFields { lib_id, fields } => self.edit_library_symbol_fields_verb(lib_id, fields),

            Cmd::OpenSymbolForEdit { lib_id } => self.open_symbol_for_edit(lib_id),
            Cmd::DeleteLibrarySymbol { lib_id } => self.delete_library_symbol(lib_id),
            Cmd::PutLibrarySymbol { symbol, overwrite } => self.put_library_symbol(symbol, *overwrite),
            Cmd::RenameLibrarySymbol { lib_id, new_lib_id, overwrite } => self.rename_library_symbol(lib_id, new_lib_id, *overwrite),
            Cmd::SetSymbolAnchor { lib_id, at } => self.set_symbol_anchor(lib_id, *at),
            Cmd::EditSymbolProperties { lib_id, reference_prefix, description, keywords, datasheet, power, in_bom, on_board, pin_numbers_hidden, pin_names_hidden, pin_name_offset_mm, unit_count, has_alternate_body_style, footprint_filters, units_locked, power_local, exclude_from_sim, exclude_from_pos_files } => self.edit_symbol_properties(
                lib_id,
                library_editors::SymbolAttributes { units_locked: *units_locked, power_local: *power_local, exclude_from_sim: *exclude_from_sim, exclude_from_pos_files: *exclude_from_pos_files },
                reference_prefix.clone(),
                description.clone(),
                keywords.clone(),
                datasheet.clone(),
                *power,
                *in_bom,
                *on_board,
                *pin_numbers_hidden,
                *pin_names_hidden,
                *pin_name_offset_mm,
                *unit_count,
                *has_alternate_body_style,
                footprint_filters.clone(),
            ),
            Cmd::UpdateSymbolOnBoard { lib_id } => self.update_symbol_on_board(lib_id),

            Cmd::AddSymbolPin { lib_id, pin } => self.add_symbol_pin(lib_id, pin.clone()),
            Cmd::MoveSymbolPin { lib_id, id, x, y } => self.move_symbol_pin(lib_id, id, *x, *y),
            Cmd::DeleteSymbolPin { lib_id, id } => self.delete_symbol_pin(lib_id, id),
            Cmd::EditSymbolPin { lib_id, id, pin } => self.edit_symbol_pin(lib_id, id, pin.clone()),
            Cmd::PushPinProperty { lib_id, source_pin_id, field, body_style } => self.push_pin_property(lib_id, source_pin_id, *field, *body_style),

            Cmd::AddSymbolGraphic { lib_id, graphic } => self.add_symbol_graphic(lib_id, graphic.clone()),
            Cmd::DeleteSymbolGraphic { lib_id, id } => self.delete_symbol_graphic(lib_id, id),
            Cmd::MoveSymbolGraphic { lib_id, id, dx_mm, dy_mm } => self.move_symbol_graphic(lib_id, id, *dx_mm, *dy_mm),
            Cmd::EditSymbolGraphic { lib_id, id, stroke_mm, fill, unit, body_style } => self.edit_symbol_graphic(lib_id, id, *stroke_mm, *fill, *unit, *body_style),
            Cmd::EditSymbolText { lib_id, id, text, angle_deg, size_mm } => self.edit_symbol_text(lib_id, id, text.clone(), *angle_deg, *size_mm),
            Cmd::ReplaceSymbolGraphic { lib_id, id, graphic } => self.replace_symbol_graphic(lib_id, id, graphic.clone()),
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
    fn courtyard_at(&self, r: &str, fp: &FootprintInstance) -> Result<(Nm, Nm, Nm, Nm), Vec<CheckResult>> {
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
    fn keepout_at(&self, r: &str, fp: &FootprintInstance) -> Result<(Nm, Nm, Nm, Nm), Vec<CheckResult>> {
        let p = self.part(r)?;
        placed_keepout(self.model, &self.placement().outline, p, fp)
            .ok_or_else(|| vec![CheckResult::fail("ops_no_footprint", r, "the part has no resolvable footprint, so it has no keepout to place")])
    }

    /// Keepout of a placed part, for collision tests.
    fn keepout_of(&self, fp: &FootprintInstance) -> Option<(Nm, Nm, Nm, Nm)> {
        let p = self.model.part(&fp.id)?;
        placed_keepout(self.model, &self.placement().outline, p, fp)
    }

    /// Does this trial rectangle hit anything already down?
    fn collides(&self, r: &str, rect: (Nm, Nm, Nm, Nm)) -> bool {
        self.placement().footprints.iter().filter(|f| f.id != r).any(|f| {
            self.keepout_of(f).map(|k| overlaps(rect, k)).unwrap_or(false)
        })
    }

    fn board_bbox(&self) -> (Nm, Nm, Nm, Nm) {
        let o = &self.placement().outline;
        (
            o.iter().map(|p| p.x).min().unwrap_or(Nm::ZERO),
            o.iter().map(|p| p.y).min().unwrap_or(Nm::ZERO),
            o.iter().map(|p| p.x).max().unwrap_or(Nm::ZERO),
            o.iter().map(|p| p.y).max().unwrap_or(Nm::ZERO),
        )
    }

    /// The board as KiCad's autoplacer works from it (`AR_AUTOPLACER`: `m_board->GetBoardPolygonOutlines( m_boardShape, true )`, the free area
    /// `m_topFreeArea = m_boardShape`): every outline with its holes, not the box round the largest one.
    fn board_region(&self) -> eda_drc::outline::BoardOutline {
        eda_drc::outline::board_outline(&self.design, true)
    }

    /// Whether the courtyard box `cr` is on the board: inside an outline of `region` and clear of its holes (the box less the board is
    /// nothing). A board with no outline at all leaves it to the box test.
    fn courtyard_on_board(region: &eda_drc::outline::BoardOutline, cr: (Nm, Nm, Nm, Nm)) -> bool {
        if region.polys.outline_count() == 0 {
            return true;
        }
        let ring = vec![eda_clipper2::Point64::new(cr.0.get(), cr.1.get()), eda_clipper2::Point64::new(cr.2.get(), cr.1.get()), eda_clipper2::Point64::new(cr.2.get(), cr.3.get()), eda_clipper2::Point64::new(cr.0.get(), cr.3.get())];
        let mut outside = eda_shape_poly_set::ShapePolySet::from_outline(ring);
        outside.boolean_subtract(&region.polys);
        outside.is_empty()
    }

    /// Whether `fp`'s pad copper keeps the board's copper-to-edge clearance
    /// from the outline (its bounding box -- the placer's own notion of the
    /// board, as everywhere else in this file).
    ///
    /// A placement constraint, not a check: whether a layout *violates* KiCad's
    /// `copper_edge_clearance` is kicad-cli's call (`eda_gates::kicad`), but
    /// the placer has to know where it may put pads in the first place, and
    /// cannot ask kicad-cli on every candidate. `place_on_edge` has always
    /// kept its connectors out of the band the same way.
    fn pads_clear_of_edge(&self, part: &str, fp: &FootprintInstance, bb: (Nm, Nm, Nm, Nm)) -> bool {
        let clearance = self.model.board.tuning.copper_edge_clearance();
        let Some(p) = self.model.part(part) else { return true };
        let Some(pads) = placed_pads(self.model, p, fp) else { return true };
        pads.iter().all(|pad| {
            pad.center.x - pad.size.0 / 2 - bb.0 >= clearance
                && pad.center.y - pad.size.1 / 2 - bb.1 >= clearance
                && bb.2 - (pad.center.x + pad.size.0 / 2) >= clearance
                && bb.3 - (pad.center.y + pad.size.1 / 2) >= clearance
        })
    }

    /// How many placed footprints have pad copper inside the board-edge
    /// clearance band ([`Self::pads_clear_of_edge`]). What the placer's own
    /// repair moves must not make worse.
    pub(crate) fn parts_in_edge_band(&self) -> usize {
        let bb = self.board_bbox();
        self.placement().footprints.iter().filter(|fp| !self.pads_clear_of_edge(&fp.id, fp, bb)).count()
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
        let probe = FootprintInstance { id: part.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::Above };
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
        // A stand-off that falls half way between two grid points takes the one nearer the anchor (`- 1`): in micrometres the
        // halving above truncated the half away, and the placer's results were made on that rule.
        let base = (ax + dx * (stand_off - Nm::new(1)), ay + dy * (stand_off - Nm::new(1)));

        // Slide perpendicular to the placement direction.
        let (sx, sy) = if dx != 0 { (0, 1) } else { (1, 0) };
        let bb = self.board_bbox();
        let board_area = self.board_region();
        for step in 0..MAX_SLIDE_STEPS {
            for sign in [1i64, -1] {
                if step == 0 && sign < 0 {
                    continue; // centred is one position, not two
                }
                let off = sign * step as i64 * self.snap;
                let at = self.snap_point(base.0 + sx * off, base.1 + sy * off);
                let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
                let Ok(cr) = self.courtyard_at(part, &fp) else { continue };
                if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
                    continue; // a corner on the boundary is not inside it
                }
                if !Self::courtyard_on_board(&board_area, cr) {
                    continue; // in a cutout, or off the piece of the board it is on
                }
                if !self.pads_clear_of_edge(part, &fp, bb) {
                    continue;
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
        let board_area = self.board_region();
        let centre = (
            bb.0 + Nm::new(((bb.2 - bb.0).as_f64() * fx) as i64),
            bb.1 + Nm::new(((bb.3 - bb.1).as_f64() * fy) as i64),
        );
        for ring in 0..MAX_SLIDE_STEPS {
            let r = ring as i64 * self.snap;
            let offsets: Vec<(Nm, Nm)> = if ring == 0 {
                vec![(Nm::ZERO, Nm::ZERO)]
            } else {
                let mut v = Vec::new();
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    v.push((dx * r, dy * r));
                }
                v
            };
            for (ox, oy) in offsets {
                let at = self.snap_point(centre.0 + ox, centre.1 + oy);
                let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
                let Ok(cr) = self.courtyard_at(part, &fp) else { continue };
                if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
                    continue; // a corner on the boundary is not inside it
                }
                if !Self::courtyard_on_board(&board_area, cr) {
                    continue; // in a cutout, or off the piece of the board it is on
                }
                if !self.pads_clear_of_edge(part, &fp, bb) {
                    continue;
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
        let flat = FootprintInstance { id: part.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let c0 = self.courtyard_at(part, &flat)?;
        let wider_than_tall = (c0.2 - c0.0) >= (c0.3 - c0.1);
        let horizontal_edge = matches!(edge, Dir::North | Dir::South);
        let rot: u32 = if horizontal_edge == wider_than_tall { 0 } else { 90_000 };

        let probe = FootprintInstance { id: part.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot, side: Side::Top, label: LabelSide::Above };
        let c = self.courtyard_at(part, &probe)?;
        let (half_w, half_h) = ((c.2 - c.0) / 2, (c.3 - c.1) / 2);

        let along = |lo: Nm, hi: Nm, half: Nm| -> Nm {
            let usable = (hi - half) - (lo + half);
            lo + half + Nm::new((usable.as_f64() * fraction) as i64)
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
        let step = self.snap.max(Nm::new(1));
        let clearance = self.model.board.tuning.copper_edge_clearance();
        let pads = placed_pads(self.model, self.part(part)?, &probe).unwrap_or_default();
        let copper = pads.iter().fold((Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN), |r, p| {
            (r.0.min(p.center.x - p.size.0 / 2), r.1.min(p.center.y - p.size.1 / 2), r.2.max(p.center.x + p.size.0 / 2), r.3.max(p.center.y + p.size.1 / 2))
        });
        // Per side, how much further in than its courtyard the part must
        // sit for that: whole snap steps, plus one for the snapping below,
        // and nothing where the pads clear it anyway. The ends count too:
        // a row put near a corner has its end pad by the other edge.
        let room = |gap: Nm| -> Nm {
            let short = clearance - gap;
            if short <= Nm::ZERO { Nm::ZERO } else { (short + step - Nm::new(1)) / step * step + step }
        };
        let (w, n, e, s) = if pads.is_empty() { (Nm::ZERO, Nm::ZERO, Nm::ZERO, Nm::ZERO) } else { (room(copper.0 - c.0), room(copper.1 - c.1), room(c.2 - copper.2), room(c.3 - copper.3)) };
        let at = match edge {
            Dir::North => Point { x: along(bb.0 + w, bb.2 - e, half_w), y: bb.1 + half_h + step.max(n) },
            Dir::South => Point { x: along(bb.0 + w, bb.2 - e, half_w), y: bb.3 - half_h - step.max(s) },
            Dir::West => Point { x: bb.0 + half_w + step.max(w), y: along(bb.1 + n, bb.3 - s, half_h) },
            Dir::East => Point { x: bb.2 - half_w - step.max(e), y: along(bb.1 + n, bb.3 - s, half_h) },
        };
        let at = self.snap_point(at.x, at.y);
        let fp = FootprintInstance { id: part.into(), at, rot, side: Side::Top, label: LabelSide::Above };
        // A part longer than the edge it was given does not fit on it,
        // and saying so is the whole point. The other two resolvers
        // already bound themselves to the outline; this one did not, so
        // a 40.3mm header went onto a 40mm edge and failed
        // `placement_within_outline` instead of being refused here.
        let cy = self.courtyard_at(part, &fp)?;
        if cy.0 < bb.0 || cy.1 < bb.1 || cy.2 > bb.2 || cy.3 > bb.3 || !Self::courtyard_on_board(&self.board_region(), cy) {
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
    fn place_at(&mut self, part: &str, x: Nm, y: Nm) -> Result<(), Vec<CheckResult>> {
        self.require_unplaced(part)?;
        let bb = self.board_bbox();
        let at = self.snap_point(x, y);
        let fp = FootprintInstance { id: part.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
        let cr = self.courtyard_at(part, &fp)?;
        if cr.0 <= bb.0 || cr.1 <= bb.1 || cr.2 >= bb.2 || cr.3 >= bb.3 {
            return Err(vec![CheckResult::fail(
                "ops_outside_board",
                part,
                "that position puts the part's courtyard on or past the board edge",
            )]);
        }
        if !Self::courtyard_on_board(&self.board_region(), cr) {
            return Err(vec![CheckResult::fail("ops_outside_board", part, "that position puts the part's courtyard over a cutout or off the board")]);
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
        // A footprint that exists only on the board (a copy) has no intent to go back to: deleting it deletes the part too.
        if let Some(dr) = self.design.drawings.as_mut() {
            // The facts a footprint of its own was made with (the microwave tools' pads on the copper layer only, no courtyard) go with it: a footprint that takes the reference later
            // starts without them. A part of the intent keeps what its board file said of it.
            if dr.board_parts.iter().any(|b| b.reference == part) {
                dr.footprint_extras.retain(|e| e.id != part);
            }
            dr.board_parts.retain(|b| b.reference != part);
            // What was edited on the footprint (its fields, attributes, pads, and how zones connect to it) goes with it: a footprint put there again, or a copy
            // that takes the reference, starts from the library's own.
            dr.footprint_edits.retain(|e| e.id != part);
            dr.zone_overrides.retain(|e| e.id != part);
        }
        Ok(())
    }

    fn set_pose(&mut self, part: &str, at: Point, rot: u32) -> Result<(), Vec<CheckResult>> {
        let at = self.snap_point(at.x, at.y);
        self.set_pose_exact(part, at, rot)
    }

    /// `set_pose` without the placement grid: the item edits (`MoveItems`, `RotateItems`, `FlipItems`) put a footprint where the edit says, as
    /// `FOOTPRINT::Move` (`m_pos + aMoveVector`), `FOOTPRINT::Rotate` and `FOOTPRINT::Flip` do (pcbnew/footprint.cpp); the editor's tools snapped the pointer
    /// to the grid already, and Move Exactly and the Properties panel's Position rows take what is typed.
    pub(crate) fn set_pose_exact(&mut self, part: &str, at: Point, rot: u32) -> Result<(), Vec<CheckResult>> {
        let fps = &mut self.design.placement.as_mut().unwrap().footprints;
        let i = fps.iter().position(|f| f.id == part).expect("caller checked the part is placed");
        fps[i].at = at;
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
        let copper = self.model.board.layers.len();
        self.flip_footprint_fields(part, copper);
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

    /// Board Editor Layers: a technical or user layer the board has turned off takes nothing new -- KiCad's tools and dialogs only offer the enabled ones.
    /// (Copper layers are the stackup's, `known_layer`; a name that is no layer of KiCad's is the caller's business, as it always was.)
    fn layer_is_on(&self, layer: &str, what: &str) -> Result<(), Vec<CheckResult>> {
        if eda_model::layers::is_optional_layer(layer) && !self.model.board.layer_setup.enabled.iter().any(|l| l == layer) {
            let shown = self.model.board.layer_setup.display_name(layer);
            return Err(vec![CheckResult::fail("ops_layer_off", what, format!("{shown} is not a layer this board has; turn it on in Board Setup > Board Editor Layers first"))]);
        }
        Ok(())
    }

    /// The board's routing section, creating an empty one if this is the
    /// first hand-added piece of copper. Unlike a part edit, adding copper
    /// must never clear an existing routing section -- see
    /// `Cmd::clears_routing`.
    fn routing_mut(&mut self) -> &mut RoutingSection {
        self.design.routing.get_or_insert_with(|| RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() })
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

    fn add_track(&mut self, net: &str, layer: &str, width: Nm, pts: &[Point]) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(layer)?;
        if width <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_track", net, "track width must be positive")]);
        }
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_track", net, "a track needs at least two points")]);
        }
        let rt = self.routing_mut();
        rt.tracks.push(Track { id: String::new(), net: net.into(), pins: vec![], layer: layer.into(), width, pts: pts.to_vec(), arc_mid_offset: None });
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

    fn set_track_width(&mut self, id: &str, width: Nm) -> Result<(), Vec<CheckResult>> {
        if width <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_track", id, "track width must be positive")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "the board has no routing yet")])?;
        let t = rt.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "no track with this id")])?;
        t.width = width;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_via(&mut self, net: &str, x: Nm, y: Nm, drill: Nm, diameter: Nm, from_layer: &str, to_layer: &str) -> Result<(), Vec<CheckResult>> {
        self.known_net(net)?;
        self.known_layer(from_layer)?;
        self.known_layer(to_layer)?;
        if from_layer == to_layer {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "a via needs two different layers to span")]);
        }
        if drill <= Nm::ZERO || diameter <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "via drill and diameter must be positive")]);
        }
        if drill >= diameter {
            return Err(vec![CheckResult::fail(
                "ops_bad_via",
                net,
                format!("drill {} mm is not smaller than the {} mm pad, so the via has no annular ring", eda_model::units::to_mm(drill), eda_model::units::to_mm(diameter)),
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

    fn move_via(&mut self, id: &str, x: Nm, y: Nm) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "the board has no routing yet")])?;
        let v = rt.vias.iter_mut().find(|v| v.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "no via with this id")])?;
        v.at = Point { x, y };
        Ok(())
    }

    fn edit_via(&mut self, id: &str, diameter: Nm, drill: Nm) -> Result<(), Vec<CheckResult>> {
        if drill <= Nm::ZERO || diameter <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_via", id, "via drill and diameter must be positive")]);
        }
        if drill >= diameter {
            return Err(vec![CheckResult::fail("ops_bad_via", id, format!("drill {} mm is not smaller than the {} mm pad, so the via has no annular ring", eda_model::units::to_mm(drill), eda_model::units::to_mm(diameter)))]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "the board has no routing yet")])?;
        let v = rt.vias.iter_mut().find(|v| v.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_via", id, "no via with this id")])?;
        v.diameter = diameter;
        v.drill = drill;
        Ok(())
    }

    /// Board Setup > Track Widths & Vias' "Track Widths" list -- a plain
    /// whole-list replace (see `Cmd::SetTrackWidthPresets`'s own doc).
    fn set_track_width_presets(&mut self, widths: &[Nm]) -> Result<(), Vec<CheckResult>> {
        if widths.iter().any(|w| *w <= Nm::ZERO) {
            return Err(vec![CheckResult::fail("ops_bad_track", "track_width_presets", "every preset width must be positive")]);
        }
        self.routing_mut().track_width_presets = widths.to_vec();
        Ok(())
    }

    /// Same panel's "Via Sizes" list.
    fn set_via_presets(&mut self, presets: &[ViaPreset]) -> Result<(), Vec<CheckResult>> {
        for p in presets {
            if p.drill <= Nm::ZERO || p.diameter <= Nm::ZERO {
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
        if let Some(SizeSpec::Value { nm: um }) = track_width {
            if *um <= Nm::ZERO {
                return Err(vec![CheckResult::fail("ops_bad_track", "edit_tracks_and_vias", "track width must be positive")]);
            }
        }
        if let Some(ViaSizeSpec::Value { diameter, drill }) = via_size {
            if *drill <= Nm::ZERO || *diameter <= Nm::ZERO {
                return Err(vec![CheckResult::fail("ops_bad_via", "edit_tracks_and_vias", "via drill and diameter must be positive")]);
            }
            if *drill >= *diameter {
                return Err(vec![CheckResult::fail("ops_bad_via", "edit_tracks_and_vias", "drill must be smaller than diameter")]);
            }
        }

        let id_set: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        // "Set to net class / custom rule values" is `PCB_EDIT_FRAME::SetTrackSegmentWidth( aItem, .., true )`: the DRC engine's constraint for the item
        // (`PCB_TRACK::GetWidthConstraint`: `EvalRules( TRACK_WIDTH_CONSTRAINT, this, nullptr, m_layer )`; a via's `VIA_DIAMETER_CONSTRAINT` and
        // `HOLE_SIZE_CONSTRAINT`), its opt else its min; nothing (`<= 0`) leaves the item's own value. So a custom rule wins over the class, and a net of a
        // diff pair takes the class's diff-pair width. Worked out first, on the board as it is.
        let wants_rules = matches!(track_width, Some(SizeSpec::NetClass)) || matches!(via_size, Some(ViaSizeSpec::NetClass));
        let mut by_rule: BTreeMap<String, (Option<Nm>, Option<Nm>)> = BTreeMap::new();
        if wants_rules {
            use eda_drc::constraints::CType;
            let drc_board = eda_drc::board::build(&self.design, self.model);
            let rb = eda_drc::rule_board::RuleBoard::new(&self.design, self.model, &drc_board, eda_drc::constraints::Units::Mm, self.model.board.custom_rules_text.as_deref());
            let value = |ctype: CType, id: &str, layer: &str| -> Option<Nm> {
                let item = rb.item(id, None).ok()?;
                let c = rb.engine().eval_rules(ctype, Some(&item), None, Some(layer), None).value;
                let v = c.opt.or_else(|| c.min.filter(|m| *m > 0))?;
                (v > 0).then_some(Nm::new(v))
            };
            if let Some(rt) = &self.design.routing {
                for t in rt.tracks.iter().filter(|t| id_set.contains(t.id.as_str())) {
                    by_rule.insert(t.id.clone(), (value(CType::TrackWidth, &t.id, &t.layer), None));
                }
                for v in rt.vias.iter().filter(|v| id_set.contains(v.id.as_str())) {
                    by_rule.insert(v.id.clone(), (value(CType::ViaDiameter, &v.id, &v.from_layer), value(CType::HoleSize, &v.id, &v.from_layer)));
                }
            }
        }
        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut().filter(|t| id_set.contains(t.id.as_str())) {
                if let Some(spec) = track_width {
                    t.width = match spec {
                        SizeSpec::NetClass => by_rule.get(&t.id).and_then(|r| r.0).unwrap_or(t.width),
                        SizeSpec::Value { nm } => *nm,
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
                            let (diameter, drill) = by_rule.get(&v.id).copied().unwrap_or((None, None));
                            v.diameter = diameter.unwrap_or(v.diameter);
                            v.drill = drill.unwrap_or(v.drill);
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

    fn add_zone(&mut self, net: &str, layer: &str, outline: &[Point]) -> Result<(), Vec<CheckResult>> {
        // "" is a real, valid choice here -- KiCad's own net code 0 ("No
        // net"), the common case for a rule area/keepout (task item 3) and
        // allowed for an ordinary zone too (matches `known_net`'s sibling
        // checks, which only ever validate a *non-empty* name).
        if !net.is_empty() {
            self.known_net(net)?;
        }
        self.known_zone_layer(layer)?;
        if outline.len() < 3 {
            return Err(vec![CheckResult::fail("ops_bad_zone", net, "a zone outline needs at least three points")]);
        }
        // A zone on no copper layer has no net (`if( !zone->IsOnCopperLayer() ) zone->SetNetCode( NETINFO_LIST::UNCONNECTED )`).
        let net = if eda_model::layers::is_copper_layer_name(layer) { net } else { "" };
        let rt = self.routing_mut();
        rt.zones.push(Zone { id: String::new(), net: net.into(), layer: layer.into(), outline: outline.to_vec(), ..Default::default() });
        rt.assign_missing_ids();
        Ok(())
    }

    /// The layers a zone can be on: a copper layer the board has, or a technical or user layer it has turned on (`LSET::AllNonCuMask` that Board Editor Layers enables).
    fn known_zone_layer(&self, layer: &str) -> Result<(), Vec<CheckResult>> {
        if eda_model::layers::is_copper_layer_name(layer) {
            return self.known_layer(layer);
        }
        if eda_model::layers::is_optional_layer(layer) {
            return self.layer_is_on(layer, "zone");
        }
        Err(vec![CheckResult::fail("ops_unknown_layer", layer, "a zone can be on a copper, technical or user layer")])
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
    fn set_zone_outline(&mut self, id: &str, outline: &[Point], holes: Option<&[Vec<Point>]>) -> Result<(), Vec<CheckResult>> {
        if outline.len() < 3 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "a zone outline needs at least three points")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")])?;
        z.outline = outline.to_vec();
        if let Some(holes) = holes {
            z.holes = holes.iter().filter(|h| h.len() >= 3).cloned().collect();
        }
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
        layers: Option<&[String]>,
        clearance: Nm,
        min_thickness: Nm,
        thermal_gap: Nm,
        thermal_spoke_width: Nm,
        pad_connection: PadConnection,
        priority: u32,
        island_removal_mode: IslandRemovalMode,
        min_island_area: i64,
        fill_mode: FillMode,
        hatch_thickness: Nm,
        hatch_gap: Nm,
        hatch_orientation_mdeg: Millideg,
        hatch_smoothing_level: i32,
        hatch_smoothing_value: f64,
        hatch_hole_min_area: f64,
        hatch_border_algorithm: i32,
        is_rule_area: bool,
        keepout_tracks: bool,
        keepout_vias: bool,
        keepout_pads: bool,
        keepout_copper_pour: bool,
        keepout_footprints: bool,
        smoothing: Option<eda_model::ir::ZoneSmoothing>,
        corner_radius: Option<Nm>,
        placement: Option<&eda_model::multichannel::PlacementArea>,
    ) -> Result<(), Vec<CheckResult>> {
        // "" (no net) is normal for a rule area, and allowed for an
        // ordinary zone too -- see `add_zone`'s matching comment.
        if !net.is_empty() {
            self.known_net(net)?;
        }
        // The zone's layer set: the dialog's list when it sends one, else `layer` (keeping a set that already holds it).
        let wanted: Vec<String> = match layers {
            Some(set) if !set.is_empty() => set.to_vec(),
            _ => vec![layer.to_string()],
        };
        for l in &wanted {
            self.known_zone_layer(l)?;
        }
        let copper = wanted.iter().any(|l| eda_model::layers::is_copper_layer_name(l));
        if !is_rule_area && copper && wanted.iter().any(|l| !eda_model::layers::is_copper_layer_name(l)) {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "a zone is on copper layers or on technical layers, not both (only a rule area can be on both)")]);
        }
        if clearance < Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "clearance cannot be negative")]);
        }
        // A zone on no copper layer may have no minimum width at all (`issue22826`'s mask zones; the non-copper dialog does not check it), and has no thermal
        // spokes to check it against: both are copper pours' (`panel_zone_properties.cpp`'s `AcceptOptions`).
        let pour = copper && !is_rule_area;
        if min_thickness < Nm::ZERO || (min_thickness == Nm::ZERO && copper) {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "minimum width must be positive")]);
        }
        if pour && thermal_spoke_width < min_thickness {
            // panel_zone_properties.cpp's AcceptOptions(): "Thermal spoke
            // width cannot be smaller than the minimum width."
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "thermal spoke width cannot be smaller than the minimum width")]);
        }
        if fill_mode == FillMode::HatchPattern && (hatch_thickness < min_thickness || hatch_gap < min_thickness) {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "hatch thickness and gap must be at least the minimum width")]);
        }
        if corner_radius.is_some_and(|r| r < Nm::ZERO) {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "the outline smoothing size cannot be negative")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")])?;
        if layers.is_some_and(|l| !l.is_empty()) || !z.is_on_layer(layer) {
            z.set_layer_set(&wanted);
        }
        // A zone on no copper layer has no net.
        z.net = if copper || is_rule_area { net.into() } else { String::new() };
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
        z.is_rule_area = is_rule_area;
        z.keepout_tracks = keepout_tracks;
        z.keepout_vias = keepout_vias;
        z.keepout_pads = keepout_pads;
        z.keepout_copper_pour = keepout_copper_pour;
        z.keepout_footprints = keepout_footprints;
        if let Some(s) = smoothing {
            z.smoothing = s;
        }
        if let Some(r) = corner_radius {
            z.corner_radius = r;
        }
        // Only a rule area has a placement (`ZONE::SetPlacementAreaEnabled` is its dialog's); turning the rule area off clears what it had.
        if let Some(pl) = placement {
            z.placement = if is_rule_area { pl.clone() } else { Default::default() };
        } else if !is_rule_area {
            z.placement = Default::default();
        }
        Ok(())
    }

    // ------------------------------------------------------- teardrops

    /// `Cmd::SetTeardropSettings`. Ratios are fractions of a KiCad
    /// `0.0..=1.0` slider (`m_BestLengthRatio`/`m_BestWidthRatio`/
    /// `m_WidthtoSizeFilterRatio`); a value outside that range has no
    /// sensible meaning (a >100% width ratio would ask for a teardrop
    /// wider than its own anchor) and is refused rather than silently
    /// clamped.
    fn set_teardrop_settings(&mut self, settings: TeardropSettings) -> Result<(), Vec<CheckResult>> {
        let in_unit_range = |v: f64| (0.0..=1.0).contains(&v);
        if !in_unit_range(settings.best_length_ratio) || !in_unit_range(settings.best_width_ratio) || !in_unit_range(settings.width_to_size_filter_ratio) {
            return Err(vec![CheckResult::fail("ops_bad_teardrop_settings", "teardrop_settings", "length/width/filter ratios must be between 0.0 and 1.0")]);
        }
        self.routing_mut().teardrop_settings = settings;
        Ok(())
    }

    /// `Cmd::AddAllTeardrops`: regenerate the board's whole teardrop set.
    /// A no-op (not a refusal) when teardrops aren't enabled or nothing
    /// qualifies -- same as KiCad's own "Update Teardrops" finding nothing
    /// to do.
    fn add_all_teardrops(&mut self) -> Result<(), Vec<CheckResult>> {
        let settings = self.design.routing.as_ref().map(|r| r.teardrop_settings).unwrap_or_default();
        let fresh = eda_connectivity::generate_teardrops(&self.design, self.model, &settings);
        let rt = self.routing_mut();
        rt.zones.retain(|z| !z.teardrop);
        rt.zones.extend(fresh);
        rt.assign_missing_ids();
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::RemoveAllTeardrops`.
    fn remove_all_teardrops(&mut self) -> Result<(), Vec<CheckResult>> {
        if let Some(rt) = self.design.routing.as_mut() {
            rt.zones.retain(|z| !z.teardrop);
        }
        Ok(())
    }

    // The group verbs (Group, Ungroup, AddToGroup, RemoveFromGroup, EditGroup) are in pcb_groups.rs.

    // ----------------------------------------------------- dimensions

    fn add_dimension(&mut self, mut dimension: Dimension) -> Result<(), Vec<CheckResult>> {
        if dimension.layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_dimension", "dimension", "a dimension needs a layer")]);
        }
        if dimension.start == dimension.end {
            return Err(vec![CheckResult::fail("ops_bad_dimension", "dimension", "a dimension needs two distinct feature points")]);
        }
        self.layer_is_on(&dimension.layer, "dimension")?;
        dimension.id = String::new();
        let dr = self.drawings_mut();
        dr.dimensions.push(dimension);
        dr.assign_missing_ids();
        Ok(())
    }

    fn delete_dimension(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.drawings_mut();
        let before = dr.dimensions.len();
        dr.dimensions.retain(|d| d.id != id);
        if dr.dimensions.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_dimension", id, "no dimension with this id")]);
        }
        Ok(())
    }

    fn find_dimension_mut<'b>(dr: &'b mut DrawingsSection, id: &str) -> Result<&'b mut Dimension, Vec<CheckResult>> {
        dr.dimensions.iter_mut().find(|d| d.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_dimension", id, "no dimension with this id")])
    }

    fn move_dimension(&mut self, id: &str, dx: Nm, dy: Nm) -> Result<(), Vec<CheckResult>> {
        let dr = self.drawings_mut();
        let dim = Self::find_dimension_mut(dr, id)?;
        eda_connectivity::dimension::translate_dimension(dim, dx, dy);
        Ok(())
    }

    fn edit_dimension(&mut self, id: &str, mut dimension: Dimension) -> Result<(), Vec<CheckResult>> {
        if dimension.layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_dimension", id, "a dimension needs a layer")]);
        }
        if dimension.start == dimension.end {
            return Err(vec![CheckResult::fail("ops_bad_dimension", id, "a dimension needs two distinct feature points")]);
        }
        let dr = self.drawings_mut();
        let slot = Self::find_dimension_mut(dr, id)?;
        dimension.id = slot.id.clone();
        *slot = dimension;
        Ok(())
    }

    fn set_dimension_settings(&mut self, settings: DimensionSettings) -> Result<(), Vec<CheckResult>> {
        self.drawings_mut().dimension_settings = settings;
        Ok(())
    }

    /// `Cmd::SwapChain` -- see that variant's own doc.
    fn swap_chain(&mut self, parts: &[String]) -> Result<(), Vec<CheckResult>> {
        if parts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_swap_chain", "swap", "swapping needs at least two parts")]);
        }
        let mut seen = BTreeSet::new();
        for p in parts {
            if !seen.insert(p.as_str()) {
                return Err(vec![CheckResult::fail("ops_swap_self", p, "a part cannot be swapped with itself")]);
            }
        }
        let poses: Vec<FootprintInstance> = parts.iter().map(|p| self.require_placed(p)).collect::<Result<_, _>>()?;
        let n = parts.len();
        for (i, part) in parts.iter().enumerate() {
            let src = &poses[(i + 1) % n];
            self.set_pose(part, src.at, src.rot)?;
            let fps = &mut self.design.placement.as_mut().unwrap().footprints;
            let k = fps.iter().position(|f| f.id == *part).expect("caller checked the part is placed");
            fps[k].side = src.side;
        }
        Ok(())
    }

    /// `Cmd::SetLocked` -- see that variant's own doc.
    fn set_locked(&mut self, ids: &[String], locked: bool) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_lock", "set_locked", "no items given")]);
        }
        let known = self.lockable_ids();
        for id in ids {
            if !known.contains(id) {
                return Err(vec![CheckResult::fail("ops_unknown_item", id, "no placed part, track, via, zone, shape, text, dimension or group with this id")]);
            }
        }
        let dr = self.drawings_mut();
        let mut set: BTreeSet<String> = dr.locked_ids.iter().filter(|i| known.contains(*i)).cloned().collect();
        for id in ids {
            if locked {
                set.insert(id.clone());
            } else {
                set.remove(id);
            }
        }
        dr.locked_ids = set.into_iter().collect();
        Ok(())
    }

    /// Every id `Cmd::SetLocked` accepts: placed part references plus every
    /// track/via/zone/shape/text id currently on the board.
    fn lockable_ids(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self.placement().footprints.iter().map(|f| f.id.clone()).collect();
        if let Some(rt) = &self.design.routing {
            out.extend(rt.tracks.iter().map(|t| t.id.clone()));
            out.extend(rt.vias.iter().map(|v| v.id.clone()));
            out.extend(rt.zones.iter().map(|z| z.id.clone()));
        }
        if let Some(dr) = &self.design.drawings {
            out.extend(dr.shapes.iter().map(|s| s.id().to_string()));
            out.extend(dr.texts.iter().map(|t| t.id.clone()));
            // Dimensions and groups are lockable in KiCad too (`(dimension .. (locked yes))`, `(group .. (locked yes))`),
            // and the `.kicad_pcb` writer and importer carry both.
            out.extend(dr.dimensions.iter().map(|d| d.id.clone()));
            out.extend(dr.groups.iter().map(|g| g.id.clone()));
            out.extend(dr.barcodes.iter().map(|b| b.id.clone()));
            // A point has no lock in the file (`parsePCB_POINT` takes none), so it is not lockable.
            out.extend(dr.board_items.lockable_ids().map(String::from));
            // A table is locked as a whole (`(table .. (locked yes))`); its cells are not items of their own to lock.
            out.extend(dr.tables.iter().map(|t| t.id.clone()));
        }
        out
    }

    /// `Cmd::ZoneCutout` -- see that variant's own doc.
    fn zone_cutout(&mut self, id: &str, cutout: &[Point]) -> Result<(), Vec<CheckResult>> {
        use eda_clipper2::Point64;
        use eda_shape_poly_set::ShapePolySet;
        if cutout.len() < 3 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "a zone cutout needs at least three points")]);
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "the board has no routing yet")])?;
        let zi = rt.zones.iter().position(|z| z.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_zone", id, "no zone with this id")])?;
        let chain = |pts: &[Point]| pts.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect::<Vec<_>>();
        // `SHAPE_POLY_SET originalOutline( *aZone.Outline() )`: the outline with its holes.
        let mut remaining = ShapePolySet::from_outline(chain(&rt.zones[zi].outline));
        for hole in rt.zones[zi].holes.iter().filter(|h| h.len() >= 3) {
            remaining.add_hole(chain(hole), Some(0));
        }
        let before_area = remaining.area();
        remaining.boolean_subtract(&ShapePolySet::from_outline(chain(cutout)));
        if (remaining.area() - before_area).abs() < 0.5 {
            return Err(vec![CheckResult::fail("ops_bad_zone", id, "the cutout does not overlap the zone")]);
        }
        // `ZONE_CREATE_HELPER::performZoneCutout`: "a zone can have only one main outline, so create as many zones as originalOutline contains main outlines",
        // each with its own holes (`newZoneOutline->AddHole( originalOutline.CHole( outline, hole ) )`).
        let template = rt.zones.remove(zi);
        let ring_of = |r: &Vec<Point64>| r.iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect::<Vec<Point>>();
        for poly in &remaining.polys {
            let ring = &poly[0];
            if ring.len() < 3 {
                continue;
            }
            let mut z = template.clone();
            z.id = String::new();
            z.uuid.clear();
            z.outline = ring_of(ring);
            z.holes = poly[1..].iter().filter(|h| h.len() >= 3).map(ring_of).collect();
            rt.zones.push(z);
        }
        rt.assign_missing_ids();
        Ok(())
    }

    /// `Cmd::SwapLayers`. See that variant's own doc for exactly which
    /// kinds this touches and why vias/footprints are excluded.
    fn swap_layers(&mut self, mapping: &[(String, String)]) -> Result<(), Vec<CheckResult>> {
        for (_, dest) in mapping {
            self.known_layer(dest)?;
        }
        let map: std::collections::HashMap<&str, &str> = mapping.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();

        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut() {
                if let Some(dest) = map.get(t.layer.as_str()) {
                    t.layer = (*dest).to_string();
                }
            }
            for z in rt.zones.iter_mut() {
                if z.layer_set().into_iter().any(|l| map.contains_key(l)) {
                    z.map_layers(|l| map.get(l).map_or_else(|| l.to_string(), |d| (*d).to_string()));
                }
            }
            // `if( via->GetViaType() == VIATYPE::THROUGH ) continue;` --
            // a blind/buried via (one not spanning both outer layers) gets
            // its own layer pair remapped via `SetLayerPair`.
            let layers = &self.model.board.layers;
            let (first, last) = (layers.first().map(String::as_str), layers.last().map(String::as_str));
            for v in rt.vias.iter_mut() {
                let through = (Some(v.from_layer.as_str()) == first && Some(v.to_layer.as_str()) == last) || (Some(v.from_layer.as_str()) == last && Some(v.to_layer.as_str()) == first);
                if through {
                    continue;
                }
                if let Some(dest) = map.get(v.from_layer.as_str()) {
                    v.from_layer = (*dest).to_string();
                }
                if let Some(dest) = map.get(v.to_layer.as_str()) {
                    v.to_layer = (*dest).to_string();
                }
            }
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            for s in dr.shapes.iter_mut() {
                if let Some(dest) = map.get(s.layer()) {
                    s.set_layer((*dest).to_string());
                }
            }
            for t in dr.texts.iter_mut() {
                if let Some(dest) = map.get(t.layer.as_str()) {
                    t.layer = (*dest).to_string();
                }
            }
            for d in dr.dimensions.iter_mut() {
                if let Some(dest) = map.get(d.layer.as_str()) {
                    d.layer = (*dest).to_string();
                }
            }
            dr.board_items.swap_layers(&|l| map.get(l).map(|d| (*d).to_string()));
            // `Drawings()` holds the tables and the barcodes too (`swapBoardItem` over every drawing).
            for t in dr.tables.iter_mut() {
                if let Some(dest) = map.get(t.layer.as_str()) {
                    t.layer = (*dest).to_string();
                }
            }
            for b in dr.barcodes.iter_mut() {
                if let Some(dest) = map.get(b.layer.as_str()) {
                    b.layer = (*dest).to_string();
                }
            }
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
        self.layer_is_on(shape.layer(), "shape")?;
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
        dr.board_items.forget_shape(id);
        Ok(())
    }

    fn move_shape(&mut self, id: &str, dx: Nm, dy: Nm) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "the board has no drawings yet")])?;
        let s = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])?;
        s.translate(dx, dy);
        Ok(())
    }

    fn edit_shape(&mut self, id: &str, layer: &str, stroke_width: Nm, filled: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "a shape needs a layer")]);
        }
        if stroke_width <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "line width must be positive")]);
        }
        self.layer_is_on(layer, id)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "the board has no drawings yet")])?;
        let s = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])?;
        s.set_layer(layer.into());
        s.set_stroke_width(stroke_width);
        s.set_filled(filled);
        // A solid fill takes the place of a hatch; "not filled" leaves one as it is (the fill choice is `SetShapeStyle`'s).
        if filled {
            dr.board_items.set_hatch(id, None);
        }
        Ok(())
    }

    fn add_text(&mut self, mut text: Text) -> Result<(), Vec<CheckResult>> {
        if text.layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", "text", "text needs a layer")]);
        }
        if text.size_nm <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_text", "text", "text size must be positive")]);
        }
        self.layer_is_on(&text.layer, "text")?;
        text.id = String::new();
        let dr = self.drawings_mut();
        dr.texts.push(text);
        dr.assign_missing_ids();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_text(&mut self, id: &str, content: String, angle: Millideg, layer: String, size_nm: Nm, stroke_width: Nm, justify: TextJustify, mirror: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text needs a layer")]);
        }
        if size_nm <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text size must be positive")]);
        }
        self.layer_is_on(&layer, id)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let t = dr.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        // A new size is a square one again (the glyph width an imported text kept is the width of its old size), and a stroke width given is not the automatic one.
        let resized = t.size_nm != size_nm;
        let stroked = t.stroke_width != stroke_width;
        t.content = content;
        t.angle = angle;
        t.layer = layer;
        t.size_nm = size_nm;
        t.stroke_width = stroke_width;
        t.justify = justify;
        t.mirror = mirror;
        if let Some(x) = dr.text_extras.iter_mut().find(|x| x.id == id) {
            if resized {
                x.width_nm = None;
            }
            if stroked {
                x.auto_thickness = false;
            }
        }
        Ok(())
    }

    fn delete_text(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let before = dr.texts.len();
        dr.texts.retain(|t| t.id != id);
        if dr.texts.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")]);
        }
        dr.text_extras.retain(|x| x.id != id);
        Ok(())
    }

    fn move_text(&mut self, id: &str, x: Nm, y: Nm) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "the board has no drawings yet")])?;
        let t = dr.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.at = Point { x, y };
        Ok(())
    }

    // ------------------------------------------------- duplicate / paste

    /// Shared by `PasteItems` (copies that travelled with the command) and `CreateArray`: blank every
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
    #[allow(clippy::too_many_arguments)] // the fields of `Cmd::CommitRoute`, one argument each, as the other verbs take theirs
    fn commit_route(&mut self, remove_track_ids: &[String], remove_via_ids: &[String], mut tracks: Vec<Track>, mut vias: Vec<Via>, lock_tracks: &[usize], via_kinds: &[(usize, eda_model::via_extra::ViaKind)]) -> Result<(), Vec<CheckResult>> {
        for t in &tracks {
            self.known_net(&t.net)?;
            self.known_layer(&t.layer)?;
            if t.width <= Nm::ZERO || t.pts.len() < 2 {
                return Err(vec![CheckResult::fail("ops_bad_track", &t.net, "a committed route track needs a positive width and at least two points")]);
            }
        }
        for v in &vias {
            self.known_net(&v.net)?;
            self.known_layer(&v.from_layer)?;
            self.known_layer(&v.to_layer)?;
            if v.drill <= Nm::ZERO || v.diameter <= Nm::ZERO || v.drill >= v.diameter {
                return Err(vec![CheckResult::fail("ops_bad_via", &v.net, "a committed route via needs a drill smaller than its diameter, both positive")]);
            }
        }
        if !remove_track_ids.is_empty() || !remove_via_ids.is_empty() {
            if let Some(rt) = self.design.routing.as_mut() {
                rt.tracks.retain(|t| !remove_track_ids.iter().any(|id| id == &t.id));
                rt.vias.retain(|v| !remove_via_ids.iter().any(|id| id == &v.id));
            }
            // a removed via's extras go with it (a displaced via comes back under a new id, its type in `via_kinds`)
            if let Some(dr) = self.design.drawings.as_mut() {
                dr.via_extras.retain(|e| !remove_via_ids.iter().any(|id| id == &e.id));
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
            let first_new = rt.tracks.len();
            let first_new_via = rt.vias.len();
            rt.tracks.append(&mut tracks);
            rt.vias.append(&mut vias);
            rt.assign_missing_ids();
            // the new tracks that are locked (the ids are the ones just assigned: the new tracks are the last ones, before the routing is sorted)
            let locked_now: Vec<String> = lock_tracks.iter().filter_map(|&i| rt.tracks.get(first_new + i).map(|t| t.id.clone())).collect();
            // and the new vias that are not through vias, with their type
            let kinds_now: Vec<(String, eda_model::via_extra::ViaKind)> = via_kinds.iter().filter(|(_, k)| *k != eda_model::via_extra::ViaKind::Through).filter_map(|&(i, k)| rt.vias.get(first_new_via + i).map(|v| (v.id.clone(), k))).collect();
            self.sort_routing();
            if !kinds_now.is_empty() {
                let dr = self.drawings_mut();
                for (id, kind) in kinds_now {
                    match dr.via_extras.iter_mut().find(|e| e.id == id) {
                        Some(e) => e.kind = kind,
                        None => dr.via_extras.push(eda_model::via_extra::ViaExtra { id, kind, tokens: vec![] }),
                    }
                }
            }
            if !locked_now.is_empty() {
                let dr = self.drawings_mut();
                let mut set: BTreeSet<String> = dr.locked_ids.iter().cloned().collect();
                set.extend(locked_now);
                dr.locked_ids = set.into_iter().collect();
            }
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
    fn move_exact(&mut self, parts: &[String], dx: Nm, dy: Nm, rotate_millideg: i64, pivot: Option<Point>) -> Result<(), Vec<CheckResult>> {
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

    /// The screen the verbs act on: the root's, or the sheet [`Cmd::OnSheet`] focused.
    fn schematic(&self) -> Result<&SchematicSection, Vec<CheckResult>> {
        let found = match &self.focus {
            None => self.design.schematic.as_ref(),
            Some(file) => self.design.sheet_contents.as_ref().and_then(|c| c.get(file)),
        };
        found.ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])
    }

    fn schematic_mut(&mut self) -> Result<&mut SchematicSection, Vec<CheckResult>> {
        let found = match &self.focus {
            None => self.design.schematic.as_mut(),
            Some(file) => self.design.sheet_contents.as_mut().and_then(|c| c.get_mut(file)),
        };
        found.ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])
    }

    /// Creates an empty schematic section on first use -- `AddSymbol`/
    /// `AddWire`/`AddLabel`/`AddNoConnect` are the only verbs allowed to
    /// start one from nothing (a brand-new design, or an intent whose own
    /// `derive_schematic` never ran); everything else targets an id that
    /// can only already exist inside a section that is already there.
    fn schematic_mut_or_create(&mut self) -> &mut SchematicSection {
        match self.focus.clone() {
            None => self.design.schematic.get_or_insert_with(empty_schematic_section),
            Some(file) => self.design.sheet_contents.get_or_insert_with(BTreeMap::new).entry(file).or_insert_with(empty_schematic_section),
        }
    }

    /// The screen (`sheet_contents` key) a `/`-joined path of sheet-instance ids names, walking down from the focused screen (the
    /// root when none is). `Ok(None)` for the empty path, which is the focused screen itself.
    fn screen_at(&self, path: &str) -> Result<Option<String>, Vec<CheckResult>> {
        let mut file: Option<String> = self.focus.clone();
        for id in path.split('/').filter(|s| !s.is_empty()) {
            let here = match &file {
                None => self.design.schematic.as_ref(),
                Some(f) => self.design.sheet_contents.as_ref().and_then(|c| c.get(f)),
            };
            let Some(sheet) = here.and_then(|s| s.sheets.iter().find(|s| s.id == id)) else {
                return Err(vec![CheckResult::fail("ops_unknown_sheet", id, "no sheet with this id on the path")]);
            };
            if !self.design.sheet_contents.as_ref().is_some_and(|c| c.contains_key(&sheet.file)) {
                return Err(vec![CheckResult::fail("ops_sheet_not_read", &sheet.file, format!("the content of '{}' was never read, so it cannot be edited", sheet.file))]);
            }
            file = Some(sheet.file.clone());
        }
        Ok(file)
    }

    /// [`Cmd::OnSheet`]: run one command with the schematic verbs pointed at another sheet. All or nothing, like a `Batch`.
    fn on_sheet(&mut self, sheet: &str, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        // Only a schematic edit has a sheet to act on. The studio addresses everything it sends from the Schematic tab to the sheet in
        // view, so a command of another editor just runs, and a path that has gone stale does not stop it.
        if cmd.domain() != Domain::Schematic {
            return self.apply(cmd);
        }
        let target = self.screen_at(sheet)?;
        let saved_design = self.design.clone();
        let saved_focus = std::mem::replace(&mut self.focus, target);
        let r = self.apply(cmd);
        self.focus = saved_focus;
        if r.is_err() {
            self.design = saved_design;
        }
        r
    }

    /// Every placed symbol of every screen but the one the verbs act on: (reference, unit). A reference is one part for the whole design
    /// (the model, the PCB and the net list know it by that name), so what is placed on another sheet counts when a symbol is added,
    /// renamed or numbered here.
    fn symbols_elsewhere(&self) -> Vec<(&str, u32)> {
        let mut out: Vec<(&str, u32)> = Vec::new();
        if self.focus.is_some() {
            out.extend(self.design.schematic.iter().flat_map(|s| s.symbols.iter().map(|x| (x.id.as_str(), x.unit))));
        }
        for (file, screen) in self.design.sheet_contents.iter().flatten() {
            if self.focus.as_ref() != Some(file) {
                out.extend(screen.symbols.iter().map(|x| (x.id.as_str(), x.unit)));
            }
        }
        out
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

    fn move_symbol(&mut self, id: &str, x: Nm, y: Nm, unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        self.schematic_mut()?.symbols[i].at = Point { x, y };
        Ok(())
    }

    /// `G`: move a symbol and drag along the endpoint of every wire the
    /// caller found attached to one of its pins before the drag started
    /// (`(wire index, point index)` pairs into `wires`/`wires[i].pts`) --
    /// see `Cmd::DragSymbol`'s own doc for why resolving "attached" is the
    /// caller's job, not this method's.
    fn drag_symbol(&mut self, id: &str, x: Nm, y: Nm, attached: &[(usize, usize)], unit: Option<u32>) -> Result<(), Vec<CheckResult>> {
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
        let sch = self.schematic_mut()?;
        let removed = sch.symbols.remove(i);
        sch_fields::drop_symbol_keys(sch, &removed.id, removed.unit);
        // Last unit gone: its user fields (`SchematicSection::user_fields`) go with it.
        if !sch.symbols.iter().any(|s| s.id == id) {
            sch.user_fields.remove(id);
        }
        Ok(())
    }

    /// The wire tool's commit, `SCH_LINE_WIRE_BUS_TOOL::finishSegments` (eeschema/tools/sch_line_wire_bus_tool.cpp): the wire goes on the sheet, and
    /// each of its ends and each connection point of the sheet that lies on it gets the junction `IsExplicitJunctionNeeded` asks for (`AddJunction`,
    /// which breaks a wire passing through the dot). Not here: `simplifyWireList` (the studio's tool drops the null segments before it sends the
    /// points), the bus unfold (its own command), `SCHEMATIC::CleanUp`'s merging of collinear wires and `TrimWire` across a two-pin symbol.
    fn add_wire(&mut self, pts: Vec<Point>, bus: bool) -> Result<(), Vec<CheckResult>> {
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_wire", "wire", "a wire needs at least two points")]);
        }
        let model = self.model;
        let sch = self.schematic_mut_or_create();
        sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus });
        let new_wire = sch.wires.len() - 1;
        let mut scene = sch_scene::Scene::new(sch, model);
        let segs: Vec<sch_scene::Item> = (0..scene.segs.len()).filter(|&i| scene.segs[i].from.is_some_and(|(w, _)| w == new_wire)).map(sch_scene::Item::Seg).collect();
        let before = scene.sch.junctions.len();
        scene.add_junctions_if_needed(&segs);
        if scene.sch.junctions.len() != before {
            *sch = scene.finish();
            sch.assign_missing_ids();
        }
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

    /// `J`: an explicit junction. A second one at the same point is refused (a click on an existing junction
    /// does nothing in `SingleClickPlace`), so the undo stack never records a step that changed nothing.
    fn add_junction(&mut self, at: Point) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut_or_create();
        if sch.junctions.iter().any(|j| j.at == at) {
            return Err(vec![CheckResult::fail("ops_junction_exists", "junction", "there is already a junction at this point")]);
        }
        sch.junctions.push(Junction { id: String::new(), at });
        sch.junctions.sort_by_key(|j| j.at);
        sch.assign_missing_ids();
        Ok(())
    }

    fn delete_junction(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.junctions.len();
        sch.junctions.retain(|j| j.id != id);
        if sch.junctions.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_junction", id, "no junction with this id")]);
        }
        Ok(())
    }

    /// `I`: a graphic line on the notes layer.
    fn add_sch_line(&mut self, mut pts: Vec<Point>, width_nm: Nm) -> Result<(), Vec<CheckResult>> {
        // A zero-length segment is dropped (`SCH_LINE_WIRE_BUS_TOOL::finishSegments` deletes null lines): a double-click lands its point twice.
        pts.dedup();
        if pts.len() < 2 {
            return Err(vec![CheckResult::fail("ops_bad_line", "line", "a line needs at least two points")]);
        }
        if width_nm < Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_line", "line", "a line width cannot be negative")]);
        }
        let sch = self.schematic_mut_or_create();
        sch.lines.push(SchLine { id: String::new(), pts, width_nm });
        sch.assign_missing_ids();
        Ok(())
    }

    fn delete_sch_line(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.lines.len();
        sch.lines.retain(|l| l.id != id);
        if sch.lines.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_line", id, "no graphic line with this id")]);
        }
        Ok(())
    }

    /// `S`: a hierarchical sheet symbol. See `Cmd::AddSheet`.
    fn add_sheet(&mut self, name: &str, file: &str, at: Point, size: (Nm, Nm)) -> Result<(), Vec<CheckResult>> {
        let name = name.trim();
        if name.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_sheet", "sheet", "a sheet needs a name")]);
        }
        let file = file.trim();
        if file.is_empty() || file.contains('/') || file.contains('\\') {
            return Err(vec![CheckResult::fail("ops_bad_sheet", file, "a sheet file is a bare file name (no folders)")]);
        }
        let file = if file.ends_with(".kicad_sch") { file.to_string() } else { format!("{file}.kicad_sch") };
        if size.0 <= Nm::ZERO || size.1 <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_sheet", name, "a sheet needs a positive size")]);
        }
        let sch = self.schematic_mut_or_create();
        if sch.sheets.iter().any(|s| s.name == name) {
            return Err(vec![CheckResult::fail("ops_sheet_name_taken", name, format!("a sheet named '{name}' already exists on this sheet"))]);
        }
        sch.sheets.push(SheetInstance { id: String::new(), name: name.to_string(), file: file.clone(), at, size, pins: vec![], page: String::new() });
        sch.assign_missing_ids();
        // a file nothing uses yet gets its own empty screen; one that is already there is shared
        self.design.sheet_contents.get_or_insert_with(BTreeMap::new).entry(file).or_insert_with(empty_schematic_section);
        Ok(())
    }

    /// Alt+S: see `Cmd::SwapSchItems`.
    fn swap_sch_items(&mut self, a: &str, b: &str) -> Result<(), Vec<CheckResult>> {
        #[derive(Clone, Copy)]
        enum Slot {
            Symbol(usize),
            Power(usize),
            Label(usize),
            Text(usize),
            /// A pin of a placed sheet: (sheet, pin). `SCH_EDIT_TOOL::Swap`: pins swap only with pins of the same sheet.
            SheetPin(usize, usize),
        }
        if a == b {
            return Err(vec![CheckResult::fail("ops_bad_swap", a, "a swap needs two different items")]);
        }
        let sch = self.schematic_mut()?;
        let find = |id: &str| -> Result<Slot, Vec<CheckResult>> {
            let symbols: Vec<usize> = sch.symbols.iter().enumerate().filter(|(_, s)| s.id == id).map(|(i, _)| i).collect();
            match symbols.len() {
                1 => return Ok(Slot::Symbol(symbols[0])),
                0 => {}
                n => return Err(vec![CheckResult::fail("ops_ambiguous_symbol", id, format!("reference '{id}' has {n} placed units -- a swap needs one"))]),
            }
            if let Some(i) = sch.power_symbols.iter().position(|p| p.id == id) {
                return Ok(Slot::Power(i));
            }
            if let Some(i) = sch.labels.iter().position(|l| l.id == id) {
                return Ok(Slot::Label(i));
            }
            if let Some(i) = sch.texts.iter().position(|t| t.id == id) {
                return Ok(Slot::Text(i));
            }
            for (si, s) in sch.sheets.iter().enumerate() {
                if let Some(pi) = s.pins.iter().position(|p| p.id == id) {
                    return Ok(Slot::SheetPin(si, pi));
                }
            }
            Err(vec![CheckResult::fail("ops_unknown_item", id, "no symbol, power symbol, label, text or sheet pin with this id to swap")])
        };
        let (sa, sb) = (find(a)?, find(b)?);
        // "Sheet pins are special, we need to make sure if we have any sheet pins, that we only have sheet pins, and that they have the same parent"
        match (sa, sb) {
            (Slot::SheetPin(x, _), Slot::SheetPin(y, _)) if x != y => return Err(vec![CheckResult::fail("ops_bad_swap", a, "sheet pins swap only with pins of the same sheet")]),
            (Slot::SheetPin(..), other) | (other, Slot::SheetPin(..)) if !matches!(other, Slot::SheetPin(..)) => return Err(vec![CheckResult::fail("ops_bad_swap", a, "a sheet pin swaps only with another pin of its sheet")]),
            _ => {}
        }
        let at_of = |s: Slot| match s {
            Slot::Symbol(i) => sch.symbols[i].at,
            Slot::Power(i) => sch.power_symbols[i].at,
            Slot::Label(i) => sch.labels[i].at,
            Slot::Text(i) => sch.texts[i].at,
            Slot::SheetPin(si, pi) => sch.sheets[si].pins[pi].at,
        };
        let (pa, pb) = (at_of(sa), at_of(sb));
        // (a pin's side is where it sits on the border, so trading places trades sides, as `SCH_EDIT_TOOL::Swap` does by hand)
        let mut set_at = |s: Slot, p: Point| match s {
            Slot::Symbol(i) => sch.symbols[i].at = p,
            Slot::Power(i) => sch.power_symbols[i].at = p,
            Slot::Label(i) => sch.labels[i].at = p,
            Slot::Text(i) => sch.texts[i].at = p,
            Slot::SheetPin(si, pi) => sch.sheets[si].pins[pi].at = p,
        };
        set_at(sa, pb);
        set_at(sb, pa);
        // "Only swap orientations when both symbols are the same library symbol" -- a resistor and an LED have different default orientations
        if let (Slot::Symbol(i), Slot::Symbol(j)) = (sa, sb) {
            if sch.symbols[i].lib_id == sch.symbols[j].lib_id {
                let (rot, mir, mir_y) = (sch.symbols[i].rot, sch.symbols[i].mirrored, sch.symbols[i].mirror_y);
                sch.symbols[i].rot = sch.symbols[j].rot;
                sch.symbols[i].mirrored = sch.symbols[j].mirrored;
                sch.symbols[i].mirror_y = sch.symbols[j].mirror_y;
                sch.symbols[j].rot = rot;
                sch.symbols[j].mirrored = mir;
                sch.symbols[j].mirror_y = mir_y;
            }
        }
        Ok(())
    }

    /// `dialog_erc.cpp`'s "Exclude this violation" -- see `Cmd::AddErcExclusion`'s own doc.
    fn add_erc_exclusion(&mut self, check: &str, location: &str, comment: &str, marker: &Option<eda_model::ir::ErcMarker>) -> Result<(), Vec<CheckResult>> {
        if location.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_erc_exclusion", check, "this finding has no location to key an exclusion on")]);
        }
        // The list belongs to the schematic as a whole, the root's: a sheet the editor is on does not keep one of its own.
        let sch = self.design.schematic.get_or_insert_with(empty_schematic_section);
        // one per placement of a shared screen: its own sheet in its marker (`eda_kicad::erc_exclusions::same_sheet`)
        match sch.erc_exclusions.iter_mut().find(|e| e.check == check && e.location == location && eda_kicad::erc_exclusions::same_sheet(e.marker.as_ref(), marker.as_ref())) {
            // Already waived: the comment is the new one (an empty one clears it), the marker the new one when there is one.
            Some(slot) => {
                slot.comment = comment.into();
                if marker.is_some() {
                    slot.marker = marker.clone();
                }
            }
            None => sch.erc_exclusions.push(ErcExclusion { check: check.into(), location: location.into(), comment: comment.into(), marker: marker.clone() }),
        }
        sch.erc_exclusions.sort();
        Ok(())
    }

    fn delete_erc_exclusion(&mut self, check: &str, location: &str, sheet: Option<&[String]>) -> Result<(), Vec<CheckResult>> {
        let sch = self.design.schematic.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])?;
        let before = sch.erc_exclusions.len();
        let of_sheet = |e: &ErcExclusion| match (sheet, &e.marker) {
            (Some(want), Some(m)) => m.sheet == want,
            _ => true,
        };
        sch.erc_exclusions.retain(|e| !(e.check == check && e.location == location && of_sheet(e)));
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

    /// `DIALOG_ERC::OnDeleteAllClick` with the exclusions: see `Cmd::ClearErcExclusions`.
    fn clear_erc_exclusions(&mut self) -> Result<(), Vec<CheckResult>> {
        let sch = self.design.schematic.as_mut().ok_or_else(|| vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")])?;
        if sch.erc_exclusions.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_erc_exclusion", "erc_exclusions", "no finding is excluded")]);
        }
        sch.erc_exclusions.clear();
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
    fn add_sch_text(&mut self, content: &str, at: Point, angle: Millideg, size_nm: Nm) -> Result<(), Vec<CheckResult>> {
        self.schematic_mut_or_create().texts.push(SchematicText { id: String::new(), content: content.into(), at, angle, size_nm });
        Ok(())
    }

    fn delete_sch_text(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        let before = sch.texts.len();
        sch.texts.retain(|t| t.id != id);
        if sch.texts.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_sch_text", id, "no text with this id")]);
        }
        sch.extras.text_justify.remove(id);
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
        if self.symbols_elsewhere().iter().any(|(r, u)| *r == id && *u == unit) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", id, format!("reference '{id}' already has unit {unit} on another sheet"))]);
        }
        let sch = self.schematic_mut_or_create();
        if sch.symbols.iter().any(|s| s.id == id && s.unit == unit) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", id, format!("reference '{id}' already has unit {unit} on the sheet"))]);
        }
        sch.symbols.push(SymbolInstance { id: id.into(), at, rot, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: value.into(), footprint: footprint.into(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false });
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

    /// Symbol Fields Table Apply: see `Cmd::SetSymbolFields`'s own doc.
    fn set_symbol_fields(&mut self, edits: &[fields_table::FieldEdit], add: &[String], rename: &[fields_table::FieldRename], remove: &[String]) -> Result<(), Vec<CheckResult>> {
        if edits.is_empty() && add.is_empty() && rename.is_empty() && remove.is_empty() {
            return Err(vec![CheckResult::fail("ops_nothing_to_apply", "symbol_fields", "no field changes to apply")]);
        }
        // The table covers the whole design (`fields_table::project_view`): each edit goes to the sheet that has its symbol, and a field column
        // added, renamed or removed is a column of every sheet. All or nothing.
        if self.design.schematic.is_none() {
            return Err(vec![CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")]);
        }
        let known: BTreeSet<String> = self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter().map(|x| x.id.clone())).collect();
        if let Some(e) = edits.iter().find(|e| !known.contains(&e.id)) {
            return Err(vec![CheckResult::fail("ops_bad_fields", "symbol_fields", format!("no symbol with reference '{}'", e.id))]);
        }
        let saved = (self.design.schematic.clone(), self.design.sheet_contents.clone());
        let apply = |sch: &mut SchematicSection| -> Result<(), String> {
            let here: BTreeSet<&str> = sch.symbols.iter().map(|s| s.id.as_str()).collect();
            let mine: Vec<fields_table::FieldEdit> = edits.iter().filter(|e| here.contains(e.id.as_str())).cloned().collect();
            fields_table::apply_field_changes(sch, &fields_table::FieldChanges { edits: mine, add_fields: add.to_vec(), rename_fields: rename.to_vec(), remove_fields: remove.to_vec() })
        };
        let mut result: Result<(), String> = Ok(());
        for sch in self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut())) {
            result = result.and_then(|()| apply(sch));
        }
        if let Err(m) = result {
            (self.design.schematic, self.design.sheet_contents) = saved;
            return Err(vec![CheckResult::fail("ops_bad_fields", "symbol_fields", m)]);
        }
        Ok(())
    }

    /// Find and Replace: see `Cmd::ReplaceText`'s own doc. Replacements
    /// are computed against a fresh walk of the matches
    /// (`SCH_FIND_REPLACE_TOOL::ReplaceAll`'s `nextMatch` loop), written
    /// back per item kind, and Reference replacements are applied last
    /// through `rename_symbol` so wires/power symbols/no-connects follow.
    fn replace_text(&mut self, search: &search::SearchData, items: Option<&[String]>, all_sheets: bool) -> Result<(), Vec<CheckResult>> {
        if search.find.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_search", "replace_text", "nothing to search for")]);
        }
        let replaced = if all_sheets {
            // `ReplaceAll` over `Schematic().Hierarchy()`: every screen once (a screen two sheets share has one set of items)
            let mut files: Vec<Option<String>> = vec![None];
            files.extend(self.design.sheet_contents.iter().flat_map(|c| c.keys().cloned()).map(Some));
            let saved_focus = self.focus.clone();
            let mut total = 0usize;
            let mut outcome: Result<(), Vec<CheckResult>> = Ok(());
            for file in files {
                self.focus = file;
                match self.replace_text_here(search, None) {
                    Ok(n) => total += n,
                    Err(e) => {
                        outcome = Err(e);
                        break;
                    }
                }
            }
            self.focus = saved_focus;
            outcome?;
            total
        } else {
            self.replace_text_here(search, items)?
        };
        if replaced == 0 {
            return Err(vec![CheckResult::fail("ops_nothing_to_replace", "replace_text", "nothing to replace")]);
        }
        Ok(())
    }

    /// [`Self::replace_text`] on the screen the verbs act on: how many items it replaced in.
    fn replace_text_here(&mut self, search: &search::SearchData, items: Option<&[String]>) -> Result<usize, Vec<CheckResult>> {
        let mut data = search.clone();
        data.search_and_replace = true;
        let model = self.model;
        let hits = search::find_items(self.schematic()?, model, &data);
        let hits: Vec<search::Hit> = hits.into_iter().filter(|h| items.is_none_or(|keys| keys.iter().any(|k| *k == h.key()))).collect();

        let mut renames: Vec<(String, String)> = Vec::new();
        let mut replaced = 0usize;
        {
            let sch = self.schematic_mut()?;
            for h in &hits {
                match h.kind {
                    search::ItemKind::Field if h.name == "Reference" => {
                        if let Some(new) = search::replace_text(&h.id, &data) {
                            renames.push((h.id.clone(), new));
                            replaced += 1;
                        }
                    }
                    search::ItemKind::Field => {
                        let current = match h.name.as_str() {
                            "Value" => sch.symbols.iter().find(|s| s.id == h.id).map(|s| if s.value.is_empty() { model.part(&h.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() }),
                            "Footprint" => sch.symbols.iter().find(|s| s.id == h.id).map(|s| s.footprint.clone()),
                            "Datasheet" => sch.symbols.iter().find(|s| s.id == h.id).map(|s| s.datasheet.clone()),
                            other => sch.user_fields.get(&h.id).and_then(|m| m.get(other)).cloned(),
                        };
                        let Some(new) = current.and_then(|c| search::replace_text(&c, &data)) else { continue };
                        match h.name.as_str() {
                            "Value" => sch.symbols.iter_mut().filter(|s| s.id == h.id).for_each(|s| s.value = new.clone()),
                            "Footprint" => sch.symbols.iter_mut().filter(|s| s.id == h.id).for_each(|s| s.footprint = new.clone()),
                            "Datasheet" => sch.symbols.iter_mut().filter(|s| s.id == h.id).for_each(|s| s.datasheet = new.clone()),
                            other => {
                                sch.user_fields.entry(h.id.clone()).or_default().insert(other.to_string(), new);
                            }
                        }
                        replaced += 1;
                    }
                    search::ItemKind::Label => {
                        if let Some(l) = sch.labels.iter_mut().find(|l| l.id == h.id) {
                            if let Some(new) = search::replace_text(&l.net, &data) {
                                if new.is_empty() {
                                    return Err(vec![CheckResult::fail("ops_bad_replace", &h.id, "replacing would leave a net label empty")]);
                                }
                                l.net = new;
                                replaced += 1;
                            }
                        }
                    }
                    search::ItemKind::Text => {
                        if let Some(t) = sch.texts.iter_mut().find(|t| t.id == h.id) {
                            if let Some(new) = search::replace_text(&t.content, &data) {
                                t.content = new;
                                replaced += 1;
                            }
                        }
                    }
                    search::ItemKind::Pin => {} // SCH_PIN::Replace is a no-op in the schematic
                }
            }
        }
        for (old, new) in renames {
            self.rename_symbol(&old, &new)?;
        }
        Ok(replaced)
    }

    /// `panel_setup_pinmap.cpp::changeErrorLevel`: see
    /// `Cmd::SetErcPinMapCell`'s own doc.
    fn set_erc_pin_map_cell(&mut self, a: usize, b: usize, level: u8) -> Result<(), Vec<CheckResult>> {
        if a >= 12 || b >= 12 || level > 2 {
            return Err(vec![CheckResult::fail("ops_bad_pin_map", "erc_pin_map", "pin map cell out of range (types 0..12, level 0..=2)")]);
        }
        let sch = self.schematic_mut()?;
        let mut matrix = sch.erc_pin_map.take().filter(|m| m.matrix.len() == 12 && m.matrix.iter().all(|r| r.len() == 12)).map(|m| m.matrix).unwrap_or_else(eda_model::ir::ErcPinMap::default_matrix);
        matrix[a][b] = level;
        matrix[b][a] = level;
        sch.erc_pin_map = Some(eda_model::ir::ErcPinMap { matrix });
        Ok(())
    }

    /// `ERC_SETTINGS::ResetPinMap`.
    fn reset_erc_pin_map(&mut self) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if sch.erc_pin_map.take().is_none() {
            return Err(vec![CheckResult::fail("ops_pin_map_default", "erc_pin_map", "the pin map is already the default")]);
        }
        Ok(())
    }

    /// `U`: see `Cmd::RenameSymbol`'s own doc. Renames *every* placed unit
    /// sharing `id` together, in one step -- a multi-unit reference's units
    /// are the same physical part, so they always carry the same reference
    /// text; renaming only one of them would desync it from its siblings.
    /// The board's part goes with the symbol ([`Board::rename_part`]).
    fn rename_symbol(&mut self, id: &str, new_id: &str) -> Result<(), Vec<CheckResult>> {
        // another footprint of the board with that reference (a copy, a footprint placed from a library, a part with no symbol on this sheet) would
        // be two parts under one key
        if new_id != id && self.board_part_without_symbol(new_id) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", new_id, "a footprint with this reference is already on the board")]);
        }
        if self.rename_symbol_in_schematic(id, new_id)? {
            self.rename_part(&BTreeMap::from([(id.to_string(), new_id.to_string())]));
        }
        Ok(())
    }

    /// Is `reference` a footprint of the board that no symbol of the schematic is for (a copy, one placed from a library, one whose symbol was deleted)? A
    /// part renamed to it would be two footprints under one key.
    pub(crate) fn board_part_without_symbol(&self, reference: &str) -> bool {
        let on_board = self.design.placement.iter().any(|p| p.footprints.iter().any(|f| f.id == reference)) || self.design.drawings.iter().any(|d| d.board_parts.iter().any(|b| b.reference == reference));
        on_board && self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values())).all(|s| s.symbols.iter().all(|x| x.id != reference))
    }

    /// The schematic's half of [`Self::rename_symbol`]: the symbol and everything keyed by its reference on the sheet. `Ok(false)` when `new_id` is `id`.
    fn rename_symbol_in_schematic(&mut self, id: &str, new_id: &str) -> Result<bool, Vec<CheckResult>> {
        if new_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", id, "a symbol needs a reference designator")]);
        }
        self.find_symbol(id)?;
        if new_id != id && self.schematic()?.symbols.iter().any(|s| s.id == new_id) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", new_id, "a symbol with this reference is already on the sheet")]);
        }
        if new_id != id && self.symbols_elsewhere().iter().any(|(r, _)| *r == new_id) {
            return Err(vec![CheckResult::fail("ops_duplicate_symbol", new_id, "a symbol with this reference is already on another sheet")]);
        }
        if new_id == id {
            return Ok(false); // renaming to the same id is a no-op, not an error
        }
        // a generated symbol is drawn from the part it is named for: it keeps its drawing under a name of its own
        self.freeze_generated_symbols(&BTreeSet::from([id.to_string()]));
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
        if let Some(fields) = sch.user_fields.remove(id) {
            sch.user_fields.insert(new_id.to_string(), fields);
        }
        // where its fields are, and whether Autoplace Fields put them there, follow the new reference
        sch_fields::rename_symbol_keys(sch, id, new_id);
        Ok(true)
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

    /// `PCB_BASE_FRAME::CreateNewFootprint`: a fresh, empty footprint with the
    /// default attribute `FP_SMD`; the caller already made the name unique.
    fn new_footprint(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        if name.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_footprint", "footprint", "a footprint needs a name")]);
        }
        if self.design.footprint_library.as_ref().and_then(|l| l.by_name(name)).is_some() || self.resolve_named_footprint(name).is_some() {
            return Err(vec![CheckResult::fail("ops_footprint_exists", name, "a footprint with this name already exists; pick another name")]);
        }
        let mut lib_fp = LibraryFootprint::new_empty(name);
        lib_fp.attributes.smd = true;
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
        lib.embedded.remove(name);
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
        models3d: Option<&[eda_model::footprint::Model3d]>,
        zone_connection: Option<PadConnection>,
    ) -> Result<(), Vec<CheckResult>> {
        // The 3D Models tab's list is judged before anything of the panel is taken (`panel_fp_properties_3d_model.cpp`), and refused with the row it is wrong at.
        let list = models3d.map(eda_model::fp_models::checked_models).transpose().map_err(|p| vec![CheckResult::fail(p.check, name, p.reason)])?;
        let fp = self.library_footprint_mut(name)?;
        fp.description = description;
        fp.keywords = keywords;
        fp.attributes = attributes;
        fp.reference_visible = reference_visible;
        fp.value_visible = value_visible;
        match list {
            // the whole list; `model` is its first path
            Some(list) => {
                fp.model = list.first().map(|m| m.path.clone());
                fp.models3d = list;
            }
            // no list sent: the footprint keeps its own, unless the one path was changed -- then that path is its one model, as it was before the list existed
            None => {
                if model != fp.models3d.first().map(|m| m.path.clone()).or_else(|| fp.model.clone()) {
                    fp.models3d.clear();
                    fp.model = model;
                }
            }
        }
        fp.zone_connection = zone_connection;
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
        // `FOOTPRINT_EDIT_FRAME::SaveFootprint`: a pad being edited as shapes is recombined before the footprint is saved.
        if self.library_footprint_mut(name)?.pad_edit.is_some() {
            self.recombine_pad(name)?;
        }
        self.library_footprint_mut(name)?.published = true;
        Ok(())
    }

    /// `pad.size` must be positive and, for a through-hole/non-plated
    /// kind, the drill must actually leave an annular ring -- reuses
    /// `Footprint::validate`'s own rules (the same ones a placed board
    /// footprint is checked against) on a one-pad probe footprint, rather
    /// than re-deriving a second copy of those rules here.
    fn validate_pad(pad: &LibraryPad) -> Result<(), Vec<CheckResult>> {
        if pad.size.0 <= Nm::ZERO || pad.size.1 <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_pad", &pad.number, "pad size must be positive")]);
        }
        let probe = Footprint { name: "probe".into(), pads: vec![pad.to_engine_pad()], courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
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

    fn move_pad(&mut self, footprint: &str, id: &str, x: Nm, y: Nm) -> Result<(), Vec<CheckResult>> {
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
        // `PAD_TOOL::Reset`: a deleted pad leaves pad edit mode.
        if fp.pad_edit.as_ref().is_some_and(|m| m.pad == id) {
            fp.pad_edit = None;
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
            // `pad->ImportSettingsFrom( aSrcPad )` -- see `library_editors::import_pad_settings`.
            library_editors::import_pad_settings(p, &src);
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

    fn move_footprint_graphic(&mut self, footprint: &str, id: &str, dx: Nm, dy: Nm) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        Self::find_footprint_graphic_mut(fp, id)?.translate(dx, dy);
        Ok(())
    }

    fn edit_footprint_graphic(&mut self, footprint: &str, id: &str, layer: &str, stroke_width: Nm, filled: bool) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "a shape needs a layer")]);
        }
        if stroke_width <= Nm::ZERO {
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
        if text.size_nm <= Nm::ZERO {
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
        size_nm: Nm,
        stroke_width: Nm,
        justify: TextJustify,
        mirror: bool,
    ) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text needs a layer")]);
        }
        if size_nm <= Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text size must be positive")]);
        }
        let fp = self.library_footprint_mut(footprint)?;
        let t = fp.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.content = content;
        t.angle = angle;
        t.layer = layer;
        t.size_nm = size_nm;
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

    fn move_footprint_text(&mut self, footprint: &str, id: &str, x: Nm, y: Nm) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(footprint)?;
        let t = fp.texts.iter_mut().find(|t| t.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_text", id, "no text with this id")])?;
        t.at = Point { x, y };
        Ok(())
    }

    // ----------------------------------------------------- symbol editor
    //
    // The Symbol Editor tab's own `Domain::SymbolEditor` verbs -- see
    // `Cmd`'s own "symbol editor" section for what each means; these are
    // its `Board::apply` targets, mirroring the footprint editor's own
    // shape immediately above (validate first, mutate
    // `self.design.symbol_library` second, never the reverse).

    /// `design.symbol_library`, creating an empty one on first use -- same
    /// pattern as `footprint_library_mut`.
    fn symbol_library_mut(&mut self) -> &mut SymbolLibrarySection {
        self.design.symbol_library.get_or_insert_with(SymbolLibrarySection::default)
    }

    fn library_symbol_mut(&mut self, lib_id: &str) -> Result<&mut LibrarySymbol, Vec<CheckResult>> {
        self.design
            .symbol_library
            .as_mut()
            .and_then(|l| l.by_lib_id_mut(lib_id))
            .ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "this symbol has not been opened in the Symbol Editor yet")])
    }

    /// [`library_symbol_mut`](Self::library_symbol_mut) for an edit of the symbol's drawing -- a pin, a graphic, the anchor: refused for a derived symbol, whose graphics and pins
    /// are its parent's (`SYMBOL_EDIT_FRAME::IsSymbolEditable`: "Symbol %s is a derived symbol. Symbol graphics will not be editable.").
    pub(crate) fn drawing_symbol_mut(&mut self, lib_id: &str) -> Result<&mut LibrarySymbol, Vec<CheckResult>> {
        let derived_from = self.design.symbol_library.as_ref().filter(|l| l.is_derived(lib_id)).and_then(|l| l.by_lib_id(lib_id)).and_then(|s| s.extends.clone());
        if let Some(parent) = derived_from {
            return Err(vec![CheckResult::fail("ops_derived_symbol", lib_id, format!("this symbol is derived from {parent}: its graphics and pins are the parent's -- edit them in the parent, or flatten the symbol first"))]);
        }
        self.library_symbol_mut(lib_id)
    }

    fn open_symbol_for_edit(&mut self, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        if lib_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", "symbol", "a symbol needs a lib_id")]);
        }
        if self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).is_some() {
            return Ok(()); // already open -- never resets in-progress edits
        }
        // Same two-tier resolution `ConstraintModel::symbol_of` already
        // does (explicit list -- real libraries already folded in by
        // `board::load`'s `resolve_symbol_libraries` pass, then the
        // builtin table); `ops` cannot read a `.kicad_sym` file directly
        // (no dependency on `eda-kicad`, same boundary `OpenFootprintForEdit`'s
        // own doc explains for footprints), so a lib_id naming a real
        // library symbol *no part in this design already uses* falls
        // through to a blank symbol, same documented simplification.
        let mut lib_sym = match self.model.symbol_of(lib_id) {
            Some(sym) => LibrarySymbol::from_engine_symbol(&sym),
            None => LibrarySymbol::new_empty(lib_id),
        };
        lib_sym.assign_missing_ids();
        self.symbol_library_mut().symbols.push(lib_sym);
        Ok(())
    }

    /// `SYMBOL_EDIT_FRAME::CreateNewSymbol`: see `Cmd::NewSymbol`.
    fn new_symbol(&mut self, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        if lib_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", "symbol", "a symbol needs a lib_id")]);
        }
        if self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).is_some() || self.model.symbol_of(lib_id).is_some() {
            return Err(vec![CheckResult::fail("ops_symbol_exists", lib_id, "a symbol with this name already exists; pick another name")]);
        }
        let mut lib_sym = LibrarySymbol::new_empty(lib_id);
        lib_sym.assign_missing_ids();
        self.symbol_library_mut().symbols.push(lib_sym);
        Ok(())
    }

    /// The symbol and, with it, every symbol derived from it (`LIB_BUFFER::DeleteBuffer`: "Remove all derived symbols to prevent broken inheritance").
    fn delete_library_symbol(&mut self, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        if self.design.symbol_library.is_none() {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", lib_id, "the project symbol library is empty")]);
        }
        let gone = self.symbols_to_delete(lib_id);
        let lib = self.design.symbol_library.as_mut().expect("checked above");
        let before = lib.symbols.len();
        lib.symbols.retain(|s| !gone.contains(&s.lib_id));
        if lib.symbols.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", lib_id, "no symbol with this lib_id in the library")]);
        }
        lib.embedded.retain(|k, _| !gone.contains(k));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_symbol_properties(
        &mut self,
        lib_id: &str,
        attributes: library_editors::SymbolAttributes,
        reference_prefix: String,
        description: String,
        keywords: String,
        datasheet: String,
        power: bool,
        in_bom: bool,
        on_board: bool,
        pin_numbers_hidden: bool,
        pin_names_hidden: bool,
        pin_name_offset_mm: f64,
        unit_count: u32,
        has_alternate_body_style: bool,
        footprint_filters: Vec<String>,
    ) -> Result<(), Vec<CheckResult>> {
        if unit_count == 0 {
            return Err(vec![CheckResult::fail("ops_bad_symbol", lib_id, "a symbol needs at least one unit")]);
        }
        let sym = self.library_symbol_mut(lib_id)?;
        sym.reference_prefix = reference_prefix;
        sym.description = description;
        sym.keywords = keywords;
        sym.datasheet = datasheet;
        sym.power = power;
        sym.in_bom = in_bom;
        sym.on_board = on_board;
        sym.pin_numbers_hidden = pin_numbers_hidden;
        sym.pin_names_hidden = pin_names_hidden;
        sym.pin_name_offset_mm = pin_name_offset_mm;
        sym.unit_count = unit_count;
        sym.has_alternate_body_style = has_alternate_body_style;
        sym.footprint_filters = footprint_filters;
        attributes.apply(sym);
        Ok(())
    }

    fn update_symbol_on_board(&mut self, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        self.library_symbol_mut(lib_id)?.published = true;
        Ok(())
    }

    /// A pin needs a non-empty number (KiCad allows an empty pin *name*,
    /// never an empty number) and a non-empty electrical type -- the one
    /// validation cheap and unambiguous enough to enforce without
    /// transcribing all of `pin_type.h`'s closed vocabulary (same "keep
    /// the raw string, round-trip whatever this port doesn't special-case"
    /// convention `LibPin::electrical_type`'s own doc already accepts).
    fn validate_pin(pin: &LibrarySymbolPin) -> Result<(), Vec<CheckResult>> {
        if pin.number.trim().is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_pin", "pin", "a pin needs a number")]);
        }
        if pin.electrical_type.trim().is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, "a pin needs an electrical type")]);
        }
        if pin.length_mm < 0.0 {
            return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, "pin length cannot be negative")]);
        }
        // `DIALOG_PIN_PROPERTIES::TransferDataFromWindow`: every alternate function has a name; its type and style are the ones a file can say
        for a in &pin.alternates {
            if a.name.is_empty() {
                return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, "Alternate pin definitions must have a name.")]);
            }
            if !library_editors::PIN_TYPE_TOKENS.contains(&a.electrical_type.as_str()) {
                return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, format!("alternate {:?}: {:?} is not a pin electrical type", a.name, a.electrical_type))]);
            }
            if !library_editors::PIN_SHAPE_TOKENS.contains(&a.shape.as_str()) {
                return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, format!("alternate {:?}: {:?} is not a pin graphic style", a.name, a.shape))]);
            }
        }
        Ok(())
    }

    /// `alternates[ alt.m_Name ] = alt` for each row of the grid: a name once (the last definition stays), in the map's order.
    fn normalize_alternates(pin: &mut LibrarySymbolPin) {
        let mut map: std::collections::BTreeMap<String, eda_model::ir::PinAlternate> = std::collections::BTreeMap::new();
        for a in std::mem::take(&mut pin.alternates) {
            map.insert(a.name.clone(), a);
        }
        pin.alternates = map.into_values().collect();
    }

    /// "Alternate pin assignments are not available for symbols with multiple body styles." (`DIALOG_PIN_PROPERTIES`: the alternates panel is disabled for them.)
    fn check_alternates_allowed(sym: &LibrarySymbol, pin: &LibrarySymbolPin) -> Result<(), Vec<CheckResult>> {
        if !pin.alternates.is_empty() && sym.has_alternate_body_style {
            return Err(vec![CheckResult::fail("ops_bad_pin", &pin.number, "Alternate pin assignments are not available for symbols with multiple body styles.")]);
        }
        Ok(())
    }

    fn find_pin_mut<'b>(sym: &'b mut LibrarySymbol, id: &str) -> Result<&'b mut LibrarySymbolPin, Vec<CheckResult>> {
        sym.pins.iter_mut().find(|p| p.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_pin", id, "no pin with this id")])
    }

    fn add_symbol_pin(&mut self, lib_id: &str, mut pin: LibrarySymbolPin) -> Result<(), Vec<CheckResult>> {
        pin.id = String::new(); // ids are ours to assign, never the caller's
        Self::normalize_alternates(&mut pin);
        Self::validate_pin(&pin)?;
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::check_alternates_allowed(sym, &pin)?;
        sym.pins.push(pin);
        sym.assign_missing_ids();
        Ok(())
    }

    fn move_symbol_pin(&mut self, lib_id: &str, id: &str, x: f64, y: f64) -> Result<(), Vec<CheckResult>> {
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::find_pin_mut(sym, id)?.at = eda_model::symbol::SPoint { x, y };
        Ok(())
    }

    fn delete_symbol_pin(&mut self, lib_id: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let sym = self.drawing_symbol_mut(lib_id)?;
        let before = sym.pins.len();
        sym.pins.retain(|p| p.id != id);
        if sym.pins.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_pin", id, "no pin with this id")]);
        }
        Ok(())
    }

    fn edit_symbol_pin(&mut self, lib_id: &str, id: &str, mut pin: LibrarySymbolPin) -> Result<(), Vec<CheckResult>> {
        pin.id = id.to_string();
        Self::normalize_alternates(&mut pin);
        Self::validate_pin(&pin)?;
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::check_alternates_allowed(sym, &pin)?;
        let slot = Self::find_pin_mut(sym, id)?;
        *slot = pin;
        Ok(())
    }

    fn push_pin_property(&mut self, lib_id: &str, source_pin_id: &str, field: PushPinField, body_style: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let sym = self.drawing_symbol_mut(lib_id)?;
        let src = sym.pins.iter().find(|p| p.id == source_pin_id).cloned().ok_or_else(|| vec![CheckResult::fail("ops_unknown_pin", source_pin_id, "no pin with this id")])?;
        let shown_style = body_style.unwrap_or(src.body_style);
        for p in sym.pins.iter_mut().filter(|p| p.id != source_pin_id) {
            match field {
                // `symbol_editor_pin_tool.cpp::PushPinProperties`: a length push reaches a pin that
                // is shared by every body style (`0`) or belongs to the style the editor shows
                // (`!pin->GetBodyStyle() || pin->GetBodyStyle() == m_frame->GetBodyStyle()`);
                // name/number size have no such guard, confirmed directly against that function.
                // (`pin->ChangeLength( sourcePin->GetLength() )`: the pin's inner end stays put, its tip moves.)
                PushPinField::Length if p.body_style == 0 || p.body_style == shown_style => library_editors::change_pin_length(p, src.length_mm),
                PushPinField::Length => {}
                PushPinField::NameSize => p.name_size_mm = src.name_size_mm,
                PushPinField::NumberSize => p.number_size_mm = src.number_size_mm,
            }
        }
        Ok(())
    }

    /// What a library graphic must be to be kept: a polyline of two points or more, a text box with a text size the dialog takes (`DIALOG_TEXT_PROPERTIES`: 0.01 to 1000 mm), that runs
    /// along the box or up it, with margins that are not negative.
    fn check_symbol_graphic(lib_id: &str, graphic: &LibrarySymbolGraphic) -> Result<(), Vec<CheckResult>> {
        match graphic {
            LibrarySymbolGraphic::Polyline { pts, .. } if pts.len() < 2 => Err(vec![CheckResult::fail("ops_bad_shape", lib_id, "a polyline needs at least two points")]),
            LibrarySymbolGraphic::TextBox { size_mm, angle_deg, margin_mm, .. } => {
                if !(0.01..=1000.0).contains(size_mm) {
                    return Err(vec![CheckResult::fail("ops_bad_text", lib_id, "the text size must be between 0.01 and 1000 mm")]);
                }
                if *angle_deg != 0.0 && *angle_deg != 90.0 {
                    return Err(vec![CheckResult::fail("ops_bad_text", lib_id, "a text box's text runs along it (0) or up it (90 degrees)")]);
                }
                if *margin_mm < 0.0 {
                    return Err(vec![CheckResult::fail("ops_bad_text", lib_id, "the margin cannot be negative")]);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn replace_symbol_graphic(&mut self, lib_id: &str, id: &str, mut graphic: LibrarySymbolGraphic) -> Result<(), Vec<CheckResult>> {
        Self::check_symbol_graphic(id, &graphic)?;
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::check_graphic_scope(sym, id, Some(graphic.unit()), Some(graphic.body_style()))?;
        let slot = Self::find_symbol_graphic_mut(sym, id)?;
        if std::mem::discriminant(slot) != std::mem::discriminant(&graphic) {
            return Err(vec![CheckResult::fail("ops_bad_shape", id, "a graphic cannot become another kind of graphic")]);
        }
        graphic.set_id(id.to_string());
        *slot = graphic;
        Ok(())
    }

    fn add_symbol_graphic(&mut self, lib_id: &str, mut graphic: LibrarySymbolGraphic) -> Result<(), Vec<CheckResult>> {
        Self::check_symbol_graphic(lib_id, &graphic)?;
        graphic.set_id(String::new());
        let sym = self.drawing_symbol_mut(lib_id)?;
        sym.graphics.push(graphic);
        sym.assign_missing_ids();
        Ok(())
    }

    fn delete_symbol_graphic(&mut self, lib_id: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let sym = self.drawing_symbol_mut(lib_id)?;
        let before = sym.graphics.len();
        sym.graphics.retain(|g| g.id() != id);
        if sym.graphics.len() == before {
            return Err(vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")]);
        }
        Ok(())
    }

    fn find_symbol_graphic_mut<'b>(sym: &'b mut LibrarySymbol, id: &str) -> Result<&'b mut LibrarySymbolGraphic, Vec<CheckResult>> {
        sym.graphics.iter_mut().find(|g| g.id() == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_shape", id, "no shape with this id")])
    }

    fn move_symbol_graphic(&mut self, lib_id: &str, id: &str, dx_mm: f64, dy_mm: f64) -> Result<(), Vec<CheckResult>> {
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::find_symbol_graphic_mut(sym, id)?.translate(dx_mm, dy_mm);
        Ok(())
    }

    fn edit_symbol_graphic(&mut self, lib_id: &str, id: &str, stroke_mm: f64, fill: LibraryFill, unit: Option<u32>, body_style: Option<u32>) -> Result<(), Vec<CheckResult>> {
        // a negative width is the border switched off (`DIALOG_SHAPE_PROPERTIES`: `stroke.SetWidth( -1 )`, a schematic's smallest unit, 0.0001 mm)
        let stroke_mm = if stroke_mm < 0.0 { NO_BORDER_MM } else { stroke_mm };
        let sym = self.drawing_symbol_mut(lib_id)?;
        Self::check_graphic_scope(sym, id, unit, body_style)?;
        let g = Self::find_symbol_graphic_mut(sym, id)?;
        match g {
            LibrarySymbolGraphic::Rectangle { stroke_mm: s, fill: f, .. } | LibrarySymbolGraphic::Polyline { stroke_mm: s, fill: f, .. } | LibrarySymbolGraphic::Circle { stroke_mm: s, fill: f, .. } | LibrarySymbolGraphic::Arc { stroke_mm: s, fill: f, .. } => {
                *s = stroke_mm;
                *f = fill;
            }
            LibrarySymbolGraphic::Text { .. } => return Err(vec![CheckResult::fail("ops_bad_shape", id, "this is a text item -- use edit_symbol_text")]),
            LibrarySymbolGraphic::TextBox { .. } => return Err(vec![CheckResult::fail("ops_bad_shape", id, "this is a text box -- use replace_symbol_graphic")]),
        }
        if let Some(u) = unit {
            g.set_unit(u);
        }
        if let Some(b) = body_style {
            g.set_body_style(b);
        }
        Ok(())
    }

    /// The unit and the body style a graphic is moved to (`DIALOG_SHAPE_PROPERTIES::TransferDataFromWindow`, `DIALOG_TEXT_PROPERTIES`): 0 is "common to all", and a number
    /// the symbol has otherwise -- a unit up to its unit count, the second body style only when the symbol has one.
    fn check_graphic_scope(sym: &LibrarySymbol, id: &str, unit: Option<u32>, body_style: Option<u32>) -> Result<(), Vec<CheckResult>> {
        if let Some(u) = unit {
            if u > sym.unit_count.max(1) {
                return Err(vec![CheckResult::fail("ops_bad_shape", id, format!("the symbol has no unit {u}"))]);
            }
        }
        if let Some(b) = body_style {
            if b > if sym.has_alternate_body_style { 2 } else { 1 } {
                return Err(vec![CheckResult::fail("ops_bad_shape", id, format!("the symbol has no body style {b}"))]);
            }
        }
        Ok(())
    }

    fn edit_symbol_text(&mut self, lib_id: &str, id: &str, text: String, angle_deg: f64, size_mm: f64) -> Result<(), Vec<CheckResult>> {
        if size_mm <= 0.0 {
            return Err(vec![CheckResult::fail("ops_bad_text", id, "text size must be positive")]);
        }
        let sym = self.drawing_symbol_mut(lib_id)?;
        let g = Self::find_symbol_graphic_mut(sym, id)?;
        match g {
            LibrarySymbolGraphic::Text { text: t, angle_deg: a, size_mm: s, .. } => {
                *t = text;
                *a = angle_deg;
                *s = size_mm;
                Ok(())
            }
            _ => Err(vec![CheckResult::fail("ops_bad_shape", id, "this is not a text item")]),
        }
    }
}

/// The width of a library shape with no border: -1 of a schematic's internal units (0.1 um), what `STROKE_PARAMS` holds and a file says as `(width -0.0001)`.
const NO_BORDER_MM: f64 = -0.0001;

/// Rotate `pt` by `angle_millideg` around `pivot` -- the same matrix
/// `crates/model/src/footprint.rs`'s `to_board` applies to a pad's local
/// offset (no Y-axis flip either side), so a part moved this way lands
/// exactly where its own rendering will subsequently draw it.
fn rotate_point_about(pt: Point, pivot: Point, angle_millideg: i64) -> Point {
    let rad = (angle_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let dx = (pt.x - pivot.x).as_f64();
    let dy = (pt.y - pivot.y).as_f64();
    Point { x: pivot.x + Nm::kiround(dx * cos - dy * sin), y: pivot.y + Nm::kiround(dx * sin + dy * cos) }
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

fn snap(v: Nm, to: Nm) -> Nm {
    if to <= Nm::ZERO {
        return v;
    }
    v.ratio(to).round() as i64 * to
}

fn overlaps(a: (Nm, Nm, Nm, Nm), b: (Nm, Nm, Nm, Nm)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod sch_control_tests;
#[cfg(test)]
mod board_setup_tests;
#[cfg(test)]
mod pcb_transform_tests;
#[cfg(test)]
mod array_tests;
#[cfg(test)]
mod fp_array_tests;
#[cfg(test)]
mod pad_edit_tests;
#[cfg(test)]
mod unused_pads_tests;
#[cfg(test)]
mod pad_nets_tests;
#[cfg(test)]
mod sym_derive_tests;
#[cfg(test)]
mod fp_lib_fields_tests;
#[cfg(test)]
mod lib_keep_tests;
#[cfg(test)]
mod pin_alternates_tests;
#[cfg(test)]
mod sym_graphic_tests;
#[cfg(test)]
mod fp_edit_tests;
#[cfg(test)]
mod fp_models_tests;
#[cfg(test)]
mod pcb_props_tests;
#[cfg(test)]
mod pcb_paste_tests;
#[cfg(test)]
mod design_blocks_tests;
#[cfg(test)]
mod import_graphics_tests;
#[cfg(test)]
mod import_graphics_sch_tests;
#[cfg(test)]
mod pcb_groups_tests;
#[cfg(test)]
mod pcb_items_tests;
#[cfg(test)]
mod embedded_tests;
#[cfg(test)]
mod microwave_tests;
#[cfg(test)]
mod tuning_tests;
#[cfg(test)]
pub(crate) mod sch_annotate_tests;
#[cfg(test)]
mod part_rename_tests;
#[cfg(test)]
mod reannotate_tests;
#[cfg(test)]
mod fp_exchange_tests;
#[cfg(test)]
mod sch_hierarchy_tests;
#[cfg(test)]
mod pcb_edit_details_tests;
#[cfg(test)]
mod pcb_global_edit_tests;
#[cfg(test)]
mod sch_clipboard_tests;
#[cfg(test)]
mod pcb_tables_tests;
#[cfg(test)]
mod sch_tables_tests;

pub mod board_control;
pub mod page_settings;
pub mod build;
pub mod fields_table;
pub mod convert;
pub mod pcb_edit;
pub mod search;
pub mod repair;
pub mod view;
pub mod episode;
pub mod flash;
pub mod sch_edit;
pub mod sch_move;
pub mod sch_pin_helpers;
mod sch_pins;
mod sch_labels;
pub mod sch_net_collision;
mod sch_drag;
mod sch_props;
mod sch_scene;
mod sch_fields;
#[cfg(test)]
mod sch_move_tests;
#[cfg(test)]
mod sch_groups_tests;
#[cfg(test)]
mod sch_fields_tests;
#[cfg(test)]
mod sch_user_fields_tests;
#[cfg(test)]
mod sch_pin_helpers_tests;
#[cfg(test)]
mod sch_body_style_tests;
#[cfg(test)]
mod sch_props_tests;
#[cfg(test)]
mod sch_pins_tests;
#[cfg(test)]
mod sch_labels_tests;
#[cfg(test)]
mod sch_net_collision_tests;
#[cfg(test)]
mod sch_sheet_pin_tests;
#[cfg(test)]
mod multichannel_tests;
