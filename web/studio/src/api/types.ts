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
  net: string;
  layer: string;
  width: Um;
  pts: [Um, Um][];
}

export interface Via {
  net: string;
  x: Um;
  y: Um;
  d: Um;
}

export interface Routing {
  tracks: Track[];
  vias: Via[];
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
  checks: Check[];
  /** Most recent 60 activity.jsonl entries, newest first. */
  activity: Activity[];
  /** "idle" | "running" | a one-line result of the last route. */
  job: string;
}

// ---------------------------------------------------------------- Cmd
//
// eda_ops::Cmd, `#[serde(tag = "op", rename_all = "snake_case")]`.
// Commands never carry raw coordinates except PlaceAt/MoveTo — see the
// module doc in crates/ops/src/lib.rs for why.

export type Dir = "north" | "south" | "east" | "west";
export type Region = "north_west" | "north" | "north_east" | "west" | "centre" | "east" | "south_west" | "south" | "south_east";

export type Cmd =
  | { op: "place"; part: string; anchor: string; side: Dir }
  | { op: "place_edge"; part: string; edge: Dir; fraction: number }
  | { op: "place_region"; part: string; region: Region }
  | { op: "place_at"; part: string; x: Um; y: Um }
  | { op: "move_to"; part: string; x: Um; y: Um }
  | { op: "nudge"; part: string; dir: Dir; steps: number }
  | { op: "rotate"; part: string; quarter_turns: number }
  | { op: "swap"; a: string; b: string }
  | { op: "rip"; part: string };

export interface CmdReply {
  ok: boolean;
  message: string;
}

export interface RouteReply {
  ok: boolean;
  message: string;
}
