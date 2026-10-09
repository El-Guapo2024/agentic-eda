// Canvas2D "GAL-like" schematic painter -- eeschema's look (KiCad Default
// theme colors from src/kicad/colors.json, which already covers eeschema's
// layers: LAYER_WIRE/LAYER_JUNCTION/LAYER_DEVICE/LAYER_PIN/etc., see
// layers.ts's layerColor(), which resolves a real KiCad key directly when
// it isn't one of this app's PCB "bucket" names) drawn from GET
// /api/schematic's structured data.
//
// Two symbol-body renderers coexist: `drawRealSymbol` (lib_symbols'
// real graphics -- rectangle/polyline/circle/arc/text -- plus real pin
// geometry: length, orientation, decoration shape, name/number
// placement, all ported from sch_painter.cpp/sch_pin.cpp/pin_layout_
// cache.cpp read this session) and `drawBoxSymbol` (the original v1
// generic-box-plus-heuristic-glyph renderer, from layout.ts's ported
// crates/engine geometry.rs box/port layout). `paintSchematic` picks
// `drawRealSymbol` whenever `libSymbol.ts`'s `resolveLibSymbol` finds
// real graphics for a symbol's `lib_id`, falling back to `drawBoxSymbol`
// otherwise -- per the task's own instruction, `drawBoxSymbol` (and
// layout.ts's box/port layout it depends on) is meant to be deleted
// outright once every symbol in practice resolves to real graphics; it
// is kept now only because that is not yet true for every board.
//
// Formulas and constants throughout are cited to the real KiCad source
// files/line ranges they were read from this session (not guessed) --
// see each constant's/function's own doc comment for the specific file.
// A few things this app's `/api/schematic` contract does not (yet) carry
// are called out explicitly as approximations where they come up: no
// per-symbol `pin_names` offset (so every pin name renders "inside" the
// body, using eeschema's own factory-default offset), no per-field
// position for a power symbol's net-name text (placed like a normal
// pin's name instead), no net-connectivity-derived "dangling" state (so
// the dangling-pin indicator circle is not drawn), and no electrical-pin
// -type annotation text (off by default in the real schematic editor
// too, so this is not actually a gap).
import type { BusEntry, ErcViolation, LabelShape, LabelScope, LibFill, NoConnect, PowerSymbol, Schematic, SchematicLabel, SchematicSymbol, SchematicText, SchematicWire, Sheet } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layerColor } from "../canvas/layers";
import { resolveSymbol, STUB, type ResolvedSymbol } from "./layout";
import { resolveLibSymbol, symbolBounds as libSymbolBounds, type ResolvedGraphic } from "./libSymbol";
import { resolvePin, symbolTransformMatrix, type ResolvedPin } from "./transform";
import { globalLabelOutline, globalLabelTextPlacement, hierLabelOutline, hierLabelTextPlacement, inferSpin, LABEL_TEXT_SIZE_UM, localLabelTextPlacement, type LabelSpin } from "./labelShape";
import { DEFAULT_PIN_TEXTS, PIN_TEXT_PEN_UM, PIN_TEXT_SIZE_UM, pinTextPlacements, type PinTexts } from "./pinText";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { defaultPenUm, FIELD_SIZE_UM, textOrigin, type SchField } from "../../kicad-port/schText";
import { ercMarkerPosition } from "./ercMarkerPosition";
import { junctionPoints } from "./junctions";
import { unitLetter } from "../../kicad-port/unitLetter";
import { DEFAULT_SCH_DISPLAY, ercSeverityShown, type SchDisplayOptions } from "./displayOptions";
import { bodyBoundsOf } from "./symbolMarkers";
import { drawSymbolMarkers } from "./symbolMarkersDraw";
import { drawSelectionBox, paintGraphics } from "./schGraphicsPainter";
import { allItems, itemBounds } from "./schItems";
import { fieldAnchors, isVerticalTwoPin, type FieldBox } from "../../kicad-port/schFields";

/**
 * Canvas2D's own `textBaseline: "middle"` centers on the *font's* actual
 * ascent/descent metrics; stroke text has no such metric to ask for
 * (every glyph's y=0 already sits exactly on the baseline, per KiCad's
 * own FONT_OFFSET convention -- see strokeFont.ts), so vertical
 * centering here is an approximation: half of a typical cap-height
 * (measured off the real font data -- 'A' spans baseline to about -0.95
 * * size), nudged down a bit to not over-shoot for mixed-/lower-case
 * strings. Applied by shifting the anchor y before calling
 * drawStrokeText, since the function itself only knows baseline-left.
 */
const MIDDLE_OFFSET_FACTOR = 0.35;
/** A full-height glyph's baseline-to-top span, measured off the real font data (decodeGlyph('A') tops out at y ~= -0.95 * size) -- used to approximate Canvas2D's old textBaseline:"top"/"bottom" behavior now that stroke text only ever anchors at its own baseline. */
const CAP_HEIGHT = 0.95;
/** Stroke text has no filled outline to embolden -- a thicker stroke is KiCad's own way of drawing "bold" stroke-font text. */
const BOLD_THICKNESS_FACTOR = 1 / 5;

export interface SchematicPaintOptions {
  selection: Set<string>;
  netHighlight: string | null;
  /** GET /api/erc's current report (kicad-cli's), or null when ERC has not run this session -- kept showing after the dialog closes, like KiCad's own markers until the next run. */
  ercViolations?: ErcViolation[] | null;
  /** The design has moved on since `ercViolations` were computed (kicad-port/checkRevision.ts): the markers are still drawn, dimmed and dashed, until the next ERC run. */
  ercStale?: boolean;
  /** `state.ercSelected` -- which `ercViolations` row the dialog's list currently has clicked/focused, drawn with the highlighted color instead of its own severity color (DRC markers' own `drcSelected` convention, mirrored here). */
  ercSelected?: number | null;
  /** GET /api/lint's schematic findings (crates/lint: our own readability checks) -- only while the ERC dialog is open, drawn as blue diamonds so they never read as KiCad's circles. */
  lintViolations?: ErcViolation[] | null;
  /** Index into `lintViolations` the dialog's Lint tab has clicked. */
  lintSelected?: number | null;
  /** The View menu's toggles (hidden pins, which ERC markers, simulation marks); KiCad's defaults when omitted. */
  display?: SchDisplayOptions;
}

