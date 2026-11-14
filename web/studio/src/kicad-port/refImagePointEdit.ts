// The point editor of a board's reference image: `REFERENCE_IMAGE_POINT_EDIT_BEHAVIOR` (pcbnew/tools/pcb_point_editor.cpp at KiCad 8303b2ad), with the scaling it ends in
// (`REFERENCE_IMAGE::SetImageScale` -> `scaleBy`, common/reference_image.cpp). Four corners scale the image about its transform origin; a fifth point, on the origin
// (the centre plus `GetTransformOriginOffset`), moves the origin (UI-LEFT-3 P3-10).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd } from "../api/types";
import type { BoardRefImage } from "../api/pcbItemTypes";
import { imageSizeNm } from "./refImage";

type P = [number, number];

export type ImagePointId = "tl" | "tr" | "br" | "bl" | "origin";

export interface ImageEditPoint {
  id: ImagePointId;
  pos: P;
}

/** `KiROUND`. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));
/** `VECTOR2I / 2`: C++ integer division, towards zero. */
const half = (v: number): number => Math.trunc(v / 2);
/** KiCad's `sign()`: -1, 0 or 1. */
const sign = (v: number): number => (v > 0 ? 1 : v < 0 ? -1 : 0);

/** 50 mils, the smallest side a corner drag scales an image to (`EDA_UNIT_UTILS::Mils2IU( pcbIUScale, 50 )`). */
export const MIN_DRAG_SIDE_NM = 1_270_000;

function offsetOf(i: BoardRefImage): P {
  return [i.offset?.x ?? 0, i.offset?.y ?? 0];
}

/** `MakePoints`: the corners (top left, top right, bottom right, bottom left: `RECT_TOP_LEFT` ..), then the transform origin (`REFIMG_ORIGIN`). */
export function imageEditPoints(i: BoardRefImage): ImageEditPoint[] {
  const [ox, oy] = offsetOf(i);
  const tl: P = [i.x - half(i.w), i.y - half(i.h)];
  const br: P = [i.x + half(i.w), i.y + half(i.h)];
  return [
    { id: "tl", pos: tl },
    { id: "tr", pos: [br[0], tl[1]] },
    { id: "br", pos: br },
    { id: "bl", pos: [tl[0], br[1]] },
    { id: "origin", pos: [i.x + ox, i.y + oy] },
  ];
}

/** The edit point of `i` under `at` (the nearest within `tolNm`), else null. */
export function imagePointAt(i: BoardRefImage, at: readonly [number, number], tolNm: number): ImagePointId | null {
  let best: ImagePointId | null = null;
  let bestD = tolNm;
  for (const p of imageEditPoints(i)) {
    const d = Math.max(Math.abs(p.pos[0] - at[0]), Math.abs(p.pos[1] - at[1]));
    if (d <= bestD) {
      bestD = d;
      best = p.id;
    }
  }
  return best;
}

/**
 * `REFERENCE_IMAGE::scaleBy( aRatio )`: the image scaled about its transform origin -- the centre moves so that the origin stays, the offset scales with the image. A ratio that
 * is not positive changes nothing.
 */
export function scaleImageBy(i: BoardRefImage, ratio: number): BoardRefImage {
  if (!(ratio > 0)) return i;
  const [ox, oy] = offsetOf(i);
  const origin: P = [i.x + ox, i.y + oy];
  const newOffset: P = [ox * ratio, oy * ratio];
  const scale_ppm = Math.round(i.scale_ppm * ratio);
  const [w, h] = imageSizeNm(i.px, i.ppi, scale_ppm);
  const offset = { x: kiRound(newOffset[0]), y: kiRound(newOffset[1]) };
  return { ...i, x: kiRound(origin[0] - newOffset[0]), y: kiRound(origin[1] - newOffset[1]), scale_ppm, w, h, offset };
}

