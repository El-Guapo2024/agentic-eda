// Pure schematic hit-testing and framing helpers, shared by SchematicView.tsx
// (clicks) and the action runner (hover fallback / zoom-to-fit hotkeys).
import type { Schematic } from "../../api/types";
import { symbolBounds } from "./painter";
import { nearestPointOnSegment } from "../../kicad-port/schBusUnfold";
import { graphicBounds } from "../../kicad-port/schItemGeom";
import { pageOf } from "./drawingSheet";

/**
 * World-space bounds for the initial fit. KiCad opens a schematic framed
 * on the whole page, not just whatever's drawn on it (an empty sheet
 * still shows the full A4 frame) -- so this is always the page rect,
 * widened to also cover any content that happens to sit outside it
 * (this app doesn't clip/reflow existing symbol positions to the page).
 */
export function schematicBounds(sch: Schematic): Array<[number, number]> {
  const page = pageOf(sch.paper);
  const pts: Array<[number, number]> = [
    [0, 0],
    [page.widthNm, page.heightNm],
  ];
  for (const s of sch.symbols) {
    const b = symbolBounds(s, sch.lib_symbols);
    pts.push([b.minX, b.minY], [b.maxX, b.maxY]);
  }
  for (const w of sch.wires) pts.push(...w.pts);
  for (const l of sch.lines ?? []) pts.push(...l.pts);
  for (const g of sch.graphics ?? []) {
    const b = graphicBounds(g);
    pts.push([b.minX, b.minY], [b.maxX, b.maxY]);
  }
  for (const j of sch.junctions ?? []) pts.push(j.at);
  for (const l of sch.labels) pts.push(l.at);
  for (const ps of sch.power_symbols) pts.push(ps.at);
  return pts;
}

export function hitSymbol(sch: Schematic, xNm: number, yNm: number): string | null {
  return hitSymbolInstance(sch, xNm, yNm)?.id ?? null;
}

/** The placed symbol (the unit, when several units of one reference are on the sheet) under the point: [`hitSymbol`]'s test, the item itself. */
export function hitSymbolInstance(sch: Schematic, xNm: number, yNm: number): Schematic["symbols"][number] | null {
  for (let i = sch.symbols.length - 1; i >= 0; i--) {
    const s = sch.symbols[i]!;
    // World-space bbox test -- symbolBounds already accounts for
    // rotation/mirror (and, for a real lib_symbols-resolved symbol, the
    // real graphics' own extent, not just a generic box), so there is no
    // local-space un-rotation to do here.
    const b = symbolBounds(s, sch.lib_symbols);
    if (xNm >= b.minX - 200_000 && xNm <= b.maxX + 200_000 && yNm >= b.minY - 200_000 && yNm <= b.maxY + 200_000) return s;
  }
  return null;
}

/** The id of the polyline in `items` nearest to the point, within `thresholdNm` (nearest segment). */
function nearestPolyline(items: ReadonlyArray<{ id: string; pts: readonly (readonly [number, number])[] }>, xNm: number, yNm: number, thresholdNm: number): string | null {
  let best: { id: string; d: number } | null = null;
  for (const w of items) {
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      const dx = x2 - x1,
        dy = y2 - y1;
      const lenSq = dx * dx + dy * dy || 1;
      let t = ((xNm - x1) * dx + (yNm - y1) * dy) / lenSq;
      t = Math.max(0, Math.min(1, t));
      const px = x1 + t * dx,
        py = y1 + t * dy;
      const d = Math.hypot(xNm - px, yNm - py);
      if (d <= thresholdNm && (!best || d < best.d) && w.id) best = { id: w.id, d };
    }
  }
  return best?.id ?? null;
}

/** sch_selection_tool.cpp: a wire is also directly selectable (by id, for Del/move), distinct from the net-highlight click `hitWireNet` below handles when nothing is selectable at that point. Nearest-segment, same threshold convention as the net click. */
export function hitWire(sch: Schematic, xNm: number, yNm: number, thresholdNm: number): string | null {
  return nearestPolyline(sch.wires, xNm, yNm, thresholdNm);
}

/** A graphic line (`I`, notes layer) under the point -- selectable by id like a wire, but it carries no net, so a plain click selects it. */
export function hitSchLine(sch: Schematic, xNm: number, yNm: number, thresholdNm: number): string | null {
  return nearestPolyline(sch.lines ?? [], xNm, yNm, thresholdNm);
}

/** An explicit junction (`J`) under the point, within `radiusNm`. */
export function hitJunction(sch: Schematic, xNm: number, yNm: number, radiusNm: number): string | null {
  let best: { id: string; d: number } | null = null;
  for (const j of sch.junctions ?? []) {
    const d = Math.hypot(j.at[0] - xNm, j.at[1] - yNm);
    if (d <= radiusNm && (!best || d < best.d) && j.id) best = { id: j.id, d };
  }
  return best?.id ?? null;
}

/**
 * The bus wire under the point (`SCH_LINE_WIRE_BUS_TOOL::getBusForUnfolding` -> a bus locate), nearest segment within `thresholdNm`.
 * `at` is where the bus entry roots (`doUnfoldBus`: `bus->GetSeg().NearestPoint( cursor )` of the grid-snapped cursor, so a horizontal
 * or vertical bus on the grid gets an on-grid entry): the point of that segment nearest `rootNear`, the point itself when omitted.
 */
export function hitBus(sch: Schematic, xNm: number, yNm: number, thresholdNm: number, rootNear?: readonly [number, number]): { id: string; net: string; members: string[]; at: [number, number] } | null {
  let best: { id: string; net: string; members: string[]; at: [number, number]; d: number } | null = null;
  for (const w of sch.wires) {
    if (!w.bus || !w.id) continue;
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const a = w.pts[i]!;
      const b = w.pts[i + 1]!;
      const [px, py] = nearestPointOnSegment(a, b, [xNm, yNm]);
      const d = Math.hypot(xNm - px, yNm - py);
      if (d <= thresholdNm && (!best || d < best.d)) best = { id: w.id, net: w.net, members: w.members ?? [], at: rootNear ? nearestPointOnSegment(a, b, rootNear) : [px, py], d };
    }
  }
  return best ? { id: best.id, net: best.net, members: best.members, at: best.at } : null;
}

