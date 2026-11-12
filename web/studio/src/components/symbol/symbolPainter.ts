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
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_DISPLAY, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO, type GridDisplay } from "../../kicad-port/grid";
import { paintEditorGrid } from "../canvas/gridPaint";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { FIELD_SIZE_NM } from "../../kicad-port/schText";
import { beginTextRecord, paintKicadText } from "../text/kicadTextPaint";
import { resolveFont } from "../../kicad-port/kicadText";
import { deviceTextDrawSpec, schFieldDrawSpec, schTextBoxDrawSpec } from "../../kicad-port/schTextSpecs";
import { shownText } from "../../kicad-port/schFieldEdit";
import { electricalTypeLayout, pinLabelsShown, pinShown } from "../../kicad-port/symPinText";
import { altIconStrokes, pinAltIcon, type PinAltIcon } from "../../kicad-port/symPinAltIcon";
import { DEFAULT_PIN_TEXTS, PIN_TEXT_SIZE_NM, shownPinName, type PinTexts } from "../schematic/pinText";
import { polyPreview } from "../../kicad-port/symPolyDraw";
import { boxOfTextBox, isVertical, textBoxMarginMm, type LibTextBox } from "../../kicad-port/symTextBox";
import { bezierFromClicks } from "../../kicad-port/symBezier";
import type { SchColor } from "../../api/schEditTypes";
import { rgba } from "../schematic/schGraphicsPainter";
import { paintEditPoints } from "../schematic/schArcEditPaint";
import type { EeEditPoints } from "../../kicad-port/eePointEdit";
import { withDefaultSchLook } from "../../kicad-port/schFormattingLook";
import { dashPattern } from "../schematic/schGraphicsPainter";
import { hatchLines, type SchHatchFill } from "../../kicad-port/schHatch";
import { arcCenter as symArcCenter } from "../../kicad-port/symTransform";
import type { FieldLooks, GraphicState } from "../../kicad-port/symLooks";
import type { SchLineStyle } from "../../api/schEditTypes";

/** `eeschema.SymbolDrawing.*` shapes in progress: `lines` (Draw Lines) and `polygon` (Draw Polygons) are the same open-ended polyline tool. */
export type SymShapeKind = "segment" | "lines" | "arc" | "rect" | "circle" | "polygon" | "bezier" | "textbox";

/** INACTIVE since UI-LEFT-2 YY1: the bezier editor's handle size; the point editor's is `EDIT_POINT::POINT_SIZE` (8 px, kicad-port/eePointEdit.ts `POINT_SIZE_PX`). */
export const POINT_EDIT_HALF_PX = 5;

/** Half the cap height, the shift that centres stroke text on its line (`MIDDLE_OFFSET_FACTOR` of `components/schematic/painter.ts`). */
const MIDDLE_OFFSET_FACTOR = 0.35;

/** This editor's own canvas has no "instance transform" the way a placed symbol does -- it draws/hit-tests the symbol's own graphics directly in its own frame, so every `resolvePin`/`resolveLibPoint` call in this file (and in `SymbolEditorCanvas.tsx`'s hit-testing) uses this identity matrix. */
export const IDENTITY = symbolTransformMatrix(0, null);

/** A `LibrarySymbolPin` (this editor's own `{x,y}`-object mm shape) as the `LibPin` `resolvePin` expects ([Mm,Mm] tuple) -- the one adapter seam between this editor's addressable, id-carrying pin type and the shared, resolved-geometry renderer. Exported for `SymbolEditorCanvas.tsx`'s own hit-testing, so there is exactly one place this adapter is written. */
export function toLibPin(p: LibrarySymbolPin): LibPin {
  return { number: p.number, name: p.name, electrical_type: p.electrical_type, shape: p.shape, at: [p.at.x, p.at.y], angle_deg: p.angle_deg, length_mm: p.length_mm, unit: p.unit, body_style: p.body_style, hidden: p.hidden };
}

