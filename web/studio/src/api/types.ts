// Mirrors the JSON the backend actually emits. Source of truth:
//   crates/cli/src/studio.rs   (the `state()` function builds this JSON)
//   crates/ops/src/lib.rs      (the `Cmd` enum, `#[serde(tag = "op")]`)
//   crates/model/src/ir.rs     (Side, LabelSide, Track, Via)
//   crates/model/src/lib.rs    (PlacementRule, `#[serde(tag = "kind")]`)
//   crates/model/src/footprint.rs (Pad shapes)
//
// Coordinates are nanometres, as KiCad's internal units are (i64 in Rust; safe as
// JS `number` up to 2^53 nm, 9000 km; an area in nm^2 is not exact past
// 2^53, about 95 x 95 mm, so it is shown in mm^2). Angles in `rot` are degrees
// (the backend already divides millidegrees by 1000 before sending).

import type { AnnotateOptions } from "./hierarchyTypes";
import type { JunctionLook, SchEditCmd, SchGraphic, SchMoveCmd, SchStroke } from "./schEditTypes";
import type { BoardBarcode, BoardDrillChart, BoardDrillMap, BoardDrillMark, BoardDrillProfile, BoardDrillSpan, BoardGridItem, BoardPoint, BoardRefImage, BoardTextBox, BoardViaStitch, PcbItemCmd } from "./pcbItemTypes";
import type { MicrowaveCmd } from "./microwaveTypes";
import type { EmbeddedCmd } from "./embeddedTypes";
import type { TableCmdWire } from "./tableTypes";
import type { TuningPattern } from "./tuneTypes";
import type { LineEnding } from "../kicad-port/lineEnding";

export type Nm = number;
export type Degrees = number;

export type Side = "top" | "bottom";
export type LabelSide = "above" | "below" | "left" | "right";

export interface Pad {
  num: string;
  net: string | null;
  /** The copper's centre on the board (the pad's position moved by its shape offset, if it has one). */
  x: Nm;
  y: Nm;
  /** The copper's extent on the board (a turned pad's bounding box). */
  w: Nm;
  h: Nm;
  round: boolean;
  /** Through-hole (drilled) vs. surface-mount. */
  th: boolean;
  /**
   * What the Pad Properties dialog edits (crates/cli/src/fp_json.rs `pad_json`; absent from an older backend): the pad's id (`REF.NUM[#k]`),
   * its shape and type, its size un-turned, its own rotation (clockwise degrees), the corner ratio, the hole, the pad's position (where the
   * hole is: differs from `x`, `y` only with an offset), and the overrides that were set on it.
   */
  id?: string;
  shape?: PadShapeName;
  kind?: PadKindName;
  size?: [Nm, Nm];
  rot?: Degrees;
  ratio?: number | null;
  drill?: Nm | null;
  slot?: [Nm, Nm] | null;
  px?: Nm;
  py?: Nm;
  offset?: [Nm, Nm] | null;
  mask_margin?: Nm | null;
  paste_margin?: Nm | null;
  paste_ratio?: number | null;
  /** A trapezoid's two deltas (`PAD::GetDelta`, KiCad keeps one of them zero); null for any other shape. */
  delta?: [Nm, Nm] | null;
  /** A chamfered rectangle's ratio and corners (`PAD::GetChamferRectRatio`, `GetChamferPositions`); null for any other shape. */
  chamfer_ratio?: number | null;
  chamfer_corners?: ChamferCorners | null;
  /** A custom pad's anchor, zone knockout and basic shapes (`PadCustom`); null for any other shape. */
  custom?: PadCustom | null;
  /**
   * The copper of a trapezoid, a chamfered rectangle or a custom pad as the DRC measures it (`eda_drc::pad_shape::pad_outline`): contours in nanometres on the
   * board, the outline of each piece first and then its holes. Fill them with the even-odd rule. Absent for the shapes that `w`, `h` and `shape` draw.
   */
  outline?: [Nm, Nm][][] | null;
  /** True once the pad has an edit of its own in the design. */
  edited?: boolean;
  /** The edit as stored (`PadEditCmd`), for the dialog to send back with the one change it makes. */
  edit?: PadEditCmd | null;
  /** The padstack as the model has it (`PadStack`), when the pad has more than one shape for all layers or does not keep every layer; the shape fields above are its front. */
  stack?: PadStack | null;
  /** For a padstack whose copper differs from layer to layer: the copper of each group of layers on the board, with the keys of a pad's own (`x`, `y`, `w`, `h`, `round`, `shape`, `ratio`, `outline`). */
  stack_layers?: PadStackLayer[] | null;
  /** A plated through-hole pad that removes the layers nothing connects it on (Remove Unused Pads): the copper layers that have its ring (`eda_connectivity::flashed_layers`). */
  flashed?: string[] | null;
}

/** `eda_model::padstack::PadStackMode` (`PADSTACK::MODE`). */
export type PadStackMode = "normal" | "front_inner_back" | "custom";
/** `eda_model::padstack::UnconnectedLayers` (`UNCONNECTED_LAYER_MODE`): "All copper layers", "Connected layers only", "Front, back and connected layers", "Start and end layers only" (a via's). */
export type UnconnectedLayers = "keep_all" | "remove_all" | "remove_except_start_and_end" | "start_end_only";
/** `eda_model::padstack::PadLayerShape`: one copper layer of a padstack (`PADSTACK::COPPER_LAYER_PROPS`), in the pad's own frame. */
export interface PadLayerShape {
  shape: PadShapeName;
  size: [Nm, Nm];
  roundrect_ratio?: number | null;
  offset?: [Nm, Nm] | null;
  trapezoid_delta?: [Nm, Nm] | null;
  chamfer_ratio?: number | null;
  chamfer_corners?: ChamferCorners;
  custom?: PadCustom | null;
  /** What the layer's node of the file says beyond its shape, as the file wrote it (thermal and clearance settings). */
  tokens?: string[];
}
/** `eda_model::padstack::PadStack`: the shapes of a pad's other copper layers and what becomes of the layers nothing connects to; the pad's own fields are the front. */
export interface PadStack {
  mode?: PadStackMode;
  inner?: PadLayerShape | null;
  back?: PadLayerShape | null;
  /** `custom`: the shape of each copper layer but the front, named as for a footprint on the top side. */
  layers?: Record<string, PadLayerShape>;
  unconnected?: UnconnectedLayers;
}
/** One group of layers that shares a copper shape of a padstack pad, as the state carries it (crates/cli/src/fp_json.rs `stack_layers_json`). */
export interface PadStackLayer {
  layers: string[];
  role: string;
  x: Nm;
  y: Nm;
  w: Nm;
  h: Nm;
  round: boolean;
  shape: PadShapeName;
  ratio?: number | null;
  outline?: [Nm, Nm][][] | null;
}

/** The engine's pad shapes (`eda_model::PadShape`) and three pad types (`PadKind`), as the studio state names them. */
export type PadShapeName = "rect" | "round_rect" | "circle" | "oval" | "trapezoid" | "chamfered_rect" | "custom";
/** `eda_model::footprint::PadPrimitive`: a basic shape of a custom pad, in the pad's own frame (the pad's position is the origin). A proxy item is not copper. */
export interface PadPrimitive {
  shape: CmdShape;
  proxy?: boolean;
}
/** `eda_model::footprint::PadCustom`: a custom pad's anchor (`PAD::GetAnchorPadShape`), what a zone keeps clear of (`CUSTOM_SHAPE_ZONE_MODE`) and its basic shapes. */
export interface PadCustom {
  anchor: "circle" | "rect";
  zone: "outline" | "convex_hull";
  primitives: PadPrimitive[];
}
export type PadKindName = "smd" | "through_hole" | "non_plated_hole";

/**
 * A footprint's field as laid out on the board (crates/cli/src/fp_json.rs `field_json`): the Reference, the Value or a user field. `x`, `y` and
 * `angle` are on the board (`angle` is KiCad's: counter-clockwise on the screen, millidegrees); `lx`, `ly` and `langle` are the same in the footprint's own
 * frame, which is what `edit_board_footprint` takes.
 */
export interface FieldInfo {
  id: string;
  name: string;
  text: string;
  /** What `${NAME}` in `text` stands for; absent when it is `text` itself. See `BoardText.shown`. */
  shown?: string;
  x: Nm;
  y: Nm;
  angle: number;
  w: Nm;
  h: Nm;
  thickness: Nm;
  layer: string;
  visible: boolean;
  halign: -1 | 0 | 1;
  valign: -1 | 0 | 1;
  mirror: boolean;
  bold: boolean;
  italic: boolean;
  upright: boolean;
  knockout: boolean;
  lx: Nm;
  ly: Nm;
  langle: number;
  /** True once the layout is stored in the design; false for the default placement. */
  custom: boolean;
}

/** `FootprintAttrs` as the studio state names them. */
export interface FootprintAttrsInfo {
  kind: "unspecified" | "smd" | "through_hole";
  board_only: boolean;
  exclude_from_pos_files: boolean;
  exclude_from_bom: boolean;
  dnp: boolean;
  allow_missing_courtyard: boolean;
  custom: boolean;
}

/** `[minX, minY, maxX, maxY]`, footprint-local, nm. */
export type CourtyardBox = [Nm, Nm, Nm, Nm];

/** `STROKE_PARAMS`' line style, the words `(stroke (type ..))` uses. A solid (or `default`) stroke has no `dash` on the wire. */
export type FpDash = "dash" | "dot" | "dash_dot" | "dash_dot_dot";
/** `FILL_T`'s hatched fills. */
export type FpHatch = "hatch" | "reverse_hatch" | "cross_hatch";

interface FpGraphicCommon {
  layer: string;
  stroke_width: Nm;
  /** Absent when false. */
  filled?: boolean;
  dash?: FpDash;
  hatch?: FpHatch;
  /** Every layer of a shape that is on several (`layer` is the first). */
  layers?: string[];
}

/**
 * One graphic of a placed footprint (crates/cli/src/fp_art_json.rs, `Part.graphics`): the silkscreen outline, the pin-1 mark, the fab body, an assembly drawing -- the
 * `Shape` the board's own graphics are, in board space like the pads, without an `id` (kicad-port/fpArt.ts `graphicsOf` gives them complete).
 */
export type FpGraphic =
  | (FpGraphicCommon & { kind: "segment"; start: [Nm, Nm]; end: [Nm, Nm] })
  | (FpGraphicCommon & { kind: "arc"; start: [Nm, Nm]; mid: [Nm, Nm]; end: [Nm, Nm] })
  | (FpGraphicCommon & { kind: "rect"; start: [Nm, Nm]; end: [Nm, Nm] })
  | (FpGraphicCommon & { kind: "circle"; center: [Nm, Nm]; end: [Nm, Nm] })
  | (FpGraphicCommon & { kind: "polygon"; pts: [Nm, Nm][] })
  | (FpGraphicCommon & { kind: "bezier"; start: [Nm, Nm]; c1: [Nm, Nm]; c2: [Nm, Nm]; end: [Nm, Nm] });

/** One text of a placed footprint (`fp_text user`, `Part.texts`): the keys of a field, at the absolute angle it has on the board, variables resolved. */
export type FpTextInfo = Pick<FieldInfo, "id" | "text" | "x" | "y" | "angle" | "w" | "h" | "thickness" | "layer" | "halign" | "valign" | "mirror" | "bold" | "italic" | "upright" | "knockout"> & {
  /** Set when the text is exactly `${REFERENCE}` or `${VALUE}`: the Appearance panel's References and Values rows switch it (`PCB_TEXT::ViewGetLOD`). */
  role?: "reference" | "value";
};

/** The options of `DIALOG_EXCHANGE_FOOTPRINTS` (crates/ops/src/fp_exchange.rs `ExchangeOptions`; kicad-port/pcbExchange.ts has the three presets). */
export interface ExchangeOptions {
  /** "Match pad positions". */
  match_pad_positions: boolean;
  /** "Remove text items if not in library footprint". */
  remove_extra_texts: boolean;
  /** "Update/reset text layers and visibilities". */
  reset_text_layers: boolean;
  /** "Update/reset text sizes and styles". */
  reset_text_effects: boolean;
  /** "Update/reset text position and rotation". */
  reset_text_positions: boolean;
  /** "Update/reset text content". */
  reset_text_content: boolean;
  /** "Update/reset fabrication attributes". */
  reset_fabrication_attrs: boolean;
  /** "Update/reset clearance overrides". */
  reset_clearance_overrides: boolean;
  /** "Update/reset 3D models". */
  reset_3d_models: boolean;
}

/** The dialog of Geographical Reannotate (`DIALOG_BOARD_REANNOTATE`; crates/model/src/reannotate.rs `ReannotateOptions`, kicad-port/reannotate.ts has the values it opens with). */
export interface ReannotateOptions {
  /** "Footprint Order": which of the eight radio buttons, 0 to 7, in the dialog's order. */
  direction: number;
  /** "Based on location of": the footprint or its Reference text. */
  location: "footprint" | "reference";
  /** "Round locations to": the grid, nm. */
  grid_nm: number;
  /** "Reannotation Scope". */
  scope: "all" | "front" | "back" | "selection";
  /** "Exclude locked footprints". */
  exclude_locked: boolean;
  /** "Exclude references": prefixes, or patterns ending in `*`, separated by commas or spaces. */
  exclude: string;
  /** "Front reference start" (1 by default) and "Back reference start" (0, blank, continues from the front). */
  front_start: number;
  back_start: number;
  front_prefix: string;
  back_prefix: string;
  remove_front_prefix: boolean;
  remove_back_prefix: boolean;
}

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
  /** Courtyard full (width, height), nm — present even when unplaced. */
  size: [Nm, Nm] | null;

  // Present only when `placed` is true:
  at?: [Nm, Nm];
  rot?: Degrees;
  /** KiCad's orientation (`FOOTPRINT::GetOrientationDegrees`, counter-clockwise, in (-180, 180]): the file's angle, half a turn from `-rot` for a back footprint
   * made or flipped here (`eda_kicad::kicad_footprint_orientation`). Absent from an older backend: read it with `fpOrientation`. */
  orientation?: Degrees;
  side?: Side;
  label?: LabelSide;
  /** The box of the courtyard the board file has, board space (`eda_model::footprint::placed_courtyard_box`): an off-centre courtyard stays off-centre. */
  courtyard?: CourtyardBox | null;
  /**
   * The part's body, board-space nm: the box of its footprint's `F.Fab` graphics (crates/cli/src/body_api.rs). The 3D tab sizes its placeholder boxes from this
   * (kicad-port/partBody.ts) -- the courtyard is the body plus a clearance margin plus the pads' reach. `null` when no `F.Fab` is known for the footprint.
   */
  body?: CourtyardBox | null;
  /**
   * The part's 3D models, each with the placement its footprint gives it (crates/cli/src/model3d_api.rs `part_json`; the same shape as kicad-port/model3d.ts's
   * `Model3dEntry`): the 3D tab loads each from `GET /api/3dmodel` and places it the way KiCad does. Absent when the footprint names no model: the tab draws the body box.
   */
  models?: Array<{ name: string; offset: [number, number, number]; scale: [number, number, number]; rotate: [number, number, number]; opacity: number; show: boolean }>;
  /** How KiCad's 3D viewer sorts the part's models: a through-hole model (`tht`), an SMD one or a virtual one (`BOARD_ADAPTER::IsFootprintShown`). */
  kind3d?: "smd" | "tht" | "virtual";
  /** The board footprint's "Do not populate" and "Exclude from position files" (`FP_DNP`, `FP_EXCLUDE_FROM_POS_FILES`), which hide its models unless the 3D viewer's
   * "Models marked DNP" / "Models not in POS File" rows are on (`IsFootprintShown`). Absent = false. */
  dnp?: boolean;
  not_in_pos?: boolean;
  pads?: Pad[];
  /** A multi-gate footprint's units (`FOOTPRINT::GetUnitInfo`): each unit's name and the numbers of its pads; absent when it has fewer than two (Swap Gate Nets). */
  units?: Array<{ name: string; pins: string[] }> | null;
  /** The footprint's silkscreen, fab and assembly drawings, board space (absent when it has none). */
  graphics?: FpGraphic[];
  /** The footprint's own texts (`${REFERENCE}` on the fab layer ...), board space (absent when it has none). */
  texts?: FpTextInfo[];
  /** Reference, Value and the user fields, as they are on the board (absent from an older backend). */
  fields?: FieldInfo[];
  /** The attributes in effect: the edit's, else derived from the pads and the schematic symbol. */
  attrs?: FootprintAttrsInfo;
  /** The zone-connection facts this placed footprint and its pads carry (`crates/cli/src/studio.rs`); absent when there are none. */
  zone?: PartZoneFacts;
  /** `FOOTPRINT::GetLibDescription` and `GetKeywords`: the board file's `(descr ..)` and `(tags ..)` of the footprint; absent when it has none. */
  lib_description?: string;
  keywords?: string;
  /** The component classes the board file gives the footprint itself (`(component_classes ..)`); absent when none. */
  component_classes?: string[];
  /** The footprint's own `(solder_paste_margin ..)` (nm) and `(solder_paste_margin_ratio ..)` (`GetLocalSolderPasteMargin`, `..Ratio`); absent when it has none. */
  paste_margin?: Nm;
  paste_margin_ratio?: number;
}

/** `Part.zone`: what Footprint Properties and Pad Properties show in their zone fields. `null` is "inherited". */
export interface PartZoneFacts {
  /** `FOOTPRINT::GetLocalZoneConnection`. */
  connection: PadConnection | null;
  /** `FOOTPRINT::GetLocalClearance`. */
  clearance: Nm | null;
  /** The pads that set something of their own. */
  pads: { num: string; connection: PadConnection | null; gap: Nm | null; spoke_width: Nm | null; spoke_angle_mdeg: number | null; clearance: Nm | null }[];
}

export interface Track {
  id: string;
  net: string;
  layer: string;
  width: Nm;
  pts: [Nm, Nm][];
  /** The arc's mid point when this track is a KiCad arc (`PCB_ARC`; `pts` is then its tessellation), else null/absent. */
  arc_mid?: [Nm, Nm] | null;
}

