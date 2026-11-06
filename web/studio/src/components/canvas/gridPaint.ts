// The editor grid of all four canvases (the board, the schematic, the Footprint Editor, the Symbol Editor): `OPENGL_GAL::DrawGrid`'s marks
// (kicad-port/grid.ts `gridMarks`) in the style, thickness and minimum spacing of the editor's Display Options (state/commonOptions.ts `WindowOptions`), drawn
// in world coordinates on a context the painter has already put in the view's transform. `skip` leaves out the marks under something that hides the grid (a
// board grid item's wash; a whole grid line is not cut). The browser checks read what was last drawn on each canvas through `window.__edaGrid[tag]`
// ({ style, lineWidthPx, minSpacingPx, size, marks: the count of each kind }).
import { gridMarks, type GridDrawSpec } from "../../kicad-port/grid";

export function paintEditorGrid(ctx: CanvasRenderingContext2D, view: { x: number; y: number; scale: number }, widthPx: number, heightPx: number, spec: GridDrawSpec, color: string, skip?: (x: number, y: number) => boolean, tag?: string): void {
  const marks = gridMarks(view, widthPx, heightPx, spec);
  if (tag && typeof window !== "undefined") {
    const counts: Record<string, number> = {};
    for (const m of marks) counts[m.kind] = (counts[m.kind] ?? 0) + 1;
    const w = window as unknown as { __edaGrid?: Record<string, unknown> };
    w.__edaGrid = { ...(w.__edaGrid ?? {}), [tag]: { style: spec.style, lineWidthPx: spec.lineWidthPx, minSpacingPx: spec.minSpacingPx, size: spec.size, marks: counts } };
  }
  if (marks.length === 0) return;
  // Each mark is put on whole screen pixels, as the GAL's own grid is (it is drawn without antialiasing): a 1 px dot or line at a fractional position would be smeared
  // over its neighbours at a fraction of the colour. A dot's corner goes to a pixel corner, a line's middle to a pixel's middle when it is an odd number of pixels wide.
  const k = view.scale;
  const px = (w: number) => Math.max(1, Math.round(w * k));
  const corner = (v: number, offset: number) => (Math.round(offset + v * k) - offset) / k;
  const middle = (v: number, offset: number, wPx: number) => ((wPx % 2 === 1 ? Math.floor(offset + v * k) + 0.5 : Math.round(offset + v * k)) - offset) / k;
  ctx.save();
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineCap = "butt";
  let width = -1;
  for (const m of marks) {
    if (m.kind === "rect") {
      if (skip && skip(m.x + m.w / 2, m.y + m.h / 2)) continue;
      ctx.fillRect(corner(m.x, view.x), corner(m.y, view.y), px(m.w) / k, px(m.h) / k);
      continue;
    }
    if (m.kind === "cross" && skip && skip(m.x, m.y)) continue;
    const wPx = px(m.width);
    if (wPx !== width) {
      width = wPx;
      ctx.lineWidth = wPx / k;
    }
    ctx.beginPath();
    if (m.kind === "cross") {
      const x = middle(m.x, view.x, wPx);
      const y = middle(m.y, view.y, wPx);
      ctx.moveTo(x - m.len, y);
      ctx.lineTo(x + m.len, y);
      ctx.moveTo(x, y - m.len);
      ctx.lineTo(x, y + m.len);
    } else if (m.y0 === m.y1) {
      const y = middle(m.y0, view.y, wPx);
      ctx.moveTo(m.x0, y);
      ctx.lineTo(m.x1, y);
    } else {
      const x = middle(m.x0, view.x, wPx);
      ctx.moveTo(x, m.y0);
      ctx.lineTo(x, m.y1);
    }
    ctx.stroke();
  }
  ctx.restore();
}
