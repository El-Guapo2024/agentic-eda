// The board items GAPS.md item 24 (package P16) added, as the canvas draws them: the text box today; reference images, points and hatch fills as their steps land. Called from
// `paintBoard` (painter.ts) once for the board and once for the items in the hand, after the dimensions. Ports of `PCB_PAINTER::draw( const PCB_TEXTBOX* )` (pcbnew/pcb_painter.cpp at
// KiCad 8303b2ad): the border in the layer's colour with its line style, the text wrapped to the box (`LinebreakText` measuring in the box's font) and drawn as `FONT::Draw` lays it
// out from `GetDrawPos` in the stroke font or its outline face (kicad-port/kicadText.ts, UI-LEFT-2 CC1), and, for a knockout box, the box filled with the text cut out.
import type { BoardGridItem } from "../../api/pcbItemTypes";
import { sizeGridToCursor } from "../../kicad-port/gridItem";
import { drawGridItemPreview } from "./gridItemPaint";
import type { BoardState } from "../../api/types";
import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox } from "../../api/pcbItemTypes";
import { shownFootprintImages } from "../../kicad-port/fpBoardItems";
import { barcodeShape, barcodeShapeKey, symbolCorners } from "../../kicad-port/barcodeShapes";
import { centrelineRect, inductorEnd } from "../../kicad-port/microwave";
import { requestBarcodeShape } from "./barcodeCache";
import { parseCssColor } from "../../kicad-port/appearance";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { keptReadableInFlippedView } from "../../kicad-port/flipView";
import { refImageBounds } from "../../kicad-port/pcbExtraItems";
import { columnWidthNm, drawPos, linebreakText, outlineOf, shownTextOf } from "../../kicad-port/pcbTextBox";
import { layoutKicadText, lineAdvance, resolveFont, type KicadFont, type KicadTextLayout } from "../../kicad-port/kicadText";
import { textBoxSpec } from "../../kicad-port/textSpecs";
import { paintLayout } from "../text/kicadTextPaint";
import { pcbImageUrl } from "../../kicad-port/refImage";
import { dashPattern } from "../schematic/schGraphicsPainter";
import { on, opacityOf } from "./appearancePaint";
import { imageFor } from "./imageCache";
import { layerColor } from "./layers";
import { refImageLayerAlpha, withAlpha, withBoardLayer, withLayer, type PaintOptions } from "./painter";
import { hairlineNm } from "./view";
import type { ViewTransform } from "../../state/store";

const layerKeyOf = (layer: string): string => layer.replace(/\./g, "_");

// The layout of a box is the same until the board's state is refreshed (which makes new box objects) or its face has loaded: laying out every repaint would measure every glyph
// of every box per mouse move.
const layouts = new WeakMap<BoardTextBox, { font: KicadFont; layout: KicadTextLayout }>();
const flippedLayouts = new WeakMap<BoardTextBox, { font: KicadFont; layout: KicadTextLayout }>();
/** `PCB_TEXTBOX::GetShownText`'s wrap (`LinebreakText( text, colWidth, GetTextSize(), GetTextThickness(), .. )`) in the box's font, then `FONT::Draw` from `GetDrawPos`. `flippedView`:
 * Flip Board View on a box that is not side specific -- `if( m_gal->IsFlippedX() && !aTextBox->IsSideSpecific() ) { attrs.m_Mirrored = !attrs.m_Mirrored;
 * strokeText( resolvedText, aTextBox->GetDrawPos( true ), attrs, metrics ); }` (`PCB_PAINTER::draw( const PCB_TEXTBOX* )`), so it reads as it did. */
