// A board's tables on the canvas: `PCB_PAINTER::draw( const PCB_TABLE* )` and `draw( const PCB_TEXTBOX* )` for a table cell. Each cell's text in the stroke font is drawn a
// line at a time, at the points the server placed the lines at (`eda_model::table_text`: `GetDrawPos`, `EDA_TEXT::GetLinePositions`), turned by the table's angle and mirrored
// as the cell says, its glyphs as `STROKE_FONT::GetTextAsGlyphs` lays them out (kicad-port/kicadText.ts `layoutLineAt`, the width apart from the height); a cell in an outline
// face is laid out here from its `GetDrawPos` (`FONT::Draw`, UI-LEFT-2 CC1: the server measures in the stroke font, so its line breaks stay the stroke font's); the border lines are `DrawBorders`' (`eda_model::table::Table::borders`), in the layer's colour with the stroke's width and dashes. A selected cell, or any cell
// of a selected table, is washed with the layer's colour at half alpha ("Highlight selected tablecells with a background wash").
//
// A cell another one covers draws nothing (`if( cell->GetColSpan() == 0 || cell->GetRowSpan() == 0 ) return`). The border and separator colours of the stroke are not used: the
// painter paints every line of the table in the layer's colour.

import type { BoardState, BoardTable } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layoutKicadText, layoutLineAt, resolveFont } from "../../kicad-port/kicadText";
import { paintLayout } from "../text/kicadTextPaint";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { dashPattern } from "../../kicad-port/fpArt";
import { layerColor } from "./layers";
import { realLayerKey, withLayer, type PaintOptions } from "./painter";
import { drawTableSession } from "../../kicad-port/tables";
import { hairlineNm } from "./view";

/** The tables of `board` that are drawn: those whose layer is on. */
export function drawTables(ctx: CanvasRenderingContext2D, view: ViewTransform, board: Pick<BoardState, "drawings">, opts: PaintOptions): void {
  for (const t of board.drawings?.tables ?? []) {
    if (!layerIsVisible(opts.layerVisible, t.layer)) continue;
    withLayer(ctx, opts, layerStateKey(t.layer), 1, () => drawTable(ctx, view, t, opts));
  }
}

function drawTable(ctx: CanvasRenderingContext2D, view: ViewTransform, t: BoardTable, opts: PaintOptions): void {
  const tableSelected = opts.selection.has(t.id);
  const base = layerColor(realLayerKey(t.layer));
  const selectedColor = layerColor("selection");

  for (const c of t.cells) {
    if (c.col_span === 0 || c.row_span === 0) continue;
    const selected = tableSelected || opts.selection.has(c.id);
    const color = selected ? selectedColor : base;
    const size: [number, number] = [c.size_w || c.size_h, c.size_h];
    const pen = opts.sketchText ? hairlineNm(view, 1) : c.pen;
    const font = resolveFont(c.face, c.bold, c.italic);
    const look = { size, pen: c.pen, angleDeg: t.angle / 1000, mirror: c.mirror, italic: c.italic, bold: c.bold };
    if (font.kind === "outline" && c.draw_pos) {
      paintLayout(ctx, layoutKicadText({ ...look, text: c.shown ?? c.text, pos: c.draw_pos, halign: c.h_align, valign: c.v_align, lineSpacing: (c.line_spacing || 1000) / 1000 }, font), { color, pen });
      continue;
    }
    for (const line of c.lines) {
      if (!line.text) continue;
      paintLayout(ctx, layoutLineAt(line.text, [line.x, line.y], look), { color, pen });
    }
  }

  ctx.save();
  ctx.lineCap = "butt";
  ctx.lineJoin = "round";
  ctx.strokeStyle = tableSelected ? selectedColor : base;
  for (const b of t.borders) {
    if (b.width < 0) continue;
    const w = Math.max(b.width, hairlineNm(view, tableSelected ? 2 : 1));
    ctx.lineWidth = w;
    const dash = b.style === "solid" || b.style === "default" ? undefined : b.style;
    ctx.setLineDash(dash ? dashPattern(dash, b.width) : []);
    ctx.beginPath();
    ctx.moveTo(b.a[0], b.a[1]);
    ctx.lineTo(b.b[0], b.b[1]);
    ctx.stroke();
  }
  ctx.restore();

  // The wash over the selected cells.
  for (const c of t.cells) {
    if (!tableSelected && !opts.selection.has(c.id)) continue;
    if (c.col_span === 0 || c.row_span === 0) continue;
    ctx.save();
    ctx.globalAlpha *= 0.5;
    ctx.fillStyle = base;
    ctx.beginPath();
    c.corners.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }
}

/**
 * `DRAWING_TOOL::DrawTable`'s preview: the grid the cursor sizes, from the first corner -- its border in the layer's colour and the cell lines dashed, so it reads as a table that is not
 * there yet.
 */
export function drawTablePreview(ctx: CanvasRenderingContext2D, view: ViewTransform, board: Pick<BoardState, "board_rules">, origin: readonly [number, number], opts: PaintOptions): void {
  const cursor = opts.cursorNm;
  if (!cursor) return;
  const at: [number, number] = opts.snappedCursor ? [opts.snappedCursor[0], opts.snappedCursor[1]] : [cursor.x, cursor.y];
  const { layer, grid, lineWidth } = drawTableSession(board.board_rules?.text_graphics, opts.activeLayer, origin, at, opts.gridNm);
  const color = layerColor(realLayerKey(layer));
  const [x0, y0] = origin;
  const w = Math.max(lineWidth, hairlineNm(view, 1));
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = w;
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
