// The items of the Footprint Editor's open footprint as its canvas and its edit tools see them (UI-LEFT F1, F2): the pads, graphics and texts, the fields
// (Reference, Value, Datasheet, Description, the user fields: `PCB_FIELD`s of the footprint, `field:<name>`), and the items `eda_model::fp_items` adds
// (dimensions, zones, text boxes, points, images, barcodes, tables, groups). Which item a click picks, where each item is and what box it covers, which
// group a click on a member selects, the point a turn or a mirror goes about, and the one `fp_item` verb each edit is.
//
// Ports the footprint editor's half of `EDIT_TOOL` (pcbnew/tools/edit_tool.cpp at KiCad 8303b2ad: `Rotate`, `Mirror`, `updateModificationPoint` -- "Exclude text in the
// footprint editor if there's anything else selected" -- `DeleteItems`), `FOOTPRINT_EDIT_FRAME::OnEditItemRequest` (pcbnew/footprint_editor_utils.cpp: which dialog
// an item opens) and `BOARD_ITEM::GetPosition` of each kind. The footprint editor's board is a footprint holder (`BOARD_USE::FPHOLDER`), whose footprint resolves no
// text variable (`FOOTPRINT::ResolveTextVar` returns false), so the Reference shows `REF**` and a text `${REFERENCE}` shows as it is written.
//
// Pure: no store, no DOM. Unit-tested in fpItems.test.ts.
import { isHiddenText, isLockedItem, textLookOf } from "./fpKeep";
import type { Cmd, CmdShape, CmdText, FieldInfo, FieldLayoutCmd, Group, LibraryFootprint, LibraryPad } from "../api/types";
import type { FlipDirectionWire, FootprintItemsDisplay, FpZoneDisplay } from "../api/fpItemTypes";
import { defaultInfoFieldLayout, defaultReferenceLayout, defaultValueLayout, LIBRARY_REFERENCE_TEXT, newFieldLayout } from "./fpLibFields";
import { fieldAsText } from "./fpFields";
import { padBox, cmdShapeToShape, type Box } from "./itemBoxes";
import { shapeBoundingBox, textBoundingBox, distToSegment } from "../components/canvas/itemHitTest";
import { padContains } from "./libEditorHit";
import { extraHitDistance, extraItems } from "./pcbExtraItems";
import { polygonDistance, tableBox } from "./tables";
import { bezierPolyline } from "./bezierPoly";
import { groupLeaves, isGroup, parentGroup } from "./groupTree";
import { arcShapeCenter } from "./pcbItems";
import { fpFieldSelectable, fpItemSelectable, fpLayersSelectable, fpPadSelectable, fpPassesFilter, fpShapeSelectable, fpTextSelectable, fpZoneSelectable, type FpVisibility } from "./fpAppearance";
import { zoneLayers } from "./zoneLayers";

export const FP_FIELD_PREFIX = "field:";
export const fpFieldId = (name: string): string => `${FP_FIELD_PREFIX}${name}`;
/** The name of the field `id` names (`field:Reference` -> `Reference`), or null. */
export function fpFieldName(id: string): string | null {
  return id.startsWith(FP_FIELD_PREFIX) ? id.slice(FP_FIELD_PREFIX.length) : null;
}

export type FpItemKind = "pad" | "graphic" | "text" | "field" | "dimension" | "zone" | "textbox" | "point" | "image" | "barcode" | "table" | "group";

/** What the footprint's lists look like to these helpers (the client defaults the IR's omitted empty lists to `[]`). */
export type Fp = Pick<LibraryFootprint, "name" | "pads" | "graphics" | "texts" | "fields" | "reference" | "value" | "reference_visible" | "value_visible" | "datasheet" | "description_field" | "items" | "display">;

const EMPTY_DISPLAY: FootprintItemsDisplay = { dimensions: [], zones: [], text_boxes: [], images: [], points: [], barcodes: [], tables: [] };
export const displayOf = (fp: Pick<Fp, "display">): FootprintItemsDisplay => ({ ...EMPTY_DISPLAY, ...(fp.display ?? {}) });
export const groupsOf = (fp: Pick<Fp, "items">): Group[] => fp.items?.groups ?? [];

// ------------------------------------------------------------------------------------------------------------------------------ fields

