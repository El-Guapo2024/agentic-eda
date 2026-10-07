// Every selectable item of a schematic sheet in one catalog: bounds, point hit-testing and marquee collection
// for symbols, power symbols, wires, labels, texts, no-connects, bus entries, junctions, graphic lines, sheets
// and the drawn graphics (`SchGraphic`). `SCH_SELECTION_TOOL` selects all of these; before this the studio
// could only select a symbol, a wire (modified click) or a junction/line, so a label, a text, a sheet or a
// shape could not be picked at all -- and every edit tool that acts on a selection (Lock, Change To Label, Break
// Wire, ...) had nothing to work on.
import type { Schematic, SchematicLabel, SchematicText } from "../../api/types";
import { boxEncloses, boxesOverlap, boxOfPoints, distPointSegment, distToPolyline, graphicBounds, graphicHit, inflateBox, pointInBox, type Box, type P } from "../../kicad-port/schItemGeom";
import { measureStrokeText } from "../text/strokeFont";
import { globalLabelOutline, hierLabelOutline, inferSpin, LABEL_TEXT_SIZE_UM, localLabelTextPlacement } from "./labelShape";
import { resolveLibSymbol, symbolBounds } from "./libSymbol";

export type SchItemKind = "symbol" | "power" | "wire" | "label" | "text" | "no_connect" | "bus_entry" | "junction" | "line" | "sheet" | "graphic";

export interface SchItemRef {
  id: string;
  kind: SchItemKind;
}

/** eeschema/default_values.h DEFAULT_NOCONNECT_SIZE half-width -- keep in step with painter.ts. */
const NOCONNECT_HALF_UM = 609.6;
const JUNCTION_RADIUS_UM = 457.2;

/** The text's rectangle for a string drawn from `at` in `justify`, rotated `angleDeg` about `at`. */
function textBox(text: string, at: P, sizeUm: number, justify: "left" | "center" | "right", angleDeg: number, baselineShift = 0): Box {
  const lines = text.split("\n");
  const w = Math.max(...lines.map((l) => measureStrokeText(l, sizeUm)), sizeUm * 0.5);
  const h = sizeUm * Math.max(1, lines.length) * 1.1;
  const x0 = justify === "left" ? 0 : justify === "center" ? -w / 2 : -w;
  // The glyphs sit above the baseline: a cap height of about 0.95 of the size.
  const corners: P[] = [
    [x0, baselineShift - sizeUm],
    [x0 + w, baselineShift - sizeUm],
    [x0 + w, baselineShift - sizeUm + h],
    [x0, baselineShift - sizeUm + h],
  ];
  const a = (angleDeg * Math.PI) / 180;
  const c = Math.cos(a);
  const s = Math.sin(a);
  return boxOfPoints(corners.map(([x, y]) => [at[0] + x * c - y * s, at[1] + x * s + y * c] as P))!;
}

export function textItemBounds(t: SchematicText): Box {
  return textBox(t.content, t.at, t.size_um, "left", t.angle);
}

export function labelBounds(sch: Schematic, l: SchematicLabel): Box {
  const spin = inferSpin(sch.wires, l.at);
  if (l.scope !== "local" && l.shape) {
    const outline = l.scope === "global" ? globalLabelOutline(l.net, l.shape, spin, l.at) : hierLabelOutline(l.shape, spin, l.at);
    return boxOfPoints(outline as P[]) ?? { minX: l.at[0], minY: l.at[1], maxX: l.at[0], maxY: l.at[1] };
  }
  const { pos, justify } = localLabelTextPlacement(spin, l.at);
  const vertical = spin === "up" || spin === "bottom";
  return textBox(l.net, pos as P, LABEL_TEXT_SIZE_UM, justify, vertical ? -90 : 0);
}

function powerSymbolBounds(sch: Schematic, ps: Schematic["power_symbols"][number]): Box {
  const resolved = resolveLibSymbol({ id: ps.id, lib_id: ps.lib_id, at: ps.at, rot: ps.rot, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [ps.pin] }, sch.lib_symbols);
  if (resolved) return resolved.bbox;
  return { minX: ps.at[0] - 600, minY: ps.at[1] - 600, maxX: ps.at[0] + 600, maxY: ps.at[1] + 600 };
}

/** Every item of the sheet, in painting order (later items sit on top of earlier ones). */
export function allItems(sch: Schematic): SchItemRef[] {
  const out: SchItemRef[] = [];
  for (const w of sch.wires) if (w.id) out.push({ id: w.id, kind: "wire" });
  for (const b of sch.bus_entries) if (b.id) out.push({ id: b.id, kind: "bus_entry" });
  for (const j of sch.junctions ?? []) if (j.id) out.push({ id: j.id, kind: "junction" });
  for (const l of sch.lines ?? []) if (l.id) out.push({ id: l.id, kind: "line" });
  for (const g of sch.graphics ?? []) if (g.id) out.push({ id: g.id, kind: "graphic" });
  for (const s of sch.sheets) if (s.id) out.push({ id: s.id, kind: "sheet" });
  for (const n of sch.no_connects) if (n.id) out.push({ id: n.id, kind: "no_connect" });
  for (const l of sch.labels) if (l.id) out.push({ id: l.id, kind: "label" });
  for (const t of sch.texts) if (t.id) out.push({ id: t.id, kind: "text" });
  for (const p of sch.power_symbols) if (p.id) out.push({ id: p.id, kind: "power" });
  const seen = new Set<string>();
  for (const s of sch.symbols) {
    if (seen.has(s.id)) continue; // a multi-unit symbol is one selectable reference
    seen.add(s.id);
    out.push({ id: s.id, kind: "symbol" });
  }
  return out;
}

