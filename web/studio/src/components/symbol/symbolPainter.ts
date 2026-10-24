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
import type { LibPin, LibrarySymbolGraphic, LibrarySymbolPin, SchField } from "../../api/types";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineNm } from "../../kicad-port/view";
import { layerColor } from "../canvas/layers";
import { drawPinDecoration, drawPinText, drawRealGraphic } from "../schematic/painter";
import type { ResolvedGraphic } from "../schematic/libSymbol";
import { resolveLibPoint, resolvePin, symbolTransformMatrix } from "../schematic/transform";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { defaultPenNm, textOrigin, FIELD_SIZE_NM } from "../../kicad-port/schText";
import { shownText } from "../../kicad-port/schFieldEdit";
import { electricalTypeLayout, pinLabelsShown, pinShown } from "../../kicad-port/symPinText";
import { altIconStrokes, pinAltIcon, type PinAltIcon } from "../../kicad-port/symPinAltIcon";
import { DEFAULT_PIN_TEXTS, PIN_TEXT_SIZE_NM, shownPinName, type PinTexts } from "../schematic/pinText";
import { polyPreview } from "../../kicad-port/symPolyDraw";
import { boxOfTextBox, layoutLibTextBox, type LibTextBox } from "../../kicad-port/symTextBox";

/** `eeschema.SymbolDrawing.*` shapes in progress: `lines` (Draw Lines) and `polygon` (Draw Polygons) are the same open-ended polyline tool. */
export type SymShapeKind = "segment" | "lines" | "arc" | "rect" | "circle" | "polygon" | "textbox";

/** Half the cap height, the shift that centres stroke text on its line (`MIDDLE_OFFSET_FACTOR` of `components/schematic/painter.ts`). */
const MIDDLE_OFFSET_FACTOR = 0.35;

/** This editor's own canvas has no "instance transform" the way a placed symbol does -- it draws/hit-tests the symbol's own graphics directly in its own frame, so every `resolvePin`/`resolveLibPoint` call in this file (and in `SymbolEditorCanvas.tsx`'s hit-testing) uses this identity matrix. */
export const IDENTITY = symbolTransformMatrix(0, null);

/** A `LibrarySymbolPin` (this editor's own `{x,y}`-object mm shape) as the `LibPin` `resolvePin` expects ([Mm,Mm] tuple) -- the one adapter seam between this editor's addressable, id-carrying pin type and the shared, resolved-geometry renderer. Exported for `SymbolEditorCanvas.tsx`'s own hit-testing, so there is exactly one place this adapter is written. */
export function toLibPin(p: LibrarySymbolPin): LibPin {
  return { number: p.number, name: p.name, electrical_type: p.electrical_type, shape: p.shape, at: [p.at.x, p.at.y], angle_deg: p.angle_deg, length_mm: p.length_mm, unit: p.unit, body_style: p.body_style, hidden: p.hidden };
}

/** A `LibrarySymbolGraphic` resolved to internal (µm, Y-down) space via the exact same `resolveLibPoint` pipeline a placed instance's own graphics go through. */
function resolveGraphic(g: Exclude<LibrarySymbolGraphic, { kind: "text_box" }>): ResolvedGraphic {
  const pt = (p: { x: number; y: number }) => resolveLibPoint([p.x, p.y], IDENTITY, [0, 0]);
  switch (g.kind) {
    case "rectangle":
      return { kind: "rectangle", start: pt(g.start), end: pt(g.end), strokeWidthNm: g.stroke_mm * 1e6, fill: g.fill };
    case "polyline":
      return { kind: "polyline", pts: g.pts.map(pt), strokeWidthNm: g.stroke_mm * 1e6, fill: g.fill };
    case "circle":
      return { kind: "circle", center: pt(g.center), radiusNm: g.radius_mm * 1e6, strokeWidthNm: g.stroke_mm * 1e6, fill: g.fill };
    case "arc":
      return { kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end), strokeWidthNm: g.stroke_mm * 1e6, fill: g.fill };
    case "text":
      return { kind: "text", content: g.text, at: pt(g.at), angleDeg: g.angle_deg, sizeNm: g.size_mm * 1e6 };
  }
}

/** Internal-space (µm) point for one `LibrarySymbolGraphic`/`LibrarySymbolPin`'s own anchor -- used only by the canvas's hit-testing/move-preview, not by this file's own drawing (which goes through `resolveGraphic`/`resolvePin` above instead). */
export function mmPointToNm(p: { x: number; y: number }): [number, number] {
  return resolveLibPoint([p.x, p.y], IDENTITY, [0, 0]);
}

