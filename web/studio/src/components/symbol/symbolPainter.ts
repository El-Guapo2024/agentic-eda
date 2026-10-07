// The Symbol Editor's own painter (eeschema's Symbol Editor canvas) --
// draws one library symbol in isolation (no sheet, no other symbols,
// origin at the symbol's own electrical origin), filtered to the unit/
// body-style currently being edited, the same way real eeschema's symbol
// editor only ever shows one unit/style at a time.
//
// Deliberately thin: every real drawing primitive (graphics, pin
// decoration/text) is the exact same code the schematic's own placed-
// instance renderer uses (`components/schematic/painter.ts`'s now-exported
// `drawRealGraphic`/`drawPinDecoration`/`drawPinText`), resolved here with
// an identity transform/no instance offset instead of a placed instance's
// (`components/schematic/transform.ts`'s `resolvePin`/`resolveLibPoint`,
// unchanged) -- this editor IS that same geometry, just not yet placed
// anywhere, so reusing the renderer outright is what keeps the Symbol
// Editor's preview and a placed instance's own rendering from ever being
// able to disagree. Only the grid/anchor/in-progress-draw/move-preview
// chrome below is new, mirroring `components/footprint/footprintPainter
// .ts`'s own small set of editor-only additions to its own board painter.
import type { LibPin, LibrarySymbolGraphic, LibrarySymbolPin } from "../../api/types";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineUm } from "../../kicad-port/view";
import { layerColor } from "../canvas/layers";
import { drawPinDecoration, drawPinText, drawRealGraphic } from "../schematic/painter";
import type { ResolvedGraphic } from "../schematic/libSymbol";
import { resolveLibPoint, resolvePin, symbolTransformMatrix } from "../schematic/transform";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";
import { drawStrokeText } from "../text/strokeFont";
import { electricalTypeLayout, pinLabelsShown, pinShown } from "../../kicad-port/symPinText";
import { polyPreview } from "../../kicad-port/symPolyDraw";
import { snapPoint } from "../canvas/gridHelper";

/** `eeschema.SymbolDrawing.*` shapes in progress: `lines` (Draw Lines) and `polygon` (Draw Polygons) are the same open-ended polyline tool. */
export type SymShapeKind = "segment" | "lines" | "arc" | "rect" | "circle" | "polygon";

/** Half the cap height, the shift that centres stroke text on its line (`MIDDLE_OFFSET_FACTOR` of `components/schematic/painter.ts`). */
const MIDDLE_OFFSET_FACTOR = 0.35;

/** This editor's own canvas has no "instance transform" the way a placed symbol does -- it draws/hit-tests the symbol's own graphics directly in its own frame, so every `resolvePin`/`resolveLibPoint` call in this file (and in `SymbolEditorCanvas.tsx`'s hit-testing) uses this identity matrix. */
export const IDENTITY = symbolTransformMatrix(0, null);

/** A `LibrarySymbolPin` (this editor's own `{x,y}`-object mm shape) as the `LibPin` `resolvePin` expects ([Mm,Mm] tuple) -- the one adapter seam between this editor's addressable, id-carrying pin type and the shared, resolved-geometry renderer. Exported for `SymbolEditorCanvas.tsx`'s own hit-testing, so there is exactly one place this adapter is written. */
export function toLibPin(p: LibrarySymbolPin): LibPin {
  return { number: p.number, name: p.name, electrical_type: p.electrical_type, shape: p.shape, at: [p.at.x, p.at.y], angle_deg: p.angle_deg, length_mm: p.length_mm, unit: p.unit, body_style: p.body_style, hidden: p.hidden };
}

/** A `LibrarySymbolGraphic` resolved to internal (µm, Y-down) space via the exact same `resolveLibPoint` pipeline a placed instance's own graphics go through. */
function resolveGraphic(g: LibrarySymbolGraphic): ResolvedGraphic {
  const pt = (p: { x: number; y: number }) => resolveLibPoint([p.x, p.y], IDENTITY, [0, 0]);
  switch (g.kind) {
    case "rectangle":
      return { kind: "rectangle", start: pt(g.start), end: pt(g.end), strokeWidthUm: g.stroke_mm * 1000, fill: g.fill };
    case "polyline":
      return { kind: "polyline", pts: g.pts.map(pt), strokeWidthUm: g.stroke_mm * 1000, fill: g.fill };
    case "circle":
      return { kind: "circle", center: pt(g.center), radiusUm: g.radius_mm * 1000, strokeWidthUm: g.stroke_mm * 1000, fill: g.fill };
    case "arc":
      return { kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end), strokeWidthUm: g.stroke_mm * 1000, fill: g.fill };
    case "text":
      return { kind: "text", content: g.text, at: pt(g.at), angleDeg: g.angle_deg, sizeUm: g.size_mm * 1000 };
  }
}

