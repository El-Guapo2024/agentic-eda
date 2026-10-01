// Mirrors the JSON the backend actually emits. Source of truth:
//   crates/cli/src/studio.rs   (the `state()` function builds this JSON)
//   crates/ops/src/lib.rs      (the `Cmd` enum, `#[serde(tag = "op")]`)
//   crates/model/src/ir.rs     (Side, LabelSide, Track, Via)
//   crates/model/src/lib.rs    (PlacementRule, `#[serde(tag = "kind")]`)
//   crates/model/src/footprint.rs (Pad shapes)
//
// Coordinates are micrometres (i64 in Rust; safe as JS `number` for the
// board sizes this project deals with). Angles in `rot` are degrees
// (the backend already divides millidegrees by 1000 before sending).

export type Um = number;
export type Degrees = number;

export type Side = "top" | "bottom";
export type LabelSide = "above" | "below" | "left" | "right";

export interface Pad {
  num: string;
  net: string | null;
  x: Um;
  y: Um;
  w: Um;
  h: Um;
  round: boolean;
  /** Through-hole (drilled) vs. surface-mount. */
  th: boolean;
}

/** `[minX, minY, maxX, maxY]`, footprint-local, µm. */
export type CourtyardBox = [Um, Um, Um, Um];

export interface Part {
  ref: string;
  value: string | null;
  package: string | null;
  mpn: string | null;
  /**
   * The name `ConstraintModel::footprint_of` actually resolved this
   * part's pads from (`Part::footprint` if set, else `package`) -- GAPS.md
   * #8's Ctrl+E ("Edit Footprint") opens exactly this name, since it can
   * differ from `package` (an explicit "Lib:Name" vs. a generic bare
   * package string). `null` only for a part with neither set, which has
   * no resolvable footprint at all.
   */
  footprint: string | null;
  /** Floorplan block this part belongs to, if any. */
  block: string | null;
  placed: boolean;
  /** Courtyard full (width, height), µm — present even when unplaced. */
  size: [Um, Um] | null;

  // Present only when `placed` is true:
  at?: [Um, Um];
  rot?: Degrees;
  side?: Side;
  label?: LabelSide;
  courtyard?: CourtyardBox | null;
  pads?: Pad[];
}

export interface Track {
  id: string;
  net: string;
  layer: string;
  width: Um;
  pts: [Um, Um][];
}

export interface Via {
  id: string;
  net: string;
  x: Um;
  y: Um;
  /** Diameter (the backend's `d`, its `Via.diameter`). */
  d: Um;
  drill: Um;
  from: string;
  to: string;
}

/** `crates/model/src/ir.rs` `PadConnection` ("ZONE_CONNECTION") -- exact Rust variant names on the wire (no `rename_all`, see api/types.ts's own convention note below). */
export type PadConnection = "None" | "Thermal" | "Full" | "ThtThermal";
/** `ISLAND_REMOVAL_MODE`. */
export type IslandRemovalMode = "Always" | "Never" | "Area";
/** `ZONE_FILL_MODE`. */
export type FillMode = "Polygons" | "HatchPattern";

/**
 * `crates/model/src/ir.rs` `Zone`'s `ZONE_SETTINGS` fill-engine fields
 * (dialog_copper_zones.cpp's panel) -- everything but id/net/layer/
 * outline, factored out so `Zone` (always populated) and `Cmd`'s
 * `edit_zone`/the dialog's own form state (`ZoneDialog.tsx`) share one
 * field list instead of three copies drifting apart.
 */
export interface ZoneSettingsFields {
  clearance: Um;
  min_thickness: Um;
  thermal_gap: Um;
  thermal_spoke_width: Um;
  pad_connection: PadConnection;
  priority: number;
  island_removal_mode: IslandRemovalMode;
  /** um^2. */
  min_island_area: number;
  fill_mode: FillMode;
  hatch_thickness: Um;
  hatch_gap: Um;
  hatch_orientation_mdeg: number;
  hatch_smoothing_level: number;
  hatch_smoothing_value: number;
  hatch_hole_min_area: number;
  hatch_border_algorithm: number;
}

/**
 * `crates/model/src/ir.rs` `Zone` -- id/net/layer/outline plus the full
 * `ZONE_SETTINGS` (`ZoneSettingsFields`), ported into the IR so
 * `crates/zone-filler` can read them; see `ZoneDialog.tsx`. Every
 * settings field has a KiCad-matching server-side default
 * (`Zone::default()`) once `add_zone` creates a zone, so these are never
 * actually absent from a real `/api/state` response -- not marked
 * optional, same convention this file uses for every other
 * always-present field.
 */
export interface Zone extends ZoneSettingsFields {
  id: string;
  net: string;
  layer: string;
  outline: [Um, Um][];
}

/** `RoutingSection::via_presets`' entry shape -- `BOARD_DESIGN_SETTINGS::m_ViaSizeList`. */
export interface ViaPreset {
  diameter: Um;
  drill: Um;
}

export interface Routing {
  tracks: Track[];
  vias: Via[];
  zones: Zone[];
  /** `BOARD_DESIGN_SETTINGS::m_TrackWidthList` -- Board Setup's editable extra-widths list for the W/Shift+W cycle, beyond `board_rules.track_width`. */
  track_width_presets: Um[];
  /** `m_ViaSizeList` -- same idea, for the via-size cycle. */
  via_presets: ViaPreset[];
}

// ---------------------------------------------------------------- drawings
//
// crates/model/src/ir.rs `Shape`/`Text`. GET /api/state's *display* shape
// (crates/cli/src/studio.rs `shape_json`/`state()`): points are `[x, y]`
// pairs (matching Track.pts/Zone.outline above), Text is flat x/y (not
// nested `at`), angle stays in millidegrees (unlike a Part's `rot`, which
// the backend already divides down to plain degrees) -- confirmed by
// reading studio.rs directly, not assumed from the naming alone. POSTing
// a new Shape/Text back through Cmd is a *different* shape -- see the Cmd
// section below.

export type ShapeKind = "segment" | "arc" | "rect" | "circle" | "polygon";

interface ShapeCommon {
  id: string;
  layer: string;
  stroke_width: Um;
  filled: boolean;
}

export type Shape =
  | (ShapeCommon & { kind: "segment"; start: [Um, Um]; end: [Um, Um] })
  | (ShapeCommon & { kind: "arc"; start: [Um, Um]; mid: [Um, Um]; end: [Um, Um] })
  | (ShapeCommon & { kind: "rect"; start: [Um, Um]; end: [Um, Um] })
  | (ShapeCommon & { kind: "circle"; center: [Um, Um]; end: [Um, Um] })
  | (ShapeCommon & { kind: "polygon"; pts: [Um, Um][] });

