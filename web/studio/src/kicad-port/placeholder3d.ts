// "Show parts without 3D model" (KiCad 25ec2335's 3D viewer, `EDA_3D_ACTIONS::toggleShowMissingModels`, `render.show_missing_models`, off by default; UI-LEFT-3 X3-2):
// which parts the 3D view draws a placeholder for, and the placeholder itself, ported from 3d-viewer/3d_rendering/opengl/render_3d_opengl.cpp
// (`get3dModelsFromFootprint`, `renderPlaceholderForFootprint`, `load3dModels`' `m_placeholderModel`) and 3d_placeholder_utils.cpp (`CalcPlaceholderLocalBox`).
//
//   none      a footprint with no models is drawn only with the setting on (`if( hasModels || showMissing )`), as its placeholder
//   missing   a model the viewer has not got (`m_3dModelMap.find( filename ) == end()`: not found, not readable, or -- the studio loads them as they come -- not in
//             yet) is the footprint's placeholder, once for all of them ("several missing models share one"), with the setting on; a hidden model or one with no
//             file name is skipped, a placeholder or not
//   box       the pads' box in the footprint's frame (each pad's size, its own rotation not counted), else its fab, courtyard and silk graphics' (here: its body or
//             courtyard box); 90% of its width and depth, half the narrower of the two tall, sitting on the board surface (`offsetZ = scaleZ / 2`); orange (diffuse
//             1, 0.5, 0, ambient 0.2, 0.1, 0) and 40% transparent
//   extruded  "Suppress placeholder if a valid extruded body was built"
import type { Part } from "../api/types";

/** `m_placeholderModel`'s material: diffuse, ambient, specular, shininess and transparency. */
export const PLACEHOLDER_MATERIAL = { diffuse: [1.0, 0.5, 0.0], ambient: [0.2, 0.1, 0.0], specular: [0.1, 0.1, 0.1], shininess: 0.1, transparency: 0.4 } as const;

/** The state of a model the viewer asked for: in, still coming, or not to be had. */
export type ModelState = "ready" | "loading" | "missing" | "failed";

/**
 * `get3dModelsFromFootprint`'s choice for a part: whether it gets the placeholder. `models` are the footprint's models (`show`, a file name, the cache's state of it);
 * `extruded` whether it has an extruded body.
 */
export function wantsPlaceholder(models: ReadonlyArray<{ show: boolean; name: string; state: ModelState }>, showMissing: boolean, extruded: boolean): boolean {
  if (!showMissing || extruded) return false;
  if (models.length === 0) return true;
  return models.some((m) => m.show && m.name !== "" && m.state !== "ready");
}

/** The placeholder of a part: the centre of its box on the board (nm), its turn and side, and the box's size in millimetres (`scaleX`, `scaleY`, `scaleZ`). */
export interface Placeholder {
  centerNm: [number, number];
  rotDeg: number;
  bottom: boolean;
  sizeMm: [number, number, number];
}

/** A board point in the part's own frame (un-turned about its anchor: the inverse of `T( at ) * R( rot )`, y down). */
function local(at: readonly [number, number], rotDeg: number, p: readonly [number, number]): [number, number] {
  const t = (rotDeg * Math.PI) / 180;
  const [c, s] = [Math.cos(t), Math.sin(t)];
  const [rx, ry] = [p[0] - at[0], p[1] - at[1]];
  return [rx * c + ry * s, -rx * s + ry * c];
}

/** `CalcPlaceholderLocalBox` and the sizing of `renderPlaceholderForFootprint`; null for a part with nothing to size it from. */
export function placeholderOf(part: Pick<Part, "at" | "rot" | "side" | "pads" | "body" | "courtyard">): Placeholder | null {
  if (!part.at || !part.side) return null;
  const at: [number, number] = [part.at[0], part.at[1]];
  const rot = part.rot ?? 0;
  let box: [number, number, number, number] | null = null;
  const merge = (x0: number, y0: number, x1: number, y1: number) => {
    box = box ? [Math.min(box[0], x0), Math.min(box[1], y0), Math.max(box[2], x1), Math.max(box[3], y1)] : [x0, y0, x1, y1];
  };
  // each pad: `GetFPRelativePosition()` and `GetSize( layer )` (its own rotation not counted)
  for (const pad of part.pads ?? []) {
    const [lx, ly] = local(at, rot, [pad.px ?? pad.x, pad.py ?? pad.y]);
    const [w, h] = pad.size ?? [pad.w, pad.h];
    merge(lx - w / 2, ly - h / 2, lx + w / 2, ly + h / 2);
  }
  // no pads: the fab, courtyard and silk graphics' box (the studio's footprint body, else its courtyard), in the footprint's frame
  if (!box) {
    const b = part.body ?? part.courtyard;
    if (b) {
      const corners = [local(at, rot, [b[0], b[1]]), local(at, rot, [b[2], b[1]]), local(at, rot, [b[2], b[3]]), local(at, rot, [b[0], b[3]])];
      merge(Math.min(...corners.map((c) => c[0])), Math.min(...corners.map((c) => c[1])), Math.max(...corners.map((c) => c[0])), Math.max(...corners.map((c) => c[1])));
    }
  }
  if (!box) return null;
  const [x0, y0, x1, y1] = box as [number, number, number, number];
  const w = (Math.abs(x1 - x0) / 1e6) * 0.9;
  const h = (Math.abs(y1 - y0) / 1e6) * 0.9;
  // the box's centre back on the board (`T( at ) * R( rot )` of its local centre)
  const [cx, cy] = [(x0 + x1) / 2, (y0 + y1) / 2];
  const t = (rot * Math.PI) / 180;
  const centerNm: [number, number] = [at[0] + cx * Math.cos(t) - cy * Math.sin(t), at[1] + cx * Math.sin(t) + cy * Math.cos(t)];
  return { centerNm, rotDeg: rot, bottom: part.side === "bottom", sizeMm: [w, h, Math.min(w, h) * 0.5] };
}
