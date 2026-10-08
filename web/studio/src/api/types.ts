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

import type { SchEditCmd, SchGraphic } from "./schEditTypes";

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
  /**
   * The part's body, board-space µm: the box of its footprint's `F.Fab` graphics (crates/cli/src/body_api.rs). The 3D tab sizes its placeholder boxes from this
   * (kicad-port/partBody.ts) -- the courtyard is the body plus a clearance margin plus the pads' reach. `null` when no `F.Fab` is known for the footprint.
   */
  body?: CourtyardBox | null;
  pads?: Pad[];
}

export interface Track {
  id: string;
  net: string;
  layer: string;
  width: Um;
  pts: [Um, Um][];
  /** The arc's mid point when this track is a KiCad arc (`PCB_ARC`; `pts` is then its tessellation), else null/absent. */
  arc_mid?: [Um, Um] | null;
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

/** `crates/ops/src/lib.rs` `SizeSpec`/`ViaSizeSpec`, `#[serde(tag = "kind")]` -- `edit_tracks_and_vias`'s track-width/via-size fields: either resolve from the item's own net class (`BoardRules::width_of`/`via_diameter_of`/`via_drill_of`), or an explicit value. */
export type SizeSpec = { kind: "net_class" } | { kind: "value"; um: Um };
export type ViaSizeSpec = { kind: "net_class" } | { kind: "value"; diameter: Um; drill: Um };

/**
 * `crates/ops/src/lib.rs` `ArrayGeometry` (task item 6), `#[serde(tag =
 * "kind")]` -- `create_array`'s geometry. Angles are millidegrees in the
 * backend's own `rotate_point_about` convention: positive = clockwise in
 * this app's Y-down board coordinates. `circular.clockwise` is the
 * user-facing direction choice (matching KiCad's own dialog radio
 * button, clockwise default) and needs no sign flip the way `move_exact`'s
 * single signed rotation field does -- pass the typed angle magnitude as
 * entered.
 */
export type ArrayGeometry =
  | { kind: "grid"; nx: number; ny: number; dx: Um; dy: Um; offset_x?: Um; offset_y?: Um; centred?: boolean; stagger?: number; stagger_rows?: boolean; horizontal_then_vertical?: boolean }
  | { kind: "circular"; center: PointXY; count: number; angle_millideg: number; angle_offset_millideg?: number; clockwise?: boolean; rotate_items?: boolean };

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
 * `crates/model/src/ir.rs` `Zone`'s rule-area (keepout) fields, task item
 * 3 -- `ZONE::GetIsRuleArea()` plus its five `DoNotAllow*` flags. Shares a
 * zone with `ZoneSettingsFields` the same way source's one dialog just
 * swaps panels on `IsRuleArea()`; see `ZoneDialog.tsx`.
 */
export interface RuleAreaFields {
  is_rule_area: boolean;
  keepout_tracks: boolean;
  keepout_vias: boolean;
  keepout_pads: boolean;
  keepout_copper_pour: boolean;
  keepout_footprints: boolean;
}

/**
 * `crates/model/src/ir.rs` `Zone` -- id/net/layer/outline plus the full
 * `ZONE_SETTINGS` (`ZoneSettingsFields`) and rule-area flags
 * (`RuleAreaFields`), ported into the IR so `crates/zone-filler` can read
 * them; see `ZoneDialog.tsx`. Every settings field has a KiCad-matching
 * server-side default (`Zone::default()`) once `add_zone` creates a zone,
 * so these are never actually absent from a real `/api/state` response --
 * not marked optional, same convention this file uses for every other
 * always-present field.
 */
export interface Zone extends ZoneSettingsFields, RuleAreaFields {
  id: string;
  net: string;
  layer: string;
  /** Task item 4: true for a generated teardrop (`eda_model::ir::Zone::teardrop`), never a hand-drawn zone. */
  teardrop: boolean;
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
  /** Task item 4: Board Setup > Teardrops. */
  teardrop_settings: TeardropSettings;
}

/** `crates/model/src/ir.rs` `TeardropSettings` -- `pcbnew/teardrop/teardrop_parameters.h`'s `TEARDROP_PARAMETERS`/`TEARDROP_PARAMETERS_LIST`, collapsed into one shared settings block (round anchors only -- see `eda_connectivity::teardrop`'s own doc for the full scope). */
export interface TeardropSettings {
  enabled: boolean;
  target_vias: boolean;
  target_pth_pads: boolean;
  target_smd_pads: boolean;
  best_length_ratio: number;
  best_width_ratio: number;
  max_len_um: Um;
  max_width_um: Um;
  width_to_size_filter_ratio: number;
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

export type ShapeKind = "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier";

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
  | (ShapeCommon & { kind: "polygon"; pts: [Um, Um][] })
  /** `PCB_SHAPE` BEZIER: a cubic from `start` to `end` with control points `c1`/`c2` (KiCad's `gr_curve` order). The canvas flattens it with kicad-port/bezierPoly.ts (`BEZIER_POLY::GetPoly`). */
  | (ShapeCommon & { kind: "bezier"; start: [Um, Um]; c1: [Um, Um]; c2: [Um, Um]; end: [Um, Um] });

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
  /** Task item 5 -- `crates/model/src/ir.rs` `Group`. Lives here purely for the lowest construction-site ripple (see that struct's own doc); a group can reference any item kind, not just a drawing. */
  groups: Group[];
  /** Task item 7 -- see `Dimension`'s own doc. */
  dimensions: Dimension[];
  dimension_settings: DimensionSettings;
}

/** `crates/model/src/ir.rs` `Group` (task item 5) -- `PCB_GROUP`: a named set of member item ids (a part reference, or a track/via/zone/shape/text id), no geometry of its own. No nested groups in this model. */
export interface Group {
  id: string;
  name: string;
  member_ids: string[];
}

/** `crates/model/src/ir.rs` `DimensionUnits`/`DimensionUnitsFormat`/`DimensionTextPosition`/`ArrowDirection` (task item 7). */
export type DimensionUnits = "mm" | "mil" | "inch" | "automatic";
export type DimensionUnitsFormat = "no_suffix" | "bare_suffix" | "paren_suffix";
export type DimensionTextPosition = "outside" | "inline";
export type ArrowDirection = "inward" | "outward";

/**
 * `crates/cli/src/studio.rs` `dimension_json` -- `state()`'s own flat
 * display shape for a `crates/model/src/ir.rs` `Dimension` (task item 7):
 * `[x,y]`-pairs and a flat `kind` string plus only the fields that kind
 * actually has (`height`/`horizontal`/`leader_length`, `null` otherwise),
 * not the nested tagged shape `CmdDimension` below sends/receives. The
 * geometry fields (`lines`/`text_at`/`computed_text_angle`/
 * `measured_value_um`/`text`) are computed once, server-side
 * (`eda_connectivity::dimension::compute_dimension_geometry`) -- a
 * renderer draws exactly these, never re-deriving crossbar/arrow/text
 * placement itself.
 */
export interface Dimension {
  id: string;
  layer: string;
  kind: "aligned" | "orthogonal" | "radial" | "leader" | "center";
  height: Um | null;
  horizontal: boolean | null;
  leader_length: Um | null;
  start: [Um, Um];
  end: [Um, Um];
  prefix: string;
  suffix: string;
  override_text: string | null;
  units: DimensionUnits;
  units_format: DimensionUnitsFormat;
  /** Decimal places, 0-5. */
  precision: number;
  suppress_trailing_zeros: boolean;
  text_position: DimensionTextPosition;
  keep_text_aligned: boolean;
  /** Millidegrees. Only meaningful when `keep_text_aligned` is false --
   * otherwise `computed_text_angle` below is what actually applies. */
  text_angle: number;
  text_size_um: Um;
  stroke_width: Um;
  arrow_length: Um;
  extension_offset: Um;
  extension_height: Um;
  arrow_direction: ArrowDirection;
  /** Every line segment to draw: extension lines, crossbar/leader pieces
   * (already split around inline text), arrow barbs/tails, a centre cross. */
  lines: [[Um, Um], [Um, Um]][];
  text_at: [Um, Um];
  computed_text_angle: number;
  measured_value_um: Um;
  /** Fully formatted (prefix + value + units suffix + suffix, or the
   * override text if set). Draw this verbatim -- do not reformat `measured_value_um` again. */
  text: string;
}

/** `crates/model/src/ir.rs` `DimensionSettings` (task item 7) -- Board Setup > Dimension Properties' defaults, applied once at creation (see that struct's own doc on why never retroactively). */
export interface DimensionSettings {
  units: DimensionUnits;
  units_format: DimensionUnitsFormat;
  precision: number;
  suppress_trailing_zeros: boolean;
  text_position: DimensionTextPosition;
  keep_text_aligned: boolean;
  text_size_um: Um;
  stroke_width: Um;
  arrow_length: Um;
  extension_offset: Um;
  extension_height: Um;
}

/** `CmdDimensionKind` -- `crates/model/src/ir.rs` `DimensionKind`'s own `#[serde(tag = "kind")]` shape, as `add_dimension`/`edit_dimension` need it (unlike `Dimension.kind` above, a flat string). */
export type CmdDimensionKind = { kind: "aligned"; height: Um } | { kind: "orthogonal"; height: Um; horizontal: boolean } | { kind: "radial"; leader_length: Um } | { kind: "leader" } | { kind: "center" };

/** `crates/model/src/ir.rs` `Dimension`, IR field names (`PointXY` objects, a nested `kind`) -- for `add_dimension`/`edit_dimension`'s own Cmd payload. See `Dimension` above for the display shape a renderer actually reads. */
export interface CmdDimension {
  id?: string;
  layer: string;
  kind: CmdDimensionKind;
  start: PointXY;
  end: PointXY;
  prefix: string;
  suffix: string;
  override_text?: string | null;
  units: DimensionUnits;
  units_format: DimensionUnitsFormat;
  precision: number;
  suppress_trailing_zeros: boolean;
  text_position: DimensionTextPosition;
  keep_text_aligned: boolean;
  text_angle: number;
  text_size_um: Um;
  stroke_width: Um;
  arrow_length: Um;
  extension_offset: Um;
  extension_height: Um;
  arrow_direction: ArrowDirection;
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
  /** The names that are entries of the project library itself (editable in place); the others come from the intent, a `.kicad_mod` or the builtin table. Absent from an older backend. */
  project?: string[];
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
  /** `BOARD_ITEM::IsLocked()` for every kind at once: a placed part's ref, or a track/via/zone/shape/text id (`DrawingsSection::locked_ids`, set by `set_locked`). Absent from an older backend = nothing locked. */
  locked?: string[];
  /** The drill/place file origin (`BOARD_DESIGN_SETTINGS::GetAuxOrigin`, `pcbnew.EditorControl.drillOrigin`), `[x, y]` µm; null/absent = (0, 0). */
  aux_origin?: [Um, Um] | null;
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
  | { kind: "polygon"; id?: string; layer: string; stroke_width: Um; filled: boolean; pts: PointXY[] }
  | { kind: "bezier"; id?: string; layer: string; stroke_width: Um; filled: boolean; start: PointXY; c1: PointXY; c2: PointXY; end: PointXY };

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
/** `crates/model/src/ir.rs` `Track` as `commit_route` takes it (ids are assigned server side). `arc_mid_offset` is the arc's mid point relative to `pts[0]`; `pts` must then be the arc's tessellation (`kicad-port/trackArc.ts`). */
export interface CmdRouteTrack {
  net: string;
  layer: string;
  width: Um;
  pts: PointXY[];
  pins?: string[];
  arc_mid_offset?: PointXY;
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
export interface CmdZone extends Partial<ZoneSettingsFields>, Partial<RuleAreaFields> {
  id?: string;
  net: string;
  layer: string;
  outline: PointXY[];
}

export type ZonePriorityMove = "top" | "raise" | "lower" | "bottom";

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
  /**
   * `dialog_global_edit_tracks_and_vias.cpp`'s "Apply and Close": bulk-set
   * width/via-size/layer on every named id in one atomic undo step. `ids`
   * is already filtered (net/net-class/layer/width/"selected only" are
   * this dialog's own client-side job -- see `GlobalEditTracksAndViasDialog.tsx`).
   * `track_width`/`via_size` omitted (or `null`) leaves that property
   * alone; this model has only one via "type" (no through/micro/blind/
   * buried distinction, no padstack/annular-ring concept).
   */
  | { op: "edit_tracks_and_vias"; ids: string[]; track_width?: SizeSpec | null; via_size?: ViaSizeSpec | null; layer?: string | null }
  /** Board Setup > Teardrops (task item 4): whole-struct replace. */
  | { op: "set_teardrop_settings"; settings: TeardropSettings }
  /** Regenerate the board's whole teardrop set from the current settings/tracks/vias/pads -- replaces, never appends to, this command's own previous output. */
  | { op: "add_all_teardrops" }
  /** Drop every generated teardrop zone; leaves `teardrop_settings.enabled` untouched. */
  | { op: "remove_all_teardrops" }
  /** Ctrl+G: create a new group from `ids` (2+ required). An id naming an existing group is flattened into the new one, not nested. */
  | { op: "group"; ids: string[] }
  /** Ctrl+Shift+G: dissolve every named group; an id not naming a group is silently skipped. */
  | { op: "ungroup"; ids: string[] }
  /** Add `ids` to an existing group, pulling each out of whatever group it was already in. */
  | { op: "add_to_group"; group_id: string; ids: string[] }
  /** Remove `ids` from whatever group each belongs to; a group left with fewer than 2 members dissolves. */
  | { op: "remove_from_group"; ids: string[] }
  /**
   * Ctrl+T (task item 6): `pcbnew.Array.createArray`. `arrange: false`
   * (the dialog's "Duplicate" default) creates `geometry`'s size minus
   * one new copies of each resolved track/via/zone/shape/text (never a
   * part or a group -- same scope `duplicate` already has); `arrange:
   * true` ("Arrange selection") repositions the given `ids` into the
   * array's own slots instead, creating nothing -- a placed part is
   * allowed there, since that only ever moves something that already
   * exists. See `ArrayGeometry`'s own doc for the angle-sign convention.
   */
  | { op: "create_array"; ids: string[]; geometry: ArrayGeometry; arrange?: boolean }
  /** Task item 7: `pcbnew/pcb_dimension.{h,cpp}`. `id` on `dimension`, if sent, is ignored. */
  | { op: "add_dimension"; dimension: CmdDimension }
  | { op: "delete_dimension"; id: string }
  /** Translate both feature points by `(dx, dy)` -- `PCB_DIMENSION_BASE::Move`. */
  | { op: "move_dimension"; id: string; dx: Um; dy: Um }
  /** Properties dialog's OK: replace every field at once. `id` on `dimension` is ignored. */
  | { op: "edit_dimension"; id: string; dimension: CmdDimension }
  /** Board Setup > Dimension Properties: applied to new dimensions from then on only. */
  | { op: "set_dimension_settings"; settings: DimensionSettings }
  /** `GLOBAL_EDIT_TOOL::SwapLayers` -- "move items on" -> "to layer" pairs (components/SwapLayersDialog.tsx). */
  | { op: "swap_layers"; mapping: [string, string][] }
  /** `Cmd::CommitRoute`: delete several tracks/vias (unknown ids tolerated) and add `tracks`/`vias` as ONE undo step -- what `unrouteSegment`/`deleteFull` (removal only) and the track edits (break, fillet, mirror) send. */
  | { op: "commit_route"; remove_track_ids: string[]; remove_via_ids: string[]; tracks?: CmdRouteTrack[]; vias?: CmdVia[] }
  /** `EDIT_TOOL::BooleanPolygons`: merge/subtract/intersect rectangles, circles and polygons (`ids` in routine order, the base first). */
  | { op: "boolean_shapes"; operation: "merge" | "subtract" | "intersect"; ids: string[] }
  /** `BOARD_EDITOR_CONTROL::modifyLockSelected` -- lock/unlock every id (part ref or track/via/zone/shape/text id). */
  | { op: "set_locked"; ids: string[]; locked: boolean }
  /** `EDIT_TOOL::Swap` -- cyclic pose shift across `parts` in selection order (position, rotation, side). */
  | { op: "swap_chain"; parts: string[] }
  /** `ZONE_CREATE_HELPER::performZoneCutout` -- subtract a closed polygon from one zone's outline. */
  | { op: "zone_cutout"; id: string; cutout: PointXY[] }
  /** `pcbnew.EditorControl.zoneMerge` -- the zones among `ids` (selection order) that touch the first one, merged into it. */
  | { op: "merge_zones"; ids: string[] }
  /** `pcbnew.EditorControl.zonePriority{MoveToTop,Raise,Lower,MoveToBottom}` -- `to` is top | raise | lower | bottom. */
  | { op: "set_zone_priority"; id: string; to: ZonePriorityMove }
  /** `pcbnew.EditorControl.drillOrigin` / `drillResetOrigin` -- the drill/place file origin; `null` resets it to (0, 0). */
  | { op: "set_aux_origin"; at: PointXY | null }
  /** `pcbnew.Control.repairBoard` -- refused ("No board problems found.") when there is nothing to repair; `POST /api/repair_board` wraps it with KiCad's report. */
  | { op: "repair_board" }
  | { op: "add_zone"; net: string; layer: string; outline: PointXY[] }
  | { op: "delete_zone"; id: string }
  /**
   * `dialog_copper_zones.cpp`'s "OK": replace a zone's net/layer and every
   * `ZONE_SETTINGS` field at once -- KiCad has no concept of editing just
   * one field of the panel, the whole thing commits together. Outline is
   * untouched (no point editor yet, see PARITY-pcb.md).
   */
  | ({ op: "edit_zone"; id: string; net: string; layer: string } & ZoneSettingsFields & RuleAreaFields)
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
  /**
   * `dialog_global_edit_text_and_graphics.cpp`'s "Apply and Close",
   * scoped to this model's two free-standing board drawing kinds (no
   * footprint reference/value fields, dimensions, tables or barcodes
   * exist as editable board items here -- see PARITY-pcb.md). Every
   * field omitted (or `null`) leaves that property alone; filtering
   * (item type/layer/"selected only") is `GlobalEditTextAndGraphicsDialog.tsx`'s
   * own client-side job.
   */
  | { op: "edit_text_and_graphics"; shape_ids?: string[]; text_ids?: string[]; layer?: string | null; line_width?: Um | null; text_size?: Um | null; text_thickness?: Um | null }
  /** Cmd+D: copy existing tracks/vias/zones/shapes/texts named by id, in place, with fresh ids. Never footprints -- see crates/ops/src/lib.rs `Cmd::Duplicate`'s own doc comment. */
  | { op: "duplicate"; ids: string[] }
  /** Cmd+V: insert fresh copies of whole items (ids ignored/reassigned) -- the clipboard's own full data, not references, so paste still works after the original was deleted. */
  | { op: "paste_items"; tracks?: CmdTrack[]; vias?: CmdVia[]; zones?: CmdZone[]; shapes?: CmdShape[]; texts?: CmdText[] }
  /** Shift+M "Move Exactly...": translate every named part by the same (dx, dy), then rotate each by the same `rotate_millideg` around `pivot` (null = each part's own anchor -- a pure spin). */
  | { op: "move_exact"; parts: string[]; dx: Um; dy: Um; rotate_millideg: number; pivot: PointXY | null }
  /** `Cmd::Batch`: the sub-commands as ONE undo step, all-or-nothing. */
  | { op: "batch"; cmds: Cmd[] }
  /** `Cmd::OnSheet`: run a schematic command on the sheet at `sheet` (the `/`-joined `SheetInstance::id`s from the root, what `GET /api/schematic?sheet=` takes) instead of the root. */
  | { op: "on_sheet"; sheet: string; cmd: Cmd }
  /** `Cmd::ReorganizeSheets` (Tools > Reorganize into Module Sheets): the flat schematic becomes one sheet per functional module under a root of sheet symbols. */
  | { op: "reorganize_sheets" }

