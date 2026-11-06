// The editor grid of all four canvases (the board, the schematic, the Footprint Editor, the Symbol Editor): `OPENGL_GAL::DrawGrid`'s marks
// (kicad-port/grid.ts `gridMarks`) in the style, thickness and minimum spacing of the editor's Display Options (state/commonOptions.ts `WindowOptions`), drawn
// in world coordinates on a context the painter has already put in the view's transform. `skip` leaves out the marks under something that hides the grid (a
// board grid item's wash; a whole grid line is not cut).
import { gridMarks, type GridDrawSpec } from "../../kicad-port/grid";

export function paintEditorGrid(ctx: CanvasRenderingContext2D, view: { x: number; y: number; scale: number }, widthPx: number, heightPx: number, spec: GridDrawSpec, color: string, skip?: (x: number, y: number) => boolean): void {
  const marks = gridMarks(view, widthPx, heightPx, spec);
  if (marks.length === 0) return;
  ctx.save();
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineCap = "butt";
  let width = -1;
  for (const m of marks) {
    if (m.kind === "rect") {
      if (skip && skip(m.x + m.w / 2, m.y + m.h / 2)) continue;
      ctx.fillRect(m.x, m.y, m.w, m.h);
      continue;
    }
    if (m.kind === "cross" && skip && skip(m.x, m.y)) continue;
    if (m.width !== width) {
      width = m.width;
      ctx.lineWidth = width;
    }
    ctx.beginPath();
    if (m.kind === "cross") {
      ctx.moveTo(m.x - m.len, m.y);
      ctx.lineTo(m.x + m.len, m.y);
      ctx.moveTo(m.x, m.y - m.len);
      ctx.lineTo(m.x, m.y + m.len);
    } else {
      ctx.moveTo(m.x0, m.y0);
      ctx.lineTo(m.x1, m.y1);
    }
    ctx.stroke();
  }
  ctx.restore();
}