/** A `LibrarySymbolGraphic` resolved to internal (µm, Y-down) space via the exact same `resolveLibPoint` pipeline a placed instance's own graphics go through. */
function resolveGraphic(g: Exclude<LibrarySymbolGraphic, { kind: "text_box" | "text" }>): ResolvedGraphic {
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
    case "bezier":
      return { kind: "bezier", start: pt(g.start), c1: pt(g.c1), c2: pt(g.c2), end: pt(g.end), strokeWidthNm: g.stroke_mm * 1e6, fill: g.fill };
  }
}

/**
 * `SCH_PAINTER::draw( SCH_TEXT )` of a symbol's text (`LAYER_DEVICE`, kicad-port/schTextSpecs.ts `deviceTextDrawSpec`): at its size, bold with a fifth of the size for a pen
 * (`GetPenSizeForBold`), italic leaning, at the centre of its box (its left or right edge for a left or right justification -- centred on both unless it says otherwise) and
 * centred vertically, a text at 90 degrees running up the sheet, in its face (Text Properties' Font: an outline font's once the browser has it, UI-LEFT-2 CC1).
 */
export function drawLibText(ctx: CanvasRenderingContext2D, g: Pick<Extract<LibrarySymbolGraphic, { kind: "text" }>, "text" | "at" | "angle_deg" | "size_mm" | "bold" | "italic" | "h_align" | "v_align" | "font" | "id">, color: string, atNm?: [number, number]) {
  const look = { at: atNm ?? mmPointToNm(g.at), h: g.h_align ?? "center", v: g.v_align ?? "center", vertical: Math.abs(g.angle_deg % 180) === 90, size_nm: g.size_mm * 1e6, bold: !!g.bold, italic: !!g.italic };
  paintKicadText(ctx, deviceTextDrawSpec(g.text, look, resolveFont(g.font, look.bold, look.italic)), g.font, { color, record: g.id ? { tag: "sym", id: g.id } : undefined });
}

/** Internal-space (µm) point for one `LibrarySymbolGraphic`/`LibrarySymbolPin`'s own anchor -- used only by the canvas's hit-testing/move-preview, not by this file's own drawing (which goes through `resolveGraphic`/`resolvePin` above instead). */
export function mmPointToNm(p: { x: number; y: number }): [number, number] {
  return resolveLibPoint([p.x, p.y], IDENTITY, [0, 0]);
}

/** The inverse of `mmPointToNm` -- an internal-space (µm, Y-down) point back to this symbol's own wire format (mm, Y-up), for sending a `Cmd`. */
export function nmPointToMm(xNm: number, yNm: number): { x: number; y: number } {
  return { x: xNm / 1e6, y: -yNm / 1e6 };
}

