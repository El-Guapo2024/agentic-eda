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
  at: [Um, Um];
}

export type LabelScope = "local" | "global" | "hierarchical";
/** eeschema's LABEL_FLAG_SHAPE -- which outline the label's text sits inside. Meaningful for "global"/"hierarchical" only; a "local" label has no outline. */
export type LabelShape = "input" | "output" | "bidirectional" | "tri_state" | "passive";

export interface SchematicWire {
  net: string;
  /** "REF.PIN" refs this wire lands on. */
  pins: string[];
  pts: [Um, Um][];
}

export interface SchematicLabel {
  net: string;
  at: [Um, Um];
  scope: LabelScope;
  shape: LabelShape | null;
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
  title_block: TitleBlock | null;
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
