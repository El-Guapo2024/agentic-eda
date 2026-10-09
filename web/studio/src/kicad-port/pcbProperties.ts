// The board editor's Properties panel: which properties each kind of board item has, and the commands that edit them.
//
// Ported from the `PROPERTY_MANAGER` registrations of pcbnew at KiCad 8303b2ad -- `BOARD_ITEM_DESC` (board_item.cpp), `BOARD_CONNECTED_ITEM_DESC`
// (board_connected_item.cpp), `FOOTPRINT_DESC`, `PAD_DESC`, `TRACK_VIA_DESC`, `ZONE_DESC`, `PCB_TEXT_DESC`, `EDA_TEXT_DESC`, `PCB_SHAPE_DESC`, `EDA_SHAPE_DESC`,
// `DIMENSION_DESC` and its five subclasses, `PCB_GROUP_DESC` -- with `PCB_PROPERTIES_PANEL` (pcbnew/widgets/pcb_properties_panel.cpp) supplying the layer and net
// choices (`updateLists`) and the way a value is set (`valueChanged`). The registrations keep KiCad's calls (`InheritsAfter`, `Mask`, `ReplaceProperty`,
// `OverrideAvailability` ...) so `propertyManager.ts` walks the same classes in the same order; what this model has no place for is not registered (a pad's
// shape and drill, a via's tenting and backdrill, text fonts and italics, footprint attributes ...: GAPS.md items 9 and 24).
//
// Every setter returns the commands of the verbs that already existed -- `move_items`, `rotate_items`, `flip_items`, `set_locked`, `set_track_width`,
// `edit_tracks_and_vias`, `edit_via`, `edit_zone`, `edit_shape`, `edit_text`, `edit_dimension`, `edit_group` -- or of the four the panel added (`edit_track`,
// `set_item_net`, `set_zone_name`, `replace_shape`; crates/ops/src/pcb_props.rs). A grid edit sends all of them as one `batch`: one undo step, KiCad's one
// `BOARD_COMMIT::Push( "Edit Properties" )`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, BoardText, Cmd, CmdDimension, Dimension, FieldInfo, Group, Pad, Part, PointXY, Shape, Track, Um, Via, Zone } from "../api/types";
import type { LengthUnit } from "../state/units";
import { toCmdDimension } from "./dimensionConvert";
import { editAttrsCmd, editFieldCmd, editPadCmd, fieldById, localAngleOf } from "./fpFields";
import { arcShapeCenter, padById } from "./pcbItems";
import { shapeToCmd } from "./pcbPointEdit";
import { circumcircle } from "./trackArc";
import { buildGrid, extractValueAndWritability, planEdit, positiveInt, rangeInt, tooSmall, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";

// ------------------------------------------------------------------------------------------------------------------------------- items

/** The class of a board item (`TYPE_HASH( *item )`). A track and an arc are two classes (`PCB_TRACK`, `PCB_ARC`), the dimension kinds five. */
export type PcbType =
  | "FOOTPRINT"
  | "PAD"
  | "PCB_TRACK"
  | "PCB_ARC"
  | "PCB_VIA"
  | "ZONE"
  | "PCB_TEXT"
  | "PCB_FIELD"
  | "PCB_SHAPE"
  | "PCB_DIM_ALIGNED"
  | "PCB_DIM_ORTHOGONAL"
  | "PCB_DIM_RADIAL"
  | "PCB_DIM_LEADER"
  | "PCB_DIM_CENTER"
  | "PCB_GROUP";

/** One selected board item: the class, the id the selection holds, and the record of the board state it stands for (only the one of its kind is set). */
export interface PcbItem {
  readonly type: PcbType;
  readonly id: string;
  readonly part?: Part;
  readonly pad?: Pad;
  /** A footprint's field (`PCB_FIELD`); `part` is its footprint. */
  readonly field?: FieldInfo;
  readonly track?: Track;
  readonly via?: Via;
  readonly zone?: Zone;
  readonly text?: BoardText;
  readonly shape?: Shape;
  readonly dim?: Dimension;
  readonly group?: Group;
}

/** What the getters read besides the item (`PCB_PROPERTIES_PANEL::updateLists` and the frame). */
export interface PcbCtx {
  board: BoardState;
  units: LengthUnit;
  locked: ReadonlySet<string>;
  /** Every net name, sorted without regard to case. */
  nets: readonly string[];
  /** The board's copper layers, front to back. */
  copperLayers: readonly string[];
  /** Copper, then the technical and user layers in `LSET::TechAndUserUIOrder`, then any other layer something on the board is drawn on. */
  allLayers: readonly string[];
}

/** `LSET::TechAndUserUIOrder`, the layers a board item can be drawn on besides copper. */
export const TECH_LAYERS: readonly string[] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

export function pcbContext(board: BoardState, units: LengthUnit): PcbCtx {
  const nets = new Set<string>();
  for (const p of board.parts) for (const pad of p.pads ?? []) if (pad.net) nets.add(pad.net);
  const rt = board.routing;
  for (const t of rt?.tracks ?? []) if (t.net) nets.add(t.net);
  for (const v of rt?.vias ?? []) if (v.net) nets.add(v.net);
  for (const z of rt?.zones ?? []) if (z.net) nets.add(z.net);
  const copper = board.layers;
  const all = [...copper, ...TECH_LAYERS];
  const used = [...(board.drawings?.shapes ?? []).map((s) => s.layer), ...(board.drawings?.texts ?? []).map((t) => t.layer), ...(board.drawings?.dimensions ?? []).map((d) => d.layer)];
  for (const l of used) if (!all.includes(l)) all.push(l);
  return {
    board,
    units,
    locked: new Set(board.locked ?? []),
    nets: [...nets].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" })),
    copperLayers: copper,
    allLayers: all,
  };
}

const DIM_TYPE: Record<Dimension["kind"], PcbType> = {
  aligned: "PCB_DIM_ALIGNED",
  orthogonal: "PCB_DIM_ORTHOGONAL",
  radial: "PCB_DIM_RADIAL",
  leader: "PCB_DIM_LEADER",
  center: "PCB_DIM_CENTER",
};

/** The board item a selection id names, or null. */
export function pcbItemOf(board: BoardState, id: string): PcbItem | null {
  const part = board.parts.find((p) => p.placed && p.ref === id);
  if (part) return { type: "FOOTPRINT", id, part };
  const rt = board.routing;
  const track = rt?.tracks.find((t) => t.id === id);
  if (track) return { type: track.arc_mid ? "PCB_ARC" : "PCB_TRACK", id, track };
  const via = rt?.vias.find((v) => v.id === id);
  if (via) return { type: "PCB_VIA", id, via };
  const zone = rt?.zones.find((z) => z.id === id);
  if (zone) return { type: "ZONE", id, zone };
  const dr = board.drawings;
  const shape = dr?.shapes.find((s) => s.id === id);
  if (shape) return { type: "PCB_SHAPE", id, shape };
  const text = dr?.texts.find((t) => t.id === id);
  if (text) return { type: "PCB_TEXT", id, text };
  const dim = dr?.dimensions.find((d) => d.id === id);
  if (dim) return { type: DIM_TYPE[dim.kind], id, dim };
  const group = dr?.groups.find((g) => g.id === id);
  if (group) return { type: "PCB_GROUP", id, group };
  const pad = id.includes(".") ? padById(board, id) : null;
  if (pad) return { type: "PAD", id, part: pad.part, pad: pad.pad };
  const field = id.includes(":") ? fieldById(board, id) : null;
  if (field) return { type: "PCB_FIELD", id, part: field.part, field: field.field };
  return null;
}

/** Every item of the board by id (the order of `pcbItemOf`'s lookups: the first kind to have an id names it), for a selection too big to search one id at a time. */
function indexOf(board: BoardState): Map<string, PcbItem> {
  const out = new Map<string, PcbItem>();
  const put = (item: PcbItem): void => void (out.has(item.id) || out.set(item.id, item));
  for (const part of board.parts) {
    if (!part.placed) continue;
    put({ type: "FOOTPRINT", id: part.ref, part });
    for (const field of part.fields ?? []) put({ type: "PCB_FIELD", id: field.id, part, field });
  }
  const rt = board.routing;
  for (const track of rt?.tracks ?? []) put({ type: track.arc_mid ? "PCB_ARC" : "PCB_TRACK", id: track.id, track });
  for (const via of rt?.vias ?? []) put({ type: "PCB_VIA", id: via.id, via });
  for (const zone of rt?.zones ?? []) put({ type: "ZONE", id: zone.id, zone });
  const dr = board.drawings;
  for (const shape of dr?.shapes ?? []) put({ type: "PCB_SHAPE", id: shape.id, shape });
  for (const text of dr?.texts ?? []) put({ type: "PCB_TEXT", id: text.id, text });
  for (const dim of dr?.dimensions ?? []) put({ type: DIM_TYPE[dim.kind], id: dim.id, dim });
  for (const group of dr?.groups ?? []) put({ type: "PCB_GROUP", id: group.id, group });
  return out;
}

/** The items of a selection, in selection order; an id the board does not have is skipped. */
export function pcbItemsOf(board: BoardState, ids: Iterable<string>): PcbItem[] {
  const list = [...ids];
  const out: PcbItem[] = [];
  // A big selection (Select All) is resolved through one index, not one search per id.
  const index = list.length > 12 ? indexOf(board) : null;
  for (const id of list) {
    const item = index ? (index.get(id) ?? (id.includes(".") ? pcbItemOf(board, id) : null)) : pcbItemOf(board, id);
    if (item) out.push(item);
  }
  return out;
}

const isCopper = (layer: string): boolean => layer.endsWith(".Cu");

/** `GetFriendlyName()`: the caption of a single selected item (`EDA_ITEM::GetFriendlyName`, `PCB_TRACK::`, `ZONE::`). */
export function pcbFriendlyName(item: PcbItem): string {
  switch (item.type) {
    case "FOOTPRINT":
      return "Footprint";
    case "PAD":
      return "Pad";
    case "PCB_TRACK":
      return "Track";
    case "PCB_ARC":
      return "Track (arc)";
    case "PCB_VIA":
      return "Via";
    case "ZONE": {
      const z = item.zone!;
      if (z.is_rule_area) return "Rule Area";
      if (z.teardrop) return "Teardrop Area";
      return isCopper(z.layer) ? "Copper Zone" : "Non-copper Zone";
    }
    case "PCB_TEXT":
      return "Text";
    case "PCB_FIELD":
      return "Field";
    case "PCB_SHAPE":
      return "Graphic";
    case "PCB_DIM_LEADER":
      return "Leader";
    case "PCB_GROUP":
      return "Group";
    default:
      return "Dimension";
  }
}

// ---------------------------------------------------------------------------------------------------------------------------- helpers

type PD = PropertyDef<PcbItem, PcbCtx, Cmd>;

const choicesOf = (names: readonly string[]): Choice[] => names.map((n) => ({ label: n, value: n }));
const norm360 = (a: number): number => ((a % 360) + 360) % 360;
const xy = (p: readonly [number, number]): PointXY => ({ x: p[0], y: p[1] });
const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const NO_NET = "<no net>";

/** The pose of a footprint: its position, KiCad's orientation (counter-clockwise on the screen; the model's `rot` runs the other way) and the side. */
const fpOrientation = (part: Part): number => norm360(-(part.rot ?? 0));

/** `ZONE_DESC`'s settings, as `edit_zone` takes them: every field of the zone with `patch` laid over it (the verb replaces the zone's settings wholesale). */
function zoneCmd(z: Zone, patch: Partial<Zone>): Cmd {
  const s = { ...z, ...patch };
  return {
    op: "edit_zone",
    id: z.id,
    net: s.net,
    layer: s.layer,
    clearance: s.clearance,
    min_thickness: s.min_thickness,
    thermal_gap: s.thermal_gap,
    thermal_spoke_width: s.thermal_spoke_width,
    pad_connection: s.pad_connection,
    priority: s.priority,
    island_removal_mode: s.island_removal_mode,
    min_island_area: s.min_island_area,
    fill_mode: s.fill_mode,
    hatch_thickness: s.hatch_thickness,
    hatch_gap: s.hatch_gap,
    hatch_orientation_mdeg: s.hatch_orientation_mdeg,
    hatch_smoothing_level: s.hatch_smoothing_level,
    hatch_smoothing_value: s.hatch_smoothing_value,
    hatch_hole_min_area: s.hatch_hole_min_area,
    hatch_border_algorithm: s.hatch_border_algorithm,
    is_rule_area: s.is_rule_area,
    keepout_tracks: s.keepout_tracks,
    keepout_vias: s.keepout_vias,
    keepout_pads: s.keepout_pads,
    keepout_copper_pour: s.keepout_copper_pour,
    keepout_footprints: s.keepout_footprints,
  };
}

/** `edit_text` takes every field of the text: the current ones with `patch` laid over them. */
function textCmd(t: BoardText, patch: Partial<{ content: string; angle: number; layer: string; size_um: Um; stroke_width: Um; justify: BoardText["justify"]; mirror: boolean }>): Cmd {
  return { op: "edit_text", id: t.id, content: t.content, angle: t.angle, layer: t.layer, size_um: t.size, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror, ...patch };
}

/** `edit_dimension` replaces the whole dimension: the current one with `patch` laid over it. */
function dimCmd(d: Dimension, patch: Partial<CmdDimension>): Cmd {
  return { op: "edit_dimension", id: d.id, dimension: { ...toCmdDimension(d), ...patch } };
}

const moveCmd = (ids: string[], dx: number, dy: number): Cmd[] => (dx === 0 && dy === 0 ? [] : [{ op: "move_items", ids, dx, dy }]);

/**
 * `BOARD_ITEM::GetPosition()` of an item (the same answer as `itemPosition` of `pcbItems.ts`, without searching the board for the id): a footprint's anchor, a pad's
 * centre, a track's start, a via's centre, a zone's first corner, a text's anchor, a dimension's first feature point, a polygon's first vertex, a circle's or an arc's
 * centre and any other shape's start.
 */
export function positionOf(i: PcbItem): [number, number] {
  if (i.field) return [i.field.x, i.field.y];
  if (i.pad) return [i.pad.x, i.pad.y];
  if (i.part) return i.part.at ? [i.part.at[0], i.part.at[1]] : [0, 0];
  if (i.track) return i.track.pts[0] ?? [0, 0];
  if (i.via) return [i.via.x, i.via.y];
  if (i.zone) return i.zone.outline[0] ?? [0, 0];
  if (i.text) return [i.text.x, i.text.y];
  if (i.dim) return [i.dim.start[0], i.dim.start[1]];
  const s = i.shape;
  if (s) {
    if (s.kind === "arc") return arcShapeCenter(s) ?? [s.start[0], s.start[1]];
    if (s.kind === "circle") return [s.center[0], s.center[1]];
    if (s.kind === "polygon") return s.pts[0] ?? [0, 0];
    return [s.start[0], s.start[1]];
  }
  return [0, 0];
}

// -------------------------------------------------------------------------------------------------------------------------- shapes

/** The first thing a `PCB_SHAPE` property gets: where the shape's points are, in the order the `EDA_SHAPE` accessors read them (`GetStart`, `GetEnd`, the circle's centre). */
function shapePoint(s: Shape, which: "start" | "end"): [number, number] {
  switch (s.kind) {
    case "circle":
      return which === "start" ? s.center : s.end;
    case "polygon":
      return s.pts[0] ?? [0, 0];
    default:
      return which === "start" ? s.start : s.end;
  }
}

/** The radius of a circle (`EDA_SHAPE::GetRadius`: the distance from the centre to the point on it), at least 1 like KiCad's. */
function circleRadius(s: Extract<Shape, { kind: "circle" }>): number {
  return Math.max(1, Math.round(Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1])));
}