const REF_FONT = 1.6;
const VALUE_FONT = 1.4;
const PIN_FONT = 1.1;
/** eeschema/default_values.h DEFAULT_JUNCTION_DIAM (36 mils) -- radius, um. A schematic can override this per-file (KiCad's own `(junction (diameter ...))`), which this app's backend doesn't expose yet, so this is the factory default only. */
const JUNCTION_RADIUS_UM = 457.2;
/** eeschema `default_line_thickness` (6 mil) -- um, the width of a graphic line with none of its own. */
const NOTES_LINE_UM = 152.4;
/** eeschema/default_values.h DEFAULT_NOCONNECT_SIZE (48 mils, the marker's full width) -- half-width, um, i.e. how far each arm of the top-level `no_connects[]` X reaches from center. Read directly from sch_painter.cpp: `d = max(m_size, 3*defaultPen)/2 = max(48,18)/2 = 24 mil`. */
const NOCONNECT_HALF_UM = 609.6;
/** Radius, um, of an ERC marker's circle -- not a KiCad constant (real KiCad's MARKER_BASE is a small fixed-pixel icon, screen-space-constant regardless of zoom; this canvas has no such primitive, so markers scale with the sheet like everything else here). Sized against this file's own JUNCTION_RADIUS_UM/NOCONNECT_HALF_UM just above rather than canvas/painter.ts's PCB-scale DRC_MARKER_RADIUS_UM (300) -- a schematic's own features already read larger at a normal working zoom, so a marker sized to match sits comfortably between the two. */
const ERC_MARKER_RADIUS_UM = 450;
/** A pin whose own `electrical_type` is `no_connect` draws a *smaller* X at its own tip -- eeschema's TARGET_PIN_RADIUS (15 mil), confirmed in sch_painter.cpp as this exact pin-level marker's half-size, distinct from the top-level no-connect marker's own (larger) size above. */
const PIN_NC_HALF_UM = 381;
const FIELD_FONT = 1.0; // footprint field: small, per the task's "footprint field in purple and small"

/**
 * eeschema's `PinSymbolSize` (sch_render_settings.cpp: `DEFAULT_TEXT_SIZE
 * * IU_PER_MILS / 2` = 25 mil = 0.635mm) -- the one size every pin-shape
 * decoration (inversion bubble radius, clock-triangle leg, non-logic "X"
 * half-size) is drawn at by default. sch_painter.cpp's `r`/`c` locals
 * both resolve to this same constant whenever PinSymbolSize is set (which
 * it always is, there being no project-level override this app exposes),
 * so this app does not need the name/number-size-derived fallback branch
 * real KiCad falls back to only when PinSymbolSize is explicitly zeroed.
 */
const PIN_DECOR_UM = 635;
/** 2x PIN_DECOR_UM -- how far past the body-attachment point (R) an inverted/inverted-clock pin's bubble-to-tip line, or a low-input/low-clock/low-output wedge's leg, reaches. */
const PIN_DECOR_D_UM = PIN_DECOR_UM * 2;
/** eeschema/default_values.h DEFAULT_PIN_NAME_OFFSET (20 mil) -- how far a pin's name sits past its body-attachment point, into the body ("inside" placement). This app's `LibSymbol` carries no per-symbol `(pin_names (offset ...))` override (not part of the coordinator's described contract), so every symbol uses KiCad's own factory default rather than varying per real library symbol -- real symbols that explicitly set `offset 0` (name drawn *outside*, past the pin's free end, as GND/power symbols typically do, though their pin is hidden anyway so it goes unseen) are the one case this simplification visibly diverges from. */
const PIN_NAME_OFFSET_UM = 508;

/** eeschema's `scope`-based label coloring (LAYER_LOCLABEL/LAYER_GLOBLABEL/LAYER_HIERLABEL) -- replaces an earlier heuristic (power/ground net-name sniffing) that only ever approximated "is this a global rail", now that the real scope is reported directly. */
function labelLayerColor(scope: LabelScope): string {
  switch (scope) {
    case "local":
      return layerColor("LAYER_LOCLABEL");
    case "global":
      return layerColor("LAYER_GLOBLABEL");
    case "hierarchical":
      return layerColor("LAYER_HIERLABEL");
  }
}

function localPortPoint(port: ResolvedSymbol["ports"][number], width: number, height: number): [number, number] {
  switch (port.side) {
    case "top":
      return [port.offset, 0];
    case "bottom":
      return [port.offset, height];
    case "left":
      return [0, port.offset];
    case "right":
      return [width, port.offset];
  }
}

function stubTip(port: ResolvedSymbol["ports"][number], lx: number, ly: number): [number, number] {
  switch (port.side) {
    case "top":
      return [lx, ly - STUB];
    case "bottom":
      return [lx, ly + STUB];
    case "left":
      return [lx - STUB, ly];
    case "right":
      return [lx + STUB, ly];
  }
}


// ------------------------------------------------------------- box fallback

function drawBoxSymbol(ctx: CanvasRenderingContext2D, view: ViewTransform, r: ResolvedSymbol, selected: boolean, unitSuffix: string) {
  const { symbol, width, height, ports, pinPort, passive } = r;
  ctx.save();
  ctx.translate(symbol.at[0], symbol.at[1]);
  if (symbol.rot !== 0) ctx.rotate((symbol.rot * Math.PI) / 180);
  // KiCad mirrors one axis at a time: "x" flips Y (vertical flip), "y"
  // flips X (horizontal flip) -- see types.ts's SchematicSymbol.mirror
  // doc comment. ctx.scale's own (x,y) factors are exactly that: (1,-1)
  // for a vertical flip, (-1,1) for a horizontal one.
  if (symbol.mirror === "x") ctx.scale(1, -1);
  else if (symbol.mirror === "y") ctx.scale(-1, 1);

  const hair = 1 / view.scale;
  ctx.lineWidth = Math.max(300, hair);

  if (passive) {
    ctx.strokeStyle = layerColor("LAYER_DEVICE");
    drawPassiveGlyph(ctx, passive, width, height);
  } else {
    ctx.fillStyle = layerColor("LAYER_DEVICE_BACKGROUND");
    ctx.strokeStyle = layerColor("LAYER_DEVICE");
    ctx.lineWidth = Math.max(400, hair);
    ctx.fillRect(0, 0, width, height);
    ctx.strokeRect(0, 0, width, height);
  }

  if (selected) {
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
    ctx.lineWidth = Math.max(800, hair * 3);
    ctx.strokeRect(-200, -200, width + 400, height + 400);
    ctx.restore();
  }

  // Pins.
  ctx.lineWidth = Math.max(150, hair);
  ctx.strokeStyle = layerColor("LAYER_PIN");
  ports.forEach((port, portIdx) => {
    const [lx, ly] = localPortPoint(port, width, height);
    const [sx, sy] = stubTip(port, lx, ly);
    const pinI = pinPort.indexOf(portIdx);
    const pin = pinI >= 0 ? symbol.pins[pinI] : undefined;
    ctx.beginPath();
    ctx.moveTo(lx, ly);
    ctx.lineTo(sx, sy);
    ctx.stroke();
    if (pin) {
      const pad = 200;
      // Pin name: just inside the body edge (KiCad shows this and the
      // number simultaneously by default -- there's no "hide names"
      // toggle in this app's model -- each in its own real KiCad color,
      // LAYER_PINNAM/LAYER_PINNUM, not one falling back to the other).
      if (pin.name) {
        const sizeUm = PIN_FONT * 1000;
        const color = layerColor("LAYER_PINNAM");
        if (port.side === "left") {
          drawStrokeText(ctx, pin.name, lx + pad, ly + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "left", color });
        } else if (port.side === "right") {
          drawStrokeText(ctx, pin.name, lx - pad, ly + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "right", color });
        } else {
          // top/bottom: text sits fully above (top-side pin) or fully
          // below (bottom-side pin) the stub point. Baseline sits close
          // to a glyph's visual bottom (pin names rarely have
          // descenders), so anchoring the baseline directly at the
          // pad point approximates "text bottom here, growing up" for a
          // top-side pin; for a bottom-side pin the baseline is pushed
          // down by one cap-height (CAP_HEIGHT) so the text's *top*
          // lands at the pad point instead.
          const y = port.side === "top" ? ly - pad : ly + pad + sizeUm * CAP_HEIGHT;
          drawStrokeText(ctx, pin.name, lx, y, { sizeUm, justify: "center", color });
        }
      }
      // Pin number: along the stub itself, at its midpoint, offset to
      // sit just above the line (below it for a top-side stub, which
      // points the opposite way) rather than on top of it.
      if (pin.number) {
        const sizeUm = PIN_FONT * 0.85 * 1000;
        const color = layerColor("LAYER_PINNUM");
        const mx = (lx + sx) / 2;
        const my = (ly + sy) / 2;
        if (port.side === "left" || port.side === "right") {
          // Sits just above the stub line (bottom-anchored -- see CAP_HEIGHT's comment).
          drawStrokeText(ctx, pin.number, mx, my - pad * 0.5, { sizeUm, justify: "center", color });
        } else {
          const justify = port.side === "top" ? "right" : "left";
          drawStrokeText(ctx, pin.number, mx + (port.side === "top" ? -pad * 0.5 : pad * 0.5), my + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify, color });
        }
      }
      // No-connect: a small blue X centered on the stub's free end --
      // LAYER_NOCONNECT, KiCad's real color for this marker.
      if (pin.kind === "nc") {
        ctx.save();
        ctx.strokeStyle = layerColor("LAYER_NOCONNECT");
        ctx.lineWidth = Math.max(150, hair);
        const s = PIN_NC_HALF_UM;
        ctx.beginPath();
        ctx.moveTo(sx - s, sy - s);
        ctx.lineTo(sx + s, sy + s);
        ctx.moveTo(sx - s, sy + s);
        ctx.lineTo(sx + s, sy - s);
        ctx.stroke();
        ctx.restore();
      }
    }
  });

  if (!symbol.fields?.length) drawFieldsAbout(ctx, symbol, { minX: 0, minY: 0, maxX: width, maxY: height }, false, unitSuffix);
  ctx.restore();
}