/** One field's layout: its own, else the place KiCad gives a field of no layout (`CreateNewFootprint`'s Reference and Value, `OnAddField`'s user field). */
function layoutOrDefault(l: FieldLayoutCmd | null | undefined, fallback: () => FieldLayoutCmd, visible: boolean): { layout: FieldLayoutCmd; custom: boolean } {
  return { layout: { ...(l ?? fallback()), visible }, custom: l != null };
}

function fieldInfo(name: string, text: string, l: { layout: FieldLayoutCmd; custom: boolean }): FieldInfo {
  const { layout: f, custom } = l;
  const [w, h] = f.size;
  return {
    id: fpFieldId(name),
    name,
    text,
    x: f.at.x,
    y: f.at.y,
    angle: f.angle ?? 0,
    w,
    h,
    thickness: f.thickness ?? 0,
    layer: f.layer,
    visible: f.visible !== false,
    halign: f.halign ?? 0,
    valign: f.valign ?? 0,
    mirror: f.mirror ?? false,
    bold: f.bold ?? false,
    italic: f.italic ?? false,
    upright: f.keep_upright !== false,
    knockout: f.knockout ?? false,
    lx: f.at.x,
    ly: f.at.y,
    langle: f.angle ?? 0,
    custom,
  };
}

/**
 * The fields of the footprint in KiCad's order (`FOOTPRINT::GetFields`: Reference, Value, Datasheet, Description, then the user fields), each as the board's
 * `FieldInfo` so the board's painter draws it (`painter.ts::drawFields`): in the footprint's frame, which is the canvas's here. The text is the field's own: the
 * Reference `REF**` unless it has another, the Value the footprint's name (`PCB_BASE_FRAME::CreateNewFootprint`).
 */
export function fpFields(fp: Fp): FieldInfo[] {
  const out: FieldInfo[] = [
    fieldInfo("Reference", fp.reference?.text ?? LIBRARY_REFERENCE_TEXT, layoutOrDefault(fp.reference, defaultReferenceLayout, fp.reference_visible !== false)),
    fieldInfo("Value", fp.value?.text ?? fp.name, layoutOrDefault(fp.value, defaultValueLayout, fp.value_visible !== false)),
  ];
  if (fp.datasheet) out.push(fieldInfo("Datasheet", fp.datasheet.text ?? "", layoutOrDefault(fp.datasheet, defaultInfoFieldLayout, fp.datasheet.visible !== false)));
  if (fp.description_field) out.push(fieldInfo("Description", fp.description_field.text ?? "", layoutOrDefault(fp.description_field, defaultInfoFieldLayout, fp.description_field.visible !== false)));
  for (const f of fp.fields ?? []) out.push(fieldInfo(f.name, f.value, layoutOrDefault(f.layout, newFieldLayout, f.visible)));
  return out;
}

export function fpFieldById(fp: Fp, id: string): FieldInfo | undefined {
  return fpFields(fp).find((f) => f.id === id);
}

/** The layout a field's dialog starts from and sends back (`FieldInfo` -> `FieldLayoutCmd`). */
export function fieldLayoutOf(f: FieldInfo): FieldLayoutCmd {
  // the field's outline font, so an edit of anything else keeps it
  return { at: { x: f.x, y: f.y }, angle: f.angle, size: [f.w, f.h], thickness: f.thickness, layer: f.layer, visible: f.visible, halign: f.halign, valign: f.valign, mirror: f.mirror, bold: f.bold, italic: f.italic, keep_upright: f.upright, knockout: f.knockout, ...(f.face ? { face: f.face } : {}) };
}

// ------------------------------------------------------------------------------------------------------------------------------ kinds

/** What kind of item `id` names in the footprint, or null. */
export function fpItemKind(fp: Fp, id: string): FpItemKind | null {
  if (fpFieldName(id) !== null) return fpFieldById(fp, id) ? "field" : null;
  if (fp.pads.some((p) => p.id === id)) return "pad";
  if (fp.graphics.some((g) => g.id === id)) return "graphic";
  if (fp.texts.some((t) => t.id === id)) return "text";
  const d = displayOf(fp);
  if (d.dimensions.some((x) => x.id === id)) return "dimension";
  if (d.zones.some((x) => x.id === id)) return "zone";
  if (d.text_boxes.some((x) => x.id === id)) return "textbox";
  if (d.points.some((x) => x.id === id)) return "point";
  if (d.images.some((x) => x.id === id)) return "image";
  if (d.barcodes.some((x) => x.id === id)) return "barcode";
  if (d.tables.some((x) => x.id === id)) return "table";
  if (isGroup(groupsOf(fp), id)) return "group";
  return null;
}

