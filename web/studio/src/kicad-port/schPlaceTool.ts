// The schematic's placing tools with an item in the hand: `SCH_DRAWING_TOOLS::TwoClickPlace` (local, global and hierarchical labels, directive
// labels, texts) and `SCH_DRAWING_TOOLS::PlaceSymbol` with the power filter (Place Power), eeschema/tools/sch_drawing_tools.cpp at 8303b2ad.
//
// KiCad's order: the tool's first click -- or the tool's hotkey, which `PrimeTool`s a click where the cursor is, or the toolbar with "immediate
// actions" on -- makes the item: `createNewLabel` / `createNewText` open their dialog (a local or global label put down on a wire a label already
// names takes that name and opens none), `PlaceSymbol` opens the chooser. Then the item rides the cursor (`prepItemForPlacement`, `addSymbol`: a
// preview, selected) until the second click places it (`commit.Push( _( "Add Label" ) )`, `_( "Place Symbol" )`). The tool stays armed: the next
// label of a multi-line entry (`itemsToPlace`) rides at once, a power symbol chosen with "Place repeated copies" (`keepSymbol`) rides again, and
// otherwise the next click makes a new item.
//
// While the item rides, the tool's own loop answers the keys before anyone else (`evt->IsAction( ... )` in each loop):
//   * Esc and Undo drop the item and the tool stays (`cleanup()`); with nothing carried Esc leaves the tool, and so does Undo in `TwoClickPlace` (undoing
//     nothing). Delete drops the item too.
//   * The tool's own hotkey is a click (`isSyntheticClick`): it places the item.
//   * Another tool is refused: "Press <ESC> to cancel item creation." (`PlaceSymbol`: "... symbol creation."); the Move tool is ignored quietly.
//   * Rotate, Mirror, Properties and Autoplace Fields go to the edit tool, which acts on the selection -- the item -- and the preview follows.
//   * Increment changes a carried label's or text's text (`SCH_TOOL_BASE::Increment` on the selection).
//   * Duplicate, Repeat and Paste are refused (`wxBell()`), and so are Redo and the Change To conversions (`toLabel`, ...);
//     `PlaceSymbol` refuses Edit with Symbol Editor and Change Symbol.
//   * Anything else passes (zoom, pan, the grid ...).
//
// Rotate and Mirror of the one carried item are `SCH_EDIT_TOOL::Rotate` / `Mirror`'s single-item branches, which this file applies to the
// properties the placing command will carry -- the same rules `crates/ops/src/sch_move.rs` applies to a placed item:
//   * a label: `Rotate90( clockwise )` turns its spin (`LabelSpin::rotated`), `MirrorSpinStyle( !vertical )` mirrors it (`LabelSpin::mirrored`);
//   * a directive label: its pole turns a quarter, `MirrorSpinStyle( !vertical )` swaps the poles across the other axis (`mirror_orientation`);
//   * a text: `Rotate90` toggles along/up the sheet and flips the justification in two of the four cases (`SchTextJustify::rotated90`), the mirror
//     flips it along the text (`mirrored_spin`);
//   * a power symbol: `symbol->Rotate( pos, !clockwise )` a quarter about its anchor; a mirror is the turn of the picture the IR keeps for a power
//     symbol (a flip across the vertical axis leaves it, one across the horizontal axis turns it half a turn: `sch_move.rs`'s `Item::Power`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LabelScope, LabelShape } from "../api/types";
import type { AnnotateOptions } from "../api/hierarchyTypes";
import type { DirectiveShape, SchHAlign, SchVAlign } from "../api/schEditTypes";
import { placeLabelCmd } from "./schLabelName";
import { ATTRIBUTE_ACTIONS, type SymbolAttrKey } from "./schControl";
import type { LabelSpinName } from "./schLastPlaced";
import { incrementString } from "./stringIncrement";

/** The tools whose item rides the cursor, and the action that arms each. */
export const CARRY_TOOL_ACTIONS = {
  sch_label_local: "eeschema.InteractiveDrawing.placeLabel",
  sch_label_global: "eeschema.InteractiveDrawing.placeGlobalLabel",
  sch_label_hier: "eeschema.InteractiveDrawing.placeHierarchicalLabel",
  sch_directive: "eeschema.InteractiveDrawing.placeClassLabel",
  sch_text: "eeschema.InteractiveDrawing.placeSchematicText",
  sch_power: "eeschema.InteractiveDrawing.placePowerSymbol",
  sch_place_symbol: "eeschema.InteractiveDrawing.placeSymbol",
} as const;

export type CarryTool = keyof typeof CARRY_TOOL_ACTIONS;

export const isCarryTool = (tool: string): tool is CarryTool => Object.prototype.hasOwnProperty.call(CARRY_TOOL_ACTIONS, tool);

/** The label tool of each scope, and back. */
export const LABEL_TOOL_OF: Record<LabelScope, CarryTool> = { local: "sch_label_local", global: "sch_label_global", hierarchical: "sch_label_hier" };
export const scopeOfTool = (tool: CarryTool): LabelScope | null => (tool === "sch_label_local" ? "local" : tool === "sch_label_global" ? "global" : tool === "sch_label_hier" ? "hierarchical" : null);