  // -------------------------------------------------------- eeschema
  // crates/ops/src/lib.rs's eeschema `Cmd` variants -- see that enum's
  // own doc comment for what each hotkey/tool sends. `rot_millideg`
  // (not `rot`) on purpose, same convention `move_exact`'s own
  // `rotate_millideg` already set: a write-side angle is always named
  // for its unit, since the read side (`SchematicSymbol.rot`, `Degrees`)
  // uses plain degrees instead.
  // `unit`: which placed instance of `id` to act on, when a multi-unit
  // part has more than one on the sheet -- omit (undefined) when `id`
  // names exactly one instance (every single-unit part, the overwhelming
  // common case); the backend refuses as ambiguous if more than one
  // instance shares `id` and no `unit` was sent.
  | { op: "move_symbol"; id: string; x: Um; y: Um; unit?: number }
  | { op: "drag_symbol"; id: string; x: Um; y: Um; attached_wire_endpoints: [number, number][]; unit?: number }
  | { op: "rotate_symbol"; id: string; quarter_turns: number; unit?: number }
  | { op: "mirror_symbol"; id: string; unit?: number }
  | { op: "mirror_symbol_vertical"; id: string; unit?: number }
  | { op: "delete_symbol"; id: string; unit?: number }
  | { op: "add_wire"; pts: PointXY[]; bus?: boolean }
  | { op: "delete_wire"; id: string }
  | { op: "add_no_connect"; at: PointXY }
  | { op: "delete_no_connect"; id: string }
  | { op: "add_bus_entry"; at: PointXY; size: PointXY }
  | { op: "delete_bus_entry"; id: string }
  /** `J` (eeschema.InteractiveDrawing.placeJunction): an explicit junction -- the item that joins wires which merely cross. */
  | { op: "add_junction"; at: PointXY }
  | { op: "delete_junction"; id: string }
  /** `I` (eeschema.InteractiveDrawingLineWireBus.drawLines): a graphic polyline on the notes layer, never a wire. */
  | { op: "add_sch_line"; pts: PointXY[]; width_um?: Um }
  | { op: "delete_sch_line"; id: string }
  /** `S` (eeschema.InteractiveDrawing.drawSheet): a hierarchical sheet symbol; `file` is a bare `.kicad_sch` name (extension added when missing). */
  | { op: "add_sheet"; name: string; file: string; at: PointXY; size: [Um, Um] }
  /** Alt+S (eeschema.InteractiveEdit.swap): exchange the positions of two symbols/power symbols/labels/texts (and the orientation of two instances of one library symbol). */
  | { op: "swap_sch_items"; a: string; b: string }
  /** The schematic editor's other tool verbs (lock, break, convert text, shapes, sheet pins, ...) -- see api/schEditTypes.ts. */
  | ({ op: "sch_edit" } & SchEditCmd)
  | { op: "add_label"; net: string; at: PointXY; kind: CmdLabelKind }
  | { op: "delete_label"; id: string }
  | { op: "add_sch_text"; content: string; at: PointXY; angle_millideg: number; size_um: Um }
  | { op: "delete_sch_text"; id: string }
  | { op: "add_power_symbol"; lib_id: string; at: PointXY; rot_millideg: number; net: string; pin: string }
  | { op: "delete_power_symbol"; id: string }
  // `unit`: which unit of a multi-unit symbol this placement is (omit for
  // 1, a single-unit part). Only refused if `(id, unit)` already exists --
  // placing `{ id: "U1", unit: 2 }` once "U1" unit 1 is already on the
  // sheet is how another unit of an existing, already-annotated part gets
  // added.
  | { op: "add_symbol"; id: string; lib_id: string; at: PointXY; rot_millideg: number; value: string; footprint: string; unit?: number }
  | { op: "edit_symbol_fields"; id: string; value?: string | null; footprint?: string | null; datasheet?: string | null }
  | { op: "rename_symbol"; id: string; new_id: string }
  | { op: "annotate"; reset_existing: boolean; order?: "y_then_x" | "x_then_y"; ids?: string[] }
  /** `eeschema.EditorControl.setDNP` / `setExcludeFromBOM` / `setExcludeFromBoard` / `setExcludeFromSimulation` (`SCH_EDIT_TOOL::SetAttribute`): the attributes given are set on every unit of every reference in `ids`; one left out stays. */
  | { op: "set_symbol_attrs"; ids: string[]; dnp?: boolean | null; exclude_from_bom?: boolean | null; exclude_from_board?: boolean | null; exclude_from_sim?: boolean | null }
  /** `eeschema.EditorControl.editPageNumber`: the page number of the sheet placement `sheet` (its id); letters and digits, empty for "by place in the hierarchy". */
  | { op: "set_sheet_page"; sheet: string; page: string }
  /** `eeschema.Interactive.increment*`: the new text of a label (its net name) or a free text, by id. */
  | { op: "set_sch_item_text"; id: string; text: string }
  /** `eeschema.EditorControl.editSymbolLibraryLinks`: every symbol linked to a `from` library id is linked to the paired `to` id (`update_fields`: the Datasheet follows the library symbol). */
  | { op: "set_symbol_lib_ids"; changes: Array<[string, string]>; update_fields?: boolean }
  /** `eeschema.EditorControl.incrementAnnotations`: every reference with the start's letters and a number at least its own moves by `increment`. */
  | { op: "increment_annotations"; start: string; increment: number }
  /** `dialog_erc.cpp`'s "Exclude this violation" / un-exclude -- `(check, location)` keys exactly one `ErcViolation`, matching it byte-for-byte against the same `location` string GET /api/erc reported (see `ErcViolation.location`'s own doc for the shapes that can be). Refused server-side when `location` is empty -- nothing to key an exclusion on. */
  | { op: "add_erc_exclusion"; check: string; location: string }
  | { op: "delete_erc_exclusion"; check: string; location: string }
  /** Symbol Fields Table's Apply (dialog_symbol_fields_table.cpp) -- ONE verb for the whole staged batch so a single undo reverts it all. Applied remove -> rename -> add -> edits; edits address the FINAL field names. */
  | { op: "set_symbol_fields"; edits: SymbolFieldEdit[]; add_fields?: string[]; rename_fields?: SymbolFieldRename[]; remove_fields?: string[] }
  /** Find and Replace's Replace / Replace All (sch_find_replace_tool.cpp): `items` = `FindMatch.key`s to restrict to (one = "Replace"), omitted/null = "Replace All". */
  | { op: "replace_text"; search: SchSearchData; items?: string[] | null }
  /** panel_setup_pinmap.cpp changeErrorLevel: one pin-map cell + its mirror (type indexes 0..11, level 0 OK / 1 warning / 2 error). */
  | { op: "set_erc_pin_map_cell"; a: number; b: number; level: number }
  /** `ERC_SETTINGS::ResetPinMap` ("Reset to Defaults"). */
  | { op: "reset_erc_pin_map" }