/** Internal-space (µm) point for one `LibrarySymbolGraphic`/`LibrarySymbolPin`'s own anchor -- used only by the canvas's hit-testing/move-preview, not by this file's own drawing (which goes through `resolveGraphic`/`resolvePin` above instead). */
export function mmPointToUm(p: { x: number; y: number }): [number, number] {
  return resolveLibPoint([p.x, p.y], IDENTITY, [0, 0]);
}

/** The inverse of `mmPointToUm` -- an internal-space (µm, Y-down) point back to this symbol's own wire format (mm, Y-up), for sending a `Cmd`. */
export function umPointToMm(xUm: number, yUm: number): { x: number; y: number } {
  return { x: xUm / 1000, y: -yUm / 1000 };
}

function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number) {
  const visible = computeVisibleGridSize(gridUm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return;
  const x0 = -view.x / view.scale;
  const y0 = -view.y / view.scale;
  const wUm = widthPx / view.scale;
  const hUm = heightPx / view.scale;
  const firstIndexX = Math.floor(x0 / visible) - 1;
  const firstIndexY = Math.floor(y0 / visible) - 1;
  const lastIndexX = Math.ceil((x0 + wUm) / visible) + 1;
  const lastIndexY = Math.ceil((y0 + hUm) / visible) + 1;
  ctx.fillStyle = layerColor("grid");
  const minorR = hairlineUm(view, stepPx < 8 ? 0.6 : 1);
  const majorR = minorR * MAJOR_GRID_LINE_WIDTH_RATIO;
  for (let i = firstIndexX; i <= lastIndexX; i++) {
    const x = i * visible;
    const tickX = isMajorGridLine(i);
    for (let j = firstIndexY; j <= lastIndexY; j++) {
      const y = j * visible;
      ctx.beginPath();
      ctx.arc(x, y, tickX && isMajorGridLine(j) ? majorR : minorR, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/** The origin cross at (0,0) -- a library symbol's own electrical origin, same small crosshair convention `footprintPainter.ts`'s `drawAnchor` uses for a footprint's own anchor. */
function drawOrigin(ctx: CanvasRenderingContext2D, view: ViewTransform) {
  const r = hairlineUm(view, 10);
  ctx.save();
  ctx.strokeStyle = layerColor("anchor");
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.beginPath();
  ctx.moveTo(-r, 0);
  ctx.lineTo(r, 0);
  ctx.moveTo(0, -r);
  ctx.lineTo(0, r);
  ctx.stroke();
  ctx.restore();
}

/** Rubber-band preview for the graphics tool currently in progress -- points already internal-space (µm), same convention the Footprint/PCB editors' own `drawInProgress` uses. */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: { shapeKind: SymShapeKind; pts: [number, number][] } | null, cursorUm: { x: number; y: number } | null) {
  if (!draw) return;
  // The floating vertex of a poly shape (`EDA_SHAPE::calcEdit`) follows the cursor.
  const pts = polyPreview(draw.pts, cursorUm ? [cursorUm.x, cursorUm.y] : null);
  if (pts.length < 2) return;
  ctx.save();
  ctx.strokeStyle = layerColor("selection");
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
  if (draw.shapeKind === "circle") {
    const [cx, cy] = pts[0]!;
    const [ex, ey] = pts[pts.length - 1]!;
    ctx.beginPath();
    ctx.arc(cx, cy, Math.hypot(ex - cx, ey - cy), 0, Math.PI * 2);
    ctx.stroke();
  } else if (draw.shapeKind === "rect") {
    const [x0, y0] = pts[0]!;
    const [x1, y1] = pts[pts.length - 1]!;
    ctx.strokeRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
  } else {
    ctx.beginPath();
    pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }
  ctx.setLineDash([]);
  ctx.restore();
}

/** `0` (shared) or an exact match -- the Symbol Editor's own "only show the unit/style being edited" filter (`visibleFor` in libSymbol.ts, same rule, used here directly against this editor's own fields instead of a resolved `LibSymbol`'s). */
function visibleHere(itemUnit: number, itemBodyStyle: number, activeUnit: number, activeBodyStyle: number): boolean {
  return (itemUnit === 0 || itemUnit === activeUnit) && (itemBodyStyle === 0 || itemBodyStyle === activeBodyStyle);
}

export interface SymPaintOptions {
  selection: Set<string>;
  gridUm: number;
  gridVisible: boolean;
  activeUnit: number;
  activeBodyStyle: number;
  drawState: { shapeKind: SymShapeKind; pts: [number, number][] } | null;
  cursorUm: { x: number; y: number } | null;
  movePreview: { refs: string[]; dxUm: number; dyUm: number } | null;
  /** `m_ShowPinsElectricalType`: the electrical type's name beside every pin. */
  showElectricalTypes: boolean;
  /** `m_ShowHiddenPins`: pins marked hidden are drawn (in the hidden colour) -- not at all when off. */
  showHiddenPins: boolean;
  /** `m_ShowPinNumbers`: force the pin numbers on even where the symbol hides them. */
  showPinNumbers: boolean;
  /** The Text tool's text between its dialog and the placing click: it follows the cursor (`SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`). */
  pendingText: { text: string; sizeMm: number; angleDeg: number } | null;
}

export function paintSymbol(
  ctx: CanvasRenderingContext2D,
  view: ViewTransform,
  widthPx: number,
  heightPx: number,
  symbol: { graphics: LibrarySymbolGraphic[]; pins: LibrarySymbolPin[]; pin_names_hidden?: boolean; pin_numbers_hidden?: boolean } | null,
  opts: SymPaintOptions
) {
  if (opts.gridVisible) drawGrid(ctx, view, widthPx, heightPx, opts.gridUm);
  if (!symbol) {
    drawOrigin(ctx, view);
    return;
  }

  const moved = (id: string | undefined) => (id && opts.movePreview?.refs.includes(id) ? opts.movePreview : null);
  const strokeColor = layerColor("LAYER_DEVICE");

  for (const g of symbol.graphics) {
    if (!visibleHere(g.unit, g.body_style, opts.activeUnit, opts.activeBodyStyle)) continue;
    const mv = moved(g.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxUm, mv.dyUm);
    const selected = opts.selection.has(g.id ?? "");
    const color = selected ? layerColor("selection") : strokeColor;
    // `drawRealGraphic` strokes with whatever `strokeStyle` is current (a placed symbol's painter sets it first); left alone it is the canvas default, black.
    ctx.strokeStyle = color;
    drawRealGraphic(ctx, resolveGraphic(g), color);
    ctx.restore();
  }

  const labels = pinLabelsShown(symbol, opts.showPinNumbers);
  const hiddenColor = layerColor("LAYER_HIDDEN");
  for (const p of symbol.pins) {
    // `SCH_PAINTER::draw( SCH_PIN )`: a pin marked invisible is drawn only while hidden pins are shown, and then in the hidden colour
    // (the symbol editor starts with them shown, `show_hidden_lib_pins`).
    if (!pinShown(p.hidden, opts.showHiddenPins) || !visibleHere(p.unit, p.body_style, opts.activeUnit, opts.activeBodyStyle)) continue;
    const mv = moved(p.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxUm, mv.dyUm);
    const rp = resolvePin(toLibPin(p), IDENTITY, [0, 0]);
    const selected = opts.selection.has(p.id ?? "");
    ctx.beginPath();
    ctx.strokeStyle = selected ? layerColor("selection") : p.hidden ? hiddenColor : layerColor("LAYER_PIN");
    ctx.lineWidth = Math.max(152.4, hairlineUm(view, 1));
    drawPinDecoration(ctx, rp);
    ctx.stroke();
    // The labels are the symbol's call (`GetShowPinNames` / `GetShowPinNumbers`): blank what is not shown, the shared painter draws the rest.
    const shownPin = { ...rp.pin, name: labels.names ? rp.pin.name : "", number: labels.numbers ? rp.pin.number : "" };
    drawPinText(ctx, { ...rp, pin: shownPin }, p.hidden ? { name: hiddenColor, number: hiddenColor } : undefined);
    if (opts.showElectricalTypes && p.electrical_type !== "no_connect") {
      // `GetPinElectricalTypeInfo`, drawn in `LAYER_PRIVATE_NOTES` (the hidden colour on a hidden pin).
      const l = electricalTypeLayout(p, rp.tip, rp.dir);
      const color = p.hidden ? hiddenColor : layerColor("LAYER_PRIVATE_NOTES");
      const shift = l.sizeUm * MIDDLE_OFFSET_FACTOR;
      if (l.vertical) drawStrokeText(ctx, l.text, l.at[0] + shift, l.at[1], { sizeUm: l.sizeUm, thicknessUm: l.thicknessUm, angleRad: -Math.PI / 2, justify: l.justify, color });
      else drawStrokeText(ctx, l.text, l.at[0], l.at[1] + shift, { sizeUm: l.sizeUm, thicknessUm: l.thicknessUm, justify: l.justify, color });
    }
    ctx.restore();
  }

  drawOrigin(ctx, view);
  drawInProgress(ctx, view, opts.drawState, opts.cursorUm);
  if (opts.pendingText && opts.cursorUm) {
    const [x, y] = snapPoint(opts.cursorUm.x, opts.cursorUm.y, opts.gridUm);
    drawRealGraphic(ctx, { kind: "text", content: opts.pendingText.text, at: [x, y], angleDeg: opts.pendingText.angleDeg, sizeUm: opts.pendingText.sizeMm * 1000 }, layerColor("selection"));
  }
}