export type TextJustify = "left" | "center" | "right";

export interface BoardText {
  id: string;
  content: string;
  x: Um;
  y: Um;
  /** Millidegrees -- NOT divided down like a Part's `rot`. */
  angle: number;
  layer: string;
  size: Um;
  stroke_width: Um;
  justify: TextJustify;
  mirror: boolean;
}

export interface Drawings {
  shapes: Shape[];
  texts: BoardText[];
}

// ------------------------------------------------------- Footprint Editor
//
// GAPS.md #8. GET /api/footprint?name=... returns `crates/model/src/ir.rs`
// `LibraryFootprint` serialized exactly as `design.json` stores it -- no
// second hand-built display shape the way `BoardState`'s `Part`/`Pad` is
// for `ConstraintModel`'s `Footprint`/`Pad` (see that type's own doc on
// why: this editor's whole job IS that IR, there is nothing further to
// project). `graphics`/`texts` reuse `CmdShape`/`CmdText` (not `Shape`/
// `BoardText`): the IR's `Shape`/`Text` types serialize their points as
// `PointXY` ({x,y}) in *every* context, board-level or footprint-local --
// `Shape`/`BoardText` above are `state()`'s own hand-built `[x,y]`-pair
// display shape, which only /api/state uses.

/** `crates/model/src/ir.rs` `LibraryPadShape` (`PAD_SHAPE`, minus KiCad's custom/primitive-based shape -- see that enum's own doc on why). */
export type LibraryPadShape = "circle" | "rect" | "oval" | "round_rect" | "trapezoid" | "chamfered_rect";

/** `crates/model/src/footprint.rs` `PadKind`. */
export type PadKind = "smd" | "through_hole" | "non_plated_hole";

/** `crates/model/src/ir.rs` `ChamferCorners` -- `PAD::SetChamferPositions`'s four corner flags, pad-local (un-rotated). */
export interface ChamferCorners {
  top_left: boolean;
  top_right: boolean;
  bottom_left: boolean;
  bottom_right: boolean;
}

/**
 * `crates/model/src/ir.rs` `LibraryPad` -- one pad on a [`LibraryFootprint`],
 * in the footprint's own local frame (origin at `LibraryFootprint.anchor`).
 * `id` is optional only for a pad not yet sent through `add_pad`/`edit_pad`
 * (the backend always assigns/keeps the real one -- same convention
 * `CmdShape`/`CmdText`'s own `id?` already uses).
 */
export interface LibraryPad {
  id?: string;
  number: string;
  at: PointXY;
  /** `PAD::SetOffset` -- the copper shape's center relative to `at`; `{x:0,y:0}` for the overwhelming majority of pads. */
  offset: PointXY;
  size: [Um, Um];
  shape: LibraryPadShape;
  kind: PadKind;
  drill: Um | null;
  drill_slot: [Um, Um] | null;
  rot: Degrees;
  roundrect_ratio: number | null;
  /** `(dx, dy)` -- KiCad's own convention of only ever one axis non-zero. */
  trapezoid_delta: [Um, Um] | null;
  /** Fraction of the shorter side, same shape as `roundrect_ratio`. Only meaningful for `"chamfered_rect"`. */
  chamfer_ratio: number | null;
  chamfer_corners: ChamferCorners;
  /** KiCad layer names this pad occupies ("F.Cu", "F.Paste", "F.Mask", "*.Cu", ...) -- the Pad Properties dialog's layer-preset buttons fill this in; empty means "not set yet". */
  layers: string[];
  /** `None`/`null` = inherit the board/footprint default, KiCad's own 0-means-inherit convention. */
  clearance_override: Um | null;
  thermal_gap_override: Um | null;
  thermal_spoke_width_override: Um | null;
}

/** `crates/model/src/ir.rs` `FootprintAttributes` (`FOOTPRINT_ATTR_T` plus the dialog's two related non-attribute-bit checkboxes). */
export interface FootprintAttributes {
  smd: boolean;
  through_hole: boolean;
  exclude_from_bom: boolean;
  exclude_from_position_files: boolean;
  board_only: boolean;
  dnp: boolean;
  allow_missing_courtyard: boolean;
  allow_soldermask_bridges: boolean;
}

/** `crates/model/src/ir.rs` `FootprintField` -- a custom field beyond the built-in Reference/Value. */
export interface FootprintField {
  name: string;
  value: string;
  visible: boolean;
}

/** The fields `edit_footprint_properties` commits all at once (dialog_footprint_properties_fp_editor.cpp's General tab) -- factored out so the `Cmd` variant and `FootprintPropertiesDialog`'s own form state share one field list. */
export interface FootprintPropertiesFields {
  description: string;
  keywords: string;
  attributes: FootprintAttributes;
  reference_visible: boolean;
  value_visible: boolean;
  model: string | null;
}

/**
 * `crates/model/src/ir.rs` `LibraryFootprint` -- the Footprint Editor's
 * own open document. `published` mirrors the field of the same name:
 * whether a board instance naming this footprint currently resolves its
 * pads from here (`Cmd::UpdateFootprintOnBoard`'s own explicit switch,
 * never automatic).
 */
export interface LibraryFootprint extends FootprintPropertiesFields {
  name: string;
  pads: LibraryPad[];
  graphics: CmdShape[];
  texts: CmdText[];
  fields: FootprintField[];
  courtyard: [Um, Um] | null;
  anchor: PointXY;
  published: boolean;
}

/** `GET /api/footprint_library`'s one field: every footprint name available to open (already-opened library entries plus intent/real-library-resolved ones), sorted. */
export interface FootprintLibraryNames {
  names: string[];
}

export interface Check {
  check: string;
  fail: boolean;
  at: string | null;
  hint: string | null;
}

export interface Activity {
  /** Unix millis. */
  t: number;
  /** "ui", "cli", or whatever EDA_ACTOR was set to. */
  by: string;
  cmd: string;
  ok: boolean;
  message: string;
}

/** crates/model/src/lib.rs `PlacementRule`, `#[serde(tag = "kind")]`. */
export type Rule =
  | { kind: "proximity"; a: string; b: string; max_mm: number; reason?: string }
  | { kind: "separation"; a: string; b: string; min_mm: number; reason?: string }
  | { kind: "keepout"; zone: string; refs: string[] }
  | { kind: "thermal_group"; refs: string[] };