  // -------------------------------------------------- footprint editor
  //
  // GAPS.md #8. crates/ops/src/lib.rs's own "footprint editor" Cmd
  // section, same order. `Domain::FootprintEditor` (api/client.ts's
  // `postUndo`/`postRedo` `domain` param) -- its own undo/redo scope,
  // independent of "pcb"/"schematic".
  | { op: "open_footprint_for_edit"; name: string }
  /** `pcbnew.ModuleEditor.newFootprint`: a fresh SMD footprint; refused when `name` already exists (the caller picks a unique one). */
  | { op: "new_footprint"; name: string }
  /** `eeschema.SymbolLibraryControl.newSymbol`: a new, empty symbol in the project library (refused when the lib_id is taken). */
  | { op: "new_symbol"; lib_id: string }
  | { op: "delete_library_footprint"; name: string }
  /** `FOOTPRINT_EDITOR_CONTROL` Duplicate / Paste / Import / Save As: store a whole footprint under `footprint.name` (refused when taken unless `overwrite`). */
  | { op: "put_library_footprint"; footprint: LibraryFootprint; overwrite?: boolean }
  /** `FOOTPRINT_EDITOR_CONTROL::RenameFootprint`. */
  | { op: "rename_library_footprint"; name: string; new_name: string; overwrite?: boolean }
  /** `FOOTPRINT_EDITOR_CONTROL::RepairFootprint`: repeated item ids get fresh ones. */
  | { op: "repair_footprint"; name: string }
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
  /** `PAD_TOOL::EnumeratePads`' commit: `[pad id, new number]` pairs, applied together. */
  | { op: "set_pad_numbers"; footprint: string; numbers: [string, string][] }
  | { op: "add_footprint_graphic"; footprint: string; shape: CmdShape }
  | { op: "delete_footprint_graphic"; footprint: string; id: string }
  | { op: "move_footprint_graphic"; footprint: string; id: string; dx: Um; dy: Um }
  | { op: "edit_footprint_graphic"; footprint: string; id: string; layer: string; stroke_width: Um; filled: boolean }
  | { op: "add_footprint_text"; footprint: string; text: CmdText }
  | { op: "edit_footprint_text"; footprint: string; id: string; content: string; angle: number; layer: string; size_um: Um; stroke_width: Um; justify: TextJustify; mirror: boolean }
  | { op: "delete_footprint_text"; footprint: string; id: string }
  | { op: "move_footprint_text"; footprint: string; id: string; x: Um; y: Um }

