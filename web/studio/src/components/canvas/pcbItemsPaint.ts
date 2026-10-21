// The board items GAPS.md item 24 (package P16) added, as the canvas draws them: the text box today; reference images, points and hatch fills as their steps land. Called from
// `paintBoard` (painter.ts) once for the board and once for the items in the hand, after the dimensions. Ports of `PCB_PAINTER::draw( const PCB_TEXTBOX* )` (pcbnew/pcb_painter.cpp at
// KiCad 8303b2ad): the border in the layer's colour with its line style, the text in the stroke font wrapped to the box, and, for a knockout box, the box filled with the text cut out.
import type { BoardState } from "../../api/types";
import type { BoardTextBox } from "../../api/pcbItemTypes";
import { layerIsVisible } from "../../kicad-port/layerPresets";
import { layoutBoxText, outlineOf, type BoxTextLayout } from "../../kicad-port/pcbTextBox";
import { dashPattern } from "../schematic/schGraphicsPainter";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { layerColor } from "./layers";
import type { PaintOptions } from "./painter";
import { hairlineUm } from "./view";
import type { ViewTransform } from "../../state/store";

const layerKeyOf = (layer: string): string => layer.replace(/\./g, "_");

// The layout of a box is the same until the board's state is refreshed (which makes new box objects): laying out every repaint would measure every glyph of every box per mouse move.
const layouts = new WeakMap<BoardTextBox, BoxTextLayout>();
function layoutOf(b: BoardTextBox): BoxTextLayout {
  let l = layouts.get(b);
  if (!l) {
    l = layoutBoxText(b, measureStrokeText);
    layouts.set(b, l);
  }
  return l;
}

/** The tool in progress that draws a rubber band of its own (the Draw Text Box tool's rectangle, set by actions/pcbItemActions.ts). */
export type ToolPreview = { kind: "rect"; from: [number, number]; layer: string };
let toolPreview: ToolPreview | null = null;
export function setToolPreview(p: ToolPreview | null): void {
  toolPreview = p;
}

/** Every item of the kinds this module draws that the board has, and the rubber band of the tool in progress. */
export function drawPcbItems(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  drawTextBoxes(ctx, view, board, opts);
  const cursor = opts.snappedCursor ?? (opts.cursorUm ? ([opts.cursorUm.x, opts.cursorUm.y] as const) : null);
  if (toolPreview?.kind === "rect" && cursor && !opts.movePreview) {
    ctx.save();
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineUm(view, 1.5);
    ctx.setLineDash([hairlineUm(view, 5), hairlineUm(view, 3)]);
    ctx.strokeRect(toolPreview.from[0], toolPreview.from[1], cursor[0] - toolPreview.from[0], cursor[1] - toolPreview.from[1]);
    ctx.restore();
  }
}

function drawTextBoxes(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  for (const b of board.drawings?.text_boxes ?? []) {
    if (!layerIsVisible(opts.layerVisible, b.layer)) continue;
    drawTextBox(ctx, view, b, opts.selection.has(b.id), !!opts.sketchText);
  }
}

/** How a text box is drawn when it is not the canvas's: the 3D viewer paints it into a texture in one colour, the knockout's text cut out of the fill. */
export interface TextBoxLook {
  color?: string;
  /** Erase the text from what is drawn (a knockout box) instead of painting it in the background's colour. */
  cutOutText?: boolean;
}

/** One text box. `selected`: drawn in the selection colour; `lineMode`: the text in thin lines (`pcbnew.Control.textOutlines`). */
export function drawTextBox(ctx: CanvasRenderingContext2D, view: ViewTransform, b: BoardTextBox, selected: boolean, lineMode: boolean, look: TextBoxLook = {}): void {
  const color = look.color ?? (selected ? layerColor("selection") : layerColor(layerKeyOf(b.layer)));
  const outline = outlineOf(b);
  const trace = () => {
    ctx.beginPath();
    outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
  };
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  // `IsKnockout()`: the box is a solid area and the text is cut out of it -- drawn as the background's colour over the fill.
  if (b.knockout) {
    trace();
    ctx.fillStyle = color;
    ctx.fill();
  }
  if (b.border && b.stroke_width > 0) {
    ctx.strokeStyle = color;
    const w = Math.max(b.stroke_width, hairlineUm(view, selected ? 2 : 1));
    ctx.lineWidth = w;
    ctx.setLineDash(dashPattern(b.line_style === "default" ? "solid" : b.line_style, w));
    trace();
    ctx.stroke();
    ctx.setLineDash([]);
  } else if (selected) {
    // A box with no border still shows where it is when it is picked.
    ctx.strokeStyle = color;
    ctx.lineWidth = hairlineUm(view, 1);
    ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
    trace();
    ctx.stroke();
    ctx.setLineDash([]);
  }
  if ((b.shown ?? b.text) !== "") {
    const layout = layoutOf(b);
    ctx.translate(layout.anchor[0], layout.anchor[1]);
    // The text's angle is counter-clockwise on the screen, canvas y points down.
    ctx.rotate(-layout.angleRad);
    if (b.mirror) ctx.scale(-1, 1);
    const cut = !!look.cutOutText && b.knockout;
    if (cut) ctx.globalCompositeOperation = "destination-out";
    const textColor = cut ? "#000000" : b.knockout ? layerColor("background") : color;
    for (const l of layout.lines) {
      drawStrokeText(ctx, l.text, l.x, l.y, { sizeUm: b.size, thicknessUm: lineMode ? hairlineUm(view, 1) : b.thickness, justify: "left", italic: b.italic, color: textColor });
    }
  }
  ctx.restore();
}