/** `EDA_SHAPE::GetArcAngle`: the angle the arc sweeps, in degrees, 0 to 360 (a straight "arc" sweeps nothing). */
function arcAngle(s: Extract<Shape, { kind: "arc" }>): number {
  const c = circumcircle(s.start, s.mid, s.end);
  if (!c) return 0;
  const a = (p: readonly [number, number]): number => Math.atan2(p[1] - c.cy, p[0] - c.cx);
  const turn = (from: number, to: number): number => (((to - from) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  const direct = turn(a(s.start), a(s.end));
  // the way round that passes through the mid point
  const sweep = turn(a(s.start), a(s.mid)) <= direct ? direct : 2 * Math.PI - direct;
  return Number(((sweep * 180) / Math.PI).toFixed(6));
}

/**
 * The shape with the geometry property `name` set to `v` (um), the way the `EDA_SHAPE` setters do it: `SetStartX/Y`, `SetEndX/Y` move that point alone;
 * `SetCenterX/Y` takes the point on a circle along with the centre; `SetRadius` puts that point `v` to the right of the centre; `SetRectangleWidth/Height` move the
 * end point to `v` from the start. null when nothing would change.
 */
export function shapeWith(s: Shape, name: "Start X" | "Start Y" | "End X" | "End Y" | "Center X" | "Center Y" | "Radius" | "Width" | "Height", v: number): Shape | null {
  const set = (p: [number, number], axis: 0 | 1): [number, number] => (axis === 0 ? [v, p[1]] : [p[0], v]);
  let next: Shape | null = null;
  switch (name) {
    case "Start X":
    case "Start Y": {
      const axis = name === "Start X" ? 0 : 1;
      if (s.kind === "segment" || s.kind === "rect" || s.kind === "arc" || s.kind === "bezier") next = { ...s, start: set(s.start, axis) };
      break;
    }
    case "End X":
    case "End Y": {
      const axis = name === "End X" ? 0 : 1;
      if (s.kind === "segment" || s.kind === "rect" || s.kind === "arc" || s.kind === "bezier") next = { ...s, end: set(s.end, axis) };
      break;
    }
    case "Center X":
    case "Center Y": {
      if (s.kind === "circle") {
        const axis = name === "Center X" ? 0 : 1;
        const center = set(s.center, axis);
        next = { ...s, center, end: [s.end[0] + center[0] - s.center[0], s.end[1] + center[1] - s.center[1]] };
      }
      break;
    }
    case "Radius":
      if (s.kind === "circle") next = { ...s, end: [s.center[0] + Math.round(v), s.center[1]] };
      break;
    case "Width":
    case "Height": {
      if (s.kind === "rect") {
        const axis = name === "Width" ? 0 : 1;
        // The sign of the corner order stays: the property is the size, `end` is on the same side of `start` as before.
        const dir = s.end[axis] >= s.start[axis] ? 1 : -1;
        const end: [number, number] = axis === 0 ? [s.start[0] + dir * v, s.end[1]] : [s.end[0], s.start[1] + dir * v];
        next = { ...s, end };
      }
      break;
    }
  }
  return next && JSON.stringify(next) !== JSON.stringify(s) ? next : null;
}

const replaceShapeCmd = (s: Shape): Cmd => ({ op: "replace_shape", id: s.id, shape: shapeToCmd(s) });

// ----------------------------------------------------------------------------------------------------------------------- validators

// ----------------------------------------------------------------------------------------------------------------------- the registry

/** `PCB_FOOTPRINT_T` ... the classes that are text, for `EDA_TEXT` properties. */
/** A footprint's field (`PCB_FIELD`), a text with a name. */
const isFieldItem = (i: PcbItem): boolean => i.type === "PCB_FIELD";
/** Free text or a field: both are a `PCB_TEXT`. */
const isTextOrField = (i: PcbItem): boolean => i.type === "PCB_TEXT" || i.type === "PCB_FIELD";

const POSITION_X = "Position X";
const POSITION_Y = "Position Y";

/** A `PCB_TEXT` property of a footprint's field (`Knockout`, `Keep Upright`): a check box that sets one flag of the field's layout. */
function fieldFlag2(_pm: PropertyManager<PcbItem, PcbCtx, Cmd>, add: (def: PropertyDef<PcbItem, PcbCtx, Cmd>, group?: string) => void, name: string, key: "knockout" | "keep_upright", read: (f: FieldInfo) => boolean): void {
  add(
    {
      owner: "PCB_TEXT",
      name,
      kind: "bool",
      get: (i) => read(i.field!),
      available: (i) => i.type === "PCB_FIELD",
      set: (i, v) => (read(i.field!) === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field!, { [key]: Boolean(v) })]),
    },
    "Text Properties"
  );
}

