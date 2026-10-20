// What Create Array needs of the Footprint Editor's selection: which of the ids are items of the open footprint, where each one is (`BOARD_ITEM::GetPosition()`) and the
// box it fills, so the dialog can start the circle at `ARRAY_TOOL::CreateArray`'s `origin` (a lone item's position, else the centre of the selection).
//
// Ported from pcbnew/tools/array_tool.cpp and `EDA_SHAPE::getPosition` (common/eda_shape.cpp) at KiCad 8303b2ad; the command is crates/ops/src/fp_array.rs.
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { CmdShape, LibraryFootprint } from "../api/types";
import { footprintItemBoxes, type Box } from "./itemBoxes";

/** The centre of the circle through three points; null when they lie on a line. */
export function arcCentre(a: readonly [number, number], b: readonly [number, number], c: readonly [number, number]): [number, number] | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-9) return null;
  const a2 = a[0] * a[0] + a[1] * a[1];
  const b2 = b[0] * b[0] + b[1] * b[1];
  const c2 = c[0] * c[0] + c[1] * c[1];
  return [Math.round((a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d), Math.round((a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d)];
}

/** `EDA_SHAPE::getPosition`: the centre of an arc or a circle, the first vertex of a polygon, the start of everything else. */
export function shapePosition(s: CmdShape): [number, number] {
  switch (s.kind) {
    case "circle":
      return [s.center.x, s.center.y];
    case "arc":
      return arcCentre([s.start.x, s.start.y], [s.mid.x, s.mid.y], [s.end.x, s.end.y]) ?? [s.start.x, s.start.y];
    case "polygon":
      return s.pts[0] ? [s.pts[0].x, s.pts[0].y] : [0, 0];
    default:
      return [s.start.x, s.start.y];
  }
}

/** `BOARD_ITEM::GetPosition()` of a pad, graphic or text of the footprint; null for an id that names none of them. */
export function fpItemPosition(fp: LibraryFootprint, id: string): [number, number] | null {
  const pad = fp.pads.find((p) => p.id === id);
  if (pad) return [pad.at.x, pad.at.y];
  const graphic = fp.graphics.find((g) => g.id === id);
  if (graphic) return shapePosition(graphic);
  const text = fp.texts.find((t) => t.id === id);
  return text ? [text.at.x, text.at.y] : null;
}

/** The box of one item, for `PCB_SELECTION::GetCenter`. */
export function fpItemBounds(fp: LibraryFootprint, id: string): Box | null {
  return footprintItemBoxes(fp).get(id) ?? null;
}

/** The ids that are items of the footprint, in the order given and each once (the array works on exactly these). */
export function fpArrayIds(fp: LibraryFootprint, ids: Iterable<string>): string[] {
  const known = new Set([...fp.pads.map((p) => p.id), ...fp.graphics.map((g) => g.id), ...fp.texts.map((t) => t.id)]);
  const out: string[] = [];
  for (const id of ids) if (known.has(id) && !out.includes(id)) out.push(id);
  return out;
}