export interface CarriedLabel {
  kind: "label";
  scope: LabelScope;
  text: string;
  /** Global and hierarchical labels only. */
  shape: LabelShape;
  /** "" is the studio's "From the wire": the spin is read off the wire the label ends at when it is placed. */
  spin: LabelSpinName | "";
  /** Null is the sheet's default text size. */
  sizeNm: number | null;
  bold: boolean;
  italic: boolean;
  /** The "Auto" check of a global or hierarchical label: the click that places it turns it to what it lands on (`AutoRotateItem`). */
  autoRotate: boolean;
}

export interface CarriedDirective {
  kind: "directive";
  netclass: string;
  componentClass: string;
  shape: DirectiveShape;
  /** The pole: 0 right, 90000 up, 180000 left, 270000 down. */
  orientation: number;
  pinLengthNm: number;
}

export interface CarriedText {
  kind: "text";
  content: string;
  sizeNm: number;
  /** Runs up the sheet (`ANGLE_VERTICAL`). */
  vertical: boolean;
  h: SchHAlign;
  v: SchVAlign;
}

export interface CarriedPower {
  kind: "power";
  libId: string;
  /** The net it puts its pin on: the symbol's Value. */
  net: string;
  rotMillideg: number;
  /** What its Properties (E) and Field Properties (U, V) did to it while it rode: their commands, by the id the server numbers it with (`edits` of a carried
   *  symbol, below), run after it is made in the same step. */
  edits?: Cmd[];
}

/** A quarter turn or a mirror of a carried symbol, applied after it is made as the placed symbol's own R / Shift+R / X / Y are (`SCH_EDIT_TOOL::Rotate` /
 *  `Mirror`'s single-symbol branch: about its anchor, `SetOrientation`). */
export type SymbolTurn = "rot_ccw" | "rot_cw" | "mirror_h" | "mirror_v";

export interface CarriedSymbol {
  kind: "symbol";
  libId: string;
  /** The reference the symbol rides with: `annotate()` numbered it when it was made, or the reference a further unit of a placed symbol goes in under. */
  ref: string;
  /** What the next symbol of "Place repeated copies" is numbered from. */
  prefix: string;
  value: string;
  footprint: string;
  unit: number;
  /** `GetUnitCount()`: how many units the library symbol has. */
  unitCount: number;
  /** The orientation it was made with (a copy of a placed unit keeps that unit's, `new SCH_SYMBOL( *symbol )`): degrees counter-clockwise, and the mirror. */
  rotDeg: number;
  mirror: "x" | "y" | null;
  /** The turns and mirrors made while it rides, in order. */
  turns: SymbolTurn[];
  /** The chooser's "Place all units" (`sel.PlaceAllUnits`): the next unit of the reference rides after each click. */
  placeAllUnits: boolean;
  /** `placeOneOnly`: the action was handed this symbol (Place Next Symbol Unit, Add Symbol to Schematic) -- placed once, and the tool ends. */
  oneOnly: boolean;
  /** The body style it is drawn in (Cycle Body Style or Symbol Properties' Body style while it rides): 1 or absent the normal one. */
  bodyStyle?: number;
  /** The attribute toggles made while it rides (`SCH_EDIT_TOOL::SetAttribute` on the selection, which is the symbol): DNP and the three exclusions. */
  attrs?: Partial<Record<SymbolAttrKey, boolean>>;
  /**
   * What Symbol Properties (E) and Field Properties (U, V, F) did to it while it rode (`PlaceSymbol` hands those actions to the edit tool, whose dialogs act on
   * the selection -- the symbol in the hand): the dialogs' own commands, naming it by the reference it rides with, run right after it is made, in the same step.
   */
  edits?: Cmd[];
}

export type CarriedItem = CarriedLabel | CarriedDirective | CarriedText | CarriedPower | CarriedSymbol;

/** What a placing tool holds. */
export interface Carry {
  tool: CarryTool;
  item: CarriedItem;
  /** The labels of a multi-line entry still to come, in order (`itemsToPlace`). */
  queue: CarriedItem[];
  /** Place Power's "Place repeated copies" (`sel.KeepSymbol`): the symbol rides again after each click. */
  keep: boolean;
}

/** Where the preview of a carried item is drawn on the server (`POST /api/sch/place_preview`): far off any sheet, so the item meets nothing there -- no
 * wire is cut, no label named, no junction added -- and the canvas shifts the drawing to the cursor. */
export const PREVIEW_AT: readonly [number, number] = [-254_000_000, -254_000_000];

// --------------------------------------------------------------------------------------------------------------------------- turning

/** `SPIN_STYLE::RotateCCW` / `RotateCW` (crates/model/src/sch_extras.rs `LabelSpin::rotated`). */
export function rotateSpin(s: LabelSpinName, ccw: boolean): LabelSpinName {
  const ring: LabelSpinName[] = ["right", "up", "left", "bottom"];
  const i = ring.indexOf(s);
  return ring[(i + (ccw ? 1 : 3)) % 4]!;
}

/** `SCH_LABEL_BASE::MirrorSpinStyle( aLeftRight )`: a left-right mirror swaps right and left, a top-bottom one up and bottom (`LabelSpin::mirrored`). */
export function mirrorSpin(s: LabelSpinName, leftRight: boolean): LabelSpinName {
  if (leftRight) return s === "right" ? "left" : s === "left" ? "right" : s;
  return s === "up" ? "bottom" : s === "bottom" ? "up" : s;
}

/** `mirror_orientation` (sch_move.rs): a directive label's pole, with `leftRight` swapping 0 and 180, else 90 and 270. */
export function mirrorPole(o: number, leftRight: boolean): number {
  const m = ((o % 360_000) + 360_000) % 360_000;
  if (leftRight) return m === 0 ? 180_000 : m === 180_000 ? 0 : m;
  return m === 90_000 ? 270_000 : m === 270_000 ? 90_000 : m;
}