function layoutOf(b: BoardTextBox, flippedView = false): KicadTextLayout {
  const font = resolveFont(b.face, b.bold, b.italic);
  const cache = flippedView ? flippedLayouts : layouts;
  const hit = cache.get(b);
  if (hit && hit.font.kind === font.kind) return hit.layout;
  const size: [number, number] = [b.width || b.size, b.size];
  const lines = linebreakText(shownTextOf(b), columnWidthNm(b), b.size, b.thickness_auto ? 0 : b.thickness, (text) => lineAdvance(text, font, size, b.italic));
  const spec = textBoxSpec(b, lines.join("\n"), drawPos(b, flippedView)).spec;
  const layout = layoutKicadText(flippedView ? { ...spec, mirror: !spec.mirror } : spec, font);
  cache.set(b, { font, layout });
  return layout;
}

/**
 * The tool in progress that draws something of its own (set by actions/pcbItemActions.ts): the Draw Text Box tool's rubber band, or the image Place Reference Image holds by its
 * centre at the cursor (`m_view->AddToPreview( image )`), drawn as the image will be, `w` x `h` micrometres on `layer`.
 */
export type ToolPreview =
  | { kind: "rect"; from: [number, number]; layer: string }
  | { kind: "image"; img: HTMLImageElement; w: number; h: number; layer: string }
  /** The Draw Microwave Lines tool between its two clicks (`CENTRELINE_RECT_ITEM`): the line from `from` to the cursor, held by the angle snap, and the rectangle along it, `aspect` as wide as it is long. */
  | { kind: "centreline_rect"; from: [number, number]; aspect: number }
  /** Place Sub-grid between its two clicks (`DRAWING_TOOL::PlaceSubGrid`): the grid centred on the first, sized and turned to the cursor, drawn as a selected grid. */
  | { kind: "grid"; item: BoardGridItem };
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
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
    ctx.strokeRect(x0, y0, i.w, i.h);
  }
  ctx.restore();
  if (selected) {
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_ANCHOR");
    ctx.lineWidth = hairlineNm(view, 2);
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
  // and the footprints' own (UI-LEFT P17: `FOOTPRINT::m_drawings` are in the view as the footprint's children), drawn as selected with their footprint
  const ofFootprints = shownFootprintImages(board, opts.layerVisible);
  if ((images.length === 0 && ofFootprints.length === 0) || !on(opts.layerVisible, "bitmaps")) return;
  const opacity = opacityOf(opts.appearance, "images");
  if (opacity <= 0) return;
  for (const i of images) {
    if (!layerIsVisible(opts.layerVisible, i.layer)) continue;
    const dim = refImageLayerAlpha(opts, layerStateKey(i.layer));
    withAlpha(ctx, dim, () => paintRefImage(ctx, view, i, colourAlpha(i.layer) * opacity, opts.selection.has(i.id)));
  }
  for (const { image: i, part } of ofFootprints) {
    const dim = refImageLayerAlpha(opts, layerStateKey(i.layer));
    // a footprint in a preview that is not the carry (Pack and Move's block: painter.ts `drawFootprint` moves it by the preview's offset) takes its image along
    const preview = opts.movePreview && opts.movePreview.kind !== "pcb" && opts.movePreview.refs.includes(part.ref) ? opts.movePreview : null;
    const own = preview?.perRefOffsetNm?.[part.ref];
    const shown = preview ? { ...i, x: i.x + preview.dxNm + (own?.[0] ?? 0), y: i.y + preview.dyNm + (own?.[1] ?? 0) } : i;
    withAlpha(ctx, dim, () => paintRefImage(ctx, view, shown, colourAlpha(i.layer) * opacity, opts.selection.has(part.ref)));
  }
}

// ------------------------------------------------------------------------------------------------------------------------------------ points

/**
 * `PCB_PAINTER::draw( const PCB_POINT* )`: a cross -- two bars from corner to corner of a box of the point's size -- in the colour of the Points object (`LAYER_POINTS`) and a ring of a quarter of the
 * size in radius in the colour of the point's own layer, both a pixel wide whatever the zoom ("just draw in pixels"); a selected point is drawn in the selection colour. Shown while the Points
 * object is on and the point's layer is.
 */
export function drawPoints(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  const points = board.drawings?.points ?? [];
  if (points.length === 0 || !on(opts.layerVisible, "ly_points")) return;
  for (const p of points) drawPoint(ctx, view, p, opts.selection.has(p.id), opts);
}

