// The geometry of the Zone Manager's preview (`ZONE_PREVIEW_CANVAS`, components/ZonePreview.tsx): the view that fits the board in the canvas, the zoom about the cursor, the path
// of a set of rings. Pure functions on numbers; no React, no DOM.
//
// Ported from:
//   zone_preview_canvas.cpp   ZoomFitScreen (the scale that fits the board's edges box in the canvas), LockZoom (the zoom a person chose is kept)

/** The canvas of the preview, in pixels. */
export const PREVIEW_SIZE = { w: 300, h: 260 };

type Pt = readonly [number, number];

/** World µm at the centre of the canvas, and µm per pixel. */
export interface PreviewView {
  cx: number;
  cy: number;
  scale: number;
}

/** The box `[x0, y0, x1, y1]` of some rings, or `null` when they have no point. */
export function ringsBounds(rings: readonly (readonly Pt[])[]): [number, number, number, number] | null {
  let box: [number, number, number, number] | null = null;
  for (const ring of rings) {
    for (const [x, y] of ring) {
      box = box ? [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)] : [x, y, x, y];
    }
  }
  return box;
}

/** An SVG path of closed rings (a ring of fewer than three points is left out). */
export function ringsPath(rings: readonly (readonly Pt[])[]): string {
  return rings.map((ring) => (ring.length < 3 ? "" : `M${ring.map(([x, y]) => `${x} ${y}`).join("L")}Z`)).join("");
}

/** `ZoomFitScreen`: the view that fits `box` (the board's edges) in the canvas with a margin; the origin for no box. */
export function fitView(box: readonly [number, number, number, number] | null): PreviewView {
  if (!box) return { cx: 0, cy: 0, scale: 100_000 };
  const [x0, y0, x1, y1] = box;
  const scale = Math.max((x1 - x0) / (PREVIEW_SIZE.w * 0.92), (y1 - y0) / (PREVIEW_SIZE.h * 0.92), 1_000);
  return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, scale };
}

/** The world point under pixel `(px, py)` of the canvas (measured from its top-left corner). */
export function worldAt(view: PreviewView, px: number, py: number): [number, number] {
  return [view.cx + (px - PREVIEW_SIZE.w / 2) * view.scale, view.cy + (py - PREVIEW_SIZE.h / 2) * view.scale];
}

/** The wheel zooms about the cursor: `factor` below 1 zooms in, and the world point under the cursor stays where it is. The scale is held between 1 µm and 1 m per pixel. */
export function zoomAbout(view: PreviewView, wx: number, wy: number, factor: number): PreviewView {
  const scale = Math.min(Math.max(view.scale * factor, 1_000), 1_000_000_000);
  const r = scale / view.scale;
  return { cx: wx - (wx - view.cx) * r, cy: wy - (wy - view.cy) * r, scale };
}