/** INACTIVE since UI-LEFT C2 (task-u4-common): the grid is `paintEditorGrid` (components/canvas/gridPaint.ts), DrawGrid's three styles. Kept, not called. */
export function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridNm: number) {
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
 * (kicad-port/symTextBox.ts), and the border when it has a width -- 0 is the default line width, a negative one is no border. `getRenderColor` (UI-LEFT Y4): a colour fill is
 * the box's fill colour, `LAYER_DEVICE_BACKGROUND`'s without one; the text is the box's text colour when it has one, the border its stroke colour (`borderColor ==
 * COLOR4D::UNSPECIFIED` or a brightened box take the layer's). `selected` draws the text and the border in the selection colour, as before.
 */
function drawLibTextBox(ctx: CanvasRenderingContext2D, g: LibTextBox, layerCol: string, selected = false, lineStyle: string | null = null) {
  const b = boxOfTextBox(g);
  const own = (c: SchColor | null | undefined) => (c && !(c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0) ? rgba(c) : null);
  const color = selected ? layerCol : own(g.text_color) ?? layerCol;
  if (g.fill !== "none") {
    ctx.fillStyle = g.fill === "color" ? own(g.fill_color) ?? layerColor("LAYER_DEVICE_BACKGROUND") : g.fill === "background" ? layerColor("LAYER_DEVICE_BACKGROUND") : color;
    ctx.fillRect(b.left, b.top, b.right - b.left, b.bottom - b.top);
  }
  // the text broken to the box less its margin, from `SCH_TEXTBOX::GetDrawPos`, in its face (kicad-port/schTextSpecs.ts `schTextBoxDrawSpec`)
  if (g.text) {
    const m = Math.round(textBoxMarginMm(g) * 1e6);
    const look = { start: [b.left, b.top] as const, end: [b.right, b.bottom] as const, margins: [m, m, m, m] as const, text: g.text, angleDeg: isVertical(g) ? 90 : 0, size_nm: g.size_mm * 1e6, h: g.h_align, v: g.v_align, bold: !!g.bold, italic: !!g.italic };
    paintKicadText(ctx, schTextBoxDrawSpec(look, resolveFont(g.font, look.bold, look.italic)), g.font, { color, record: g.id ? { tag: "sym", id: g.id } : undefined });
  }
  if (g.stroke_mm >= 0) {
    ctx.strokeStyle = selected ? layerCol : own(g.stroke_color) ?? layerCol;
    ctx.lineWidth = g.stroke_mm > 0 ? Math.max(g.stroke_mm * 1e6, 21_200) : 152_400;
    // the border's line style (UI-LEFT-2 YY2, `STROKE_PARAMS::Stroke`)
    ctx.setLineDash(dashPattern((lineStyle ?? "solid") as SchLineStyle, ctx.lineWidth));
    ctx.strokeRect(b.left, b.top, b.right - b.left, b.bottom - b.top);
    ctx.setLineDash([]);
  }
}

/**
 * `SCH_PAINTER::draw( SCH_SHAPE )` of a symbol's shape with the looks it keeps beside it (UI-LEFT-2 YY2): the fill on the background layer -- a hatch's lines or a colour fill
 * in the fill colour, the body outline fill in the stroke's, each `LAYER_DEVICE`'s when unspecified (`getRenderColor`), the body background's -- then the outline in the
 * stroke's colour with its line style (`STROKE_PARAMS::Stroke`'s dashes); a rectangle with a corner radius is a rounded one (`ROUNDRECT`). `selected` draws the outline in
 * the selection colour as before. A shape with none of these keeps `drawRealGraphic`.
 */
function drawLibShape(ctx: CanvasRenderingContext2D, g: Exclude<LibrarySymbolGraphic, { kind: "text_box" | "text" }>, st: GraphicState, layerCol: string, selected: boolean) {
  const own = (c: SchColor | null | undefined) => (c && !(c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0) ? rgba(c) : null);
  const device = layerColor("LAYER_DEVICE");
  const strokeCol = selected ? layerCol : own(st.looks.stroke_color) ?? layerCol;
  const pt = (p: { x: number; y: number }): [number, number] => mmPointToNm(p);
  const radiusNm = g.kind === "rectangle" ? Math.max(0, (st.looks.radius_mm ?? 0) * 1e6) : 0;
  const trace = () => {
    ctx.beginPath();
    switch (g.kind) {
      case "rectangle": {
        const [x0, y0] = pt(g.start);
        const [x1, y1] = pt(g.end);
        const x = Math.min(x0, x1);
        const y = Math.min(y0, y1);
        const w = Math.abs(x1 - x0);
        const h = Math.abs(y1 - y0);
        const r = Math.min(radiusNm, w / 2, h / 2);
        if (r > 0 && typeof ctx.roundRect === "function") ctx.roundRect(x, y, w, h, r);
        else ctx.rect(x, y, w, h);
        break;
      }
      case "polyline":
        g.pts.map(pt).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        break;
      case "circle": {
        const [cx, cy] = pt(g.center);
        ctx.moveTo(cx + g.radius_mm * 1e6, cy);
        ctx.arc(cx, cy, g.radius_mm * 1e6, 0, Math.PI * 2);
        break;
      }
      case "bezier": {
        const [a, c1, c2, b] = [pt(g.start), pt(g.c1), pt(g.c2), pt(g.end)];
        ctx.moveTo(a[0], a[1]);
        ctx.bezierCurveTo(c1[0], c1[1], c2[0], c2[1], b[0], b[1]);
        break;
      }
      case "arc": {
        const c = pt(symArcCenter(g.start, g.mid, g.end));
        const [s0, m, e] = [pt(g.start), pt(g.mid), pt(g.end)];
        const r = Math.hypot(s0[0] - c[0], s0[1] - c[1]);
        const a0 = Math.atan2(s0[1] - c[1], s0[0] - c[0]);
        const n180 = (a: number) => Math.atan2(Math.sin(a), Math.cos(a));
        const sweep = n180(Math.atan2(m[1] - c[1], m[0] - c[0]) - a0) + n180(Math.atan2(e[1] - c[1], e[0] - c[0]) - Math.atan2(m[1] - c[1], m[0] - c[0]));
        ctx.arc(c[0], c[1], r, a0, a0 + sweep, sweep < 0);
        break;
      }
    }
  };
  const widthNm = g.stroke_mm > 0 ? Math.max(g.stroke_mm * 1e6, 21_200) : g.stroke_mm === 0 ? 152_400 : null;
  // the background layer: a hatch, a colour fill, the body background (an arc is not filled: "we have no idea which side of the arc is inside")
  if (g.fill !== "none" && g.kind !== "arc") {
    ctx.save();
    if (g.fill === "color" && st.looks.hatch) {
      trace();
      ctx.clip();
      const xs: number[] = [];
      const ys: number[] = [];
      const pts = g.kind === "circle" ? [{ x: g.center.x - g.radius_mm, y: g.center.y - g.radius_mm }, { x: g.center.x + g.radius_mm, y: g.center.y + g.radius_mm }] : g.kind === "polyline" ? g.pts : g.kind === "bezier" ? [g.start, g.c1, g.c2, g.end] : [g.start, g.end];
      for (const p of pts.map(pt)) {
        xs.push(p[0]);
        ys.push(p[1]);
      }
      const h = hatchLines(st.looks.hatch as SchHatchFill, [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)], widthNm ?? 152_400);
      ctx.strokeStyle = own(st.looks.fill_color) ?? device;
      ctx.lineWidth = h.width;
      ctx.beginPath();
      for (const [ax, ay, bx, by] of h.segments) {
        ctx.moveTo(ax, ay);
        ctx.lineTo(bx, by);
      }
      ctx.stroke();
    } else {
      trace();
      ctx.fillStyle = g.fill === "color" ? (own(st.looks.fill_color) ?? device) : g.fill === "background" ? layerColor("LAYER_DEVICE_BACKGROUND") : selected ? layerCol : (own(st.looks.stroke_color) ?? layerCol);
      ctx.fill();
    }
    ctx.restore();
  }
  if (widthNm !== null) {
    trace();
    ctx.strokeStyle = strokeCol;
    ctx.lineWidth = widthNm;
    ctx.setLineDash(dashPattern((st.lineStyle ?? "solid") as SchLineStyle, widthNm));
    ctx.stroke();
    ctx.setLineDash([]);
  }
}

/** Rubber-band preview for the graphics tool currently in progress -- points already internal-space (µm), same convention the Footprint/PCB editors' own `drawInProgress` uses. */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: { shapeKind: SymShapeKind; pts: [number, number][] } | null, cursorNm: { x: number; y: number } | null) {
  if (!draw) return;
  if (draw.shapeKind === "bezier") {
    // `CalcEdit( cursorPos )` of the bezier's edit state, as `AddToPreview( item->Clone() )` shows it
    const b = bezierFromClicks(draw.pts, cursorNm ? [cursorNm.x, cursorNm.y] : null);
    if (!b) return;
    ctx.save();
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineNm(view, 1.5);
    ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
    ctx.beginPath();
    ctx.moveTo(b.start[0], b.start[1]);
    ctx.bezierCurveTo(b.c1[0], b.c1[1], b.c2[0], b.c2[1], b.end[0], b.end[1]);
    ctx.stroke();
    ctx.restore();
    return;
  }
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
  /** The current grid's Y size when it is not square. */
  gridYNm?: number;
  /** The Symbol Editor's Display Options > Grid Display. */
  gridDisplay?: GridDisplay;
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
  /** What the cursor carries until a click places it -- the Pin tool's pin and the Text tool's text after their dialogs, the items of Paste and Duplicate -- where they are now
   * (kicad-port/symCarry.ts `carriedItems`): drawn as the selection, over the symbol (`m_view->AddToPreview( item )`). */
  carried?: { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[]; looks?: (GraphicState | null)[] } | null;
  /** The looks a graphic keeps beside it (kicad-port/symLooks.ts `graphicStateOf`; UI-LEFT-2 YY2). */
  looksOf?: (id: string) => GraphicState | null;
  /** `SetBrightened` items (`IsBrightened()`): Find's matches while its dialog is open, the Pin Table's cross-probed pin -- drawn in `LAYER_BRIGHTENED`. */
  brightened?: ReadonlySet<string>;
  /** The items only: no grid, axes or anchor (Copy's image of the selection, `renderSymbolToBitmap`). */
  itemsOnly?: boolean;
  /** A field's face and colour kept beside it (UI-LEFT-2 YY2): drawn in its face, and in its colour unless selected or hidden (`getRenderColor`). */
  fieldLooksOf?: (name: string) => FieldLooks;
  /** The point editor (`SCH_POINT_EDITOR`) on the one selected shape: its edit points and indicator lines (internal nm), and the shape as a drag of one point has it so far
   * (`replace`, drawn in place of the symbol's). */
  /** The point editor on one shape (kicad-port/symPointEdit.ts): its handles, and the shape as the drag has it. */
  pointEditor?: { edit: EeEditPoints; replace?: LibrarySymbolGraphic } | null;
  /** The symbol's fields that are on show (kicad-port/symFieldLayout.ts `fieldsOnShow`: the ones with a text, the hidden ones only while Show Hidden Fields is on). */
  fields?: SchField[];
}

/** The colour KiCad draws a field of that name in (`SCH_PAINTER::draw( SCH_FIELD )`): the reference, the value, or a field's own. */
function fieldColor(name: string): string {
  return layerColor(name === "Reference" ? "LAYER_REFERENCEPART" : name === "Value" ? "LAYER_VALUEPART" : "LAYER_FIELDS");
}

/**
 * One field where KiCad draws it (`SCH_PAINTER::draw( SCH_FIELD )`, kicad-port/schTextSpecs.ts `schFieldDrawSpec`): centred on the centre of its box (the box of its text
 * with its own justification, turned a quarter counter-clockwise about its anchor for a field at 90 degrees), in its face; a bold one with a fifth of its size for a pen
 * (`GetPenSizeForBold`), an italic one leaning.
 */
function drawField(ctx: CanvasRenderingContext2D, f: SchField, color: string) {
  const text = shownText(f);
  if (!text) return;
  const look = { at: f.at, h: f.h, v: f.v, vertical: f.vertical, size_nm: f.size_nm && f.size_nm > 0 ? f.size_nm : FIELD_SIZE_NM, bold: !!f.bold, italic: !!f.italic };
  paintKicadText(ctx, schFieldDrawSpec(text, look, resolveFont(f.font, look.bold, look.italic)), f.font, { color, record: f.id ? { tag: "sym", id: f.id } : undefined });
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

type PaintedSymbol = { graphics: LibrarySymbolGraphic[]; pins: LibrarySymbolPin[]; pin_names_hidden?: boolean; pin_numbers_hidden?: boolean; pin_name_offset_mm?: number } | null;

/** The Symbol Editor's canvas and the choosers' previews: drawn with the library editor's render settings, KiCad's defaults, not the schematic's Formatting
 *  (kicad-port/schFormattingLook.ts `withDefaultSchLook`). */
export function paintSymbol(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, symbol: PaintedSymbol, opts: SymPaintOptions) {
  withDefaultSchLook(() => paintSymbolDefaults(ctx, view, widthPx, heightPx, symbol, opts));
}

function paintSymbolDefaults(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, symbol: PaintedSymbol, opts: SymPaintOptions) {
  // what `window.__edaText.sym` reports is what this paint draws
  beginTextRecord("sym");
  // the Symbol Editor's axes are always on (`SYMBOL_EDIT_FRAME`: `m_axesEnabled = true`), in its theme's `LAYER_SCHEMATIC_GRID_AXES`
  // (`SYMBOL_EDIT_FRAME::SYMBOL_EDIT_FRAME` and `SwitchCanvas`: `SetAxesColor( m_colorSettings->GetColor( LAYER_SCHEMATIC_GRID_AXES ) )`); the grid itself only when shown
  if (!opts.itemsOnly) {
    const d = opts.gridDisplay ?? DEFAULT_GRID_DISPLAY;
    paintEditorGrid(ctx, view, widthPx, heightPx, { size: { x: opts.gridNm, y: opts.gridYNm ?? opts.gridNm }, origin: [0, 0], style: d.style, lineWidthPx: d.lineWidthPx, minSpacingPx: d.minSpacingPx, visible: opts.gridVisible, axes: d.axes ?? true }, layerColor("grid"), undefined, "symbol", layerColor("LAYER_SCHEMATIC_GRID_AXES"));
  }
  if (!symbol) {
    drawOrigin(ctx, view);
    return;
  }

  const moved = (id: string | undefined) => (id && opts.movePreview?.refs.includes(id) ? opts.movePreview : null);
  const strokeColor = layerColor("LAYER_DEVICE");

  const looksOf = (g: LibrarySymbolGraphic, st?: GraphicState | null): GraphicState | null => st ?? (g.id && opts.looksOf ? opts.looksOf(g.id) : null);
  const drawGraphic = (g: LibrarySymbolGraphic, color: string, carriedLooks?: GraphicState | null) => {
    // `drawRealGraphic` strokes with whatever `strokeStyle` is current (a placed symbol's painter sets it first); left alone it is the canvas default, black.
    ctx.strokeStyle = color;
    if (g.kind === "text_box") drawLibTextBox(ctx, g, color, color !== strokeColor, looksOf(g, carriedLooks)?.lineStyle ?? null);
    // `getRenderColor`: a symbol's text in its own colour when it has one (`!isSymbolChild || GetTextColor() != UNSPECIFIED`)
    else if (g.kind === "text") drawLibText(ctx, g, color === strokeColor && g.text_color && g.text_color.a > 0 ? rgba(g.text_color) : color);
    else {
      const st = looksOf(g, carriedLooks);
      if (st && (Object.keys(st.looks).length > 0 || (st.lineStyle && st.lineStyle !== "solid") || g.fill === "color")) drawLibShape(ctx, g, st, color, color !== strokeColor);
      else if (g.fill === "color") drawLibShape(ctx, g, { looks: {}, lineStyle: null, thicknessMm: null }, color, color !== strokeColor);
      else drawRealGraphic(ctx, resolveGraphic(g), color);
    }
  };
  const replace = opts.pointEditor?.replace;
  for (const stored of symbol.graphics) {
    const g = replace && replace.id === stored.id ? replace : stored;
    if (!visibleHere(g.unit, g.body_style, opts.activeUnit, opts.activeBodyStyle)) continue;
    const mv = moved(g.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    const selected = opts.selection.has(g.id ?? "");
    // `getRenderColor`: a brightened item (Find's matches, the Pin Table's cross-probe) in `LAYER_BRIGHTENED`
    drawGraphic(g, selected ? layerColor("selection") : opts.brightened?.has(g.id ?? "") ? layerColor("LAYER_BRIGHTENED") : strokeColor);
    ctx.restore();
  }

  const labels = pinLabelsShown(symbol, opts.showPinNumbers);
  // `(pin_names (offset x))`: the names are inside the body from `x` on, over the pin line when it is zero (`GetPinNameInfo`)
  const texts: PinTexts = { ...DEFAULT_PIN_TEXTS, nameOffsetNm: symbol.pin_name_offset_mm === undefined ? DEFAULT_PIN_TEXTS.nameOffsetNm : symbol.pin_name_offset_mm * 1e6 };
  const hiddenColor = layerColor("LAYER_HIDDEN");
  const drawPin = (p: LibrarySymbolPin, selected: boolean) => {
    const rp = resolvePin(toLibPin(p), IDENTITY, [0, 0]);
    const bright = !selected && !!p.id && !!opts.brightened?.has(p.id);
    ctx.beginPath();
    ctx.strokeStyle = selected ? layerColor("selection") : bright ? layerColor("LAYER_BRIGHTENED") : p.hidden ? hiddenColor : layerColor("LAYER_PIN");
    ctx.lineWidth = Math.max(152.4, hairlineNm(view, 1));
    drawPinDecoration(ctx, rp);
    ctx.stroke();
    // The labels are the symbol's call (`GetShowPinNames` / `GetShowPinNumbers`): blank what is not shown, the shared painter draws the rest.
    const shownPin = { ...rp.pin, name: labels.names ? rp.pin.name : "", number: labels.numbers ? rp.pin.number : "" };
    drawPinText(ctx, { ...rp, pin: shownPin }, bright ? { name: layerColor("LAYER_BRIGHTENED"), number: layerColor("LAYER_BRIGHTENED") } : p.hidden ? { name: hiddenColor, number: hiddenColor } : undefined, texts);
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
  };
  for (const p of symbol.pins) {
    // `SCH_PAINTER::draw( SCH_PIN )`: a pin marked invisible is drawn only while hidden pins are shown, and then in the hidden colour
    // (the symbol editor starts with them shown, `show_hidden_lib_pins`).
    if (!pinShown(p.hidden, opts.showHiddenPins) || !visibleHere(p.unit, p.body_style, opts.activeUnit, opts.activeBodyStyle)) continue;
    const mv = moved(p.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    drawPin(p, opts.selection.has(p.id ?? ""));
    ctx.restore();
  }

  // The fields (`SCH_FIELD` items of the symbol): a field that is not shown is drawn, in the hidden-items colour, because the list holds it only while Show Hidden Fields is on.
  for (const f of opts.fields ?? []) {
    const mv = moved(f.id);
    ctx.save();
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    const fl = opts.fieldLooksOf?.(f.name);
    const ownColor = fl?.color && !(fl.color.r === 0 && fl.color.g === 0 && fl.color.b === 0 && fl.color.a === 0) ? rgba(fl.color) : null;
    drawField(ctx, fl?.font ? { ...f, font: fl.font } : f, opts.selection.has(f.id ?? "") ? layerColor("selection") : opts.brightened?.has(f.id ?? "") ? layerColor("LAYER_BRIGHTENED") : !f.visible ? hiddenColor : (ownColor ?? fieldColor(f.name)));
    ctx.restore();
  }

  if (!opts.itemsOnly) drawOrigin(ctx, view);
  drawInProgress(ctx, view, opts.drawState, opts.cursorNm);
  // `EDIT_POINTS::ViewDraw`: the indicator lines, then a square at each edit point and a round handle in the middle of each side (components/schematic/schArcEditPaint.ts)
  if (opts.pointEditor) paintEditPoints(ctx, view, opts.pointEditor.edit, { fill: "#1a1a1a", stroke: layerColor("selection") });

  // what the cursor carries, where it is now, as the selection (`AddToPreview`)
  if (opts.carried) {
    opts.carried.graphics.forEach((g, n) => {
      ctx.save();
      drawGraphic(g, layerColor("selection"), opts.carried?.looks?.[n] ?? null);
      ctx.restore();
    });
    for (const p of opts.carried.pins) {
      ctx.save();
      drawPin(p, true);
      ctx.restore();
    }
  }
}
