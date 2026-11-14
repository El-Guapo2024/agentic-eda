// The footprint a click in the 3D view lands on through its pads (UI-LEFT-3 X3-2 follow-up, gate v19).
//
// KiCad's 3D click (`EDA_3D_CANVAS::OnLeftUp`, 3d-viewer/3d_canvas/eda_3d_canvas.cpp at 8303b2ad) asks the ray tracer for the board item the ray hits first
// (`RENDER_3D_RAYTRACE_BASE::IntersectBoardItem`). With the OpenGL renderer, the ray tracer's scene is loaded with the copper and the shapes only
// (`m_3d_render_raytracing->Reload( nullptr, nullptr, true )`, `aOnlyLoadCopperAndShapes`), and every copper object keeps the item it was made from
// (create_scene.cpp: `createObject( .., object2d_A->GetBoardItem() )`), so a ray on a pad's copper hits the pad, and `case PCB_PAD_T` selects
// `GetParentFootprint()`. A footprint is picked by its pads whether or not it has a 3D model drawn -- since "Show parts without 3D model" (off by default)
// draws nothing for a footprint without its models, its pads are what the click finds.
//
// The copper here is the pads' outer faces: the front copper's top and the back copper's bottom (`m_layerZcoordTop[F_Cu]`, `m_layerZcoordBottom[B_Cu]`),
// each pad on the sides it has copper on, measured by `PAD::HitTest` (padHit.ts). Pure: no three.js, no store.
import type { Pad, Part } from "../api/types";
import { padHitDistance } from "./padHit";
import type { Stack } from "./model3d";

/** A ray in the scene's frame (x the board's x, y up, z the board's y; millimetres), its direction of length 1. */
export interface Ray3 {
  origin: readonly [number, number, number];
  dir: readonly [number, number, number];
}

/** The outer copper sides a pad has copper on, on the board (a back footprint's `F.Cu` is the board's `B.Cu`); none for a non-plated hole. */
export function padCopperSides(pad: Pick<Pad, "kind" | "layers" | "th">, bottomPart: boolean): { top: boolean; bottom: boolean } {
  if (pad.kind === "non_plated_hole") return { top: false, bottom: false };
  let front: boolean;
  let back: boolean;
  if (pad.layers?.length) {
    const all = pad.layers.includes("*.Cu");
    front = all || pad.layers.includes("F.Cu");
    back = all || pad.layers.includes("B.Cu");
  } else {
    front = true;
    back = !!pad.th;
  }
  return bottomPart ? { top: back, bottom: front } : { top: front, bottom: back };
}

/**
 * The placed footprint whose pad copper `ray` meets first (`IntersectBoardItem` -> `PCB_PAD_T` -> `GetParentFootprint()`), with the distance along the ray,
 * or null. The front copper's top is at `boardThicknessMm / 2 + copperMm`, the back copper's bottom at minus the half thickness and the back copper.
 */
export function pickFootprintByPads(parts: readonly Part[], ray: Ray3, stack: Stack): { ref: string; distance: number } | null {
  const half = stack.boardThicknessMm / 2;
  const faces: Array<{ y: number; top: boolean }> = [
    { y: half + stack.copperMm, top: true },
    { y: -(half + (stack.copperBackMm ?? stack.copperMm)), top: false },
  ];
  const hits: Array<{ t: number; top: boolean; x: number; z: number }> = [];
  const [ox, oy, oz] = ray.origin;
  const [dx, dy, dz] = ray.dir;
  if (Math.abs(dy) < 1e-12) return null;
  for (const f of faces) {
    const t = (f.y - oy) / dy;
    if (t > 0) hits.push({ t, top: f.top, x: ox + t * dx, z: oz + t * dz });
  }
  hits.sort((a, b) => a.t - b.t);
  for (const h of hits) {
    const xNm = h.x * 1e6;
    const yNm = h.z * 1e6;
    for (const part of parts) {
      if (!part.at) continue;
      const bottom = part.side === "bottom";
      for (const pad of part.pads ?? []) {
        const sides = padCopperSides(pad, bottom);
        if (!(h.top ? sides.top : sides.bottom)) continue;
        if (padHitDistance(pad, xNm, yNm) <= 0) return { ref: part.ref, distance: h.t };
      }
    }
  }
  return null;
}
