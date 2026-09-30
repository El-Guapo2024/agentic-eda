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

export interface Zone {
  id: string;
  net: string;
  layer: string;
  outline: [Um, Um][];
}

export interface Routing {
  tracks: Track[];
  vias: Via[];
  zones: Zone[];
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
   * display-only track-width/via-size indicators. Optional: only
   * present once the backend serving this board has picked up the
   * `board_rules` field (crates/cli/src/studio.rs) -- older `eda`
   * binaries won't send it.
   */
  board_rules?: {
    track_width: Um;
    via_drill: Um;
    via_diameter: Um;
    clearance: Um;
  };
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
  | { op: "add_track"; net: string; layer: string; width: Um; pts: PointXY[] }
  | { op: "delete_track"; id: string }
  | { op: "set_track_width"; id: string; width: Um }
  | { op: "add_via"; net: string; x: Um; y: Um; drill: Um; diameter: Um; from_layer: string; to_layer: string }
  | { op: "delete_via"; id: string }
  | { op: "move_via"; id: string; x: Um; y: Um }
  | { op: "add_zone"; net: string; layer: string; outline: PointXY[] }
  | { op: "delete_zone"; id: string }
  | { op: "add_shape"; shape: CmdShape }
  | { op: "delete_shape"; id: string }
  | { op: "move_shape"; id: string; dx: Um; dy: Um }
  | { op: "add_text"; text: CmdText }
  | { op: "edit_text"; id: string; content: string; angle: number; layer: string; size_um: Um; stroke_width: Um; justify: TextJustify; mirror: boolean }
  | { op: "delete_text"; id: string }
  | { op: "move_text"; id: string; x: Um; y: Um };

export interface CmdReply {
  ok: boolean;
  message: string;
}

export interface RouteReply {
  ok: boolean;
  message: string;
}

// ---------------------------------------------------------------- Schematic
//
// GET /api/schematic. Source of truth: crates/cli/src/studio.rs
// `schematic_json()`, crates/model/src/ir.rs `SchematicSection`/
// `SymbolInstance`/`Wire`/`NetLabel`, crates/model/src/lib.rs `Pin`/`PinKind`.

export type PinKind = "power" | "ground" | "signal" | "passive" | "nc";

export interface SchematicPin {
  number: string;
  name: string | null;
  kind: PinKind;
}

export interface SchematicSymbol {
  /** Reference designator ("U1") -- the same id PCB parts use. */
  id: string;
  at: [Um, Um];
  rot: Degrees;
  mirrored: boolean;
  value: string | null;
  mpn: string | null;
  package: string | null;
  pins: SchematicPin[];
}

export interface SchematicWire {
  net: string;
  /** "REF.PIN" refs this wire lands on. */
  pins: string[];
  pts: [Um, Um][];
}

export interface SchematicLabel {
  net: string;
  at: [Um, Um];
}

export interface Schematic {
  symbols: SchematicSymbol[];
  wires: SchematicWire[];
  labels: SchematicLabel[];
}