export interface BoardState {
  name: string;
  dir: string;
  outline: [Um, Um][] | null;
  /** Copper stackup layer names, e.g. ["F.Cu", "B.Cu"]. */
  layers: string[];
  /** Placement grid pitch, µm. */
  snap: Um;
  parts: Part[];
  rules: Rule[];
  routing: Routing | null;
  drawings: Drawings | null;
  checks: Check[];
  /** Most recent 60 activity.jsonl entries, newest first. */
  activity: Activity[];
  /** "idle" | "running" | a one-line result of the last route. */
  job: string;
  /**
   * Board-wide track/via defaults, for the auxiliary toolbar's
   * display-only track-width/via-size indicators, and (this session) the
   * Board Setup dialog. Optional: only present once the backend serving
   * this board has picked up the `board_rules` field (crates/cli/src/
   * studio.rs) -- older `eda` binaries won't send it.
   */
  board_rules?: {
    track_width: Um;
    via_drill: Um;
    via_diameter: Um;
    clearance: Um;
    /**
     * `crates/model/src/lib.rs` `NetClass` -- read-only here: net classes
     * live on the *intent*-derived `ConstraintModel`, not the editable
     * `design.json` IR this app's `Cmd`s mutate, so there is no command
     * to change them yet (GAPS.md #10; see BoardSetupDialog.tsx's Net
     * Classes panel, which shows this list but cannot edit it).
     */
    net_classes: NetClass[];
    hole_to_hole_min_um: Um;
    hole_clearance_um: Um;
    silk_clearance_um: Um;
    annular_width_min_um: Um;
    min_silk_text_height_um: Um;
    min_silk_text_thickness_um: Um;
    refdes_font_um: Um | null;
    stackup: Stackup | null;
  };
}

/** `crates/model/src/lib.rs` `NetClass` -- read-only, see `BoardState.board_rules.net_classes`'s doc. */
export interface NetClass {
  name: string;
  /** Globs over net names, e.g. `["GND", "VBAT*"]` -- first class whose pattern matches a net owns it. */
  nets: string[];
  track_width?: Um;
  clearance?: Um;
  via_diameter?: Um;
  via_drill?: Um;
  microvia_diameter?: Um;
  microvia_drill?: Um;
  diff_pair_width?: Um;
  diff_pair_gap?: Um;
  diff_pair_via_gap?: Um;
  priority: number;
}

/** `crates/model/src/lib.rs` `Stackup`/`StackupLayer` -- read-only, same reasoning as `NetClass`. */
export interface Stackup {
  layers: StackupLayer[];
}
export interface StackupLayer {
  name: string;
  material: string | null;
  thickness_mm: number | null;
}

// ---------------------------------------------------------------- Cmd
//
// eda_ops::Cmd, `#[serde(tag = "op", rename_all = "snake_case")]`.
// Commands never carry raw coordinates except PlaceAt/MoveTo — see the
// module doc in crates/ops/src/lib.rs for why.

export type Dir = "north" | "south" | "east" | "west";
export type Region = "north_west" | "north" | "north_east" | "west" | "centre" | "east" | "south_west" | "south" | "south_east";

// crates/model/src/ir.rs `Point` -- a plain {x,y} struct with the derived
// serde impl (no custom flattening), so this is what a `Vec<Point>` field
// (Track.pts, Zone.outline, a Shape's own geometry, Text.at) actually
// serializes to. NOT the same as the `[x, y]` pairs GET /api/state uses
// for the exact same data -- that array form is `state()`'s own hand-
// built display JSON, not IR's native shape.
export interface PointXY {
  x: Um;
  y: Um;
}

/** crates/model/src/ir.rs `Shape`, IR field names/point shape -- for `add_shape` only. Same `kind` tags as the display `Shape` above, different point representation ({x,y}, not [x,y]) and `id` left empty for a new one (the backend assigns it). */
export type CmdShape =
  | { kind: "segment"; id?: string; layer: string; stroke_width: Um; filled: boolean; start: PointXY; end: PointXY }
  | { kind: "arc"; id?: string; layer: string; stroke_width: Um; filled: boolean; start: PointXY; mid: PointXY; end: PointXY }
  | { kind: "rect"; id?: string; layer: string; stroke_width: Um; filled: boolean; start: PointXY; end: PointXY }
  | { kind: "circle"; id?: string; layer: string; stroke_width: Um; filled: boolean; center: PointXY; end: PointXY }
  | { kind: "polygon"; id?: string; layer: string; stroke_width: Um; filled: boolean; pts: PointXY[] };

/** crates/model/src/ir.rs `Text`, IR field names -- for `add_text` only (`edit_text` takes flat fields instead, see Cmd below). */
export interface CmdText {
  id?: string;
  content: string;
  at: PointXY;
  angle: number;
  layer: string;
  size_um: Um;
  stroke_width: Um;
  justify: TextJustify;
  mirror: boolean;
}

// crates/model/src/ir.rs `Track`/`Via`/`Zone`, full IR shape (PointXY,
// not the display [x,y] pairs) -- only `paste_items` needs a whole one of
// these at once; every other Cmd that touches a track/via/zone takes
// flat fields instead (`add_track`/`add_via`/`add_zone`/`move_via`/...).
// `id` is accepted but always ignored by the backend (`insert_copies`
// blanks and reassigns it, same as `add_shape`/`add_text`) -- optional
// here for callers that would rather not bother clearing it themselves.
export interface CmdTrack {
  id?: string;
  net: string;
  layer: string;
  width: Um;
  pts: PointXY[];
}
export interface CmdVia {
  id?: string;
  net: string;
  at: PointXY;
  drill: Um;
  diameter: Um;
  from_layer: string;
  to_layer: string;
}
/**
 * Every `ZONE_SETTINGS` field is optional here (`Zone::default()` fills
 * in whichever the caller skips, server-side via serde) -- `add_zone`'s
 * call sites that only ever cared about net/layer/outline (ZoneDialog's
 * "Add Zone" before this session's settings panel, the CLI) keep working
 * unchanged; `clipboard.ts`'s `zoneToCmd` sends every field explicitly so
 * a copy/paste or duplicate of a customized zone does not silently reset
 * it to KiCad's defaults.
 */
export interface CmdZone extends Partial<ZoneSettingsFields> {
  id?: string;
  net: string;
  layer: string;
  outline: PointXY[];
}