function build(): PropertyManager<PcbItem, PcbCtx, Cmd> {
  const pm = new PropertyManager<PcbItem, PcbCtx, Cmd>();
  const add = (def: PD, group = "") => pm.addProperty(def, group);

  // ---------------------------------------------------------------------------------------------------------------- BOARD_ITEM
  pm.registerType("BOARD_ITEM");

  /** `BOARD_ITEM::GetPosition()` (`pcbItems.ts`'s `itemPosition`, read off the item): a pad's is its own (a pad moves its footprint), a dimension's its first feature point. */
  const position = (i: PcbItem): [number, number] => positionOf(i);
  const setPosition = (axis: 0 | 1) => (i: PcbItem, v: PropValue): Cmd[] => {
    const at = position(i);
    const delta = Math.round(num(v)) - at[axis];
    if (delta === 0) return [];
    const [dx, dy] = axis === 0 ? [delta, 0] : [0, delta];
    // `PCB_PROPERTIES_PANEL::valueChanged`: "In the PCB Editor, we generally restrict pad movement to the footprint (like dragging)."
    if (i.type === "PAD") return moveCmd([i.part!.ref], dx, dy);
    // `PCB_DIMENSION_BASE::SetPosition` sets the first feature point alone.
    if (i.dim) {
      const d = toCmdDimension(i.dim);
      return [dimCmd(i.dim, { start: { x: d.start.x + dx, y: d.start.y + dy } })];
    }
    return moveCmd([i.id], dx, dy);
  };
  add({ owner: "BOARD_ITEM", name: POSITION_X, kind: "int", display: "coord", get: (i) => position(i)[0], set: setPosition(0) });
  add({ owner: "BOARD_ITEM", name: POSITION_Y, kind: "int", display: "coord", get: (i) => position(i)[1], set: setPosition(1) });
  // The layer of a text or a dimension: any layer (`BOARD_ITEM`'s Layer takes every enabled layer; the classes with a layer of their own replace it).
  add({
    owner: "BOARD_ITEM",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => choicesOf(c.allLayers),
    get: (i) => i.text?.layer ?? i.dim?.layer ?? i.field?.layer ?? "",
    set: (i, v) => {
      const layer = String(v);
      if (i.text) return i.text.layer === layer ? [] : [textCmd(i.text, { layer })];
      if (i.dim) return i.dim.layer === layer ? [] : [dimCmd(i.dim, { layer })];
      if (i.field) return i.field.layer === layer ? [] : [editFieldCmd(i.part!, i.field, { layer })];
      return [];
    },
  });
  // `BOARD_ITEM::SetLocked` -> `set_locked` (every kind that can be locked; a pad inherits its footprint's lock, so `PAD_DESC` masks the row).
  add({
    owner: "BOARD_ITEM",
    name: "Locked",
    kind: "bool",
    get: (i, c) => c.locked.has(i.id),
    set: (i, v, c) => (c.locked.has(i.id) === Boolean(v) ? [] : [{ op: "set_locked", ids: [i.id], locked: Boolean(v) }]),
  });

  // ------------------------------------------------------------------------------------------------------ BOARD_CONNECTED_ITEM
  pm.inheritsAfter("BOARD_CONNECTED_ITEM", "BOARD_ITEM");
  // "Replace layer property as the properties panel will set a restriction for copper layers only for BOARD_CONNECTED_ITEM that we don't want to apply to BOARD_ITEM"
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "BOARD_CONNECTED_ITEM",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => choicesOf(c.copperLayers),
    get: (i) => i.track?.layer ?? i.zone?.layer ?? "",
    set: (i, v) => {
      const layer = String(v);
      if (i.track) return i.track.layer === layer ? [] : [{ op: "edit_tracks_and_vias", ids: [i.id], layer }];
      return [];
    },
  });
  const netOf = (i: PcbItem): string => i.track?.net ?? i.via?.net ?? i.zone?.net ?? i.pad?.net ?? "";
  add({
    owner: "BOARD_CONNECTED_ITEM",
    name: "Net",
    kind: "net",
    choices: (_i, c) => [{ label: NO_NET, value: "" }, ...choicesOf(c.nets)],
    get: netOf,
    validate: (v, i) => (v === "" && (i.track || i.via) ? "A track or a via needs a net." : null),
    set: (i, v) => (netOf(i) === String(v) || i.pad ? [] : [{ op: "set_item_net", ids: [i.id], net: String(v) }]),
  });

  // ------------------------------------------------------------------------------------------------------------------ FOOTPRINT
  pm.inheritsAfter("FOOTPRINT", "BOARD_ITEM");
  // A footprint is on the front or the back: `fpLayers`, and `SetLayerAndFlip` turns it over about its position.
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "FOOTPRINT",
    name: "Layer",
    kind: "enum",
    choices: () => choicesOf(["F.Cu", "B.Cu"]),
    get: (i) => (i.part!.side === "bottom" ? "B.Cu" : "F.Cu"),
    set: (i, v) => {
      const part = i.part!;
      const side = part.side === "bottom" ? "B.Cu" : "F.Cu";
      if (side === String(v) || !part.at) return [];
      return [{ op: "flip_items", ids: [part.ref], pivot: xy(part.at), direction: "left_right" }];
    },
  });
  add({
    owner: "FOOTPRINT",
    name: "Orientation",
    kind: "double",
    display: "degree",
    get: (i) => fpOrientation(i.part!),
    set: (i, v) => {
      const part = i.part!;
      if (!part.at) return [];
      // `rot` turns clockwise, KiCad's orientation counter-clockwise: the property is the second, the verb takes the first.
      let delta = norm360(-num(v)) - (part.rot ?? 0);
      delta = ((((delta + 540) % 360) + 360) % 360) - 180;
      return Math.abs(delta) < 1e-9 ? [] : [{ op: "rotate_items", ids: [part.ref], pivot: xy(part.at), angle_millideg: Math.round(delta * 1000) }];
    },
  });
  // The texts are read-only: a footprint's reference, value and library link come from the schematic and the intent (the fields' own layout is the Reference and
  // Value items' to edit, and a user field's text the field's).
  add({ owner: "FOOTPRINT", name: "Reference", kind: "string", get: (i) => i.part!.fields?.[0]?.text ?? i.part!.ref }, "Fields");
  add({ owner: "FOOTPRINT", name: "Value", kind: "string", get: (i) => i.part!.value ?? "" }, "Fields");
  add({ owner: "FOOTPRINT", name: "MPN", kind: "string", get: (i) => i.part!.mpn ?? "", available: (i) => !!i.part!.mpn }, "Fields");
  add({ owner: "FOOTPRINT", name: "Library Link", kind: "string", get: (i) => i.part!.footprint ?? "" }, "Footprint Properties");
  // `FOOTPRINT_DESC`'s "Attributes" and "Overrides": the type (the dialog's "Component Type"; KiCad's panel has no row for it), "Not in Schematic", "Exclude From
  // Position Files", "Exclude From Bill of Materials", "Do not Populate" and "Exempt From Courtyard Requirement" are all written to the footprint's `(attr ..)`.
  const ATTRIBUTES = "Attributes";
  const hasAttrs = (i: PcbItem): boolean => !!i.part!.attrs;
  add(
    {
      owner: "FOOTPRINT",
      name: "Component Type",
      kind: "enum",
      choices: () => [
        { label: "Through hole", value: "through_hole" },
        { label: "SMD", value: "smd" },
        { label: "Unspecified", value: "unspecified" },
      ],
      get: (i) => i.part!.attrs?.kind ?? "unspecified",
      available: hasAttrs,
      set: (i, v) => (i.part!.attrs!.kind === v ? [] : [editAttrsCmd(i.part!, { kind: String(v) as "smd" | "through_hole" | "unspecified" })]),
    },
    ATTRIBUTES
  );
  const attribute = (name: string, key: "board_only" | "exclude_from_pos_files" | "exclude_from_bom" | "dnp" | "allow_missing_courtyard", group: string): void => {
    add(
      {
        owner: "FOOTPRINT",
        name,
        kind: "bool",
        get: (i) => i.part!.attrs?.[key] ?? false,
        available: hasAttrs,
        set: (i, v) => ((i.part!.attrs![key] ?? false) === Boolean(v) ? [] : [editAttrsCmd(i.part!, { [key]: Boolean(v) })]),
      },
      group
    );
  };
  attribute("Not in Schematic", "board_only", ATTRIBUTES);
  attribute("Exclude From Position Files", "exclude_from_pos_files", ATTRIBUTES);
  attribute("Exclude From Bill of Materials", "exclude_from_bom", ATTRIBUTES);
  attribute("Do not Populate", "dnp", ATTRIBUTES);
  attribute("Exempt From Courtyard Requirement", "allow_missing_courtyard", "Overrides");
  add({ owner: "FOOTPRINT", name: "Block", kind: "string", get: (i) => i.part!.block ?? "", available: (i) => !!i.part!.block }, "Placement");
  add(
    {
      owner: "FOOTPRINT",
      name: "Nets",
      kind: "string",
      get: (i) => [...new Set((i.part!.pads ?? []).map((p) => p.net).filter((n): n is string => !!n))].join(", "),
    },
    "Placement"
  );

  // ----------------------------------------------------------------------------------------------------------------------- PAD
  pm.inheritsAfter("PAD", "BOARD_ITEM");
  pm.inheritsAfter("PAD", "BOARD_CONNECTED_ITEM");
  pm.mask("PAD", "BOARD_CONNECTED_ITEM", "Layer");
  pm.mask("PAD", "BOARD_ITEM", "Locked");
  // A pad's net is the schematic's: shown, not edited (no verb re-nets a pad).
  pm.overrideWriteability("PAD", "BOARD_CONNECTED_ITEM", "Net", () => false);
  // `PAD_DESC` as far as this model has the data: the type, shape, size, corner radius and hole are edited through `edit_board_pad`; a pad's number, pin name and
  // type, post-machining, backdrill, fabrication property, copper layers, pad-to-die, the zone connection overrides and thermal reliefs are not (the margins
  // and clearance overrides are the Pad Properties dialog's).
  const padKind = (p: Pad): string => p.kind ?? (p.th ? "through_hole" : "smd");
  const padShape = (p: Pad): string => p.shape ?? (p.round ? (Math.abs(p.w - p.h) < 1 ? "circle" : "oval") : "rect");
  const padSize = (p: Pad): [number, number] => p.size ?? [p.w, p.h];
  const shortest = (p: Pad): number => Math.min(...padSize(p));
  const canHaveHole = (i: PcbItem): boolean => padKind(i.pad!) !== "smd";
  const edit = (i: PcbItem, patch: Parameters<typeof editPadCmd>[2]): Cmd[] => [editPadCmd(i.part!, i.pad!, patch)];
  const PAD = "Pad Properties";
  add(
    {
      owner: "PAD",
      name: "Pad Type",
      kind: "enum",
      choices: () => [
        { label: "Through-hole", value: "through_hole" },
        { label: "SMD", value: "smd" },
        { label: "NPTH, mechanical", value: "non_plated_hole" },
      ],
      get: (i) => padKind(i.pad!),
      set: (i, v) => {
        const pad = i.pad!;
        if (padKind(pad) === v) return [];
        // A pad that gets a hole needs one: half its smaller side, to the 0.05 mm, when it has none (`PAD::SetAttribute` alone would leave it with no hole at all).
        const needsHole = v !== "smd" && !pad.drill && !pad.slot;
        const drill = needsHole ? Math.max(100, Math.round((shortest(pad) * 0.5) / 50) * 50) : undefined;
        return edit(i, { kind: String(v) as "smd" | "through_hole" | "non_plated_hole", ...(drill ? { drill } : {}) });
      },
    },
    PAD
  );
  add(
    {
      owner: "PAD",
      name: "Pad Shape",
      kind: "enum",
      choices: () => [
        { label: "Circle", value: "circle" },
        { label: "Rectangle", value: "rect" },
        { label: "Oval", value: "oval" },
        { label: "Rounded rectangle", value: "round_rect" },
      ],
      get: (i) => padShape(i.pad!),
      set: (i, v) => (padShape(i.pad!) === v ? [] : edit(i, { shape: String(v) as "rect" | "round_rect" | "circle" | "oval" })),
    },
    PAD
  );
  add({ owner: "PAD", name: "Pad Number", kind: "string", get: (i) => i.pad!.num }, PAD);
  const sizeRow = (name: "Size X" | "Size Y", axis: 0 | 1): void => {
    add(
      {
        owner: "PAD",
        name,
        kind: "int",
        display: "size",
        get: (i) => padSize(i.pad!)[axis],
        // "Circle pads have no usable y-size"
        available: (i) => axis === 0 || padShape(i.pad!) !== "circle",
        validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
        set: (i, v) => {
          const [w, h] = padSize(i.pad!);
          const n = Math.round(num(v));
          if ((axis === 0 ? w : h) === n) return [];
          return edit(i, { size: axis === 0 ? [n, padShape(i.pad!) === "circle" ? n : h] : [w, n] });
        },
      },
      PAD
    );
  };
  sizeRow("Size X", 0);
  sizeRow("Size Y", 1);
  // `hasRoundRadius`: only a rounded rectangle has a radius that means anything.
  const hasRadius = (i: PcbItem): boolean => padShape(i.pad!) === "round_rect";
  const ratio = (p: Pad): number => p.ratio ?? 0.25;
  add(
    {
      owner: "PAD",
      name: "Corner Radius Ratio",
      kind: "double",
      display: "ratio",
      get: (i) => ratio(i.pad!),
      available: hasRadius,
      validate: (v) => (num(v) < 0 ? "Value must be greater than or equal to 0" : num(v) > 0.5 ? "Value must be less than or equal to 0.5" : null),
      set: (i, v) => (ratio(i.pad!) === num(v) ? [] : edit(i, { roundrect_ratio: num(v) })),
    },
    PAD
  );
  add(
    {
      owner: "PAD",
      name: "Corner Radius Size",
      kind: "int",
      display: "size",
      get: (i) => Math.round(ratio(i.pad!) * shortest(i.pad!)),
      available: hasRadius,
      validate: (v, i) => (num(v) < 0 ? "Value must be greater than or equal to 0" : num(v) > shortest(i.pad!) / 2 ? "Value must be less than or equal to half the pad's smaller side" : null),
      set: (i, v) => {
        const r = shortest(i.pad!) > 0 ? Math.round((num(v) / shortest(i.pad!)) * 10000) / 10000 : 0;
        return ratio(i.pad!) === r ? [] : edit(i, { roundrect_ratio: r });
      },
    },
    PAD
  );
  add(
    {
      owner: "PAD",
      name: "Hole Shape",
      kind: "enum",
      choices: () => [
        { label: "Round", value: "round" },
        { label: "Oblong", value: "oblong" },
      ],
      get: (i) => (i.pad!.slot ? "oblong" : "round"),
      writeable: canHaveHole,
      set: (i, v) => {
        const pad = i.pad!;
        if ((pad.slot ? "oblong" : "round") === v) return [];
        const d = pad.slot ? Math.min(...pad.slot) : (pad.drill ?? 800);
        return v === "oblong" ? edit(i, { drill_slot: [d, d + Math.max(200, Math.round(d / 2))] }) : edit(i, { drill: d });
      },
    },
    PAD
  );
  const hole = (name: "Hole Size X" | "Hole Size Y", axis: 0 | 1): void => {
    add(
      {
        owner: "PAD",
        name,
        kind: "int",
        display: "size",
        get: (i) => (i.pad!.slot ? i.pad!.slot[axis] : axis === 0 ? (i.pad!.drill ?? 0) : 0),
        // "Circle holes have no usable y-size"
        available: (i) => axis === 0 || !!i.pad!.slot,
        writeable: canHaveHole,
        validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
        set: (i, v) => {
          const pad = i.pad!;
          const n = Math.round(num(v));
          if (pad.slot) return pad.slot[axis] === n ? [] : edit(i, { drill_slot: axis === 0 ? [n, pad.slot[1]] : [pad.slot[0], n] });
          return (pad.drill ?? 0) === n ? [] : edit(i, { drill: n });
        },
      },
      PAD
    );
  };
  hole("Hole Size X", 0);
  hole("Hole Size Y", 1);

  // ------------------------------------------------------------------------------------------------------------ PCB_TRACK, PCB_ARC
  pm.inheritsAfter("PCB_TRACK", "BOARD_CONNECTED_ITEM");
  add({
    owner: "PCB_TRACK",
    name: "Width",
    kind: "int",
    display: "size",
    get: (i) => i.track!.width,
    validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
    set: (i, v) => (i.track!.width === num(v) ? [] : [{ op: "set_track_width", id: i.id, width: Math.round(num(v)) }]),
  });
  const end = (t: Track, which: "start" | "end"): [number, number] => (which === "start" ? t.pts[0] : t.pts[t.pts.length - 1]) ?? [0, 0];
  const trackEnd = (name: string, which: "start" | "end", axis: 0 | 1, replaces?: string): void => {
    const def: PD = {
      owner: "PCB_TRACK",
      name,
      kind: "int",
      display: "coord",
      get: (i) => end(i.track!, which)[axis],
      set: (i, v) => {
        const at = end(i.track!, which);
        const value = Math.round(num(v));
        if (at[axis] === value) return [];
        const next = xy(axis === 0 ? [value, at[1]] : [at[0], value]);
        return [{ op: "edit_track", id: i.id, [which]: next } as Cmd];
      },
    };
    if (replaces) pm.replaceProperty("BOARD_ITEM", replaces, def);
    else add(def);
  };
  trackEnd("Start X", "start", 0, POSITION_X);
  trackEnd("Start Y", "start", 1, POSITION_Y);
  trackEnd("End X", "end", 0);
  trackEnd("End Y", "end", 1);
  pm.inheritsAfter("PCB_ARC", "PCB_TRACK");

  // ----------------------------------------------------------------------------------------------------------------------- PCB_VIA
  pm.inheritsAfter("PCB_VIA", "BOARD_CONNECTED_ITEM");
  pm.mask("PCB_VIA", "BOARD_CONNECTED_ITEM", "Layer");
  const VIA = "Via Properties";
  /** `PCB_VIA::ValidateViaParameters`' diameter and drill checks (the layer ones are for properties this model's vias do not edit). */
  const viaCheck = (diameter: number, drill: number): string | null => {
    if (diameter < 1) return "Via diameter is too small.";
    if (drill < 1) return "Via drill is too small.";
    if (diameter <= drill) return "Via hole size must be smaller than via diameter";
    return null;
  };
  add(
    {
      owner: "PCB_VIA",
      name: "Diameter",
      kind: "int",
      display: "size",
      get: (i) => i.via!.d,
      validate: (v, i) => viaCheck(num(v), i.via!.drill),
      set: (i, v) => (i.via!.d === num(v) ? [] : [{ op: "edit_via", id: i.id, diameter: Math.round(num(v)), drill: i.via!.drill }]),
    },
    VIA
  );
  add(
    {
      owner: "PCB_VIA",
      name: "Hole",
      kind: "int",
      display: "size",
      get: (i) => i.via!.drill,
      validate: (v, i) => viaCheck(i.via!.d, num(v)),
      set: (i, v) => (i.via!.drill === num(v) ? [] : [{ op: "edit_via", id: i.id, diameter: i.via!.d, drill: Math.round(num(v)) }]),
    },
    VIA
  );
  // The layer pair has no verb (KiCad does not let a drawn via change its span from the dialog either): shown, with the type it makes.
  add({ owner: "PCB_VIA", name: "Layer Top", kind: "enum", choices: (_i, c) => choicesOf(c.copperLayers), get: (i) => i.via!.from }, VIA);
  add({ owner: "PCB_VIA", name: "Layer Bottom", kind: "enum", choices: (_i, c) => choicesOf(c.copperLayers), get: (i) => i.via!.to }, VIA);
  add(
    {
      owner: "PCB_VIA",
      name: "Via Type",
      kind: "string",
      get: (i, c) => {
        const first = c.copperLayers[0];
        const last = c.copperLayers[c.copperLayers.length - 1];
        const outer = (l: string): boolean => l === first || l === last;
        const v = i.via!;
        if ((v.from === first && v.to === last) || (v.from === last && v.to === first)) return "Through";
        return outer(v.from) || outer(v.to) ? "Blind" : "Buried";
      },
    },
    VIA
  );

  // ------------------------------------------------------------------------------------------------------------------------- ZONE
  pm.inheritsAfter("ZONE", "BOARD_CONNECTED_ITEM");
  // "Mask layer and position properties; they aren't useful in current form": replaced by hidden ones.
  pm.replaceProperty("BOARD_ITEM", POSITION_X, { owner: "ZONE", name: POSITION_X, kind: "int", display: "coord", hidden: true, get: () => 0 });
  pm.replaceProperty("BOARD_ITEM", POSITION_Y, { owner: "ZONE", name: POSITION_Y, kind: "int", display: "coord", hidden: true, get: () => 0 });
  // "Layer property is hidden because it only holds a single layer and zones actually use a layer set"
  pm.replaceProperty("BOARD_CONNECTED_ITEM", "Layer", { owner: "ZONE", name: "Layer", kind: "enum", hidden: true, get: (i) => i.zone!.layer });
  const isCopperZone = (i: PcbItem): boolean => !!i.zone && !i.zone.is_rule_area && isCopper(i.zone.layer);
  const isRuleArea = (i: PcbItem): boolean => !!i.zone?.is_rule_area;
  const isHatched = (i: PcbItem): boolean => i.zone?.fill_mode === "HatchPattern";
  pm.overrideAvailability("ZONE", "BOARD_CONNECTED_ITEM", "Net", isCopperZone);
  const zoneSet = <K extends keyof Zone>(key: K, convert: (v: PropValue) => Zone[K] = (v) => v as Zone[K]) => (i: PcbItem, v: PropValue): Cmd[] => {
    const next = convert(v);
    return i.zone![key] === next ? [] : [zoneCmd(i.zone!, { [key]: next } as Partial<Zone>)];
  };
  add({ owner: "ZONE", name: "Priority", kind: "int", get: (i) => i.zone!.priority, available: isCopperZone, validate: (v) => (num(v) < 0 ? tooSmall(0, "default", "mm") : null), set: zoneSet("priority", (v) => Math.round(num(v))) });
  add({ owner: "ZONE", name: "Name", kind: "string", get: (i) => i.zone!.name ?? "", set: (i, v) => ((i.zone!.name ?? "") === String(v).trim() ? [] : [{ op: "set_zone_name", id: i.id, name: String(v) }]) });
  const KEEPOUT = "Keepout";
  const keepout = (name: string, key: "keepout_tracks" | "keepout_vias" | "keepout_pads" | "keepout_copper_pour" | "keepout_footprints"): void => {
    add({ owner: "ZONE", name, kind: "bool", get: (i) => i.zone![key], available: isRuleArea, set: zoneSet(key, Boolean) }, KEEPOUT);
  };
  keepout("Keep Out Tracks", "keepout_tracks");
  keepout("Keep Out Vias", "keepout_vias");
  keepout("Keep Out Pads", "keepout_pads");
  keepout("Keep Out Zone Fills", "keepout_copper_pour");
  keepout("Keep Out Footprints", "keepout_footprints");

  const FILL = "Fill Style";
  const atLeastMinWidth = (v: PropValue, i: PcbItem): string | null => (num(v) < i.zone!.min_thickness ? "Cannot be less than zone minimum width" : null);
  add(
    {
      owner: "ZONE",
      name: "Fill Mode",
      kind: "enum",
      choices: () => [
        { label: "Solid fill", value: "Polygons" },
        { label: "Hatch pattern", value: "HatchPattern" },
      ],
      get: (i) => i.zone!.fill_mode,
      available: isCopperZone,
      set: zoneSet("fill_mode", (v) => String(v) as Zone["fill_mode"]),
    },
    FILL
  );
  add({ owner: "ZONE", name: "Hatch Orientation", kind: "double", display: "degree", get: (i) => i.zone!.hatch_orientation_mdeg / 1000, available: isCopperZone, writeable: isHatched, set: zoneSet("hatch_orientation_mdeg", (v) => Math.round(num(v) * 1000)) }, FILL);
  add({ owner: "ZONE", name: "Hatch Width", kind: "int", display: "size", get: (i) => i.zone!.hatch_thickness, available: isCopperZone, writeable: isHatched, validate: atLeastMinWidth, set: zoneSet("hatch_thickness", (v) => Math.round(num(v))) }, FILL);
  add({ owner: "ZONE", name: "Hatch Gap", kind: "int", display: "size", get: (i) => i.zone!.hatch_gap, available: isCopperZone, writeable: isHatched, validate: atLeastMinWidth, set: zoneSet("hatch_gap", (v) => Math.round(num(v))) }, FILL);
  add(
    {
      owner: "ZONE",
      name: "Hatch Minimum Hole Ratio",
      kind: "double",
      display: "ratio",
      get: (i) => i.zone!.hatch_hole_min_area,
      available: isCopperZone,
      writeable: isHatched,
      validate: (v) => (num(v) > 1 ? "Value must be less than or equal to 1" : num(v) < 0 ? "Value must be greater than or equal to 0" : null),
      set: zoneSet("hatch_hole_min_area", num),
    },
    FILL
  );
  add({ owner: "ZONE", name: "Smoothing Effort", kind: "int", get: (i) => i.zone!.hatch_smoothing_level, available: isCopperZone, writeable: isHatched, set: zoneSet("hatch_smoothing_level", (v) => Math.round(num(v))) }, FILL);
  add({ owner: "ZONE", name: "Smoothing Amount", kind: "double", get: (i) => i.zone!.hatch_smoothing_value, available: isCopperZone, writeable: isHatched, set: zoneSet("hatch_smoothing_value", num) }, FILL);
  add(
    {
      owner: "ZONE",
      name: "Remove Islands",
      kind: "enum",
      choices: () => [
        { label: "Always", value: "Always" },
        { label: "Never", value: "Never" },
        { label: "Below area limit", value: "Area" },
      ],
      get: (i) => i.zone!.island_removal_mode,
      available: isCopperZone,
      set: zoneSet("island_removal_mode", (v) => String(v) as Zone["island_removal_mode"]),
    },
    FILL
  );
  add(
    {
      owner: "ZONE",
      name: "Minimum Island Area",
      kind: "int",
      display: "area",
      get: (i) => i.zone!.min_island_area,
      available: isCopperZone,
      writeable: (i) => i.zone!.island_removal_mode === "Area",
      set: zoneSet("min_island_area", (v) => Math.round(num(v))),
    },
    FILL
  );

  const ELECTRICAL = "Electrical";
  add(
    { owner: "ZONE", name: "Clearance", kind: "int", display: "size", get: (i) => i.zone!.clearance, available: isCopperZone, validate: (v, _i, c) => rangeInt(0, 100_000, "size", c.units)(v), set: zoneSet("clearance", (v) => Math.round(num(v))) },
    ELECTRICAL
  );
  add(
    { owner: "ZONE", name: "Minimum Width", kind: "int", display: "size", get: (i) => i.zone!.min_thickness, available: isCopperZone, validate: (v, _i, c) => rangeInt(25, Number.MAX_SAFE_INTEGER, "size", c.units)(v), set: zoneSet("min_thickness", (v) => Math.round(num(v))) },
    ELECTRICAL
  );
  add(
    {
      owner: "ZONE",
      name: "Pad Connections",
      kind: "enum",
      choices: () => [
        { label: "None", value: "None" },
        { label: "Thermal reliefs", value: "Thermal" },
        { label: "Solid", value: "Full" },
        { label: "Thermal reliefs for PTH", value: "ThtThermal" },
      ],
      get: (i) => i.zone!.pad_connection,
      available: isCopperZone,
      set: zoneSet("pad_connection", (v) => String(v) as Zone["pad_connection"]),
    },
    ELECTRICAL
  );
  add({ owner: "ZONE", name: "Thermal Relief Gap", kind: "int", display: "size", get: (i) => i.zone!.thermal_gap, available: isCopperZone, validate: (v) => positiveInt(v), set: zoneSet("thermal_gap", (v) => Math.round(num(v))) }, ELECTRICAL);
  add({ owner: "ZONE", name: "Thermal Relief Spoke Width", kind: "int", display: "size", get: (i) => i.zone!.thermal_spoke_width, available: isCopperZone, validate: atLeastMinWidth, set: zoneSet("thermal_spoke_width", (v) => Math.round(num(v))) }, ELECTRICAL);

  // ------------------------------------------------------------------------------------------------------------- EDA_TEXT, PCB_TEXT, PCB_FIELD
  pm.registerType("EDA_TEXT");
  /** The angle of the text of a text item, a field or a dimension, degrees (a text's and a field's is KiCad's counter-clockwise one). */
  const textAngle = (i: PcbItem): number => (i.text ? i.text.angle : i.field ? i.field.angle : i.dim!.keep_text_aligned ? i.dim!.computed_text_angle : i.dim!.text_angle) / 1000;
  add({
    owner: "EDA_TEXT",
    name: "Orientation",
    kind: "double",
    display: "degree",
    get: textAngle,
    set: (i, v) => {
      const angle = Math.round(norm360(num(v)) * 1000);
      if (i.field) return i.field.angle === angle ? [] : [editFieldCmd(i.part!, i.field, { angle: localAngleOf(i.part!, angle) })];
      return i.text && i.text.angle !== angle ? [textCmd(i.text, { angle })] : [];
    },
  });
  const TEXT = "Text Properties";
  /** A field's Reference and Value come from the schematic and the intent; its user fields are the board's. */
  const isUserField = (i: PcbItem): boolean => !!i.field && i.field.name !== "Reference" && i.field.name !== "Value";
  add(
    {
      owner: "EDA_TEXT",
      name: "Text",
      kind: "string",
      get: (i) => (i.field ? i.field.text : i.text!.content),
      available: isTextOrField,
      writeable: (i) => !i.field || isUserField(i),
      set: (i, v) => {
        if (i.field) return i.field.text === String(v) || !isUserField(i) ? [] : [editFieldCmd(i.part!, i.field, {}, String(v))];
        return i.text!.content === String(v) ? [] : [textCmd(i.text!, { content: String(v) })];
      },
    },
    TEXT
  );
  /** `EDA_TEXT::GetTextThicknessProperty`: a text's pen; a dimension's label's, 15 % of its size unless it says otherwise. */
  const thickness = (i: PcbItem): number => (i.text ? i.text.stroke_width : i.field ? i.field.thickness : i.dim!.text_thickness_um ?? Math.round(i.dim!.text_size_um * 0.15));
  add(
    {
      owner: "EDA_TEXT",
      name: "Thickness",
      kind: "int",
      display: "size",
      get: thickness,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => {
        const w = Math.round(num(v));
        if (thickness(i) === w) return [];
        if (i.field) return [editFieldCmd(i.part!, i.field, { thickness: w })];
        return [i.text ? textCmd(i.text, { stroke_width: w }) : dimCmd(i.dim!, { text_thickness_um: w })];
      },
    },
    TEXT
  );
  // Italic, Bold, Visible, Vertical Justification, Keep Upright and Knockout are a footprint field's: the free-standing text of this model has no place for them.
  const fieldFlag = (name: string, key: "italic" | "bold" | "visible" | "knockout" | "keep_upright", read: (f: FieldInfo) => boolean): void => {
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "bool",
        get: (i) => read(i.field!),
        available: isFieldItem,
        set: (i, v) => (read(i.field!) === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field!, { [key]: Boolean(v) })]),
      },
      TEXT
    );
  };
  fieldFlag("Italic", "italic", (f) => f.italic);
  fieldFlag("Bold", "bold", (f) => f.bold);
  add(
    {
      owner: "EDA_TEXT",
      name: "Mirrored",
      kind: "bool",
      get: (i) => (i.field ? i.field.mirror : i.text!.mirror),
      available: isTextOrField,
      set: (i, v) => {
        if (i.field) return i.field.mirror === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field, { mirror: Boolean(v) })];
        return i.text!.mirror === Boolean(v) ? [] : [textCmd(i.text!, { mirror: Boolean(v) })];
      },
    },
    TEXT
  );
  fieldFlag("Visible", "visible", (f) => f.visible);
  // This model's free text is square (one size for width and height): both rows read and write it. A field has a width and a height of its own.
  const size = (name: "Width" | "Height"): void => {
    const own = (i: PcbItem): number => (i.text ? i.text.size : i.field ? (name === "Width" ? i.field.w : i.field.h) : i.dim!.text_size_um);
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "int",
        display: "size",
        get: own,
        validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
        set: (i, v) => {
          const s = Math.round(num(v));
          if (own(i) === s) return [];
          if (i.field) {
            const f = i.field;
            return [editFieldCmd(i.part!, f, { size: name === "Width" ? [s, f.h] : [f.w, s] })];
          }
          return [i.text ? textCmd(i.text, { size_um: s }) : dimCmd(i.dim!, { text_size_um: s })];
        },
      },
      TEXT
    );
  };
  size("Width");
  size("Height");
  add(
    {
      owner: "EDA_TEXT",
      name: "Horizontal Justification",
      kind: "enum",
      choices: () => [
        { label: "Left", value: "left" },
        { label: "Center", value: "center" },
        { label: "Right", value: "right" },
      ],
      get: (i) => (i.field ? (i.field.halign < 0 ? "left" : i.field.halign > 0 ? "right" : "center") : i.text!.justify),
      available: isTextOrField,
      set: (i, v) => {
        if (i.field) {
          const halign = v === "left" ? -1 : v === "right" ? 1 : 0;
          return i.field.halign === halign ? [] : [editFieldCmd(i.part!, i.field, { halign })];
        }
        return i.text!.justify === v ? [] : [textCmd(i.text!, { justify: String(v) as BoardText["justify"] })];
      },
    },
    TEXT
  );
  add(
    {
      owner: "EDA_TEXT",
      name: "Vertical Justification",
      kind: "enum",
      choices: () => [
        { label: "Top", value: "top" },
        { label: "Center", value: "center" },
        { label: "Bottom", value: "bottom" },
      ],
      get: (i) => (i.field!.valign < 0 ? "top" : i.field!.valign > 0 ? "bottom" : "center"),
      available: isFieldItem,
      set: (i, v) => {
        const valign = v === "top" ? -1 : v === "bottom" ? 1 : 0;
        return i.field!.valign === valign ? [] : [editFieldCmd(i.part!, i.field!, { valign })];
      },
    },
    TEXT
  );

  pm.inheritsAfter("PCB_TEXT", "BOARD_ITEM");
  pm.inheritsAfter("PCB_TEXT", "EDA_TEXT");
  // `PCB_TEXT_DESC`: Knockout and Keep Upright (a footprint's text only).
  fieldFlag2(pm, add, "Knockout", "knockout", (f) => f.knockout);
  fieldFlag2(pm, add, "Keep Upright", "keep_upright", (f) => f.upright);

  // `PCB_FIELD_DESC`: a field is a PCB_TEXT with a name; its position and layer are the field's own, it has no lock of its own (its footprint's is).
  pm.inheritsAfter("PCB_FIELD", "BOARD_ITEM");
  pm.inheritsAfter("PCB_FIELD", "PCB_TEXT");
  pm.inheritsAfter("PCB_FIELD", "EDA_TEXT");
  pm.mask("PCB_FIELD", "BOARD_ITEM", "Locked");

  // ---------------------------------------------------------------------------------------------------------- EDA_SHAPE, PCB_SHAPE
  pm.registerType("EDA_SHAPE");
  const SHAPE = "Shape Properties";
  const kindOf = (i: PcbItem): Shape["kind"] => i.shape!.kind;
  const isNotPolygonOrCircle = (i: PcbItem): boolean => kindOf(i) !== "polygon" && kindOf(i) !== "circle";
  add(
    {
      owner: "EDA_SHAPE",
      name: "Shape",
      kind: "string",
      get: (i) => ({ segment: "Segment", rect: "Rectangle", arc: "Arc", circle: "Circle", polygon: "Polygon", bezier: "Bezier" })[kindOf(i)],
    },
    SHAPE
  );
  const geometry = (name: "Start X" | "Start Y" | "End X" | "End Y" | "Center X" | "Center Y", which: "start" | "end", axis: 0 | 1, available: (i: PcbItem) => boolean): void => {
    add(
      {
        owner: "EDA_SHAPE",
        name,
        kind: "int",
        display: "coord",
        get: (i) => shapePoint(i.shape!, which)[axis],
        available,
        set: (i, v) => {
          const next = shapeWith(i.shape!, name, Math.round(num(v)));
          return next ? [replaceShapeCmd(next)] : [];
        },
      },
      SHAPE
    );
  };
  const isCircle = (i: PcbItem): boolean => kindOf(i) === "circle";
  const isRect = (i: PcbItem): boolean => kindOf(i) === "rect";
  geometry("Start X", "start", 0, isNotPolygonOrCircle);
  geometry("Start Y", "start", 1, isNotPolygonOrCircle);
  geometry("Center X", "start", 0, isCircle);
  geometry("Center Y", "start", 1, isCircle);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Radius",
      kind: "int",
      display: "size",
      get: (i) => circleRadius(i.shape as Extract<Shape, { kind: "circle" }>),
      available: isCircle,
      validate: (v, _i, c) => (num(v) < 1 ? tooSmall(1, "size", c.units) : null),
      set: (i, v) => {
        const next = shapeWith(i.shape!, "Radius", Math.round(num(v)));
        return next ? [replaceShapeCmd(next)] : [];
      },
    },
    SHAPE
  );
  geometry("End X", "end", 0, isNotPolygonOrCircle);
  geometry("End Y", "end", 1, isNotPolygonOrCircle);
  const rectSide = (name: "Width" | "Height", axis: 0 | 1): void => {
    add(
      {
        owner: "EDA_SHAPE",
        name,
        kind: "int",
        display: "size",
        get: (i) => {
          const s = i.shape as Extract<Shape, { kind: "rect" }>;
          return Math.abs(s.end[axis] - s.start[axis]);
        },
        available: isRect,
        validate: (v, _i, c) => (num(v) < 1 ? tooSmall(1, "size", c.units) : null),
        set: (i, v) => {
          const next = shapeWith(i.shape!, name, Math.round(num(v)));
          const s = i.shape as Extract<Shape, { kind: "rect" }>;
          return next && Math.abs(s.end[axis] - s.start[axis]) !== Math.round(num(v)) ? [replaceShapeCmd(next)] : [];
        },
      },
      SHAPE
    );
  };
  rectSide("Width", 0);
  rectSide("Height", 1);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Width",
      kind: "int",
      display: "size",
      get: (i) => i.shape!.stroke_width,
      validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
      set: (i, v) => (i.shape!.stroke_width === num(v) ? [] : [{ op: "edit_shape", id: i.id, layer: i.shape!.layer, stroke_width: Math.round(num(v)), filled: i.shape!.filled }]),
    },
    SHAPE
  );
  add({ owner: "EDA_SHAPE", name: "Angle", kind: "double", display: "degree", get: (i) => arcAngle(i.shape as Extract<Shape, { kind: "arc" }>), available: (i) => kindOf(i) === "arc" }, SHAPE);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Fill",
      kind: "enum",
      choices: () => [
        { label: "None", value: 0 },
        { label: "Solid", value: 1 },
      ],
      get: (i) => (i.shape!.filled ? 1 : 0),
      // `fillAvailable`: polygons, rectangles, circles and Beziers (`PCB_SHAPE_DESC` takes the Bezier away again)
      available: (i) => kindOf(i) === "polygon" || kindOf(i) === "rect" || kindOf(i) === "circle",
      set: (i, v) => (i.shape!.filled === (num(v) === 1) ? [] : [{ op: "edit_shape", id: i.id, layer: i.shape!.layer, stroke_width: i.shape!.stroke_width, filled: num(v) === 1 }]),
    },
    SHAPE
  );

  pm.inheritsAfter("PCB_SHAPE", "BOARD_CONNECTED_ITEM");
  pm.inheritsAfter("PCB_SHAPE", "EDA_SHAPE");
  pm.replaceProperty("BOARD_CONNECTED_ITEM", "Layer", {
    owner: "PCB_SHAPE",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => choicesOf(c.allLayers),
    get: (i) => i.shape!.layer,
    set: (i, v) => (i.shape!.layer === String(v) ? [] : [{ op: "edit_shape", id: i.id, layer: String(v), stroke_width: i.shape!.stroke_width, filled: i.shape!.filled }]),
  });
  // "Only polygons have meaningful Position properties. On other shapes, these are duplicates of the Start properties."
  pm.overrideAvailability("PCB_SHAPE", "BOARD_ITEM", POSITION_X, (i) => kindOf(i) === "polygon");
  pm.overrideAvailability("PCB_SHAPE", "BOARD_ITEM", POSITION_Y, (i) => kindOf(i) === "polygon");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Fill", (i) => kindOf(i) !== "bezier");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Start X", (i) => kindOf(i) !== "circle");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Start Y", (i) => kindOf(i) !== "circle");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "End X", (i) => kindOf(i) !== "circle");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "End Y", (i) => kindOf(i) !== "circle");
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Center X", isCircle);
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Center Y", isCircle);
  pm.overrideAvailability("PCB_SHAPE", "EDA_SHAPE", "Radius", isCircle);
  // A drawn shape has no net in this model (a copper graphic is not part of the netlist).
  pm.overrideAvailability("PCB_SHAPE", "BOARD_CONNECTED_ITEM", "Net", () => false);

  // ------------------------------------------------------------------------------------------------------------ dimensions
  pm.inheritsAfter("PCB_DIMENSION_BASE", "PCB_TEXT");
  pm.inheritsAfter("PCB_DIMENSION_BASE", "BOARD_ITEM");
  pm.inheritsAfter("PCB_DIMENSION_BASE", "EDA_TEXT");
  pm.mask("PCB_DIMENSION_BASE", "EDA_TEXT", "Orientation");
  const DIMENSION = "Dimension Properties";
  const isLeader = (i: PcbItem): boolean => i.dim?.kind === "leader";
  const notLeader = (i: PcbItem): boolean => !isLeader(i);
  const dimSet = (patch: (i: PcbItem, v: PropValue) => Partial<CmdDimension>, changed: (i: PcbItem, v: PropValue) => boolean) => (i: PcbItem, v: PropValue): Cmd[] => (changed(i, v) ? [dimCmd(i.dim!, patch(i, v))] : []);
  add({ owner: "PCB_DIMENSION_BASE", name: "Prefix", kind: "string", get: (i) => i.dim!.prefix, available: notLeader, set: dimSet((_i, v) => ({ prefix: String(v) }), (i, v) => i.dim!.prefix !== String(v)) }, DIMENSION);
  add({ owner: "PCB_DIMENSION_BASE", name: "Suffix", kind: "string", get: (i) => i.dim!.suffix, available: notLeader, set: dimSet((_i, v) => ({ suffix: String(v) }), (i, v) => i.dim!.suffix !== String(v)) }, DIMENSION);
  const overrideSet = dimSet(
    (_i, v) => ({ override_text: String(v) === "" ? null : String(v) }),
    (i, v) => (i.dim!.override_text ?? "") !== String(v)
  );
  add({ owner: "PCB_DIMENSION_BASE", name: "Override Text", kind: "string", get: (i) => i.dim!.override_text ?? "", available: notLeader, set: overrideSet }, DIMENSION);
  add({ owner: "PCB_DIMENSION_BASE", name: "Text", kind: "string", get: (i) => i.dim!.override_text ?? "", available: isLeader, set: overrideSet }, DIMENSION);
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Units",
      kind: "enum",
      choices: () => [
        { label: "Inches", value: "inch" },
        { label: "Mils", value: "mil" },
        { label: "Millimeters", value: "mm" },
        { label: "Automatic", value: "automatic" },
      ],
      get: (i) => i.dim!.units,
      available: notLeader,
      set: dimSet((_i, v) => ({ units: String(v) as Dimension["units"] }), (i, v) => i.dim!.units !== v),
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Units Format",
      kind: "enum",
      choices: () => [
        { label: "1234.0", value: "no_suffix" },
        { label: "1234.0 mm", value: "bare_suffix" },
        { label: "1234.0 (mm)", value: "paren_suffix" },
      ],
      get: (i) => i.dim!.units_format,
      available: notLeader,
      set: dimSet((_i, v) => ({ units_format: String(v) as Dimension["units_format"] }), (i, v) => i.dim!.units_format !== v),
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Precision",
      kind: "enum",
      choices: () => [0, 1, 2, 3, 4, 5].map((n) => ({ label: n === 0 ? "0" : `0.${"0".repeat(n)}`, value: n })),
      get: (i) => i.dim!.precision,
      available: notLeader,
      set: dimSet((_i, v) => ({ precision: num(v) }), (i, v) => i.dim!.precision !== v),
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Suppress Trailing Zeroes",
      kind: "bool",
      get: (i) => i.dim!.suppress_trailing_zeros,
      available: notLeader,
      set: dimSet((_i, v) => ({ suppress_trailing_zeros: Boolean(v) }), (i, v) => i.dim!.suppress_trailing_zeros !== Boolean(v)),
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Arrow Direction",
      kind: "enum",
      choices: () => [
        { label: "Inward", value: "inward" },
        { label: "Outward", value: "outward" },
      ],
      get: (i) => i.dim!.arrow_direction,
      // `isMultiArrowDirection`: the aligned dimension and the orthogonal one that derives from it
      available: (i) => i.dim?.kind === "aligned" || i.dim?.kind === "orthogonal",
      set: dimSet((_i, v) => ({ arrow_direction: String(v) as Dimension["arrow_direction"] }), (i, v) => i.dim!.arrow_direction !== v),
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Keep Aligned with Dimension",
      kind: "bool",
      get: (i) => i.dim!.keep_text_aligned,
      set: dimSet((_i, v) => ({ keep_text_aligned: Boolean(v) }), (i, v) => i.dim!.keep_text_aligned !== Boolean(v)),
    },
    TEXT
  );
  add(
    {
      owner: "PCB_DIMENSION_BASE",
      name: "Orientation",
      kind: "double",
      display: "degree",
      get: textAngle,
      writeable: (i) => !i.dim!.keep_text_aligned,
      set: dimSet((_i, v) => ({ text_angle: Math.round(norm360(num(v)) * 1000) }), (i, v) => i.dim!.text_angle !== Math.round(norm360(num(v)) * 1000)),
    },
    TEXT
  );

  // PCB_DIM_ALIGNED (and PCB_DIM_ORTHOGONAL, which derives from it)
  const alignedBases = (derived: string): void => {
    pm.inheritsAfter(derived, "BOARD_ITEM");
    pm.inheritsAfter(derived, "EDA_TEXT");
    pm.inheritsAfter(derived, "PCB_TEXT");
    pm.inheritsAfter(derived, "PCB_DIMENSION_BASE");
  };
  /** The overrides every dimension class repeats (they are not inherited): the text, vertical justification, hyperlink and knockout of an ordinary text are not a dimension's. */
  const dimensionOverrides = (derived: string, keepText: boolean): void => {
    if (!keepText) pm.overrideAvailability(derived, "EDA_TEXT", "Text", () => false);
    // Mirrored and Horizontal Justification are text properties this model's dimension does not carry.
    pm.overrideAvailability(derived, "EDA_TEXT", "Mirrored", () => false);
    pm.overrideAvailability(derived, "EDA_TEXT", "Horizontal Justification", () => false);
  };
  alignedBases("PCB_DIM_ALIGNED");
  add(
    {
      owner: "PCB_DIM_ALIGNED",
      name: "Crossbar Height",
      kind: "int",
      display: "size",
      get: (i) => i.dim!.height ?? 0,
      set: (i, v) => {
        const d = i.dim!;
        const height = Math.round(num(v));
        if ((d.height ?? 0) === height) return [];
        const kind = toCmdDimension(d).kind;
        return kind.kind === "aligned" || kind.kind === "orthogonal" ? [dimCmd(d, { kind: { ...kind, height } })] : [];
      },
    },
    DIMENSION
  );
  add(
    {
      owner: "PCB_DIM_ALIGNED",
      name: "Extension Line Overshoot",
      kind: "int",
      display: "size",
      get: (i) => i.dim!.extension_height,
      set: (i, v) => (i.dim!.extension_height === num(v) ? [] : [dimCmd(i.dim!, { extension_height: Math.round(num(v)) })]),
    },
    DIMENSION
  );
  dimensionOverrides("PCB_DIM_ALIGNED", false);

  alignedBases("PCB_DIM_ORTHOGONAL");
  pm.inheritsAfter("PCB_DIM_ORTHOGONAL", "PCB_DIM_ALIGNED");
  dimensionOverrides("PCB_DIM_ORTHOGONAL", false);

  alignedBases("PCB_DIM_RADIAL");
  add(
    {
      owner: "PCB_DIM_RADIAL",
      name: "Leader Length",
      kind: "int",
      display: "size",
      get: (i) => i.dim!.leader_length ?? 0,
      set: (i, v) => {
        const d = i.dim!;
        const leader_length = Math.round(num(v));
        return (d.leader_length ?? 0) === leader_length ? [] : [dimCmd(d, { kind: { kind: "radial", leader_length } })];
      },
    },
    DIMENSION
  );
  dimensionOverrides("PCB_DIM_RADIAL", false);

  alignedBases("PCB_DIM_LEADER");
  // "Text Frame" (`DIM_TEXT_BORDER`) has no place in this model.
  dimensionOverrides("PCB_DIM_LEADER", true);
  alignedBases("PCB_DIM_CENTER");
  dimensionOverrides("PCB_DIM_CENTER", false);

  // ----------------------------------------------------------------------------------------------------------------- PCB_GROUP
  pm.registerType("EDA_GROUP");
  pm.inheritsAfter("PCB_GROUP", "BOARD_ITEM");
  pm.inheritsAfter("PCB_GROUP", "EDA_GROUP");
  pm.mask("PCB_GROUP", "BOARD_ITEM", POSITION_X);
  pm.mask("PCB_GROUP", "BOARD_ITEM", POSITION_Y);
  pm.mask("PCB_GROUP", "BOARD_ITEM", "Layer");
  add(
    {
      owner: "EDA_GROUP",
      name: "Name",
      kind: "string",
      get: (i) => i.group!.name,
      set: (i, v) => (i.group!.name === String(v) ? [] : [{ op: "edit_group", id: i.id, name: String(v), member_ids: [...i.group!.member_ids] }]),
    },
    "Group Properties"
  );

  pm.rebuild();
  return pm;
}

/** The registry of the board editor's properties. */
export const PCB_PROPERTIES: PropertyManager<PcbItem, PcbCtx, Cmd> = build();

// ----------------------------------------------------------------------------------------------------------------------- the panel

/** The grid the Properties panel shows for a selection of board items. */
export function pcbGrid(board: BoardState, ids: Iterable<string>, units: LengthUnit): GridModel {
  const ctx = pcbContext(board, units);
  return buildGrid(PCB_PROPERTIES, pcbItemsOf(board, ids), ctx, pcbFriendlyName);
}

/** The commands an edit of one row sets on the selection, or why it is refused. */
export function pcbEdit(board: BoardState, ids: Iterable<string>, name: string, value: PropValue, units: LengthUnit): EditPlan<Cmd> {
  return planEdit(PCB_PROPERTIES, pcbItemsOf(board, ids), name, value, pcbContext(board, units));
}

/** `extractValueAndWritability` for one named property of a selection (tests and the panel's tooltips). */
export function pcbRow(board: BoardState, ids: Iterable<string>, name: string, units: LengthUnit): ReturnType<typeof extractValueAndWritability> {
  return extractValueAndWritability(PCB_PROPERTIES, pcbItemsOf(board, ids), name, pcbContext(board, units));
}
