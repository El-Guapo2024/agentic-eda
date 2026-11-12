// Resolves one placed symbol instance against GET /api/schematic's
// `lib_symbols` into real, world-space (um, this app's shared Y-down
// space) geometry -- the library-symbol equivalent of layout.ts's
// `resolveSymbol`, which only ever produces a generic box. painter.ts
// tries this first and falls back to layout.ts's box when a symbol's
// `lib_id` is null or isn't a key in `lib_symbols` (no real graphics
// resolved for it yet -- see types.ts's `SchematicSymbol.lib_id` doc
// comment).
import type { LibFill, LibGraphic, LibPin, LibShapeLooks, LibSymbols, Mm, SchematicSymbol } from "../../api/types";
import { resolveSymbol } from "./layout";
import { DEFAULT_PIN_TEXTS, type PinTexts } from "./pinText";
import { resolveLibPoint, resolvePin, symbolTransformMatrix, type ResolvedPin } from "./transform";
import type { SchColor, SchGraphicShape } from "../../api/schEditTypes";
import { isVerticalAngle, itemOrientationOf, orientedText } from "../../kicad-port/symTextBoxPlaced";

/** A placed symbol's shape's looks (`LibShapeLooks`), the corner radius in nm. Absent: a shape drawn in the symbol's colour, solid, as before. */
export interface ResolvedShapeLooks {
  lineStyle?: LibShapeLooks["line_style"];
  strokeColor?: SchColor;
  fillColor?: SchColor;
  hatch?: LibShapeLooks["hatch"];
  radiusNm?: number;
}

export type ResolvedGraphic =
  | { kind: "rectangle"; start: [number, number]; end: [number, number]; strokeWidthNm: number; fill: LibFill; looks?: ResolvedShapeLooks }
  | { kind: "polyline"; pts: [number, number][]; strokeWidthNm: number; fill: LibFill; looks?: ResolvedShapeLooks }
  | { kind: "circle"; center: [number, number]; radiusNm: number; strokeWidthNm: number; fill: LibFill; looks?: ResolvedShapeLooks }
  | { kind: "arc"; start: [number, number]; mid: [number, number]; end: [number, number]; strokeWidthNm: number; fill: LibFill; looks?: ResolvedShapeLooks }
  | { kind: "text"; content: string; at: [number, number]; angleDeg: number; sizeNm: number }
  /** A library symbol's bezier (the Symbol Editor's; a placed symbol's engine drawing has its polyline instead): `SCH_PAINTER`'s `DrawCurve( start, c1, c2, end )`. */
  | { kind: "bezier"; start: [number, number]; c1: [number, number]; c2: [number, number]; end: [number, number]; strokeWidthNm: number; fill: LibFill }
  /** The text of a library text box on the placed symbol (kicad-port/symTextBoxPlaced.ts), as a sheet text box's shape -- corners on the sheet, the shown text, the direction and justification the symbol's orientation gives it -- with its own text colour when it has one. Its border and fill are a `rectangle`. */
  | { kind: "text_box"; shape: Extract<SchGraphicShape, { type: "text_box" }>; color?: SchColor };

export interface ResolvedLibSymbol {
  graphics: ResolvedGraphic[];
  pins: ResolvedPin[];
  /** World-space, um -- every graphic point and pin tip/root, so it covers pins sticking out past the body outline (a generic box's bounds, by contrast, are defined to just be the box). */
  bbox: { minX: number; minY: number; maxX: number; maxY: number };
  /** How this symbol's pins show their names and numbers. */
  texts: PinTexts;
}

const mmToNm = (v: Mm) => v * 1e6;

/** 0 = shared by every unit/body-style -- eeschema's own convention (lib_symbol.h), confirmed directly against source this session: an item with `unit`/`body_style` 0 is drawn for every instance regardless of which unit/style that instance is. */
function visibleFor(itemUnit: number, itemBodyStyle: number, instanceUnit: number, instanceBodyStyle: number): boolean {
  return (itemUnit === 0 || itemUnit === instanceUnit) && (itemBodyStyle === 0 || itemBodyStyle === instanceBodyStyle);
}