export type Cmd =
  | { op: "place"; part: string; anchor: string; side: Dir }
  | { op: "place_edge"; part: string; edge: Dir; fraction: number }
  | { op: "place_region"; part: string; region: Region }
  | { op: "place_at"; part: string; x: Um; y: Um }
  | { op: "move_to"; part: string; x: Um; y: Um }
  | { op: "nudge"; part: string; dir: Dir; steps: number }
  | { op: "rotate"; part: string; quarter_turns: number }
  | { op: "swap"; a: string; b: string }
  | { op: "rip"; part: string }
  | { op: "flip"; part: string }
  /** Footprint Properties' "Text Placement" field (the refdes label's side) -- does not move anything, so (unlike Flip/Rotate/MoveTo) it does not clear routing. */
  | ({ op: "set_label_side"; part: string } & { side: LabelSide })
  | { op: "add_track"; net: string; layer: string; width: Um; pts: PointXY[] }
  | { op: "delete_track"; id: string }
  | { op: "set_track_width"; id: string; width: Um }
  | { op: "add_via"; net: string; x: Um; y: Um; drill: Um; diameter: Um; from_layer: string; to_layer: string }
  | { op: "delete_via"; id: string }
  | { op: "move_via"; id: string; x: Um; y: Um }
  /** Track/Via Properties' editable via fields -- net, position and layer span are unchanged (KiCad does not let you re-net/re-span an existing via from this dialog either). */
  | { op: "edit_via"; id: string; diameter: Um; drill: Um }
  /** Board Setup > Track Widths & Vias: replace the W/Shift+W preset list wholesale (no per-entry add/remove Cmd -- same spirit as `paste_items`). The board's own default (`board_rules.track_width`) is not part of this list. */
  | { op: "set_track_width_presets"; widths: Um[] }
  /** Same panel's via-size-cycle preset list. */
  | { op: "set_via_presets"; presets: ViaPreset[] }
  | { op: "add_zone"; net: string; layer: string; outline: PointXY[] }
  | { op: "delete_zone"; id: string }
  /**
   * `dialog_copper_zones.cpp`'s "OK": replace a zone's net/layer and every
   * `ZONE_SETTINGS` field at once -- KiCad has no concept of editing just
   * one field of the panel, the whole thing commits together. Outline is
   * untouched (no point editor yet, see PARITY-pcb.md).
   */
  | ({ op: "edit_zone"; id: string; net: string; layer: string } & ZoneSettingsFields)
  /** `pcb_point_editor.cpp`'s zone-outline editing (drag/add/remove a corner) -- the whole edited outline, replacing it wholesale (no live point-by-point Cmd). */
  | { op: "set_zone_outline"; id: string; outline: PointXY[] }
  | { op: "add_shape"; shape: CmdShape }
  | { op: "delete_shape"; id: string }
  | { op: "move_shape"; id: string; dx: Um; dy: Um }
  /** Shape Properties' editable fields (layer, line width, filled) -- geometry has no dialog field to edit in source either, only by dragging its own points (no point editor for shapes yet). */
  | { op: "edit_shape"; id: string; layer: string; stroke_width: Um; filled: boolean }
  | { op: "add_text"; text: CmdText }
  | { op: "edit_text"; id: string; content: string; angle: number; layer: string; size_um: Um; stroke_width: Um; justify: TextJustify; mirror: boolean }
  | { op: "delete_text"; id: string }
  | { op: "move_text"; id: string; x: Um; y: Um }
  /** Cmd+D: copy existing tracks/vias/zones/shapes/texts named by id, in place, with fresh ids. Never footprints -- see crates/ops/src/lib.rs `Cmd::Duplicate`'s own doc comment. */
  | { op: "duplicate"; ids: string[] }
  /** Cmd+V: insert fresh copies of whole items (ids ignored/reassigned) -- the clipboard's own full data, not references, so paste still works after the original was deleted. */
  | { op: "paste_items"; tracks?: CmdTrack[]; vias?: CmdVia[]; zones?: CmdZone[]; shapes?: CmdShape[]; texts?: CmdText[] }
  /** Shift+M "Move Exactly...": translate every named part by the same (dx, dy), then rotate each by the same `rotate_millideg` around `pivot` (null = each part's own anchor -- a pure spin). */
  | { op: "move_exact"; parts: string[]; dx: Um; dy: Um; rotate_millideg: number; pivot: PointXY | null }

  // -------------------------------------------------------- eeschema
  // crates/ops/src/lib.rs's eeschema `Cmd` variants -- see that enum's
  // own doc comment for what each hotkey/tool sends. `rot_millideg`
  // (not `rot`) on purpose, same convention `move_exact`'s own
  // `rotate_millideg` already set: a write-side angle is always named
  // for its unit, since the read side (`SchematicSymbol.rot`, `Degrees`)
  // uses plain degrees instead.
  | { op: "move_symbol"; id: string; x: Um; y: Um }
  | { op: "drag_symbol"; id: string; x: Um; y: Um; attached_wire_endpoints: [number, number][] }
  | { op: "rotate_symbol"; id: string; quarter_turns: number }
  | { op: "mirror_symbol"; id: string }
  | { op: "mirror_symbol_vertical"; id: string }
  | { op: "delete_symbol"; id: string }
  | { op: "add_wire"; pts: PointXY[] }
  | { op: "delete_wire"; id: string }
  | { op: "add_no_connect"; at: PointXY }
  | { op: "delete_no_connect"; id: string }
  | { op: "add_label"; net: string; at: PointXY; kind: CmdLabelKind }
  | { op: "delete_label"; id: string }
  | { op: "add_sch_text"; content: string; at: PointXY; angle_millideg: number; size_um: Um }
  | { op: "delete_sch_text"; id: string }
  | { op: "add_power_symbol"; lib_id: string; at: PointXY; rot_millideg: number; net: string; pin: string }
  | { op: "delete_power_symbol"; id: string }
  | { op: "add_symbol"; id: string; lib_id: string; at: PointXY; rot_millideg: number; value: string; footprint: string }
  | { op: "edit_symbol_fields"; id: string; value?: string | null; footprint?: string | null; datasheet?: string | null }
  | { op: "rename_symbol"; id: string; new_id: string }
  | { op: "annotate"; reset_existing: boolean; order?: "y_then_x" | "x_then_y"; ids?: string[] }
  /** `dialog_erc.cpp`'s "Exclude this violation" / un-exclude -- `(check, location)` keys exactly one `ErcViolation`, matching it byte-for-byte against the same `location` string GET /api/erc reported (see `ErcViolation.location`'s own doc for the shapes that can be). Refused server-side when `location` is empty -- nothing to key an exclusion on. */
  | { op: "add_erc_exclusion"; check: string; location: string }
  | { op: "delete_erc_exclusion"; check: string; location: string }

  // -------------------------------------------------- footprint editor
  //
  // GAPS.md #8. crates/ops/src/lib.rs's own "footprint editor" Cmd
  // section, same order. `Domain::FootprintEditor` (api/client.ts's
  // `postUndo`/`postRedo` `domain` param) -- its own undo/redo scope,
  // independent of "pcb"/"schematic".
  | { op: "open_footprint_for_edit"; name: string }
  | { op: "delete_library_footprint"; name: string }
  | ({ op: "edit_footprint_properties"; name: string } & FootprintPropertiesFields)
  | { op: "set_footprint_anchor"; name: string; at: PointXY }
  | { op: "update_footprint_on_board"; name: string }
  | { op: "add_pad"; footprint: string; pad: LibraryPad }
  | { op: "move_pad"; footprint: string; id: string; x: Um; y: Um }
  | { op: "rotate_pad"; footprint: string; id: string; quarter_turns: number }
  | { op: "delete_pad"; footprint: string; id: string }
  | { op: "edit_pad"; footprint: string; id: string; pad: LibraryPad }
  | { op: "push_pad_properties"; footprint: string; source_pad_id: string; filter_shape: boolean; filter_orientation: boolean; filter_layers: boolean; filter_type: boolean }
  | { op: "renumber_pads"; footprint: string; start: number; prefix: string; step: number }
  | { op: "add_footprint_graphic"; footprint: string; shape: CmdShape }
  | { op: "delete_footprint_graphic"; footprint: string; id: string }
  | { op: "move_footprint_graphic"; footprint: string; id: string; dx: Um; dy: Um }
  | { op: "edit_footprint_graphic"; footprint: string; id: string; layer: string; stroke_width: Um; filled: boolean }
  | { op: "add_footprint_text"; footprint: string; text: CmdText }
  | { op: "edit_footprint_text"; footprint: string; id: string; content: string; angle: number; layer: string; size_um: Um; stroke_width: Um; justify: TextJustify; mirror: boolean }
  | { op: "delete_footprint_text"; footprint: string; id: string }
  | { op: "move_footprint_text"; footprint: string; id: string; x: Um; y: Um };