/** One point (see [`drawPoints`]), when the Points object and its layer are shown. */
export function drawPoint(ctx: CanvasRenderingContext2D, view: ViewTransform, p: BoardPoint, selected: boolean, opts: PaintOptions): void {
  if (!on(opts.layerVisible, "ly_points") || !layerIsVisible(opts.layerVisible, p.layer)) return;
  const half = p.size / 2;
  withLayer(ctx, opts, layerStateKey(p.layer), 1, () => {
    ctx.save();
    ctx.lineWidth = hairlineNm(view, selected ? 2 : 1);
    ctx.lineCap = "round";
    ctx.strokeStyle = selected ? layerColor("selection") : layerColor("LAYER_POINTS");
    ctx.beginPath();
    ctx.moveTo(p.x - half, p.y - half);
    ctx.lineTo(p.x + half, p.y + half);
    ctx.moveTo(p.x + half, p.y - half);
    ctx.lineTo(p.x - half, p.y + half);
    ctx.stroke();
    ctx.strokeStyle = selected ? layerColor("selection") : layerColor(layerKeyOf(p.layer));
    ctx.beginPath();
    ctx.arc(p.x, p.y, half / 2, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  });
}

/** Every item of the kinds this module draws that the board has, and the rubber band of the tool in progress. */
export function drawPcbItems(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  drawTextBoxes(ctx, view, board, opts);
  drawBarcodes(ctx, view, board, opts);
  drawPoints(ctx, view, board, opts);
  const cursor = opts.snappedCursor ?? (opts.cursorNm ? ([opts.cursorNm.x, opts.cursorNm.y] as const) : null);
  // The image in the hand follows the cursor, centred on it, at the alpha it will have.
  if (toolPreview?.kind === "image" && cursor && !opts.movePreview) {
    const { img, w, h, layer } = toolPreview;
    ctx.save();
    ctx.globalAlpha *= colourAlpha(layer) * opacityOf(opts.appearance, "images");
    ctx.drawImage(img, cursor[0] - Math.trunc(w / 2), cursor[1] - Math.trunc(h / 2), w, h);
    ctx.restore();
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_ANCHOR");
    ctx.lineWidth = hairlineNm(view, 2);
    ctx.strokeRect(cursor[0] - Math.trunc(w / 2), cursor[1] - Math.trunc(h / 2), w, h);
    ctx.restore();
  }
  if (toolPreview?.kind === "centreline_rect" && cursor && !opts.movePreview) {
    // `inductorAreaFill`, `inductorAreaStroke` and `inductorAreaStrokeWidth` of `drawMicrowaveInductor`; the end is where `TWO_POINT_GEOMETRY_MANAGER::SetEnd` holds it.
    const start = { x: toolPreview.from[0], y: toolPreview.from[1] };
    const end = inductorEnd(start, { x: cursor[0], y: cursor[1] }, opts.angleSnapMode ?? "45");
    const corners = centrelineRect(start, end, toolPreview.aspect);
    ctx.save();
    ctx.fillStyle = "rgba(77, 77, 128, 0.3)";
    ctx.strokeStyle = "rgb(102, 255, 255)";
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.beginPath();
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(end.x, end.y);
    corners.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.restore();
  }
  if (toolPreview?.kind === "grid" && cursor && !opts.movePreview) drawGridItemPreview(ctx, view, sizeGridToCursor(toolPreview.item, cursor));
  if (toolPreview?.kind === "rect" && cursor && !opts.movePreview) {
    ctx.save();
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineNm(view, 1.5);
    ctx.setLineDash([hairlineNm(view, 5), hairlineNm(view, 3)]);
    ctx.strokeRect(toolPreview.from[0], toolPreview.from[1], cursor[0] - toolPreview.from[0], cursor[1] - toolPreview.from[1]);
    ctx.restore();
  }
}

// ---------------------------------------------------------------------------------------------------------------------------------------- barcodes

function drawBarcodes(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  for (const b of board.drawings?.barcodes ?? []) {
    if (!layerIsVisible(opts.layerVisible, b.layer)) continue;
    requestBarcodeShape(b);
    withBoardLayer(ctx, opts, b.layer, () => drawBarcode(ctx, view, b, opts.selection.has(b.id)));
  }
}

/**
 * `PCB_PAINTER::draw( const PCB_BARCODE* )`: the polygons of the symbol and of its text (`m_poly`) filled in the layer's colour, in the selection colour when selected. They are what kicad-cli
 * plots of the barcode (kicad-port/barcodeShapes.ts); until they arrive, or when the code cannot be made, the box the symbol will fill is drawn with a cross through it.
 */
export function drawBarcode(ctx: CanvasRenderingContext2D, view: ViewTransform, b: BoardBarcode, selected: boolean, color?: string): void {
  const ink = color ?? (selected ? layerColor("selection") : layerColor(layerKeyOf(b.layer)));
  const state = barcodeShape(barcodeShapeKey(b));
  ctx.save();
  ctx.fillStyle = ink;
  ctx.strokeStyle = ink;
  if (state?.status === "ready" && state.paths.length > 0) {
    for (const path of state.paths) {
      ctx.beginPath();
      for (const ring of path) {
        ring.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(b.x + x, b.y + y) : ctx.lineTo(b.x + x, b.y + y)));
        ctx.closePath();
      }
      ctx.fill("evenodd");
    }
  } else if (b.text.length > 0 || state?.status === "failed") {
    // not here yet (or none: an empty text has nothing to encode): the box, crossed
    const c = symbolCorners(b);
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
    ctx.beginPath();
    c.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.moveTo(c[0]![0], c[0]![1]);
    ctx.lineTo(c[2]![0], c[2]![1]);
    ctx.moveTo(c[1]![0], c[1]![1]);
    ctx.lineTo(c[3]![0], c[3]![1]);
    ctx.stroke();
  } else {
    const c = symbolCorners(b);
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.beginPath();
    c.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.stroke();
  }
  ctx.restore();
}