export interface Via {
  id: string;
  net: string;
  x: Nm;
  y: Nm;
  /** Diameter (the backend's `d`, its `Via.diameter`). */
  d: Nm;
  drill: Nm;
  from: string;
  to: string;
  /** What the via does with the layers nothing connects it on (`remove_unused_layers`); absent when it keeps them all. */
  unconnected?: UnconnectedLayers | null;
  /** `VIATYPE` when it is not a through via (the router's Place Blind/Buried Via and Place Microvia, or the board file's `(via blind ..)`); absent for a through via. */
  type?: "blind" | "buried" | "micro" | null;
  /** With `unconnected`: the copper layers that have its ring. */
  flashed?: string[] | null;
}

/** `crates/model/src/ir.rs` `PadConnection` ("ZONE_CONNECTION") -- exact Rust variant names on the wire (no `rename_all`, see api/types.ts's own convention note below). */
export type PadConnection = "None" | "Thermal" | "Full" | "ThtThermal";
/** `ISLAND_REMOVAL_MODE`. */
export type IslandRemovalMode = "Always" | "Never" | "Area";
/** `ZONE_FILL_MODE`. */
export type FillMode = "Polygons" | "HatchPattern";
/** `ZONE_SETTINGS::SMOOTHING_*` (`crates/model/src/ir.rs` `ZoneSmoothing`, snake_case on the wire): how the zone outline's corners are cut before the fill. */
export type ZoneSmoothing = "none" | "chamfer" | "fillet";

/** `crates/ops/src/lib.rs` `SizeSpec`/`ViaSizeSpec`, `#[serde(tag = "kind")]` -- `edit_tracks_and_vias`'s track-width/via-size fields: either resolve from the item's own net class (`BoardRules::width_of`/`via_diameter_of`/`via_drill_of`), or an explicit value. */
export type SizeSpec = { kind: "net_class" } | { kind: "value"; nm: Nm };
export type ViaSizeSpec = { kind: "net_class" } | { kind: "value"; diameter: Nm; drill: Nm };

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
  | { kind: "grid"; nx: number; ny: number; dx: Nm; dy: Nm; offset_x?: Nm; offset_y?: Nm; centred?: boolean; stagger?: number; stagger_rows?: boolean; horizontal_then_vertical?: boolean; reverse_alternate?: boolean }
  | { kind: "circular"; center: PointXY; count: number; angle_millideg: number; angle_offset_millideg?: number; clockwise?: boolean; rotate_items?: boolean };

/** `crates/ops/src/array_numbering.rs` `NumberingType` (`ARRAY_AXIS::NUMBERING_TYPE`), snake_case on the wire. */
export type NumberingType = "numeric" | "hex" | "alpha_no_iosqxz" | "alpha_full";

/** One numbering axis of the Footprint Editor's Create Array: the alphabet, the "Pad numbering start" entry as typed and the "Pad numbering increment". */
export interface AxisSpec {
  kind: NumberingType;
  start: string;
  step: number;
}

/**
 * `crates/ops/src/array_numbering.rs` `PadNumbering` -- how `create_footprint_array` renumbers the pads of the copies. `start_specified: false` is "Use first free
 * number" (numerals from 1 that skip the numbers of the pads outside the array); `two_d` is a grid's "Coordinate (A1, A2, ... B1, ...)" scheme.
 */
export interface PadNumbering {
  start_specified: boolean;
  two_d: boolean;
  primary: AxisSpec;
  secondary: AxisSpec;
}

/**
 * `crates/model/src/ir.rs` `Zone`'s `ZONE_SETTINGS` fill-engine fields
 * (dialog_copper_zones.cpp's panel) -- everything but id/net/layer/
 * outline, factored out so `Zone` (always populated) and `Cmd`'s
 * `edit_zone`/the dialog's own form state (`ZoneDialog.tsx`) share one
 * field list instead of three copies drifting apart.
 */
export interface ZoneSettingsFields {
  clearance: Nm;
  min_thickness: Nm;
  thermal_gap: Nm;
  thermal_spoke_width: Nm;
  pad_connection: PadConnection;
  priority: number;
  island_removal_mode: IslandRemovalMode;
  /** nm^2. */
  min_island_area: number;
  fill_mode: FillMode;
  hatch_thickness: Nm;
  hatch_gap: Nm;
  hatch_orientation_mdeg: number;
  hatch_smoothing_level: number;
  hatch_smoothing_value: number;
  hatch_hole_min_area: number;
  hatch_border_algorithm: number;
  /**
   * "Corner smoothing": chamfer or fillet the outline's corners by `corner_radius` before filling (`ZONE::BuildSmoothedPoly`). Always in the
   * backend's zone JSON; optional here so that an `edit_zone` built without it (it keeps the zone's) and a zone literal in a test still type.
   */
  smoothing?: ZoneSmoothing;
  /** The chamfer distance or fillet radius, nm; only meaningful with `smoothing`. */
  corner_radius?: Nm;
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
  /**
   * The rule area's placement source (`ZONE::GetPlacementAreaEnabled`, `GetPlacementAreaSourceType`, `GetPlacementAreaSource`; GAPS.md items 24 and 32, package P24): the sheet,
   * component class or group whose footprints the area stands for, which Repeat Layout reads. Absent from a backend that predates it, and for a zone with none.
   */
  placement?: PlacementArea;
}

/** `REPEAT_LAYOUT_OPTIONS` as `Cmd` `repeat_layout` takes them. */
export interface RepeatLayoutOptions {
  copy_placement: boolean;
  copy_routing: boolean;
  connected_routing_only: boolean;
  copy_other_items: boolean;
  group_items: boolean;
  include_locked_items: boolean;
  anchor: string | null;
}

/** `PLACEMENT_SOURCE_T` as the `(placement ..)` of a rule area names it. */
export type PlacementSourceType = "sheetname" | "component_class" | "group";

/** `ZONE::m_placementAreaEnabled`, `m_placementAreaSourceType`, `m_placementAreaSource`. */
export interface PlacementArea {
  enabled: boolean;
  source_type: PlacementSourceType;
  /** The sheet path (`/ch1/`), the component class or the group name, by `source_type`. */
  source: string;
}

/**
 * `crates/model/src/ir.rs` `Zone` -- id/net/layer/outline plus the full
 * `ZONE_SETTINGS` (`ZoneSettingsFields`) and rule-area flags
 * (`RuleAreaFields`), kept in the IR and written to the derived `.kicad_pcb`, where
 * kicad-cli's filler reads them; see `ZoneDialog.tsx`. Every settings field has a KiCad-matching
 * server-side default (`Zone::default()`) once `add_zone` creates a zone,
 * so these are never actually absent from a real `/api/state` response --
 * not marked optional, same convention this file uses for every other
 * always-present field.
 */
export interface Zone extends ZoneSettingsFields, RuleAreaFields {
  id: string;
  net: string;
  /** The zone's first layer (`ZONE::GetFirstLayer`): the front-most copper layer of its set, else its first technical layer. */
  layer: string;
  /**
   * `ZONE::GetLayerSet()`: every layer the zone is on, `layer` first, in the order the Layers tab lists them (the copper stack from the front, then the technical layers) --
   * one layer for a zone on one. A zone with no copper layer in it is a non-copper zone (`fillNonCopperZone`: silkscreen, mask, paste and user layers; no net).
   * Absent from a backend that predates layer sets: read it with `zoneLayers` (`kicad-port/zoneLayers.ts`).
   */
  layers?: string[];
  /** Task item 4: true for a generated teardrop (`eda_model::ir::Zone::teardrop`), never a hand-drawn zone. */
  teardrop: boolean;
  /** `ZONE::GetZoneName()` ("" = unnamed); absent from a backend built before the Properties panel. */
  name?: string;
  outline: [Nm, Nm][];
  /** The outline's holes (`ZONE::Outline()`'s holes, `eda_model::ir::Zone::holes`): nothing of the zone is inside one. Absent for a zone without holes. */
  holes?: [Nm, Nm][][];
}

/** `RoutingSection::via_presets`' entry shape -- `BOARD_DESIGN_SETTINGS::m_ViaSizeList`. */
export interface ViaPreset {
  diameter: Nm;
  drill: Nm;
}

export interface Routing {
  tracks: Track[];
  vias: Via[];
  zones: Zone[];
  /** `BOARD_DESIGN_SETTINGS::m_TrackWidthList` -- Board Setup's editable extra-widths list for the W/Shift+W cycle, beyond `board_rules.track_width`. */
  track_width_presets: Nm[];
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
  max_len_nm: Nm;
  max_width_nm: Nm;
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

/** A hatch line of a hatched shape: `[x1, y1, x2, y2]`, nm (the `SEG`s of `EDA_SHAPE::GetHatchLines`). */
export type HatchLine = [Nm, Nm, Nm, Nm];

interface ShapeCommon {
  id: string;
  layer: string;
  stroke_width: Nm;
  filled: boolean;
  /** The hatch kind of a hatched rectangle, circle or polygon (`FILL_T::HATCH`, `REVERSE_HATCH`, `CROSS_HATCH`); `filled` is false then. Absent for a shape that is not hatched. */
  fill?: "hatch" | "reverse_hatch" | "cross_hatch";
  /** The lines `EDA_SHAPE::UpdateHatching` makes of that hatch from the board as it is, and the width they are drawn with (the shape's line width). */
  hatch_lines?: HatchLine[];
  hatch_width?: Nm;
  /** KiCad 10's line endings of a line, an arc or a curve (`(start_shape ..)`, `(end_shape ..)`; kicad-port/lineEnding.ts); absent for none. */
  start_ending?: LineEnding;
  end_ending?: LineEnding;
  /** The net of a shape on a copper layer; absent for none. */
  net?: string;
}

export type Shape =
  | (ShapeCommon & { kind: "segment"; start: [Nm, Nm]; end: [Nm, Nm] })
  | (ShapeCommon & { kind: "arc"; start: [Nm, Nm]; mid: [Nm, Nm]; end: [Nm, Nm] })
  | (ShapeCommon & { kind: "rect"; start: [Nm, Nm]; end: [Nm, Nm]; /** `EDA_SHAPE::GetCornerRadius()`, absent for a sharp rectangle. */ corner_radius?: Nm })
  | (ShapeCommon & { kind: "circle"; center: [Nm, Nm]; end: [Nm, Nm] })
  | (ShapeCommon & { kind: "polygon"; pts: [Nm, Nm][] })
  /** `PCB_SHAPE` BEZIER: a cubic from `start` to `end` with control points `c1`/`c2` (KiCad's `gr_curve` order). The canvas flattens it with kicad-port/bezierPoly.ts (`BEZIER_POLY::GetPoly`). */
  | (ShapeCommon & { kind: "bezier"; start: [Nm, Nm]; c1: [Nm, Nm]; c2: [Nm, Nm]; end: [Nm, Nm] });

export type TextJustify = "left" | "center" | "right";

export interface BoardText {
  id: string;
  content: string;
  /** What `${NAME}` in `content` stands for (the Text Variables page, KiCad's own variables); absent when it is `content` itself. The painter and the boxes use it; an edit starts from `content`. */
  shown?: string;
  x: Nm;
  y: Nm;
  /** Millidegrees -- NOT divided down like a Part's `rot`. */
  angle: number;
  layer: string;
  size: Nm;
  stroke_width: Nm;
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
  /** The length-tuning patterns of the board (`PCB_TUNING_PATTERN`, crates/model/src/tuning.rs); each is also listed in `groups` with the tracks of its meander as members. */
  tuning_patterns?: TuningPattern[];
  /** The board's text boxes (`PCB_TEXTBOX`) -- see api/pcbItemTypes.ts. Absent from a backend built before they existed. */
  text_boxes?: BoardTextBox[];
  /** The board's reference images (`PCB_REFERENCE_IMAGE`) -- see api/pcbItemTypes.ts. */
  images?: BoardRefImage[];
  /** The board's points (`PCB_POINT`) and barcodes (`PCB_BARCODE`) -- see api/pcbItemTypes.ts. */
  points?: BoardPoint[];
  /** KiCad 10's grid items (`PCB_GRID_ITEM`) -- see api/pcbItemTypes.ts. */
  grid_items?: BoardGridItem[];
  /** KiCad 10's via stitchings (`PCB_VIA_STITCH`) -- see api/pcbItemTypes.ts. */
  via_stitches?: BoardViaStitch[];
  /** KiCad 10's drill charts (what makes a table one), drill maps, drill symbol profile, and -- when the board has a map -- every hole's mark and the spans a map can show. */
  drill_charts?: BoardDrillChart[];
  drill_maps?: BoardDrillMap[];
  drill_profile?: BoardDrillProfile;
  drill_marks?: BoardDrillMark[];
  drill_spans?: BoardDrillSpan[];
  barcodes?: BoardBarcode[];
  /** The tables of the board (`PCB_TABLE`, crates/model/src/table.rs), with the geometry `crates/cli/src/table_json.rs` works out. Absent when the board has none. */
  tables?: BoardTable[];
}

/** `STROKE_PARAMS` of a table's border or separators: a width of -1 is "not drawn", `default` is drawn solid. */
export interface TableStrokeInfo {
  width: Nm;
  style: "default" | "solid" | "dash" | "dot" | "dash_dot" | "dash_dot_dot";
  color: [number, number, number, number] | null;
}

/** One line of a cell's text, placed: the point its first glyph's origin is at, board nm (fractions kept). */
export interface TablePlacedLine {
  text: string;
  x: number;
  y: number;
}

/** `PCB_TABLECELL` (`crates/cli/src/table_json.rs`). `corners` are top-left, top-right, bottom-right, bottom-left as the table reads them. */
export interface BoardTableCell {
  /** `<table id>:<row>:<col>`, the id a click on the cell selects. */
  id: string;
  row: number;
  col: number;
  /** The text as typed (`${VAR}` and all). */
  text: string;
  /** The variables resolved and the lines broken to the column; absent when it is `text`. */
  shown?: string;
  col_span: number;
  row_span: number;
  corners: [[Nm, Nm], [Nm, Nm], [Nm, Nm], [Nm, Nm]];
  /** left, top, right, bottom. */
  margins: [Nm, Nm, Nm, Nm];
  h_align: "left" | "center" | "right";
  v_align: "top" | "center" | "bottom";
  size_w: Nm;
  size_h: Nm;
  /** 0 is KiCad's automatic stroke width. */
  thickness: Nm;
  /** The stroke width the glyphs are drawn with. */
  pen: number;
  bold: boolean;
  italic: boolean;
  mirror: boolean;
  face: string | null;
  line_spacing: number;
  color: [number, number, number, number] | null;
  lines: TablePlacedLine[];
  /** A sheet's cell only: its background (`none`, `outline`, `background` or `color` with `fill_color`) and whether it is left out of simulation. */
  fill?: "none" | "outline" | "background" | "color";
  fill_color?: [number, number, number, number] | null;
  exclude_from_sim?: boolean;
}

/** `PCB_TABLE` (`crates/cli/src/table_json.rs`). */
export interface BoardTable {
  id: string;
  layer: string;
  /** The top-left corner of the first cell. */
  x: Nm;
  y: Nm;
  /** Millidegrees. */
  angle: number;
  rows: number;
  cols: number;
  col_widths: Nm[];
  row_heights: Nm[];
  border: { external: boolean; header: boolean; stroke: TableStrokeInfo };
  separators: { rows: boolean; cols: boolean; stroke: TableStrokeInfo };
  /** x0, y0, x1, y1 of the box round the first and the last cell. */
  bbox: [Nm, Nm, Nm, Nm];
  /** The lines `DrawBorders` draws. */
  borders: { a: [Nm, Nm]; b: [Nm, Nm]; width: Nm; style: TableStrokeInfo["style"]; color: [number, number, number, number] | null }[];
  cells: BoardTableCell[];
}

/** `crates/model/src/ir.rs` `Group` -- `PCB_GROUP` (on the board) and `SCH_GROUP` (on a sheet): a named set of member item ids (a part reference, a track/via/zone/shape/text id, a wire, a label ...) and of other groups' ids, no geometry of its own. */
export interface Group {
  id: string;
  name: string;
  member_ids: string[];
  /** `EDA_GROUP::m_designBlockLibId` (`Lib:Name`): the design block the group was placed from or saved to; empty or missing for none. */
  design_block?: string;
  /** A `PCB_GENERATOR` (which is a `PCB_GROUP`): what makes the group -- a tuning pattern (`Drawings.tuning_patterns`). It is selected as a group but edited through its properties and edit points, and deleting it puts the track back. */
  generator?: "tuning_pattern";
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
 * `measured_value_nm`/`text`) are computed once, server-side
 * (`eda_connectivity::dimension::compute_dimension_geometry`) -- a
 * renderer draws exactly these, never re-deriving crossbar/arrow/text
 * placement itself.
 */
export interface Dimension {
  id: string;
  layer: string;
  kind: "aligned" | "orthogonal" | "radial" | "leader" | "center";
  height: Nm | null;
  horizontal: boolean | null;
  leader_length: Nm | null;
  start: [Nm, Nm];
  end: [Nm, Nm];
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
  text_size_nm: Nm;
  /** The label's pen, nm; null/absent = 15 % of `text_size_nm` (`EDA_TEXT::GetTextThickness`). */
  text_thickness_nm?: Nm | null;
  stroke_width: Nm;
  arrow_length: Nm;
  extension_offset: Nm;
  extension_height: Nm;
  arrow_direction: ArrowDirection;
  /** Every line segment to draw: extension lines, crossbar/leader pieces
   * (already split around inline text), arrow barbs/tails, a centre cross. */
  lines: [[Nm, Nm], [Nm, Nm]][];
  text_at: [Nm, Nm];
  computed_text_angle: number;
  measured_value_nm: Nm;
  /** Fully formatted (prefix + value + units suffix + suffix, or the
   * override text if set). Draw this verbatim -- do not reformat `measured_value_nm` again. */
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
  text_size_nm: Nm;
  stroke_width: Nm;
  arrow_length: Nm;
  extension_offset: Nm;
  extension_height: Nm;
}

/** `CmdDimensionKind` -- `crates/model/src/ir.rs` `DimensionKind`'s own `#[serde(tag = "kind")]` shape, as `add_dimension`/`edit_dimension` need it (unlike `Dimension.kind` above, a flat string). */
export type CmdDimensionKind = { kind: "aligned"; height: Nm } | { kind: "orthogonal"; height: Nm; horizontal: boolean } | { kind: "radial"; leader_length: Nm } | { kind: "leader" } | { kind: "center" };

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
  text_size_nm: Nm;
  /** Null/absent = the default pen (15 % of the size). */
  text_thickness_nm?: Nm | null;
  stroke_width: Nm;
  arrow_length: Nm;
  extension_offset: Nm;
  extension_height: Nm;
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

/** `crates/model/src/ir.rs` `LibraryPadShape` (`PAD_SHAPE`); a custom pad keeps its anchor and basic shapes in `LibraryPad::custom`. */
export type LibraryPadShape = "circle" | "rect" | "oval" | "round_rect" | "trapezoid" | "chamfered_rect" | "custom";

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
  size: [Nm, Nm];
  shape: LibraryPadShape;
  kind: PadKind;
  drill: Nm | null;
  drill_slot: [Nm, Nm] | null;
  rot: Degrees;
  roundrect_ratio: number | null;
  /** `(dx, dy)` -- KiCad's own convention of only ever one axis non-zero. */
  trapezoid_delta: [Nm, Nm] | null;
  /** Fraction of the shorter side, same shape as `roundrect_ratio`. Only meaningful for `"chamfered_rect"`. */
  chamfer_ratio: number | null;
  chamfer_corners: ChamferCorners;
  /** A custom pad's anchor, zone knockout and basic shapes (`PAD::GetPrimitives`); absent or null for any other shape. */
  custom?: PadCustom | null;
  /** KiCad layer names this pad occupies ("F.Cu", "F.Paste", "F.Mask", "*.Cu", ...) -- the Pad Properties dialog's layer-preset buttons fill this in; empty means "not set yet". */
  layers: string[];
  /** `None`/`null` = inherit the board/footprint default, KiCad's own 0-means-inherit convention. */
  clearance_override: Nm | null;
  thermal_gap_override: Nm | null;
  thermal_spoke_width_override: Nm | null;
  /** The dialog's "Pad connection" (`PAD::GetLocalZoneConnection`); absent/`null` is "From parent footprint". */
  zone_connection?: PadConnection | null;
  /** The dialog's "Spoke angle" (`PAD::GetThermalSpokeAngle`), millidegrees as in the `.kicad_pcb`; absent/`null` is the shape's default (90 degrees, 45 for a circle). */
  thermal_spoke_angle_mdeg?: number | null;
  /** The padstack: the shapes of the pad's other copper layers and what becomes of the layers nothing connects to; the fields above are the front. Absent: one shape for every layer, every layer kept. */
  stack?: PadStack | null;
  /** What the pad's file form says that the editor does not edit (an edge connector, the fabrication property, the margins, ...), carried back. */
  kicad?: LibraryKeep;
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
  /** Where and how its text is drawn (`FieldLayout`; its `visible` is this field's). Absent for a field that was never placed: hidden at the origin on the fabrication layer. */
  layout?: FieldLayoutCmd;
}