  // ----------------------------------------------------------- symbol editor
  //
  // The Symbol Editor tab. crates/ops/src/lib.rs's own "symbol editor" Cmd
  // section, same order. `Domain::SymbolEditor` (api/client.ts's
  // `postUndo`/`postRedo` `domain` param) -- its own undo/redo scope.
  | { op: "open_symbol_for_edit"; lib_id: string }
  | { op: "delete_library_symbol"; lib_id: string }
  /** `SYMBOL_EDIT_FRAME` Duplicate / Paste / Import / Save Copy As: store a whole symbol under `symbol.lib_id` (refused when taken unless `overwrite`). */
  | { op: "put_library_symbol"; symbol: LibrarySymbol; overwrite?: boolean }
  /** `SYMBOL_EDITOR_CONTROL::RenameSymbol`. */
  | { op: "rename_library_symbol"; lib_id: string; new_lib_id: string; overwrite?: boolean }
  /** `SYMBOL_EDITOR_DRAWING_TOOLS::PlaceAnchor`: `at` (mm, symbol frame, Y up) becomes the new origin. */
  | { op: "set_symbol_anchor"; lib_id: string; at: { x: Mm; y: Mm } }
  | ({ op: "edit_symbol_properties"; lib_id: string } & SymbolPropertiesFields)
  | { op: "update_symbol_on_board"; lib_id: string }
  | { op: "add_symbol_pin"; lib_id: string; pin: LibrarySymbolPin }
  | { op: "move_symbol_pin"; lib_id: string; id: string; x: Mm; y: Mm }
  | { op: "delete_symbol_pin"; lib_id: string; id: string }
  | { op: "edit_symbol_pin"; lib_id: string; id: string; pin: LibrarySymbolPin }
  /** `body_style` is the editor's shown body style: a length push reaches pins shared by every style (0) or of that one. */
  | { op: "push_pin_property"; lib_id: string; source_pin_id: string; field: PushPinField; body_style?: number | null }
  | { op: "add_symbol_graphic"; lib_id: string; graphic: LibrarySymbolGraphic }
  | { op: "delete_symbol_graphic"; lib_id: string; id: string }
  | { op: "move_symbol_graphic"; lib_id: string; id: string; dx_mm: Mm; dy_mm: Mm }
  | { op: "edit_symbol_graphic"; lib_id: string; id: string; stroke_mm: Mm; fill: LibraryFill }
  | { op: "edit_symbol_text"; lib_id: string; id: string; text: string; angle_deg: Degrees; size_mm: Mm };

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
  /** Which tuner answered: `7` single track, `8` differential pair, `9` skew. */
  mode?: TuneMode;
  net?: string;
  layer?: string;
  width?: Um;
  /** Single: the source track's own straight-line length, before tuning. Pair (`8`): the pair's length, the longer net's whole routed length. Skew (`9`): the selected net's whole routed length. Also present on a pair/skew *error* reply, so the dialog can offer a sensible default target. */
  original_length?: Um;
  /** The generated meander's real, measured length -- usually within a
   * few um of the requested target when reachable, see that module's own
   * doc comment on why it isn't always exact to the micrometer. For a pair
   * (`8`) it is the pair's length after tuning; for skew (`9`) the selected net's. */
  achieved_length?: Um;
  pts?: [Um, Um][];
  colliding?: boolean;
  /** `8`/`9`: the complementary net (`BOARD::MatchDpSuffix`). */
  partner_net?: string;
  /** `8`: the partner line's own replacement points, and the two lines' centre-to-centre distance (`gap + width`). */
  partner_pts?: [Um, Um][];
  pitch?: Um;
  /** `9`: the partner net's whole routed length (`m_coupledLength`). */
  partner_length?: Um;
  /** `8`/`9`: this net minus the partner, before and after tuning (`CurrentSkew`). */
  skew_before?: Um;
  skew_after?: Um;
}

/** `7` / `8` / `9`: which `pcbnew.LengthTuner.*` the dialog is running. */
export type TuneMode = "single" | "diffpair" | "skew";

/** `dialog_cleanup_tracks_and_vias_base.cpp`'s checkboxes -- see
 * `POST /api/cleanup_tracks/{preview,apply}` and
 * `components/CleanupTracksDialog.tsx`. Every field defaults `false`,
 * matching the base dialog's own ctor (no checkbox starts checked). */
export interface CleanupOptions {
  delete_shorting: boolean;
  delete_redundant_vias: boolean;
  delete_dangling_vias: boolean;
  merge_segments: boolean;
  delete_dangling_tracks: boolean;
  delete_tracks_in_pads: boolean;
}

/** `crates/connectivity/src/cleanup.rs`'s `CleanupKind`, wire names. */
export type CleanupChangeKind = "redundant_via" | "zero_length_track" | "duplicate_track" | "shorting_track" | "shorting_via" | "track_in_pad" | "dangling_track" | "dangling_via" | "merged_tracks";

export interface CleanupChange {
  kind: CleanupChangeKind;
  /** Human-readable label (`CleanupKind::label()`), e.g. "dangling track". */
  label: string;
  net: string;
  remove_track_ids: string[];
  remove_via_ids: string[];
}

/** `POST /api/cleanup_tracks/{preview,apply}` (task item 1): "Cleanup
 * Tracks & Vias..." -- stateless, same shape as `TuneLengthReply`. `apply`
 * recomputes against the live board rather than trusting the client's own
 * cached preview. */
export interface CleanupReply {
  ok: boolean;
  message?: string;
  changes?: CleanupChange[];
  tracks_removed?: number;
  vias_removed?: number;
  tracks_added?: number;
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
  /** The items the two ends join (a pad is `REF.NUMBER`, anything else its own id), and the copper layers each spans (inclusive indexes into `BoardState.layers`) -- what the Local Ratsnest tool and the visible-layers ratsnest mode read, as `RATSNEST_VIEW_ITEM::ViewDraw` does. Absent from an older backend. */
  from_id?: string;
  to_id?: string;
  from_layers?: [number, number];
  to_layers?: [number, number];
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
  /** Only with `GET /api/fill?polys=1`: the same fill unfractured, one outline and its holes per island (what the triangulation display needs). */
  polys?: { outline: [Um, Um][]; holes: [Um, Um][][] }[];
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

/** One field of a symbol, power symbol or sheet as `GET /api/schematic` sends it: the text and where KiCad draws it on the sheet (`eda_engine::fields::PageField`). */
export interface SchField {
  /** `Reference`, `Value`, `Footprint`, `Datasheet` (the last two hidden unless shown); a sheet's `Sheetname`, `Sheetfile`. */
  name: string;
  text: string;
  /** The text's anchor on the sheet, micrometres. */
  at: [Um, Um];
  /** The text runs upward: a quarter turn counter-clockwise about the anchor. */
  vertical: boolean;
  /** How the text is justified against the anchor, in its own axes. */
  h: "left" | "center" | "right";
  v: "top" | "center" | "bottom";
  visible: boolean;
}

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
  /** `SCH_SYMBOL::GetDNP` (Do not Populate): the symbol is drawn with a cross over it and left out of the assembly. */
  dnp?: boolean;
  /** Exclude from Bill of Materials / from Board / from Simulation (`SetAttribute`). Absent from a backend built before they existed. */
  exclude_from_bom?: boolean;
  exclude_from_board?: boolean;
  exclude_from_sim?: boolean;
  /** Where its Reference, Value, ... are drawn (absent from a backend built before fields had positions: painter.ts places them by its own rule then). */
  fields?: SchField[];
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
  /** The Value (its net name) where KiCad draws it, and the hidden Reference. */
  fields?: SchField[];
}

export interface NoConnect {
  /** Stable id (`nc_xxxxxxxxxxxx`) -- for `delete_no_connect`. */
  id: string;
  at: [Um, Um];
}

/** An explicit junction (`SCH_JUNCTION`, the `J` tool): joins every wire passing through or ending at `at`. */
export interface SchJunction {
  id: string;
  at: [Um, Um];
}

/** A graphic polyline on the schematic's notes layer (`SCH_LINE` on `LAYER_NOTES`, the `I` tool): decoration, never part of a net. `width_um` 0 is the default line width. */
export interface SchLine {
  id: string;
  pts: [Um, Um][];
  width_um: Um;
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
  /** True for a bus wire (GAPS.md #20) -- KiCad's `LAYER_BUS` vs `LAYER_WIRE`, same shape either way. */
  bus: boolean;
  /** A bus's member nets (`D[0..3]` -> D0..D3, aliases and groups expanded) -- only sent for a bus wire; what the Unfold from Bus menu lists. */
  members?: string[];
}

/** A bus entry (`SCH_BUS_WIRE_ENTRY`, GAPS.md #20): a short diagonal stub tying one specific member net into a bus. `at` and `at + size` are its two endpoints -- which one is "the bus side" is never stored, only read off whichever endpoint lands on a bus wire. */
export interface BusEntry {
  id: string;
  at: [Um, Um];
  size: [Um, Um];
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

/** A hierarchical sheet pin (`SCH_SHEET_PIN`) on a placed sheet's own border -- GAPS.md #6. Tied *by name only* to a hierarchical label of the same name inside the sheet's own file (see crates/kicad/src/erc.rs's `check_hierarchy` doc for why shape is never compared). */
export interface SheetPin {
  id: string;
  name: string;
  shape: LabelShape;
  at: [Um, Um];
}

/** One child sheet placed directly on the schematic view currently being shown -- `GET /api/schematic`'s own `sheets` field, not the whole project's tree (see `sheet_path` for how deep the current view already is). */
export interface Sheet {
  id: string;
  name: string;
  file: string;
  /** The page number the user gave this placement (Edit Sheet Page Number); empty or absent when it is numbered by its place in the hierarchy. */
  page?: string;
  at: [Um, Um];
  size: [Um, Um];
  pins: SheetPin[];
  /** The sheet's name and file where KiCad's Autoplace Fields puts them. */
  fields?: SchField[];
}

/** One step of the breadcrumb from the root down to the sheet `GET /api/schematic?sheet=...` actually returned -- empty for the root itself. */
export interface SheetPathEntry {
  id: string;
  name: string;
}

/** The paper a sheet is drawn on (KiCad's names, landscape, micrometres) -- A4 unless the sheet's title block says otherwise. */
export interface SchematicPaper {
  name: string;
  width_um: Um;
  height_um: Um;
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
  /** Bus entries (GAPS.md #20) -- see `BusEntry`'s own doc. */
  bus_entries: BusEntry[];
  /** Explicit junctions (`J`) -- see `SchJunction`. Absent from a backend built before they existed. */
  junctions?: SchJunction[];
  /** Graphic lines on the notes layer (`I`) -- see `SchLine`. Absent from a backend built before they existed. */
  lines?: SchLine[];
  /** Drawn shapes, text boxes, rule areas and directive labels -- see `SchGraphic` (api/schEditTypes.ts). Absent from a backend built before they existed. */
  graphics?: SchGraphic[];
  /** Ids of locked items (Lock / Unlock). Absent from a backend built before locks existed. */
  locked?: string[];
  /** Child sheets placed directly on *this* view (GAPS.md #6) -- empty for a single-sheet design, or for a sheet with no children of its own. */
  sheets: Sheet[];
  /** The root-to-here breadcrumb for whichever sheet this response is actually showing (see `fetchSchematic`'s own `sheetPath` param) -- empty when showing the root. */
  sheet_path: SheetPathEntry[];
  /** The paper this sheet is drawn on; absent from a backend built before sheets chose their own. */
  paper?: SchematicPaper;
  /** The file of the screen shown (empty for the root sheet): the title block names it. The design's whole tree is `fetchHierarchy` (`GET /api/sch/hierarchy`). */
  file?: string;
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
  /** How many units this symbol declares (1 for a single-unit part). >1 means `SymbolChooserDialog` offers a unit picker before placing. */
  unit_count: number;
}

export interface SymbolLibrary {
  entries: SymbolLibraryEntry[];
  /** Resolved graphics for every entry above, keyed by `lib_id` -- same shape `Schematic.lib_symbols` already uses, so the chooser's live preview reuses the exact same renderer the canvas itself does. */
  lib_symbols: LibSymbols;
}

// ----------------------------------------------------------- Symbol Editor
//
// The Symbol Editor tab. GET /api/symbol?lib_id=... returns
// `crates/model/src/ir.rs` `LibrarySymbol` serialized exactly as
// `design.symbol_library` stores it -- the same "no second hand-built
// shape" contract the Footprint Editor's own types above already follow
// (see `LibraryFootprint`'s own doc). Coordinates use this file's own `Mm`
// alias (a library symbol's own native unit, +y **up** -- KiCad's library
// convention, not this API's usual +y-down sheet/board millimetres) and a
// plain `{x,y}` point: NOT the `[Mm,Mm]` tuple `LibPin`/`LibGraphic` above
// use, since those two exist only for the *read-only*, placed-instance
// rendering path (`lib_symbol_json`'s own hand-built `pt()` helper) --
// this editor's data instead goes straight through serde's default struct
// encoding, exactly like `LibraryPad.at`'s own `PointXY` does for the
// Footprint Editor.

/** `crates/model/src/symbol.rs` `SPoint`, as this editor's own types see it (object form -- see this section's own intro on why). */
export interface MmPoint {
  x: Mm;
  y: Mm;
}

/** `crates/model/src/ir.rs` `LibraryFill` -- the Shape Properties dialog's own three-way fill choice (KiCad's `color`/hatch modes are real file values but not authorable here, same simplification `crate::symbol::SymbolGraphic`'s single `filled: bool` already made for every other reader of a *resolved* symbol). */
export type LibraryFill = "none" | "outline" | "background";

/**
 * `crates/model/src/ir.rs` `LibrarySymbolGraphic` -- one drawn primitive of
 * a [`LibrarySymbol`], in its own local mm frame. `id` is optional only
 * for a graphic not yet sent through `add_symbol_graphic` (the backend
 * always assigns/keeps the real one). `unit`/`body_style`: `0` means
 * shared by every unit/style, same convention `LibGraphic` above already
 * documents for the *resolved* type.
 */
export type LibrarySymbolGraphic =
  | { kind: "rectangle"; id?: string; unit: number; body_style: number; start: MmPoint; end: MmPoint; stroke_mm: Mm; fill: LibraryFill }
  | { kind: "polyline"; id?: string; unit: number; body_style: number; pts: MmPoint[]; stroke_mm: Mm; fill: LibraryFill }
  | { kind: "circle"; id?: string; unit: number; body_style: number; center: MmPoint; radius_mm: Mm; stroke_mm: Mm; fill: LibraryFill }
  | { kind: "arc"; id?: string; unit: number; body_style: number; start: MmPoint; mid: MmPoint; end: MmPoint; stroke_mm: Mm; fill: LibraryFill }
  | { kind: "text"; id?: string; unit: number; body_style: number; text: string; at: MmPoint; angle_deg: Degrees; size_mm: Mm };

/** `crates/model/src/ir.rs` `LibrarySymbolPin` -- one pin on a [`LibrarySymbol`], addressable (unlike the read-only `LibPin` above) for move/edit/delete. */
export interface LibrarySymbolPin {
  id?: string;
  number: string;
  name: string;
  electrical_type: PinElectricalType;
  shape: PinShape;
  at: MmPoint;
  angle_deg: Degrees;
  length_mm: Mm;
  unit: number;
  body_style: number;
  /** `(hide yes)` -- a hidden pin (almost every power-symbol pin in a real library) draws no line or decoration, only its net-effect on connectivity. */
  hidden: boolean;
  name_size_mm: Mm | null;
  number_size_mm: Mm | null;
}

/** The fields `edit_symbol_properties` commits all at once (`dialog_lib_symbol_properties.cpp`'s General + Units&&Body Styles tabs) -- factored out so the `Cmd` variant and the Symbol Properties dialog's own form state share one field list, same pattern `FootprintPropertiesFields` already sets. */
export interface SymbolPropertiesFields {
  reference_prefix: string;
  description: string;
  keywords: string;
  datasheet: string;
  power: boolean;
  in_bom: boolean;
  on_board: boolean;
  pin_numbers_hidden: boolean;
  pin_names_hidden: boolean;
  pin_name_offset_mm: Mm;
  unit_count: number;
  has_alternate_body_style: boolean;
  footprint_filters: string[];
}

/**
 * `crates/model/src/ir.rs` `LibrarySymbol` -- the Symbol Editor's own open
 * document. `published` mirrors `LibraryFootprint.published`'s own doc:
 * whether a placed schematic symbol/power-symbol instance naming this
 * `lib_id` currently resolves its graphics/pins from here.
 */
export interface LibrarySymbol extends SymbolPropertiesFields {
  lib_id: string;
  graphics: LibrarySymbolGraphic[];
  pins: LibrarySymbolPin[];
  published: boolean;
}

/** `GET /api/symbol_editor/names`'s one field: every `lib_id` available to open (already-opened library entries, every lib_id the project's intent already resolved, and the small builtin catalog), sorted. */
export interface SymbolEditorNames {
  names: string[];
  /** The `lib_id`s that are entries of the project library itself (editable in place). Absent from an older backend. */
  project?: string[];
}

/** `symbol_editor_pin_tool.cpp`'s three "Push Pin ..." context-menu items, folded into one Cmd with a field selector -- see `Cmd`'s own `push_pin_property` doc. */
export type PushPinField = "length" | "name_size" | "number_size";

// ---------------------------------------------------------------- DRC
//
// GET /api/drc: `kicad-cli pcb drc`'s report on the current design
// (crates/cli/src/kicad_engine.rs `drc`, run through crates/kicad-engine),
// with each item's KiCad uuid pointed back at our own id. It takes seconds,
// so the studio runs it on demand and shows a running state; there is no
// other DRC engine. `type`/`description`/`severity`/`items` are
// kicad-cli's own JSON; positions are this API's µm integers (every other
// endpoint here uses board-space µm, not kicad-cli's mm), as `[x, y]`.
// Our own checks, the ones KiCad does not have, are `LintReport` below.

export type DrcSeverity = "error" | "warning";

export interface DrcItem {
  description: string;
  /** Board-space um, like every other position in this file -- *not* kicad-cli's own mm. */
  pos: [Um, Um];
  /** Our id for the referenced item (track/via/zone id, or `<ref>.<pad>`/`<ref>` for a footprint/pad; `outline` for the Edge.Cuts outline) -- enough to select it without re-matching on position. `null` for an item that isn't one of ours (e.g. a fill polygon KiCad computed). */
  id: string | null;
}

/** `crates/lint`'s agent-repair metadata for a placement finding: which part to move, toward what, how far. kicad-cli's report has no such thing, so only lint findings carry one. */
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
  /** KiCad's own DRC type name ("clearance", "courtyards_overlap", ...) for a kicad-cli violation; the check's own name ("placement_proximity", ...) for a lint finding. */
  type: string;
  description: string;
  severity: DrcSeverity;
  items: DrcItem[];
  /** Lint findings only. */
  fix?: DrcFix | null;
}

export interface DrcReport {
  violations: DrcViolation[];
  /** Violation count by `type`. */
  counts: Record<string, number>;
  /** "kicad-cli <version>". */
  engine?: string;
  /** The ratsnest kicad-cli reports as `unconnected_items`. */
  unconnected_items?: DrcViolation[];
  /** Whether kicad-cli refilled zones itself (`--refill-zones`). Off by default: kicad-cli 10.99 skips its courtyard checks on a run that refills, so it judges the fills the exported board already carries (the ones the studio shows). */
  zones_refilled_by_kicad?: boolean;
  /** The design revision this run started from: the stamp GET /api/version served at that moment. The report is out of date once the board's revision is another one (kicad-port/checkRevision.ts). Absent from a server that does not stamp. */
  revision?: string;
}

// ---------------------------------------------------------------- ERC
//
// GET /api/erc: `kicad-cli sch erc`'s report on the current schematic
// (crates/cli/src/kicad_engine.rs `erc`), flattened over every sheet, with
// each item's KiCad uuid pointed back at our own id. Run on demand, like DRC.
// `location` is our id for the first item the violation names -- a symbol
// ("REF"), a pin ("REF.PIN"), a power symbol, a wire, a label, a no-connect
// or a text -- never a ready-made point: `ercMarkerPosition`
// (components/schematic/ercMarkerPosition.ts) resolves it back to something
// selectable/panable, the one place this app does that. (kicad-cli's own
// ERC positions are not reliable units, so they are not used.)

/** `"excluded"` is `dialog_erc.cpp`'s "Exclude this violation" (right-click a marker), persisted server-side in `design.schematic.erc_exclusions` and applied by the backend to the report -- a finding stays in `violations[]` (so `ErcDialog.tsx` can still show and un-exclude it) rather than disappearing. */
export type ErcSeverity = "error" | "warning" | "excluded";

/** One item an ERC violation names, as kicad-cli describes it ("Symbol U1 Pin 2 [VOUT, Power output, Line]"). */
export interface ErcItem {
  description: string;
  /** Our id for it (see `ErcViolation.location`), or `null` when it is not one of ours. */
  id: string | null;
}

export interface ErcViolation {
  /** KiCad's own ERC type name ("pin_not_connected", "wire_dangling", ...) for a kicad-cli violation; the readability check's own name ("schematic_wire_overlap", ...) for a lint finding. */
  check: string;
  severity: ErcSeverity;
  /**
   * Our id for the first item the violation names (see the block comment
   * above). A lint finding's `location` keeps the shapes those checks have
   * always used ("REF.PIN", "NET:REF.PIN", "x,y", "NET@x,y", a bare net
   * name, ...), which `ercMarkerPosition` also resolves. `null` when
   * there is nothing to key on.
   */
  location: string | null;
  hint: string | null;
  /** kicad-cli violations only: every item it names. */
  items?: ErcItem[];
}

export interface ErcReport {
  violations: ErcViolation[];
  /** Violation count by `check` (excluded ones are not counted). */
  counts: Record<string, number>;
  /** "kicad-cli <version>". */
  engine?: string;
  /** The design revision this run started from -- see `DrcReport.revision`. */
  revision?: string;
}

// ---------------------------------------------------------------- Lint
//
// GET /api/lint: our own checks, the ones KiCad does not have (crates/lint):
// placement quality and net-class track width on the PCB, readability on the
// schematic. Never DRC, never ERC: clearance, courtyards, pin conflicts and
// the rest are kicad-cli's. In-process and cheap, so the studio refetches it
// on every board change while a DRC/ERC dialog is open.

export interface LintReport {
  /** Shaped like `DrcReport.violations` (`type` is the check name, `fix` is set on placement findings). */
  pcb: { violations: DrcViolation[]; counts: Record<string, number> };
  /** Shaped like `ErcReport.violations`. */
  schematic: { violations: ErcViolation[]; counts: Record<string, number> };
}

/**
 * `POST /api/board_stats` (crates/cli/src/kicad_engine.rs `board_stats`):
 * `kicad-cli pcb export stats`, in the shape components/
 * BoardStatisticsDialog.tsx reads. Lengths in µm, areas in µm². An unset
 * minimum is KiCad's own INT_MAX nm sentinel (2147483.647 µm), so it
 * displays the same.
 */
export interface BoardStatsOptions {
  exclude_footprints_without_pads: boolean;
  subtract_holes_from_board_area: boolean;
  subtract_holes_from_copper_areas: boolean;
}

export interface BoardStatsEntry {
  title: string;
  qty: number;
}

export interface BoardStatsDrill {
  qty: number;
  shape: "round" | "slot";
  x_um: number;
  y_um: number;
  plated: boolean;
  is_pad: boolean;
  start_layer: string | null;
  stop_layer: string | null;
}

export interface BoardStatsReply {
  ok: boolean;
  message?: string;
  /** The design revision the statistics were computed on (the /api/version stamp), like every kicad-cli reply. */
  revision?: string;
  board?: {
    has_outline: boolean;
    width_um: number;
    height_um: number;
    area_um2: number;
    front_copper_area_um2: number;
    back_copper_area_um2: number;
    front_courtyard_area_um2: number;
    back_courtyard_area_um2: number;
    front_density_pct: number;
    back_density_pct: number;
    min_track_width_um: number;
    min_clearance_um: number;
    min_drill_um: number;
    thickness_um: number;
  };
  footprints?: { title: string; front: number; back: number }[];
  pads?: BoardStatsEntry[];
  pad_properties?: BoardStatsEntry[];
  vias?: BoardStatsEntry[];
  drills?: BoardStatsDrill[];
  /** `FormatBoardStatisticsReport`'s text, only when requested with `report: true`. */
  report?: string;
  /** `<board>_report.txt`, source's default Save dialog name. */
  report_file_name?: string;
}

// ---------------------------------------------------------------- Symbol Fields Table / Find / ERC pin map
//
// Source of truth: crates/ops/src/fields_table.rs, crates/ops/src/search.rs,
// crates/cli/src/sch_api.rs (the /api/sch/* routes).

export interface SymbolFieldEdit {
  /** Reference designator of the symbol (all its units). */
  id: string;
  /** "Value" / "Footprint" / "Datasheet" or a user field's name. Never "Reference". */
  field: string;
  value: string;
}

export interface SymbolFieldRename {
  from: string;
  to: string;
}

/** One column of the fields table (`BOM_FIELD`). `name` is the canonical field name ("Reference", "Value", "Footprint", "Datasheet", "${QUANTITY}", or a user field). */
export interface FieldsTableColumn {
  name: string;
  label: string;
  show: boolean;
  group_by: boolean;
}

/** The dialog's view state (`BOM_PRESET`). */
export interface FieldsTableSpec {
  columns: FieldsTableColumn[];
  group_symbols: boolean;
  sort_field: string;
  sort_asc: boolean;
  filter: string;
}

export interface FieldsTableRow {
  /** Distinct reference designators in this row -- the ids a cell edit applies to. */
  refs: string[];
  flag: "singleton" | "group" | "child";
  item_number: number;
  /** One display string per column, in `spec.columns` order. */
  cells: string[];
  /** True where the cell is the "-- mixed values --" placeholder. */
  mixed: boolean[];
  /** A group row's per-symbol rows (what expanding it reveals). */
  children: FieldsTableRow[];
}

export interface FieldsTableReply {
  ok: boolean;
  message?: string;
  spec: FieldsTableSpec;
  user_fields: string[];
  rows: FieldsTableRow[];
}

/** `BOM_FMT_PRESET`. */
export interface BomFmt {
  name: string;
  field_delimiter: string;
  string_delimiter: string;
  ref_delimiter: string;
  ref_range_delimiter: string;
  keep_tabs: boolean;
  keep_line_breaks: boolean;
}

export interface BomExportReply {
  ok: boolean;
  message?: string;
  text?: string;
  /** Relative to the board directory; present once a file was written. */
  file?: string;
  default_path?: string;
}

/** `EDA_SEARCH_DATA` + `SCH_SEARCH_DATA` (wire format of `eda_ops::search::SearchData`). */
export interface SchSearchData {
  find: string;
  replace: string;
  match_case: boolean;
  mode: "plain" | "whole_word" | "wildcard";
  search_hidden_fields: boolean;
  search_pins: boolean;
  search_net_names: boolean;
  replace_references: boolean;
  search_and_replace: boolean;
}

export interface FindMatch {
  /** `kind:id:name` -- stable key `replace_text.items` is restricted by. */
  key: string;
  kind: "field" | "label" | "text" | "pin";
  /** Symbol reference / label id / text id that owns the match. */
  id: string;
  /** Field name or pin number (empty for labels/text). */
  name: string;
  /** World-space um. */
  at: [number, number];
  text: string;
}

export interface FindReply {
  ok: boolean;
  message?: string;
  matches: FindMatch[];
}

export interface ErcPinMapReply {
  ok: boolean;
  message?: string;
  /** 12x12 `PIN_ERROR` grid (0 OK / 1 warning / 2 error), `ELECTRICAL_PINTYPE` order. */
  matrix: number[][];
  /** True when the design stores its own map (`schematic.erc_pin_map`). */
  custom: boolean;
}
