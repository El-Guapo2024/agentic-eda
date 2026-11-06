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
import { stitchPosition } from "./viaStitch";
import { effectiveTextPenWidth } from "./kicadText";
import type { TeardropParams } from "../api/types";
import type { BoardState, BoardTable, BoardTableCell, BoardText, Cmd, CmdDimension, Dimension, FieldInfo, Group, Pad, PadConnection, PadDrillProps, PadShapeName, Part, PlacementArea, PlacementSourceType, PointXY, Shape, Track, Nm, UnconnectedLayers, Via, ViaProps, Zone } from "../api/types";
import type { LengthUnit } from "../state/units";
import { enabledOptionalLayers, layerDisplayName } from "./boardLayers";
import { effectiveClassName } from "./boardSetupRules";
import { DIM_PRECISION_CHOICES, toCmdDimension } from "./dimensionConvert";
import { boldToggled, labelLooksOf, labelLooksOfCmd, withLabelLooks, type DimensionLabelLooks } from "./dimensionLabel";
import { editAttrsCmd, editFieldCmd, editPadCmd, fieldById, localAngleOf } from "./fpFields";
import { arcShapeCenter, padById } from "./pcbItems";
import { shapeToCmd } from "./pcbPointEdit";
import { circumcircle } from "./trackArc";
import { buildGrid, extractValueAndWritability, planEdit, positiveInt, rangeInt, tooLarge, tooSmall, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";
import type { BoardBarcode, BoardDrillMap, BoardGridItem, BoardPoint, BoardRefImage, BoardTextBox, BoardViaStitch } from "../api/pcbItemTypes";
import { registerExtraItemProperties } from "./pcbItemProperties";
import { fillOf, SHAPE_FILL_CHOICES } from "./shapeFill";
import { NO_ENDING, type LineEnding, type LineEndingStyle } from "./lineEnding";
import { registerTableProperties, tableItemOf, tableItemsOf } from "./pcbTableProperties";
import { PLACEMENT_SOURCE_CHOICES } from "./multichannel";
import { zoneEditCmd as zoneCmd } from "./zoneCmd";
import { padZoneForm, setPadZoneCmd, type PadZoneForm } from "./padZone";
import { padTeardrops, viaTeardrops } from "./teardropParams";
import { LINE_STYLE_CHOICES } from "./strokeDash";
import {
  BACKDRILL_MODE_CHOICES,
  backdrillEnd,
  backdrillMode,
  backdrillSize,
  CAPPING_CHOICES,
  COVERING_CHOICES,
  FILLING_CHOICES,
  modeOf,
  PLUGGING_CHOICES,
  POST_MACHINING_MODE_CHOICES,
  postMachiningKey,
  postMachiningMode,
  TENTING_CHOICES,
  withBackdrillEnd,
  withBackdrillMode,
  withBackdrillSize,
  withFlag,
  withPostMachining,
  withPostMachiningMode,
  type BackdrillMode,
  type FlagKey,
  type PostMachiningMode,
  type SideMode,
} from "./viaProps";

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
  | "PCB_GROUP"
  | "PCB_TEXTBOX"
  | "PCB_REFERENCE_IMAGE"
  | "PCB_POINT"
  | "PCB_BARCODE"
  | "PCB_GRID_ITEM"
  | "PCB_VIA_STITCH"
  | "PCB_DRILL_MAP"
  | "PCB_TABLE"
  | "PCB_TABLECELL";

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
  readonly textBox?: BoardTextBox;
  readonly image?: BoardRefImage;
  readonly point?: BoardPoint;
  readonly barcode?: BoardBarcode;
  /** KiCad 10's grid item. */
  readonly grid?: BoardGridItem;
  /** KiCad 10's via stitching (a generator). */
  readonly stitch?: BoardViaStitch;
  /** KiCad 10's drill map. */
  readonly drillMap?: BoardDrillMap;
  /** A table (`PCB_TABLE`), or the table a cell (`PCB_TABLECELL`) is in. */
  readonly table?: BoardTable;
  readonly cell?: BoardTableCell;
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
  /** How many items are selected: a reference names one footprint, so it is edited only on its own. */
  count: number;
}

/** `LSET::TechAndUserUIOrder`, the layers a board item can be drawn on besides copper (the 18 technical layers KiCad starts a board with; a board's own set is `enabledOptionalLayers`). */
export const TECH_LAYERS: readonly string[] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

export function pcbContext(board: BoardState, units: LengthUnit, count = 1): PcbCtx {
  const nets = new Set<string>();
  for (const p of board.parts) for (const pad of p.pads ?? []) if (pad.net) nets.add(pad.net);
  const rt = board.routing;
  for (const t of rt?.tracks ?? []) if (t.net) nets.add(t.net);
  for (const v of rt?.vias ?? []) if (v.net) nets.add(v.net);
  for (const z of rt?.zones ?? []) if (z.net) nets.add(z.net);
  const copper = board.layers;
  const all = [...copper, ...enabledOptionalLayers(board.board_rules?.layer_setup)];
  const used = [...(board.drawings?.shapes ?? []).map((s) => s.layer), ...(board.drawings?.texts ?? []).map((t) => t.layer), ...(board.drawings?.dimensions ?? []).map((d) => d.layer), ...(board.drawings?.text_boxes ?? []).map((t) => t.layer), ...(board.drawings?.images ?? []).map((i) => i.layer), ...(board.drawings?.points ?? []).map((p) => p.layer)];
  for (const l of used) if (!all.includes(l)) all.push(l);
  return {
    board,
    units,
    locked: new Set(board.locked ?? []),
    nets: [...nets].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" })),
    copperLayers: copper,
    allLayers: all,
    count,
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
  const textBox = dr?.text_boxes?.find((t) => t.id === id);
  if (textBox) return { type: "PCB_TEXTBOX", id, textBox };
  const image = dr?.images?.find((i) => i.id === id);
  if (image) return { type: "PCB_REFERENCE_IMAGE", id, image };
  const point = dr?.points?.find((p) => p.id === id);
  if (point) return { type: "PCB_POINT", id, point };
  const barcode = dr?.barcodes?.find((b) => b.id === id);
  if (barcode) return { type: "PCB_BARCODE", id, barcode };
  const grid = dr?.grid_items?.find((g) => g.id === id);
  if (grid) return { type: "PCB_GRID_ITEM", id, grid };
  const stitch = dr?.via_stitches?.find((v) => v.id === id);
  if (stitch) return { type: "PCB_VIA_STITCH", id, stitch };
  const drillMap = dr?.drill_maps?.find((m) => m.id === id);
  if (drillMap) return { type: "PCB_DRILL_MAP", id, drillMap };
  const tableItem = tableItemOf(board, id);
  if (tableItem) return tableItem;
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
  for (const textBox of dr?.text_boxes ?? []) put({ type: "PCB_TEXTBOX", id: textBox.id, textBox });
  for (const image of dr?.images ?? []) put({ type: "PCB_REFERENCE_IMAGE", id: image.id, image });
  for (const point of dr?.points ?? []) put({ type: "PCB_POINT", id: point.id, point });
  for (const barcode of dr?.barcodes ?? []) put({ type: "PCB_BARCODE", id: barcode.id, barcode });
  for (const grid of dr?.grid_items ?? []) put({ type: "PCB_GRID_ITEM", id: grid.id, grid });
  for (const stitch of dr?.via_stitches ?? []) put({ type: "PCB_VIA_STITCH", id: stitch.id, stitch });
  for (const drillMap of dr?.drill_maps ?? []) put({ type: "PCB_DRILL_MAP", id: drillMap.id, drillMap });
  for (const item of tableItemsOf(board)) put(item);
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

/** `ENUM_MAP<KICAD_T>`: the name the language of expressions and Find by Properties know the class of an item by. */
export function typeName(item: PcbItem): string {
  switch (item.type) {
    case "FOOTPRINT":
      return "Footprint";
    case "PAD":
      return "Pad";
    case "PCB_TRACK":
    case "PCB_ARC":
      return "Track";
    case "PCB_VIA":
      return "Via";
    case "ZONE":
      return "Zone";
    case "PCB_TEXT":
    case "PCB_FIELD":
      return "Text";
    case "PCB_SHAPE":
      return "Graphic";
    case "PCB_DIM_LEADER":
      return "Leader";
    case "PCB_GROUP":
      return "Group";
    case "PCB_TEXTBOX":
      return "Text Box";
    case "PCB_REFERENCE_IMAGE":
      return "Reference Image";
    case "PCB_POINT":
      return "Point";
    case "PCB_BARCODE":
      return "Barcode";
    case "PCB_GRID_ITEM":
      return "Grid";
    case "PCB_VIA_STITCH":
      return "Generator";
    case "PCB_DRILL_MAP":
      return "Drill Map";
    case "PCB_TABLE":
      return "Table";
    case "PCB_TABLECELL":
      return "Table Cell";
    default:
      return "Dimension";
  }
}

/** Every `KICAD_T` name an item here can have (the choices of the Type property). */
export const TYPE_NAMES: readonly string[] = ["Barcode", "Dimension", "Drill Map", "Footprint", "Generator", "Graphic", "Grid", "Group", "Leader", "Pad", "Point", "Reference Image", "Table", "Table Cell", "Text", "Text Box", "Track", "Via", "Zone"];

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
    case "PCB_TEXTBOX":
      return "Text Box";
    case "PCB_REFERENCE_IMAGE":
      return "Reference Image";
    case "PCB_POINT":
      return "Point";
    case "PCB_BARCODE":
      return "Barcode";
    case "PCB_GRID_ITEM":
      return "Grid";
    case "PCB_VIA_STITCH":
      return "Via Stitching";
    case "PCB_DRILL_MAP":
      return "Drill Map";
    case "PCB_TABLE":
      return "Table";
    case "PCB_TABLECELL":
      return "Table Cell";
    default:
      return "Dimension";
  }
}

// ---------------------------------------------------------------------------------------------------------------------------- helpers

type PD = PropertyDef<PcbItem, PcbCtx, Cmd>;

const choicesOf = (names: readonly string[]): Choice[] => names.map((n) => ({ label: n, value: n }));
/** Layer choices: the value is the name in the board file, the label what the board calls the layer (Board Editor Layers). */
const layerChoicesOf = (c: PcbCtx, names: readonly string[]): Choice[] => names.map((n) => ({ label: layerDisplayName(c.board.board_rules?.layer_setup, n), value: n }));
const norm360 = (a: number): number => ((a % 360) + 360) % 360;
const xy = (p: readonly [number, number]): PointXY => ({ x: p[0], y: p[1] });
const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const NO_NET = "<no net>";
/** `KICAD_FONT_NAME`: the stroke font's name in the Font row. */
const KICAD_FONT_NAME = "KiCad Font";