/** The fields `edit_footprint_properties` commits all at once (dialog_footprint_properties_fp_editor.cpp's General tab) -- factored out so the `Cmd` variant and `FootprintPropertiesDialog`'s own form state share one field list. */
export interface FootprintPropertiesFields {
  description: string;
  keywords: string;
  attributes: FootprintAttributes;
  reference_visible: boolean;
  value_visible: boolean;
  /** The first model's path (the one model of an older entry). The 3D Models tab sends the whole list in `models3d`, and this follows its first path. */
  model: string | null;
  /** The 3D Models tab: the footprint's whole list of models (`PANEL_FP_PROPERTIES_3D_MODEL`). Left out of the JSON when the footprint has none; a key left out of a model is KiCad's default. */
  models3d?: Model3dCmd[];
  /** The Clearances tab's "Zone connection" (`FOOTPRINT::GetLocalZoneConnection`); absent/`null` is "inherited". */
  zone_connection?: PadConnection | null;
  /**
   * The General tab's fields grid (`PCB_FIELDS_GRID_TABLE`): the Reference's and the Value's layout (the text they show, when it is not `REF**` / the footprint's name, in `text`) and the
   * user fields, in order, each with its layout. A key left out of a command keeps what the footprint has; the Show check boxes are `reference_visible`, `value_visible` and each field's `visible`.
   */
  reference?: FieldLayoutCmd | null;
  value?: FieldLayoutCmd | null;
  fields?: FootprintField[];
  /** The grid's third and fourth mandatory rows, the Datasheet and the Description fields (`FIELD_T::DATASHEET`, `DESCRIPTION`), each with its text in `text`. Left out, the footprint keeps what it has. */
  datasheet?: FieldLayoutCmd | null;
  description_field?: FieldLayoutCmd | null;
}

/** `crates/model/src/lib_keep.rs`: what a library item's file says that the editors do not edit, kept for the writers. The studio only carries it back. */
export type LibraryKeep = Record<string, unknown>;

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
  courtyard: [Nm, Nm] | null;
  anchor: PointXY;
  published: boolean;
  /** `PAD_TOOL::InPadEditMode`: the pad being edited as graphic shapes (Edit Pad as Graphic Shapes), until Finish Pad Edit recombines it (kicad-port/padEditMode.ts). Absent when none is. */
  pad_edit?: PadEditMode | null;
  kicad?: LibraryKeep;
}

/** `crates/model/src/ir.rs` `PadEditMode`: the pad flagged `ENTERED`, and what its explode set aside (the custom pad's zone option and proxy items, which the recombine gives back). */
export interface PadEditMode {
  pad: string;
  zone?: "outline" | "convex_hull";
  proxies?: unknown[];
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
  outline: [Nm, Nm][] | null;
  /** The outline the way KiCad builds it from Edge.Cuts (`BOARD::GetBoardPolygonOutlines`): every outline with its cutouts, where `outline` is one polygon. Absent from an older backend. See `kicad-port/pcbOutline.ts` `boardOutlinePolygons`. */
  outline_polys?: { outer: [Nm, Nm][]; holes: [Nm, Nm][][] }[];
  /** The Edge.Cuts shapes in `drawings` are the whole outline (arcs, circles, cutouts, an open chain...) and `outline` is only their summary: the canvas draws the shapes, not the polygon. */
  outline_is_shapes?: boolean;
  /** What `kicad-cli pcb drc` reports as `invalid_outline` ("Board has malformed outline") for this board. */
  outline_errors?: { message: string; at: [Nm, Nm]; items: string[] }[];
  /** Copper stackup layer names, e.g. ["F.Cu", "B.Cu"]. */
  layers: string[];
  /** Placement grid pitch, nm. */
  snap: Nm;
  parts: Part[];
  rules: Rule[];
  routing: Routing | null;
  drawings: Drawings | null;
  checks: Check[];
  /** `BOARD_ITEM::IsLocked()` for every kind at once: a placed part's ref, or a track/via/zone/shape/text id (`DrawingsSection::locked_ids`, set by `set_locked`). Absent from an older backend = nothing locked. */
  locked?: string[];
  /** The drill/place file origin (`BOARD_DESIGN_SETTINGS::GetAuxOrigin`, `pcbnew.EditorControl.drillOrigin`), `[x, y]` nm; null/absent = (0, 0). */
  aux_origin?: [Nm, Nm] | null;
  /** The board's plot settings (`PCB_PLOT_PARAMS`, `(setup (pcbplotparams ..))`) the Plot and drill dialogs start from: the layer selection of the layers the board has, and "Use drill/place file origin". Absent from an older backend. */
  plot_params?: { layers: string[]; use_aux_origin: boolean };
  /** The point the editing grid is anchored at (`BOARD_DESIGN_SETTINGS::GetGridOrigin`, `common.Control.gridSetOrigin`), `[x, y]` nm; null/absent = (0, 0). */
  grid_origin?: [Nm, Nm] | null;
  /** The board's paper and title block (Page Settings, `common.Control.pageSettings`); null/absent are A4 landscape and an empty title block. */
  page?: PageInfo | null;
  title_block?: TitleBlock | null;
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
    track_width: Nm;
    via_drill: Nm;
    via_diameter: Nm;
    clearance: Nm;
    /**
     * `crates/model/src/lib.rs` `NetClass`, the classes other than Default. They start in the intent (or the imported project); Board
     * Setup > Net Classes edits them with `set_net_classes`, the edit lives in `design.json` and `board::load` lays it over the intent.
     */
    net_classes: NetClass[];
    hole_to_hole_min_nm: Nm;
    hole_clearance_nm: Nm;
    silk_clearance_nm: Nm;
    annular_width_min_nm: Nm;
    min_silk_text_height_nm: Nm;
    min_silk_text_thickness_nm: Nm;
    refdes_font_nm: Nm | null;
    stackup: Stackup | null;
    /*
     * Board Setup's pages (`eda_model::rules`), as the board is judged by them right now -- the intent's (or the imported
     * project's) values with any Board Setup edit laid over them. Optional: an older `eda` binary does not send them.
     */
    /** The Default net class: its sizes (and the micro-via / differential-pair ones, when set). `name` is "Default", `nets` empty. */
    default_class?: NetClass;
    constraints?: Constraints;
    /** True when the minimums are stated (an imported project, or an edit); false for the unstated ones of an intent. */
    constraints_explicit?: boolean;
    mask_paste?: MaskPaste;
    text_graphics?: TextGraphicsDefaults;
    /** `BOARD_DESIGN_SETTINGS::GetBoardThickness`, nm. */
    board_thickness_nm?: Nm;
    copper_layers?: number;
    /** Copper layer names in stack order: `F.Cu`, `In1.Cu` .. `B.Cu`. */
    layer_names?: string[];
    /** DRC settings key -> `error` | `warning` | `ignore`, for the checks whose severity is not KiCad's default (or is fixed by this app). */
    severities?: Record<string, RuleSeverity>;
    /** The text of the board's custom DRC rules (`.kicad_dru`); null = none. */
    custom_rules_text?: string | null;
    /** The project's text variables as `${NAME}` reads them, name -> value (`PROJECT::GetTextVars`): the Text Variables page's rows once it has been used, else the board folder's `path-variables.json`. */
    text_variables?: Record<string, string>;
    /** Board Editor Layers (`eda_model::layers`): the non-copper layers the board has in KiCad's order, the names the user gave its layers and the types that are not the default's. Absent from an older backend (then a board has the 18 technical layers). */
    layer_setup?: LayerSetup;
    /** Text & Graphics > Formatting (`eda_model::setup_pages`). */
    formatting?: FormattingSettings;
    /** Board Stackup > Zone Hatch Offsets: copper layer -> the offset of a hatched zone's pattern; only the layers that have one. */
    zone_hatch_offsets?: HatchOffsets;
    /** Design Rules > Length-tuning Patterns: what a tuning pattern of each kind starts from. */
    tuning_patterns?: TuningPatterns;
    /** Design Rules > Tuning Profiles: the profiles and the profile each net class names. */
    tuning_profiles?: TuningProfiles;
    /** The Board Setup pages an edit has replaced: `net_classes`, `constraints`, `mask_paste`, `text_graphics`, `stackup`, `severities`, `custom_rules`, `text_variables`, `board_layers`, `formatting`, `zone_hatch_offsets`, `tuning_patterns`, `tuning_profiles`. */
    overlay?: string[];
  };
}

export type RuleSeverity = "error" | "warning" | "ignore";

/** `eda_model::setup_pages::FormattingSettings` -- Text & Graphics > Formatting: the dash and gap of dashed lines, in line widths, and the "apply board defaults to footprint .." boxes. */
export interface FormattingSettings {
  dash_length_ratio: number;
  gap_length_ratio: number;
  apply_to_fp_fields: boolean;
  apply_to_fp_text: boolean;
  apply_to_fp_shapes: boolean;
  apply_to_fp_dimensions: boolean;
  apply_to_fp_barcodes: boolean;
}

/** `ZONE_LAYER_PROPERTIES::hatching_offset`, nm. */
export interface HatchOffset {
  x_nm: Nm;
  y_nm: Nm;
}
export type HatchOffsets = Record<string, HatchOffset>;

export type MeanderCornerStyle = "chamfer" | "round";

/** `PNS::MEANDER_SETTINGS`, the part Board Setup > Length-tuning Patterns edits. */
export interface MeanderSettings {
  min_amplitude_nm: Nm;
  max_amplitude_nm: Nm;
  spacing_nm: Nm;
  corner_style: MeanderCornerStyle;
  corner_radius_percentage: number;
  single_sided: boolean;
}
export interface TuningPatterns {
  single_track: MeanderSettings;
  diff_pair: MeanderSettings;
  diff_pair_skew: MeanderSettings;
}

export type TuningProfileType = "single" | "differential";

/** One row of a tuning profile's track propagation grid. Delays are KiCad's own unit, attoseconds per millimetre (1 ps/mm is 1,000,000). */
export interface ProfileLayerEntry {
  signal_layer: string;
  top_reference_layer?: string | null;
  bottom_reference_layer?: string | null;
  width_nm: Nm;
  diff_pair_gap_nm: Nm;
  delay_as_per_mm: number;
}

/** One row of the via overrides grid; the delay is a time in attoseconds (1 ps is 1,000,000). */
export interface ProfileViaOverride {
  signal_layer_from: string;
  signal_layer_to: string;
  via_layer_from: string;
  via_layer_to: string;
  delay_as: number;
}

export interface TuningProfile {
  name: string;
  profile_type: TuningProfileType;
  target_impedance: number;
  enable_time_domain_tuning: boolean;
  layer_entries: ProfileLayerEntry[];
  via_propagation_delay_as: number;
  via_overrides: ProfileViaOverride[];
}

/** Design Rules > Tuning Profiles: the profiles, and the profile each net class (`Default` included) names -- one the project does not have is what kicad-cli reports as `missing_tuning_profile`. */
export interface TuningProfiles {
  profiles: TuningProfile[];
  class_profiles?: Record<string, string>;
}

/** `LAYER_T`: a copper layer is a signal, a power plane, a mixed or a jumper layer; a user layer is auxiliary (it does not flip with the board) or off-board at the front or the back (it does). */
export type BoardLayerType = "signal" | "power" | "mixed" | "jumper" | "auxiliary" | "front" | "back";

/**
 * `eda_model::layers::LayerSetup` -- Board Setup > Board Editor Layers: the non-copper layers the board has (`enabled`: the technical layers by the names the board file writes, `F.SilkS`,
 * and `User.1` .. `User.45`), the name the user gave a layer (else KiCad's standard name shows) and the types that are not the layer's default.
 */
export interface LayerSetup {
  enabled: string[];
  names?: Record<string, string>;
  types?: Record<string, BoardLayerType>;
}

/** `eda_model::rules::NetClassSettings` -- Board Setup > Net Classes: the Default class and the others, with the net-name patterns that assign nets to each. */
export interface NetClassSettings {
  default: NetClass;
  classes: NetClass[];
}

/** `eda_model::rules::Constraints` -- Board Setup > Design Rules > Constraints (`rules.*` of the `.kicad_pro`), nm unless noted. */
export interface Constraints {
  min_clearance_nm: Nm;
  min_connection_nm: Nm;
  min_track_width_nm: Nm;
  min_annular_width_nm: Nm;
  min_via_diameter_nm: Nm;
  min_through_hole_nm: Nm;
  min_microvia_diameter_nm: Nm;
  min_microvia_drill_nm: Nm;
  min_hole_to_hole_nm: Nm;
  min_hole_clearance_nm: Nm;
  min_copper_edge_clearance_nm: Nm;
  min_silk_clearance_nm: Nm;
  min_groove_width_nm: Nm;
  min_silk_text_height_nm: Nm;
  min_silk_text_thickness_nm: Nm;
  max_error_nm: Nm;
  min_resolved_spokes: number;
  use_height_for_length_calcs: boolean;
  zones_allow_external_fillets: boolean;
}

/** `eda_model::SolderMaskRules` -- Board Setup > Board Stackup > Solder Mask/Paste. */
export interface MaskPaste {
  expansion_nm: Nm;
  min_width_nm: Nm;
  to_copper_clearance_nm: Nm;
  allow_bridges_in_footprints: boolean;
  tent_vias_front: boolean;
  tent_vias_back: boolean;
  /** `pad_to_paste_clearance`: the absolute margin, nm (negative shrinks the opening). */
  paste_margin_nm: Nm;
  /** `pad_to_paste_clearance_ratio`: a fraction of the pad (-0.05 = -5 %). */
  paste_margin_ratio: number;
}

/** One row of Board Setup > Text & Graphics > Defaults. */
export interface LayerClassDefaults {
  line_width_nm: Nm;
  text_width_nm: Nm;
  text_height_nm: Nm;
  text_thickness_nm: Nm;
  italic: boolean;
  upright: boolean;
}

/** `eda_model::rules::TextGraphicsDefaults`. */
export interface TextGraphicsDefaults {
  silk: LayerClassDefaults;
  copper: LayerClassDefaults;
  edge_cuts_line_width_nm: Nm;
  courtyard_line_width_nm: Nm;
  fab: LayerClassDefaults;
  others: LayerClassDefaults;
}

/** `eda_model::rules::StackupSettings` -- Board Setup > Board Stackup > Physical Stackup. */
export interface StackupSettings {
  /** 2, 4, .. 32. */
  copper_layers: number;
  stackup: Stackup;
}

/** `crates/model/src/lib.rs` `NetClass`; see `BoardState.board_rules.net_classes`'s doc. */
export interface NetClass {
  name: string;
  /** Patterns over net names (`*` any run, `?` any one character, anchored), e.g. `["GND", "VBAT*"]` -- first class whose pattern matches a net owns it. */
  nets: string[];
  track_width?: Nm;
  clearance?: Nm;
  via_diameter?: Nm;
  via_drill?: Nm;
  microvia_diameter?: Nm;
  microvia_drill?: Nm;
  diff_pair_width?: Nm;
  diff_pair_gap?: Nm;
  diff_pair_via_gap?: Nm;
  priority: number;
}