function drawPassiveGlyph(ctx: CanvasRenderingContext2D, kind: NonNullable<ResolvedSymbol["passive"]>, width: number, height: number) {
  const cy = height / 2;
  const lead = width * 0.32;
  const far = width - lead;
  ctx.beginPath();
  switch (kind) {
    case "resistor": {
      const bodyHalf = height * 0.18;
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      ctx.strokeRect(lead, cy - bodyHalf, far - lead, bodyHalf * 2);
      return;
    }
    case "capacitor": {
      const gap = width * 0.06;
      const plateHalf = height * 0.28;
      ctx.moveTo(0, cy);
      ctx.lineTo(width / 2 - gap, cy);
      ctx.moveTo(width / 2 - gap, cy - plateHalf);
      ctx.lineTo(width / 2 - gap, cy + plateHalf);
      ctx.moveTo(width / 2 + gap, cy - plateHalf);
      ctx.lineTo(width / 2 + gap, cy + plateHalf);
      ctx.moveTo(width / 2 + gap, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      return;
    }
    case "inductor": {
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      const bumpR = (far - lead) / 6;
      for (let i = 0; i < 3; i++) {
        const c = lead + bumpR * (2 * i + 1);
        ctx.moveTo(c - bumpR, cy);
        ctx.arc(c, cy, bumpR, Math.PI, 0, false);
      }
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      return;
    }
    case "diode": {
      const triHalf = height * 0.22;
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(lead, cy - triHalf);
      ctx.lineTo(lead, cy + triHalf);
      ctx.lineTo(far, cy);
      ctx.closePath();
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(far, cy - triHalf);
      ctx.lineTo(far, cy + triHalf);
      ctx.stroke();
      return;
    }
  }
}

/**
 * One line of text where KiCad puts it: `anchor` and the justification in the text's own axes, a vertical text turned a quarter
 * counter-clockwise about the anchor (`kicad-port/schText.ts`, `FONT::Draw`'s placement).
 */
function drawKicadText(ctx: CanvasRenderingContext2D, text: string, anchor: [number, number], o: { sizeUm: number; h: "left" | "center" | "right"; v: "top" | "center" | "bottom"; vertical: boolean; color: string; thicknessUm?: number }) {
  if (!text) return;
  const thickness = o.thicknessUm ?? defaultPenUm(o.sizeUm);
  const [dx, dy] = textOrigin(measureStrokeText(text, o.sizeUm), o.sizeUm, thickness, o.h, o.v);
  ctx.save();
  ctx.translate(anchor[0], anchor[1]);
  if (o.vertical) ctx.rotate(-Math.PI / 2);
  drawStrokeText(ctx, text, dx, dy, { sizeUm: o.sizeUm, thicknessUm: thickness, justify: "left", color: o.color });
  ctx.restore();
}

/** The colour KiCad draws a field of that name in. */
function fieldColor(name: string): string {
  switch (name) {
    case "Reference":
      return layerColor("LAYER_REFERENCEPART");
    case "Value":
      return layerColor("LAYER_VALUEPART");
    case "Sheetname":
      return layerColor("LAYER_SHEETNAME");
    case "Sheetfile":
      return layerColor("LAYER_SHEETFILENAME");
    default:
      return layerColor("LAYER_FIELDS");
  }
}

/** The fields of one item where the server says they are (`SchField`): every visible one, a Reference with its unit letter, in the item's own field colours (or `color`, when it is drawn selected). */
function drawSchFields(ctx: CanvasRenderingContext2D, fields: SchField[], unitSuffix = "", color?: string) {
  for (const f of fields) {
    if (!f.visible || !f.text) continue;
    drawKicadText(ctx, f.name === "Reference" ? f.text + unitSuffix : f.text, f.at, { sizeUm: FIELD_SIZE_UM, h: f.h, v: f.v, vertical: f.vertical, color: color ?? fieldColor(f.name) });
  }
}

/**
 * Reference/value/footprint text, shared by both the box and real-symbol
 * renderers. Real KiCad places each of these at its own per-instance
 * field position (not reported by this app's `/api/schematic`), so both
 * callers use the same "above/below the symbol's own bounding box, at
 * localX" approximation instead -- consistent between the two renderers
 * even though neither is pixel-exact against a real recorded field
 * position.
 */
function drawFieldsAbout(ctx: CanvasRenderingContext2D, symbol: SchematicSymbol, bbox: FieldBox, verticalTwoPin: boolean, unitSuffix: string) {
  // kicad-port/schFields.ts: beside the body for a resistor/capacitor standing on its end (a wire leaves both ends, straight through
  // "above and below"), above and below and centred for everything else.
  const at = fieldAnchors(bbox, verticalTwoPin);
  const refSizeUm = REF_FONT * 1000;
  drawStrokeText(ctx, symbol.id + unitSuffix, at.ref[0], at.ref[1], { sizeUm: refSizeUm, thicknessUm: refSizeUm * BOLD_THICKNESS_FACTOR, justify: at.justify, color: layerColor("LAYER_REFERENCEPART") });
  if (symbol.value || symbol.mpn) {
    const sizeUm = VALUE_FONT * 1000;
    drawStrokeText(ctx, symbol.value ?? symbol.mpn ?? "", at.value[0], at.value[1], { sizeUm, justify: at.justify, color: layerColor("LAYER_VALUEPART") });
  }
  // Footprint field: small, LAYER_FIELDS purple -- a real field KiCad
  // draws alongside ref/value (this app has no separate "hide field"
  // flag per field, so it always shows when the symbol has a package).
  const pkg = symbol.footprint ?? symbol.package;
  if (pkg) {
    const sizeUm = FIELD_FONT * 1000;
    drawStrokeText(ctx, pkg, at.footprint[0], at.footprint[1], { sizeUm, justify: at.justify, color: layerColor("LAYER_FIELDS") });
  }
}

// ------------------------------------------------------------- real symbols

/** eeschema's FILL_T -> how to paint it. `color`/the hatch modes fall back to `outline` (fill with the stroke's own color) -- see types.ts's LibFill doc comment. */
function fillPaint(fill: LibFill): "none" | "outline" | "background" {
  if (fill === "background") return "background";
  if (fill === "none") return "none";
  return "outline"; // "outline", "color", and every hatch mode
}

/** sch_shape.cpp's stroke-width resolution: an explicit positive width wins (clamped to a sane minimum); zero means "use the schematic's own default line thickness"; negative explicitly means "no stroke" (rare -- a hatch-only shape). */
function effectiveStrokeWidthUm(strokeWidthUm: number): number | null {
  if (strokeWidthUm > 0) return Math.max(strokeWidthUm, 21.2);
  if (strokeWidthUm === 0) return 152.4; // DEFAULT_LINE_WIDTH_MILS, 6 mil
  return null;
}

/** The circumcenter of 3 points -- CalcArcCenter, standard circumcenter algebra (not KiCad-specific; used because KiCad's own arcs are stored start/mid/end, like this app's PCB `Shape` arcs). Returns null for (near-)collinear points, which a real arc should never be. */
function arcCenter(a: [number, number], b: [number, number], c: [number, number]): [number, number] | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-6) return null;
  const a2 = a[0] * a[0] + a[1] * a[1];
  const b2 = b[0] * b[0] + b[1] * b[1];
  const c2 = c[0] * c[0] + c[1] * c[1];
  const ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
  const uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
  return [ux, uy];
}