const flipH = (h: SchHAlign): SchHAlign => (h === "left" ? "right" : h === "right" ? "left" : "center");

/** The item turned a quarter, counter-clockwise when `ccw` (R is `rotateCCW`, Shift+R `rotateCW`). */
export function rotateCarried(item: CarriedItem, ccw: boolean): CarriedItem {
  switch (item.kind) {
    case "label":
      // "textItem->Rotate90( clockwise )" on a label: its spin; "From the wire" is the right spin a label nothing is under has
      return { ...item, spin: rotateSpin(item.spin || "right", ccw) };
    case "directive":
      return { ...item, orientation: (item.orientation + (ccw ? 90_000 : 270_000)) % 360_000 };
    case "text": {
      // `SchTextJustify::rotated90( vertical, clockwise )`
      const clockwise = !ccw;
      const flip = (!item.vertical && clockwise) || (item.vertical && !clockwise);
      return { ...item, vertical: !item.vertical, h: flip ? flipH(item.h) : item.h };
    }
    case "power":
      return { ...item, rotMillideg: (item.rotMillideg + (ccw ? 90_000 : 270_000)) % 360_000 };
    case "symbol":
      // `symbol->Rotate( rotPoint, !clockwise )` about its anchor: the turn the placed symbol's R makes, sent after it is made
      return { ...item, turns: [...item.turns, ccw ? "rot_ccw" : "rot_cw"] };
  }
}

/** The item mirrored: `vertical` is Mirror Vertically (`mirrorV`, top and bottom trade places), else Mirror Horizontally (`mirrorH`). */
export function mirrorCarried(item: CarriedItem, vertical: boolean): CarriedItem {
  switch (item.kind) {
    case "label":
      return { ...item, spin: mirrorSpin(item.spin || "right", !vertical) };
    case "directive":
      // `SCH_DIRECTIVE_LABEL::MirrorSpinStyle( aLeftRight )` is `SCH_TEXT::MirrorSpinStyle( !aLeftRight )`: the pole is drawn across the text
      return { ...item, orientation: mirrorPole(item.orientation, vertical) };
    case "text": {
      // `MirrorSpinStyle( !vertical )` (`SchTextJustify::mirrored_spin`)
      const leftRight = !vertical;
      const flip = (!item.vertical && leftRight) || (item.vertical && !leftRight);
      return flip ? { ...item, h: flipH(item.h) } : item;
    }
    case "power":
      return vertical ? { ...item, rotMillideg: (item.rotMillideg + 180_000) % 360_000 } : item;
    case "symbol":
      // `symbol->SetOrientation( vertical ? SYM_MIRROR_X : SYM_MIRROR_Y )`
      return { ...item, turns: [...item.turns, vertical ? "mirror_v" : "mirror_h"] };
  }
}

/** `SCH_TOOL_BASE::Increment` on the carried item: a label's or a text's text steps by `delta` in its `index`-th part; a directive label and a symbol are not incrementable. */
export function incrementCarried(item: CarriedItem, delta: number, index: number): CarriedItem | null {
  if (item.kind === "label") {
    const next = incrementString(item.text, delta, index, { skipIOSQXZ: false });
    return next !== null && next !== item.text ? { ...item, text: next } : null;
  }
  if (item.kind === "text") {
    const next = incrementString(item.content, delta, index, { skipIOSQXZ: false });
    return next !== null && next !== item.content ? { ...item, content: next } : null;
  }
  return null;
}

// ------------------------------------------------------------------------------------------------------------------------- commands

export interface PlaceOptions {
  /** For a label whose spin is "From the wire": the spin read off the wire at the point it is placed. */
  plainSpin?: LabelSpinName | null;
  /** A power symbol is numbered by the project's Annotate method and start number (`PlaceSymbol`'s `annotate()`). */
  annotate?: AnnotateOptions;
  /** The preview: the click's `AutoRotateItem` is not run while the item rides. */
  preview?: boolean;
  /** The id the server numbers a carried power symbol with (its preview's): what the power symbol's `edits` name it by. */
  powerId?: string;
}

const xy = (p: readonly [number, number]) => ({ x: p[0], y: p[1] });

/** The one command the placing click commits -- the label, text and power verbs the dialogs always sent -- one undo step. */
export function placeCmd(item: CarriedItem, at: readonly [number, number], o: PlaceOptions = {}): Cmd {
  switch (item.kind) {
    case "label":
      return placeLabelCmd({ at, scope: item.scope, text: item.text, shape: item.shape, spin: item.spin, plainSpin: o.plainSpin ?? null, sizeNm: item.sizeNm, bold: item.bold, italic: item.italic, autoRotate: !o.preview && item.scope !== "local" && item.autoRotate });
    case "directive":
      return { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "directive", at: xy(at), orientation: item.orientation, shape: item.shape, pin_length_nm: item.pinLengthNm, netclass: item.netclass, component_class: item.componentClass } } };
    case "text":
      return { op: "sch_edit", verb: "place_text", content: item.content, at: xy(at), angle: item.vertical ? 90_000 : 0, size_nm: item.sizeNm, h: item.h, v: item.v };
    case "power": {
      const add: Cmd = { op: "add_power_symbol", lib_id: item.libId, at: xy(at), rot_millideg: item.rotMillideg, net: item.net, pin: "", ...(o.annotate ? { annotate: o.annotate } : {}) };
      // what its dialogs did while it rode, by the id it is numbered with (the server numbers it the same at the preview's point and at the click's)
      const edits = item.edits ?? [];
      const id = o.powerId;
      if (edits.length === 0 || !id) return add;
      const from = powerIdOfEdits(edits);
      return { op: "batch", cmds: [add, ...edits.map((c) => (from ? renameInCmd(c, from, id) : c))] };
    }
    case "symbol":
      return placeSymbolCmd(item, at, o);
  }
}