/** The inverse of `mmPointToNm` -- an internal-space (µm, Y-down) point back to this symbol's own wire format (mm, Y-up), for sending a `Cmd`. */
export function nmPointToMm(xNm: number, yNm: number): { x: number; y: number } {
  return { x: xNm / 1e6, y: -yNm / 1e6 };
}

function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridNm: number) {
  const visible = computeVisibleGridSize(gridNm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return;
  const x0 = -view.x / view.scale;
  const y0 = -view.y / view.scale;
  const wNm = widthPx / view.scale;
  const hNm = heightPx / view.scale;
  const firstIndexX = Math.floor(x0 / visible) - 1;
  const firstIndexY = Math.floor(y0 / visible) - 1;
  const lastIndexX = Math.ceil((x0 + wNm) / visible) + 1;
  const lastIndexY = Math.ceil((y0 + hNm) / visible) + 1;
  ctx.fillStyle = layerColor("grid");
  const minorR = hairlineNm(view, stepPx < 8 ? 0.6 : 1);
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
  const r = hairlineNm(view, 10);
  ctx.save();
  ctx.strokeStyle = layerColor("anchor");
  ctx.lineWidth = hairlineNm(view, 1.5);
  ctx.beginPath();
  ctx.moveTo(-r, 0);
  ctx.lineTo(r, 0);
  ctx.moveTo(0, -r);
  ctx.lineTo(0, r);
  ctx.stroke();
  ctx.restore();
}

/**
 * `SCH_PAINTER::draw( SCH_TEXTBOX )` for a box of the symbol (`LAYER_DEVICE`): the fill first (the body's background colour, or the outline colour), the wrapped text in the box
 * (kicad-port/symTextBox.ts), and the border when it has a width -- 0 is the default line width, a negative one is no border.
 */
function drawLibTextBox(ctx: CanvasRenderingContext2D, g: LibTextBox, color: string) {
  const b = boxOfTextBox(g);
  if (g.fill !== "none") {
    ctx.fillStyle = g.fill === "background" ? layerColor("LAYER_DEVICE_BACKGROUND") : color;
    ctx.fillRect(b.left, b.top, b.right - b.left, b.bottom - b.top);
  }
  const sizeNm = g.size_mm * 1e6;
  const layout = layoutLibTextBox(g, measureStrokeText);
  ctx.save();
  ctx.translate(layout.origin[0], layout.origin[1]);
  if (layout.vertical) ctx.rotate(-Math.PI / 2);
  for (const l of layout.lines) drawStrokeText(ctx, l.text, l.u, l.v, { sizeNm, justify: l.justify, italic: !!g.italic, thicknessNm: g.bold ? sizeNm / 5 : undefined, color });
  ctx.restore();
  if (g.stroke_mm >= 0) {
    ctx.strokeStyle = color;
    ctx.lineWidth = g.stroke_mm > 0 ? Math.max(g.stroke_mm * 1e6, 21_200) : 152_400;
    ctx.strokeRect(b.left, b.top, b.right - b.left, b.bottom - b.top);
  }
}

/** Rubber-band preview for the graphics tool currently in progress -- points already internal-space (µm), same convention the Footprint/PCB editors' own `drawInProgress` uses. */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: { shapeKind: SymShapeKind; pts: [number, number][] } | null, cursorNm: { x: number; y: number } | null) {
  if (!draw) return;
  // The floating vertex of a poly shape (`EDA_SHAPE::calcEdit`) follows the cursor.
  const pts = polyPreview(draw.pts, cursorNm ? [cursorNm.x, cursorNm.y] : null);
  if (pts.length < 2) return;
  ctx.save();
  ctx.strokeStyle = layerColor("selection");
  ctx.lineWidth = hairlineNm(view, 1.5);
  ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
  if (draw.shapeKind === "circle") {
    const [cx, cy] = pts[0]!;
    const [ex, ey] = pts[pts.length - 1]!;
    ctx.beginPath();
    ctx.arc(cx, cy, Math.hypot(ex - cx, ey - cy), 0, Math.PI * 2);
    ctx.stroke();
  } else if (draw.shapeKind === "rect" || draw.shapeKind === "textbox") {
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
  gridNm: number;
  gridVisible: boolean;
  activeUnit: number;
  activeBodyStyle: number;
  drawState: { shapeKind: SymShapeKind; pts: [number, number][] } | null;
  cursorNm: { x: number; y: number } | null;
  movePreview: { refs: string[]; dxNm: number; dyNm: number } | null;
  /** `m_ShowPinsElectricalType`: the electrical type's name beside every pin. */
  showElectricalTypes: boolean;
  /** `m_ShowHiddenPins`: pins marked hidden are drawn (in the hidden colour) -- not at all when off. */
  showHiddenPins: boolean;
  /** `m_ShowPinNumbers`: force the pin numbers on even where the symbol hides them. */
  showPinNumbers: boolean;
  /** `m_ShowPinAltIcons`: the icon beside the name of a pin that has alternate functions. */
  showPinAltIcons?: boolean;
  /** The Text tool's text between its dialog and the placing click: it follows the cursor (`SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`). */
  pendingText: { text: string; sizeMm: number; angleDeg: number } | null;
  /** The symbol's fields that are on show (kicad-port/symFieldLayout.ts `fieldsOnShow`: the ones with a text, the hidden ones only while Show Hidden Fields is on). */
  fields?: SchField[];
}

/** The colour KiCad draws a field of that name in (`SCH_PAINTER::draw( SCH_FIELD )`): the reference, the value, or a field's own. */
function fieldColor(name: string): string {
  return layerColor(name === "Reference" ? "LAYER_REFERENCEPART" : name === "Value" ? "LAYER_VALUEPART" : "LAYER_FIELDS");
}

/** One field where KiCad puts it: the anchor, the justification in the text's own axes, a text at 90 degrees turned a quarter counter-clockwise about its anchor. */
function drawField(ctx: CanvasRenderingContext2D, f: SchField, color: string) {
  const text = shownText(f);
  if (!text) return;
  const sizeNm = f.size_nm && f.size_nm > 0 ? f.size_nm : FIELD_SIZE_NM;
  // a bold text is written with a fifth of its size for a pen (`GetPenSizeForBold`), an italic one leans
  const thickness = f.bold ? Math.round(sizeNm / 5) : defaultPenNm(sizeNm);
  const [dx, dy] = textOrigin(measureStrokeText(text, sizeNm), sizeNm, thickness, f.h, f.v);
  ctx.save();
  ctx.translate(f.at[0], f.at[1]);
  if (f.vertical) ctx.rotate(-Math.PI / 2);
  drawStrokeText(ctx, text, dx, dy, { sizeNm, thicknessNm: thickness, justify: "left", color, italic: f.italic });
  ctx.restore();
}

/** `drawAltPinModesIcon`: the strokes of the glyph, turned a quarter counter-clockwise about its centre beside a pin that runs up or down. */
export function drawPinAltIcon(ctx: CanvasRenderingContext2D, icon: PinAltIcon, color: string) {
  const g = altIconStrokes(icon.sizeNm);
  ctx.save();
  ctx.translate(icon.center[0], icon.center[1]);
  if (icon.vertical) ctx.rotate(-Math.PI / 2);
  ctx.strokeStyle = color;
  ctx.lineWidth = g.widthNm;
  ctx.lineCap = "round";
  ctx.beginPath();
  for (const [x1, y1, x2, y2] of g.lines) {
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
  }
  ctx.moveTo(g.arc.cx + g.arc.r * Math.cos(g.arc.from), g.arc.cy + g.arc.r * Math.sin(g.arc.from));
  ctx.arc(g.arc.cx, g.arc.cy, g.arc.r, g.arc.from, g.arc.to, true);
  ctx.stroke();
  ctx.restore();
}

export function paintSymbol(
  ctx: CanvasRenderingContext2D,
  view: ViewTransform,
  widthPx: number,
  heightPx: number,
  symbol: { graphics: LibrarySymbolGraphic[]; pins: LibrarySymbolPin[]; pin_names_hidden?: boolean; pin_numbers_hidden?: boolean; pin_name_offset_mm?: number } | null,
  opts: SymPaintOptions
) {
  if (opts.gridVisible) drawGrid(ctx, view, widthPx, heightPx, opts.gridNm);
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
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    const selected = opts.selection.has(g.id ?? "");
    const color = selected ? layerColor("selection") : strokeColor;
    // `drawRealGraphic` strokes with whatever `strokeStyle` is current (a placed symbol's painter sets it first); left alone it is the canvas default, black.
    ctx.strokeStyle = color;
    if (g.kind === "text_box") drawLibTextBox(ctx, g, color);
    else drawRealGraphic(ctx, resolveGraphic(g), color);
    ctx.restore();
  }

  const labels = pinLabelsShown(symbol, opts.showPinNumbers);
  // `(pin_names (offset x))`: the names are inside the body from `x` on, over the pin line when it is zero (`GetPinNameInfo`)
  const texts: PinTexts = { ...DEFAULT_PIN_TEXTS, nameOffsetNm: symbol.pin_name_offset_mm === undefined ? DEFAULT_PIN_TEXTS.nameOffsetNm : symbol.pin_name_offset_mm * 1e6 };
  const hiddenColor = layerColor("LAYER_HIDDEN");
  for (const p of symbol.pins) {
    // `SCH_PAINTER::draw( SCH_PIN )`: a pin marked invisible is drawn only while hidden pins are shown, and then in the hidden colour
    // (the symbol editor starts with them shown, `show_hidden_lib_pins`).
    if (!pinShown(p.hidden, opts.showHiddenPins) || !visibleHere(p.unit, p.body_style, opts.activeUnit, opts.activeBodyStyle)) continue;
    const mv = moved(p.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    const rp = resolvePin(toLibPin(p), IDENTITY, [0, 0]);
    const selected = opts.selection.has(p.id ?? "");
    ctx.beginPath();
    ctx.strokeStyle = selected ? layerColor("selection") : p.hidden ? hiddenColor : layerColor("LAYER_PIN");
    ctx.lineWidth = Math.max(152.4, hairlineNm(view, 1));
    drawPinDecoration(ctx, rp);
    ctx.stroke();
    // The labels are the symbol's call (`GetShowPinNames` / `GetShowPinNumbers`): blank what is not shown, the shared painter draws the rest.
    const shownPin = { ...rp.pin, name: labels.names ? rp.pin.name : "", number: labels.numbers ? rp.pin.number : "" };
    drawPinText(ctx, { ...rp, pin: shownPin }, p.hidden ? { name: hiddenColor, number: hiddenColor } : undefined, texts);
    // `SCH_PAINTER::draw( SCH_PIN )`: beside the name, in its colour, when the pin has alternates and the name is drawn
    if (opts.showPinAltIcons !== false && p.alternates?.length && labels.names && shownPinName(p.name) !== "") {
      const nameWidth = measureStrokeText(shownPinName(p.name), PIN_TEXT_SIZE_NM);
      drawPinAltIcon(ctx, pinAltIcon(rp.tip, rp.root, { widthNm: nameWidth, sizeNm: PIN_TEXT_SIZE_NM }, texts.nameOffsetNm), p.hidden ? hiddenColor : layerColor("LAYER_PINNAM"));
    }
    if (opts.showElectricalTypes && p.electrical_type !== "no_connect") {
      // `GetPinElectricalTypeInfo`, drawn in `LAYER_PRIVATE_NOTES` (the hidden colour on a hidden pin).
      const l = electricalTypeLayout(p, rp.tip, rp.dir);
      const color = p.hidden ? hiddenColor : layerColor("LAYER_PRIVATE_NOTES");
      const shift = l.sizeNm * MIDDLE_OFFSET_FACTOR;
      if (l.vertical) drawStrokeText(ctx, l.text, l.at[0] + shift, l.at[1], { sizeNm: l.sizeNm, thicknessNm: l.thicknessNm, angleRad: -Math.PI / 2, justify: l.justify, color });
      else drawStrokeText(ctx, l.text, l.at[0], l.at[1] + shift, { sizeNm: l.sizeNm, thicknessNm: l.thicknessNm, justify: l.justify, color });
    }
    ctx.restore();
  }

  // The fields (`SCH_FIELD` items of the symbol): a field that is not shown is drawn, in the hidden-items colour, because the list holds it only while Show Hidden Fields is on.
  for (const f of opts.fields ?? []) {
    const mv = moved(f.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    drawField(ctx, f, opts.selection.has(f.id ?? "") ? layerColor("selection") : !f.visible ? hiddenColor : fieldColor(f.name));
    ctx.restore();
  }

  drawOrigin(ctx, view);
  drawInProgress(ctx, view, opts.drawState, opts.cursorNm);
  if (opts.pendingText && opts.cursorNm) {
    // (the canvas hands over the cursor its grid helper gave -- on the text grid, or an anchor -- while the Text tool runs)
    const [x, y] = [opts.cursorNm.x, opts.cursorNm.y];
    drawRealGraphic(ctx, { kind: "text", content: opts.pendingText.text, at: [x, y], angleDeg: opts.pendingText.angleDeg, sizeNm: opts.pendingText.sizeMm * 1e6 }, layerColor("selection"));
  }
}