/** crates/model/src/ir.rs `LabelKind`, `#[serde(tag = "scope")]` -- for `add_label` only (`SchematicLabel`'s own `scope`/`shape` pair is the read-side mirror of this). */
export type CmdLabelKind = { scope: "local" } | { scope: "global"; shape: LabelShape } | { scope: "hierarchical"; shape: LabelShape };

export interface CmdReply {
  ok: boolean;
  message: string;
}

export interface RouteReply {
  ok: boolean;
  message: string;
}

// ------------------------------------------------------- interactive router
//
// POST /api/route/{start,move,fix,undo_segment,via,finish,cancel}
// (crates/cli/src/route_api.rs, gap #7's interactive push-and-shove
// router). Unlike every other POST here, these drive a session that lives
// in the backend's memory between calls (see that file's own doc comment)
// rather than reading/writing design.json on every request -- only
// `finish`'s reply actually changes the board.

export type RouteMode = "mark_obstacles" | "walkaround" | "shove";

/** A fixed (already-placed-this-session) or displaced run of track. */
export interface RoutePreviewRun {
  layer: string;
  pts: [Um, Um][];
}

export interface RouteDisplacedLine {
  source_track: string | null;
  layer: string;
  pts: [Um, Um][];
}

/** `Mode::Shove` only: a via the live preview would push aside -- no
 * diameter/drill of its own (the board still carries the real via, by
 * `source_via`, until a commit actually replaces it; the frontend looks
 * those up from `BoardState.routing.vias` itself rather than this
 * repeating them over the wire). */
export interface RouteDisplacedVia {
  source_via: string;
  x: Um;
  y: Um;
}

export interface RouteVia {
  x: Um;
  y: Um;
  diameter: Um;
  drill: Um;
}

/** The shape every one of start/move/fix's successful replies carries
 * (fix nests it under `preview`; see `RouteFixReply`). */
export interface RoutePreview {
  ok: boolean;
  message?: string;
  net: string | null;
  colliding: boolean;
  layer: string;
  /** The live, not-yet-fixed head: last fixed point (or the route's
   * origin) to the cursor, already resolved (walked/shoved/mark-obstacled)
   * server-side -- draw this directly, no client-side posture math needed. */
  head: [Um, Um][];
  /** Already-fixed runs from earlier in this session (e.g. before a via/
   * layer switch) -- empty until the route has more than one leg. */
  runs: RoutePreviewRun[];
  via: RouteVia | null;
  /** A same-net anchor near the cursor the route would snap onto and
   * finish at, if fixed now. */
  snapped_end: [Um, Um] | null;
  /** `Mode::Shove` only: other tracks this preview would push out of the
   * way if accepted. */
  displaced: RouteDisplacedLine[];
  /** `Mode::Shove` only: other vias this preview would push out of the way. */
  displaced_vias: RouteDisplacedVia[];
}

export interface RouteFixReply {
  ok: boolean;
  message?: string;
  /** `true` if the head still collides and the active mode refused to fix
   * it (nothing changed; keep moving the cursor and try again). */
  blocked: boolean;
  /** `true` if this fix reached a same-net anchor and finished the whole
   * connection -- call `postRouteFinish` at the same point next, or just
   * read `preview` (the route is already fully committed into the
   * session's own runs at this point). */
  real_end?: boolean;
  preview?: RoutePreview;
}

/** `POST /api/route/drag_{start,move,finish}` (gap #7 stage 5, `D`):
 * dragging an existing track segment/corner or via while it keeps its
 * connections -- `eda_pns::dragger::Dragger`. Shares the same backend
 * session as the route endpoints above (`crates/cli/src/route_api.rs`'s
 * own doc comment); only one of a route or a drag can be in progress at
 * once. */
export interface DragPreview {
  ok: boolean;
  message?: string;
  colliding: boolean;
  /** The dragged item's own new shape: just its live position (1 point)
   * for a via, or N points for a stretched track corner drag. */
  pts: [Um, Um][];
  /** `shove` mode only: other tracks this drag would push out of the way. */
  displaced: RouteDisplacedLine[];
  /** `shove` mode only: other vias this drag would push out of the way. */
  displaced_vias: RouteDisplacedVia[];
  /** Via drag only: the via's own directly-attached tracks, each already
   * stretched to follow `pts[0]` -- draw these too, or a dragged via's
   * connections won't visibly follow it until the drag commits. Always
   * empty for a corner drag (see `DragPreview::fanout` on the Rust side).
   * Carries its own `width` (unlike `RoutePreviewRun`'s `runs`/`displaced`,
   * which reuse the one active session width) since each attached track
   * can genuinely have a different width from its neighbors. */
  fanout: { layer: string; width: Um; pts: [Um, Um][] }[];
}

/** `POST /api/route/dp_{start,move,fix,undo_segment,finish}` (gap #7 task
 * item 6, `6`): route two parallel, gap-matched lines (a differential
 * pair) from one session -- `eda_pns::diff_pair::DiffPairPlacer`. Shares
 * the same backend session as the route/drag endpoints above; only one of
 * a route, a drag, or a diff pair can be in progress at once. No shove/
 * walkaround/via-switch for a pair in this port -- `colliding` is report-
 * only, same contract as a plain route in `mark_obstacles` mode -- see
 * `crates/pns/PARITY.md`'s own diff-pair section for why. */