/** Every id of an item of the footprint (fields included, groups not). */
export function fpItemIds(fp: Fp): string[] {
  const d = displayOf(fp);
  const ids = [...fp.pads.map((p) => p.id), ...fp.graphics.map((g) => g.id), ...fp.texts.map((t) => t.id), ...fpFields(fp).map((f) => f.id)];
  ids.push(...d.dimensions.map((x) => x.id), ...d.zones.map((x) => x.id), ...d.text_boxes.map((x) => x.id), ...d.points.map((x) => x.id), ...d.images.map((x) => x.id), ...d.barcodes.map((x) => x.id), ...d.tables.map((x) => x.id));
  return ids.filter((id): id is string => !!id);
}

// --------------------------------------------------------------------------------------------------------------------- positions, boxes

const zoneRing = (z: FpZoneDisplay): [number, number][] => z.outline_xy;

function boxOf(pts: readonly (readonly [number, number])[]): Box | null {
  if (pts.length === 0) return null;
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

function union(boxes: readonly (Box | null)[]): Box | null {
  let out: Box | null = null;
  for (const b of boxes) if (b) out = out ? [Math.min(out[0], b[0]), Math.min(out[1], b[1]), Math.max(out[2], b[2]), Math.max(out[3], b[3])] : b;
  return out;
}

const textOfCmd = (t: CmdText) => ({ id: t.id ?? "", content: t.content, x: t.at.x, y: t.at.y, angle: t.angle, layer: t.layer, size: t.size_nm, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror });

/** A text's box with its angle (`EDA_TEXT::GetTextBox` turned; the angle is KiCad's, counter-clockwise). */
function turnedTextBox(t: ReturnType<typeof textOfCmd>): Box {
  const b = textBoundingBox(t);
  const rad = (t.angle / 1000) * (Math.PI / 180);
  if (rad === 0) return [b.x0, b.y0, b.x1, b.y1];
  const [c, s] = [Math.cos(rad), Math.sin(rad)];
  const corners: [number, number][] = [
    [b.x0, b.y0],
    [b.x1, b.y0],
    [b.x1, b.y1],
    [b.x0, b.y1],
  ].map(([x, y]) => [t.x + (x! - t.x) * c + (y! - t.y) * s, t.y - (x! - t.x) * s + (y! - t.y) * c]);
  return boxOf(corners)!;
}

function fieldBox(f: FieldInfo): Box {
  // The anchor is where the text box is centred (`drawFields` moves the baseline down by `0.36 h`); `textBoundingBox` puts the text about its baseline.
  const t = fieldAsText(f);
  const down = f.valign > 0 ? 0 : f.valign < 0 ? 0.72 * f.h : 0.36 * f.h;
  const rad = (-t.angle / 1000) * (Math.PI / 180);
  return turnedTextBox({ ...t, x: t.x - down * Math.sin(rad), y: t.y + down * Math.cos(rad), size: f.h });
}

/** A text of the footprint's box: its anchor is where the text box is centred (or its top or bottom edge sits: the kept vertical justification, UI-LEFT F5), as
 *  footprintPainter.ts draws it -- the painter puts the baseline `0.36 h` below a centred text's anchor (`0.72 h` below a top-justified one's, on a bottom-justified
 *  one's) and the glyphs' middle `0.36 h` above the baseline, where `textBoundingBox` centres its box; so a centred text's box is about its anchor and a top- or
 *  bottom-justified one's is moved `0.36 h` down or up. */
function cmdTextBox(fp: Fp, t: CmdText): Box {
  const valign = t.id ? textLookOf(fp as { kicad?: Record<string, unknown> }, t.id).valign : 0;
  const base = textOfCmd(t);
  const down = valign > 0 ? -0.36 * base.size : valign < 0 ? 0.36 * base.size : 0;
  const rad = (-base.angle / 1000) * (Math.PI / 180);
  return turnedTextBox({ ...base, x: base.x - down * Math.sin(rad), y: base.y + down * Math.cos(rad) });
}

/** `EDA_ITEM::GetBoundingBox()` of the item `id`, `[x0, y0, x1, y1]`; a group's is its members'. */
export function fpItemBounds(fp: Fp, id: string): Box | null {
  const kind = fpItemKind(fp, id);
  const d = displayOf(fp);
  switch (kind) {
    case "pad":
      return padBox(fp.pads.find((p) => p.id === id)!);
    case "graphic":
      return shapeBoundingBox(cmdShapeToShape(fp.graphics.find((g) => g.id === id)!));
    case "text":
      return cmdTextBox(fp, fp.texts.find((t) => t.id === id)!);
    case "field":
      return fieldBox(fpFieldById(fp, id)!);
    case "dimension": {
      const dim = d.dimensions.find((x) => x.id === id)!;
      const half = dim.text_size_nm * Math.max(dim.text.length, 1) * 0.3;
      return union([boxOf(dim.lines.flatMap(([a, b]) => [a, b])), [dim.text_at[0] - half, dim.text_at[1] - dim.text_size_nm / 2, dim.text_at[0] + half, dim.text_at[1] + dim.text_size_nm / 2]]);
    }
    case "zone":
      return boxOf(zoneRing(d.zones.find((x) => x.id === id)!));
    case "table":
      return tableBox(d.tables.find((x) => x.id === id)!);
    case "textbox":
    case "point":
    case "image":
    case "barcode":
      return extraItems({ drawings: d as never }).find((x) => x.id === id)?.bounds ?? null;
    case "group":
      return union(groupLeaves(groupsOf(fp), id).map((m) => fpItemBounds(fp, m)));
    default:
      return null;
  }
}

/** The centre of an arc's circle (through its three points), or null for three points on a line. */
function arcCenter(g: Extract<CmdShape, { kind: "arc" }>): [number, number] | null {
  const c = arcShapeCenter(cmdShapeToShape(g) as Extract<ReturnType<typeof cmdShapeToShape>, { kind: "arc" }>);
  return c ? [c[0], c[1]] : null;
}

/** `PCB_SHAPE::GetPosition` -> `EDA_SHAPE::getPosition`: an arc's and a circle's centre, a polygon's first corner, the start of the others. */
function shapePosition(g: CmdShape): [number, number] {
  switch (g.kind) {
    case "circle":
      return [g.center.x, g.center.y];
    case "arc":
      return arcCenter(g) ?? [g.mid.x, g.mid.y];
    case "polygon":
      return [g.pts[0]!.x, g.pts[0]!.y];
    default:
      return [g.start.x, g.start.y];
  }
}

/** `BOARD_ITEM::GetPosition()` of the item `id`. */
export function fpItemPosition(fp: Fp, id: string): [number, number] | null {
  const d = displayOf(fp);
  switch (fpItemKind(fp, id)) {
    case "pad": {
      const p = fp.pads.find((x) => x.id === id)!;
      return [p.at.x, p.at.y];
    }
    case "graphic":
      return shapePosition(fp.graphics.find((g) => g.id === id)!);
    case "text": {
      const t = fp.texts.find((x) => x.id === id)!;
      return [t.at.x, t.at.y];
    }
    case "field": {
      const f = fpFieldById(fp, id)!;
      return [f.x, f.y];
    }
    case "dimension": {
      const dim = d.dimensions.find((x) => x.id === id)!;
      return [dim.start[0], dim.start[1]];
    }
    case "zone":
      return d.zones.find((x) => x.id === id)!.outline_xy[0] ?? null; // `ZONE::GetPosition`: its first corner
    case "table": {
      const t = d.tables.find((x) => x.id === id)!;
      return [t.x, t.y];
    }
    case "textbox":
    case "point":
    case "image":
    case "barcode": {
      const item = extraItems({ drawings: d as never }).find((x) => x.id === id);
      return item ? [item.position[0], item.position[1]] : null;
    }
    case "group": {
      const b = fpItemBounds(fp, id); // `PCB_GROUP::GetPosition`: the centre of its box
      return b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : null;
    }
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------------------------------------------------------- picking

function shapeHitDistance(s: CmdShape, wx: number, wy: number): number {
  const segs = (pts: readonly (readonly [number, number])[], closed: boolean) => {
    let best = Infinity;
    for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(wx, wy, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
    if (closed && pts.length > 2) best = Math.min(best, distToSegment(wx, wy, pts[pts.length - 1]![0], pts[pts.length - 1]![1], pts[0]![0], pts[0]![1]));
    return best;
  };
  switch (s.kind) {
    case "circle": {
      const r = Math.hypot(s.end.x - s.center.x, s.end.y - s.center.y);
      const d = Math.abs(Math.hypot(wx - s.center.x, wy - s.center.y) - r);
      return s.filled && Math.hypot(wx - s.center.x, wy - s.center.y) <= r ? 0 : d;
    }
    case "rect": {
      const ring: [number, number][] = [
        [s.start.x, s.start.y],
        [s.end.x, s.start.y],
        [s.end.x, s.end.y],
        [s.start.x, s.end.y],
      ];
      const inside = wx >= Math.min(s.start.x, s.end.x) && wx <= Math.max(s.start.x, s.end.x) && wy >= Math.min(s.start.y, s.end.y) && wy <= Math.max(s.start.y, s.end.y);
      return s.filled && inside ? 0 : segs(ring, true);
    }
    case "polygon": {
      const ring = s.pts.map((p) => [p.x, p.y] as [number, number]);
      return s.filled && polygonDistance(ring, wx, wy) === 0 ? 0 : segs(ring, true);
    }
    case "bezier":
      return segs(bezierPolyline([s.start.x, s.start.y], [s.c1.x, s.c1.y], [s.c2.x, s.c2.y], [s.end.x, s.end.y], 5), false);
    case "arc": {
      const center = arcCenter(s);
      if (!center) return segs([[s.start.x, s.start.y], [s.mid.x, s.mid.y], [s.end.x, s.end.y]], false);
      // the circle's distance, limited to the arc's sweep by the chord test: near the circle and on the arc's side of the chord through its ends
      const [cx, cy] = center;
      const r = Math.hypot(s.start.x - cx, s.start.y - cy);
      const onCircle = Math.abs(Math.hypot(wx - cx, wy - cy) - r);
      const side = (px: number, py: number) => (s.end.x - s.start.x) * (py - s.start.y) - (s.end.y - s.start.y) * (px - s.start.x);
      const sameSide = Math.sign(side(wx, wy)) === Math.sign(side(s.mid.x, s.mid.y));
      return sameSide ? onCircle : Math.min(Math.hypot(wx - s.start.x, wy - s.start.y), Math.hypot(wx - s.end.x, wy - s.end.y));
    }
    case "segment":
    default:
      return segs([[s.start.x, s.start.y], [s.end.x, s.end.y]], false);
  }
}

function inBox(b: Box | null, wx: number, wy: number, tol: number): boolean {
  return !!b && wx >= b[0] - tol && wx <= b[2] + tol && wy >= b[1] - tol && wy <= b[3] + tol;
}

/**
 * The topmost item under `(wx, wy)` (`PCB_SELECTION_TOOL::selectPoint`'s collector, narrowed to one): pads first, then the fields and the texts (a hidden field is
 * not shown and so not picked), the text boxes, points, images, barcodes and tables, the dimensions, the graphics, and the zones by their outline (or inside a
 * rule area). `tol` is the click's slop in nm.
 *
 * With `vis` (the Appearance manager's switches, kicad-port/fpAppearance.ts) only what `PCB_SELECTION_TOOL::Selectable` lets through is picked: an item on a hidden
 * layer, or whose object row is off or at opacity 0, is not, and with high contrast only an item on the active layer is. Without it every item can be picked.
 */
export function fpHitTest(fp: Fp, wx: number, wy: number, tol: number, vis?: FpVisibility): { kind: FpItemKind; id: string } | null {
  // the selection filter (`itemPassesFilter`, UI-LEFT-2 FF5): a kind whose row is off, a locked item while Locked items is off
  const filter = vis?.filter;
  const passes = (kind: string, id: string | undefined, ruleArea = false): boolean => !filter || fpPassesFilter(filter, kind, fpItemLocked(fp, id), ruleArea);
  for (let i = fp.pads.length - 1; i >= 0; i--) {
    const p = fp.pads[i]!;
    if (vis && !fpPadSelectable(vis, p.layers, p.kind === "through_hole" || p.kind === "non_plated_hole")) continue;
    if (!passes("pad", p.id)) continue;
    if (p.id && padContains(p, wx, wy)) return { kind: "pad", id: p.id };
  }
  for (const f of [...fpFields(fp)].reverse()) if (f.visible && f.text && (!vis || fpFieldSelectable(vis, f)) && passes("field", f.id) && inBox(fieldBox(f), wx, wy, tol / 2)) return { kind: "field", id: f.id };
  for (let i = fp.texts.length - 1; i >= 0; i--) {
    const t = fp.texts[i]!;
    if ((vis && !fpTextSelectable(vis, t)) || !passes("text", t.id)) continue;
    // a text read hidden is neither drawn nor picked (`PCB_TEXT::ViewGetLOD`, UI-LEFT F5)
    if (isHiddenText(fp as { kicad?: Record<string, unknown> }, t.id)) continue;
    if (t.id && inBox(cmdTextBox(fp, t), wx, wy, tol / 2)) return { kind: "text", id: t.id };
  }
  const d = displayOf(fp);
  for (const item of extraItems({ drawings: d as never }).reverse()) if ((!vis || fpItemSelectable(vis, item.kind, item.layer)) && passes(item.kind, item.id) && extraHitDistance(item, wx, wy) <= tol) return { kind: item.kind as FpItemKind, id: item.id };
  for (const t of [...d.tables].reverse()) if ((!vis || fpLayersSelectable(vis, [t.layer])) && passes("table", t.id) && inBox(tableBox(t), wx, wy, tol)) return { kind: "table", id: t.id };
  for (const dim of [...d.dimensions].reverse()) {
    if ((vis && !fpLayersSelectable(vis, [dim.layer])) || !passes("dimension", dim.id)) continue;
    const near = dim.lines.some(([a, b]) => distToSegment(wx, wy, a[0], a[1], b[0], b[1]) <= Math.max(tol, dim.stroke_width / 2));
    if (near || inBox(fpItemBounds(fp, dim.id), wx, wy, 0) && Math.hypot(wx - dim.text_at[0], wy - dim.text_at[1]) <= dim.text_size_nm * Math.max(dim.text.length, 1) * 0.35) return { kind: "dimension", id: dim.id };
  }
  for (let i = fp.graphics.length - 1; i >= 0; i--) {
    const g = fp.graphics[i]!;
    if ((vis && !fpShapeSelectable(vis, g)) || !passes("graphic", g.id)) continue;
    if (g.id && shapeHitDistance(g, wx, wy) <= Math.max(tol, g.stroke_width / 2)) return { kind: "graphic", id: g.id };
  }
  for (const z of [...d.zones].reverse()) {
    if ((vis && !fpZoneSelectable(vis, zoneLayers(z))) || !passes("zone", z.id, !!z.is_rule_area)) continue;
    const ring = zoneRing(z);
    let edge = Infinity;
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i]!;
      const b = ring[(i + 1) % ring.length]!;
      edge = Math.min(edge, distToSegment(wx, wy, a[0], a[1], b[0], b[1]));
    }
    if (edge <= tol || (z.is_rule_area && polygonDistance(ring, wx, wy) === 0)) return { kind: "zone", id: z.id };
  }
  return null;
}

/** `BOARD_ITEM::IsLocked()` of an item of the footprint: the lock its file gave it (`LibraryFootprintKeep::locked`, a pad's own `(locked yes)`). */
export function fpItemLocked(fp: Fp, id: string | undefined): boolean {
  if (!id) return false;
  if (isLockedItem(fp as { kicad?: Record<string, unknown> }, id)) return true;
  const pad = fp.pads.find((p) => p.id === id);
  return !!pad && ((pad.kicad as { tokens?: string[] } | undefined)?.tokens ?? []).some((t) => /^\(locked( yes)?\)$/.test(t.trim()));
}

/**
 * `PCB_SELECTION_TOOL::FilterCollectorForHierarchy`: a click on an item of a group selects the outermost group that holds it -- inside the group being worked in
 * (`m_enteredGroup`, `PCB_GROUP::TopLevelGroup( item, m_enteredGroup, .. )`), the outermost one below it. With a group entered, an item outside it cannot be picked
 * (`!PCB_GROUP::WithinScope( item, m_enteredGroup, .. )`): null.
 */
export function fpSelectionTarget(fp: Fp, id: string, entered: string | null = null): string | null {
  const groups = groupsOf(fp);
  let at = id;
  for (let hops = 0; hops < 64; hops++) {
    const g = parentGroup(groups, at);
    if (!g) return entered ? null : at;
    if (g.id === entered) return at;
    at = g.id;
  }
  return entered ? null : at;
}

/** `PCB_SELECTION_TOOL::EnterGroup`: what entering the group `id` selects -- its members, each as it is (a group inside it too). Empty for an id that names no group. */
export function fpGroupMembers(fp: Fp, id: string): string[] {
  return groupsOf(fp).find((g) => g.id === id)?.member_ids.slice() ?? [];
}

/** The items a selection stands for: each group replaced by the items below it. */
export function fpLeaves(fp: Fp, ids: readonly string[]): string[] {
  const groups = groupsOf(fp);
  const out: string[] = [];
  for (const id of ids) for (const m of isGroup(groups, id) ? groupLeaves(groups, id) : [id]) if (!out.includes(m)) out.push(m);
  return out;
}

// ------------------------------------------------------------------------------------------------------------------- reference points

export type Snap = (p: readonly [number, number]) => [number, number];
const NO_SNAP: Snap = (p) => [p[0], p[1]];

/**
 * `EDIT_TOOL::updateModificationPoint` in the footprint editor: one item (not a table) turns about its own position; several about the centre of their box, snapped to
 * the grid -- the box of what is not a text or a field when there is any ("Exclude text in the footprint editor if there's anything else selected").
 */
export function fpModificationPoint(fp: Fp, ids: readonly string[], snap: Snap = NO_SNAP): [number, number] | null {
  if (ids.length === 0) return null;
  if (ids.length === 1 && fpItemKind(fp, ids[0]!) !== "table") return fpItemPosition(fp, ids[0]!);
  const all = union(ids.map((id) => fpItemBounds(fp, id)));
  const nonText = union(ids.filter((id) => !["text", "field"].includes(fpItemKind(fp, id) ?? "")).map((id) => fpItemBounds(fp, id)));
  const b = nonText ?? all;
  return b ? snap([(b[0] + b[2]) / 2, (b[1] + b[3]) / 2]) : null;
}

/** `EDIT_TOOL::Rotate`'s pivot: a lone text box about its centre, a lone rectangle, polygon or table about its centre (`usePcbShapeCenter`), else the modification point. */
export function fpRotationPivot(fp: Fp, ids: readonly string[], snap: Snap = NO_SNAP): [number, number] | null {
  if (ids.length === 1) {
    const id = ids[0]!;
    const kind = fpItemKind(fp, id);
    const g = kind === "graphic" ? fp.graphics.find((x) => x.id === id) : undefined;
    if (kind === "textbox" || kind === "table" || (g && (g.kind === "rect" || g.kind === "polygon"))) {
      const b = fpItemBounds(fp, id);
      return b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : null;
    }
  }
  return fpModificationPoint(fp, ids, snap);
}

// -------------------------------------------------------------------------------------------------------------------------------- verbs

const xy = (p: readonly [number, number]) => ({ x: Math.round(p[0]), y: Math.round(p[1]) });

/** `EDIT_TOOL::Move`'s commit: the selection by `(dx, dy)`, one `fp_item move`. */
export function fpMoveCmd(footprint: string, ids: readonly string[], dx: number, dy: number): Cmd | null {
  if (ids.length === 0 || (dx === 0 && dy === 0)) return null;
  return { op: "fp_item", verb: "move", footprint, ids: [...ids], dx: Math.round(dx), dy: Math.round(dy) };
}

/** `EDIT_TOOL::Rotate` by `quarterTurnsCcw` steps of `stepDeg` (the footprint editor's rotation angle, 90 degrees), about `fpRotationPivot`. */
export function fpRotateCmd(fp: Fp, footprint: string, ids: readonly string[], quarterTurnsCcw: number, snap: Snap = NO_SNAP, stepDeg = 90): Cmd | null {
  const pivot = fpRotationPivot(fp, ids, snap);
  if (!pivot || ids.length === 0) return null;
  // clockwise-positive on the wire (`rotate_items`); KiCad's Rotate Counterclockwise is a positive KiCad angle
  return { op: "fp_item", verb: "rotate", footprint, ids: [...ids], pivot: xy(pivot), angle_millideg: Math.round(-quarterTurnsCcw * stepDeg * 1000) };
}

/** `EDIT_TOOL::MirrorableItems` (the board's list, of the kinds a footprint has): shapes, fields, texts, text boxes, zones, pads, groups, points. */
const MIRRORABLE: readonly FpItemKind[] = ["graphic", "field", "text", "textbox", "zone", "pad", "group", "point"];

/** Whether the selection holds an item Mirror acts on (`canMirror`: `SELECTION_CONDITIONS::HasTypes( EDIT_TOOL::MirrorableItems )`). */
export function fpCanMirror(fp: Fp, ids: readonly string[]): boolean {
  return ids.some((id) => MIRRORABLE.includes(fpItemKind(fp, id) as FpItemKind));
}

/** `EDIT_TOOL::Mirror` (`mirrorH`: left-right, `mirrorV`: top-bottom) about the modification point. */
export function fpMirrorCmd(fp: Fp, footprint: string, ids: readonly string[], direction: FlipDirectionWire, snap: Snap = NO_SNAP): Cmd | null {
  if (!fpCanMirror(fp, ids)) return null;
  const pivot = fpModificationPoint(fp, ids, snap);
  if (!pivot) return null;
  return { op: "fp_item", verb: "mirror", footprint, ids: [...ids], pivot: xy(pivot), direction };
}

/** `PCB_GROUP_TOOL::Group` (two groupable items or more) and `Ungroup` (the groups among the selection). */
export function fpGroupCmd(fp: Fp, footprint: string, ids: readonly string[]): Cmd | null {
  const groupable = ids.filter((id) => fpItemKind(fp, id) !== "point" && fpItemKind(fp, id) !== null);
  return groupable.length >= 2 ? { op: "fp_item", verb: "group", footprint, ids: groupable } : null;
}

export function fpUngroupCmd(fp: Fp, footprint: string, ids: readonly string[]): Cmd | null {
  const groups = ids.filter((id) => isGroup(groupsOf(fp), id));
  return groups.length > 0 ? { op: "fp_item", verb: "ungroup", footprint, ids: groups } : null;
}

/** What `FOOTPRINT_EDIT_FRAME::OnEditItemRequest` opens for the item: its dialog, or nothing (a point has none). */
export type FpEditTarget =
  | { dialog: "pad"; id: string }
  | { dialog: "text"; id: string }
  | { dialog: "field"; id: string }
  | { dialog: "shape"; id: string }
  | { dialog: "textbox"; id: string }
  | { dialog: "dimension"; id: string }
  | { dialog: "zone"; id: string }
  | { dialog: "table"; id: string }
  | { dialog: "barcode"; id: string }
  | { dialog: "image"; id: string }
  | { dialog: "group"; id: string }
  | { dialog: "footprint" }
  | null;

/** `EDIT_TOOL::Properties` in the footprint editor: one item opens its dialog (`OnEditItemRequest`), nothing selected opens Footprint Properties. */
export function fpEditTarget(fp: Fp, ids: readonly string[]): FpEditTarget {
  if (ids.length === 0) return { dialog: "footprint" };
  if (ids.length !== 1) return null;
  const id = ids[0]!;
  switch (fpItemKind(fp, id)) {
    case "pad":
      return { dialog: "pad", id };
    case "field":
      return { dialog: "field", id };
    case "text":
      return { dialog: "text", id };
    case "graphic":
      return { dialog: "shape", id };
    case "textbox":
      return { dialog: "textbox", id };
    case "dimension":
      return { dialog: "dimension", id };
    case "zone":
      return { dialog: "zone", id };
    case "table":
      return { dialog: "table", id };
    case "barcode":
      return { dialog: "barcode", id };
    case "image":
      return { dialog: "image", id };
    case "group":
      return { dialog: "group", id };
    default:
      return null; // `case PCB_POINT_T: break;`
  }
}

/** Every id the footprint knows, for a "which items are new" diff after a command. */
export function fpAllIds(fp: Fp): Set<string> {
  return new Set([...fpItemIds(fp), ...groupsOf(fp).map((g) => g.id)]);
}

/** The pad `id` (a convenience for the canvas). */
export const fpPad = (fp: Fp, id: string): LibraryPad | undefined => fp.pads.find((p) => p.id === id);