export function kindOf(sch: Schematic, id: string): SchItemKind | null {
  return allItems(sch).find((r) => r.id === id)?.kind ?? null;
}

/** The bounds of an item, or null when the id is not on this sheet. For a multi-unit symbol: every placed unit. */
export function itemBounds(sch: Schematic, ref: SchItemRef): Box | null {
  switch (ref.kind) {
    case "symbol": {
      let b: Box | null = null;
      for (const s of sch.symbols) {
        if (s.id !== ref.id) continue;
        const sb = symbolBounds(s, sch.lib_symbols);
        b = b ? { minX: Math.min(b.minX, sb.minX), minY: Math.min(b.minY, sb.minY), maxX: Math.max(b.maxX, sb.maxX), maxY: Math.max(b.maxY, sb.maxY) } : sb;
      }
      return b;
    }
    case "power": {
      const ps = sch.power_symbols.find((p) => p.id === ref.id);
      return ps ? powerSymbolBounds(sch, ps) : null;
    }
    case "wire": {
      const w = sch.wires.find((x) => x.id === ref.id);
      return w ? boxOfPoints(w.pts) : null;
    }
    case "label": {
      const l = sch.labels.find((x) => x.id === ref.id);
      return l ? labelBounds(sch, l) : null;
    }
    case "text": {
      const t = sch.texts.find((x) => x.id === ref.id);
      return t ? textItemBounds(t) : null;
    }
    case "no_connect": {
      const n = sch.no_connects.find((x) => x.id === ref.id);
      return n ? { minX: n.at[0] - NOCONNECT_HALF_UM, minY: n.at[1] - NOCONNECT_HALF_UM, maxX: n.at[0] + NOCONNECT_HALF_UM, maxY: n.at[1] + NOCONNECT_HALF_UM } : null;
    }
    case "bus_entry": {
      const b = sch.bus_entries.find((x) => x.id === ref.id);
      return b ? boxOfPoints([b.at, [b.at[0] + b.size[0], b.at[1] + b.size[1]]]) : null;
    }
    case "junction": {
      const j = (sch.junctions ?? []).find((x) => x.id === ref.id);
      return j ? { minX: j.at[0] - JUNCTION_RADIUS_UM, minY: j.at[1] - JUNCTION_RADIUS_UM, maxX: j.at[0] + JUNCTION_RADIUS_UM, maxY: j.at[1] + JUNCTION_RADIUS_UM } : null;
    }
    case "line": {
      const l = (sch.lines ?? []).find((x) => x.id === ref.id);
      return l ? boxOfPoints(l.pts) : null;
    }
    case "sheet": {
      const s = sch.sheets.find((x) => x.id === ref.id);
      return s ? { minX: s.at[0], minY: s.at[1], maxX: s.at[0] + s.size[0], maxY: s.at[1] + s.size[1] } : null;
    }
    case "graphic": {
      const g = (sch.graphics ?? []).find((x) => x.id === ref.id);
      return g ? graphicBounds(g) : null;
    }
  }
}

/** Does the point `p` hit this item, within `tol` um? (Symbols: their bounds; wires/lines: the polyline; shapes: `EDA_SHAPE::hitTest`.) */
export function hitItem(sch: Schematic, ref: SchItemRef, p: P, tol: number): boolean {
  switch (ref.kind) {
    case "wire": {
      const w = sch.wires.find((x) => x.id === ref.id);
      return w ? distToPolyline(p, w.pts as P[], false) <= tol : false;
    }
    case "line": {
      const l = (sch.lines ?? []).find((x) => x.id === ref.id);
      return l ? distToPolyline(p, l.pts as P[], false) <= tol : false;
    }
    case "bus_entry": {
      const b = sch.bus_entries.find((x) => x.id === ref.id);
      return b ? distPointSegment(p, b.at, [b.at[0] + b.size[0], b.at[1] + b.size[1]]) <= tol : false;
    }
    case "graphic": {
      const g = (sch.graphics ?? []).find((x) => x.id === ref.id);
      return g ? graphicHit(g, p, tol) : false;
    }
    case "sheet": {
      // A sheet is selected by its border (`SCH_SHEET::HitTest`: the outline, plus the filled body when it has a fill).
      const s = sch.sheets.find((x) => x.id === ref.id);
      if (!s) return false;
      const [x, y] = s.at;
      const [w, h] = s.size;
      const corners: P[] = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
      return distToPolyline(p, corners, true) <= tol;
    }
    default: {
      const b = itemBounds(sch, ref);
      return b ? pointInBox(p, inflateBox(b, tol)) : false;
    }
  }
}

/** Every item under the point, topmost first. */
export function hitItems(sch: Schematic, x: number, y: number, tol: number): SchItemRef[] {
  const out: SchItemRef[] = [];
  const all = allItems(sch);
  for (let i = all.length - 1; i >= 0; i--) if (hitItem(sch, all[i]!, [x, y], tol)) out.push(all[i]!);
  return out;
}

/** The ids a marquee selects: items overlapping the box (`crossing`) or sitting wholly inside it. */
export function boxItems(sch: Schematic, box: [number, number, number, number], crossing: boolean): string[] {
  const b: Box = { minX: box[0], minY: box[1], maxX: box[2], maxY: box[3] };
  const out: string[] = [];
  for (const ref of allItems(sch)) {
    const ib = itemBounds(sch, ref);
    if (!ib) continue;
    if (crossing ? boxesOverlap(ib, b) : boxEncloses(b, ib)) out.push(ref.id);
  }
  return out;
}