export interface DiffPairPreview {
  ok: boolean;
  message?: string;
  net_a: string | null;
  net_b: string | null;
  layer: string;
  width: Um;
  colliding: boolean;
  /** The live, not-yet-fixed head of each line -- already resolved
   * (offset from the spine, snapped to the real pad(s) at either end)
   * server-side, draw directly. */
  head_a: [Um, Um][];
  head_b: [Um, Um][];
  /** Already-fixed runs from earlier in this session, one array per line. */
  runs_a: RoutePreviewRun[];
  runs_b: RoutePreviewRun[];
  /** Both lines found a same-net anchor near the cursor at once --
   * finishing now lands exactly on a real coupled pad pair. */
  snapped_end: boolean;
}

export interface DpFixReply {
  ok: boolean;
  message?: string;
  blocked: boolean;
  real_end?: boolean;
  preview?: DiffPairPreview;
}

/** `POST /api/tune_length/{preview,apply}` (gap #7 task item 4, `7`):
 * lengthen a straight, single-segment track to a target length by
 * inserting a meander -- `eda_pns::meander`. Unlike every other
 * interactive-router endpoint, this is entirely stateless: no session,
 * every call re-reads `design.json` fresh (see that module's own doc
 * comment on why length tuning is a one-shot dialog here, not a third
 * live mouse-driven session). */
export interface TuneLengthReply {
  ok: boolean;
  message?: string;
  net?: string;
  layer?: string;
  width?: Um;
  /** The source track's own straight-line length, before tuning. */
  original_length?: Um;
  /** The generated meander's real, measured length -- usually within a
   * few um of the requested target when reachable, see that module's own
   * doc comment on why it isn't always exact to the micrometer. */
  achieved_length?: Um;
  pts?: [Um, Um][];
  colliding?: boolean;
}

// ---------------------------------------------------------------- Ratsnest
//
// GET /api/ratsnest. Source of truth: crates/cli/src/studio.rs
// `ratsnest_json()`, crates/connectivity (KiCad's own connectivity +
// ratsnest algorithm, ported: Delaunay + Kruskal MST between connectivity
// clusters -- ground-truthed against kicad-cli's own unconnected-item
// list). Coordinates are integer board µm, same convention as everything
// else in BoardState.

export interface RatsnestEdge {
  net: string;
  from: [Um, Um];
  to: [Um, Um];
}

export interface Ratsnest {
  edges: RatsnestEdge[];
}

// ---------------------------------------------------------------- fill
//
// GET /api/fill. Source of truth: crates/cli/src/studio.rs `fill_json()`,
// `crates/zone-filler` via `eda_drc::fill::fill_all_zones` -- the real
// KiCad zone-fill algorithm (clearance/thermal-relief/min-width/island
// knockouts), computed fresh on every call (no caching, same reasoning as
// /api/drc). `pcbnew.ZoneFiller.zoneFillAll`/`zoneUnfillAll` (B/Ctrl+B,
// zone_filler_tool.cpp) are this app's own fetch/clear of this report --
// see state/store.tsx's `zoneFill`.

/** One zone's computed fill -- possibly several disjoint fragments (islands). Each fragment is a single closed ring, already "Fracture"d (slit at any hole), never a separate outer+holes pair. */
export interface FillZone {
  id: string;
  net: string;
  layer: string;
  area_um2: number;
  fragments: [Um, Um][][];
}

export interface FillReport {
  zones: FillZone[];
}

// ---------------------------------------------------------------- board.glb
//
// GET /api/board.glb. Source of truth: crates/cli/src/studio.rs
// `serve_board_glb`/`build_glb`/`GlbBuild`. Unlike every other route in
// this file, a single GET does not necessarily carry the answer: the
// backend runs kicad-cli's (potentially multi-minute) STEP-model export
// on its own thread and answers immediately either way --
//  - 202, body `{"status":"pending"}`               -- still building
//  - 200, `Content-Type: model/gltf-binary`          -- the GLB itself
//  - 200, body `{"status":"failed","error":string}`  -- kicad-cli
//    errored or timed out (left cached as-is until the board's version
//    actually changes -- the backend does not retry on its own, and
//    Viewer3D.tsx must not either).
// api/client.ts's `fetchBoardGlb` turns this into one discriminated
// union rather than exposing the raw HTTP shape.
export type BoardGlbResult = { status: "pending" } | { status: "failed"; error: string } | { status: "ready"; bytes: ArrayBuffer };

// ---------------------------------------------------------------- Schematic
//
// GET /api/schematic. Source of truth: crates/cli/src/studio.rs
// `schematic_json()`, crates/model/src/ir.rs `SchematicSection`/
// `SymbolInstance`/`Wire`/`NetLabel`, crates/model/src/lib.rs `Pin`/`PinKind`.
//
// `lib_symbols`/`power_symbols`/`no_connects`/`title_block` and `symbols[]`'s
// lib_id/unit/footprint/datasheet, and `labels[]`'s scope/shape, mirror the
// just-finished Eeschema-port merge (sch_painter.cpp/lib_symbol.h/sch_pin.h,
// ported to crates/kicad) -- written from the coordinator's description of
// that contract *before* the merge landed in this worktree, so the shapes
// below are this session's best-effort match, not read off the real JSON.
// Tag/field names follow this file's own established convention elsewhere
// (snake_case tags, `rename_all = "snake_case"` -- see the PCB `Shape` type
// above) and eeschema's real enum names (ELECTRICAL_PINTYPE, GRAPHIC_
// PINSHAPE, LABEL_FLAG_SHAPE) where those differ from this app's own PCB
// vocabulary. `git merge main` once it lands and reconcile any mismatch
// here first -- the fix should be small and mechanical (this file's types
// and painter.ts's tag `switch`es are the only two places that assume this
// shape).

export type PinKind = "power" | "ground" | "signal" | "passive" | "nc";

export interface SchematicPin {
  number: string;
  name: string | null;
  kind: PinKind;
}

/** eeschema's ELECTRICAL_PINTYPE, serialized name per .kicad_sym. */
export type PinElectricalType =
  | "input"
  | "output"
  | "bidirectional"
  | "tri_state"
  | "passive"
  | "free"
  | "unspecified"
  | "power_in"
  | "power_out"
  | "open_collector"
  | "open_emitter"
  | "no_connect";

/** eeschema's GRAPHIC_PINSHAPE -- the decoration drawn at a pin's outer (connection) end. */
export type PinShape = "line" | "inverted" | "clock" | "inverted_clock" | "input_low" | "clock_low" | "output_low" | "edge_clock_high" | "non_logic";

