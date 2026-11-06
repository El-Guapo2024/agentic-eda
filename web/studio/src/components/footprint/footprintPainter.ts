// The Footprint Editor's own painter (GAPS.md #8) -- pcbnew's
// pcb_painter.cpp as it draws one footprint in isolation (no board
// outline, no other footprints, origin at the footprint's own anchor).
//
// Deliberately a separate small file rather than extending components/
// canvas/painter.ts's `paintBoard`: that file's `Shape`/`Pad` types use
// the *board display* wire shape (`[x,y]` coordinate pairs -- see
// api/types.ts's own doc on why `BoardState`'s `Shape`/`Pad` differ from
// the Cmd-side `CmdShape`/`LibraryPad`), whereas `GET /api/footprint`
// returns the IR's native shape directly (`{x,y}` objects) -- reusing
// `paintBoard`'s per-shape drawing would need an adapter at every call
// site for no real sharing of logic, only of a few primitives. Those few
// (the grid, hairline width compensation, circumcircle-through-3-points
// for an arc, KiCad's own stroke font) *are* reused, imported directly.
import { isHiddenText, textFaceOf, textLookOf } from "../../kicad-port/fpKeep";
import type { TextLook } from "../../api/fpEditorTypes";
import type { ChamferCorners, CmdShape, CmdText, LibraryKeep, LibraryPad, Nm, Shape } from "../../api/types";
import { dashPieces, isDashStyle } from "../../kicad-port/strokeDash";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineNm } from "../../kicad-port/view";
import { layerColor } from "../canvas/layers";
import { circleThrough, drawGridOrigin, normalizeSweep } from "../canvas/painter";
import { drawArcPreview } from "../canvas/arcPreview";
import { drawBezierPreview } from "../canvas/bezierPreview";
import { BEZIER_MAX_ERROR_NM } from "../canvas/itemHitTest";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { trapezoidCorners } from "../../kicad-port/padShape";
import type { ArcGeom } from "../../kicad-port/arcGeom";
import type { BezierGeom } from "../../kicad-port/bezierGeom";
import { drawStrokeText } from "../text/strokeFont";
import { beginTextRecord, paintKicadText } from "../text/kicadTextPaint";
import { effectiveTextPenWidth, type KicadTextSpec } from "../../kicad-port/kicadText";
import { drawAngle } from "../../kicad-port/fpFields";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_DISPLAY, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO, type GridDisplay } from "../../kicad-port/grid";
import { paintEditorGrid } from "../canvas/gridPaint";
import type { FieldInfo } from "../../api/types";
import type { FootprintItemsDisplay } from "../../api/fpItemTypes";
import { paintFpFields, paintFpItems } from "./fpItemsPaint";
import { layerIsVisible } from "../../kicad-port/layerPresets";
import { objectOn } from "../../kicad-port/appearance";
import { fpPadOnLayer, fpPadShown, fpShapeOpacity, fpTextShown } from "../../kicad-port/fpAppearance";

/** Footprint-local layer name ("F.SilkS") -> colors.json's real key, same punctuation swap `painter.ts`'s own `realLayerKey` uses. */
function realLayerKey(layer: string): string {
  return layer.replace(/\./g, "_");
}