/** `ENUM_MAP<ZONE_CONNECTION>` (footprint.cpp, pad.cpp). */
const ZONE_CONNECTION_CHOICES: Choice[] = [
  { label: "Inherited", value: "inherited" },
  { label: "None", value: "None" },
  { label: "Thermal reliefs", value: "Thermal" },
  { label: "Solid", value: "Full" },
  { label: "Thermal reliefs for PTH", value: "ThtThermal" },
];
/** `ENUM_MAP<PAD_DRILL_POST_MACHINING_MODE>` (pad.cpp). */
const POST_MACHINING_CHOICES: Choice[] = [
  { label: "Not post-machined", value: "not_post_machined" },
  { label: "Counterbore", value: "counterbore" },
  { label: "Countersink", value: "countersink" },
];
/** `ENUM_MAP<BACKDRILL_MODE>` (pad.cpp). */
const BACKDRILL_CHOICES: Choice[] = [
  { label: "No backdrill", value: "no_backdrill" },
  { label: "Backdrill bottom", value: "bottom" },
  { label: "Backdrill top", value: "top" },
  { label: "Backdrill both", value: "both" },
];
/** `ENUM_MAP<PAD_PROP>` (pad.cpp). */
const PAD_PROP_CHOICES: Choice[] = [
  { label: "None", value: "none" },
  { label: "BGA pad", value: "bga" },
  { label: "Fiducial, global to board", value: "fiducial_glob" },
  { label: "Fiducial, local to footprint", value: "fiducial_loc" },
  { label: "Test point pad", value: "testpoint" },
  { label: "Heatsink pad", value: "heatsink" },
  { label: "Castellated pad", value: "castellated" },
  { label: "Mechanical pad", value: "mechanical" },
  { label: "Press-fit pad", value: "pressfit" },
];
/** "Pin Type"'s choices (`GetCanonicalElectricalTypeName` of every `ELECTRICAL_PINTYPE`), and none. */
const PIN_TYPE_CHOICES: Choice[] = [{ label: "", value: "" }, ...["input", "output", "bidirectional", "tri_state", "passive", "free", "unspecified", "power_in", "power_out", "open_collector", "open_emitter", "no_connect"].map((t) => ({ label: t, value: t }))];
/** `ENUM_MAP<UNCONNECTED_LAYER_MODE>` (pad.cpp), the values `PadStack::unconnected` names them by. */
const UNCONNECTED_CHOICES: Choice[] = [
  { label: "All copper layers", value: "keep_all" },
  { label: "Connected layers only", value: "remove_all" },
  { label: "Front, back and connected layers", value: "remove_except_start_and_end" },
  { label: "Start and end layers only", value: "start_end_only" },
];

/** `Normalize180`: an angle in (-180, 180]. */
const norm180 = (a: number): number => {
  const n = norm360(a);
  return n > 180 ? n - 360 : n;
};
/** KiCad's orientation of a footprint (`FOOTPRINT::GetOrientationDegrees`, counter-clockwise on the screen, in (-180, 180]): the backend's `orientation`, which is half a
 * turn from `-rot` for a back footprint made or flipped here; `-rot` from an older backend. */
export const fpOrientation = (part: Pick<Part, "rot" | "orientation">): number => norm180(part.orientation ?? -(part.rot ?? 0));

