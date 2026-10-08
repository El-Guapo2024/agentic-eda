// The marks KiCad draws on a symbol that carries an attribute (`SCH_PAINTER::draw( SCH_SYMBOL )`, eeschema/sch_painter.cpp at 8303b2ad):
//   * Do not Populate: a red cross over the symbol -- the body's box widened by 60 % of the margin the pins add (`LAYER_DNP_MARKER`, a pen of 3 x the default
//     6 mil line), the two diagonals corner to corner.
//   * Exclude from Simulation, with "Mark items excluded from simulation" on: a frame around the body (a 25 mil pen) and, past its bottom right corner, a small
//     circle with an "S" curve in it (`LAYER_EXCLUDED_FROM_SIM`).
// The geometry is pure (micrometres, +y down, like the rest of the schematic painter); `symbolMarkersDraw.ts` strokes it.
import type { ResolvedGraphic } from "./libSymbol";

export interface Box {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

/** `DEFAULT_LINE_WIDTH_MILS` (6 mil) in um. */
export const DEFAULT_LINE_UM = 152.4;
/** `m_ExcludeFromSimulationLineWidth` (25, mils) in um. */
const EXCLUDE_SIM_PEN_UM = 635;

/** The bounding box of a symbol's graphics alone (`GetBodyBoundingBox`: no pins, no fields), or null when it draws none. A text is a point here. */
export function bodyBoundsOf(graphics: readonly ResolvedGraphic[]): Box | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const grow = (x: number, y: number) => {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  };
  for (const g of graphics) {
    switch (g.kind) {
      case "rectangle":
        grow(g.start[0], g.start[1]);
        grow(g.end[0], g.end[1]);
        break;
      case "polyline":
        for (const p of g.pts) grow(p[0], p[1]);
        break;
      case "circle":
        grow(g.center[0] - g.radiusUm, g.center[1] - g.radiusUm);
        grow(g.center[0] + g.radiusUm, g.center[1] + g.radiusUm);
        break;
      case "arc":
        for (const p of [g.start, g.mid, g.end]) grow(p[0], p[1]);
        break;
      case "text":
        grow(g.at[0], g.at[1]);
        break;
    }
  }
  return Number.isFinite(minX) ? { minX, minY, maxX, maxY } : null;
}

/**
 * The two diagonals of the Do not Populate cross, `[from, to]` each. `body` is the symbol's body box, `all` the box that also holds its pins: the
 * margin is how far the pins stick out past the body, and the body's box is widened by 60 % of it (at least 30 % of the other axis's).
 */
export function dnpCross(body: Box, all: Box): Array<[[number, number], [number, number]]> {
  let mx = Math.max(body.minX - all.minX, all.maxX - body.maxX);
  let my = Math.max(body.minY - all.minY, all.maxY - body.maxY);
  mx = Math.max(mx * 0.6, my * 0.3);
  my = Math.max(my * 0.6, mx * 0.3);
  const x0 = body.minX - Math.round(mx);
  const y0 = body.minY - Math.round(my);
  const x1 = body.maxX + Math.round(mx);
  const y1 = body.maxY + Math.round(my);
  return [
    [[x0, y0], [x1, y1]],
    [[x1, y0], [x0, y1]],
  ];
}

/** The Exclude from Simulation mark: the frame (the body widened by half the pen), and the circle with its "S" curve. */
export function simExclusionMark(body: Box): { frame: Box; pen: number; center: [number, number]; radius: number; curve: [[number, number], [number, number], [number, number], [number, number]] } {
  const pen = EXCLUDE_SIM_PEN_UM;
  const half = Math.round(pen * 0.5);
  const frame: Box = { minX: body.minX - half, minY: body.minY - half, maxX: body.maxX + half, maxY: body.maxY + half };
  const offset = 2 * pen;
  const center: [number, number] = [frame.maxX + offset + pen, frame.maxY - offset];
  return {
    frame,
    pen,
    center,
    radius: offset,
    // DrawCurve( left, top, bottom, right ): a cubic Bezier from the left of the circle, pulled up then down, to the right
    curve: [[center[0] - offset, center[1]], [center[0], center[1] + offset], [center[0], center[1] - offset], [center[0] + offset, center[1]]],
  };
}