/** The power symbol id the first of `edits` names (`set_power_props`' `id`, a field id's owner). */
function powerIdOfEdits(edits: readonly Cmd[]): string | null {
  for (const c of edits) {
    const id = (c as { id?: unknown }).id;
    if (typeof id === "string" && id !== "") return id.startsWith("fld:") ? id.slice(4, id.lastIndexOf(":")) : id;
  }
  return null;
}

/**
 * `c` with every name of the item `from` made a name of `to`: the item's own id, a placed unit's key (`<from>#<unit>`), a field's id (`fld:<from>:<name>`,
 * `fld:<from>#<unit>:<name>`) and a pin's (`pin:<from>:<number>`). What the commands a dialog made on a carried symbol become when its reference changes
 * while it rides (`annotate()`'s answer, a rename in Symbol Properties).
 */
export function renameInCmd(c: Cmd, from: string, to: string): Cmd {
  if (from === to) return c;
  const name = (v: string): string => {
    if (v === from) return to;
    if (v.startsWith(`${from}#`)) return to + v.slice(from.length);
    for (const head of ["fld:", "pin:"]) if (v.startsWith(head + from) && (v[head.length + from.length] === ":" || v[head.length + from.length] === "#")) return head + to + v.slice(head.length + from.length);
    return v;
  };
  const walk = (v: unknown): unknown => (typeof v === "string" ? name(v) : Array.isArray(v) ? v.map(walk) : v && typeof v === "object" ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x)])) : v);
  return walk(c) as Cmd;
}

/**
 * What OK in a dialog opened on the carried symbol or power symbol does (Symbol Properties, a power symbol's Properties, Field Properties of one of its fields --
 * `PlaceSymbol` hands Properties and U / V / F to the edit tool, whose dialogs edit the selection, the symbol in the hand): `cmds` are the dialog's own
 * commands, by the name the item rides with. What the placing command makes the symbol with goes into the item -- the Value, the Footprint, the unit, the body
 * style, the attributes, a new reference (a rename; the commands kept are renamed with it), a power symbol's net -- and the rest is kept, to run right after it
 * is made, in the same step. A field edit's text that went into the item is taken out of it, as the field already has it then (the verb refuses an edit that
 * changes nothing).
 */
export function applyCarriedCmds(item: CarriedSymbol | CarriedPower, cmds: readonly Cmd[]): CarriedSymbol | CarriedPower {
  const flat = cmds.flatMap((c) => (c.op === "batch" ? c.cmds : [c]));
  const kept: Cmd[] = [];
  const fieldName = (id: unknown): string | null => (typeof id === "string" && id.startsWith("fld:") ? id.slice(id.lastIndexOf(":") + 1) : null);
  /** A field edit without its text, or nothing when the text was all it did. */
  const withoutText = (x: Record<string, unknown>): Cmd | null => {
    const rest = Object.fromEntries(Object.entries(x).filter(([k]) => k !== "text"));
    return Object.keys(rest).some((k) => k !== "op" && k !== "verb" && k !== "id") ? (rest as unknown as Cmd) : null;
  };
  if (item.kind === "power") {
    let net = item.net;
    for (const c of flat) {
      const x = c as unknown as Record<string, unknown>;
      if (c.op === "sch_edit" && x.verb === "edit_field" && typeof x.text === "string" && fieldName(x.id) === "Value") {
        // the Value is the net it asserts (`add_power_symbol`'s `net`)
        net = x.text.trim();
        const rest = withoutText(x);
        if (rest) kept.push(rest);
        continue;
      }
      kept.push(c);
    }
    return { ...item, net, edits: [...(item.edits ?? []), ...kept] };
  }
  const next: CarriedSymbol = { ...item };
  let renameTo: string | null = null;
  for (const c of flat) {
    const x = c as unknown as Record<string, unknown>;
    if (c.op === "rename_symbol") {
      renameTo = String(x.new_id);
      continue;
    }
    if (c.op === "edit_symbol_fields") {
      if (typeof x.value === "string") next.value = x.value;
      if (typeof x.footprint === "string") next.footprint = x.footprint;
      kept.push(c);
      continue;
    }
    if (c.op === "set_symbol_attrs") {
      const flags = Object.fromEntries(Object.entries(x).filter(([k, v]) => k !== "op" && k !== "ids" && typeof v === "boolean")) as Partial<Record<SymbolAttrKey, boolean>>;
      next.attrs = { ...next.attrs, ...flags };
      continue;
    }
    if (c.op === "sch_edit" && x.verb === "set_symbol_unit" && typeof x.new_unit === "number") {
      next.unit = x.new_unit;
      continue;
    }
    if (c.op === "sch_edit" && x.verb === "set_body_style" && typeof x.style === "number") {
      next.bodyStyle = x.style;
      continue;
    }
    if (c.op === "sch_edit" && x.verb === "edit_field" && typeof x.text === "string") {
      const name = fieldName(x.id);
      if (name === "Value" || name === "Footprint" || name === "Reference") {
        if (name === "Value") next.value = x.text;
        else if (name === "Footprint") next.footprint = x.text;
        else renameTo = x.text.trim();
        const rest = withoutText(x);
        if (rest) kept.push(rest);
        continue;
      }
    }
    kept.push(c);
  }
  let edits = [...(item.edits ?? []), ...kept];
  if (renameTo && renameTo !== item.ref) {
    const to = renameTo;
    edits = edits.map((c) => renameInCmd(c, item.ref, to));
    next.ref = to;
  }
  return { ...next, edits };
}

