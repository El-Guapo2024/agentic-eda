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
export interface CmdZone {
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
  | { op: "delete_erc_exclusion"; check: string; location: string };

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