/** Plain millimetres (a library symbol's own native unit, not this file's usual Um) -- the coordinator's contract description names pin length "length_mm" specifically; kept as a distinct alias so every place that touches library-symbol geometry is grep-able. */
export type Mm = number;

/** One pin on a library symbol, in that symbol's own local coordinate space -- eeschema's SCH_PIN. */
export interface LibPin {
  number: string;
  name: string | null;
  electrical_type: PinElectricalType;
  shape: PinShape;
  /** The pin's *outer*, wire-connection end (KiCad's convention: a pin's `at` is the free end, not the body-attachment end) -- local to the owning lib_id, mm, library Y-up. */
  at: [Mm, Mm];
  /** Direction from `at` *into* the symbol body, degrees -- 0/90/180/270 in every real library symbol. */
  angle_deg: Degrees;
  length_mm: Mm;
  /** 0 = shared by every unit (KiCad's convention for a multi-unit symbol's common graphics/pins); otherwise the 1-based unit this pin belongs to. */
  unit: number;
  /** 0 = shared by every body style (DeMorgan alternate); otherwise 1 (normal) or 2 (alternate). Most symbols have no alternate and every item is 0 or 1. */
  body_style: number;
  /** eeschema's `(hide yes)` -- true for almost every power-symbol pin (the pin itself is invisible; only its net-name text, drawn separately as the field KiCad calls the symbol's Value, is ever shown -- see PowerSymbol's doc comment). A hidden pin is not drawn at all, line or decoration. */
  hidden: boolean;
}

/** eeschema's FILL_T. `color`/the hatch modes are rare in practice (most real symbols use none/outline/background) -- painter.ts treats any hatch mode as `outline` rather than leaving it blank, since real KiCad never leaves a filled-looking shape unfilled. */
export type LibFill = "none" | "outline" | "background" | "color" | "hatch" | "reverse_hatch" | "cross_hatch";

/** One graphic item on a library symbol, local coordinate space (mm, library Y-up), `unit`/`body_style`: 0 = shared by every unit/style -- KiCad's SHAPE_T-derived library graphics (RECTANGLE/POLY/CIRCLE/ARC) plus SCH_TEXT. */
export type LibGraphic =
  | { kind: "rectangle"; start: [Mm, Mm]; end: [Mm, Mm]; stroke_width: Mm; fill: LibFill; unit: number; body_style: number }
  | { kind: "polyline"; pts: [Mm, Mm][]; stroke_width: Mm; fill: LibFill; unit: number; body_style: number }
  | { kind: "circle"; center: [Mm, Mm]; radius: Mm; stroke_width: Mm; fill: LibFill; unit: number; body_style: number }
  | { kind: "arc"; start: [Mm, Mm]; mid: [Mm, Mm]; end: [Mm, Mm]; stroke_width: Mm; fill: LibFill; unit: number; body_style: number }
  | { kind: "text"; content: string; at: [Mm, Mm]; angle_deg: Degrees; size_mm: Mm; unit: number; body_style: number };

export interface LibSymbol {
  graphics: LibGraphic[];
  pins: LibPin[];
}

/** GET /api/schematic's `lib_symbols`: every distinct lib_id used on the sheet, keyed by that lib_id ("Device:R", "power:GND", ...). */
export type LibSymbols = Record<string, LibSymbol>;

export interface SchematicSymbol {
  /** Reference designator ("U1") -- the same id PCB parts use. */
  id: string;
  /** Key into `Schematic.lib_symbols` ("Device:R"). Null, or present but missing from `lib_symbols`, both mean "no real graphics for this instance yet" -- painter.ts falls back to the generic box either way. */
  lib_id: string | null;
  at: [Um, Um];
  rot: Degrees;
  /** KiCad mirrors one axis at a time, never both at once as a single flag -- `(mirror x)` flips the symbol vertically (negates Y), `(mirror y)` flips it horizontally (negates X). Replaces an earlier, incomplete `mirrored: boolean` that could only ever express one of the two. */
  mirror: "x" | "y" | null;
  /** 1-based unit (gate) of a multi-unit symbol -- e.g. a quad op-amp, or ecc83-pp's three-triode ECC83. */
  unit: number;
  /** 0 = no DeMorgan alternate (every real-world symbol in this app so far); 1 = normal, 2 = alternate, when one exists. */
  body_style: number;
  value: string | null;
  mpn: string | null;
  package: string | null;
  footprint: string | null;
  datasheet: string | null;
  pins: SchematicPin[];
}

/**
 * A power symbol instance (GND, +5V, PWR_FLAG, ...) -- eeschema draws
 * these from the same LIB_SYMBOL machinery as any other symbol, but this
 * app's backend reports them separately since this app has no other use
 * for a generic symbol carrying exactly one (almost always hidden,
 * zero-length) pin. The visible net-name text beside the symbol is real
 * KiCad's Value *field* (drawn exactly like a resistor's "10k", not pin
 * text) -- confirmed directly from sch_painter.cpp this session, correcting
 * an earlier assumption here that it was the pin's own name. This app has
 * no per-instance field-position data to place that field at its real
 * recorded position, so painter.ts places the `net` text using the same
 * pin-name-offset convention a normal pin's name would use (a reasonable,
 * KiCad-adjacent approximation, not a pixel-exact port -- `pin.name` is
 * normally identical to `net` anyway for a real power symbol, e.g. a GND
 * pin is named "GND").
 */
export interface PowerSymbol {
  id: string;
  lib_id: string;
  at: [Um, Um];
  rot: Degrees;
  net: string;
  pin: SchematicPin;
}

export interface NoConnect {
  /** Stable id (`nc_xxxxxxxxxxxx`) -- for `delete_no_connect`. */
  id: string;
  at: [Um, Um];
}

export type LabelScope = "local" | "global" | "hierarchical";
/** eeschema's LABEL_FLAG_SHAPE -- which outline the label's text sits inside. Meaningful for "global"/"hierarchical" only; a "local" label has no outline. */
export type LabelShape = "input" | "output" | "bidirectional" | "tri_state" | "passive";

export interface SchematicWire {
  /** Stable id (`wire_xxxxxxxxxxxx`) -- for `delete_wire`/selecting this one wire. */
  id: string;
  net: string;
  /** "REF.PIN" refs this wire lands on. */
  pins: string[];
  pts: [Um, Um][];
}

export interface SchematicLabel {
  /** Stable id (`lbl_xxxxxxxxxxxx`) -- for `delete_label`. */
  id: string;
  net: string;
  at: [Um, Um];
  scope: LabelScope;
  shape: LabelShape | null;
}

