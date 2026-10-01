// Resolves one placed symbol instance against GET /api/schematic's
// `lib_symbols` into real, world-space (um, this app's shared Y-down
// space) geometry -- the library-symbol equivalent of layout.ts's
// `resolveSymbol`, which only ever produces a generic box. painter.ts
// tries this first and falls back to layout.ts's box when a symbol's
// `lib_id` is null or isn't a key in `lib_symbols` (no real graphics
// resolved for it yet -- see types.ts's `SchematicSymbol.lib_id` doc
// comment).
import type { LibFill, LibGraphic, LibSymbols, Mm, SchematicSymbol } from "../../api/types";
import { resolveSymbol } from "./layout";
import { resolveLibPoint, resolvePin, symbolTransformMatrix, type ResolvedPin } from "./transform";

export type ResolvedGraphic =
  | { kind: "rectangle"; start: [number, number]; end: [number, number]; strokeWidthUm: number; fill: LibFill }
  | { kind: "polyline"; pts: [number, number][]; strokeWidthUm: number; fill: LibFill }
  | { kind: "circle"; center: [number, number]; radiusUm: number; strokeWidthUm: number; fill: LibFill }
  | { kind: "arc"; start: [number, number]; mid: [number, number]; end: [number, number]; strokeWidthUm: number; fill: LibFill }
  | { kind: "text"; content: string; at: [number, number]; angleDeg: number; sizeUm: number };

export interface ResolvedLibSymbol {
  graphics: ResolvedGraphic[];
  pins: ResolvedPin[];
  /** World-space, um -- every graphic point and pin tip/root, so it covers pins sticking out past the body outline (a generic box's bounds, by contrast, are defined to just be the box). */
  bbox: { minX: number; minY: number; maxX: number; maxY: number };
}

const mmToUm = (v: Mm) => v * 1000;

/** 0 = shared by every unit/body-style -- eeschema's own convention (lib_symbol.h), confirmed directly against source this session: an item with `unit`/`body_style` 0 is drawn for every instance regardless of which unit/style that instance is. */
function visibleFor(itemUnit: number, itemBodyStyle: number, instanceUnit: number, instanceBodyStyle: number): boolean {
  return (itemUnit === 0 || itemUnit === instanceUnit) && (itemBodyStyle === 0 || itemBodyStyle === instanceBodyStyle);
}

export function resolveLibSymbol(instance: SchematicSymbol, lib: LibSymbols): ResolvedLibSymbol | null {
  if (!instance.lib_id) return null;
  const sym = lib[instance.lib_id];
  if (!sym) return null;

  const m = symbolTransformMatrix(instance.rot, instance.mirror);
  const at = instance.at;
  const unit = instance.unit || 1;
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
    graphics.push(resolveGraphic(g, m, at, grow));
  }

  const pins: ResolvedPin[] = [];
  for (const p of sym.pins) {
    if (!visibleFor(p.unit, p.body_style, unit, bodyStyle)) continue;
    const resolved = resolvePin(p, m, at);
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

  return { graphics, pins, bbox: { minX, minY, maxX, maxY } };
}

function resolveGraphic(g: LibGraphic, m: ReturnType<typeof symbolTransformMatrix>, at: [number, number], grow: (p: [number, number]) => void): ResolvedGraphic {
  const pt = (p: [Mm, Mm]) => {
    const r = resolveLibPoint(p, m, at);
    grow(r);
    return r;
  };
  switch (g.kind) {
    case "rectangle":
      return { kind: "rectangle", start: pt(g.start), end: pt(g.end), strokeWidthUm: mmToUm(g.stroke_width), fill: g.fill };
    case "polyline":
      return { kind: "polyline", pts: g.pts.map(pt), strokeWidthUm: mmToUm(g.stroke_width), fill: g.fill };
    case "circle":
      return { kind: "circle", center: pt(g.center), radiusUm: mmToUm(g.radius), strokeWidthUm: mmToUm(g.stroke_width), fill: g.fill };
    case "arc":
      return { kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end), strokeWidthUm: mmToUm(g.stroke_width), fill: g.fill };
    case "text":
      return { kind: "text", content: g.content, at: pt(g.at), angleDeg: g.angle_deg, sizeUm: mmToUm(g.size_mm) };
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