/** Normalizes an angle difference to (-180, 180] degrees, in radians -- sch_shape.cpp's `norm180`, used to build an arc's signed sweep from its three stored points. */
function norm180(rad: number): number {
  let d = rad % (2 * Math.PI);
  if (d <= -Math.PI) d += 2 * Math.PI;
  if (d > Math.PI) d -= 2 * Math.PI;
  return d;
}

function applyFill(ctx: CanvasRenderingContext2D, mode: "none" | "outline" | "background", strokeColor: string) {
  if (mode === "background") ctx.fillStyle = layerColor("LAYER_DEVICE_BACKGROUND");
  else if (mode === "outline") ctx.fillStyle = strokeColor;
}

/** Exported for `components/symbol/symbolPainter.ts` (the Symbol Editor tab): the exact same resolved-graphic drawing logic applies there -- a library symbol's own graphics are this same `ResolvedGraphic` shape, just resolved with an identity transform/no instance offset instead of a placed instance's. */
export function drawRealGraphic(ctx: CanvasRenderingContext2D, g: ResolvedGraphic, strokeColor: string) {
  if (g.kind === "text") {
    drawStrokeText(ctx, g.content, g.at[0], g.at[1], { sizeUm: g.sizeUm, angleRad: -(g.angleDeg * Math.PI) / 180, justify: "center", color: strokeColor });
    return;
  }

  const mode = fillPaint(g.fill);
  const widthUm = effectiveStrokeWidthUm(g.strokeWidthUm);
  applyFill(ctx, mode, strokeColor);

  ctx.beginPath();
  switch (g.kind) {
    case "rectangle": {
      const [x0, y0] = g.start;
      const [x1, y1] = g.end;
      ctx.rect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
      break;
    }
    case "polyline":
      g.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      break;
    case "circle":
      ctx.arc(g.center[0], g.center[1], g.radiusUm, 0, Math.PI * 2);
      break;
    case "arc": {
      const center = arcCenter(g.start, g.mid, g.end);
      if (!center) {
        ctx.moveTo(g.start[0], g.start[1]);
        ctx.lineTo(g.end[0], g.end[1]);
        break;
      }
      const r = Math.hypot(g.start[0] - center[0], g.start[1] - center[1]);
      const a0 = Math.atan2(g.start[1] - center[1], g.start[0] - center[0]);
      const aMid = Math.atan2(g.mid[1] - center[1], g.mid[0] - center[0]);
      const aEnd = Math.atan2(g.end[1] - center[1], g.end[0] - center[0]);
      const sweep = norm180(aMid - a0) + norm180(aEnd - aMid);
      ctx.arc(center[0], center[1], r, a0, a0 + sweep, sweep < 0);
      break;
    }
  }
  if (g.kind === "polyline") {
    // A library polyline's fill is the implicitly-closed polygon; its
    // stroke is the open path as given (sch_painter.cpp: fill closes,
    // stroke does not) -- fill before closing the path for the stroke.
    if (mode !== "none") ctx.fill();
    if (widthUm !== null) {
      ctx.lineWidth = widthUm;
      ctx.stroke();
    }
    return;
  }
  if (g.kind === "rectangle" || g.kind === "circle" || g.kind === "arc") ctx.closePath();
  if (mode !== "none" && g.kind !== "arc") ctx.fill();
  if (widthUm !== null) {
    ctx.lineWidth = widthUm;
    ctx.stroke();
  }
}

/** `dir`'s horizontal/vertical classification is exact (every real rotation here is axis-aligned -- see transform.ts's ResolvedPin doc comment), so this is a plain equality test, not a tolerance check. */
function isHorizontalPin(dir: [number, number]): boolean {
  return dir[1] === 0;
}