/** INACTIVE since UI-LEFT C2 (task-u4-common): the grid is `paintEditorGrid` (components/canvas/gridPaint.ts), DrawGrid's three styles. Kept, not called. */
export function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridNm: number, origin: readonly [number, number] = [0, 0]) {
  const visible = computeVisibleGridSize(gridNm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return;
  // The grid is anchored at the grid origin (`GAL::SetGridOrigin`): dots at `origin + i * visible`.
  const x0 = -view.x / view.scale - origin[0];
  const y0 = -view.y / view.scale - origin[1];
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
    const x = origin[0] + i * visible;
    const tickX = isMajorGridLine(i);
    for (let j = firstIndexY; j <= lastIndexY; j++) {
      const y = origin[1] + j * visible;
      ctx.beginPath();
      ctx.arc(x, y, tickX && isMajorGridLine(j) ? majorR : minorR, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/** The anchor marker at (0,0) -- `footprint_edit_frame.cpp`'s own small crosshair at the footprint's local origin (distinct from the grid origin, which is this same point here -- the footprint editor has no separate "move the grid" concept). */
function drawAnchor(ctx: CanvasRenderingContext2D, view: ViewTransform) {
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
 * A rectangle centered at the origin with some corners cut and the others rounded (`TransformRoundChamferedRectToPolygon`; the caller translates and rotates first).
 * `chamfer` is the cut along each edge from a flagged corner, `radius` the radius of the corners that are not.
 */
function chamferedRectPath(ctx: CanvasRenderingContext2D, w: Nm, h: Nm, chamfer: number, corners: ChamferCorners, radius = 0) {
  const hw = w / 2;
  const hh = h / 2;
  const c = Math.max(0, Math.min(chamfer, Math.min(w, h) / 2));
  const r = Math.max(0, Math.min(radius, hw, hh));
  type P = [number, number];
  // Clockwise from the middle of the top edge: each corner is the point itself, and where a cut starts and ends.
  const quad: { cut: boolean; at: P; from: P; to: P }[] = [
    { cut: !!corners.top_right, at: [hw, -hh], from: [hw - c, -hh], to: [hw, -hh + c] },
    { cut: !!corners.bottom_right, at: [hw, hh], from: [hw, hh - c], to: [hw - c, hh] },
    { cut: !!corners.bottom_left, at: [-hw, hh], from: [-hw + c, hh], to: [-hw, hh - c] },
    { cut: !!corners.top_left, at: [-hw, -hh], from: [-hw, -hh + c], to: [-hw + c, -hh] },
  ];
  const start: P = [0, -hh];
  ctx.beginPath();
  ctx.moveTo(start[0], start[1]);
  for (let i = 0; i < 4; i++) {
    const k = quad[i]!;
    if (k.cut) {
      ctx.lineTo(k.from[0], k.from[1]);
      ctx.lineTo(k.to[0], k.to[1]);
    } else if (r > 0) {
      // The way out of this corner is along the next side: toward the next corner (or where its cut starts), the last side toward where the path began.
      const next = quad[i + 1];
      const toward = next ? (next.cut ? next.from : next.at) : start;
      ctx.arcTo(k.at[0], k.at[1], toward[0], toward[1], r);
    } else {
      ctx.lineTo(k.at[0], k.at[1]);
    }
  }
  ctx.closePath();
}

/** A trapezoid centered at the origin: `PAD::buildEffectiveShape`'s four corners (`trapezoidCorners`). */
function trapezoidPath(ctx: CanvasRenderingContext2D, w: Nm, h: Nm, delta: [Nm, Nm]) {
  ctx.beginPath();
  trapezoidCorners(w, h, delta).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
}

/** The pad's own copper outline as a path, pad-local (origin at the pad's own center, unrotated -- caller translates/rotates to the pad's actual pose first). */
function tracePadPath(ctx: CanvasRenderingContext2D, pad: LibraryPad) {
  const [w, h] = pad.size;
  switch (pad.shape) {
    case "circle":
      ctx.beginPath();
      ctx.arc(0, 0, Math.min(w, h) / 2, 0, Math.PI * 2);
      return;
    case "oval":
      // A stadium: ctx.roundRect with radius = half the shorter side gives
      // exactly two semicircular ends plus straight sides, same shape
      // `PAD_SHAPE::OVAL` draws.
      ctx.beginPath();
      ctx.roundRect(-w / 2, -h / 2, w, h, Math.min(w, h) / 2);
      return;
    case "round_rect":
      ctx.beginPath();
      ctx.roundRect(-w / 2, -h / 2, w, h, (pad.roundrect_ratio ?? 0.25) * Math.min(w, h));
      return;
    case "trapezoid":
      trapezoidPath(ctx, w, h, pad.trapezoid_delta ?? [0, 0]);
      return;
    case "chamfered_rect":
      chamferedRectPath(ctx, w, h, (pad.chamfer_ratio ?? 0.2) * Math.min(w, h), pad.chamfer_corners, (pad.roundrect_ratio ?? 0) * Math.min(w, h));
      return;
    case "custom":
      // The anchor only (`PAD::GetAnchorPadShape`: a circle of the pad's width, or its rectangle); the basic shapes follow in `paintCustomPrimitives`.
      ctx.beginPath();
      if (pad.custom?.anchor === "rect") ctx.rect(-w / 2, -h / 2, w, h);
      else ctx.arc(0, 0, w / 2, 0, Math.PI * 2);
      return;
    case "rect":
    default:
      ctx.beginPath();
      ctx.rect(-w / 2, -h / 2, w, h);
  }
}

/**
 * A custom pad's basic shapes (`PAD::GetPrimitives`) as copper, in the pad's frame like its anchor: a filled shape is its area and half its width all
 * round, an unfilled one a stroke of its width with round ends -- the copper `PAD::GetEffectiveShape` hands the DRC. `grow` widens each by that much
 * on every side. A proxy item (`gr_vector`, `gr_bbox`) is not copper.
 */
function paintCustomPrimitives(ctx: CanvasRenderingContext2D, pad: LibraryPad, color: string, grow: number) {
  for (const prim of pad.custom?.primitives ?? []) {
    if (prim.proxy) continue;
    ctx.save();
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    // A canvas ignores a line width of 0, which would leave the one the last shape set; a micrometre is no line at any zoom.
    ctx.lineWidth = Math.max(Math.max(prim.shape.stroke_width, 0) + 2 * grow, 1);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    drawShapeGeometry(ctx, prim.shape);
    ctx.restore();
  }
}

function drawPad(ctx: CanvasRenderingContext2D, view: ViewTransform, pad: LibraryPad, opts: { selected: boolean; sketch?: boolean }) {
  // `PAD::SetOffset`: the copper shape sits at `at + rotate(offset)`
  // (offset is defined in the pad's own, pre-rotation local frame, so it
  // turns with the pad) while the drill stays exactly at `at` -- the two
  // nested `save/translate` scopes below give the copper its own origin
  // without moving the drill drawn afterward in the outer (un-offset) one.
  ctx.save();
  ctx.translate(pad.at.x, pad.at.y);
  ctx.rotate((pad.rot / 1000) * (Math.PI / 180));

  ctx.save();
  ctx.translate(pad.offset.x, pad.offset.y);
  if (pad.shape === "custom") {
    // A custom pad is the anchor and its basic shapes. The selection is a halo of every part grown a little, under the copper, so the outline
    // that shows is the union's and not each part's own.
    if (opts.selected) {
      const halo = hairlineNm(view, 1.25);
      const selection = layerColor("selection");
      tracePadPath(ctx, pad);
      ctx.fillStyle = selection;
      ctx.strokeStyle = selection;
      ctx.lineWidth = 2 * halo;
      ctx.fill();
      ctx.stroke();
      paintCustomPrimitives(ctx, pad, selection, halo);
    }
    tracePadPath(ctx, pad);
    ctx.fillStyle = layerColor("f_cu");
    ctx.fill();
    paintCustomPrimitives(ctx, pad, layerColor("f_cu"), 0);
  } else if (opts.sketch) {
    // Sketch Pads (`m_DisplayPadFill` off, `pcbnew.Control.padDisplayMode`): the pad's outline, a line of one and a half pixels, as the board's painter draws it.
    tracePadPath(ctx, pad);
    ctx.strokeStyle = opts.selected ? layerColor("selection") : layerColor("f_cu");
    ctx.lineWidth = hairlineNm(view, opts.selected ? 2.5 : 1.5);
    ctx.stroke();
  } else {
    tracePadPath(ctx, pad);
    ctx.fillStyle = layerColor("f_cu");
    ctx.fill();
    if (opts.selected) {
      ctx.strokeStyle = layerColor("selection");
      ctx.lineWidth = hairlineNm(view, 2.5);
      ctx.stroke();
    }
  }
  ctx.restore();

  if (pad.kind !== "smd") {
    // A through-hole/non-plated pad's drill, same "golden copper" hole-wall convention painter.ts's own board pads use.
    const drillR = pad.drill_slot ? Math.min(pad.drill_slot[0], pad.drill_slot[1]) / 2 : (pad.drill ?? 0) / 2;
    if (drillR > 0) {
      ctx.beginPath();
      if (pad.drill_slot) {
        const [dw, dh] = pad.drill_slot;
        ctx.roundRect(-dw / 2, -dh / 2, dw, dh, Math.min(dw, dh) / 2);
      } else {
        ctx.arc(0, 0, drillR, 0, Math.PI * 2);
      }
      ctx.fillStyle = layerColor("background");
      ctx.fill();
      ctx.strokeStyle = layerColor("pad_th");
      ctx.lineWidth = hairlineNm(view, 1);
      ctx.stroke();
    }
  }
  ctx.restore();

  // Pad number, centered, once legible.
  if (pad.size[0] * view.scale > 22 && pad.size[1] * view.scale > 10) {
    const fontNm = Math.min(pad.size[0], pad.size[1]) * 0.35;
    drawStrokeText(ctx, pad.number, pad.at.x, pad.at.y + fontNm * 0.35, { sizeNm: fontNm, justify: "center", thicknessNm: fontNm / 6, color: "rgba(0,0,0,0.75)" });
  }
}

function drawShapeGeometry(ctx: CanvasRenderingContext2D, s: CmdShape, sketch = false) {
  ctx.beginPath();
  switch (s.kind) {
    case "segment":
      ctx.moveTo(s.start.x, s.start.y);
      ctx.lineTo(s.end.x, s.end.y);
      ctx.stroke();
      return;
    case "rect":
      ctx.rect(s.start.x, s.start.y, s.end.x - s.start.x, s.end.y - s.start.y);
      break;
    case "circle": {
      const r = Math.hypot(s.end.x - s.center.x, s.end.y - s.center.y);
      ctx.arc(s.center.x, s.center.y, r, 0, Math.PI * 2);
      break;
    }
    case "polygon":
      s.pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
      ctx.closePath();
      break;
    case "bezier":
      // `BEZIER_POLY::GetPoly` at the board's max error (an open curve is never filled).
      bezierPolyline([s.start.x, s.start.y], [s.c1.x, s.c1.y], [s.c2.x, s.c2.y], [s.end.x, s.end.y], BEZIER_MAX_ERROR_NM).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      return;
    case "arc": {
      const a = circleThrough([s.start.x, s.start.y], [s.mid.x, s.mid.y], [s.end.x, s.end.y]);
      if (!a) {
        ctx.moveTo(s.start.x, s.start.y);
        ctx.lineTo(s.end.x, s.end.y);
        ctx.stroke();
        return;
      }
      const [cx, cy, r] = a;
      const a0 = Math.atan2(s.start.y - cy, s.start.x - cx);
      const aMid = Math.atan2(s.mid.y - cy, s.mid.x - cx);
      const a1 = Math.atan2(s.end.y - cy, s.end.x - cx);
      ctx.arc(cx, cy, r, a0, a1, !normalizeSweep(a0, aMid, a1)); // see painter.ts: `normalizeSweep` is true for the INCREASING-angle sweep, `anticlockwise` is its negation
      ctx.stroke();
      return;
    }
  }
  // Sketch Graphic Items (`m_DisplayGraphicsFill` off): a filled shape is its outline only.
  if (s.filled && !sketch) ctx.fill();
  ctx.stroke();
}

function drawGraphic(ctx: CanvasRenderingContext2D, view: ViewTransform, s: CmdShape, selected: boolean, sketch = false, strokeType?: string) {
  ctx.save();
  const color = selected ? layerColor("selection") : layerColor(realLayerKey(s.layer));
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  // Sketch Graphic Items draws every graphic in line mode, a pixel wide (`m_DisplayGraphicsFill` off: `m_gal->SetLineWidth( m_pcbSettings.m_outlineWidth )`).
  ctx.lineWidth = sketch ? hairlineNm(view, selected ? 2 : 1) : Math.max(s.stroke_width, hairlineNm(view, selected ? 2 : 1));
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (isDashStyle(strokeType)) {
    // The footprint editor's `PCB_PAINTER` strokes a dashed graphic as the board does (`STROKE_PARAMS::Stroke`, kicad-port/strokeDash.ts) with the plotter's 12 and 3:
    // the editor's board has no Formatting page (UI-LEFT P5). A solid fill is filled without its outline.
    const shape = cmdShapeAsShape(s);
    if (s.filled && !sketch && (s.kind === "rect" || s.kind === "circle" || s.kind === "polygon")) {
      ctx.save();
      ctx.strokeStyle = "rgba(0,0,0,0)"; // the fill alone: `SetIsStroke( lineStyle == LINE_STYLE::SOLID .. )`
      drawShapeGeometry(ctx, s, false);
      ctx.restore();
    }
    const pieces = dashPieces(shape, { style: strokeType, width: s.stroke_width > 0 ? s.stroke_width : ctx.lineWidth }, BEZIER_MAX_ERROR_NM);
    ctx.beginPath();
    for (const [[ax, ay], [bx, by]] of pieces) {
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
  } else {
    drawShapeGeometry(ctx, s, sketch);
  }
  ctx.restore();
}

/** A library graphic (`CmdShape`, points as `{x, y}`) as the board's display `Shape` the dash cutter reads. */
function cmdShapeAsShape(s: CmdShape): Shape {
  const p = (q: { x: number; y: number }): [number, number] => [q.x, q.y];
  const base = { id: "", layer: s.layer, stroke_width: s.stroke_width, filled: s.filled };
  switch (s.kind) {
    case "segment":
      return { ...base, kind: "segment", start: p(s.start), end: p(s.end) };
    case "arc":
      return { ...base, kind: "arc", start: p(s.start), mid: p(s.mid), end: p(s.end) };
    case "rect":
      return { ...base, kind: "rect", start: p(s.start), end: p(s.end) };
    case "circle":
      return { ...base, kind: "circle", center: p(s.center), end: p(s.end) };
    case "polygon":
      return { ...base, kind: "polygon", pts: s.pts.map(p) };
    case "bezier":
      return { ...base, kind: "bezier", start: p(s.start), c1: p(s.c1), c2: p(s.c2), end: p(s.end) };
  }
}

/**
 * A text of the footprint (`PCB_TEXT` of a footprint) as `PCB_PAINTER::draw( PCB_TEXT )` draws it: `FONT::Draw` from its position with its looks (the kept bold, italic,
 * vertical justification, glyph width, knockout and keep-upright, UI-LEFT F5; the face it is set in), the board's layout (kicad-port/kicadText.ts, UI-LEFT-2 PP1).
 * Sketch Text Items (`m_DisplayTextFill` off): the text in line mode, a pixel wide. A `${REFERENCE}` shows as written: the editor's board is a footprint holder.
 */
function drawText(ctx: CanvasRenderingContext2D, view: ViewTransform, t: CmdText, selected: boolean, sketch = false, look?: TextLook, face: string | null = null) {
  const color = selected ? layerColor("selection") : layerColor(realLayerKey(t.layer));
  const size: [number, number] = [look?.width_nm ?? t.size_nm, t.size_nm];
  const bold = !!look?.bold;
  const spec: KicadTextSpec = {
    text: t.content,
    pos: [t.at.x, t.at.y],
    size,
    pen: effectiveTextPenWidth(t.stroke_width, size, bold),
    halign: t.justify,
    valign: look ? (look.valign < 0 ? "top" : look.valign > 0 ? "bottom" : "center") : "center",
    // `PCB_TEXT::GetDrawRotation`: kept upright, ]-90, 90]
    angleDeg: drawAngle(t.angle, look?.keep_upright ?? true) / 1000,
    mirror: t.mirror,
    italic: !!look?.italic,
    bold,
  };
  paintKicadText(ctx, spec, face, { color, pen: sketch ? hairlineNm(view, 1) : undefined, minPen: hairlineNm(view, 0.5), knockout: look?.knockout ? layerColor("background") : null, record: t.id ? { tag: "fp", id: t.id } : undefined });
}

/** Rubber-band preview for the graphics tool currently in progress (click-to-add-points, same convention the PCB tab's own `drawInProgress` uses -- including drawing it in the selection color regardless of target layer). */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: FpPaintDrawState | null, cursorNm: { x: number; y: number } | null) {
  if (!draw) return;
  // `drawArc` / `drawOneBezier`: the construction managers' own geometry, not a polyline through the clicks.
  const style = { color: layerColor("selection"), hair: (px: number) => hairlineNm(view, px), units: "mm" as const };
  if (draw.shapeKind === "arc" && draw.arc) {
    drawArcPreview(ctx, draw.arc, style);
    return;
  }
  if (draw.shapeKind === "bezier" && draw.bezier) {
    drawBezierPreview(ctx, draw.bezier, style, BEZIER_MAX_ERROR_NM);
    return;
  }
  const pts = draw.pts.slice();
  if (draw.leader && draw.leader.length > 0) pts.push(...draw.leader);
  else if (cursorNm) pts.push([cursorNm.x, cursorNm.y]);
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

export interface FpPaintOptions {
  selection: Set<string>;
  gridNm: number;
  gridVisible: boolean;
  /** The point the grid is anchored at (`common.Control.gridSetOrigin` in this editor): the dots follow it and its marker is drawn when it is not at (0, 0). */
  gridOrigin?: [number, number] | null;
  /** The current grid's Y size when it is not square. */
  gridYNm?: number;
  /** The Footprint Editor's Display Options > Grid Display. */
  gridDisplay?: GridDisplay;
  drawState: FpPaintDrawState | null;
  cursorNm: { x: number; y: number } | null;
  movePreview: { refs: string[]; dxNm: number; dyNm: number } | null;
  /** High Contrast Mode (`common.Control.highContrastMode`): what is not on `activeLayer` is drawn dimmed. */
  highContrast?: boolean;
  /** `HIGH_CONTRAST_MODE::HIDDEN` (Inactive layers "Hide"): with `highContrast`, what is not on `activeLayer` is not drawn. */
  contrastHidden?: boolean;
  activeLayer?: string;
  /** The Footprint Checker's markers (`PCB_MARKER`s of the editor's board): where, how severe, and whether the dialog has it selected. */
  markers?: { at: [number, number]; severity: string; selected: boolean }[];
  /** The display modes of the left toolbar: Sketch Pads, Sketch Graphic Items, Sketch Text Items. */
  sketchPads?: boolean;
  sketchGraphics?: boolean;
  sketchText?: boolean;
  /** The Appearance manager's switches (layers by name or state key, objects under `obj:<id>`) and opacities; absent = everything shown, opaque. */
  layerVisible?: Record<string, boolean>;
  layerOpacity?: Record<string, number>;
}

/** What High Contrast Mode leaves of an item off the active layer (the board painter's `layerAlpha` uses the same 0.25). */
const DIMMED_ALPHA = 0.25;

/**
 * `PCB_PAINTER`'s `m_contrastModeDisplay`: with High Contrast Mode on, an item not on the active layer is dimmed (DIMMED) or not drawn (HIDDEN). A pad is on the active
 * layer when one of its layers is it (`PAD::IsOnLayer`: `*.Cu` is every copper layer), which the Appearance manager's copper rows can make it.
 */
function contrastAlpha(opts: FpPaintOptions, layer: string | null): number {
  if (!opts.highContrast || !opts.activeLayer) return 1;
  return layer === opts.activeLayer ? 1 : opts.contrastHidden ? 0 : DIMMED_ALPHA;
}

/** The slice of `FpDrawState` the painter reads. */
export interface FpPaintDrawState {
  shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier";
  pts: [Nm, Nm][];
  /** A polygon's leader from its last corner to the cursor, bent by the line mode (`POLYGON_GEOM_MANAGER::m_leaderPts`); drawn in place of the straight rubber band. */
  leader?: [Nm, Nm][];
  arc?: ArcGeom;
  bezier?: BezierGeom;
}

export function paintFootprint(
  ctx: CanvasRenderingContext2D,
  view: ViewTransform,
  widthPx: number,
  heightPx: number,
  footprint: { pads: LibraryPad[]; graphics: CmdShape[]; texts: CmdText[]; fieldsToDraw?: FieldInfo[]; display?: FootprintItemsDisplay; kicad?: LibraryKeep } | null,
  opts: FpPaintOptions
) {
  // the Footprint Editor's axes are always on (`m_axesEnabled = true`), drawn whether the grid is shown or not, in `LAYER_GRID_AXES`
  {
    const d = opts.gridDisplay ?? DEFAULT_GRID_DISPLAY;
    paintEditorGrid(ctx, view, widthPx, heightPx, { size: { x: opts.gridNm, y: opts.gridYNm ?? opts.gridNm }, origin: opts.gridOrigin ?? [0, 0], style: d.style, lineWidthPx: d.lineWidthPx, minSpacingPx: d.minSpacingPx, visible: opts.gridVisible, axes: d.axes ?? true }, layerColor("grid"), undefined, "footprint", layerColor("grid_axes"));
  }
  drawGridOrigin(ctx, view, opts.gridOrigin, "#1a1a1a"); // the canvas is filled with this colour (FootprintCanvas.tsx)
  // what the texts of this paint are, for `window.__edaText.fp` (components/text/kicadTextPaint.ts)
  beginTextRecord("fp");
  if (!footprint) {
    drawAnchor(ctx, view);
    return;
  }

  const moved = (id: string | undefined) => (id && opts.movePreview?.refs.includes(id) ? opts.movePreview : null);
  const visible = opts.layerVisible ?? {};
  const opacity = (layer: string) => opts.layerOpacity?.[layer] ?? 1;
  const itemOpts = {
    selection: opts.selection,
    layerVisible: visible,
    layerOpacity: opts.layerOpacity ?? {},
    highContrast: !!opts.highContrast,
    contrastHidden: !!opts.contrastHidden,
    activeLayer: opts.activeLayer ?? "F.SilkS",
    sketchText: !!opts.sketchText,
    sketchGraphics: !!opts.sketchGraphics,
    movePreview: opts.movePreview,
  };

  // The zones, tables, dimensions, text boxes, points, barcodes and images of the footprint (kicad-port/fpItems.ts), under the graphics.
  if (footprint.display) paintFpItems(ctx, view, footprint.display, itemOpts);

  // Graphics first (silkscreen/fab/courtyard art), then pads on top, same
  // "copper reads above art" ordering the PCB tab's own painter uses.
  for (const g of footprint.graphics) {
    if (!layerIsVisible(visible, g.layer)) continue;
    const mv = moved(g.id);
    ctx.save();
    // `GetColor`'s per-type opacity: a filled shape takes Filled Shapes', a copper one Tracks' (kicad-port/fpAppearance.ts `fpShapeOpacity`)
    ctx.globalAlpha *= contrastAlpha(opts, g.layer) * opacity(g.layer) * fpShapeOpacity(g, opts.layerOpacity);
    if (mv) ctx.translate(mv.dxNm, mv.dyNm);
    // A graphic's `(stroke (type ..))` when it is not solid, kept by its id (`LibraryFootprintKeep::stroke_types`).
    const strokeTypes = footprint.kicad?.stroke_types as Record<string, string> | undefined;
    drawGraphic(ctx, view, g, opts.selection.has(g.id ?? ""), !!opts.sketchGraphics, g.id ? strokeTypes?.[g.id] : undefined);
    ctx.restore();
  }
  if (objectOn(visible, "footprint_text")) {
    for (const t of footprint.texts) {
      if (!layerIsVisible(visible, t.layer) || !fpTextShown(visible, t.content)) continue;
      // a text read hidden is not drawn (`PCB_TEXT::ViewGetLOD`: `!IsVisible()`, UI-LEFT F5)
      if (isHiddenText(footprint, t.id)) continue;
      const mv = moved(t.id);
      ctx.save();
      ctx.globalAlpha *= contrastAlpha(opts, t.layer) * opacity(t.layer);
      if (mv) ctx.translate(mv.dxNm, mv.dyNm);
      drawText(ctx, view, t, opts.selection.has(t.id ?? ""), !!opts.sketchText, t.id ? textLookOf(footprint, t.id) : undefined, textFaceOf(footprint, t.id));
      ctx.restore();
    }
  }
  // The Reference, the Value and the other fields (`PCB_FIELD`s of the footprint), by the board's field painter.
  if (footprint.fieldsToDraw) paintFpFields(ctx, view, footprint.fieldsToDraw, itemOpts);
  if (objectOn(visible, "pads")) {
    for (const p of footprint.pads) {
      // a pad shows while one of its layers does (`PAD::ViewGetLOD`); one that names none is on the front copper
      const layers = p.layers.length > 0 ? p.layers : ["F.Cu"];
      if (!fpPadShown(layers, visible)) continue;
      const mv = moved(p.id);
      ctx.save();
      ctx.globalAlpha *= contrastAlpha(opts, opts.activeLayer && fpPadOnLayer(layers, opts.activeLayer) ? opts.activeLayer : null) * (opts.layerOpacity?.["obj:pads"] ?? 1);
      if (mv) ctx.translate(mv.dxNm, mv.dyNm);
      drawPad(ctx, view, p, { selected: opts.selection.has(p.id ?? ""), sketch: !!opts.sketchPads });
      ctx.restore();
    }
  }

  drawAnchor(ctx, view);
  drawInProgress(ctx, view, opts.drawState, opts.cursorNm);
  if (opts.markers && opts.markers.length > 0) drawCheckMarkers(ctx, view, opts.markers);
}

const CHECK_MARKER_RADIUS_NM = 300_000;

/** The Footprint Checker's markers: a ring and a "!" in the colour of the severity (`LAYER_DRC_ERROR` / `LAYER_DRC_WARNING`), the selected one larger and highlighted (`LAYER_DRC_HIGHLIGHTED`) -- the board painter's stand-in for `MARKER_BASE`'s icon. */
function drawCheckMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, markers: readonly { at: [number, number]; severity: string; selected: boolean }[]) {
  const hair = hairlineNm(view, 1.5);
  for (const m of markers) {
    const [x, y] = m.at;
    const color = m.selected ? layerColor("LAYER_DRC_HIGHLIGHTED") : layerColor(m.severity === "exclusion" ? "LAYER_DRC_EXCLUSION" : m.severity === "error" ? "LAYER_DRC_ERROR" : "LAYER_DRC_WARNING");
    const r = m.selected ? CHECK_MARKER_RADIUS_NM * 1.4 : CHECK_MARKER_RADIUS_NM;
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(100_000, hair);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = 0.35;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.stroke();
    drawStrokeText(ctx, "!", x, y + r * 0.5, { sizeNm: r * 1.3, justify: "center", color, thicknessNm: r * 0.22 });
    ctx.restore();
  }
}