/** `edit_text` takes every field of the text: the current ones with `patch` laid over them. */
function textCmd(t: BoardText, patch: Partial<{ content: string; angle: number; layer: string; size_nm: Nm; stroke_width: Nm; justify: BoardText["justify"]; mirror: boolean }>): Cmd {
  return { op: "edit_text", id: t.id, content: t.content, angle: t.angle, layer: t.layer, size_nm: t.size, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror, ...patch };
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
  if (i.cell) return [i.cell.corners[0][0], i.cell.corners[0][1]];
  if (i.table) return [i.table.x, i.table.y];
  if (i.field) return [i.field.x, i.field.y];
  if (i.pad) return [i.pad.x, i.pad.y];
  if (i.part) return i.part.at ? [i.part.at[0], i.part.at[1]] : [0, 0];
  if (i.track) return i.track.pts[0] ?? [0, 0];
  if (i.via) return [i.via.x, i.via.y];
  if (i.zone) return i.zone.outline[0] ?? [0, 0];
  if (i.text) return [i.text.x, i.text.y];
  if (i.dim) return [i.dim.start[0], i.dim.start[1]];
  if (i.textBox) return i.textBox.pts[0] ?? i.textBox.start;
  if (i.image) return [i.image.x, i.image.y];
  if (i.point) return [i.point.x, i.point.y];
  if (i.barcode) return [i.barcode.x, i.barcode.y];
  if (i.grid) return [i.grid.x, i.grid.y];
  if (i.stitch) return stitchPosition(i.stitch);
  if (i.drillMap) return [i.drillMap.offset.x, i.drillMap.offset.y];
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

function build(): PropertyManager<PcbItem, PcbCtx, Cmd> {
  const pm = new PropertyManager<PcbItem, PcbCtx, Cmd>();
  const add = (def: PD, group = "") => pm.addProperty(def, group);

  // ----------------------------------------------------------------------------------------------------------------------- EDA_ITEM
  // `EDA_ITEM_DESC`: "Type", the class of the item by the name `ENUM_MAP<KICAD_T>` gives it, hidden from the Properties panel (Find by Properties lists it, and so
  // does the language of expressions: `A.Type == 'Via'`).
  pm.registerType("EDA_ITEM");
  add({ owner: "EDA_ITEM", name: "Type", kind: "enum", hidden: true, choices: () => choicesOf(TYPE_NAMES), get: (i) => typeName(i) });

  // ---------------------------------------------------------------------------------------------------------------- BOARD_ITEM
  pm.registerType("BOARD_ITEM");
  pm.inheritsAfter("BOARD_ITEM", "EDA_ITEM");

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
  // `PT_COORD` with `ABS_X_COORD` / `ABS_Y_COORD` (board_item.cpp): shown from the Origins & Axes origin
  add({ owner: "BOARD_ITEM", name: POSITION_X, kind: "int", display: "coord", coordType: "abs_x", get: (i) => position(i)[0], set: setPosition(0) });
  add({ owner: "BOARD_ITEM", name: POSITION_Y, kind: "int", display: "coord", coordType: "abs_y", get: (i) => position(i)[1], set: setPosition(1) });
  // The layer of a text or a dimension: any layer (`BOARD_ITEM`'s Layer takes every enabled layer; the classes with a layer of their own replace it).
  add({
    owner: "BOARD_ITEM",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => layerChoicesOf(c, c.allLayers),
    get: (i) => i.text?.layer ?? i.dim?.layer ?? i.field?.layer ?? i.table?.layer ?? "",
    set: (i, v) => {
      const layer = String(v);
      if (i.table) return i.table.layer === layer ? [] : [{ op: "table", verb: "set", id: i.table.id, layer }];
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
    // a footprint's field has a lock of its own (`BOARD_ITEM::IsLocked` of the `PCB_FIELD`: its `m_isLocked`, not its footprint's; UI-LEFT-2 PP8)
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
    choices: (_i, c) => layerChoicesOf(c, c.copperLayers),
    get: (i) => i.track?.layer ?? i.zone?.layer ?? "",
    set: (i, v) => {
      const layer = String(v);
      if (i.track) return i.track.layer === layer ? [] : [{ op: "edit_tracks_and_vias", ids: [i.id], layer }];
      return [];
    },
  });
  const netOf = (i: PcbItem): string => i.track?.net ?? i.via?.net ?? i.zone?.net ?? i.pad?.net ?? i.shape?.net ?? "";
  // `BOARD_CONNECTED_ITEM_DESC`: "Net Class", the name of the net's effective class, hidden from the panel (it is how a rule says `A.Net_Class`).
  add({ owner: "BOARD_CONNECTED_ITEM", name: "Net Class", kind: "string", hidden: true, get: (i, c) => (netOf(i) === "" ? "Default" : effectiveClassName(c.board.board_rules?.net_classes ?? [], netOf(i))) });
  add({
    owner: "BOARD_CONNECTED_ITEM",
    name: "Net",
    kind: "net",
    choices: (_i, c) => [{ label: NO_NET, value: "" }, ...choicesOf(c.nets)],
    get: netOf,
    validate: (v, i) => (v === "" && (i.track || i.via) ? "A track or a via needs a net." : null),
    set: (i, v) => (netOf(i) === String(v) || i.pad ? [] : [{ op: "set_item_net", ids: [i.id], net: String(v) }]),
  });

  // "Teardrops" (`BOARD_CONNECTED_ITEM_DESC`): a pad's and a via's own teardrop parameters (`GetTeardropParams()`), each row `SetTeardrop..` of one of them --
  // `set_teardrop_params` with the item's parameters in effect and the one change (UI-LEFT P3). An item without parameters of its own shows what Board Setup's
  // block gives it (kicad-port/teardropParams.ts); an edit gives it its own. Differs: KiCad hides the rows on a board with legacy teardrops (`supportsTeardrops`);
  // the studio's derived file says `(legacy_teardrops yes)` only so that kicad-cli keeps the teardrops it was given, and its teardrops are made from these
  // parameters (Add All Teardrops), so the rows stay.
  const TEARDROPS = "Teardrops";
  const supportsTeardrops = (i: PcbItem): boolean => i.type === "PAD" || i.type === "PCB_VIA";
  const tdOf = (i: PcbItem, c: PcbCtx): TeardropParams => (i.via ? viaTeardrops(c.board, i.via) : padTeardrops(c.board, i.pad!));
  const tdRow = <K extends keyof TeardropParams>(name: string, kind: "bool" | "double" | "int", key: K, available: (i: PcbItem) => boolean = supportsTeardrops): void => {
    add(
      {
        owner: "BOARD_CONNECTED_ITEM",
        name,
        kind,
        ...(kind === "int" ? { display: "size" as const } : {}),
        get: (i, c) => tdOf(i, c)[key] as unknown as PropValue,
        available,
        set: (i, v, c) => {
          const now = tdOf(i, c);
          const value = (kind === "bool" ? v === true || v === "true" : Number(v)) as TeardropParams[K];
          if (now[key] === value) return [];
          return [{ op: "set_teardrop_params", ids: [i.id], params: { ...now, [key]: value } }];
        },
      },
      TEARDROPS
    );
  };
  tdRow("Enable Teardrops", "bool", "enabled");
  tdRow("Best Length Ratio", "double", "best_length_ratio");
  tdRow("Max Length", "int", "max_length");
  tdRow("Best Width Ratio", "double", "best_width_ratio");
  tdRow("Max Width", "int", "max_width");
  tdRow("Curved Teardrops", "bool", "curved_edges");
  tdRow("Prefer Zone Connections", "bool", "prefer_zone_connections", (i) => i.type === "PAD");
  tdRow("Allow Teardrops To Span Two Tracks", "bool", "allow_two_segments");
  tdRow("Max Width Ratio", "double", "filter_ratio");

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
      // `rot` turns clockwise, KiCad's orientation counter-clockwise: the property is the second, the verb takes the first (by the difference, so the
      // half turn of a back footprint drops out).
      let delta = fpOrientation(part) - num(v);
      delta = ((((delta + 540) % 360) + 360) % 360) - 180;
      return Math.abs(delta) < 1e-9 ? [] : [{ op: "rotate_items", ids: [part.ref], pivot: xy(part.at), angle_millideg: Math.round(delta * 1000) }];
    },
  });
  // `FOOTPRINT_DESC`'s "Fields" and "Footprint Properties": the Reference (`SetReference`: a rename, the symbol's with it -- `rename_symbol` renames the board's part
  // in the same step; one footprint at a time, as two cannot share a reference), then the read-only Library Link (`GetFPIDAsString`), Library Description
  // (`GetLibDescription`, the file's `(descr ..)`), Keywords (`(tags ..)`) and Component Class (`GetComponentClassAsString`: the footprint's own classes, sorted and
  // joined by "," as `GetFullClassNameForConstituents` names a composite class; the classes the project's assignment rules add are kicad-cli's DRC's, not shown here).
  add(
    {
      owner: "FOOTPRINT",
      name: "Reference",
      kind: "string",
      get: (i) => i.part!.fields?.[0]?.text ?? i.part!.ref,
      writeable: (_i, c) => c.count === 1,
      readOnlyReason: "Two footprints cannot share a reference: rename them one at a time.",
      validate: (v) => (String(v).trim() === "" ? "A reference cannot be empty." : null),
      set: (i, v) => (i.part!.ref === String(v).trim() ? [] : [{ op: "rename_symbol", id: i.part!.ref, new_id: String(v).trim() }]),
    },
    "Fields"
  );
  // `GetFPIDAsString`: the link the board file writes (`lib_link`, `GetFPID().Format()`: a built-in `0603` is `Resistor_SMD:R_0603_1608Metric`), else the footprint's name
  add({ owner: "FOOTPRINT", name: "Library Link", kind: "string", get: (i) => i.part!.lib_link ?? i.part!.footprint ?? "" }, "Footprint Properties");
  add({ owner: "FOOTPRINT", name: "Library Description", kind: "string", get: (i) => i.part!.lib_description ?? "" }, "Footprint Properties");
  add({ owner: "FOOTPRINT", name: "Keywords", kind: "string", get: (i) => i.part!.keywords ?? "" }, "Footprint Properties");
  add({ owner: "FOOTPRINT", name: "Component Class", kind: "string", get: (i) => [...(i.part!.component_classes ?? [])].sort().join(",") }, "Footprint Properties");
  // `FOOTPRINT_DESC`'s "Attributes" and "Overrides": "Not in Schematic", "Exclude From Position Files", "Exclude From Bill of Materials", "Do not Populate" and
  // "Exempt From Courtyard Requirement" are all written to the footprint's `(attr ..)`. (The footprint's type, SMD or through hole, is the dialog's "Component Type"
  // radio: KiCad's panel has no row for it.)
  const ATTRIBUTES = "Attributes";
  const hasAttrs = (i: PcbItem): boolean => !!i.part!.attrs;
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
  // "Clearance Override" (`SetLocalClearance`, a `std::optional<int>`: an empty cell is none) and "Zone Connection Style" (`SetLocalZoneConnection`, "Inherited"
  // the default) -- both through `set_footprint_zone_connection`, which sets the two together, as Footprint Properties' Clearance Overrides tab does.
  const fpZoneCmd = (part: Part, zone_connection: PadConnection | null, clearance: number | null): Cmd => ({ op: "set_footprint_zone_connection", part: part.ref, zone_connection, clearance });
  add(
    {
      owner: "FOOTPRINT",
      name: "Clearance Override",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => i.part!.zone?.clearance ?? "",
      validate: (v) => (v !== "" && num(v) < 0 ? "Value must be greater than or equal to 0" : null),
      set: (i, v) => {
        const now = i.part!.zone?.clearance ?? null;
        const next = v === "" ? null : Math.round(num(v));
        return now === next ? [] : [fpZoneCmd(i.part!, i.part!.zone?.connection ?? null, next)];
      },
    },
    "Overrides"
  );
  // "Solderpaste Margin Override" and "Solderpaste Margin Ratio Override": the footprint's own `(solder_paste_margin ..)` / `(solder_paste_margin_ratio ..)`,
  // `set_footprint_paste_margin` (each keeping the other): a board file's tokens of the footprint, or the edit of one placed from the intent (UI-LEFT P14).
  const pasteCmd = (part: Part, margin: number | null, ratio: number | null): Cmd => ({ op: "set_footprint_paste_margin", part: part.ref, margin, ratio });
  add(
    {
      owner: "FOOTPRINT",
      name: "Solderpaste Margin Override",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => i.part!.paste_margin ?? "",
      set: (i, v) => {
        const next = v === "" ? null : Math.round(num(v));
        return (i.part!.paste_margin ?? null) === next ? [] : [pasteCmd(i.part!, next, i.part!.paste_margin_ratio ?? null)];
      },
    },
    "Overrides"
  );
  add(
    {
      owner: "FOOTPRINT",
      name: "Solderpaste Margin Ratio Override",
      kind: "double",
      display: "ratio",
      optional: true,
      get: (i) => i.part!.paste_margin_ratio ?? "",
      set: (i, v) => {
        const next = v === "" ? null : num(v);
        return (i.part!.paste_margin_ratio ?? null) === next ? [] : [pasteCmd(i.part!, i.part!.paste_margin ?? null, next)];
      },
    },
    "Overrides"
  );
  add(
    {
      owner: "FOOTPRINT",
      name: "Zone Connection Style",
      kind: "enum",
      choices: () => ZONE_CONNECTION_CHOICES,
      get: (i) => i.part!.zone?.connection ?? "inherited",
      set: (i, v) => {
        const next = v === "inherited" ? null : (String(v) as PadConnection);
        return (i.part!.zone?.connection ?? null) === next ? [] : [fpZoneCmd(i.part!, next, i.part!.zone?.clearance ?? null)];
      },
    },
    "Overrides"
  );

  // ----------------------------------------------------------------------------------------------------------------------- PAD
  pm.inheritsAfter("PAD", "BOARD_ITEM");
  pm.inheritsAfter("PAD", "BOARD_CONNECTED_ITEM");
  pm.mask("PAD", "BOARD_CONNECTED_ITEM", "Layer");
  pm.mask("PAD", "BOARD_ITEM", "Locked");
  // A pad's net is the schematic's: shown, not edited (no verb re-nets a pad).
  pm.overrideWriteability("PAD", "BOARD_CONNECTED_ITEM", "Net", () => false, "A pad's net comes from the schematic (Update PCB from Schematic); no verb re-nets one pad.");
  // `PAD_DESC`. The type, shape, size, corner radius, hole and orientation are edited through `edit_board_pad`, the margins too (the pad's edit), and the zone
  // overrides through `set_pad_zone_overrides`; the number, the pin's name and type, the post-machining, the backdrill, the fabrication property and the
  // pad-to-die length and delay are shown read-only (each says why).
  const padKind = (p: Pad): string => p.kind ?? (p.th ? "through_hole" : "smd");
  /** `PAD::GetAttribute`: the kind, an SMD pad with the connector flag being `PAD_ATTRIB::CONN`. */
  const padTypeOf = (p: Pad): string => (padKind(p) === "smd" && p.connector === true ? "conn" : padKind(p));
  const padShape = (p: Pad): string => p.shape ?? (p.round ? (Math.abs(p.w - p.h) < 1 ? "circle" : "oval") : "rect");
  const padSize = (p: Pad): [number, number] => p.size ?? [p.w, p.h];
  const shortest = (p: Pad): number => Math.min(...padSize(p));
  const canHaveHole = (i: PcbItem): boolean => padKind(i.pad!) !== "smd";
  /** `isCopperPad`: anything but a mechanical hole. */
  const isCopperPad = (i: PcbItem): boolean => padKind(i.pad!) !== "non_plated_hole";
  /** `hasNormalPadstack`: one shape for every copper layer (a front/inner/back or custom padstack is sized in Pad Properties only). */
  const hasNormalPadstack = (i: PcbItem): boolean => (i.pad!.stack?.mode ?? "normal") === "normal";
  const edit = (i: PcbItem, patch: Parameters<typeof editPadCmd>[2]): Cmd[] => [editPadCmd(i.part!, i.pad!, patch)];
  const PAD = "Pad Properties";
  // `PAD::GetOrientationDegrees`: the pad's angle on the board (KiCad's counter-clockwise one), its own in the footprint's frame turned with the footprint -- the
  // way a field's is (`localAngleOf`); `SetOrientationDegrees` sets it there, which is the edit's `rot` (clockwise, the footprint's frame).
  const padOrientation = (i: PcbItem): number => {
    const part = i.part!;
    const sense = part.side === "bottom" ? -1 : 1;
    return norm180(sense * -(i.pad!.rot ?? 0) - (part.rot ?? 0));
  };
  add({
    owner: "PAD",
    name: "Orientation",
    kind: "double",
    display: "degree",
    get: padOrientation,
    set: (i, v) => {
      if (Math.abs(norm180(padOrientation(i) - num(v))) < 1e-9) return [];
      const local = localAngleOf(i.part!, Math.round(num(v) * 1000)) / 1000;
      return edit(i, { rot: norm360(-local) });
    },
  });
  pm.overrideAvailability("PAD", "BOARD_CONNECTED_ITEM", "Net", isCopperPad);
  pm.overrideAvailability("PAD", "BOARD_CONNECTED_ITEM", "Net Class", isCopperPad);
  add(
    {
      owner: "PAD",
      name: "Pad Type",
      kind: "enum",
      // `ENUM_MAP<PAD_ATTRIB>`: PTH, SMD, CONN ("Edge connector": an SMD pad with the connector flag, `PadEdit::connector`) and NPTH, in KiCad's order (UI-LEFT-2 PP8).
      choices: () => [
        { label: "Through-hole", value: "through_hole" },
        { label: "SMD", value: "smd" },
        { label: "Edge connector", value: "conn" },
        { label: "NPTH, mechanical", value: "non_plated_hole" },
      ],
      get: (i) => padTypeOf(i.pad!),
      set: (i, v) => {
        const pad = i.pad!;
        if (padTypeOf(pad) === v) return [];
        // `PAD::SetAttribute`: an SMD or edge connector pad has no hole; a pad that gets a hole needs one: half its smaller side, to the 0.05 mm, when it has none
        // (`PAD::SetAttribute` alone would leave it with no hole at all).
        const kind = v === "conn" ? "smd" : (String(v) as "smd" | "through_hole" | "non_plated_hole");
        const needsHole = kind !== "smd" && !pad.drill && !pad.slot;
        const drill = needsHole ? Math.max(100_000, Math.round((shortest(pad) * 0.5) / 50_000) * 50_000) : undefined;
        const connector = v === "conn";
        return edit(i, { kind, ...(connector !== (pad.connector === true) ? { connector } : {}), ...(drill ? { drill } : {}) });
      },
    },
    PAD
  );
  add(
    {
      owner: "PAD",
      name: "Pad Shape",
      kind: "enum",
      // `ENUM_MAP<PAD_SHAPE>` of `PAD_DESC`: all seven shapes (the dialog edits the delta, the chamfer and the basic shapes they need).
      choices: () => [
        { label: "Circle", value: "circle" },
        { label: "Rectangle", value: "rect" },
        { label: "Oval", value: "oval" },
        { label: "Trapezoid", value: "trapezoid" },
        { label: "Rounded rectangle", value: "round_rect" },
        { label: "Chamfered rectangle", value: "chamfered_rect" },
        { label: "Custom", value: "custom" },
      ],
      get: (i) => padShape(i.pad!),
      available: hasNormalPadstack,
      set: (i, v) => (padShape(i.pad!) === v ? [] : edit(i, { shape: String(v) as PadShapeName })),
    },
    PAD
  );
  // `PAD::SetNumber`: `set_pad_number` renames the one pad, keeping its net (its new pin joins the net in force), its edits and its zone overrides (UI-LEFT-2 PP8).
  add(
    {
      owner: "PAD",
      name: "Pad Number",
      kind: "string",
      get: (i) => i.pad!.num,
      available: isCopperPad,
      set: (i, v) => (i.pad!.num === String(v) || !i.pad!.id ? [] : [{ op: "set_pad_number", pad: i.pad!.id, number: String(v) }]),
    },
    PAD
  );
  // `SetPinFunction`, `SetPinType`: the schematic's pin, as Update PCB from Schematic sets them (`updateComponentPadConnections`) and the board file is written with,
  // until the board sets its own (the pad's edit, `pin_function` / `pin_type`; UI-LEFT P3).
  add({ owner: "PAD", name: "Pin Name", kind: "string", get: (i) => i.pad!.pin_function ?? "", set: (i, v) => ((i.pad!.pin_function ?? "") === String(v) ? [] : edit(i, { pin_function: String(v) })) }, PAD);
  add({
    owner: "PAD",
    name: "Pin Type",
    kind: "enum",
    choices: () => PIN_TYPE_CHOICES,
    get: (i) => i.pad!.pin_type ?? "",
    set: (i, v) => ((i.pad!.pin_type ?? "") === String(v) ? [] : edit(i, { pin_type: String(v) })),
  }, PAD);
  const sizeRow = (name: "Size X" | "Size Y", axis: 0 | 1): void => {
    add(
      {
        owner: "PAD",
        name,
        kind: "int",
        display: "size",
        get: (i) => padSize(i.pad!)[axis],
        // "Custom padstacks can't have size modified through panel"; "Circle pads have no usable y-size"
        available: (i) => hasNormalPadstack(i) && (axis === 0 || padShape(i.pad!) !== "circle"),
        // `PGPROPERTY_SIZE` (a `wxUIntProperty`) takes 0, as `PAD_DESC` registers no validator (UI-LEFT-2 PP8)
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
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
  // `hasRoundRadius` (`PAD_UTILS::PadHasMeaningfulRoundingRadius`): a rounded rectangle and a chamfered one have a radius that means anything.
  const hasRadius = (i: PcbItem): boolean => hasNormalPadstack(i) && (padShape(i.pad!) === "round_rect" || padShape(i.pad!) === "chamfered_rect");
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
        const d = pad.slot ? Math.min(...pad.slot) : (pad.drill ?? 800_000);
        return v === "oblong" ? edit(i, { drill_slot: [d, d + Math.max(200_000, Math.round(d / 2))] }) : edit(i, { drill: d });
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
        // `PositiveIntValidator`: below 0 only (UI-LEFT-2 PP8)
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
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

  // "Post-machining Properties" and "Backdrill Properties" (KiCad 10): a pad's counterbore, countersink and backdrill -- the pad's `drill_props` (the edit's, else
  // its board file's tokens), set whole by the pad's edit with the one change the `PAD` / `PADSTACK` setter makes (`PADSTACK::SetBackdrillMode` .., the via's
  // rules: kicad-port/viaProps.ts). UI-LEFT-2 PP8.
  const drillOf = (i: PcbItem): ViaProps => i.pad!.drill_props ?? {};
  const drillCmd = (i: PcbItem, next: ViaProps): Cmd[] => {
    const props: PadDrillProps = { backdrill: next.backdrill, tertiary_drill: next.tertiary_drill, front_post_machining: next.front_post_machining, back_post_machining: next.back_post_machining };
    for (const k of Object.keys(props) as (keyof PadDrillProps)[]) if (props[k] === undefined) delete props[k];
    return JSON.stringify(props) === JSON.stringify(drillOf(i)) ? [] : edit(i, { drill_props: props });
  };
  /** `GetDrillShape() == PAD_DRILL_SHAPE::CIRCLE`. */
  const isRoundHole = (i: PcbItem): boolean => !i.pad!.slot;
  const POST_MACHINING = "Post-machining Properties";
  for (const [side, front] of [
    ["Top", true],
    ["Bottom", false],
  ] as const) {
    const mode = (i: PcbItem): string => postMachiningMode(drillOf(i), front);
    const pm_ = (i: PcbItem) => drillOf(i)[postMachiningKey(front)];
    add(
      {
        owner: "PAD",
        name: `${side} Post-machining`,
        kind: "enum",
        choices: () => POST_MACHINING_CHOICES,
        get: mode,
        available: isRoundHole,
        writeable: canHaveHole,
        set: (i, v) => (mode(i) === v ? [] : drillCmd(i, withPostMachiningMode(drillOf(i), front, String(v) as PostMachiningMode))),
      },
      POST_MACHINING
    );
    const pmRow = (name: string, field: "size" | "depth" | "angle_ddeg", available: (m: string) => boolean): void => {
      const angle = field === "angle_ddeg";
      add(
        {
          owner: "PAD",
          name,
          kind: angle ? "double" : "int",
          display: angle ? "degree" : "size",
          get: (i) => (angle ? (pm_(i)?.angle_ddeg ?? 0) / 10 : (pm_(i)?.[field] ?? 0)),
          available: (i) => isRoundHole(i) && available(mode(i)),
          writeable: canHaveHole,
          validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, angle ? "degree" : "size", c.units) : null),
          set: (i, v) => {
            const n = angle ? Math.round(num(v) * 10) : Math.round(num(v));
            return (pm_(i)?.[field] ?? 0) === n ? [] : drillCmd(i, withPostMachining(drillOf(i), front, field, n));
          },
        },
        POST_MACHINING
      );
    };
    // Size: a counterbore or a countersink; Depth: a counterbore; Angle: a countersink.
    pmRow(`${side} Post-machining Size`, "size", (m) => m === "counterbore" || m === "countersink");
    pmRow(`${side} Counterbore Depth`, "depth", (m) => m === "counterbore");
    pmRow(`${side} Countersink Angle`, "angle_ddeg", (m) => m === "countersink");
  }
  const BACKDRILL = "Backdrill Properties";
  add(
    {
      owner: "PAD",
      name: "Backdrill Mode",
      kind: "enum",
      choices: () => BACKDRILL_CHOICES,
      get: (i) => backdrillMode(drillOf(i)),
      set: (i, v) => (backdrillMode(drillOf(i)) === v ? [] : drillCmd(i, withBackdrillMode(drillOf(i), String(v) as BackdrillMode, i.pad!.drill ?? 0))),
    },
    BACKDRILL
  );
  for (const [side, top] of [
    ["Bottom", false],
    ["Top", true],
  ] as const) {
    const drills = (i: PcbItem): boolean => {
      const m = backdrillMode(drillOf(i));
      return isRoundHole(i) && (m === "both" || m === (top ? "top" : "bottom"));
    };
    add(
      {
        owner: "PAD",
        name: `${side} Backdrill Size`,
        kind: "int",
        display: "size",
        optional: true,
        get: (i) => backdrillSize(drillOf(i), top) ?? "",
        available: drills,
        validate: (v, _i, c) => (v !== "" && num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const next = v === "" ? null : Math.round(num(v));
          return backdrillSize(drillOf(i), top) === next ? [] : drillCmd(i, withBackdrillSize(drillOf(i), top, next));
        },
      },
      BACKDRILL
    );
    add(
      {
        owner: "PAD",
        name: `${side} Backdrill Must-Cut`,
        kind: "enum",
        choices: (_i, c) => layerChoicesOf(c, c.copperLayers),
        get: (i) => backdrillEnd(drillOf(i), top),
        available: drills,
        set: (i, v) => (backdrillEnd(drillOf(i), top) === v ? [] : drillCmd(i, withBackdrillEnd(drillOf(i), top, String(v)))),
      },
      BACKDRILL
    );
  }
  // `ENUM_MAP<PAD_PROP>`: the pad's fabrication property (`PAD::SetProperty`), the pad's edit `property` (the file's word, `pad_prop_<value>`).
  add(
    {
      owner: "PAD",
      name: "Fabrication Property",
      kind: "enum",
      choices: () => PAD_PROP_CHOICES,
      get: (i) => (i.pad!.property ?? "pad_prop_none").replace(/^pad_prop_/, ""),
      set: (i, v) => {
        const now = (i.pad!.property ?? "pad_prop_none").replace(/^pad_prop_/, "");
        return now === String(v) ? [] : edit(i, { property: `pad_prop_${String(v)}` });
      },
    },
    PAD
  );
  // `PAD::SetUnconnectedLayerMode`: the padstack's (`PadStack::unconnected`), through the pad's edit.
  add(
    {
      owner: "PAD",
      name: "Copper Layers",
      kind: "enum",
      choices: () => UNCONNECTED_CHOICES,
      get: (i) => i.pad!.stack?.unconnected ?? "keep_all",
      set: (i, v) => {
        const now = i.pad!.stack?.unconnected ?? "keep_all";
        if (now === v) return [];
        return edit(i, { stack: { ...(i.pad!.stack ?? { mode: "normal" }), unconnected: String(v) as UnconnectedLayers } });
      },
    },
    PAD
  );
  // `SetPadToDieLength`, `SetPadToDieDelay` (attoseconds): the pad's edit; 0 is none.
  add({ owner: "PAD", name: "Pad To Die Length", kind: "int", display: "size", get: (i) => i.pad!.die_length ?? 0, available: isCopperPad, set: (i, v) => ((i.pad!.die_length ?? 0) === Number(v) ? [] : edit(i, { die_length: Math.round(Number(v)) })) }, PAD);
  add({ owner: "PAD", name: "Pad To Die Delay", kind: "int", display: "time", get: (i) => i.pad!.die_delay ?? 0, available: isCopperPad, set: (i, v) => ((i.pad!.die_delay ?? 0) === Number(v) ? [] : edit(i, { die_delay: Math.round(Number(v)) })) }, PAD);

  // "Overrides": the margins are the pad's edit (`solder_mask_margin`, `solder_paste_margin`, `solder_paste_margin_ratio`: a key left out is the library's), the
  // clearance, the zone connection and the thermal relief are the pad's zone overrides (`set_pad_zone_overrides`, every one of them at once, `null` inherited).
  const OVERRIDES = "Overrides";
  const zoneOf = (i: PcbItem): PadZoneForm => padZoneForm(i.part, i.pad!.num);
  const zoneSetCmd = (i: PcbItem, patch: Partial<PadZoneForm>): Cmd[] => {
    const now = zoneOf(i);
    const next = { ...now, ...patch };
    return JSON.stringify(next) === JSON.stringify(now) ? [] : [setPadZoneCmd(i.part!.ref, i.pad!.num, next)];
  };
  const optionalLength = (v: PropValue): number | null => (v === "" ? null : Math.round(num(v)));
  add(
    {
      owner: "PAD",
      name: "Clearance Override",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => zoneOf(i).clearance ?? "",
      validate: (v, _i, c) => (v !== "" && num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => zoneSetCmd(i, { clearance: optionalLength(v) }),
    },
    OVERRIDES
  );
  const margin = (name: string, read: (p: Pad) => number | null | undefined, key: "solder_mask_margin" | "solder_paste_margin" | "solder_paste_margin_ratio", ratio: boolean): void => {
    add(
      {
        owner: "PAD",
        name,
        kind: ratio ? "double" : "int",
        display: ratio ? "ratio" : "size",
        optional: true,
        get: (i) => read(i.pad!) ?? "",
        set: (i, v) => {
          const now = read(i.pad!) ?? null;
          const next = v === "" ? null : ratio ? num(v) : Math.round(num(v));
          return now === next ? [] : edit(i, { [key]: next ?? undefined });
        },
      },
      OVERRIDES
    );
  };
  margin("Soldermask Margin Override", (p) => p.mask_margin, "solder_mask_margin", false);
  margin("Solderpaste Margin Override", (p) => p.paste_margin, "solder_paste_margin", false);
  margin("Solderpaste Margin Ratio Override", (p) => p.paste_ratio, "solder_paste_margin_ratio", true);
  add(
    {
      owner: "PAD",
      name: "Zone Connection Style",
      kind: "enum",
      choices: () => ZONE_CONNECTION_CHOICES,
      get: (i) => zoneOf(i).connection ?? "inherited",
      set: (i, v) => zoneSetCmd(i, { connection: v === "inherited" ? null : (String(v) as PadConnection) }),
    },
    OVERRIDES
  );
  add(
    {
      owner: "PAD",
      name: "Thermal Relief Spoke Width",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => zoneOf(i).spokeWidth ?? "",
      // `RangeIntValidator<minZoneWidth, INT_MAX>` (`ZONE_THICKNESS_MIN_VALUE_MM`, 0.025 mm)
      validate: (v, _i, c) => (v === "" ? null : rangeInt(25_000, Number.MAX_SAFE_INTEGER, "size", c.units)(v)),
      set: (i, v) => zoneSetCmd(i, { spokeWidth: optionalLength(v) }),
    },
    OVERRIDES
  );
  // `PADSTACK::ThermalSpokeAngle`: the pad's own, else `DefaultThermalSpokeAngleForShape` (90 degrees for an oval or a rectangle of any kind, 45 for the rest).
  const defaultSpokeAngle = (p: Pad): number => (["oval", "rect", "round_rect", "chamfered_rect"].includes(padShape(p)) ? 90 : 45);
  add(
    {
      owner: "PAD",
      name: "Thermal Relief Spoke Angle",
      kind: "double",
      display: "degree",
      get: (i) => (zoneOf(i).spokeAngleMdeg ?? defaultSpokeAngle(i.pad!) * 1000) / 1000,
      set: (i, v) => {
        const mdeg = Math.round(norm360(num(v)) * 1000);
        return mdeg === (zoneOf(i).spokeAngleMdeg ?? defaultSpokeAngle(i.pad!) * 1000) ? [] : zoneSetCmd(i, { spokeAngleMdeg: mdeg });
      },
    },
    OVERRIDES
  );
  add(
    {
      owner: "PAD",
      name: "Thermal Relief Gap",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => zoneOf(i).gap ?? "",
      validate: (v, _i, c) => (v === "" ? null : positiveInt(v, "size", c.units)),
      set: (i, v) => zoneSetCmd(i, { gap: optionalLength(v) }),
    },
    OVERRIDES
  );

  // ------------------------------------------------------------------------------------------------------------ PCB_TRACK, PCB_ARC
  pm.inheritsAfter("PCB_TRACK", "BOARD_CONNECTED_ITEM");
  add({
    owner: "PCB_TRACK",
    name: "Width",
    kind: "int",
    display: "size",
    get: (i) => i.track!.width,
    // `PGPROPERTY_SIZE` (no validator): 0 is a width, a negative one is not (r2-pcb-edit A7)
    validate: (v) => (num(v) < 0 ? "Value must be greater than or equal to 0" : null),
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
  // "Technical Layers" (`isExternalLayerTrack`): a track's own solder-mask opening and margin (`DrawingsSection::track_masks`, `set_solder_mask`). The model keeps a
  // margin only with an opening, so the margin is writeable when the track has one.
  const isOuterTrack = (i: PcbItem): boolean => i.track!.layer === "F.Cu" || i.track!.layer === "B.Cu";
  add(
    {
      owner: "PCB_TRACK",
      name: "Soldermask",
      kind: "bool",
      get: (i) => !!i.track!.mask,
      available: isOuterTrack,
      set: (i, v) => (!!i.track!.mask === Boolean(v) ? [] : [{ op: "set_solder_mask", ids: [i.id], solder_mask: Boolean(v), margin: i.track!.mask?.margin ?? null }]),
    },
    "Technical Layers"
  );
  add(
    {
      owner: "PCB_TRACK",
      name: "Soldermask Margin Override",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => i.track!.mask?.margin ?? "",
      available: isOuterTrack,
      writeable: (i) => !!i.track!.mask,
      readOnlyReason: "The model keeps a margin only for a track with a solder mask opening: tick Soldermask first.",
      set: (i, v) => {
        const next = v === "" ? null : Math.round(num(v));
        return (i.track!.mask?.margin ?? null) === next ? [] : [{ op: "set_solder_mask", ids: [i.id], solder_mask: true, margin: next }];
      },
    },
    "Technical Layers"
  );
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
  // "Layer Top", "Layer Bottom" and "Via Type" (`SetTopLayer`, `SetBottomLayer`, `SetViaType`): `set_via_layers`, which sanitises the pair as Track & Via
  // Properties does (`SanitizeLayers`: a through via spans the board, a pair given back to front is turned round) and refuses what `ValidateViaParameters` does.
  add({
    owner: "PCB_VIA",
    name: "Layer Top",
    kind: "enum",
    choices: (_i, c) => choicesOf(c.copperLayers),
    get: (i) => i.via!.from,
    set: (i, v) => (i.via!.from === v ? [] : [{ op: "set_via_layers", ids: [i.id], from_layer: String(v) }]),
  }, VIA);
  add({
    owner: "PCB_VIA",
    name: "Layer Bottom",
    kind: "enum",
    choices: (_i, c) => choicesOf(c.copperLayers),
    get: (i) => i.via!.to,
    set: (i, v) => (i.via!.to === v ? [] : [{ op: "set_via_layers", ids: [i.id], to_layer: String(v) }]),
  }, VIA);
  /** `GetViaType()` when the via says it (`ENUM_MAP<VIATYPE>`: Through, Blind, Buried, Micro), else worked out from the span. */
  const viaType = (i: PcbItem, c: PcbCtx): string => {
    const v = i.via!;
    if (v.type === "micro") return "Micro";
    if (v.type === "blind") return "Blind";
    if (v.type === "buried") return "Buried";
    const first = c.copperLayers[0];
      coordType: axis === 0 ? "abs_x" : "abs_y",
    const last = c.copperLayers[c.copperLayers.length - 1];
    const outer = (l: string): boolean => l === first || l === last;
    if ((v.from === first && v.to === last) || (v.from === last && v.to === first)) return "Through";
    return outer(v.from) || outer(v.to) ? "Blind" : "Buried";
  };
  add(
    {
      owner: "PCB_VIA",
      name: "Via Type",
      kind: "enum",
      choices: () => choicesOf(["Through", "Blind", "Buried", "Micro"]),
      get: viaType,
      set: (i, v, c) => (viaType(i, c) === v ? [] : [{ op: "set_via_layers", ids: [i.id], kind: String(v).toLowerCase() as "through" | "blind" | "buried" | "micro" }]),
    },
    VIA
  );
  // The tenting, covering, plugging, capping and filling (`PADSTACK::OUTER_LAYER_PROPS`, `DRILL_PROPS`), then "Backdrill" and "Post-machining": the via's
  // `props` (its tokens), set whole by `set_via_props` with the one change the `PCB_VIA` / `PADSTACK` setter makes (kicad-port/viaProps.ts).
  const propsOf = (i: PcbItem): ViaProps => i.via!.props ?? {};
  const propsCmd = (i: PcbItem, next: ViaProps): Cmd[] => (JSON.stringify(next) === JSON.stringify(propsOf(i)) ? [] : [{ op: "set_via_props", id: i.id, props: next }]);
  const sideRow = (name: string, key: FlagKey, choices: Choice[]): void => {
    add({ owner: "PCB_VIA", name, kind: "enum", choices: () => choices, get: (i) => modeOf(propsOf(i)[key]), set: (i, v) => propsCmd(i, withFlag(propsOf(i), key, String(v) as SideMode)) }, VIA);
  };
  sideRow("Front tenting", "front_tenting", TENTING_CHOICES);
  sideRow("Back tenting", "back_tenting", TENTING_CHOICES);
  sideRow("Front covering", "front_covering", COVERING_CHOICES);
  sideRow("Back covering", "back_covering", COVERING_CHOICES);
  sideRow("Front plugging", "front_plugging", PLUGGING_CHOICES);
  sideRow("Back plugging", "back_plugging", PLUGGING_CHOICES);
  sideRow("Capping", "capping", CAPPING_CHOICES);
  sideRow("Filling", "filling", FILLING_CHOICES);

  const BACKDRILL_GROUP = "Backdrill";
  // `canHaveBackdrill`: a through via, or one that already has a backdrill.
  const canHaveBackdrill = (i: PcbItem, c: PcbCtx): boolean => viaType(i, c) === "Through" || backdrillMode(propsOf(i)) !== "no_backdrill";
  const drillsFrom = (top: boolean) => (i: PcbItem): boolean => {
    const m = backdrillMode(propsOf(i));
    return m === "both" || m === (top ? "top" : "bottom");
  };
  add(
    {
      owner: "PCB_VIA",
      name: "Backdrill Mode",
      kind: "enum",
      choices: () => BACKDRILL_MODE_CHOICES,
      get: (i) => backdrillMode(propsOf(i)),
      available: canHaveBackdrill,
      set: (i, v) => (backdrillMode(propsOf(i)) === v ? [] : propsCmd(i, withBackdrillMode(propsOf(i), String(v) as BackdrillMode, i.via!.drill))),
    },
    BACKDRILL_GROUP
  );
  for (const [side, top] of [
    ["Bottom", false],
    ["Top", true],
  ] as const) {
    add(
      {
        owner: "PCB_VIA",
        name: `${side} Backdrill Size`,
        kind: "int",
        display: "size",
        optional: true,
        get: (i) => backdrillSize(propsOf(i), top) ?? "",
        available: drillsFrom(top),
        validate: (v, _i, c) => (v !== "" && num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const next = v === "" ? null : Math.round(num(v));
          return backdrillSize(propsOf(i), top) === next ? [] : propsCmd(i, withBackdrillSize(propsOf(i), top, next));
        },
      },
      BACKDRILL_GROUP
    );
    add(
      {
        owner: "PCB_VIA",
        name: `${side} Backdrill Must-Cut`,
        kind: "enum",
        choices: (_i, c) => choicesOf(c.copperLayers),
        get: (i) => backdrillEnd(propsOf(i), top),
        available: drillsFrom(top),
        set: (i, v) => (backdrillEnd(propsOf(i), top) === v ? [] : propsCmd(i, withBackdrillEnd(propsOf(i), top, String(v)))),
      },
      BACKDRILL_GROUP
    );
  }

  const POST_MACHINING_GROUP = "Post-machining";
  for (const [side, front] of [
    ["Front", true],
    ["Back", false],
  ] as const) {
    const mode = (i: PcbItem): string => postMachiningMode(propsOf(i), front);
    const pm_ = (i: PcbItem) => propsOf(i)[postMachiningKey(front)];
    add(
      {
        owner: "PCB_VIA",
        name: `${side} Post-machining`,
        kind: "enum",
        choices: () => POST_MACHINING_MODE_CHOICES,
        get: mode,
        set: (i, v) => (mode(i) === v ? [] : propsCmd(i, withPostMachiningMode(propsOf(i), front, String(v) as PostMachiningMode))),
      },
      POST_MACHINING_GROUP
    );
    const pmRow = (name: string, field: "size" | "depth" | "angle_ddeg", available: (m: string) => boolean): void => {
      const angle = field === "angle_ddeg";
      add(
        {
          owner: "PCB_VIA",
          name: `${side} Post-machining ${name}`,
          kind: angle ? "double" : "int",
          display: angle ? "degree" : "size",
          get: (i) => (angle ? (pm_(i)?.angle_ddeg ?? 0) / 10 : (pm_(i)?.[field] ?? 0)),
          available: (i) => available(mode(i)),
          validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, angle ? "degree" : "size", c.units) : null),
          set: (i, v) => {
            const n = angle ? Math.round(num(v) * 10) : Math.round(num(v));
            return (pm_(i)?.[field] ?? 0) === n ? [] : propsCmd(i, withPostMachining(propsOf(i), front, field, n));
          },
        },
        POST_MACHINING_GROUP
      );
    };
    // Size: a counterbore or a countersink; Depth: a counterbore; Angle: a countersink.
    pmRow("Size", "size", (m) => m === "counterbore" || m === "countersink");
    pmRow("Depth", "depth", (m) => m === "counterbore");
    pmRow("Angle", "angle_ddeg", (m) => m === "countersink");
  }

  // ------------------------------------------------------------------------------------------------------------------------- ZONE
  pm.inheritsAfter("ZONE", "BOARD_CONNECTED_ITEM");
  // "Mask layer and position properties; they aren't useful in current form": replaced by hidden ones.
  pm.replaceProperty("BOARD_ITEM", POSITION_X, { owner: "ZONE", name: POSITION_X, kind: "int", display: "coord", coordType: "abs_x", hidden: true, get: () => 0 });
  pm.replaceProperty("BOARD_ITEM", POSITION_Y, { owner: "ZONE", name: POSITION_Y, kind: "int", display: "coord", coordType: "abs_y", hidden: true, get: () => 0 });
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

  // The Placement group (`ZONE::GetPlacementArea*`): the multichannel tool's source of a rule area.
  const PLACEMENT = "Placement";
  const placementOf = (i: PcbItem): PlacementArea => i.zone!.placement ?? { enabled: false, source_type: "sheetname", source: "" };
  const placementSet = (change: Partial<PlacementArea>) => (i: PcbItem): Cmd[] => {
    const now = placementOf(i);
    const next = { ...now, ...change };
    return next.enabled === now.enabled && next.source_type === now.source_type && next.source === now.source ? [] : [zoneCmd(i.zone!, { placement: next })];
  };
  add({ owner: "ZONE", name: "Enable", kind: "bool", get: (i) => placementOf(i).enabled, available: isRuleArea, set: (i, v) => placementSet({ enabled: Boolean(v) })(i) }, PLACEMENT);
  add(
    {
      owner: "ZONE",
      name: "Source Type",
      kind: "enum",
      choices: () => PLACEMENT_SOURCE_CHOICES,
      get: (i) => placementOf(i).source_type,
      available: isRuleArea,
      set: (i, v) => placementSet({ source_type: String(v) as PlacementSourceType })(i),
    },
    PLACEMENT
  );
  add({ owner: "ZONE", name: "Source Name", kind: "string", get: (i) => placementOf(i).source, available: isRuleArea, set: (i, v) => placementSet({ source: String(v) })(i) }, PLACEMENT);

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
    { owner: "ZONE", name: "Clearance", kind: "int", display: "size", get: (i) => i.zone!.clearance, available: isCopperZone, validate: (v, _i, c) => rangeInt(0, 100_000_000, "size", c.units)(v), set: zoneSet("clearance", (v) => Math.round(num(v))) },
    ELECTRICAL
  );
  add(
    { owner: "ZONE", name: "Minimum Width", kind: "int", display: "size", get: (i) => i.zone!.min_thickness, available: isCopperZone, validate: (v, _i, c) => rangeInt(25_000, Number.MAX_SAFE_INTEGER, "size", c.units)(v), set: zoneSet("min_thickness", (v) => Math.round(num(v))) },
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
  // `EDA_TEXT_DESC` (common/eda_text.cpp), over a board text (`text`), a footprint's field (`field`) and a dimension's label (`dim`). A free text's italic, bold,
  // vertical justification, glyph width, knockout and automatic stroke width are its `TextExtra` (`edit_text_style`); the rest of it is `edit_text`'s.
  /** The angle of the text of a text item, a field or a dimension, degrees (a text's and a field's is KiCad's counter-clockwise one). */
  const textAngle = (i: PcbItem): number => (i.text ? i.text.angle : i.field ? i.field.angle : i.dim!.keep_text_aligned ? i.dim!.computed_text_angle : i.dim!.text_angle) / 1000;
  const styleCmd = (t: BoardText, patch: Omit<Extract<Cmd, { op: "edit_text_style" }>, "op" | "id">): Cmd => ({ op: "edit_text_style", id: t.id, ...patch });
  /** A dimension's label is its own text (UI-LEFT-2 PP11, kicad-port/dimensionLabel.ts): a row of `EDA_TEXT_DESC` / `PCB_TEXT_DESC` sets its looks in one `edit_dimension`. */
  const dimLooks = (i: PcbItem): DimensionLabelLooks => labelLooksOf(i.dim!);
  const dimLookCmd = (i: PcbItem, patch: (l: DimensionLabelLooks) => DimensionLabelLooks): Cmd[] => {
    const base = toCmdDimension(i.dim!);
    const before = labelLooksOfCmd(base);
    const after = patch(before);
    return JSON.stringify(after) === JSON.stringify(before) ? [] : [{ op: "edit_dimension", id: i.dim!.id, dimension: withLabelLooks(base, after) }];
  };
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
      readOnlyReason: "A footprint's Reference and Value come from the schematic: edit them there, or the footprint's Reference row.",
      set: (i, v) => {
        if (i.field) return i.field.text === String(v) || !isUserField(i) ? [] : [editFieldCmd(i.part!, i.field, {}, String(v))];
        return i.text!.content === String(v) ? [] : [textCmd(i.text!, { content: String(v) })];
      },
    },
    TEXT
  );
  // "Font" (`SetFontProp`: `SetFont( FONT::GetFont( name, IsBold(), IsItalic() ) )`): KiCad's stroke font or a face, which the canvas draws as fontconfig finds it on the
  // server (components/text/outlineFonts.ts, UI-LEFT-2 CC1). A text's goes through `edit_text_style`, a field's through its layout; a dimension's label has no face here.
  add(
    {
      owner: "EDA_TEXT",
      name: "Font",
      kind: "string",
      get: (i) => (i.text ? i.text.face : i.field ? i.field.face : i.dim ? dimLooks(i).face : null) || KICAD_FONT_NAME,
      set: (i, v) => {
        const next = String(v).trim();
        const face = next === "" || next.startsWith(KICAD_FONT_NAME) ? "" : next;
        if (i.field) return (i.field.face ?? "") === face ? [] : [editFieldCmd(i.part!, i.field, { face: face || null })];
        if (i.dim) return dimLookCmd(i, (l) => ({ ...l, face: face || null }));
        return !i.text || (i.text.face ?? "") === face ? [] : [styleCmd(i.text, { face })];
      },
    },
    TEXT
  );
  // "Auto Thickness" (`SetAutoThickness`): the text has no stroke width of its own and is drawn with `GetPenSizeForNormal` / `GetPenSizeForBold`.
  add(
    {
      owner: "EDA_TEXT",
      name: "Auto Thickness",
      kind: "bool",
      get: (i) => (i.text ? !!i.text.auto_thickness : i.field ? i.field.thickness === 0 : i.dim!.text_thickness_nm == null),
      writeable: (i) => !!i.text || !!i.field,
      readOnlyReason: () => "A dimension's label keeps its own pen (15% of its size unless set).",
      set: (i, v) => {
        // a field's automatic pen is a thickness of 0 (`FieldLayout::thickness`): `SetAutoThickness` sets 0, or the pen it is drawn with
        // (`GetEffectiveTextPenWidth()`, the bold or normal pen of its width, clamped) when it is turned off (UI-LEFT-2 PP8)
        if (i.field) {
          const f = i.field;
          if ((f.thickness === 0) === Boolean(v)) return [];
          return [editFieldCmd(i.part!, f, { thickness: Boolean(v) ? 0 : effectiveTextPenWidth(f.thickness, [f.w, f.h], f.bold) })];
        }
        return !!i.text!.auto_thickness === Boolean(v) ? [] : [styleCmd(i.text!, { auto_thickness: Boolean(v) })];
      },
    },
    TEXT
  );
  /** `EDA_TEXT::GetTextThicknessProperty`: a text's pen; a dimension's label's, 15 % of its size unless it says otherwise. */
  const thickness = (i: PcbItem): number => (i.text ? i.text.stroke_width : i.field ? (i.field.thickness || effectiveTextPenWidth(0, [i.field.w, i.field.h], i.field.bold)) : i.dim!.text_thickness_nm ?? Math.round(i.dim!.text_size_nm * 0.15));
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
        // a field's typed pen ends its automatic one even when it is the pen it had (`SetTextThickness`)
        if (i.field) return i.field.thickness === w ? [] : [editFieldCmd(i.part!, i.field, { thickness: w })];
        if (thickness(i) === w && !i.text?.auto_thickness) return [];
        if (!i.text) return [dimCmd(i.dim!, { text_thickness_nm: w })];
        // `SetTextThickness` ends the automatic width (`GetAutoThickness` is a thickness of 0).
        return [...(thickness(i) === w ? [] : [textCmd(i.text, { stroke_width: w })]), ...(i.text.auto_thickness ? [styleCmd(i.text, { auto_thickness: false })] : [])];
      },
    },
    TEXT
  );
  const textFlag = (name: "Italic" | "Bold", key: "italic" | "bold"): void => {
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "bool",
        get: (i) => (i.field ? i.field[key] : i.text ? !!i.text[key] : i.dim ? dimLooks(i)[key] : false),
        set: (i, v) => {
          if (i.field) return i.field[key] === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field, { [key]: Boolean(v) })];
          // `EDA_TEXT::SetBold`: in the stroke font the pen goes to the bold or the normal one (KiCad gives back a pen it kept aside, `m_StoredStrokeWidth`;
          // the model keeps none: the normal pen); an outline face is its own weight
          if (i.dim) return dimLookCmd(i, (l) => (key === "bold" && !l.face ? boldToggled(l, Boolean(v)) : { ...l, [key]: Boolean(v) }));
          return !!i.text![key] === Boolean(v) ? [] : [styleCmd(i.text!, { [key]: Boolean(v) })];
        },
      },
      TEXT
    );
  };
  textFlag("Italic", "italic");
  textFlag("Bold", "bold");
  add(
    {
      owner: "EDA_TEXT",
      name: "Mirrored",
      kind: "bool",
      get: (i) => (i.field ? i.field.mirror : i.text ? i.text.mirror : i.dim ? dimLooks(i).mirrored : false),
      set: (i, v) => {
        if (i.field) return i.field.mirror === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field, { mirror: Boolean(v) })];
        if (i.dim) return dimLookCmd(i, (l) => ({ ...l, mirrored: Boolean(v) }));
        return i.text!.mirror === Boolean(v) ? [] : [textCmd(i.text!, { mirror: Boolean(v) })];
      },
    },
    TEXT
  );
  // "Visible" (`isField`: a field's only).
  add(
    {
      owner: "EDA_TEXT",
      name: "Visible",
      kind: "bool",
      get: (i) => i.field!.visible,
      available: isFieldItem,
      set: (i, v) => (i.field!.visible === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field!, { visible: Boolean(v) })]),
    },
    TEXT
  );
  // "Width" and "Height" (`SetTextWidth`, `SetTextHeight`): a free text's height is its `size`, its width the `TextExtra` width when it has one (else the height).
  // A dimension's label's are its own (UI-LEFT-2 PP11): its height and its `text_effects` width.
  const textWidth = (t: BoardText): number => t.width ?? t.size;
  const size = (name: "Width" | "Height"): void => {
    const own = (i: PcbItem): number => (i.text ? (name === "Width" ? textWidth(i.text) : i.text.size) : i.field ? (name === "Width" ? i.field.w : i.field.h) : name === "Width" ? dimLooks(i).widthNm : dimLooks(i).heightNm);
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "int",
        display: "size",
        get: own,
        // no validator: `SetTextWidth` / `SetTextHeight` clamp to `TEXT_MIN_SIZE_MM` .. `TEXT_MAX_SIZE_MM`, so a size of 0 is 1 um (UI-LEFT-2 PP8)
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const s = Math.min(250_000_000, Math.max(1_000, Math.round(num(v))));
          if (own(i) === s) return [];
          if (i.field) {
            const f = i.field;
            return [editFieldCmd(i.part!, f, { size: name === "Width" ? [s, f.h] : [f.w, s] })];
          }
          if (i.text) {
            const t = i.text;
            if (name === "Width") return [styleCmd(t, { width_nm: s })];
            // The height alone: a square text keeps its width.
            return [textCmd(t, { size_nm: s }), ...(t.width == null ? [styleCmd(t, { width_nm: t.size })] : [])];
          }
          return dimLookCmd(i, (l) => (name === "Width" ? { ...l, widthNm: s } : { ...l, heightNm: s }));
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
      get: (i) => (i.field ? (i.field.halign < 0 ? "left" : i.field.halign > 0 ? "right" : "center") : i.text ? i.text.justify : i.dim ? dimLooks(i).hJustify : "center"),
      set: (i, v) => {
        if (i.dim) return dimLookCmd(i, (l) => ({ ...l, hJustify: String(v) as DimensionLabelLooks["hJustify"] }));
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
      get: (i) => (i.field ? (i.field.valign < 0 ? "top" : i.field.valign > 0 ? "bottom" : "center") : (i.text?.valign ?? "center")),
      set: (i, v) => {
        if (i.field) {
          const valign = v === "top" ? -1 : v === "bottom" ? 1 : 0;
          return i.field.valign === valign ? [] : [editFieldCmd(i.part!, i.field, { valign })];
        }
        return (i.text!.valign ?? "center") === v ? [] : [styleCmd(i.text!, { valign: String(v) as "top" | "center" | "bottom" })];
      },
    },
    TEXT
  );
  // "Color" and "Hyperlink": a board text masks both (`PCB_TEXT_DESC`, `PCB_FIELD_DESC`); a dimension's label shows its colour, which a board file has no place for.
  add({ owner: "EDA_TEXT", name: "Color", kind: "color", get: () => "", readOnlyReason: "A board item has no colour of its own in the board file: it is drawn in its layer's colour." }, TEXT);
  add({ owner: "EDA_TEXT", name: "Hyperlink", kind: "string", get: () => "" }, TEXT);

  pm.inheritsAfter("PCB_TEXT", "BOARD_ITEM");
  pm.inheritsAfter("PCB_TEXT", "EDA_TEXT");
  pm.mask("PCB_TEXT", "EDA_TEXT", "Color");
  // `PCB_TEXT_DESC`: Knockout (`BOARD_ITEM::SetIsKnockout`, any board text) and Keep Upright (a footprint's text only: `isFootprintText`).
  add(
    {
      owner: "PCB_TEXT",
      base: "BOARD_ITEM",
      name: "Knockout",
      kind: "bool",
      get: (i) => (i.field ? i.field.knockout : !!i.text?.knockout),
      set: (i, v) => {
        if (i.field) return i.field.knockout === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field, { knockout: Boolean(v) })];
        return !!i.text!.knockout === Boolean(v) ? [] : [styleCmd(i.text!, { knockout: Boolean(v) })];
      },
    },
    TEXT
  );
  add(
    {
      owner: "PCB_TEXT",
      base: "EDA_TEXT",
      name: "Keep Upright",
      kind: "bool",
      get: (i) => (i.field ? i.field.upright : i.dim ? dimLooks(i).keepUpright : false),
      // A dimension's label has the row too (`PCB_TEXT`'s override is its own class's, not inherited): `SetKeepUpright`, which KiCad keeps in memory only
      // (never written for a dimension, never drawn: `GetDrawRotation` turns only a footprint's text) and the design keeps (UI-LEFT-2 PP11).
      set: (i, v) => (i.dim ? dimLookCmd(i, (l) => ({ ...l, keepUpright: Boolean(v) })) : i.field!.upright === Boolean(v) ? [] : [editFieldCmd(i.part!, i.field!, { keep_upright: Boolean(v) })]),
    },
    TEXT
  );
  pm.overrideAvailability("PCB_TEXT", "EDA_TEXT", "Keep Upright", isFieldItem);
  pm.mask("PCB_TEXT", "EDA_TEXT", "Hyperlink");

  // `PCB_FIELD_DESC`: a field is a PCB_TEXT with a name; its position and layer are the field's own, it has no lock of its own (its footprint's is).
  pm.inheritsAfter("PCB_FIELD", "BOARD_ITEM");
  pm.inheritsAfter("PCB_FIELD", "PCB_TEXT");
  pm.inheritsAfter("PCB_FIELD", "EDA_TEXT");
  pm.mask("PCB_FIELD", "EDA_TEXT", "Hyperlink");
  pm.mask("PCB_FIELD", "EDA_TEXT", "Color");
  // KiCad's field has a lock of its own (`BOARD_ITEM::SetLocked`); here it is locked with its footprint.

  // ------------------------------------------------------------------------------------------------------------------ PCB_TABLE, PCB_TABLECELL
  registerTableProperties(pm, { add });

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
      // `SetRadius( 0 )` is taken (`EDA_SHAPE_DESC` registers no validator; UI-LEFT-2 PP8)
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
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
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
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
  // "Corner Radius" (`SetCornerRadius`, the shape's style); the validator: not more than half the shorter side, not below zero.
  const cornerRadius = (i: PcbItem): number => (i.shape as Extract<Shape, { kind: "rect" }>).corner_radius ?? 0;
  add(
    {
      owner: "EDA_SHAPE",
      name: "Corner Radius",
      kind: "int",
      display: "size",
      get: cornerRadius,
      available: isRect,
      validate: (v, i, c) => {
        const s = i.shape as Extract<Shape, { kind: "rect" }>;
        const max = Math.floor(Math.min(Math.abs(s.end[0] - s.start[0]), Math.abs(s.end[1] - s.start[1])) / 2);
        return num(v) > max ? tooLarge(max, "size", c.units) : num(v) < 0 ? tooSmall(0, "size", c.units) : null;
      },
      set: (i, v) => (cornerRadius(i) === Math.round(num(v)) ? [] : [{ op: "set_corner_radius", ids: [i.id], radius: Math.round(num(v)) }]),
    },
    SHAPE
  );
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Width",
      kind: "int",
      display: "size",
      get: (i) => i.shape!.stroke_width,
      // `SetWidth( 0 )` is taken (no validator): a filled shape with no outline (UI-LEFT-2 PP8)
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => (i.shape!.stroke_width === num(v) ? [] : [{ op: "edit_shape", id: i.id, layer: i.shape!.layer, stroke_width: Math.round(num(v)), filled: i.shape!.filled }]),
    },
    SHAPE
  );
  // "Line Style" (`ENUM_MAP<LINE_STYLE>`, `EDA_SHAPE::SetLineStyle` / `GetLineStyle`): the shape's style (the board's `shape_styles`), `set_shape_style`'s
  // `line_style` (UI-LEFT P5). A file's `default` is none of the five; it shows as Solid, as the dialog shows it.
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Style",
      kind: "enum",
      choices: () => LINE_STYLE_CHOICES.map((c) => ({ label: c.label, value: c.value })),
      get: (i) => i.shape!.dash ?? "solid",
      set: (i, v) => {
        const style = LINE_STYLE_CHOICES.find((c) => c.value === String(v))?.value;
        return style === undefined || style === (i.shape!.dash ?? "solid") ? [] : [{ op: "pcb_item", verb: "set_shape_style", ids: [i.id], line_style: style }];
      },
    },
    SHAPE
  );
  // "Line Color" and "Fill Color" are a schematic shape's: `PCB_SHAPE_DESC` masks both.
  add({ owner: "EDA_SHAPE", name: "Line Color", kind: "color", get: () => "" }, SHAPE);
  add({ owner: "EDA_SHAPE", name: "Angle", kind: "double", display: "degree", get: (i) => arcAngle(i.shape as Extract<Shape, { kind: "arc" }>), available: (i) => kindOf(i) === "arc" }, SHAPE);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Fill",
      kind: "enum",
      // `UI_FILL_MODE`: None, Solid, Hatch, Reverse Hatch, Cross-hatch (`EDA_SHAPE::GetFillModeProp`), numbered in that order.
      choices: () => SHAPE_FILL_CHOICES.map((c, n) => ({ label: c.label, value: n })),
      get: (i) => SHAPE_FILL_CHOICES.findIndex((c) => c.value === fillOf(i.shape!)),
      // `fillAvailable`: polygons, rectangles, circles and Beziers (`PCB_SHAPE_DESC` takes the Bezier away again)
      available: (i) => kindOf(i) === "polygon" || kindOf(i) === "rect" || kindOf(i) === "circle",
      set: (i, v) => {
        const fill = SHAPE_FILL_CHOICES[Math.round(num(v))]?.value;
        return fill === undefined || fill === fillOf(i.shape!) ? [] : [{ op: "pcb_item", verb: "set_shape_style", ids: [i.id], fill }];
      },
    },
    SHAPE
        coordType: axis === 0 ? "abs_x" : "abs_y",
  );

  add({ owner: "EDA_SHAPE", name: "Fill Color", kind: "color", get: () => "", available: (i) => kindOf(i) === "polygon" || kindOf(i) === "rect" || kindOf(i) === "circle" }, SHAPE);

  // KiCad 10's line endings (`_EDA_SHAPE_DESC`, after Fill Color): Start Shape, Start Length, Start Width, Start Stroke Width and the End ones, on an open shape (`isOpenShape`).
  // The enum lists the styles in `ENUM_MAP<LINE_ENDING_STYLE>`'s order: None, Arrow, Circle, Square, Open Arrow.
  const ENDING_STYLES: { label: string; value: LineEndingStyle }[] = [
    { label: "None", value: "none" },
    { label: "Arrow", value: "arrow" },
    { label: "Circle", value: "circle" },
    { label: "Square", value: "square" },
    { label: "Open Arrow", value: "arrow_open" },
  ];
  const isOpenShape = (i: PcbItem): boolean => kindOf(i) === "segment" || kindOf(i) === "arc" || kindOf(i) === "bezier";
  const endingOf = (i: PcbItem, side: "start" | "end"): LineEnding => (side === "start" ? i.shape!.start_ending : i.shape!.end_ending) ?? NO_ENDING;
  const endingSet = (i: PcbItem, side: "start" | "end", next: LineEnding): Cmd[] => {
    const cur = endingOf(i, side);
    if (cur.style === next.style && cur.length === next.length && cur.width === next.width && cur.stroke_width === next.stroke_width) return [];
    const ending = { style: next.style, length_nm: next.length, width_nm: next.width, stroke_width_nm: next.stroke_width };
    return [{ op: "pcb_item", verb: "set_shape_style", ids: [i.id], ...(side === "start" ? { start_ending: ending } : { end_ending: ending }) }];
  };
  for (const [side, cap] of [
    ["start", "Start"],
    ["end", "End"],
  ] as const) {
    add(
      {
        owner: "EDA_SHAPE",
        name: `${cap} Shape`,
        kind: "enum",
        choices: () => ENDING_STYLES.map((c, n) => ({ label: c.label, value: n })),
        get: (i) => Math.max(0, ENDING_STYLES.findIndex((c) => c.value === endingOf(i, side).style)),
        available: isOpenShape,
        set: (i, v) => {
          const style = ENDING_STYLES[Math.round(num(v))]?.value;
          return style === undefined ? [] : endingSet(i, side, { ...endingOf(i, side), style });
        },
      },
      SHAPE
    );
    for (const [prop, field] of [
      ["Length", "length"],
      ["Width", "width"],
      ["Stroke Width", "stroke_width"],
    ] as const) {
      add(
        {
          owner: "EDA_SHAPE",
          name: `${cap} ${prop}`,
          kind: "int",
          display: "size",
          get: (i) => endingOf(i, side)[field],
          available: isOpenShape,
          set: (i, v) => endingSet(i, side, { ...endingOf(i, side), [field]: Math.max(0, Math.round(num(v))) }),
        },
        SHAPE
      );
    }
  }

  pm.inheritsAfter("PCB_SHAPE", "BOARD_CONNECTED_ITEM");
  pm.inheritsAfter("PCB_SHAPE", "EDA_SHAPE");
  pm.replaceProperty("BOARD_CONNECTED_ITEM", "Layer", {
    owner: "PCB_SHAPE",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => layerChoicesOf(c, c.allLayers),
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
  pm.mask("PCB_SHAPE", "EDA_SHAPE", "Line Color");
  pm.mask("PCB_SHAPE", "EDA_SHAPE", "Fill Color");
  // `isCopper`: a shape on copper has a net (the board file's `(net ..)`, the shape's style), set by `set_item_net` (`SetNetCode`; <no net> taken: UI-LEFT-2 PP8).
  pm.overrideAvailability("PCB_SHAPE", "BOARD_CONNECTED_ITEM", "Net", (i) => isCopper(i.shape!.layer));
  // "Pad Primitives": a custom pad's shapes in the Footprint Editor's pad edit mode (`isPadEditMode`), never on the board.
  add({ owner: "PCB_SHAPE", name: "Number Box", kind: "bool", get: () => false, available: () => false }, "Pad Primitives");
  add({ owner: "PCB_SHAPE", name: "Thermal Spoke Template", kind: "bool", get: () => false, available: () => false }, "Pad Primitives");
  // "Technical Layers": a solder-mask opening of a shape on the front or back copper (`isExternalCuLayer`), `set_solder_mask`. The model keeps a margin only with
  // an opening, so the margin is writeable when the shape has one.
  const TECH = "Technical Layers";
  const isOuterCopper = (layer: string): boolean => layer === "F.Cu" || layer === "B.Cu";
  add(
    {
      owner: "PCB_SHAPE",
      name: "Soldermask",
      kind: "bool",
      get: (i) => !!i.shape!.solder_mask,
      available: (i) => isOuterCopper(i.shape!.layer),
      set: (i, v) => (!!i.shape!.solder_mask === Boolean(v) ? [] : [{ op: "set_solder_mask", ids: [i.id], solder_mask: Boolean(v), margin: i.shape!.solder_mask_margin ?? null }]),
    },
    TECH
  );
  add(
    {
      owner: "PCB_SHAPE",
      name: "Soldermask Margin Override",
      kind: "int",
      display: "size",
      optional: true,
      get: (i) => i.shape!.solder_mask_margin ?? "",
      available: (i) => isOuterCopper(i.shape!.layer),
      writeable: (i) => !!i.shape!.solder_mask,
      readOnlyReason: "The model keeps a margin only for a shape with a solder mask opening: tick Soldermask first.",
      set: (i, v) => {
        const next = v === "" ? null : Math.round(num(v));
        return (i.shape!.solder_mask_margin ?? null) === next ? [] : [{ op: "set_solder_mask", ids: [i.id], solder_mask: true, margin: next }];
      },
    },
    TECH
  );

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
      choices: () => DIM_PRECISION_CHOICES,
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
  const dimensionOverrides = (derived: string): void => {
    pm.overrideAvailability(derived, "EDA_TEXT", "Text", () => false);
    pm.overrideAvailability(derived, "EDA_TEXT", "Vertical Justification", () => false);
    pm.overrideAvailability(derived, "EDA_TEXT", "Hyperlink", () => false);
    pm.overrideAvailability(derived, "BOARD_ITEM", "Knockout", () => false);
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
  dimensionOverrides("PCB_DIM_ALIGNED");

  alignedBases("PCB_DIM_ORTHOGONAL");
  pm.inheritsAfter("PCB_DIM_ORTHOGONAL", "PCB_DIM_ALIGNED");
  dimensionOverrides("PCB_DIM_ORTHOGONAL");

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
  dimensionOverrides("PCB_DIM_RADIAL");

  alignedBases("PCB_DIM_LEADER");
  // "Text Frame" (`ENUM_MAP<DIM_TEXT_BORDER>`: None, Rectangle, Circle; `PCB_DIM_LEADER::ChangeTextBorder`): the leader's `text_frame`, drawn round its text (UI-LEFT P13).
  add(
    {
      owner: "PCB_DIM_LEADER",
      name: "Text Frame",
      kind: "enum",
      choices: () => [
        { label: "None", value: "none" },
        { label: "Rectangle", value: "rectangle" },
        { label: "Circle", value: "circle" },
      ],
      get: (i) => i.dim!.text_frame ?? "none",
      set: (i, v) => {
        const frame = String(v) as "none" | "rectangle" | "circle";
        if (!["none", "rectangle", "circle"].includes(frame) || frame === (i.dim!.text_frame ?? "none")) return [];
        return [dimCmd(i.dim!, { text_frame: frame === "none" ? undefined : frame })];
      },
    },
    DIMENSION
  );
  dimensionOverrides("PCB_DIM_LEADER");
  alignedBases("PCB_DIM_CENTER");
  dimensionOverrides("PCB_DIM_CENTER");

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

  // The text boxes, reference images, points and barcodes (GAPS.md item 24): kicad-port/pcbItemProperties.ts.
  registerExtraItemProperties(pm, add, (c) => layerChoicesOf(c, c.allLayers));

  pm.rebuild();
  return pm;
}

/** The registry of the board editor's properties. */
export const PCB_PROPERTIES: PropertyManager<PcbItem, PcbCtx, Cmd> = build();

// ----------------------------------------------------------------------------------------------------------------------- the panel

/** The grid the Properties panel shows for a selection of board items. */
export function pcbGrid(board: BoardState, ids: Iterable<string>, units: LengthUnit): GridModel {
  const items = pcbItemsOf(board, ids);
  return buildGrid(PCB_PROPERTIES, items, pcbContext(board, units, items.length), pcbFriendlyName);
}

/** The commands an edit of one row sets on the selection, or why it is refused. */
export function pcbEdit(board: BoardState, ids: Iterable<string>, name: string, value: PropValue, units: LengthUnit): EditPlan<Cmd> {
  const items = pcbItemsOf(board, ids);
  return planEdit(PCB_PROPERTIES, items, name, value, pcbContext(board, units, items.length));
}

/** `extractValueAndWritability` for one named property of a selection (tests and the panel's tooltips). */
export function pcbRow(board: BoardState, ids: Iterable<string>, name: string, units: LengthUnit): ReturnType<typeof extractValueAndWritability> {
  const items = pcbItemsOf(board, ids);
  return extractValueAndWritability(PCB_PROPERTIES, items, name, pcbContext(board, units, items.length));
}
