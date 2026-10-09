// Small lookups over the studio's board view shared by the pcbnew edit-tool
// ports (Mirror, Fillet Tracks, Break Track, selection by net, the
// Convert tool...): what kind of item an id names, where `GetPosition()`
// puts it, and its bounding box.
//
// Source references (pcbnew at 8303b2ad): `EDA_SHAPE::getPosition`
// (common/eda_shape.cpp), `PCB_TRACK::GetPosition` (start),
// `ZONE::GetPosition` (first corner), `PCB_TEXT::GetPosition`.

import type { BoardState, BoardText, Pad, Part, Shape } from "../api/types";
import { shapeBoundingBox, textBoundingBox } from "../components/canvas/itemHitTest";
import { circumcircle } from "./trackArc";
import { fieldAsText, fieldById } from "./fpFields";

export type ItemKind = "part" | "track" | "via" | "zone" | "shape" | "text" | "dimension" | "group" | "pad" | "field";

/**
 * The id a pad is selected, highlighted and listed under: `REF.NUMBER`, with `#k` after it for the k-th pad (from 2) of the footprint that
 * shares the number (a shield tab, several pads on one pin). Pads have no id of their own in the board state; the footprint's reference makes
 * this one unique, and a pad is always found through its footprint.
 */
export function padIds(part: { ref: string; pads?: readonly { num: string }[] }): string[] {
  const seen = new Map<string, number>();
  return (part.pads ?? []).map((pad) => {
    const n = (seen.get(pad.num) ?? 0) + 1;
    seen.set(pad.num, n);
    return n === 1 ? `${part.ref}.${pad.num}` : `${part.ref}.${pad.num}#${n}`;
  });
}

/** The pad `id` names and the footprint it belongs to, or null (a `REF.NUMBER[#k]` of a placed footprint). */
export function padById(board: BoardState, id: string): { part: Part; pad: Pad; index: number } | null {
  const dot = id.indexOf(".");
  if (dot <= 0) return null;
  const part = board.parts.find((p) => p.placed && p.ref === id.slice(0, dot));
  if (!part) return null;
  const index = padIds(part).indexOf(id);
  return index < 0 ? null : { part, pad: part.pads![index]!, index };
}

/** The footprint a pad id belongs to (`FilterCollectorForFreePads`: in the board editor a pad is edited through its footprint), else null. */
export function padParent(board: BoardState, id: string): string | null {
  return padById(board, id)?.part.ref ?? null;
}

/** Which kind of item `id` names (a placed part's reference, or a track/via/zone/shape/text/dimension/group id). */
export function itemKind(board: BoardState, id: string): ItemKind | null {
  if (board.parts.some((p) => p.ref === id && p.placed)) return "part";
  const rt = board.routing;
  if (rt?.tracks.some((t) => t.id === id)) return "track";
  if (rt?.vias.some((v) => v.id === id)) return "via";
  if (rt?.zones.some((z) => z.id === id)) return "zone";
  const dr = board.drawings;
  if (dr?.shapes.some((s) => s.id === id)) return "shape";
  if (dr?.texts.some((t) => t.id === id)) return "text";
  if (dr?.dimensions.some((d) => d.id === id)) return "dimension";
  if (dr?.groups.some((g) => g.id === id)) return "group";
  if (id.includes(".") && padById(board, id)) return "pad";
  // A footprint's Reference, Value or user field (`PCB_FIELD`): an item of its own, `REF:Name`.
  if (id.includes(":") && fieldById(board, id)) return "field";
  return null;
}

export const shapeById = (board: BoardState, id: string): Shape | undefined => board.drawings?.shapes.find((s) => s.id === id);

/** `PCB_SHAPE::GetCenter` for an arc: the circle through start, mid and end. */
export function arcShapeCenter(s: Extract<Shape, { kind: "arc" }>): [number, number] | null {
  const c = circumcircle(s.start, s.mid, s.end);
  return c ? [c.cx, c.cy] : null;
}

/** `BOARD_ITEM::GetPosition()` of the item (arcs: their centre, circles: their centre, polygons: the first vertex, other shapes: the start point). */
export function itemPosition(board: BoardState, id: string): [number, number] | null {
  const kind = itemKind(board, id);
  switch (kind) {
    case "part": {
      const p = board.parts.find((q) => q.ref === id);
      return p?.at ? [p.at[0], p.at[1]] : null;
    }
    case "track": {
      const t = board.routing?.tracks.find((q) => q.id === id);
      return t?.pts[0] ? [t.pts[0][0], t.pts[0][1]] : null;
    }
    case "via": {
      const v = board.routing?.vias.find((q) => q.id === id);
      return v ? [v.x, v.y] : null;
    }
    case "zone": {
      const z = board.routing?.zones.find((q) => q.id === id);
      return z?.outline[0] ? [z.outline[0][0], z.outline[0][1]] : null;
    }
    case "shape": {
      const s = shapeById(board, id);
      if (!s) return null;
      if (s.kind === "arc") return arcShapeCenter(s) ?? [s.start[0], s.start[1]];
      if (s.kind === "circle") return [s.center[0], s.center[1]];
      if (s.kind === "polygon") return s.pts[0] ? [s.pts[0][0], s.pts[0][1]] : null;
      return [s.start[0], s.start[1]];
    }
    case "text": {
      const t = board.drawings?.texts.find((q) => q.id === id);
      return t ? [t.x, t.y] : null;
    }
    // `PCB_DIMENSION_BASE::GetPosition()` is its first feature point.
    case "dimension": {
      const d = board.drawings?.dimensions.find((q) => q.id === id);
      return d ? [d.start[0], d.start[1]] : null;
    }
    // `PCB_GROUP::GetPosition()` is the centre of its bounding box.
    case "group": {
      const b = itemBounds(board, id);
      return b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : null;
    }
    case "pad": {
      const hit = padById(board, id);
      return hit ? [hit.pad.x, hit.pad.y] : null;
    }
    case "field": {
      const hit = fieldById(board, id);
      return hit ? [hit.field.x, hit.field.y] : null;
    }
    default:
      return null;
  }
}