// ------------------------------------------------------------------------------------------------------------------- orientation

/** `crates/ops/src/sch_scene.rs` `Matrix` (KiCad's `TRANSFORM` of a placed symbol): `x' = x1*x + y1*y`, `y' = x2*x + y2*y`. */
interface OrientMatrix {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

const M_IDENTITY: OrientMatrix = { x1: 1, y1: 0, x2: 0, y2: 1 };
/** `SetOrientation( SYM_ROTATE_COUNTERCLOCKWISE )` / `SYM_ROTATE_CLOCKWISE`. */
const M_ROT_CCW: OrientMatrix = { x1: 0, y1: 1, x2: -1, y2: 0 };
const M_ROT_CW: OrientMatrix = { x1: 0, y1: -1, x2: 1, y2: 0 };
/** `SYM_MIRROR_Y` negates x ("Mirror Horizontally", the IR's `mirrored`, this file's mirror `"y"`); `SYM_MIRROR_X` negates y (`mirror_y`, `"x"`). */
const M_MIRROR_Y: OrientMatrix = { x1: -1, y1: 0, x2: 0, y2: 1 };
const M_MIRROR_X: OrientMatrix = { x1: 1, y1: 0, x2: 0, y2: -1 };

/** `SCH_SYMBOL::SetOrientation( incremental )`: the new matrix is `temp * old` (`Matrix::then`). */
function thenM(m: OrientMatrix, t: OrientMatrix): OrientMatrix {
  return { x1: m.x1 * t.x1 + m.x2 * t.y1, y1: m.y1 * t.x1 + m.y2 * t.y1, x2: m.x1 * t.x2 + m.x2 * t.y2, y2: m.y1 * t.x2 + m.y2 * t.y2 };
}

/** `Matrix::of`: the rotation first, then the mirror (`parseSymbol`). */
function matrixOf(rotMillideg: number, mirrored: boolean, mirrorY: boolean): OrientMatrix {
  const q = Math.trunc((Math.trunc(Math.trunc(rotMillideg / 1000) % 360) + 45) / 90) % 4;
  let m = q === 0 ? M_IDENTITY : q === 1 ? thenM(M_IDENTITY, M_ROT_CCW) : q === 2 ? thenM(thenM(M_IDENTITY, M_ROT_CCW), M_ROT_CCW) : thenM(M_IDENTITY, M_ROT_CW);
  if (mirrorY) m = thenM(m, M_MIRROR_X);
  if (mirrored) m = thenM(m, M_MIRROR_Y);
  return m;
}

/** `SCH_SYMBOL::GetOrientation` as `Matrix::orientation` reads it back: the first of KiCad's twelve orientations that gives the matrix. */
function orientationOf(m: OrientMatrix): { rotMillideg: number; mirrored: boolean; mirrorY: boolean } {
  const list: Array<[number, boolean, boolean]> = [
    [0, false, false],
    [90_000, false, false],
    [180_000, false, false],
    [270_000, false, false],
    [0, false, true],
    [90_000, false, true],
    [270_000, false, true],
    [0, true, false],
    [90_000, true, false],
    [180_000, true, false],
    [270_000, true, false],
  ];
  for (const [rot, mirrored, mirrorY] of list) {
    const o = matrixOf(rot, mirrored, mirrorY);
    if (o.x1 === m.x1 && o.y1 === m.y1 && o.x2 === m.x2 && o.y2 === m.y2) return { rotMillideg: rot, mirrored, mirrorY };
  }
  return { rotMillideg: 0, mirrored: false, mirrorY: false };
}

/**
 * Where the carried symbol's turns leave it: the orientation it was made with, then each quarter turn and mirror as the placed unit's own `sch_move` makes it
 * (`Matrix::of( .. ).then( ROT_CCW / ROT_CW / MIRROR_X / MIRROR_Y )`), read back as `SCH_SYMBOL::GetOrientation` reads it.
 */
export function symbolOrientation(item: Pick<CarriedSymbol, "rotDeg" | "mirror" | "turns">): { rotMillideg: number; mirror: "x" | "y" | null } {
  let m = matrixOf(Math.round(item.rotDeg * 1000), item.mirror === "y", item.mirror === "x");
  for (const t of item.turns) m = thenM(m, t === "rot_ccw" ? M_ROT_CCW : t === "rot_cw" ? M_ROT_CW : t === "mirror_v" ? M_MIRROR_X : M_MIRROR_Y);
  const o = orientationOf(m);
  return { rotMillideg: o.rotMillideg, mirror: o.mirrored ? "y" : o.mirrorY ? "x" : null };
}

/** The carried symbol with its turns folded into the orientation it is made with: what Autoplace Fields leaves of it while it rides (its fields placed anew). */
export function settledSymbol(item: CarriedSymbol): CarriedSymbol {
  const o = symbolOrientation(item);
  return { ...item, rotDeg: o.rotMillideg / 1000, mirror: o.mirror, turns: [] };
}

/**
 * The click that places a carried symbol (`commit.Push( _( "Place Symbol" ) )`): `add_symbol` with its reference, value, footprint and unit, the
 * orientation it was made with, then its turns and mirrors as the placed symbol's own verbs make them (`sch_move` on that one placed unit,
 * `<reference>#<unit>`) -- one command, one undo step. A symbol of an installed library brings its definition (the server's `EmbedLibSymbol`).
 */
export function placeSymbolCmd(item: CarriedSymbol, at: readonly [number, number], o: PlaceOptions = {}): Cmd {
  // The click (`if( m_frame->eeconfig()->m_AutoplaceFields.enable ) symbol->AutoplaceFields( screen, AUTOPLACE_AUTO )` before `commit.Push`): the symbol is made
  // in the orientation its turns left it in, so its fields are placed by Autoplace Fields there -- a mirror while it rode leaves them where the mirror put them
  // (`SetFieldsAutoplaced( AUTOPLACE_NONE )`) only until it is placed. The preview draws it as the turns leave it, fields and all (`PlaceSymbol`'s preview).
  // (with no turn it is made as it was given: a copy of a placed unit in that unit's own orientation, `new SCH_SYMBOL( *symbol )`)
  const settled = o.preview || item.turns.length === 0 ? item : settledSymbol(item);
  const add: Cmd = { op: "add_symbol", id: item.ref, lib_id: item.libId, at: xy(at), rot_millideg: Math.round(settled.rotDeg * 1000), value: item.value, footprint: item.footprint, unit: item.unit, ...(o.annotate ? { annotate: o.annotate } : {}) };
  const target = `${item.ref}#${item.unit}`;
  const cmds: Cmd[] = [add];
  // the IR's mirror flags: Mirror Horizontally (`mirrored`, negate x) is "y", Mirror Vertically (`mirror_y`) is "x"
  if (settled.mirror === "y") cmds.push({ op: "mirror_symbol", id: item.ref, unit: item.unit });
  else if (settled.mirror === "x") cmds.push({ op: "mirror_symbol_vertical", id: item.ref, unit: item.unit });
  for (const t of settled.turns) {
    const base = { op: "sch_move" as const, ids: [target], vertices: {} };
    cmds.push(t === "rot_ccw" || t === "rot_cw" ? { ...base, verb: "rotate", ccw: t === "rot_ccw" } : { ...base, verb: "mirror", vertical: t === "mirror_v" });
  }
  // what was done to it while it rode: the body style, the attributes, and the dialogs' commands
  if ((item.bodyStyle ?? 1) > 1) cmds.push({ op: "sch_edit", verb: "set_body_style", ids: [item.ref], style: item.bodyStyle } as unknown as Cmd);
  const attrs = Object.entries(item.attrs ?? {}).filter(([, v]) => v !== undefined);
  if (attrs.length > 0) cmds.push({ op: "set_symbol_attrs", ids: [item.ref], ...Object.fromEntries(attrs) } as Cmd);
  cmds.push(...(item.edits ?? []));
  return cmds.length === 1 ? add : { op: "batch", cmds };
}

/** What the units of a reference are, for the next symbol of "Place all units" (`unitOccupied`): is the unit placed? */
export interface NextSymbolContext {
  occupied: (unit: number) => boolean;
  /** `annotate()`: the reference a new symbol of "Place repeated copies" is numbered with. */
  nextRef: () => string;
}

/**
 * `PlaceSymbol` after a click placed `item` (`if( keepSymbol || placeAllUnits ) { ... }`): with "Place all units" the reference steps to its next unit
 * not placed (`while( currentReference.GetUnit() <= symbol->GetUnitCount() && unitOccupied( .. ) ) SetUnit( + 1 )`, back to 1 past the last); a next
 * symbol rides when "Place repeated copies" is on or a further unit is (`if( keepSymbol || currentReference.GetUnit() > 1 )`), a copy of the one placed
 * (`Duplicate`: its orientation and fields) in that unit, numbered anew when it is unit 1 (`if( currentReference.GetUnit() == 1 ) annotate();`).
 */
export function nextSymbolAfter(item: CarriedSymbol, keep: boolean, ctx: NextSymbolContext): CarriedSymbol | null {
  if (item.oneOnly || (!keep && !item.placeAllUnits)) return null;
  let unit = item.unit;
  if (item.placeAllUnits) {
    while (unit <= item.unitCount && ctx.occupied(unit)) unit++;
    if (unit > item.unitCount) unit = 1;
  }
  if (!keep && unit <= 1) return null;
  return { ...item, unit, ref: unit === 1 ? ctx.nextRef() : item.ref };
}

/** The command the server draws the riding item from (`POST /api/sch/place_preview`). */
export const previewCmd = (item: CarriedItem, powerId?: string): Cmd => placeCmd(item, PREVIEW_AT, { preview: true, powerId });

/** What the tool holds after the click that placed `carry`'s item: the next label of the entry, the same symbol again, or nothing (the next click makes one). */
export function afterPlaced(carry: Carry, symbols?: NextSymbolContext): Carry | null {
  // TwoClickPlace: "item = nullptr; ... else if( !itemsToPlace.empty() ) { item = itemsToPlace.front(); ... prepItemForPlacement( item, cursorPos ); }"
  if (carry.queue.length > 0) return { ...carry, item: carry.queue[0]!, queue: carry.queue.slice(1) };
  // PlaceSymbol: "if( keepSymbol || placeAllUnits ) ... nextSymbol = symbol->Duplicate(); addSymbol( nextSymbol ); annotate();"
  if (carry.item.kind === "symbol") {
    const next = symbols ? nextSymbolAfter(carry.item, carry.keep, symbols) : null;
    return next ? { ...carry, item: next } : null;
  }
  if (carry.tool === "sch_power" && carry.keep) return carry;
  return null;
}

/** The labels a dialog's entry makes: one, or one per non-empty line of a multi-line entry (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`: `wxSplit( text, '\n' )`, each trimmed). */
export function labelsOfEntry(base: CarriedLabel, text: string, multiLine: boolean): CarriedLabel[] {
  const lines = multiLine ? text.replace(/\r/g, "\n").split("\n") : [text];
  return lines.map((l) => (multiLine ? l.trim() : l)).filter((l) => l.trim() !== "").map((t) => ({ ...base, text: t }));
}

// ------------------------------------------------------------------------------------------------------------------------------ keys

export type CarryKey =
  /** The tool's own action: a click where the cursor is (`isSyntheticClick`). */
  | { do: "place" }
  | { do: "rotate"; ccw: boolean }
  | { do: "mirror"; vertical: boolean }
  /** Properties of the carried item: its dialog again (Symbol Properties for a symbol, a power symbol's Properties). */
  | { do: "properties" }
  /** U, V, F on a carried symbol or power symbol: Field Properties on that field of it (`SCH_EDIT_TOOL::EditField` on the selection). */
  | { do: "edit_field"; name: "Reference" | "Value" | "Footprint" }
  /** Autoplace Fields on a carried symbol: its fields go where Autoplace Fields puts them (`AutoplaceFields( screen, AUTOPLACE_MANUAL )` on the selection). */
  | { do: "autoplace" }
  /** Cycle Body Style on a carried symbol (`SCH_EDIT_TOOL::CycleBodyStyle` on the selection). */
  | { do: "body_style" }
  /** An attribute toggle on a carried symbol or power symbol (`SCH_EDIT_TOOL::SetAttribute` on the selection). */
  | { do: "attribute"; key: SymbolAttrKey }
  /** The right-button menu's Symbol Unit choice for the carried symbol (`ID_POPUP_SCH_SELECT_UNIT`: `m_frame->SelectUnit( symbol, unit )`). */
  | { do: "unit" }
  /** Esc, Undo, Delete: the item goes, the tool stays. */
  | { do: "drop" }
  | { do: "increment"; delta: number; index: number }
  /** `wxBell()`: nothing happens. */
  | { do: "refuse" }
  /** Another tool while an item rides: the message, and nothing else. */
  | { do: "busy"; message: string }
  /** Not the tool's: the action runs as it always does. */
  | { do: "pass" };

const INCREMENTS: Readonly<Record<string, { delta: number; index: number }>> = {
  "eeschema.Interactive.increment": { delta: 1, index: 0 },
  "eeschema.Interactive.incrementPrimary": { delta: 1, index: 0 },
  "eeschema.Interactive.decrementPrimary": { delta: -1, index: 0 },
  "eeschema.Interactive.incrementSecondary": { delta: 1, index: 1 },
  "eeschema.Interactive.decrementSecondary": { delta: -1, index: 1 },
};

/** `ACTIONS::duplicate`, `SCH_ACTIONS::repeatDrawItem`, `ACTIONS::paste` (and Paste Special, which is a paste), and `ACTIONS::redo`. */
const REFUSED = new Set(["common.Interactive.duplicate", "eeschema.InteractiveEdit.repeatDrawItem", "common.Interactive.paste", "common.Interactive.pasteSpecial", "common.Interactive.redo"]);
/** TwoClickPlace's `toDLabel` (named `toCLabel`), `toGLabel`, `toHLabel`, `toLabel`, `toText`, `toTextBox`. */
const CONVERSIONS = new Set(["eeschema.InteractiveEdit.toCLabel", "eeschema.InteractiveEdit.toGLabel", "eeschema.InteractiveEdit.toHLabel", "eeschema.InteractiveEdit.toLabel", "eeschema.InteractiveEdit.toText", "eeschema.InteractiveEdit.toTextBox"]);
/** PlaceSymbol's `editWithLibEdit`, `changeSymbol`. */
const SYMBOL_REFUSED = new Set(["eeschema.EditorControl.editWithSymbolEditor", "eeschema.InteractiveEdit.changeSymbol"]);
/** U, V, F: `SCH_ACTIONS::editReference`, `editValue`, `editFootprint`. */
const FIELD_EDITS: Readonly<Record<string, "Reference" | "Value" | "Footprint">> = {
  "eeschema.InteractiveEdit.editReference": "Reference",
  "eeschema.InteractiveEdit.editValue": "Value",
  "eeschema.InteractiveEdit.editFootprint": "Footprint",
};
/** The right-button menu's Symbol Unit entries (kicad-port/schSymbolMenus.ts `SELECT_UNIT_ACTION`). */
const SELECT_UNIT = "studio.SymbolUnit.select";

/**
 * What an action does while `tool` carries an item (the loops of `TwoClickPlace` and `PlaceSymbol`). `activates` is the action's `AF_ACTIVATE` flag
 * (`evt->IsActivate()`: it starts a tool).
 */
export function carryKey(tool: CarryTool, action: string, activates: boolean): CarryKey {
  const symbol = tool === "sch_power" || tool === "sch_place_symbol";
  // "evt->IsCancelInteractive() || evt->IsAction( &ACTIONS::undo )": cleanup(); `PlaceSymbol` checks Undo only with a symbol in hand, which is always here
  if (action === "common.Interactive.cancel" || action === "common.Interactive.undo") return { do: "drop" };
  if (activates) {
    if (action === CARRY_TOOL_ACTIONS[tool]) return { do: "place" };
    // "we're already moving our own item; ignore the move tool"
    if (action.includes("InteractiveMove")) return { do: "refuse" };
    return { do: "busy", message: symbol ? "Press <ESC> to cancel symbol creation." : "Press <ESC> to cancel item creation." };
  }
  if (REFUSED.has(action)) return { do: "refuse" };
  // "evt->IsAction( &ACTIONS::doDelete )": cleanup()
  if (action === "common.Interactive.delete") return { do: "drop" };
  if (action === "eeschema.InteractiveEdit.rotateCCW") return { do: "rotate", ccw: true };
  if (action === "eeschema.InteractiveEdit.rotateCW") return { do: "rotate", ccw: false };
  if (action === "eeschema.InteractiveEdit.mirrorV") return { do: "mirror", vertical: true };
  if (action === "eeschema.InteractiveEdit.mirrorH") return { do: "mirror", vertical: false };
  if (symbol) {
    if (SYMBOL_REFUSED.has(action)) return { do: "refuse" };
    // `PlaceSymbol` hands Properties, Edit Reference / Value / Footprint, Autoplace Fields, Cycle Body Style and the attribute toggles to the edit tool
    // (`m_toolMgr->PostAction( ACTIONS::refreshPreview ); evt->SetPassEvent();`), which acts on the selection: the symbol in the hand
    if (action === "eeschema.InteractiveEdit.properties") return { do: "properties" };
    const field = FIELD_EDITS[action];
    // a power symbol has no Footprint field to edit (`editFieldText` of a power symbol's Footprint: none)
    if (field) return tool === "sch_power" && field === "Footprint" ? { do: "refuse" } : { do: "edit_field", name: field };
    // a power symbol's fields are placed again by the preview; it has one body style
    if (action === "eeschema.InteractiveEdit.autoplaceFields") return tool === "sch_power" ? { do: "refuse" } : { do: "autoplace" };
    if (action === "eeschema.InteractiveEdit.toggleDeMorgan") return tool === "sch_power" ? { do: "refuse" } : { do: "body_style" };
    const attr = ATTRIBUTE_ACTIONS[action];
    if (attr) return { do: "attribute", key: attr };
    if (action === SELECT_UNIT && tool === "sch_place_symbol") return { do: "unit" };
    // a symbol is not incrementable: the edit tool's Increment finds nothing in the selection
    if (action in INCREMENTS) return { do: "refuse" };
    return { do: "pass" };
  }
  if (CONVERSIONS.has(action)) return { do: "refuse" };
  if (action === "eeschema.InteractiveEdit.properties") return { do: "properties" };
  // the fields of a carried item are placed by the preview already (`AutoplaceFields( nullptr, AUTOPLACE_AUTO )` on every motion)
  if (action === "eeschema.InteractiveEdit.autoplaceFields") return { do: "refuse" };
  const inc = INCREMENTS[action];
  if (inc) return { do: "increment", ...inc };
  return { do: "pass" };
}

/**
 * What an action does while `tool` is armed with nothing in the hand: `TwoClickPlace` takes Undo as it takes Esc -- "else { m_frame->PopTool( aEvent );
 * break; }", the event not passed on, so nothing is undone -- where `PlaceSymbol` checks Undo only with a symbol in hand and lets it through.
 */
export function armedKey(tool: CarryTool, action: string): "leave" | "pass" {
  return tool !== "sch_power" && tool !== "sch_place_symbol" && action === "common.Interactive.undo" ? "leave" : "pass";
}

// --------------------------------------------------------------------------------------------------------------------------- reading

/** What the test hook and the status bar say the tool holds. */
export function describeCarried(item: CarriedItem): Record<string, unknown> {
  switch (item.kind) {
    case "label":
      return { kind: "label", scope: item.scope, text: item.text, spin: item.spin || "right", shape: item.scope === "local" ? null : item.shape };
    case "directive":
      return { kind: "directive", netclass: item.netclass, orientation: item.orientation, shape: item.shape };
    case "text":
      return { kind: "text", text: item.content, vertical: item.vertical, h: item.h, v: item.v };
    case "power":
      return { kind: "power", libId: item.libId, net: item.net, rot: item.rotMillideg };
    case "symbol":
      return { kind: "symbol", libId: item.libId, ref: item.ref, unit: item.unit, turns: [...item.turns], placeAllUnits: item.placeAllUnits, oneOnly: item.oneOnly };
  }
}

/** A directive label's pole for a label spin (`createNewLabel` starts it with `m_lastTextOrientation`, the label tools' last spin). */
export const poleOfSpin = (s: LabelSpinName | ""): number => ({ right: 0, up: 90_000, left: 180_000, bottom: 270_000 })[s || "right"];
/** The label spin a directive label's pole leaves as the tools' last (`m_lastTextOrientation = labelItem->GetSpinStyle()`). */
export const spinOfPole = (o: number): LabelSpinName => (["right", "up", "left", "bottom"] as const)[Math.round((((o % 360_000) + 360_000) % 360_000) / 90_000) % 4]!;