/** `crates/model/src/lib.rs` `Stackup`/`StackupLayer`. Edited whole through `set_stackup`. */
export interface Stackup {
  layers: StackupLayer[];
  /** `(copper_finish ..)`: "ENIG", "HASL", .. ; absent = not specified. */
  copper_finish?: string;
  dielectric_constraints?: boolean;
  /** 0 none, 1 yes, 2 bevelled. */
  edge_connector?: number;
  edge_plating?: boolean;
}
export interface StackupLayer {
  name: string;
  material: string | null;
  thickness_mm: number | null;
  /** KiCad's stackup item type: `copper`, `core`, `prepreg`, `Top Silk Screen`, `Top Solder Paste`, `Top Solder Mask` and their bottom counterparts. */
  kind?: string;
  epsilon_r?: number;
  loss_tangent?: number;
  /** `(color ..)` of a solder mask, silkscreen or dielectric: KiCad's name for a standard colour (`Green`, `FR4 natural`, ..) or `#RRGGBB[AA]`; absent = not specified. The 3D viewer paints the board with it ("Use board stackup colors"). */
  color?: string;
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
  x: Nm;
  y: Nm;
}

/** crates/model/src/ir.rs `Shape`, IR field names/point shape -- for `add_shape` only. Same `kind` tags as the display `Shape` above, different point representation ({x,y}, not [x,y]) and `id` left empty for a new one (the backend assigns it). */
export type CmdShape =
  | { kind: "segment"; id?: string; layer: string; stroke_width: Nm; filled: boolean; start: PointXY; end: PointXY }
  | { kind: "arc"; id?: string; layer: string; stroke_width: Nm; filled: boolean; start: PointXY; mid: PointXY; end: PointXY }
  | { kind: "rect"; id?: string; layer: string; stroke_width: Nm; filled: boolean; start: PointXY; end: PointXY }
  | { kind: "circle"; id?: string; layer: string; stroke_width: Nm; filled: boolean; center: PointXY; end: PointXY }
  | { kind: "polygon"; id?: string; layer: string; stroke_width: Nm; filled: boolean; pts: PointXY[] }
  | { kind: "bezier"; id?: string; layer: string; stroke_width: Nm; filled: boolean; start: PointXY; c1: PointXY; c2: PointXY; end: PointXY };