/** The pin-shape decoration geometry -- sch_painter.cpp's `draw(SCH_PIN)`, read directly (formulas for every GRAPHIC_PINSHAPE value, plus the `no_connect` electrical-type override). Operates entirely on `root`/`tip`/`dir`, all already resolved to world space. Exported for the Symbol Editor's own canvas (`components/symbol/symbolPainter.ts`) -- same reasoning as `drawRealGraphic`'s own doc. */
export function drawPinDecoration(ctx: CanvasRenderingContext2D, rp: ResolvedPin) {
  const { pin, tip: P, root: R, dir } = rp;
  const [dx, dy] = dir;
  const r = PIN_DECOR_UM;
  const d = PIN_DECOR_D_UM;
  const line = (a: [number, number], b: [number, number]) => {
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
  };
  /** Two connected segments a->b->c -- sch_painter.cpp's own `triLine` helper, by the same name, used for every pin-shape decoration below. */
  const triLine = (a: [number, number], b: [number, number], c: [number, number]) => {
    line(a, b);
    line(b, c);
  };
  const circle = (cx: number, cy: number, rad: number) => {
    ctx.moveTo(cx + rad, cy);
    ctx.arc(cx, cy, rad, 0, Math.PI * 2);
  };
  /** The INVERTED_CLOCK triangle alone (shared by "inverted_clock" and the "clock_low"/"edge_clock_high" compound shapes). */
  const invertedClockTriangle = () => triLine([R[0] + dy * r, R[1] - dx * r], [R[0] - dx * r, R[1] - dy * r], [R[0] - dy * r, R[1] + dx * r]);
  /** The INPUT_LOW wedge alone (shared by "input_low" and "clock_low"/"edge_clock_high"). */
  const inputLowWedge = () => {
    if (dy === 0) triLine([R[0] + dx * d, R[1]], [R[0] + dx * d, R[1] - d], R);
    else triLine([R[0], R[1] + dy * d], [R[0] - d, R[1] + dy * d], R);
  };

  if (pin.electrical_type === "no_connect") {
    const t = PIN_NC_HALF_UM;
    line(R, P);
    line([P[0] - t, P[1] - t], [P[0] + t, P[1] + t]);
    line([P[0] + t, P[1] - t], [P[0] - t, P[1] + t]);
    return;
  }

  switch (pin.shape) {
    case "line":
      line(R, P);
      return;
    case "inverted":
      circle(R[0] + dx * r, R[1] + dy * r, r);
      line([R[0] + dx * d, R[1] + dy * d], P);
      return;
    case "clock":
      line(R, P);
      if (dy === 0) triLine([R[0], R[1] + r], [R[0] - dx * r, R[1]], [R[0], R[1] - r]);
      else triLine([R[0] + r, R[1]], [R[0], R[1] - dy * r], [R[0] - r, R[1]]);
      return;
    case "inverted_clock":
      invertedClockTriangle();
      circle(R[0] + dx * r, R[1] + dy * r, r);
      line([R[0] + dx * d, R[1] + dy * d], P);
      return;
    case "input_low":
      line(R, P);
      inputLowWedge();
      return;
    case "clock_low":
    case "edge_clock_high":
      line(R, P);
      invertedClockTriangle();
      inputLowWedge();
      return;
    case "output_low":
      line(R, P);
      if (dy === 0) line([R[0], R[1] - d], [R[0] + dx * d, R[1]]);
      else line([R[0] - d, R[1]], [R[0], R[1] + dy * d]);
      return;
    case "non_logic":
      line(R, P);
      line([R[0] - (dx + dy) * r, R[1] - (dy - dx) * r], [R[0] + (dx + dy) * r, R[1] + (dy - dx) * r]);
      line([R[0] - (dx - dy) * r, R[1] - (dx + dy) * r], [R[0] + (dx - dy) * r, R[1] + (dx + dy) * r]);
      return;
  }
}

/**
 * Pin name/number text where KiCad writes it (`PIN_LAYOUT_CACHE::GetPinNameInfo`/`GetPinNumberInfo`, `pinText.ts`): the symbol says whether its
 * names are inside the body (an offset above zero) or over the pins, and whether names and numbers are shown at all; both are set in 50
 * mils, and a no-connect pin's are written like any other's. `colors` overrides the two label colours (the symbol editor draws a hidden pin's
 * labels in the hidden colour, `getColorForLayer`).
 */
export function drawPinText(ctx: CanvasRenderingContext2D, rp: ResolvedPin, colors?: { name: string; number: string }, texts: PinTexts = DEFAULT_PIN_TEXTS) {
  const { pin, tip, root } = rp;
  const placed = pinTextPlacements(pin, tip, root, texts);
  if (placed.name) {
    drawKicadText(ctx, placed.name.text, placed.name.at, { sizeUm: PIN_TEXT_SIZE_UM, h: placed.name.h, v: placed.name.v, vertical: placed.name.vertical, color: colors?.name ?? layerColor("LAYER_PINNAM"), thicknessUm: PIN_TEXT_PEN_UM });
  }
  if (placed.number) {
    drawKicadText(ctx, placed.number.text, placed.number.at, { sizeUm: PIN_TEXT_SIZE_UM, h: placed.number.h, v: placed.number.v, vertical: placed.number.vertical, color: colors?.number ?? layerColor("LAYER_PINNUM"), thicknessUm: PIN_TEXT_PEN_UM });
  }
}

function drawPins(ctx: CanvasRenderingContext2D, view: ViewTransform, pins: ResolvedPin[], showHiddenPins = false, texts: PinTexts = DEFAULT_PIN_TEXTS) {
  const hair = 1 / view.scale;
  ctx.strokeStyle = layerColor("LAYER_PIN");
  ctx.lineWidth = Math.max(PIN_TEXT_PEN_UM, hair);
  for (const rp of pins) {
    // `SCH_PAINTER::draw( SCH_PIN )`: a hidden pin is drawn only while "Show Hidden Pins" is on, and then in the hidden-items colour (`LAYER_HIDDEN`).
    if (rp.pin.hidden && !showHiddenPins) continue;
    const hidden = rp.pin.hidden;
    ctx.strokeStyle = hidden ? layerColor("LAYER_HIDDEN") : layerColor("LAYER_PIN");
    ctx.beginPath();
    drawPinDecoration(ctx, rp);
    ctx.stroke();
    drawPinText(ctx, rp, hidden ? { name: layerColor("LAYER_HIDDEN"), number: layerColor("LAYER_HIDDEN") } : undefined, texts);
  }
}

function drawRealSymbol(ctx: CanvasRenderingContext2D, view: ViewTransform, instance: SchematicSymbol, graphics: ResolvedGraphic[], pins: ResolvedPin[], bbox: { minX: number; minY: number; maxX: number; maxY: number }, selected: boolean, unitSuffix: string, showHiddenPins = false, texts: PinTexts = DEFAULT_PIN_TEXTS) {
  const hair = 1 / view.scale;
  const strokeColor = layerColor("LAYER_DEVICE");
  ctx.strokeStyle = strokeColor;
  for (const g of graphics) drawRealGraphic(ctx, g, strokeColor);

  if (selected) {
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
    ctx.lineWidth = Math.max(800, hair * 3);
    ctx.strokeRect(bbox.minX - 400, bbox.minY - 400, bbox.maxX - bbox.minX + 800, bbox.maxY - bbox.minY + 800);
    ctx.restore();
  }

  drawPins(ctx, view, pins, showHiddenPins, texts);
  if (!instance.fields?.length) drawFieldsAbout(ctx, instance, bbox, isVerticalTwoPin(pins), unitSuffix);
}

/**
 * A power symbol's net-name text -- really KiCad's Value *field*, drawn
 * at its own per-instance position in a real schematic; this app has no
 * such position (see PowerSymbol's doc comment), so it's placed using
 * the exact same "inside the body, past the pin's root" formula a normal
 * pin's name would use, keyed off the symbol's own (almost always
 * hidden, often zero-length) pin. A zero-length pin has no direction
 * (`dir` is `(0,0)` -- see transform.ts's `resolvePin`), so that case
 * falls back to a fixed offset to the right of the symbol's anchor, a
 * reasonable default matching where GND/power text conventionally sits.
 */