/** The item's bounding box `[x0, y0, x1, y1]` (copper and strokes include half their width, like `GetBoundingBox`). */
export function itemBounds(board: BoardState, id: string): [number, number, number, number] | null {
  const kind = itemKind(board, id);
  switch (kind) {
    case "part": {
      const p = board.parts.find((q) => q.ref === id);
      if (!p?.at) return null;
      if (p.courtyard) return [p.courtyard[0], p.courtyard[1], p.courtyard[2], p.courtyard[3]];
      return [p.at[0], p.at[1], p.at[0], p.at[1]];
    }
    case "track": {
      const t = board.routing?.tracks.find((q) => q.id === id);
      return t ? padded(boundsOf(t.pts), t.width / 2) : null;
    }
    case "via": {
      const v = board.routing?.vias.find((q) => q.id === id);
      return v ? [v.x - v.d / 2, v.y - v.d / 2, v.x + v.d / 2, v.y + v.d / 2] : null;
    }
    case "zone": {
      const z = board.routing?.zones.find((q) => q.id === id);
      return z ? boundsOf(z.outline) : null;
    }
    case "shape": {
      const s = shapeById(board, id);
      return s ? padded(shapeBoundingBox(s), s.stroke_width / 2) : null;
    }
    case "text": {
      const t = board.drawings?.texts.find((q) => q.id === id);
      return t ? textBounds(t) : null;
    }
    // Every line of the dimension and its text (`PCB_DIMENSION_BASE::GetBoundingBox` joins its shapes and its text).
    case "dimension": {
      const d = board.drawings?.dimensions.find((q) => q.id === id);
      if (!d) return null;
      const pts: [number, number][] = d.lines.flatMap(([a, b]) => [a, b]);
      const text = d.text ? textBounds({ id: d.id, content: d.text, x: d.text_at[0], y: d.text_at[1], angle: -d.computed_text_angle, layer: d.layer, size: d.text_size_um, stroke_width: d.stroke_width, justify: "center", mirror: false }) : null;
      const lines = pts.length > 0 ? padded(boundsOf(pts), d.stroke_width / 2) : null;
      return unionBounds([lines, text]);
    }
    // The union of its members' boxes (`PCB_GROUP::GetBoundingBox`).
    case "group": {
      const g = board.drawings?.groups.find((q) => q.id === id);
      return g ? unionBounds(g.member_ids.map((m) => itemBounds(board, m))) : null;
    }
    case "pad": {
      const hit = padById(board, id);
      return hit ? [hit.pad.x - hit.pad.w / 2, hit.pad.y - hit.pad.h / 2, hit.pad.x + hit.pad.w / 2, hit.pad.y + hit.pad.h / 2] : null;
    }
    case "field": {
      const hit = fieldById(board, id);
      return hit && hit.field.text !== "" ? textBounds(fieldAsText(hit.field)) : hit ? [hit.field.x, hit.field.y, hit.field.x, hit.field.y] : null;
    }
    default:
      return null;
  }
}

/** A text's box with its angle applied (`EDA_TEXT::GetTextBox` turned by the text angle; the angle is KiCad's, counter-clockwise on the screen). */
function textBounds(t: BoardText): [number, number, number, number] {
  const b = textBoundingBox(t);
  const rad = (t.angle / 1000) * (Math.PI / 180);
  if (rad === 0) return [b.x0, b.y0, b.x1, b.y1];
  const [cos, sin] = [Math.cos(rad), Math.sin(rad)];
  const corners: [number, number][] = [
    [b.x0, b.y0],
    [b.x1, b.y0],
    [b.x1, b.y1],
    [b.x0, b.y1],
  ].map(([x, y]) => {
    const dx = x! - t.x;
    const dy = y! - t.y;
    return [t.x + dx * cos + dy * sin, t.y - dx * sin + dy * cos];
  });
  return boundsOf(corners);
}

/** The union of several boxes. */
export function unionBounds(boxes: readonly ([number, number, number, number] | null)[]): [number, number, number, number] | null {
  let out: [number, number, number, number] | null = null;
  for (const b of boxes) {
    if (!b) continue;
    out = out ? [Math.min(out[0], b[0]), Math.min(out[1], b[1]), Math.max(out[2], b[2]), Math.max(out[3], b[3])] : [b[0], b[1], b[2], b[3]];
  }
  return out;
}

function boundsOf(pts: readonly (readonly [number, number])[]): [number, number, number, number] {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

function padded(b: [number, number, number, number], by: number): [number, number, number, number] {
  return [b[0] - by, b[1] - by, b[2] + by, b[3] + by];
}