/**
 * The pins of one placed symbol with the places Swap Pins traded (`SwapPinGeometry`, `SchExtras::pin_swaps`): pin `n` has the position, direction and length of the library
 * pin `swaps[n]`, and keeps its own number, name and type. A number the list does not have is left out of the exchange.
 */
export function withSwappedPlaces(pins: LibPin[], swaps: Record<string, string> | null | undefined): LibPin[] {
  if (!swaps) return pins;
  const byNumber = new Map(pins.map((p) => [p.number, p]));
  return pins.map((p) => {
    const other = byNumber.get(swaps[p.number] ?? p.number);
    return other && other !== p ? { ...p, at: other.at, angle_deg: other.angle_deg, length_mm: other.length_mm } : p;
  });
}

/**
 * A library pin as a placed symbol draws it when the pin took the alternate function `alt` (`SCH_PIN::SetAlt`): named, typed and shaped by the alternate
 * (`SCH_PIN::GetShownName` / `GetType` / `GetShape`: `m_alt` first). An alternate the library pin has not is no alternate (`SetAlt`'s `contains` check), and
 * the pin keeps its own name, type and shape; so does a pin with none.
 */
export function withAlternate(pin: LibPin, alt: string | null | undefined): LibPin {
  if (!alt || alt === pin.name) return pin;
  const a = pin.alternates?.find((x) => x.name === alt);
  return a ? { ...pin, name: a.name, electrical_type: a.electrical_type, shape: a.shape } : pin;
}

export function resolveLibSymbol(instance: SchematicSymbol, lib: LibSymbols): ResolvedLibSymbol | null {
  if (!instance.lib_id) return null;
  const sym = lib[instance.lib_id];
  if (!sym) return null;

  const m = symbolTransformMatrix(instance.rot, instance.mirror);
  const at = instance.at;
  // the unit the placement in view draws (`GetUnitSelection( &GetCurrentSheet() )`, UI-LEFT S9)
  const unit = instance.path_unit || instance.unit || 1;
  const bodyStyle = instance.body_style || 1;

  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  const grow = (p: [number, number]) => {
    minX = Math.min(minX, p[0]);
    minY = Math.min(minY, p[1]);
    maxX = Math.max(maxX, p[0]);
    maxY = Math.max(maxY, p[1]);
  };

  const graphics: ResolvedGraphic[] = [];
  for (const g of sym.graphics) {
    if (!visibleFor(g.unit, g.body_style, unit, bodyStyle)) continue;
    graphics.push(resolveGraphic(g, m, at, grow, instance.text_box_shown));
  }

  const pins: ResolvedPin[] = [];
  // the alternate function each placed pin took (`SchematicPin.alt`, the symbol's `SchExtras::pin_alternates`)
  const alts = new Map((instance.pins ?? []).filter((p) => p.alt).map((p) => [p.number, p.alt]));
  for (const p of withSwappedPlaces(sym.pins.filter((q) => visibleFor(q.unit, q.body_style, unit, bodyStyle)), instance.pin_swaps)) {
    const resolved = resolvePin(withAlternate(p, alts.get(p.number)), m, at);
    grow(resolved.tip);
    grow(resolved.root);
    pins.push(resolved);
  }

  if (!Number.isFinite(minX)) {
    // No visible graphics or pins for this (unit, body_style) -- a real
    // gap in the library data, not something to crash over. Degenerate
    // zero-size box at the instance's own origin.
    minX = minY = maxX = maxY = 0;
    const [ox, oy] = at;
    minX += ox;
    minY += oy;
    maxX += ox;
    maxY += oy;
  }

  const texts: PinTexts = {
    namesHidden: sym.pin_names_hidden ?? DEFAULT_PIN_TEXTS.namesHidden,
    numbersHidden: sym.pin_numbers_hidden ?? DEFAULT_PIN_TEXTS.numbersHidden,
    nameOffsetNm: sym.pin_name_offset === undefined ? DEFAULT_PIN_TEXTS.nameOffsetNm : sym.pin_name_offset * 1e6,
  };
  return { graphics, pins, bbox: { minX, minY, maxX, maxY }, texts };
}