function drawPowerSymbolText(ctx: CanvasRenderingContext2D, ps: PowerSymbol, resolvedPin: ResolvedPin | null, on: boolean) {
  if (ps.fields?.length) {
    drawSchFields(ctx, ps.fields, "", on ? layerColor("LAYER_SELECTION_SHADOWS") : undefined);
    return;
  }
  const sizeUm = VALUE_FONT * 1000;
  const color = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_VALUEPART");
  if (resolvedPin && (resolvedPin.dir[0] !== 0 || resolvedPin.dir[1] !== 0)) {
    const { root: R, dir } = resolvedPin;
    const anchor: [number, number] = [R[0] + dir[0] * PIN_NAME_OFFSET_UM, R[1] + dir[1] * PIN_NAME_OFFSET_UM];
    if (isHorizontalPin(dir)) {
      drawStrokeText(ctx, ps.net, anchor[0], anchor[1] + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: dir[0] >= 0 ? "left" : "right", color });
    } else {
      drawStrokeText(ctx, ps.net, anchor[0], anchor[1], { sizeUm, angleRad: -Math.PI / 2, justify: dir[1] < 0 ? "left" : "right", color });
    }
  } else {
    drawStrokeText(ctx, ps.net, ps.at[0] + PIN_NAME_OFFSET_UM, ps.at[1] + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "left", color });
  }
}

function drawPowerSymbol(ctx: CanvasRenderingContext2D, view: ViewTransform, ps: PowerSymbol, graphics: ResolvedGraphic[] | null, resolvedPin: ResolvedPin | null, on: boolean) {
  const strokeColor = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_DEVICE");
  ctx.strokeStyle = strokeColor;
  if (graphics) {
    for (const g of graphics) drawRealGraphic(ctx, g, strokeColor);
  } else {
    // No real graphics resolved for this power symbol's lib_id -- same
    // "net name only, no body" fallback the generic net-label dot used
    // to be for every label (see drawLabel): still legible, not a blank
    // gap on the sheet.
    ctx.save();
    ctx.fillStyle = strokeColor;
    ctx.beginPath();
    ctx.arc(ps.at[0], ps.at[1], Math.max(150, 1 / view.scale), 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
  if (resolvedPin && !resolvedPin.pin.hidden) {
    ctx.strokeStyle = layerColor("LAYER_PIN");
    ctx.lineWidth = Math.max(PIN_TEXT_PEN_UM, 1 / view.scale);
    ctx.beginPath();
    drawPinDecoration(ctx, resolvedPin);
    ctx.stroke();
  }
  drawPowerSymbolText(ctx, ps, resolvedPin, on);
}

// ------------------------------------------------------------- labels / no-connects

function drawLabel(ctx: CanvasRenderingContext2D, view: ViewTransform, l: SchematicLabel, wires: SchematicWire[], on: boolean) {
  const color = on ? layerColor("LAYER_SELECTION_SHADOWS") : labelLayerColor(l.scope);
  const hair = 1 / view.scale;

  if (l.scope === "local" || !l.shape) {
    const spin = inferSpin(wires, l.at);
    const { pos, justify } = localLabelTextPlacement(spin, l.at);
    const sizeUm = LABEL_TEXT_SIZE_UM;
    // V BOTTOM: stroke text has no native top/bottom baseline (see this
    // file's CAP_HEIGHT comment) -- a local label's text sits just above
    // its anchor, so the baseline itself (no cap-height push needed,
    // unlike the box-renderer's generic top/bottom approximation) lands
    // close enough.
    drawStrokeText(ctx, l.net, pos[0], pos[1], { sizeUm, justify, color });
    return;
  }

  const spin = inferSpin(wires, l.at);
  const shape: LabelShape = l.shape;
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(l.scope === "global" ? 159 : 159, hair);
  ctx.beginPath();
  const outline = l.scope === "global" ? globalLabelOutline(l.net, shape, spin, l.at) : hierLabelOutline(shape, spin, l.at);
  outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.stroke();

  const { pos, justify } = l.scope === "global" ? globalLabelTextPlacement(shape, spin, l.at) : hierLabelTextPlacement(l.net, spin, l.at);
  const sizeUm = LABEL_TEXT_SIZE_UM;
  if (spin === "up" || spin === "bottom") {
    drawStrokeText(ctx, l.net, pos[0], pos[1], { sizeUm, angleRad: -Math.PI / 2, justify, color });
  } else {
    drawStrokeText(ctx, l.net, pos[0], pos[1] + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify, color });
  }
}

/** `T`: free-standing text -- `SCH_TEXT`'s own render, drawn with the Newstroke font like every other schematic text (labels, pin names, fields). `t.angle` is already world-space (like `SymbolInstance.rot`, not a library-local `LibGraphic.angle_deg`), so it feeds `ctx.rotate`/`angleRad` directly, no Y-flip negation -- see `drawRealSymbol`'s own `ctx.rotate((symbol.rot * Math.PI) / 180)` for the parallel case this mirrors. */
function drawSchText(ctx: CanvasRenderingContext2D, t: SchematicText, on: boolean) {
  drawStrokeText(ctx, t.content, t.at[0], t.at[1], { sizeUm: t.size_um, angleRad: (t.angle * Math.PI) / 180, justify: "left", color: on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_NOTES") });
}

/**
 * A child hierarchical sheet (GAPS.md #6) -- `SCH_SHEET`'s own on-canvas
 * look: a plain rectangle (`LAYER_SHEET` border, transparent fill, same as
 * real eeschema's default sheet color scheme), its name above the
 * top-left corner (`LAYER_SHEETNAME`) and filename below the bottom-left
 * corner (`LAYER_SHEETFILENAME`), and each of its own pins as a short stub
 * on the border with its name (`LAYER_SHEETLABEL`) -- not full
 * `SCH_SHEET_PIN` arrow glyphs (shape-specific triangle/chevron outlines,
 * `labelShape.ts`'s own `hierLabelOutline`), since this pass is about
 * making the hierarchy visible and navigable at all (previously nothing
 * drew here, the view simply had no sheets to show) rather than full
 * pixel-parity with source's own pin glyphs.
 */
function drawSheet(ctx: CanvasRenderingContext2D, view: ViewTransform, s: Sheet, on: boolean) {
  const hair = 1 / view.scale;
  const [x, y] = s.at;
  const [w, h] = s.size;
  ctx.save();
  ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_SHEET");
  ctx.lineWidth = Math.max(152.4, hair);
  ctx.strokeRect(x, y, w, h);
  ctx.restore();

  const nameSizeUm = 1270;
  if (s.fields?.length) {
    drawSchFields(ctx, s.fields);
  } else {
    drawStrokeText(ctx, s.name, x, y - 400, { sizeUm: nameSizeUm, justify: "left", color: layerColor("LAYER_SHEETNAME") });
    drawStrokeText(ctx, s.file, x, y + h + 400 + nameSizeUm * 0.8, { sizeUm: nameSizeUm * 0.8, justify: "left", color: layerColor("LAYER_SHEETFILENAME") });
  }

  // A sheet pin sits on the border it names (`SCH_SHEET_PIN::SetSide`): its flag points into the sheet from that edge and its name is
  // written inside, after the flag -- so a wire reaches the pin from outside and the name never runs over it.
  const pinColor = layerColor("LAYER_SHEETLABEL");
  ctx.save();
  ctx.strokeStyle = pinColor;
  ctx.lineWidth = Math.max(159, hair);
  for (const p of s.pins) {
    const [px, py] = p.at;
    const edge = px <= x ? "left" : px >= x + w ? "right" : py <= y ? "top" : "bottom";
    const spin: LabelSpin = edge === "left" ? "right" : edge === "right" ? "left" : edge === "top" ? "bottom" : "up";
    ctx.beginPath();
    hierLabelOutline(p.shape, spin, [px, py]).forEach(([ox, oy], i) => (i === 0 ? ctx.moveTo(ox, oy) : ctx.lineTo(ox, oy)));
    ctx.stroke();
    const gap = 1_270 + 500; // past the flag
    const sizeUm = 1000;
    if (edge === "left") drawStrokeText(ctx, p.name, px + gap, py + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "left", color: pinColor });
    else if (edge === "right") drawStrokeText(ctx, p.name, px - gap, py + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "right", color: pinColor });
    else drawStrokeText(ctx, p.name, px + 600, edge === "top" ? py + gap : py - gap, { sizeUm, justify: "left", color: pinColor });
  }
  ctx.restore();
}

function drawNoConnect(ctx: CanvasRenderingContext2D, view: ViewTransform, nc: NoConnect, on: boolean) {
  const hair = 1 / view.scale;
  ctx.save();
  ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_NOCONNECT");
  ctx.lineWidth = Math.max(152.4, hair);
  const [x, y] = nc.at;
  const s = NOCONNECT_HALF_UM;
  ctx.beginPath();
  ctx.moveTo(x - s, y - s);
  ctx.lineTo(x + s, y + s);
  ctx.moveTo(x + s, y - s);
  ctx.lineTo(x - s, y + s);
  ctx.stroke();
  ctx.restore();
}

/**
 * A bus entry (`SCH_BUS_WIRE_ENTRY`, GAPS.md #20): a single diagonal stub
 * from `at` to `at + size` -- real KiCad gives it no outline/fill
 * distinction of its own, just a `LAYER_BUS`-colored line the same width
 * as a bus wire, which of `at`/`at + size` is "the bus side" is purely
 * geometric (whichever end lands on a bus wire's own point -- see
 * `crates/kicad/src/bus.rs`'s own doc) and irrelevant to drawing it: both
 * ends are simply connected by one straight segment.
 */
function drawBusEntry(ctx: CanvasRenderingContext2D, view: ViewTransform, be: BusEntry, on: boolean) {
  const hair = 1 / view.scale;
  const [x, y] = be.at;
  const [dx, dy] = be.size;
  ctx.save();
  ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_BUS");
  ctx.lineWidth = Math.max(150, hair);
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x + dx, y + dy);
  ctx.stroke();
  ctx.restore();
}