function drawTextBoxes(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions): void {
  for (const b of board.drawings?.text_boxes ?? []) {
    if (!layerIsVisible(opts.layerVisible, b.layer)) continue;
    withBoardLayer(ctx, opts, b.layer, () => drawTextBox(ctx, view, b, opts.selection.has(b.id), !!opts.sketchText, { flippedView: keptReadableInFlippedView(opts.viewFlipped, b.layer, opts.layerTypes) }));
  }
}

/** How a text box is drawn when it is not the canvas's: the 3D viewer paints it into a texture in one colour, the knockout's text cut out of the fill. */
export interface TextBoxLook {
  color?: string;
  /** Erase the text from what is drawn (a knockout box) instead of painting it in the background's colour. */
  cutOutText?: boolean;
  /** Flip Board View on a box that is not side specific (kicad-port/flipView.ts): its text mirrored and hung from `GetDrawPos( true )`. */
  flippedView?: boolean;
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
    const w = Math.max(b.stroke_width, hairlineNm(view, selected ? 2 : 1));
    ctx.lineWidth = w;
    ctx.setLineDash(dashPattern(b.line_style === "default" ? "solid" : b.line_style, w));
    trace();
    ctx.stroke();
    ctx.setLineDash([]);
  } else if (selected) {
    // A box with no border still shows where it is when it is picked.
    ctx.strokeStyle = color;
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
    trace();
    ctx.stroke();
    ctx.setLineDash([]);
  }
  if ((b.shown ?? b.text) !== "") {
    const cut = !!look.cutOutText && b.knockout;
    if (cut) ctx.globalCompositeOperation = "destination-out";
    const textColor = cut ? "#000000" : b.knockout ? layerColor("background") : color;
    paintLayout(ctx, layoutOf(b, !!look.flippedView), { color: textColor, pen: lineMode ? hairlineNm(view, 1) : undefined });
  }
  ctx.restore();
}
