// A sheet's tables on the canvas: `SCH_PAINTER::draw( const SCH_TABLE* )` and `draw( const SCH_TEXTBOX* )` for a table cell. Each cell's text is drawn in the stroke font a line at a
// time, at the points the server placed the lines at (`eda_model::table_text::sch_cell_draw`: `SCH_TEXTBOX::GetDrawPos`, `EDA_TEXT::GetLinePositions`), turned by the table's angle,
// with the sheet's pen; a cell with a fill is filled first; the border lines are `SCH_TABLE::DrawBorders`', each in its stroke's colour (the notes layer's when it has none), width (the
// default one when it is 0) and dashes. A selected cell, or any cell of a selected table, is drawn in the selection colour over a wash of it ("selection shadows").
//
// A cell another one covers draws nothing (`if( cell->GetColSpan() == 0 || cell->GetRowSpan() == 0 ) return`).
import type { BoardTable, BoardTableCell } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layerColor } from "../canvas/layers";
import { drawStrokeText } from "../text/strokeFont";
import { dashPattern } from "./schGraphicsPainter";

const rgbaOf = (c: readonly [number, number, number, number]): string => `rgba(${c[0]},${c[1]},${c[2]},${(c[3] / 255).toFixed(3)})`;

/** A cell's background (`IsSolidFill`): the colour it names, the notes background, or the outline's colour; none when it has no fill. */
function fillStyle(c: BoardTableCell, base: string): string | null {
  switch (c.fill) {
    case "color":
      return c.fill_color ? rgbaOf(c.fill_color) : null;
    case "background":
      return layerColor("LAYER_NOTES_BACKGROUND");
    case "outline":
      return base;
    default:
      return null;
  }
}

/** Paint the tables of a sheet. `selection` holds the ids drawn selected: a table's, or one of its cells'. */
export function paintSchTables(ctx: CanvasRenderingContext2D, view: ViewTransform, tables: readonly BoardTable[], selection: ReadonlySet<string>): void {
  const hair = 1 / view.scale;
  for (const t of tables) {
    const tableSelected = selection.has(t.id);
    const base = layerColor("LAYER_NOTES");
    const selectedColor = layerColor("LAYER_SELECTION_SHADOWS");
    const angleRad = (-t.angle / 1000) * (Math.PI / 180);

    // The cells: the background, then the text (`SCH_TABLE::Plot` plots the cells first).
    for (const c of t.cells) {
      if (c.col_span === 0 || c.row_span === 0) continue;
      const selected = tableSelected || selection.has(c.id);
      const fill = fillStyle(c, base);
      if (fill) {
        ctx.save();
        ctx.fillStyle = fill;
        ctx.beginPath();
        c.corners.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        ctx.closePath();
        ctx.fill();
        ctx.restore();
      }
      const color = selected ? selectedColor : c.color ? rgbaOf(c.color) : base;
      for (const line of c.lines) {
        if (!line.text) continue;
        const sizeUm = Math.max(c.size_h, hair * 8);
        const thicknessUm = Math.max(c.pen, hair);
        if (c.size_w !== c.size_h && c.size_h > 0) {
          ctx.save();
          ctx.translate(line.x, line.y);
          ctx.rotate(angleRad);
          ctx.scale(c.size_w / c.size_h, 1);
          drawStrokeText(ctx, line.text, 0, 0, { sizeUm, thicknessUm, justify: "left", italic: c.italic, color });
          ctx.restore();
        } else {
          drawStrokeText(ctx, line.text, line.x, line.y, { sizeUm, thicknessUm, justify: "left", angleRad, italic: c.italic, color });
        }
      }
    }

    // The lines of the border and the separators.
    ctx.save();
    ctx.lineCap = "butt";
    ctx.lineJoin = "round";
    for (const b of t.borders) {
      if (b.width < 0) continue;
      ctx.strokeStyle = tableSelected ? selectedColor : b.color ? rgbaOf(b.color) : base;
      ctx.lineWidth = Math.max(b.width, hair * (tableSelected ? 2 : 1));
      const dash = b.style === "solid" || b.style === "default" ? undefined : b.style;
      ctx.setLineDash(dash ? dashPattern(dash, b.width) : []);
      ctx.beginPath();
      ctx.moveTo(b.a[0], b.a[1]);
      ctx.lineTo(b.b[0], b.b[1]);
      ctx.stroke();
    }
    ctx.restore();

    // The selection shadow over the selected cells.
    for (const c of t.cells) {
      if (!tableSelected && !selection.has(c.id)) continue;
      if (c.col_span === 0 || c.row_span === 0) continue;
      ctx.save();
      ctx.globalAlpha *= 0.4;
      ctx.fillStyle = selectedColor;
      ctx.beginPath();
      c.corners.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    }
  }
}

/**
 * `SCH_DRAWING_TOOLS::DrawTable`'s preview: the grid the cursor sizes, from the first corner -- its outline in the notes colour and the lines between the cells, so it reads as a table that
 * is not there yet.
 */
export function drawSchTablePreview(ctx: CanvasRenderingContext2D, view: ViewTransform, origin: readonly [number, number], grid: { cols: number; rows: number; cellW: number; cellH: number }): void {
  const [x0, y0] = origin;
  ctx.save();
  ctx.strokeStyle = layerColor("LAYER_NOTES");
  ctx.lineWidth = Math.max(152.4, 1 / view.scale);
  ctx.lineCap = "butt";
  ctx.beginPath();
  ctx.rect(x0, y0, grid.cols * grid.cellW, grid.rows * grid.cellH);
  for (let c = 1; c < grid.cols; c++) {
    ctx.moveTo(x0 + c * grid.cellW, y0);
    ctx.lineTo(x0 + c * grid.cellW, y0 + grid.rows * grid.cellH);
  }
  for (let r = 1; r < grid.rows; r++) {
    ctx.moveTo(x0, y0 + r * grid.cellH);
    ctx.lineTo(x0 + grid.cols * grid.cellW, y0 + r * grid.cellH);
  }
  ctx.stroke();
  ctx.restore();
}
