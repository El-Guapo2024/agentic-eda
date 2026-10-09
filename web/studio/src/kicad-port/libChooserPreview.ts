// What the choosers' preview panes frame: the box around the symbol unit / body style or the footprint they draw (`SYMBOL_PREVIEW_WIDGET::DisplaySymbol`
// and `FOOTPRINT_PREVIEW_WIDGET::DisplayFootprint` zoom to the item's bounding box). Pure geometry on the editable types the server sends, in the
// painters' own space -- micrometres, Y down -- so the components only fit a view to it. (A library symbol is mm, Y up in the file; the painters flip it.)
import type { LibraryPad, LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import type { Bounds } from "./view";

type Pt = [number, number];

/** A shape item shown in a unit/body style: shared (0) or exactly that one (`visibleFor`). */
function shown(itemUnit: number, itemStyle: number, unit: number, style: number): boolean {
  return (itemUnit === 0 || itemUnit === unit) && (itemStyle === 0 || itemStyle === style);
}

const toUm = (x: number, y: number): Pt => [x * 1000, -y * 1000];

const DIRECTION: Record<number, Pt> = { 0: [1, 0], 90: [0, -1], 180: [-1, 0], 270: [0, 1] };

/** The box of what `unit` / `bodyStyle` of a library symbol draws: its graphics and its pins (tip to root), in the preview's space. Null when nothing is drawn. */
export function symbolPreviewBounds(symbol: { graphics: readonly LibrarySymbolGraphic[]; pins: readonly LibrarySymbolPin[] }, unit: number, bodyStyle: number): Bounds | null {
  const pts: Pt[] = [];
  for (const g of symbol.graphics) {
    if (!shown(g.unit, g.body_style, unit, bodyStyle)) continue;
    switch (g.kind) {
      case "rectangle":
        pts.push(toUm(g.start.x, g.start.y), toUm(g.end.x, g.end.y));
        break;
      case "polyline":
        for (const p of g.pts) pts.push(toUm(p.x, p.y));
        break;
      case "circle":
        pts.push(toUm(g.center.x - g.radius_mm, g.center.y - g.radius_mm), toUm(g.center.x + g.radius_mm, g.center.y + g.radius_mm));
        break;
      case "arc":
        pts.push(toUm(g.start.x, g.start.y), toUm(g.mid.x, g.mid.y), toUm(g.end.x, g.end.y));
        break;
      case "text":
        pts.push(toUm(g.at.x, g.at.y));
        break;
    }
  }
  for (const p of symbol.pins) {
    if (!shown(p.unit, p.body_style, unit, bodyStyle) || p.hidden) continue;
    const [dx, dy] = DIRECTION[((Math.round(p.angle_deg) % 360) + 360) % 360] ?? [1, 0];
    const tip = toUm(p.at.x, p.at.y);
    pts.push(tip, [tip[0] + dx * p.length_mm * 1000, tip[1] + dy * p.length_mm * 1000]);
  }
  return boundsOf(pts);
}

function boundsOf(pts: readonly Pt[]): Bounds | null {
  if (pts.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const [x, y] of pts) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { minX, minY, maxX, maxY };
}

/** The graphics' points of a footprint (board space, µm), a bezier by its control points. */
function shapeBoundsPoints(s: { kind: string; [k: string]: unknown }): Pt[] {
  const p = (v: unknown): Pt => {
    const o = v as { x: number; y: number };
    return [o.x, o.y];
  };
  switch (s.kind) {
    case "segment":
    case "rect":
      return [p(s.start), p(s.end)];
    case "circle": {
      const c = p(s.center);
      const e = p(s.end);
      const r = Math.hypot(e[0] - c[0], e[1] - c[1]);
      return [[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]];
    }
    case "arc":
      return [p(s.start), p(s.mid), p(s.end)];
    case "polygon":
      return (s.pts as unknown[]).map(p);
    case "bezier":
      return [p(s.start), p(s.c1), p(s.c2), p(s.end)];
    default:
      return [];
  }
}

/** The box of a footprint's pads (their full size, so a turned pad is inside), its drawn graphics and its courtyard. Null when it has none of them. */
export function footprintPreviewBounds(fp: { pads: readonly LibraryPad[]; graphics: readonly { kind: string }[]; courtyard?: readonly [number, number] | null }): Bounds | null {
  const pts: Pt[] = [];
  for (const pad of fp.pads) {
    const r = Math.max(pad.size[0], pad.size[1]) / 2;
    pts.push([pad.at.x - r, pad.at.y - r], [pad.at.x + r, pad.at.y + r]);
  }
  for (const g of fp.graphics) pts.push(...shapeBoundsPoints(g as { kind: string; [k: string]: unknown }));
  if (fp.courtyard) pts.push([-fp.courtyard[0], -fp.courtyard[1]], [fp.courtyard[0], fp.courtyard[1]]);
  return boundsOf(pts);
}
