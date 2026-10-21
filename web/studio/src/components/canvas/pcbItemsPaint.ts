// The board items GAPS.md item 24 (package P16) added, as the canvas draws them: the text box today; reference images, points and hatch fills as their steps land. Called from
// `paintBoard` (painter.ts) once for the board and once for the items in the hand, after the dimensions. Ports of `PCB_PAINTER::draw( const PCB_TEXTBOX* )` (pcbnew/pcb_painter.cpp at
// KiCad 8303b2ad): the border in the layer's colour with its line style, the text in the stroke font wrapped to the box, and, for a knockout box, the box filled with the text cut out.
import type { BoardState } from "../../api/types";
import type { BoardRefImage, BoardTextBox } from "../../api/pcbItemTypes";
import { parseCssColor } from "../../kicad-port/appearance";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { refImageBounds } from "../../kicad-port/pcbExtraItems";
import { layoutBoxText, outlineOf, type BoxTextLayout } from "../../kicad-port/pcbTextBox";
import { pcbImageUrl } from "../../kicad-port/refImage";
import { dashPattern } from "../schematic/schGraphicsPainter";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { on, opacityOf } from "./appearancePaint";
import { imageFor } from "./imageCache";
import { layerColor } from "./layers";
import { layerAlpha, withAlpha, type PaintOptions } from "./painter";
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

/**
 * The tool in progress that draws something of its own (set by actions/pcbItemActions.ts): the Draw Text Box tool's rubber band, or the image Place Reference Image holds by its
 * centre at the cursor (`m_view->AddToPreview( image )`), drawn as the image will be, `w` x `h` micrometres on `layer`.
 */
export type ToolPreview = { kind: "rect"; from: [number, number]; layer: string } | { kind: "image"; img: HTMLImageElement; w: number; h: number; layer: string };
let toolPreview: ToolPreview | null = null;
export function setToolPreview(p: ToolPreview | null): void {
  toolPreview = p;
}

// ------------------------------------------------------------------------------------------------------------------------------- reference images

/** The alpha of a layer's colour (`COLOR4D::a`): the part of `PCB_PAINTER::draw( PCB_REFERENCE_IMAGE )`'s `imgAlpha` the layer itself gives. */
function colourAlpha(layer: string): number {
  return parseCssColor(layerColor(layer.replace(/\./g, "_")))?.a ?? 1;
}

/**
 * `PCB_PAINTER::draw( const PCB_REFERENCE_IMAGE* )`: the bitmap at its place and size, at the alpha of its layer's colour times the Images opacity of the Appearance panel;
 * a selected image has a box drawn round it in the anchor colour and is shown opaque ("Keep reference images opaque when selected").
 */
function paintRefImage(ctx: CanvasRenderingContext2D, view: ViewTransform, i: BoardRefImage, alpha: number, selected: boolean): void {
  const [x0, y0] = refImageBounds(i);
  const el = imageFor(pcbImageUrl(i.id, i.hash));
  ctx.save();
  if (el) {
    ctx.globalAlpha *= selected ? 1 : alpha;
    ctx.drawImage(el, x0, y0, i.w, i.h);
  } else {
    // not here yet: the box it will fill, so it can be seen and picked
    ctx.globalAlpha *= alpha;
    ctx.strokeStyle = layerColor(i.layer.replace(/\./g, "_"));
    ctx.lineWidth = hairlineUm(view, 1);
    ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
    ctx.strokeRect(x0, y0, i.w, i.h);
  }
  ctx.restore();
  if (selected) {
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_ANCHOR");
    ctx.lineWidth = hairlineUm(view, 2);
    ctx.strokeRect(x0, y0, i.w, i.h);
    ctx.restore();
  }
}

/**
 * The board's reference images, as `LAYER_DRAW_BITMAPS` draws them: below every other item, on their layer when it is shown, and only while the Images object is on and its opacity
 * is not zero (`PCB_REFERENCE_IMAGE::ViewGetLOD`). In high contrast an image on a layer other than the active one fades with the layer (or is hidden).
 */
export function drawRefImages(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  const images = board.drawings?.images ?? [];
  if (images.length === 0 || !on(opts.layerVisible, "bitmaps")) return;
  const opacity = opacityOf(opts.appearance, "images");
  if (opacity <= 0) return;
  for (const i of images) {
    if (!layerIsVisible(opts.layerVisible, i.layer)) continue;
    const dim = layerAlpha(opts, layerStateKey(i.layer));
    withAlpha(ctx, dim, () => paintRefImage(ctx, view, i, colourAlpha(i.layer) * opacity, opts.selection.has(i.id)));
  }
}

/** Every item of the kinds this module draws that the board has, and the rubber band of the tool in progress. */
export function drawPcbItems(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  drawTextBoxes(ctx, view, board, opts);
  const cursor = opts.snappedCursor ?? (opts.cursorUm ? ([opts.cursorUm.x, opts.cursorUm.y] as const) : null);
  // The image in the hand follows the cursor, centred on it, at the alpha it will have.
  if (toolPreview?.kind === "image" && cursor && !opts.movePreview) {
    const { img, w, h, layer } = toolPreview;
    ctx.save();
    ctx.globalAlpha *= colourAlpha(layer) * opacityOf(opts.appearance, "images");
    ctx.drawImage(img, cursor[0] - Math.trunc(w / 2), cursor[1] - Math.trunc(h / 2), w, h);
    ctx.restore();
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_ANCHOR");
    ctx.lineWidth = hairlineUm(view, 2);
    ctx.strokeRect(cursor[0] - Math.trunc(w / 2), cursor[1] - Math.trunc(h / 2), w, h);
    ctx.restore();
  }
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