/**
 * ERC violation markers -- canvas/painter.ts's `drawDrcMarkers`, ported to
 * this sheet: one circle per violation whose `location` resolves to a
 * point (`ercMarkerPosition` -- not every shape does, see its own doc;
 * an unresolved one is simply not drawn, same as a violation dialog row
 * that still shows but can't additionally re-frame the view), color-coded
 * by severity including the third `"excluded"` one DRC has no equivalent
 * of (LAYER_ERC_EXCLUSION -- dialog_erc.cpp still draws an excluded
 * marker, just visually muted, rather than hiding it outright). Drawn
 * last, like DRC's own markers, so a marker is never hidden under a wire
 * or symbol.
 */
function drawErcMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, violations: ErcViolation[], selected: number | null, stale = false, display: SchDisplayOptions = DEFAULT_SCH_DISPLAY) {
  const hair = 1 / view.scale;
  violations.forEach((v, i) => {
    // View > Show ERC Errors / Warnings / Exclusions: each severity is a layer that can be switched off (`LAYER_ERC_ERR`, `_WARN`, `_EXCLUSION`).
    if (!ercSeverityShown(v.severity, display)) return;
    const resolved = ercMarkerPosition(v.location, sch);
    if (!resolved) return;
    const [x, y] = resolved.at;
    const on = i === selected;
    const color = on ? layerColor("LAYER_DRC_HIGHLIGHTED") : layerColor(v.severity === "error" ? "LAYER_ERC_ERR" : v.severity === "warning" ? "LAYER_ERC_WARN" : "LAYER_ERC_EXCLUSION");
    const r = on ? ERC_MARKER_RADIUS_UM * 1.4 : ERC_MARKER_RADIUS_UM;
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(100, hair);
    // Out of date: dimmed and dashed, like the PCB's DRC markers.
    if (stale) ctx.setLineDash([r * 0.35, r * 0.25]);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = stale ? 0.1 : v.severity === "excluded" ? 0.18 : 0.35;
    ctx.fill();
    ctx.globalAlpha = stale ? 0.55 : 1;
    ctx.stroke();
    ctx.setLineDash([]);
    drawStrokeText(ctx, "!", x, y + r * 0.5, { sizeUm: r * 1.3, justify: "center", color, thicknessUm: r * 0.22 });
    ctx.restore();
  });
}

/** Our own lint findings (crates/lint) on the sheet: a blue diamond where the finding's `location` resolves to a point (many readability checks name a pair or a net, which do not), bigger and filled when the dialog's Lint tab has it selected. */
function drawLintMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, findings: ErcViolation[], selected: number | null) {
  const hair = 1 / view.scale;
  findings.forEach((v, i) => {
    const resolved = ercMarkerPosition(v.location, sch);
    if (!resolved) return;
    const [x, y] = resolved.at;
    const on = i === selected;
    const r = on ? ERC_MARKER_RADIUS_UM * 1.5 : ERC_MARKER_RADIUS_UM * 1.1;
    ctx.save();
    ctx.strokeStyle = "#4ea1ff";
    ctx.fillStyle = "#4ea1ff";
    ctx.lineWidth = Math.max(100, hair);
    ctx.beginPath();
    ctx.moveTo(x, y - r);
    ctx.lineTo(x + r, y);
    ctx.lineTo(x, y + r);
    ctx.lineTo(x - r, y);
    ctx.closePath();
    ctx.globalAlpha = on ? 0.45 : 0.2;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.stroke();
    ctx.restore();
  });
}