/** `T`: free-standing text -- `crates/model/src/ir.rs`'s `SchematicText`, deliberately minimal next to a PCB `BoardText` (no layer/justify/mirror -- a schematic has none of those concepts). */
export interface SchematicText {
  /** Stable id (`txt_xxxxxxxxxxxx`) -- for `delete_sch_text`. */
  id: string;
  content: string;
  at: [Um, Um];
  angle: Degrees;
  size_um: Um;
}

export interface TitleBlock {
  title: string;
  date: string;
  rev: string;
  company: string;
  comment1: string;
  comment2: string;
  comment3: string;
  comment4: string;
}

export interface Schematic {
  /** Empty object on a board with no schematic yet, never absent -- see api/client.ts's fetchSchematic for the defensive `?? {}` this file's other optional-till-populated collections already use. */
  lib_symbols: LibSymbols;
  symbols: SchematicSymbol[];
  power_symbols: PowerSymbol[];
  wires: SchematicWire[];
  no_connects: NoConnect[];
  labels: SchematicLabel[];
  texts: SchematicText[];
  title_block: TitleBlock | null;
}

// ---------------------------------------------------------------- Symbol library
//
// GET /api/symbol_library. Source of truth: crates/cli/src/studio.rs
// `symbol_library_json()`. `A`'s symbol chooser's own catalog -- "the
// libraries we already load" (every library name this project's intent
// already resolved a part against, or that's already on the sheet,
// scanned for its *full* contents), not a browse-every-installed-library
// search. Power symbols and the parametric `Connector_Generic:Conn_01x*`
// family are deliberately excluded -- see `symbol_library_json`'s own doc.

export interface SymbolLibraryEntry {
  lib_id: string;
  description: string;
  /** The library's own default `Reference` ("R", "C", "U", ...) -- "U" when unknown. Seeds `A`'s own next-free-number placement, same as a real reference designator always needs a letter prefix to start from. */
  reference_prefix: string;
}

export interface SymbolLibrary {
  entries: SymbolLibraryEntry[];
  /** Resolved graphics for every entry above, keyed by `lib_id` -- same shape `Schematic.lib_symbols` already uses, so the chooser's live preview reuses the exact same renderer the canvas itself does. */
  lib_symbols: LibSymbols;
}

// ---------------------------------------------------------------- DRC
//
// GET /api/drc. Source of truth: crates/cli/src/studio.rs `drc_json()`,
// crates/drc/src/item.rs `DrcViolation`/`DrcRefItem`/`FixHint`, shaped
// like kicad-cli's own `pcb drc --format json` report (`type`/
// `description`/`severity`/`items`) plus an extra `fix` key kicad-cli's
// own JSON has no concept of.

export type DrcSeverity = "error" | "warning";

export interface DrcItem {
  description: string;
  /** Board-space um, like every other position in this file -- *not* kicad-cli's own mm. */
  pos: [Um, Um];
  /** Stable id of the referenced item (track/via/zone id, or `<ref>.<pad>`/`<ref>` for a footprint/pad) -- enough to select it without re-matching on position. */
  id: string;
}

/** `crates/drc::FixHint` -- this workspace's own agent-repair metadata, no real-KiCad equivalent. Absent (not null -- `#[serde(skip_serializing_if = "Option::is_none")]`) when a violation has no computed fix; the `?? null` this file's other optional fields already use handles either. */
export interface DrcFix {
  /** Reference designator of the part a fix would move. */
  mover: string;
  /** What to move it toward: another part's reference, "board center", "nearest edge", or similar. */
  toward: string;
  distance_to_close_um: Um;
  /** Human-readable next step. */
  suggested_command: string;
}

export interface DrcViolation {
  /** `crates/drc::ErrorType`'s own snake_case name ("clearance", "courtyards_overlap", ...) -- real KiCad's own DRC type names, per `kicad-cli pcb drc`'s report. */
  type: string;
  description: string;
  severity: DrcSeverity;
  items: DrcItem[];
  fix?: DrcFix | null;
}

export interface DrcReport {
  violations: DrcViolation[];
  /** Violation count by `type`. */
  counts: Record<string, number>;
}

// ---------------------------------------------------------------- ERC
//
// GET /api/erc. Source of truth: crates/cli/src/studio.rs `erc_json()`,
// crates/kicad/src/erc.rs `check_erc`. Flatter than `DrcReport`: ERC
// reports plain `CheckResult`s, not DRC's richer positioned-item list --
// `location` is a "REF" or "REF.PIN" string (an id into `Schematic.
// symbols`/pin, not a sheet coordinate), which `ErcDialog.tsx` resolves
// back to something selectable/panable itself rather than reading a
// ready-made position the way DRC's `DrcItem.pos` gives one.

/** `"excluded"` is `dialog_erc.cpp`'s "Exclude this violation" (right-click a marker), persisted server-side in `design.schematic.erc_exclusions` and applied by `check_erc_excluding` -- a finding stays in `violations[]` (so `ErcDialog.tsx` can still show and un-exclude it) rather than disappearing the way an unexcluded Pass does. */
export type ErcSeverity = "error" | "warning" | "excluded";

export interface ErcViolation {
  /** `eda_kicad::erc`'s own check name ("pin_not_connected", "wire_dangling", ...) -- real KiCad's own ERC type names, per `kicad-cli sch erc`'s report. */
  check: string;
  severity: ErcSeverity;
  /**
   * An id into the schematic, in one of several shapes depending on which
   * check produced it (`crates/kicad/src/erc.rs`, every `location: Some(format!(...))`
   * call read directly) -- never a ready-made point the way DRC's `DrcItem.pos`
   * is. `ercMarkerPosition` (components/schematic/ercMarkerPosition.ts) is the
   * one place this app resolves any of these back to a canvas position:
   *   "REF.PIN"      -- pin_not_connected (PIN is a pin *number*, not name).
   *   "NET:REF.PIN"  -- pin_to_pin / pin_not_driven / power_pin_not_driven.
   *   "NET:ID"       -- same two checks, when the flagged net member is a
   *                     power symbol: ID is its own `PowerSymbol.id`, not
   *                     a part ref (no dot).
   *   "x,y"          -- no_connect_connected / no_connect_dangling: a
   *                     literal point, board-space um.
   *   "NET:x,y"      -- unconnected_wire_endpoint: same literal point,
   *                     net name prefix ignored.
   *   a bare net name -- wire_dangling: no symbol/point in the string at
   *                     all.
   * `null` on a (currently theoretical) location-less finding.
   */
  location: string | null;
  hint: string | null;
}

export interface ErcReport {
  violations: ErcViolation[];
  /** Violation count by `check`. */
  counts: Record<string, number>;
}