/** crates/model/src/ir.rs `Text`, IR field names -- for `add_text` only (`edit_text` takes flat fields instead, see Cmd below). */
export interface CmdText {
  id?: string;
  content: string;
  at: PointXY;
  angle: number;
  layer: string;
  size_nm: Nm;
  stroke_width: Nm;
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
  width: Nm;
  pts: PointXY[];
}
/** `crates/model/src/ir.rs` `Track` as `commit_route` takes it (ids are assigned server side). `arc_mid_offset` is the arc's mid point relative to `pts[0]`; `pts` must then be the arc's tessellation (`kicad-port/trackArc.ts`). */
export interface CmdRouteTrack {
  net: string;
  layer: string;
  width: Nm;
  pts: PointXY[];
  pins?: string[];
  arc_mid_offset?: PointXY;
}
export interface CmdVia {
  id?: string;
  net: string;
  at: PointXY;
  drill: Nm;
  diameter: Nm;
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

/** `eda_model::fp_edit::FieldLayout`: a field's text in the footprint's own frame (`at` and `angle` as `FieldInfo.lx`, `ly`, `langle`). */
export interface FieldLayoutCmd {
  at: PointXY;
  angle?: number;
  size: [Nm, Nm];
  thickness?: Nm;
  layer: string;
  visible?: boolean;
  halign?: -1 | 0 | 1;
  valign?: -1 | 0 | 1;
  mirror?: boolean;
  bold?: boolean;
  italic?: boolean;
  keep_upright?: boolean;
  knockout?: boolean;
  /** The text shown when it is not the footprint's reference or the part's value. */
  text?: string | null;
}
export interface UserFieldCmd {
  name: string;
  text: string;
  layout: FieldLayoutCmd;
}
/** `eda_model::fp_edit::FootprintAttrs`. */
export interface FootprintAttrsCmd {
  kind: "unspecified" | "smd" | "through_hole";
  board_only: boolean;
  exclude_from_pos_files: boolean;
  exclude_from_bom: boolean;
  dnp: boolean;
  allow_missing_courtyard: boolean;
}
/**
 * `eda_model::footprint::Model3d`: one `(model ...)` of a footprint, as the 3D Models tab edits it (crates/model/src/fp_models.rs). The placement is the footprint file's, in the
 * frame of the footprint on the top side: millimetres and degrees, the rotation as the file stores it (clockwise; the dialog shows it the other way round). A key left out is KiCad's
 * default: offset 0, scale 1, rotation 0, opaque, shown. `GET /api/state` sends the same model as `Part.models` entry, which names its file `name`.
 */
export interface Model3dCmd {
  path: string;
  offset?: [number, number, number];
  scale?: [number, number, number];
  rotate?: [number, number, number];
  opacity?: number;
  show?: boolean;
}
/** `eda_model::fp_edit::PadEdit`: the changes to one pad; a key left out is the pad as the library has it. */
export interface PadEditCmd {
  number: string;
  nth?: number;
  kind?: "smd" | "through_hole" | "non_plated_hole";
  shape?: PadShapeName;
  size?: [Nm, Nm];
  drill?: Nm;
  drill_slot?: [Nm, Nm];
  offset?: PointXY;
  rot?: number;
  roundrect_ratio?: number;
  solder_mask_margin?: Nm;
  solder_paste_margin?: Nm;
  solder_paste_margin_ratio?: number;
  /** A trapezoid's deltas; `[0, 0]` takes the delta away. */
  trapezoid_delta?: [Nm, Nm];
  /** A chamfered rectangle's chamfer ratio, `0..1` of the shorter side, and the corners it cuts. */
  chamfer_ratio?: number;
  chamfer_corners?: ChamferCorners;
  /** A custom pad's anchor, zone knockout and basic shapes, replaced whole. */
  custom?: PadCustom;
  /** The padstack, replaced whole (`PAD::SetPadstack`): the shapes of the other copper layers and the unconnected-layer mode. */
  stack?: PadStack;
}

/**
 * File > Import > Graphics... (`DRAWING_TOOL::PlaceImportedGraphics`): a DXF or SVG file (its `text`) as the board's own lines, arcs, circles, polygons, curves and text on `layer`,
 * one undo step. `offset_*_nm` is the importer's offset after the scale (nanometres); `fix_tolerance_nm` is "Fix discontinuities" with its tolerance; `replace_outline` makes
 * the imported Edge.Cuts shapes the board's outline. See crates/ops/src/import_graphics.rs.
 */
export interface ImportGraphicsCmd {
  op: "import_graphics";
  format: "dxf" | "svg";
  text: string;
  layer: string;
  dxf_units?: string;
  scale?: number;
  offset_x_nm?: number;
  offset_y_nm?: number;
  line_width_nm?: Nm;
  group?: boolean;
  fix_tolerance_nm?: Nm | null;
  replace_outline?: boolean;
}

/**
 * `SCH_DRAWING_TOOLS::ImportGraphics` (schematic) and `SYMBOL_EDITOR_DRAWING_TOOLS::ImportGraphics` (Symbol Editor): the same file as graphic lines, shapes and text of the sheet in
 * view, or as drawn items of the symbol `lib_id` on `unit` / `body_style`, in one undo step. `offset_*_nm` is the importer's offset after the scale; `line_width_nm` 0 is the sheet's
 * default width. See crates/ops/src/import_graphics_sch.rs.
 */
export interface ImportSchGraphicsCmd {
  op: "import_sch_graphics";
  format: "dxf" | "svg";
  text: string;
  dxf_units?: string;
  scale?: number;
  offset_x_nm?: number;
  offset_y_nm?: number;
  line_width_nm?: Nm;
}

export interface ImportSymbolGraphicsCmd {
  op: "import_symbol_graphics";
  lib_id: string;
  unit?: number;
  body_style?: number;
  format: "dxf" | "svg";
  text: string;
  dxf_units?: string;
  scale?: number;
  offset_x_nm?: number;
  offset_y_nm?: number;
  line_width_nm?: Nm;
}

/** `PASTE_MODE` (`include/dialogs/dialog_paste_special.h`): the reference designators of pasted footprints are given the next free number ("unique"), left as the clipboard has them even if another footprint has them ("keep", a plain paste's default) or cleared to `REF**` ("remove"). */
export type PcbPasteMode = "unique" | "keep" | "remove";

export type Cmd =
  | { op: "place"; part: string; anchor: string; side: Dir }
  | { op: "place_edge"; part: string; edge: Dir; fraction: number }
  | { op: "place_region"; part: string; region: Region }
  | { op: "place_at"; part: string; x: Nm; y: Nm }
  | { op: "move_to"; part: string; x: Nm; y: Nm }
  | { op: "nudge"; part: string; dir: Dir; steps: number }
  | { op: "rotate"; part: string; quarter_turns: number }
  | { op: "swap"; a: string; b: string }
  | { op: "rip"; part: string }
  | { op: "flip"; part: string }
  /** Footprint Properties' "Text Placement" field (the refdes label's side) -- does not move anything, so (unlike Flip/Rotate/MoveTo) it does not clear routing. */
  | ({ op: "set_label_side"; part: string } & { side: LabelSide })
  /** Footprint Properties' "Zone connection" and "Clearance" on a placed footprint (`FOOTPRINT::SetLocalZoneConnection` / `SetLocalClearance`): a whole-panel commit, `null` inherits. */
  | { op: "set_footprint_zone_connection"; part: string; zone_connection: PadConnection | null; clearance: Nm | null }
  /** Pad Properties' zone fields on the pads numbered `pad` of a placed footprint: "Pad connection", "Relief gap", "Spoke width", "Spoke angle" (millidegrees) and "Clearance". A whole-panel commit, `null` inherits. */
  | { op: "set_pad_zone_overrides"; part: string; pad: string; zone_connection: PadConnection | null; thermal_gap: Nm | null; thermal_spoke_width: Nm | null; thermal_spoke_angle_mdeg: number | null; clearance: Nm | null }
  | { op: "add_track"; net: string; layer: string; width: Nm; pts: PointXY[] }
  | { op: "delete_track"; id: string }
  | { op: "set_track_width"; id: string; width: Nm }
  | { op: "add_via"; net: string; x: Nm; y: Nm; drill: Nm; diameter: Nm; from_layer: string; to_layer: string }
  | { op: "delete_via"; id: string }
  | { op: "move_via"; id: string; x: Nm; y: Nm }
  /** Track/Via Properties' editable via fields -- net, position and layer span are unchanged (KiCad does not let you re-net/re-span an existing via from this dialog either). */
  | { op: "edit_via"; id: string; diameter: Nm; drill: Nm }
  /** `crates/ops/src/via_layers.rs`: Track & Via Properties' via type, layer pair and annular rings over `ids`; an omitted row is left alone. */
  | { op: "set_via_layers"; ids: string[]; kind?: "through" | "blind" | "buried" | "micro"; from_layer?: string; to_layer?: string; unconnected?: UnconnectedLayers }
  /** Board Setup > Track Widths & Vias: replace the W/Shift+W preset list wholesale (no per-entry add/remove Cmd -- same spirit as `paste_items`). The board's own default (`board_rules.track_width`) is not part of this list. */
  | { op: "set_track_width_presets"; widths: Nm[] }
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
  /** Group Properties' OK (`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`): rename the group and make its members exactly `member_ids` (each pulled out of any other group; under 2 members dissolves it). */
  | { op: "edit_group"; id: string; name: string; member_ids: string[]; design_block?: string }
  /** Save Selection as Design Block / Update Design Block from Selection: the items (a group included) are linked to the block (`Lib:Name`) -- a lone unlinked group takes the link, anything else goes in a new group named after the block. The server's save route sends it. */
  | { op: "link_design_block"; ids: string[]; block: string }
  /** Place Design Block on the board (`PCB_CONTROL::AppendDesignBlock`): the block's board file `text` with its origin at `at`; `as_group` puts what came in in a group named `group_name` and linked to `block`. */
  | { op: "place_design_block"; text: string; at: PointXY; block: string; as_group?: boolean; group_name?: string }
  /** The multichannel tool (crates/ops/src/multichannel.rs; GAPS.md items 24 and 32, package P24): a rule area for each sheet, class or group ticked in the Generate dialog. */
  | { op: "multichannel"; verb: "generate_rule_areas"; areas: { kind: "sheet" | "component_class" | "group"; key: string }[]; replace_existing?: boolean; group_items?: boolean }
  /** Repeat Layout after its dialog: the layout of the rule area `reference` onto the rule areas `targets` that match it (`REPEAT_LAYOUT_OPTIONS`). */
  | { op: "multichannel"; verb: "repeat_layout"; reference: string; targets: string[]; options?: RepeatLayoutOptions }
  /** Apply Design Block Layout: the design block `block`'s board file `text` repeated on the footprints of the group `group`, which is linked to it. */
  | { op: "multichannel"; verb: "apply_design_block_layout"; group: string; block: string; text: string }
  /**
   * Ctrl+T: `pcbnew.Array.createArray` (crates/ops/src/array.rs). `arrange: false` (the dialog's "Duplicate selection") makes a copy of the whole
   * selection for every point of the array but one -- footprints as new parts, tracks, vias, zones, graphics, text, dimensions, groups -- and
   * leaves the selection on the first; `arrange: true` ("Arrange selection") makes nothing and puts the given `ids`, in order, on the points.
   * `reannotate` (default true) is "Assign unique reference designators"; false is "Keep original reference designators". See `ArrayGeometry`'s
   * own doc for the angle-sign convention.
   */
  | { op: "create_array"; ids: string[]; geometry: ArrayGeometry; arrange?: boolean; reannotate?: boolean }
  /** Task item 7: `pcbnew/pcb_dimension.{h,cpp}`. `id` on `dimension`, if sent, is ignored. */
  | { op: "add_dimension"; dimension: CmdDimension }
  | { op: "delete_dimension"; id: string }
  /** Translate both feature points by `(dx, dy)` -- `PCB_DIMENSION_BASE::Move`. */
  | { op: "move_dimension"; id: string; dx: Nm; dy: Nm }
  /** Properties dialog's OK: replace every field at once. `id` on `dimension` is ignored. */
  | { op: "edit_dimension"; id: string; dimension: CmdDimension }
  /** Board Setup > Dimension Properties: applied to new dimensions from then on only. */
  | { op: "set_dimension_settings"; settings: DimensionSettings }
  /*
   * Board Setup's rule pages (`eda_ops::board_setup`): each replaces one whole page after the checks KiCad's own panel makes (a
   * refusal names the setting), and is one undo step. The board's rules, the router, DRC and the derived KiCad project all see the edit.
   */
  | { op: "set_net_classes"; settings: NetClassSettings }
  | { op: "set_constraints"; constraints: Constraints }
  | { op: "set_mask_paste"; settings: MaskPaste }
  | { op: "set_text_graphics_defaults"; settings: TextGraphicsDefaults }
  | { op: "set_stackup"; settings: StackupSettings }
  /** DRC settings key -> severity, for the checks that differ from KiCad's default. */
  | { op: "set_rule_severities"; severities: Record<string, RuleSeverity> }
  /** The text of the board's `.kicad_dru`, verbatim; empty = no custom rules. */
  | { op: "set_custom_rules"; text: string }
  /** The DRC Rule Editor's Save, Delete, Move Up and Move Down (`Cmd::EditDrcRule`; kicad-port/ruleEditor.ts has the rule's shape). */
  | { op: "edit_drc_rule"; action: "save" | "delete" | "move_up" | "move_down"; entry: number | null; name: string; rule?: import("../kicad-port/ruleEditor").RuleEditPayload }
  /** Board Setup > Text Variables (`panel_text_variables.cpp`): the project's variables, name -> value, the whole set. `${NAME}` shows the value; the derived `.kicad_pro` carries them for kicad-cli. */
  | { op: "set_text_variables"; variables: Record<string, string> }
  /** Board Setup > Board Editor Layers (`panel_setup_layers.cpp`): the whole page -- the layers the board has, their names and types. Refused with the layer's name when a name or type is out of the panel's rules or a layer is turned off while items are on it. */
  | { op: "set_board_layers"; settings: LayerSetup }
  /** Board Setup > Formatting (`panel_setup_formatting.cpp`): the whole page. Refused with the setting's name (`dash_length_ratio`) when a ratio is not above 0 and at most 1000. */
  | { op: "set_formatting"; settings: FormattingSettings }
  /** Board Setup > Zone Hatch Offsets (`panel_setup_zone_hatch_offsets.cpp`): the whole page, copper layer -> offset; a layer left out (or at 0, 0) has none. */
  | { op: "set_zone_hatch_offsets"; offsets: HatchOffsets }
  /** Board Setup > Length-tuning Patterns (`panel_setup_tuning_patterns.cpp`): the whole page. Refused with `group.field` (`diff_pair.min_amplitude`). */
  | { op: "set_tuning_patterns"; settings: TuningPatterns }
  /** Board Setup > Tuning Profiles (`panel_setup_tuning_profiles.cpp`): the whole page, with the profile each net class names. Refused with the profile's name. */
  | { op: "set_tuning_profiles"; settings: TuningProfiles }
  /**
   * `dialog_drc.cpp`'s "Exclude this violation", "Exclude with comment...", "Edit exclusion comment..." and "Exclude all violations of ..." (and Exclude
   * Marker): waive the DRC violations as one undo step. An exclusion whose `(check, items)` is waived already is replaced (that edits its comment).
   * Persisted in `design.drawings.drc_exclusions` and written into the derived `.kicad_pro` for kicad-cli (`DrcExclusionSpec`).
   */
  | { op: "add_drc_exclusions"; exclusions: DrcExclusionSpec[] }
  /** "Remove exclusion for this violation" (and "Remove all exclusions ..."): put the violations back in the report. */
  | { op: "delete_drc_exclusions"; exclusions: Array<{ check: string; items: string[] }> }
  | { op: "clear_drc_exclusions" }
  /** Schematic Setup > Violation Severity (`panel_setup_severities.cpp`): ERC settings key -> severity, the whole table (see `ErcSeveritiesReply`). */
  | { op: "set_erc_severities"; severities: Record<string, RuleSeverity> }
  /** `GLOBAL_EDIT_TOOL::SwapLayers` -- "move items on" -> "to layer" pairs (components/SwapLayersDialog.tsx). */
  | { op: "swap_layers"; mapping: [string, string][] }
  /** Remove Unused Pads: `selected` is the ids of "Selected only" (null: the whole board), `remove: false` is "Restore All Layers". */
  | { op: "remove_unused_pads"; selected: string[] | null; vias: boolean; pads: boolean; keep_outside: boolean; remove: boolean }
  /** Swap Pad Nets (the pads in selection order) and Swap Gate Nets (`target`: the unit to swap the one selected unit with). `connected_pads`: the answer to "Ignore Unselected Pads" (false) / "Swap All Connected Pads" (true); absent while it has not been asked. */
  | { op: "swap_pad_nets"; ids: string[]; connected_pads?: boolean | null }
  | { op: "swap_gate_nets"; ids: string[]; target?: string | null; connected_pads?: boolean | null }
  /** `Cmd::CommitRoute`: delete several tracks/vias (unknown ids tolerated) and add `tracks`/`vias` as ONE undo step -- what `unrouteSegment`/`deleteFull` (removal only) and the track edits (break, fillet, mirror) send. */
  | { op: "commit_route"; remove_track_ids: string[]; remove_via_ids: string[]; tracks?: CmdRouteTrack[]; vias?: CmdVia[]; /** indices into `tracks` of the new tracks that are locked */ lock_tracks?: number[] }
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
  | { op: "set_plot_settings"; layers?: string[]; use_aux_origin?: boolean }
  /** `common.Control.gridSetOrigin` / `gridResetOrigin` / `editGridOrigin` -- the point the editing grid is anchored at; `null` resets it to (0, 0). */
  | { op: "set_grid_origin"; at: PointXY | null }
  /** `common.Control.pageSettings` -- the board's paper and title block, one undo step (`BOARD_EDITOR_CONTROL::PageSettings`). Refused when nothing changes. */
  | { op: "set_board_page"; page: CmdPage; title_block: TitleBlock }
  /** The same for the schematic (`SCH_EDITOR_CONTROL::PageSetup`). */
  | { op: "set_schematic_page"; page: CmdPage; title_block: TitleBlock }
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
  | ({ op: "edit_zone"; id: string; net: string; layer: string; /** The layers the zone is on (`ZONE_SETTINGS::m_Layers`); absent leaves a set that holds `layer` as it is. */ layers?: string[] } & ZoneSettingsFields & RuleAreaFields)
  /** `pcb_point_editor.cpp`'s zone-outline editing (drag/add/remove a corner) -- the whole edited outline, replacing it wholesale (no live point-by-point Cmd). */
  | { op: "set_zone_outline"; id: string; outline: PointXY[]; holes?: PointXY[][] }
  | { op: "add_shape"; shape: CmdShape }
  | { op: "delete_shape"; id: string }
  | { op: "move_shape"; id: string; dx: Nm; dy: Nm }
  /** Shape Properties' editable fields (layer, line width, filled) -- geometry has no dialog field to edit in source either, only by dragging its own points (no point editor for shapes yet). */
  | { op: "edit_shape"; id: string; layer: string; stroke_width: Nm; filled: boolean }
  | { op: "add_text"; text: CmdText }
  | { op: "edit_text"; id: string; content: string; angle: number; layer: string; size_nm: Nm; stroke_width: Nm; justify: TextJustify; mirror: boolean }
  | { op: "delete_text"; id: string }
  | { op: "move_text"; id: string; x: Nm; y: Nm }
  /**
   * `dialog_global_edit_text_and_graphics.cpp`'s "Apply and Close",
   * scoped to the free-standing board items this model has: graphics,
   * text and dimensions (no footprint reference/value fields, tables or
   * barcodes exist as editable board items here -- see PARITY-pcb.md).
   * Every field omitted (or `null`) leaves that property alone;
   * `layer_defaults` is "Set to layer default values:" and ignores the
   * fields. Filtering (scope, layer, "selected only") is
   * `kicad-port/globalEditTextGraphics.ts`'s client-side job.
   */
  | { op: "edit_text_and_graphics"; shape_ids?: string[]; text_ids?: string[]; dimension_ids?: string[]; layer_defaults?: boolean; layer?: string | null; line_width?: Nm | null; text_size?: Nm | null; text_thickness?: Nm | null }
  /** Cmd+D (`EDIT_TOOL::Duplicate`): exact copies of the named items in place, under fresh ids -- footprints (as new parts), tracks, vias, zones, shapes, texts, dimensions, groups (with copies of their members). See crates/ops/src/lib.rs `Cmd::Duplicate`. */
  | { op: "duplicate"; ids: string[] }
  /** Cmd+V (`PCB_CONTROL::Paste`): KiCad's own clipboard text (the text a Copy or a KiCad puts there), its origin placed at `at`. See crates/ops/src/pcb_paste.rs. */
  | { op: "paste_clipboard"; text: string; at: PointXY }
  | ImportGraphicsCmd
  | ImportSchGraphicsCmd
  | ImportSymbolGraphicsCmd
  /** File > Import > Non-KiCad Board File... (`BOARD_EDITOR_CONTROL::OpenNonKicadBoard`): the open board replaced by `text`, the `.kicad_pcb` kicad-cli made of another EDA format's file (`POST /api/import_foreign`). One undo step; see crates/ops/src/import_foreign.rs. `keep_kicad_layer_names` is the dialog's "Keep KiCad layer names"; `project` is the `.kicad_pro` kicad-cli made with the board, which holds the design rules and net classes the importer set. */
  | { op: "replace_board"; text: string; name?: string; keep_kicad_layer_names?: boolean; project?: string; rules?: string }
  /** File > Import > Non-KiCad Schematic File... (`SCH_EDIT_FRAME::OnImportProject`): the schematic replaced by `content`, the sheet file `POST /api/import_foreign` answered. One undo step. */
  | { op: "replace_schematic"; content: unknown; name?: string }
  /** File > Import > Specctra Session... (`PCB_EDIT_FRAME::ImportSpecctraSession`, `SPECCTRA_DB::FromSESSION`): the session's wires and vias in place of the board's unlocked tracks and vias, and the footprints where its placement puts them. `session` is the file's text. One undo step; see crates/ops/src/specctra.rs. */
  | { op: "specctra"; session: string; name?: string }
  /** Paste Special... (`DIALOG_PASTE_SPECIAL`, crates/ops/src/pcb_paste_special.rs): a paste with the dialog's two answers -- what happens to the reference designators of the pasted footprints (`PASTE_MODE`; "keep" is what a plain paste does) and "Clear net assignments". */
  | { op: "paste_special"; text: string; at: PointXY; mode?: PcbPasteMode; clear_nets?: boolean }
  /** Run `cmd` with the board editor in the group `group` (the entered one): what it adds joins the group (`BOARD_COMMIT::Push`; crates/ops/src/pcb_groups.rs `Cmd::InGroup`). */
  | { op: "in_group"; group: string; cmd: Cmd }
  /** `A` (`BOARD_EDITOR_CONTROL::PlaceFootprint`): a footprint of a library (`Lib:Name`) as a part of its own at `at`. The server brings the pads from the installed libraries; `reference` / `value` empty take the library's usual prefix with the next free number and the footprint's name. See crates/ops/src/library_place.rs. `definition` is the footprint's own copy, for one the board cannot find by name (Insert footprint into PCB sends the Footprint Editor's). */
  | { op: "place_footprint"; footprint: string; at: PointXY; reference?: string; value?: string; definition?: LibraryFootprint | null }
  /** Change Footprint(s) / Update Footprint(s) from Library (`DIALOG_EXCHANGE_FOOTPRINTS`) and the footprint a symbol's Footprint field names (`replaceFootprint`): put `references` on `footprint`, or -- `footprint` absent -- each on the library copy of the footprint it is on. The pose, lock and group stay; the rest follows `options`. The server brings the library copy. See crates/ops/src/fp_exchange.rs. */
  | { op: "exchange_footprints"; references: string[]; footprint?: string | null; options?: ExchangeOptions }
  /** Geographical Reannotate (`DIALOG_BOARD_REANNOTATE`): the footprints renumbered by where they are, in the order, scope, prefixes and start numbers of `options`; `selection` is the footprints the "Selection" scope names. The symbols take the new references in the same step; one undo step. Refused when two footprints would be left with one reference. See crates/ops/src/reannotate.rs. */
  | { op: "reannotate_board"; options: ReannotateOptions; selection: string[] }
  /** Cmd+V: insert fresh copies of whole items (ids ignored/reassigned) -- the clipboard's own full data, not references, so paste still works after the original was deleted. */
  | { op: "paste_items"; tracks?: CmdTrack[]; vias?: CmdVia[]; zones?: CmdZone[]; shapes?: CmdShape[]; texts?: CmdText[] }
  /** Shift+M "Move Exactly...": translate every named part by the same (dx, dy), then rotate each by the same `rotate_millideg` around `pivot` (null = each part's own anchor -- a pure spin). */
  | { op: "move_exact"; parts: string[]; dx: Nm; dy: Nm; rotate_millideg: number; pivot: PointXY | null }
  /**
   * `EDIT_TOOL::Move` / `Rotate` / `Flip` of any mix of footprints (references), tracks, vias, zones, shapes, texts, dimensions and groups,
   * as one undo step each (crates/ops/src/pcb_transform.rs). `rotate_items`'s angle is positive CLOCKWISE on the screen, KiCad's runs the other
   * way; `kicad-port/pcbTransform.ts` plans all three.
   */
  | { op: "move_items"; ids: string[]; dx: Nm; dy: Nm }
  | { op: "rotate_items"; ids: string[]; pivot: PointXY; angle_millideg: number }
  | { op: "flip_items"; ids: string[]; pivot: PointXY; direction: "left_right" | "top_bottom" }
  /** `EDIT_TOOL::Mirror` of tracks and arcs (`PCB_TRACK::Mirror`): the points mirrored across the axis through `pivot`, in place (the id, the layer, the net and the group stay). */
  | { op: "mirror_tracks"; ids: string[]; pivot: PointXY; direction: "left_right" | "top_bottom" }
  /** `crates/ops/src/pcb_transform.rs` `mirror_items`: `PCB_TEXTBOX::Mirror` and `PCB_TUNING_PATTERN::Mirror` of text boxes and tuning patterns. */
  | { op: "mirror_items"; ids: string[]; pivot: PointXY; direction: "left_right" | "top_bottom" }
  /**
   * The Properties panel's own verbs (crates/ops/src/pcb_props.rs): a track's or arc's first and last point (a field left out stays), the net of tracks, vias and zones
   * (only a zone may be given ""), a zone's name, and a shape's new geometry in place (its id and lock stay). Every other row of the grid goes through the verbs above.
   */
  | { op: "edit_track"; id: string; start?: PointXY; end?: PointXY }
  | { op: "set_item_net"; ids: string[]; net: string }
  /** Track & Via Properties' net and its questions (crates/ops/src/pad_nets.rs `track_via_nets`): `net` null leaves every net; the answers left out make a question a refusal. */
  | { op: "track_via_nets"; ids: string[]; net: string | null; apply_anyway?: boolean; change_pads?: boolean }
  | { op: "set_zone_name"; id: string; name: string }
  | { op: "replace_shape"; id: string; shape: CmdShape }
  /**
   * The Footprint Properties and Pad Properties dialogs' OK, and the Properties panel's footprint, field and pad rows (crates/ops/src/fp_edit.rs): what is given
   * replaces what the footprint has -- the Reference layout, the Value layout, the whole list of user fields, the attributes (type, board only, exclude from
   * position files and BOM, DNP, exempt from the courtyard requirement); what is left out stays. `edit_board_pad` replaces the edit of one pad (an edit that
   * changes nothing puts the pad back as the library has it). `move_items`, `rotate_items` and `flip_items` take a field's id (`REF:Reference`).
   */
  | { op: "edit_board_footprint"; part: string; reference?: FieldLayoutCmd; value?: FieldLayoutCmd; fields?: UserFieldCmd[]; attrs?: FootprintAttrsCmd }
  | { op: "edit_board_field"; part: string; name: string; layout?: FieldLayoutCmd; text?: string }
  | { op: "edit_board_pad"; part: string; edit: PadEditCmd }
  /**
   * The 3D Models tab of Footprint Properties (crates/ops/src/fp_models.rs): `set_board_models` makes the footprint's whole list the one given (the dialog's OK; an empty list is a
   * footprint with no model); the others are the tab's Add, Remove and cell edits and the order of the rows, one at a time, by 0-based position (`edit_board_model` leaves a key
   * out to leave it). A bad position, an empty or illegal file name or a number the preview panel does not take is refused with its reason; a command that changes nothing is
   * refused too. One undo step each.
   */
  | { op: "set_board_models"; part: string; models: Model3dCmd[] }
  | { op: "add_board_model"; part: string; model: Model3dCmd; at?: number }
  | { op: "remove_board_model"; part: string; index: number }
  | { op: "move_board_model"; part: string; from: number; to: number }
  /** Migrate 3D Models' Replace (`DIALOG_MIGRATE_3D_MODELS::OnReplaceClick`): every model of every footprint on the board under one of the keys' file names takes the name it maps to, the placement as it was; one undo step. */
  | { op: "migrate_board_models"; replacements: Record<string, string> }
  | { op: "edit_board_model"; part: string; index: number; path?: string; offset?: [number, number, number]; scale?: [number, number, number]; rotate?: [number, number, number]; opacity?: number; show?: boolean }
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
  | { op: "move_symbol"; id: string; x: Nm; y: Nm; unit?: number }
  | { op: "drag_symbol"; id: string; x: Nm; y: Nm; attached_wire_endpoints: [number, number][]; unit?: number }
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
  | { op: "add_sch_line"; pts: PointXY[]; width_nm?: Nm }
  | { op: "delete_sch_line"; id: string }
  /** `S` (eeschema.InteractiveDrawing.drawSheet): a hierarchical sheet symbol; `file` is a bare `.kicad_sch` name (extension added when missing). */
  | { op: "add_sheet"; name: string; file: string; at: PointXY; size: [Nm, Nm] }
  /**
   * Import Sheet... / Draw Sheet from File (`SCH_DRAWING_TOOLS::ImportSheet` / `DrawSheet`, `LoadSheetFromFile`): the schematic file `source` (a path under the board's folder or an
   * absolute one; the server reads it) as a sheet `name` of `size` at `at` showing a copy of it under `file`, or -- with `as_sheet` off -- its items on the sheet in view carried to `at`.
   * The symbols are numbered anew (in `order`, by `options`) unless `keep_annotations`. Refused for a file that places itself, symbols that name no library, a name already used.
   */
  | {
      op: "import_sheet";
      source: string;
      as_sheet?: boolean;
      name?: string;
      file?: string;
      at: PointXY;
      size?: [Nm, Nm];
      keep_annotations?: boolean;
      order?: "y_then_x" | "x_then_y";
      options?: AnnotateOptions;
      /** Place as group: what comes in goes in a group named `group_name` (the file's name when empty); with `block` the group is linked to the design block. */
      as_group?: boolean;
      group_name?: string;
      /** The design block (`Lib:Name`) the file is the schematic of (Place Design Block). */
      block?: string;
    }
  /** Alt+S (eeschema.InteractiveEdit.swap): exchange the positions of two symbols/power symbols/labels/texts (and the orientation of two instances of one library symbol). */
  | { op: "swap_sch_items"; a: string; b: string }
  /** Ctrl+V / Ctrl+Shift+V / Ctrl+D (`SCH_EDITOR_CONTROL::Paste`): the `fragment` `POST /api/sch/clipboard/parse` returned, added to the sheet in view moved by (`dx`, `dy`); `mode` is Paste Special's reference-designator choice. One undo step. */
  | { op: "paste_sch"; fragment: unknown; dx?: Nm; dy?: Nm; mode?: "unique" | "keep" | "remove"; numbering?: { algo?: "first_free" | "sheet_x100" | "sheet_x1000"; start?: number; order?: "by_x" | "by_y" } }
  /** The schematic editor's other tool verbs (lock, break, convert text, shapes, sheet pins, ...) -- see api/schEditTypes.ts. */
  | ({ op: "sch_edit" } & SchEditCmd)
  /** Move, Drag, Rotate, Mirror and Align of every kind of schematic item (crates/ops/src/sch_move.rs) -- see api/schEditTypes.ts. */
  | ({ op: "sch_move" } & SchMoveCmd)
  /** The board's text boxes, reference images, points and hatch fills (crates/ops/src/pcb_items.rs) -- see api/pcbItemTypes.ts. */
  | ({ op: "pcb_item" } & PcbItemCmd)
  /** The microwave tools: a gap, a stub, an arc stub, a polygonal shape or an inductor as a footprint of its own (crates/ops/src/microwave.rs) -- see api/microwaveTypes.ts. */
  | ({ op: "microwave" } & MicrowaveCmd)
  | ({ op: "embedded" } & EmbeddedCmd)
  /** The board's tables (crates/ops/src/pcb_tables.rs): Draw Table, the Table Properties and Table Cell Properties dialogs, rows, columns, merged cells -- see api/tableTypes.ts. */
  | ({ op: "table" } & TableCmdWire)
  | ({ op: "sch_table" } & TableCmdWire)
  | { op: "add_label"; net: string; at: PointXY; kind: CmdLabelKind }
  | { op: "delete_label"; id: string }
  | { op: "add_sch_text"; content: string; at: PointXY; angle_millideg: number; size_nm: Nm }
  | { op: "delete_sch_text"; id: string }
  | { op: "add_power_symbol"; lib_id: string; at: PointXY; rot_millideg: number; net: string; pin: string; annotate?: AnnotateOptions }
  | { op: "delete_power_symbol"; id: string }
  // `unit`: which unit of a multi-unit symbol this placement is (omit for
  // 1, a single-unit part). Only refused if `(id, unit)` already exists --
  // placing `{ id: "U1", unit: 2 }` once "U1" unit 1 is already on the
  // sheet is how another unit of an existing, already-annotated part gets
  // added.
  | { op: "add_symbol"; id: string; lib_id: string; at: PointXY; rot_millideg: number; value: string; footprint: string; unit?: number; annotate?: AnnotateOptions }
  | { op: "edit_symbol_fields"; id: string; value?: string | null; footprint?: string | null; datasheet?: string | null }
  | { op: "rename_symbol"; id: string; new_id: string }
  /** `DIALOG_ANNOTATE` (`options`: scope, numbering algorithm, start number, recursive, regroup units; see hierarchyTypes.ts). */
  | { op: "annotate"; reset_existing: boolean; order?: "y_then_x" | "x_then_y"; ids?: string[]; options?: AnnotateOptions }
  /** `eeschema.EditorControl.setDNP` / `setExcludeFromBOM` / `setExcludeFromBoard` / `setExcludeFromSimulation` (`SCH_EDIT_TOOL::SetAttribute`): the attributes given are set on every unit of every reference in `ids`; one left out stays. */
  | { op: "set_symbol_attrs"; ids: string[]; dnp?: boolean | null; exclude_from_bom?: boolean | null; exclude_from_board?: boolean | null; exclude_from_sim?: boolean | null }
  /** `eeschema.EditorControl.editPageNumber`: the page number of the sheet placement `sheet` (its id); letters and digits, empty for "by place in the hierarchy".
   *  `path` (the sheet path, root down, ending in `sheet`): that path's page only, as KiCad keeps one per path of a sheet on a screen shown twice. */
  | { op: "set_sheet_page"; sheet: string; page: string; path?: string[] }
  /** `eeschema.Interactive.increment*`: the new text of a label (its net name) or a free text, by id. */
  | { op: "set_sch_item_text"; id: string; text: string }
  /** `eeschema.EditorControl.editSymbolLibraryLinks`: every symbol linked to a `from` library id is linked to the paired `to` id (`update_fields`: the Datasheet follows the library symbol). */
  | { op: "set_symbol_lib_ids"; changes: Array<[string, string]>; update_fields?: boolean }
  /** `eeschema.EditorControl.incrementAnnotations`: every reference with the start's letters and a number at least its own moves by `increment`. */
  | { op: "increment_annotations"; start: string; increment: number }
  /** `dialog_erc.cpp`'s "Exclude this violation" / un-exclude -- `(check, location)` keys exactly one `ErcViolation`, matching it byte-for-byte against the same `location` string GET /api/erc reported (see `ErcViolation.location`'s own doc for the shapes that can be). Refused server-side when `location` is empty -- nothing to key an exclusion on. */
  | { op: "add_erc_exclusion"; check: string; location: string; comment?: string; marker?: ErcMarker }
  /** `sheet`: the sheet path of the finding's marker, so that deleting one placement's exclusion leaves the other placements' of a shared screen. */
  | { op: "delete_erc_exclusion"; check: string; location: string; sheet?: string[] }
  | { op: "clear_erc_exclusions" }
  | { op: "set_connection_grid"; mils: number }
  | { op: "set_intersheet_refs"; settings: IntersheetRefs }
  // The SPICE simulator's settings (`NGSPICE_SETTINGS`, the project's `schematic.ngspice`) and the workbook's text (crates/ops/src/simulator.rs).
  | { op: "set_simulator"; ngspice: { workbook_filename: string; fix_include_paths: boolean; model_mode: number } | null; workbook: string | null }
  /** Symbol Fields Table's Apply (dialog_symbol_fields_table.cpp) -- ONE verb for the whole staged batch so a single undo reverts it all. Applied remove -> rename -> add -> edits; edits address the FINAL field names. */
  | { op: "set_symbol_fields"; edits: SymbolFieldEdit[]; add_fields?: string[]; rename_fields?: SymbolFieldRename[]; remove_fields?: string[] }
  /** Find and Replace's Replace / Replace All (sch_find_replace_tool.cpp): `items` = `FindMatch.key`s to restrict to (one = "Replace"), omitted/null = "Replace All". */
  | { op: "replace_text"; search: SchSearchData; items?: string[] | null; /** Replace All over every sheet (`SCH_FIND_REPLACE_TOOL::ReplaceAll` without "Search the current sheet only"). */ all_sheets?: boolean }
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
  | { op: "move_pad"; footprint: string; id: string; x: Nm; y: Nm }
  | { op: "rotate_pad"; footprint: string; id: string; quarter_turns: number }
  | { op: "delete_pad"; footprint: string; id: string }
  | { op: "edit_pad"; footprint: string; id: string; pad: LibraryPad }
  | { op: "push_pad_properties"; footprint: string; source_pad_id: string; filter_shape: boolean; filter_orientation: boolean; filter_layers: boolean; filter_type: boolean }
  | { op: "renumber_pads"; footprint: string; start: number; prefix: string; step: number }
  /** `PAD_TOOL::EnumeratePads`' commit: `[pad id, new number]` pairs, applied together. */
  | { op: "set_pad_numbers"; footprint: string; numbers: [string, string][] }
  | { op: "add_footprint_graphic"; footprint: string; shape: CmdShape }
  | { op: "delete_footprint_graphic"; footprint: string; id: string }
  | { op: "move_footprint_graphic"; footprint: string; id: string; dx: Nm; dy: Nm }
  | { op: "edit_footprint_graphic"; footprint: string; id: string; layer: string; stroke_width: Nm; filled: boolean }
  | { op: "add_footprint_text"; footprint: string; text: CmdText }
  | { op: "edit_footprint_text"; footprint: string; id: string; content: string; angle: number; layer: string; size_nm: Nm; stroke_width: Nm; justify: TextJustify; mirror: boolean }
  | { op: "delete_footprint_text"; footprint: string; id: string }
  | { op: "move_footprint_text"; footprint: string; id: string; x: Nm; y: Nm }
  /** `pcbnew.Array.createArray` in the Footprint Editor (`ARRAY_TOOL::CreateArray`, `m_isFootprintEditor`): `numbering` renumbers the pads of the array; left out, the copies keep their source's number. */
  | { op: "create_footprint_array"; footprint: string; ids: string[]; geometry: ArrayGeometry; arrange?: boolean; numbering?: PadNumbering | null }
  /** `pcbnew.PadTool.explodePad` / `recombinePad` (Ctrl+E), `PAD_TOOL::EditPad`: a custom pad's basic shapes as free graphics of the footprint on the pad's layer, and the way back. One undo step each. */
  | { op: "explode_pad"; footprint: string; id: string }
  | { op: "recombine_pad"; footprint: string }

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
  /** Shape Properties: the width (negative: no border), the fill and -- when given -- the unit and the body style the shape is on (0: common to all). */
  | { op: "edit_symbol_graphic"; lib_id: string; id: string; stroke_mm: Mm; fill: LibraryFill; unit?: number; body_style?: number }
  | { op: "edit_symbol_text"; lib_id: string; id: string; text: string; angle_deg: Degrees; size_mm: Mm }
  /** A text box's properties dialog: the whole graphic, of the same kind, as the dialog left it. */
  | { op: "replace_symbol_graphic"; lib_id: string; id: string; graphic: LibrarySymbolGraphic }
  // ---------------------------------------------------------------- derived symbols
  /** `deriveFromExistingSymbol` (`DIALOG_LIB_NEW_SYMBOL` with a parent): `lib_id`, a new symbol in the parent's library, derived from `parent`. */
  | { op: "derive_symbol"; parent: string; lib_id: string; keep_footprint?: boolean; keep_datasheet?: boolean; transfer_user_fields?: boolean; keep_content_user_fields?: boolean; overwrite?: boolean }
  /** `flattenSymbol`: the derived symbol becomes the flat symbol it stands for. */
  | { op: "flatten_symbol"; lib_id: string }
  /** `updateSymbolFields` (`DIALOG_UPDATE_SYMBOL_FIELDS`): the named fields of a derived symbol follow its parent's. `reset_*` default to true, `remove_extras` and `reset_empty` to false. */
  | { op: "update_symbol_fields"; lib_id: string; fields: string[]; remove_extras?: boolean; reset_empty?: boolean; reset_text?: boolean; reset_visibility?: boolean; reset_effects?: boolean; reset_positions?: boolean }
  /** Symbol Properties' "Derive from" choice. */
  | { op: "set_symbol_parent"; lib_id: string; parent: string }
  /** Symbol Properties' fields grid, all at once. */
  | { op: "edit_library_symbol_fields"; lib_id: string; fields: LibrarySymbolField[] };

/** crates/model/src/ir.rs `LabelKind`, `#[serde(tag = "scope")]` -- for `add_label` only (`SchematicLabel`'s own `scope`/`shape` pair is the read-side mirror of this). */
export type CmdLabelKind = { scope: "local" } | { scope: "global"; shape: LabelShape } | { scope: "hierarchical"; shape: LabelShape };

export interface CmdReply {
  ok: boolean;
  message: string;
}

/**
 * POST /api/check_rules (crates/cli/src/kicad_engine.rs `check_rules`): kicad-cli's verdict on a custom-rules (`.kicad_dru`) text. kicad-cli
 * has no checker of its own and drops a rules file it cannot parse without saying so, so the backend loads the text on a probe board and,
 * when it does not load, finds the first top-level form that stops it. `ok: false` = the check itself could not run (`message` says why).
 */
export interface RulesCheckReply {
  ok: boolean;
  message: string;
  engine?: string;
  /** Every rule in the text loaded. */
  valid?: boolean;
  /** How many top-level forms the text has (the `(version 1)` header is one). */
  forms?: number;
  /** When the text does not load: the first form kicad-cli refuses (position among the forms from 0, its first line from 1, its text cut at 400 characters). */
  bad?: { index: number; line: number; text: string } | null;
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
  pts: [Nm, Nm][];
}

export interface RouteDisplacedLine {
  source_track: string | null;
  layer: string;
  pts: [Nm, Nm][];
}

/** `Mode::Shove` only: a via the live preview would push aside -- no
 * diameter/drill of its own (the board still carries the real via, by
 * `source_via`, until a commit actually replaces it; the frontend looks
 * those up from `BoardState.routing.vias` itself rather than this
 * repeating them over the wire). */
export interface RouteDisplacedVia {
  source_via: string;
  x: Nm;
  y: Nm;
}

export interface RouteVia {
  x: Nm;
  y: Nm;
  diameter: Nm;
  drill: Nm;
}

/** The shape every one of start/move/fix's successful replies carries
 * (fix nests it under `preview`; see `RouteFixReply`). */
export interface RoutePreview {
  ok: boolean;
  message?: string;
  net: string | null;
  /** `/api/route/start` only: what building the router's world had to say (`syncZone`: "%s is malformed." -- a rule area it left out). */
  warnings?: string[];
  /** The width the route is drawn at (`m_sizes.TrackWidth()`): what a start without a width took from the start net (`ImportSizes`). */
  width?: Nm | null;
  colliding: boolean;
  layer: string;
  /** The live, not-yet-fixed head: last fixed point (or the route's
   * origin) to the cursor, already resolved (walked/shoved/mark-obstacled)
   * server-side -- draw this directly, no client-side posture math needed. */
  head: [Nm, Nm][];
  /** Already-fixed runs from earlier in this session (e.g. before a via/
   * layer switch) -- empty until the route has more than one leg. */
  runs: RoutePreviewRun[];
  via: RouteVia | null;
  /** A same-net anchor near the cursor the route would snap onto and
   * finish at, if fixed now. */
  snapped_end: [Nm, Nm] | null;
  /** `Mode::Shove` only: other tracks this preview would push out of the
   * way if accepted. */
  displaced: RouteDisplacedLine[];
  /** `Mode::Shove` only: other vias this preview would push out of the way. */
  displaced_vias: RouteDisplacedVia[];
  /** `POST /api/route/continue_from_end` only (Route From Other End): the route fixed so far was committed, so the board changed. */
  committed?: boolean;
  /** `POST /api/route/continue_from_end` only: the far end of the connection, where the new route starts. */
  from?: [Nm, Nm];
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
  /** `drag_start` refused because the item taken hold of is locked (`ROUTER_TOOL::performDragging`: "The selected item is locked."): ask "Drag Anyway" and start again with `drag_locked`. */
  locked?: boolean;
  /** `drag_start` only: the board ids of the tracks of the line being dragged, which the cursor must not snap onto (`TOOL_BASE::checkSnap`). */
  line_tracks?: string[];
  /** What `DRAGGER::Start` grabbed: the whole segment (`DM_SEGMENT`, it slides sideways, its neighbours following at 45 degrees), one
   * vertex (`DM_CORNER`), a via (`DM_VIA`) or an arc track (`DM_ARC`: its curvature follows the cursor, tangent to the tracks next to it). */
  drag?: "segment" | "corner" | "via" | "arc";
  /** A `"via"` drag of a virtual via (`DRAGGER::checkVirtualVia`): the end of a segment where tracks meet with no pad or via -- the junction moves, no via is drawn or committed. */
  virtual_via?: boolean;
  colliding: boolean;
  /** The cursor is somewhere the drag cannot be resolved: `pts` is the last position that could (`DRAGGER::Drag`, "Restore last
   * solution"), and letting go commits that one. */
  restored?: boolean;
  /** The first position failed, so the drag only marks obstacles (`GetForceMarkObstaclesMode`): a violation is committed with Ctrl+click only. */
  forced_mark_obstacles?: boolean;
  /** The dragged item's own new shape: just its live position (1 point)
   * for a via, or N points for the whole stretched track (a corner or a segment drag). */
  pts: [Nm, Nm][];
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
  fanout: { layer: string; width: Nm; pts: [Nm, Nm][] }[];
}

/** A via of a pair (`PNS::VIA`, as `DIFF_PAIR_PLACER` places them): one per net, a `viaGap()` apart. */
export interface DpVia {
  net: string | null;
  x: Nm;
  y: Nm;
  diameter: Nm;
  drill: Nm;
  from_layer: string;
  to_layer: string;
}

/** `POST /api/route/dp_{start,move,fix,via,undo_segment,finish}` (`6`):
 * route two parallel, gap-matched lines (a differential pair) from one
 * session -- `eda_pns::dp_placer::DiffPairPlacer`, KiCad's
 * `DIFF_PAIR_PLACER`. Shares the same backend session as the route/drag
 * endpoints above; only one of a route, a drag, or a diff pair can be in
 * progress at once. The pair is coupled in all three router modes: walked
 * around what is in the way (`walkaround`), pushed through it with the
 * tracks and vias moved aside (`shove`), or only marked (`mark_obstacles`:
 * `colliding` reports it). `V` (`dp_via`) ends the head in a pair of vias
 * and the route goes on from them on the other layer. */
export interface DiffPairPreview {
  ok: boolean;
  message?: string;
  /** The pair's P and N nets (`DpNetPair`: the one named `+` or `P` first). */
  net_a: string | null;
  net_b: string | null;
  layer: string;
  width: Nm;
  /** The edge-to-edge gap the pair is routed at. */
  gap?: Nm;
  /** The pair does not fit as drawn (`!m_fitOk`). */
  colliding: boolean;
  /** The live, not-yet-fixed head of each line -- already resolved
   * (coupled, walked or shoved per the mode, snapped to the real pad(s) at
   * either end) server-side, draw directly. */
  head_a: [Nm, Nm][];
  head_b: [Nm, Nm][];
  /** Already-fixed legs from earlier in this session, one per leg for each line, each on its own layer. */
  runs_a: RoutePreviewRun[];
  runs_b: RoutePreviewRun[];
  /** Both lines end on a coupled pair of pads, vias or track ends --
   * finishing now lands exactly on it. */
  snapped_end: boolean;
  /** `V` is on: the head ends in `vias` (one per net). */
  placing_via?: boolean;
  vias?: DpVia[];
  /** The vias of the fixed legs. */
  fixed_vias?: DpVia[];
  /** `shove` mode: the tracks and vias the head would push aside. */
  displaced?: RouteDisplacedLine[];
  displaced_vias?: RouteDisplacedVia[];
  /** `POST /api/route/dp_continue_from_end` only (Route From Other End): the pair fixed so far was committed, so the board changed. */
  committed?: boolean;
  /** `POST /api/route/dp_continue_from_end` only: the far end of the connection, where the new pair starts. */
  from?: [Nm, Nm];
}

export interface DpFixReply {
  ok: boolean;
  message?: string;
  blocked: boolean;
  real_end?: boolean;
  preview?: DiffPairPreview;
}

/** `7` / `8` / `9`: which `pcbnew.LengthTuner.*` the tool is running. */
export type TuneMode = "single" | "diffpair" | "skew";

/** `dialog_cleanup_tracks_and_vias_base.cpp`'s checkboxes -- see
 * `POST /api/cleanup_tracks/{preview,apply}` and
 * `components/CleanupTracksDialog.tsx`. Every field defaults `false`,
 * matching the base dialog's own ctor (no checkbox starts checked). */
export interface CleanupActions {
  delete_shorting: boolean;
  delete_redundant_vias: boolean;
  delete_dangling_vias: boolean;
  merge_segments: boolean;
  delete_dangling_tracks: boolean;
  delete_tracks_in_pads: boolean;
}

/** The checkboxes and the dialog's "Filter Items" box (kicad-port/cleanupFilters.ts; crates/connectivity/src/cleanup_filter.rs). A filter that is off has no key. */
export interface CleanupOptions extends CleanupActions {
  filter_net?: string;
  filter_netclass?: string;
  filter_layer?: string;
  /** "Selected items only": the selected ids, groups among them (the backend opens them). Present -- even empty -- is on. */
  selected_ids?: string[];
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
// list). Coordinates are integer board nm, same convention as everything
// else in BoardState.

export interface RatsnestEdge {
  net: string;
  from: [Nm, Nm];
  to: [Nm, Nm];
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
// GET /api/fill. Source of truth: crates/cli/src/fill_api.rs `report()`. The zone fill is
// KiCad's, made by `kicad-cli pcb drc --refill-zones --save-board` (B, "Fill All Zones") and kept
// beside design.json (`eda_kicad::zone_fills`); there is no other filler. Between fills the board
// shows the last one, as KiCad does with auto-refill off. A zone that was never filled, or was edited
// since, has none: `filled` is false, there are no fragments and the canvas draws its outline
// (`pcbnew.ZoneFiller.zoneFillAll`/`zoneUnfillAll`, B/Ctrl+B, zone_filler_tool.cpp -- see
// state/store.tsx's `zoneFill`).

/** One zone's fill on one layer -- possibly several disjoint fragments (islands). Each fragment is a single closed ring, already "Fracture"d (slit at any hole), never a separate outer+holes pair. */
export interface FillZone {
  id: string;
  net: string;
  layer: string;
  /** The zone has a fill that is current: it was filled by kicad-cli and not edited since. False: no copper yet, drawn as an outline. */
  filled: boolean;
  area_nm2: number;
  fragments: [Nm, Nm][][];
  /** Only with `GET /api/fill?polys=1`: the same fill unfractured, one outline and its holes per island (what the triangulation display needs). */
  polys?: { outline: [Nm, Nm][]; holes: [Nm, Nm][][] }[];
}

/** Whose fill the board holds: `null` when no zone has ever been filled (or the fills were all taken away). */
export interface FillSource {
  /** "kicad-cli 10.99.0", or "the .kicad_pcb that was imported" for the fills a file was saved with. */
  engine: string;
  /** The design revision (`/api/version`) the run started from. */
  revision: string;
  /** How long kicad-cli took. */
  seconds: number;
  /** Zones with a fill that is current, and zones with one that was edited since (they show as outlines). */
  zones: number;
  stale: number;
}

export interface FillReport {
  zones: FillZone[];
  source: FillSource | null;
}

/** POST /api/fill/kicad: kicad-cli's filler ran (`ok`), or could not (`message`). */
export interface KicadFillReply {
  ok: boolean;
  message?: string;
  engine?: string;
  seconds?: number;
  layers?: number;
  rings?: number;
  zones?: number;
  revision?: string;
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
  /** The net the pin is on (the netlist: wires, labels, power symbols), absent for a pin on none. */
  net?: string | null;
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

/** Plain millimetres (a library symbol's own native unit, not this file's usual Nm) -- the coordinator's contract description names pin length "length_mm" specifically; kept as a distinct alias so every place that touches library-symbol geometry is grep-able. */
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
  /** `(pin_names (hide yes))`: no pin name of the symbol is shown. KiCad's default (and what a server that does not say means) is shown. */
  pin_names_hidden?: boolean;
  /** `(pin_numbers (hide yes))`: no pin number of the symbol is shown. */
  pin_numbers_hidden?: boolean;
  /** `(pin_names (offset x))`, mm: names are written inside the body, from the pin's inner end on, when it is above zero; over the pin line when it is zero. KiCad's default is 0.508 mm (20 mils). */
  pin_name_offset?: Mm;
  /** `LIB_SYMBOL::GetBodyStyleCount`: 2 for a symbol with an alternate ("De Morgan") body style, whose items are then of `body_style` 1 or 2 (or 0, drawn in both); 1 (or absent) for one with a single body. */
  body_style_count?: number;
}

/** GET /api/schematic's `lib_symbols`: every distinct lib_id used on the sheet, keyed by that lib_id ("Device:R", "power:GND", ...). */
export type LibSymbols = Record<string, LibSymbol>;

/** One field of a symbol, power symbol or sheet as `GET /api/schematic` sends it: the text and where KiCad draws it on the sheet (`eda_engine::fields::PageField`). */
export interface SchField {
  /** `Reference`, `Value`, `Footprint`, `Datasheet` (the last two hidden unless shown) and, after them, the symbol's user-defined fields (hidden until shown); a sheet's `Sheetname`, `Sheetfile`. */
  name: string;
  text: string;
  /** What `${NAME}` in `text` stands for (`SCH_FIELD::GetShownText`); absent when it is `text` itself. A Reference never has one: the painter writes it with its unit letter. */
  shown?: string;
  /** The text's anchor on the sheet, nanometres. */
  at: [Nm, Nm];
  /** The text runs upward: a quarter turn counter-clockwise about the anchor. */
  vertical: boolean;
  /** How the text is justified against the anchor, in its own axes. */
  h: "left" | "center" | "right";
  v: "top" | "center" | "bottom";
  visible: boolean;
  /** The id the field is selected, moved and edited by (`fld:<owner>:<name>`, `eda_engine::fields_edit::field_id`); absent from a backend built before fields were items. */
  id?: string;
  /** The item that has the field: a symbol's reference (`#<unit>` after it for a unit but the first), a power symbol's or a sheet's id. */
  owner?: string;
  /** The size of the text (its height and width), nanometres; absent is the default, 50 mil. */
  size_nm?: Nm;
  bold?: boolean;
  italic?: boolean;
  /** `SCH_FIELD::IsNameShown`: the field is drawn as `Name: value`. */
  name_shown?: boolean;
  /** `SCH_FIELD::CanAutoplace`: false leaves the field where it is when its item's fields are autoplaced. */
  allow_autoplace?: boolean;
}

/** Whether Autoplace Fields put an item's fields where they are (`SCH_ITEM::GetFieldsAutoplaced`): null is not (or never did). */
export type FieldsAutoplaced = "auto" | "manual" | null;

export interface SchematicSymbol {
  /** Reference designator ("U1") -- the same id PCB parts use. */
  id: string;
  /** The reference this sheet's placement gives the symbol (`SCH_SYMBOL::GetRef( &sheetPath )`): `id` unless the screen is shown by more than one sheet. */
  reference?: string;
  /** Key into `Schematic.lib_symbols` ("Device:R"). Null, or present but missing from `lib_symbols`, both mean "no real graphics for this instance yet" -- painter.ts falls back to the generic box either way. */
  lib_id: string | null;
  at: [Nm, Nm];
  rot: Degrees;
  /** KiCad mirrors one axis at a time, never both at once as a single flag -- `(mirror x)` flips the symbol vertically (negates Y), `(mirror y)` flips it horizontally (negates X). Replaces an earlier, incomplete `mirrored: boolean` that could only ever express one of the two. */
  mirror: "x" | "y" | null;
  /** 1-based unit (gate) of a multi-unit symbol -- e.g. a quad op-amp, or ecc83-pp's three-triode ECC83. */
  unit: number;
  /** The body style the symbol is drawn in (`SCH_SYMBOL::GetBodyStyle`): 1 the normal one, 2 the alternate ("De Morgan") one of a library symbol that has it. */
  body_style: number;
  value: string | null;
  mpn: string | null;
  package: string | null;
  footprint: string | null;
  datasheet: string | null;
  pins: SchematicPin[];
  /** The places Swap Pins traded (`SwapPinGeometry`): pin number -> the number of the library pin whose place (position, direction, length) it has. Absent or null: every pin is where the library puts it. */
  pin_swaps?: Record<string, string> | null;
  /** `SCH_SYMBOL::GetDNP` (Do not Populate): the symbol is drawn with a cross over it and left out of the assembly. */
  dnp?: boolean;
  /** Exclude from Bill of Materials / from Board / from Simulation (`SetAttribute`). Absent from a backend built before they existed. */
  exclude_from_bom?: boolean;
  exclude_from_board?: boolean;
  exclude_from_sim?: boolean;
  /** Where its Reference, Value, ... are drawn (absent from a backend built before fields had positions: painter.ts places them by its own rule then). */
  fields?: SchField[];
  fields_autoplaced?: FieldsAutoplaced;
  /** What `${NAME}` in `value` and `footprint` stands for, for the symbols the state lists no `fields` of (a sheet read from a KiCad file); only the ones that differ. */
  shown?: { value?: string; footprint?: string };
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
  at: [Nm, Nm];
  rot: Degrees;
  net: string;
  pin: SchematicPin;
  /** The Value (its net name) where KiCad draws it, and the hidden Reference. */
  fields?: SchField[];
  fields_autoplaced?: FieldsAutoplaced;
}

export interface NoConnect {
  /** Stable id (`nc_xxxxxxxxxxxx`) -- for `delete_no_connect`. */
  id: string;
  at: [Nm, Nm];
}

/** An explicit junction (`SCH_JUNCTION`, the `J` tool): joins every wire passing through or ending at `at`. */
export interface SchJunction {
  id: string;
  at: [Nm, Nm];
  /** The diameter and colour Junction Properties set; absent or null is the default dot. */
  look?: JunctionLook | null;
}

/** A graphic polyline on the schematic's notes layer (`SCH_LINE` on `LAYER_NOTES`, the `I` tool): decoration, never part of a net. `width_nm` 0 is the default line width. */
export interface SchLine {
  id: string;
  pts: [Nm, Nm][];
  width_nm: Nm;
  /** The style and colour Line Properties set (the width is `width_nm`); absent or null is the default. */
  stroke?: SchStroke | null;
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
  pts: [Nm, Nm][];
  /** True for a bus wire (GAPS.md #20) -- KiCad's `LAYER_BUS` vs `LAYER_WIRE`, same shape either way. */
  bus: boolean;
  /** A bus's member nets (`D[0..3]` -> D0..D3, aliases and groups expanded) -- only sent for a bus wire; what the Unfold from Bus menu lists. */
  members?: string[];
  /** The stroke Wire/Bus Properties set (width, style, colour); absent when the wire has the default one. */
  stroke?: SchStroke;
  /** `SCH_DRAWING_TOOLS::findWireLabelDriverName`: the wire's connection's `LocalName()` on this sheet path when the net's driver is a local or a global label,
   *  absent otherwise (a power symbol, a hierarchical label, a pin, or no driver). A local or global label put down on the wire takes it with no dialog (r2 C-L5). */
  label_driver?: string;
}

/** A bus entry (`SCH_BUS_WIRE_ENTRY`, GAPS.md #20): a short diagonal stub tying one specific member net into a bus. `at` and `at + size` are its two endpoints -- which one is "the bus side" is never stored, only read off whichever endpoint lands on a bus wire. */
export interface BusEntry {
  id: string;
  at: [Nm, Nm];
  size: [Nm, Nm];
  /** The stroke Wire/Bus Properties set; absent or null is the default one. */
  stroke?: SchStroke | null;
}

export interface SchematicLabel {
  /** Stable id (`lbl_xxxxxxxxxxxx`) -- for `delete_label`. */
  id: string;
  /** The label's text. */
  net: string;
  /** The net the label is on as the tracer names it (`/sub/X`, `A_EN`), the wire's under it; absent or null for a label on no wire (r3 A3-M4). */
  net_name?: string | null;
  at: [Nm, Nm];
  scope: LabelScope;
  shape: LabelShape | null;
  /** Which way the text runs from the anchor once Rotate or Mirror has set it (`SCH_LABEL_BASE::GetSpinStyle`); absent or null reads off the wire that ends at the label. */
  spin?: "right" | "up" | "left" | "bottom" | null;
  /** The text's size (height and width), nanometres, and whether it is bold or italic (`EDA_TEXT`); absent is the default, 50 mil, regular. */
  size_nm?: Nm;
  bold?: boolean;
  italic?: boolean;
  /** The "Auto" check of a global or hierarchical label (`SCH_LABEL_BASE::AutoRotateOnPlacement`): the label takes the spin of what it is connected to when it is placed or moved. */
  auto_rotate?: boolean;
  /** The label's fields where `AutoplaceFields` puts them: a global label's Sheet References first, then its own (its net class, ...). */
  fields?: SchLabelField[];
}

/** One field of a label (`SCH_FIELD` of an `SCH_LABEL_BASE`), placed: `shown` is its text as drawn (a global label's Sheet References read as the pages its name is on). */
export interface SchLabelField {
  name: string;
  text: string;
  shown: string;
  visible: boolean;
  italic: boolean;
  /** The global label's Sheet References (`FIELD_T::INTERSHEET_REFS`): shown or hidden by Schematic Setup, not by the label. */
  intersheet_refs: boolean;
  at: [Nm, Nm];
  vertical: boolean;
  h: "left" | "center" | "right";
  size_nm: Nm;
}

/** Schematic Setup > Formatting > Inter-sheet References (`SCHEMATIC_SETTINGS::m_IntersheetRefs*`). */
export interface IntersheetRefs {
  show: boolean;
  own_page: boolean;
  short: boolean;
  prefix: string;
  suffix: string;
}

/**
 * `T`: free-standing text -- `crates/model/src/ir.rs`'s `SchematicText`, next to a PCB `BoardText` without a layer, a stroke width or a mirror flag.
 * `h` and `v` are how it is justified about its anchor in its own axes (`EDA_TEXT::GetHorizJustify` / `GetVertJustify`), `vertical` whether it runs up the sheet
 * (`ANGLE_VERTICAL`); an older backend sends none of them, which reads as left, bottom and along the sheet.
 */
export interface SchematicText {
  /** Stable id (`txt_xxxxxxxxxxxx`) -- for `delete_sch_text`. */
  id: string;
  content: string;
  /** What `${NAME}` in `content` stands for (`SCH_TEXT::GetShownText`); absent when it is `content` itself. */
  shown?: string;
  at: [Nm, Nm];
  angle: Degrees;
  size_nm: Nm;
  vertical?: boolean;
  h?: "left" | "center" | "right";
  v?: "top" | "center" | "bottom";
}

/** A title block as `GET /api/state` / `GET /api/schematic` report it (`TITLE_BLOCK`): `comments[0]` is KiCad's comment 1, up to nine, trailing empty ones dropped. */
export interface TitleBlock {
  title: string;
  date: string;
  rev: string;
  company: string;
  comments: string[];
  /** The values with the project's text variables in them expanded (`TITLE_BLOCK::TextVarResolver`), when that changes any of them: the drawing sheet shows these, the Page Settings dialog edits the stored ones above. */
  shown?: { title: string; date: string; rev: string; company: string; comments: string[] };
}

/** A document's paper (`PAGE_INFO`) as the state JSON reports it: `size_nm` is the resolved width and height in its orientation. */
export interface PageInfo {
  /** KiCad's name: A5, A4, A3, A2, A1, A0, A, B, C, D, E, USLetter, USLegal, USLedger or User. */
  paper: string;
  portrait: boolean;
  /** A User paper's width and height, nm. */
  user_size_nm: [Nm, Nm] | null;
  size_nm: [Nm, Nm] | null;
}

/** What `set_board_page` / `set_schematic_page` take for the paper: `PageSettings` of the model (no `size_nm`). */
export interface CmdPage {
  paper: string;
  portrait?: boolean;
  user_size_nm?: [Nm, Nm] | null;
}

/** A hierarchical sheet pin (`SCH_SHEET_PIN`) on a placed sheet's own border -- GAPS.md #6. Tied *by name only* to a hierarchical label of the same name inside the sheet's own file (see crates/kicad/src/erc.rs's `check_hierarchy` doc for why shape is never compared). */
export interface SheetPin {
  id: string;
  name: string;
  shape: LabelShape;
  at: [Nm, Nm];
  /** How the pin's text is set (Sheet Pin Properties; `SCH_SHEET_PIN` is an `EDA_TEXT`): absent is 50 mil, regular, upright. */
  size_nm?: Nm;
  bold?: boolean;
  italic?: boolean;
}

/** One child sheet placed directly on the schematic view currently being shown -- `GET /api/schematic`'s own `sheets` field, not the whole project's tree (see `sheet_path` for how deep the current view already is). */
export interface Sheet {
  id: string;
  name: string;
  file: string;
  /** The page number the user gave this placement (Edit Sheet Page Number); empty or absent when it is numbered by its place in the hierarchy. */
  page?: string;
  at: [Nm, Nm];
  size: [Nm, Nm];
  pins: SheetPin[];
  /** The sheet's name and file where KiCad's Autoplace Fields puts them. */
  fields?: SchField[];
  fields_autoplaced?: FieldsAutoplaced;
}

/** One step of the breadcrumb from the root down to the sheet `GET /api/schematic?sheet=...` actually returned -- empty for the root itself. */
export interface SheetPathEntry {
  id: string;
  name: string;
}

/** The paper a sheet is drawn on (KiCad's names, landscape, nanometres) -- A4 unless the sheet's title block says otherwise. */
export interface SchematicPaper {
  name: string;
  width_nm: Nm;
  height_nm: Nm;
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
  /** The sheet's paper (Page Settings); null/absent is A4 landscape. */
  page?: PageInfo | null;
  /** Bus entries (GAPS.md #20) -- see `BusEntry`'s own doc. */
  bus_entries: BusEntry[];
  /** Explicit junctions (`J`) -- see `SchJunction`. Absent from a backend built before they existed. */
  junctions?: SchJunction[];
  /** Graphic lines on the notes layer (`I`) -- see `SchLine`. Absent from a backend built before they existed. */
  lines?: SchLine[];
  /** Drawn shapes, text boxes, rule areas and directive labels -- see `SchGraphic` (api/schEditTypes.ts). Absent from a backend built before they existed. */
  graphics?: SchGraphic[];
  /** The tables of this sheet (`SCH_TABLE`, `crates/cli/src/table_json.rs` `sch_table_json`): the board's `BoardTable` shape with an empty layer. Absent from a backend built before sheet tables. */
  tables?: BoardTable[];
  /** Ids of locked items (Lock / Unlock). Absent from a backend built before locks existed. */
  locked?: string[];
  /** The groups of this sheet (`SCH_GROUP`): a named set of item ids and of other groups' ids. Absent from a backend built before schematic groups. */
  groups?: Group[];
  /** Child sheets placed directly on *this* view (GAPS.md #6) -- empty for a single-sheet design, or for a sheet with no children of its own. */
  sheets: Sheet[];
  /** The root-to-here breadcrumb for whichever sheet this response is actually showing (see `fetchSchematic`'s own `sheetPath` param) -- empty when showing the root. */
  sheet_path: SheetPathEntry[];
  /** The page of the sheet in view as a number (`SCH_SHEET_PATH::GetPageNumberAsInt`: the page when it is a number, else its place in the walk), which
   *  the "sheet number x 100 / x 1000" numbering of a placed symbol counts from; absent from an older backend (page 1). */
  page_number?: number;
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
  | { kind: "text"; id?: string; unit: number; body_style: number; text: string; at: MmPoint; angle_deg: Degrees; size_mm: Mm }
  /**
   * `SCH_TEXTBOX` of a library symbol (`(text_box ...)`): `start` and `end` are two opposite corners (Y up, mm), the text runs along the box (`angle_deg` 0) or up it (90) and is
   * wrapped to it. `margin_mm` 0 is the default margin (`kicad-port/symTextBox.ts`), a negative `stroke_mm` is no border, 0 the default line width. The server leaves out what is
   * false or zero (`bold`, `italic`, `margin_mm`).
   */
  | {
      kind: "text_box";
      id?: string;
      unit: number;
      body_style: number;
      text: string;
      start: MmPoint;
      end: MmPoint;
      angle_deg: Degrees;
      size_mm: Mm;
      bold?: boolean;
      italic?: boolean;
      h_align: FieldHAlign;
      v_align: FieldVAlign;
      margin_mm?: Mm;
      stroke_mm: Mm;
      fill: LibraryFill;
    };

/** `crates/model/src/ir.rs` `PinAlternate` -- one alternate function of a pin (`SCH_PIN::ALT`, `(alternate "name" type shape)`): a name, an electrical type and a graphic style. A pin keeps each name once, in the order of the names. */
export interface PinAlternate {
  name: string;
  electrical_type: PinElectricalType;
  shape: PinShape;
}

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
  /** The pin's alternate functions (`DIALOG_PIN_PROPERTIES`' "Alternate pin function definitions"); the server leaves the list out when it is empty. */
  alternates?: PinAlternate[];
}

/** `crates/model/src/sym_fields.rs` `FieldHAlign` / `FieldVAlign`: a field's `(justify ...)`. */
export type FieldHAlign = "left" | "center" | "right";
export type FieldVAlign = "top" | "center" | "bottom";

/**
 * `crates/model/src/sym_fields.rs` `LibrarySymbolField` -- one property of a library symbol (`SCH_FIELD` of a `LIB_SYMBOL`): `Reference`, `Value`, `Footprint`, `Datasheet`,
 * `Description` or a user field, with the place and look `(property "Name" "text" (at x y angle) ... (effects ...))` keeps. The server leaves out what has its default (an angle
 * of 0, a size of 1.27, nothing hidden, centred); `kicad-port/symFields.ts` puts it back.
 */
export interface LibrarySymbolField {
  name: string;
  text?: string;
  at?: MmPoint;
  angle_deg?: Degrees;
  size_mm?: Mm;
  hidden?: boolean;
  show_name?: boolean;
  do_not_autoplace?: boolean;
  bold?: boolean;
  italic?: boolean;
  h_align?: FieldHAlign;
  v_align?: FieldVAlign;
  /** `(property private ..)`: a field of the library symbol only (`SCH_FIELD::IsPrivate`). */
  private?: boolean;
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
  /** "All units are interchangeable" unchecked (`LIB_SYMBOL::UnitsLocked`, the file's `ki_locked`); only for a symbol of several units. Absent: false (in a command, kept). */
  units_locked?: boolean;
  /** "Define as local power symbol" (`(power local)`), with `power`. */
  power_local?: boolean;
  /** "Exclude from simulation". */
  exclude_from_sim?: boolean;
  /** "Exclude from position files". */
  exclude_from_pos_files?: boolean;
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
  /** `(extends "Parent")`: the `lib_id` of the symbol this one is derived from (`LIB_SYMBOL::SetParent`). A derived symbol keeps no graphics or pins of its own; `GET /api/symbol?flatten=1` gives it with everything it inherits. */
  extends?: string | null;
  /** The symbol's fields beyond the text members above (`crates/model/src/sym_fields.rs`): the layout of each field, `Value`, `Footprint` and the user fields. Empty for a symbol that never had one. */
  fields?: LibrarySymbolField[];
  kicad?: LibraryKeep;
}

/** `GET /api/symbol_editor/names`'s one field: every `lib_id` available to open (already-opened library entries, every lib_id the project's intent already resolved, and the small builtin catalog), sorted. */
export interface SymbolEditorNames {
  names: string[];
  /** The `lib_id`s that are entries of the project library itself (editable in place). Absent from an older backend. */
  project?: string[];
  /** The derived symbols of the project library, by `lib_id`, each with the `lib_id` of its parent (`(extends ...)`). */
  derived?: Record<string, string>;
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
// kicad-cli's own JSON; positions are this API's nm integers (every other
// endpoint here uses board-space nm, not kicad-cli's mm), as `[x, y]`.
// Our own checks, the ones KiCad does not have, are `LintReport` below.

export type DrcSeverity = "error" | "warning";

export interface DrcItem {
  description: string;
  /** Board-space nm, like every other position in this file -- *not* kicad-cli's own mm. */
  pos: [Nm, Nm];
  /** Our id for the referenced item (track/via/zone id, or `<ref>.<pad>`/`<ref>` for a footprint/pad; `outline` for the Edge.Cuts outline) -- enough to select it without re-matching on position. `null` for an item that isn't one of ours (e.g. a fill polygon KiCad computed). */
  id: string | null;
  /** The item's uuid in the derived board (kicad-cli's own name for it); what an exclusion is keyed on. Absent on a lint finding. */
  uuid?: string;
}

/** `crates/lint`'s agent-repair metadata for a placement finding: which part to move, toward what, how far. kicad-cli's report has no such thing, so only lint findings carry one. */
export interface DrcFix {
  /** Reference designator of the part a fix would move. */
  mover: string;
  /** What to move it toward: another part's reference, "board center", "nearest edge", or similar. */
  toward: string;
  distance_to_close_nm: Nm;
  /** Human-readable next step. */
  suggested_command: string;
}

export interface DrcViolation {
  /** KiCad's own DRC type name ("clearance", "courtyards_overlap", ...) for a kicad-cli violation; the check's own name ("placement_proximity", ...) for a lint finding. */
  type: string;
  description: string;
  /** The severity the check has -- a waived violation keeps the one it would have had (`excluded` says it is waived). */
  severity: DrcSeverity;
  items: DrcItem[];
  /** Lint findings only. */
  fix?: DrcFix | null;
  /** `RC_JSON::VIOLATION::excluded`: the violation is waived (`design.drawings.drc_exclusions`). Absent on a lint finding and on a report from an older server. */
  excluded?: boolean;
  /** Why it was waived (`PCB_MARKER::GetComment`); empty for none. */
  comment?: string;
  /** Whether kicad-cli itself matched the exclusion in the derived project. KiCad keys an exclusion by the marker's exact position, which a report does not give, so an exclusion the studio holds can be one kicad-cli's own report still lists. */
  kicad_matched?: boolean;
  /** Where the violation's marker may sit, in nanometres: the positions an exclusion keeps for the derived project (`eda_kicad_engine::marker_candidates`). */
  marker_nm?: Array<[number, number]>;
  /** The studio's own, never sent: the kind this row was sorted as when the DRC dialog last refiltered, kept while a marker waived or restored from
   *  its row stays where it was (`ValueChanged`, r2 D-E3; `rcItems.ts::listedIndexes`). */
  listed_as?: "error" | "warning" | "exclusion";
}

/** What `add_drc_exclusions` takes for one violation (`eda_model::ir::DrcExclusion`). */
export interface DrcExclusionSpec {
  /** The check's KiCad settings key (`DrcViolation.type`). */
  check: string;
  /** The uuids of the items in the derived board, main item first (`DrcItem.uuid`). */
  items: string[];
  /** Our ids for the same items (empty string for an item that is not one of ours). */
  ids?: string[];
  /** `DrcViolation.marker_nm`. */
  positions_nm?: Array<[number, number]>;
  comment?: string;
}

/** A check set to Ignore, which kicad-cli does not run (`ignored_checks` of its report): the Ignored Tests tab. */
export interface IgnoredCheck {
  key: string;
  description: string;
}

export interface DrcReport {
  violations: DrcViolation[];
  /** Violation count by `type`. */
  counts: Record<string, number>;
  /** "kicad-cli <version>". */
  engine?: string;
  /** The ratsnest kicad-cli reports as `unconnected_items`. */
  unconnected_items?: DrcViolation[];
  /** Whether kicad-cli's filler ran before the DRC ("Refill all zones before performing DRC"): on a copy of the board, the DRC then judging the board with that fill. Off by default: the DRC judges the fills the board holds. */
  zones_refilled_by_kicad?: boolean;
  /** The design revision this run started from: the stamp GET /api/version served at that moment. The report is out of date once the board's revision is another one (kicad-port/checkRevision.ts). Absent from a server that does not stamp. */
  revision?: string;
  /** `--schematic-parity`: where the board differs from the schematic (the Schematic Parity tab). Empty unless the test ran (`schematic_parity_run`). */
  schematic_parity?: DrcViolation[];
  /** Whether kicad-cli ran the parity test on this report. */
  schematic_parity_run?: boolean;
  /** Set when the test was asked for and kicad-cli could not run it, in its own words ("Schematic parity tests require a fully annotated schematic."). */
  schematic_parity_error?: string;
  /** The checks whose severity is Ignore (the Ignored Tests tab). */
  ignored_checks?: IgnoredCheck[];
  /** Set by Delete All Markers (`DIALOG_DRC::OnDeleteAllClick`, `m_drcRun = false`): the lists were emptied by hand, not by a clean board; Run DRC makes a report again. */
  markers_deleted?: boolean;
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

/** An ERC marker as the report offers it and `add_erc_exclusion` keeps it (`eda_model::ir::ErcMarker`). */
export interface ErcMarker {
  /** KiCad's name for the kind of finding (`ERCET_PIN_NOT_CONNECTED`, ...). */
  error_type: string;
  /** Where the marker is, in nanometres. */
  at_nm: [number, number];
  /** The uuids of the items it names in the derived schematic, main first. */
  items: string[];
  /** The sheet paths of the marker (the uuids from the root sheet down); absent when it has none. */
  sheet?: string[];
  main_sheet?: string[];
  aux_sheet?: string[];
}

export interface ErcViolation {
  /** KiCad's own ERC type name ("pin_not_connected", "wire_dangling", ...) for a kicad-cli violation; the readability check's own name ("schematic_wire_overlap", ...) for a lint finding. */
  check: string;
  severity: ErcSeverity;
  /** What the finding is when it is not excluded (`severity` folds the two together): `error` or `warning`. kicad-cli findings only. */
  base_severity?: "error" | "warning";
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
  /** Why it was waived (`SCH_MARKER::GetComment`); empty for none. kicad-cli findings only. */
  comment?: string;
  /** Whether kicad-cli itself matched the exclusion in the derived project (`erc.erc_exclusions`). An exclusion the studio holds can be one kicad-cli's own report still lists: its marker is not known to the letter. */
  kicad_matched?: boolean;
  /** The finding's marker as KiCad keeps an exclusion (`kiapi.schematic.ErcMarker`), when the report says enough to write it to the letter: what `add_erc_exclusion` keeps for the derived project. */
  marker?: ErcMarker;
  /** The sheet kicad-cli's report lists the finding under (`ERC_SHEET::uuid_path`). */
  sheet?: string;
}

export interface ErcReport {
  violations: ErcViolation[];
  /** Violation count by `check` (excluded ones are not counted). */
  counts: Record<string, number>;
  /** "kicad-cli <version>". */
  engine?: string;
  /** The design revision this run started from -- see `DrcReport.revision`. */
  revision?: string;
  /** The checks whose severity is Ignore (the Ignored Tests tab). */
  ignored_checks?: IgnoredCheck[];
  /** Set by Delete All Markers (`DIALOG_ERC::OnDeleteAllClick`, `m_ercRun = false`): see `DrcReport.markers_deleted`. */
  markers_deleted?: boolean;
}

/** What the checker dialogs' "Save..." asks for (`POST /api/erc/report`, `/api/drc/report`): the file type, the Show boxes and the units; the DRC dialog's own checkboxes too. */
export interface SavedReportRequest {
  /** "report": the text file, `.rpt` (`WriteTextReport`); "json": `.json` (`WriteJsonReport`). */
  format: "report" | "json";
  errors: boolean;
  warnings: boolean;
  exclusions: boolean;
  units: "mm" | "mil" | "in";
  refill_zones?: boolean;
  schematic_parity?: boolean;
}

/** The report kicad-cli wrote: the file name the dialog suggests (`ERC.rpt`, `DRC.json`) and its text. */
export interface SavedReportReply {
  ok: boolean;
  message?: string;
  file_name?: string;
  report?: string;
}

/** GET /api/sch/formatting: the settings of Schematic Setup > Formatting the design keeps. */
export interface SchFormattingReply {
  ok: boolean;
  message?: string;
  /** Connection grid spacing in mils: the design's own, else KiCad's default (50). */
  connection_grid_mils: number;
  min_connection_grid_mils: number;
  max_connection_grid_mils: number;
  /** True when the design stores a setting of its own. */
  custom: boolean;
  /** Inter-sheet References, KiCad's defaults when the design set none (an older backend sends none). */
  intersheet_refs?: IntersheetRefs;
}

/** GET /api/sch/erc_severities: the ERC severities Schematic Setup > Violation Severity edits. */
export interface ErcSeveritiesReply {
  ok: boolean;
  message?: string;
  /** ERC settings key -> severity, for the checks that differ from KiCad's default (plus the two library-link checks the derived project ignores unless the table says otherwise). */
  severities: Record<string, RuleSeverity>;
  /** True when the design stores a table of its own. */
  custom: boolean;
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
 * BoardStatisticsDialog.tsx reads. Lengths in nm, areas in nm². An unset
 * minimum is KiCad's own INT_MAX nm sentinel (2147483647 nm), so it
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
  x_nm: number;
  y_nm: number;
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
    width_nm: number;
    height_nm: number;
    area_nm2: number;
    front_copper_area_nm2: number;
    back_copper_area_nm2: number;
    front_courtyard_area_nm2: number;
    back_courtyard_area_nm2: number;
    front_density_pct: number;
    back_density_pct: number;
    min_track_width_nm: number;
    min_clearance_nm: number;
    min_drill_nm: number;
    thickness_nm: number;
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
  /** `BOM_PRESET::excludeDNP`: symbols marked Do not populate are left out of the table and of an export ("Include 'DNP' Symbols" off). Absent from a backend built before it existed. */
  exclude_dnp?: boolean;
  /** `BOM_PRESET::includeExcludedFromBOM`: symbols marked Exclude from BOM are listed ("Include 'Exclude from BOM' Symbols" on); they are never in an export. */
  include_excluded_from_bom?: boolean;
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
  /** The names of KiCad's built-in views (`BOM_PRESET::BuiltInPresets`), which a request can apply with `preset`. */
  presets?: string[];
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
  /** The sheet and the key (`<placement ids>#<key>`): a key alone names an item of one sheet, this one names it in the hierarchy. */
  uid?: string;
  /** The sheet the match is on: the placement ids from the root (`[]` the root). */
  sheet?: string[];
  sheet_name?: string;
  /** The page the sheet shows (`SCH_SHEET_PATH::GetPageNumber`, else its place in the hierarchy). */
  page?: string;
  /** The sheet's place in the page order: matches come sorted by it. */
  sheet_rank?: number;
  kind: "field" | "label" | "text" | "pin";
  /** Symbol reference / label id / text id that owns the match. */
  id: string;
  /** Field name or pin number (empty for labels/text). */
  name: string;
  /** World-space nm. */
  at: [number, number];
  text: string;
}

export interface FindReply {
  ok: boolean;
  message?: string;
  /** The page-order rank of the sheet that was in view: a Find with no cursor starts there. */
  current_rank?: number;
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

/** `POST /api/inspect/{clearance,constraints}` (Inspect > Clearance Resolution / Constraints Resolution, `BOARD_INSPECTION_TOOL`): the report pages in KiCad's
 * wording (HTML lines) with the winning constraint of each, as numbers (nanometres), and every rule it considered. Read-only: it shows which rule wins, it checks nothing. */
export interface InspectConsidered {
  rule: string;
  implicit: boolean;
  constraint: string;
  condition: string;
  applied: boolean;
  note: string;
}

export interface InspectResolution {
  constraint: string;
  layer: string | null;
  min: number | null;
  opt: number | null;
  max: number | null;
  rule: string;
  implicit: boolean;
  null: boolean;
  zone_connection: string | null;
  disallowed: boolean;
  considered: InspectConsidered[];
}

export interface InspectPage {
  title: string;
  lines: string[];
  /** A page that is one report per copper layer (the "Clearance" page of items that share several): the layer in the board file, its shown name, its lines. */
  per_layer: { layer: string; name: string; lines: string[] }[];
  resolutions: InspectResolution[];
}

export interface InspectReply {
  ok: boolean;
  /** Set when `ok` is false: the info bar message (`ShowInfoBarError`). */
  message?: string;
  title: string;
  /** The `.kicad_dru` did not compile: the report is on the implicit rules alone. */
  compile_error: string | null;
  pages: InspectPage[];
}

/** `POST /api/footprint/check` (the Footprint Editor's Footprint Checker, `DIALOG_FOOTPRINT_CHECKER`): the markers kicad-cli's DRC puts on a scratch board that holds the footprint
 * being edited -- courtyard, attribute, pad, shorting-pad, net-tie and silkscreen checks -- with the editor's own item ids on the items. */
export interface FpCheckMarker {
  /** The check, by the board DRC's key: `missing_courtyard`, `malformed_courtyard`, `shorting_items`, `padstack_invalid`, `footprint_type_mismatch`, ... */
  type: string;
  description: string;
  severity: "error" | "warning" | "exclusion" | "ignore";
  /** Where the marker is drawn, nanometres, the footprint's own frame. */
  at: [Nm, Nm];
  /** The items the violation names: kicad-cli's description, their position, and the editor's id (a pad, a graphic, a text) when it is one of the footprint's. */
  items: { description: string; pos: [Nm, Nm]; id: string | null }[];
}

export interface FpCheckReply {
  ok: boolean;
  message?: string;
  engine?: string;
  markers?: FpCheckMarker[];
  counts?: { errors: number; warnings: number };
}