// Background is filled once in screen-space by the caller, before the
// world transform is applied (see canvas/Canvas.tsx's PCB equivalent) --
// not here, which would mean computing an inverse-transformed rect on
// every repaint for no benefit.
export function paintSchematic(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, opts: SchematicPaintOptions): void {
  const hair = 1 / view.scale;
  const display = opts.display ?? DEFAULT_SCH_DISPLAY;

  // Wires. `on`: net-highlighted (click a wire with no modifier), or
  // box/modified-click *selected* (new this session -- `opts.selection`
  // previously only ever affected a symbol's own outline; a selected
  // wire drew no differently from an unselected one, so Del on a box- or
  // shift-selected wire had no visual confirmation it would do anything).
  for (const w of sch.wires) {
    const on = opts.netHighlight === w.net || opts.selection.has(w.id);
    // Bus wires (GAPS.md #20) are the same `SCH_LINE` shape as a plain
    // wire, just `LAYER_BUS` instead of `LAYER_WIRE` -- same convention
    // real eeschema uses (a visibly different, blue by default, color; not
    // a different line width).
    ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor(w.bus ? "LAYER_BUS" : "LAYER_WIRE");
    ctx.lineWidth = Math.max(on ? 300 : 150, hair * (on ? 2.5 : 1));
    ctx.beginPath();
    w.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }

  // Bus entries (GAPS.md #20) -- a short diagonal stub from `at` to
  // `at + size`, `LAYER_BUS`-colored same as the bus wire it taps.
  for (const be of sch.bus_entries) drawBusEntry(ctx, view, be, opts.selection.has(be.id));

  // Junctions -- see junctions.ts's own doc for exactly which points
  // besides coincident wire endpoints (power symbol/label anchors landing
  // on a wire's interior, i.e. a T) now also get a dot.
  const junctionAnchors: Array<[number, number]> = [...sch.power_symbols.map((p) => p.at), ...sch.labels.map((l) => l.at)];
  ctx.fillStyle = layerColor("LAYER_JUNCTION");
  for (const [x, y] of junctionPoints(sch.wires, junctionAnchors)) {
    ctx.beginPath();
    ctx.arc(x, y, Math.max(JUNCTION_RADIUS_UM, hair * 2), 0, Math.PI * 2);
    ctx.fill();
  }
  // Explicit junctions (`J`, `SCH_JUNCTION`): the same dot, drawn wherever one was placed -- on a crossing it is what joins the two
  // wires. A selected one is drawn in the selection color.
  for (const j of sch.junctions ?? []) {
    ctx.fillStyle = layerColor(opts.selection.has(j.id) ? "LAYER_SELECTION_SHADOWS" : "LAYER_JUNCTION");
    ctx.beginPath();
    ctx.arc(j.at[0], j.at[1], Math.max(JUNCTION_RADIUS_UM, hair * 2), 0, Math.PI * 2);
    ctx.fill();
  }

  // Graphic lines on the notes layer (`I`, `SCH_LINE` on `LAYER_NOTES`): decoration, no electrical meaning. KiCad's default line
  // thickness (`default_line_thickness`, 6 mil) when none was set.
  for (const l of sch.lines ?? []) {
    const on = opts.selection.has(l.id);
    ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_NOTES");
    ctx.lineWidth = Math.max(l.width_um > 0 ? l.width_um : NOTES_LINE_UM, hair * (on ? 2.5 : 1));
    ctx.beginPath();
    l.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }

  // Drawn shapes, text boxes, rule areas and directive labels (`SchGraphic`).
  // `SCH_PAINTER::draw( SCH_DIRECTIVE_LABEL )`: with Show Directive Labels off, a directive label is drawn only while it is selected.
  paintGraphics(ctx, view, display.showDirectiveLabels ? (sch.graphics ?? []) : (sch.graphics ?? []).filter((g) => g.shape.type !== "directive" || opts.selection.has(g.id)), opts.selection);

  // Hierarchical sheets (GAPS.md #6) -- drawn early, like the wires/
  // junctions pass above, so a sheet's own local wires/labels/symbols
  // (all drawn later below) read as sitting "on" the page rather than
  // under it.
  for (const s of sch.sheets) drawSheet(ctx, view, s, opts.selection.has(s.id));

  // No-connects.
  for (const nc of sch.no_connects) drawNoConnect(ctx, view, nc, opts.selection.has(nc.id));

  // Labels.
  for (const l of sch.labels) {
    drawLabel(ctx, view, l, sch.wires, opts.netHighlight === l.net || opts.selection.has(l.id));
  }

  // Free text.
  for (const t of sch.texts) drawSchText(ctx, t, opts.selection.has(t.id));

  // Power symbols (GND, +5V, PWR_FLAG, ...).
  for (const ps of sch.power_symbols) {
    const on = opts.netHighlight === ps.net || opts.selection.has(ps.id);
    const resolved = resolveLibSymbol({ id: ps.id, lib_id: ps.lib_id, at: ps.at, rot: ps.rot, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [ps.pin] }, sch.lib_symbols);
    const m = symbolTransformMatrix(ps.rot, null);
    const resolvedPin = resolved?.pins[0] ?? (sch.lib_symbols[ps.lib_id]?.pins.find((p) => p.unit === 0 || p.unit === 1) ? resolvePin(sch.lib_symbols[ps.lib_id]!.pins.find((p) => p.unit === 0 || p.unit === 1)!, m, ps.at) : null);
    drawPowerSymbol(ctx, view, ps, resolved?.graphics ?? null, resolvedPin, on);
  }

  // Symbols (drawn last, like eda-render, so their fill sits on top of any wire stub reaching into the box).
  // Multi-unit: a reference with more than one placed instance gets its
  // KiCad-style unit-letter suffix drawn next to the reference ("U1" ->
  // "U1A"/"U1B"/...), same as real eeschema -- `unitCounts` is only used
  // to decide *whether* to suffix at all, so a single-unit part (every
  // placed instance appears here exactly once) keeps the bare reference it
  // always had.
  const unitCounts = new Map<string, number>();
  for (const s of sch.symbols) unitCounts.set(s.id, (unitCounts.get(s.id) ?? 0) + 1);
  for (const s of sch.symbols) {
    const selected = opts.selection.has(s.id);
    const unitSuffix = (unitCounts.get(s.id) ?? 1) > 1 ? unitLetter(s.unit) : "";
    const real = resolveLibSymbol(s, sch.lib_symbols);
    if (real) {
      drawRealSymbol(ctx, view, s, real.graphics, real.pins, real.bbox, selected, unitSuffix, display.showHiddenPins, real.texts);
    } else {
      const r = resolveSymbol(s);
      drawBoxSymbol(ctx, view, r, selected, unitSuffix);
    }
    // Its fields, where KiCad puts them on the sheet (not turned with the symbol's own frame).
    if (s.fields?.length) drawSchFields(ctx, s.fields, unitSuffix);
    // Do not Populate / Exclude from Simulation marks over the symbol body.
    if (s.dnp || s.exclude_from_sim) {
      const all = real ? real.bbox : libSymbolBounds(s, sch.lib_symbols);
      drawSymbolMarkers(ctx, (real && bodyBoundsOf(real.graphics)) || all, all, s, display.markSimExclusions);
    }
  }

  // A dashed box around a selected item whose own colour change reads too faintly (text, labels, sheets, shapes, power symbols).
  if (opts.selection.size > 0) {
    for (const ref of allItems(sch)) {
      if (!opts.selection.has(ref.id)) continue;
      if (ref.kind === "label" || ref.kind === "text" || ref.kind === "sheet" || ref.kind === "graphic" || ref.kind === "power" || ref.kind === "no_connect") {
        const b = itemBounds(sch, ref);
        if (b) drawSelectionBox(ctx, view, b);
      }
    }
  }

  // ERC markers last of all -- an overlay above every sheet layer, matching real KiCad (and this app's own canvas/painter.ts for DRC).
  if (opts.ercViolations) drawErcMarkers(ctx, view, sch, opts.ercViolations, opts.ercSelected ?? null, opts.ercStale, display);
  if (opts.lintViolations) drawLintMarkers(ctx, view, sch, opts.lintViolations, opts.lintSelected ?? null);
}

// Re-exported for SchematicView.tsx's bounds/hit-testing, which need the
// same box-vs-real split this file's own paint loop uses.
export { symbolBounds } from "./libSymbol";