/** `r` with the looks the library shape `g` has (`LibShapeLooks`), when it has any. */
function withLooks<T extends ResolvedGraphic>(r: T, g: LibShapeLooks): T {
  const looks: ResolvedShapeLooks = {};
  if (g.line_style && g.line_style !== "default") looks.lineStyle = g.line_style;
  if (g.stroke_color) looks.strokeColor = g.stroke_color;
  if (g.fill_color) looks.fillColor = g.fill_color;
  if (g.hatch) looks.hatch = g.hatch;
  if (g.corner_radius_mm && g.corner_radius_mm > 0) looks.radiusNm = g.corner_radius_mm * 1e6;
  return Object.keys(looks).length > 0 ? { ...r, looks } : r;
}

function resolveGraphic(g: LibGraphic, m: ReturnType<typeof symbolTransformMatrix>, at: [number, number], grow: (p: [number, number]) => void, textBoxShown?: Record<string, string>): ResolvedGraphic {
  const pt = (p: [Mm, Mm]) => {
    const r = resolveLibPoint(p, m, at);
    grow(r);
    return r;
  };
  switch (g.kind) {
    case "rectangle":
      return withLooks({ kind: "rectangle", start: pt(g.start), end: pt(g.end), strokeWidthNm: mmToNm(g.stroke_width), fill: g.fill }, g);
    case "polyline":
      return withLooks({ kind: "polyline", pts: g.pts.map(pt), strokeWidthNm: mmToNm(g.stroke_width), fill: g.fill }, g);
    case "circle":
      return withLooks({ kind: "circle", center: pt(g.center), radiusNm: mmToNm(g.radius), strokeWidthNm: mmToNm(g.stroke_width), fill: g.fill }, g);
    case "arc":
      return withLooks({ kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end), strokeWidthNm: mmToNm(g.stroke_width), fill: g.fill }, g);
    case "text":
      return { kind: "text", content: g.content, at: pt(g.at), angleDeg: g.angle_deg, sizeNm: mmToNm(g.size_mm) };
    case "text_box": {
      // `SCH_PAINTER::draw( SCH_SYMBOL )`'s temp copy: the corners where the transform puts them, the text turned and justified by `OrientAndMirrorSymbolItems`, and
      // its variables the symbol's (`text_box_shown`)
      const [sx, sy] = pt(g.start);
      const [ex, ey] = pt(g.end);
      const text = orientedText(isVerticalAngle(g.angle_deg), g.h_align, itemOrientationOf(m));
      const shape: Extract<SchGraphicShape, { type: "text_box" }> = {
        type: "text_box",
        start: { x: sx, y: sy },
        end: { x: ex, y: ey },
        text: textBoxShown?.[String(g.index)] ?? g.text,
        angle: text.vertical ? 90_000 : 0,
        size_nm: mmToNm(g.size_mm),
        bold: !!g.bold,
        italic: !!g.italic,
        h_align: text.hAlign,
        v_align: g.v_align,
        margin_nm: mmToNm(g.margin_mm),
      };
      return g.text_color ? { kind: "text_box", shape, color: g.text_color } : { kind: "text_box", shape };
    }
  }
}

export interface SymbolBounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

/**
 * World-space bounds for `s`, trying real lib_symbols geometry first and
 * falling back to layout.ts's generic box -- the one place both
 * SchematicView.tsx (view-fit, hit-testing) and painter.ts (selection
 * outline) go for "how big is this symbol", so the box-vs-real split
 * only has to be reasoned about once. When `lib_symbols` eventually
 * covers every symbol, this function (and layout.ts's box path it falls
 * back to) can shrink to just the `resolveLibSymbol` branch.
 */
export function symbolBounds(s: SchematicSymbol, lib: LibSymbols): SymbolBounds {
  const real = resolveLibSymbol(s, lib);
  if (real) return real.bbox;
  const r = resolveSymbol(s);
  const rot = ((s.rot % 360) + 360) % 360;
  const [w, h] = rot === 90 || rot === 270 ? [r.height, r.width] : [r.width, r.height];
  return { minX: s.at[0], minY: s.at[1], maxX: s.at[0] + w, maxY: s.at[1] + h };
}