/**
 * `UpdateItem` with the point `id` dropped at `to`, the others where `MakePoints` put them: the origin point gives the new offset (from the middle of the corners); a corner
 * gives the ratio of its distance from the origin to the old corner's (zero when it crossed the origin on either axis), the sides kept at least 50 mils, and the image is
 * scaled by it about the origin (`SetImageScale( GetImageScale() * ratio )`). The image as it would be.
 */
export function moveImagePoint(i: BoardRefImage, id: ImagePointId, to: readonly [number, number]): BoardRefImage {
  const pts = imageEditPoints(i);
  const at = (k: ImagePointId): P => (k === id ? [to[0], to[1]] : pts.find((p) => p.id === k)!.pos);
  const topLeft = at("tl");
  const botRight = at("br");
  const xfrmOrigin = at("origin");
  if (id === "origin") {
    // "Moving the transform origin. As the other points didn't move, we can get the image extent from them."
    const offset = { x: xfrmOrigin[0] - half(topLeft[0] + botRight[0]), y: xfrmOrigin[1] - half(topLeft[1] + botRight[1]) };
    return { ...i, offset };
  }
  const [ox, oy] = offsetOf(i);
  const oldOrigin: P = [i.x + ox, i.y + oy];
  const oldSize: P = [i.w, i.h];
  let oldCorner: P = [i.x, i.y];
  if (id === "tl") oldCorner = [oldCorner[0] - half(oldSize[0]), oldCorner[1] - half(oldSize[1])];
  else if (id === "tr") oldCorner = [oldCorner[0] - half(-oldSize[0]), oldCorner[1] - half(oldSize[1])];
  else if (id === "bl") oldCorner = [oldCorner[0] - half(oldSize[0]), oldCorner[1] - half(-oldSize[1])];
  else oldCorner = [oldCorner[0] + half(oldSize[0]), oldCorner[1] + half(oldSize[1])];
  // "Turn in the respective vectors from the origin"
  let newCorner: P = [to[0] - xfrmOrigin[0], to[1] - xfrmOrigin[1]];
  oldCorner = [oldCorner[0] - oldOrigin[0], oldCorner[1] - oldOrigin[1]];
  // "If we tried to cross the origin, clamp it to stop it"
  if (sign(newCorner[0]) !== sign(oldCorner[0]) || sign(newCorner[1]) !== sign(oldCorner[1])) newCorner = [0, 0];
  const newLength = Math.hypot(newCorner[0], newCorner[1]);
  const oldLength = Math.hypot(oldCorner[0], oldCorner[1]);
  let ratio = oldLength > 0 ? newLength / oldLength : 1.0;
  // "Clamp the scaling to a minimum of 50 mils"
  const newWidth = Math.max(Math.trunc(oldSize[0] * ratio), MIN_DRAG_SIDE_NM);
  const newHeight = Math.max(Math.trunc(oldSize[1] * ratio), MIN_DRAG_SIDE_NM);
  ratio = Math.min(newWidth / oldSize[0], newHeight / oldSize[1]);
  return scaleImageBy(i, ratio);
}

/** The commands that make `base` what the drag left (`next`): `edit_ref_image` for its centre and scale, `set_ref_image_offset` for its origin -- one undo step. */
export function imagePointCmds(base: BoardRefImage, next: BoardRefImage): Cmd[] {
  const cmds: Cmd[] = [];
  if (next.x !== base.x || next.y !== base.y || next.scale_ppm !== base.scale_ppm) {
    cmds.push({ op: "pcb_item", verb: "edit_ref_image", id: base.id, layer: base.layer, at: { x: next.x, y: next.y }, scale_ppm: next.scale_ppm });
  }
  const [bx, by] = offsetOf(base);
  const [nx, ny] = offsetOf(next);
  if (nx !== bx || ny !== by) cmds.push({ op: "pcb_item", verb: "set_ref_image_offset", id: base.id, offset: { x: nx, y: ny } });
  return cmds;
}
