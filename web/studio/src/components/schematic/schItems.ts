// Every selectable item of a schematic sheet in one catalog: bounds, point hit-testing and marquee collection
// for symbols, power symbols, wires, labels, texts, no-connects, bus entries, junctions, graphic lines, sheets
// and the drawn graphics (`SchGraphic`). `SCH_SELECTION_TOOL` selects all of these; before this the studio
// could only select a symbol, a wire (modified click) or a junction/line, so a label, a text, a sheet or a
// shape could not be picked at all -- and every edit tool that acts on a selection (Lock, Change To Label, Break
// Wire, ...) had nothing to work on.
import type { Schematic, SchematicLabel, SchematicText } from "../../api/types";
import { boxEncloses, boxesOverlap, boxOfPoints, distPointSegment, distToPolyline, graphicBounds, graphicHit, inflateBox, pointInBox, type Box, type P } from "../../kicad-port/schItemGeom";
import { measureStrokeText } from "../text/strokeFont";
import { globalLabelOutline, hierLabelOutline, inferSpin, LABEL_TEXT_SIZE_NM, localLabelTextPlacement } from "./labelShape";
import { resolveLibSymbol, symbolBounds } from "./libSymbol";
import { fieldBox, fieldItems, isShownField, type FieldItem } from "../../kicad-port/schFieldEdit";
import { hitPin, pinBox, pinById, pinItems } from "../../kicad-port/schPins";
import { sheetPinBox, sheetPinById, sheetPinItems } from "../../kicad-port/schSheetPinItems";
import { schTextBox } from "../../kicad-port/schTextItem";
import { cellHitDistance, isSelectableCell } from "../../kicad-port/tables";

export type SchItemKind = "symbol" | "power" | "wire" | "label" | "text" | "no_connect" | "bus_entry" | "junction" | "line" | "sheet" | "sheet_pin" | "graphic" | "table" | "tablecell" | "field" | "pin";

export interface SchItemRef {
  id: string;
  kind: SchItemKind;
}

/** eeschema/default_values.h DEFAULT_NOCONNECT_SIZE half-width -- keep in step with painter.ts. */
const NOCONNECT_HALF_NM = 609_600;
const JUNCTION_RADIUS_NM = 457_200;

/** The text's rectangle for a string drawn from `at` in `justify`, rotated `angleDeg` about `at`. */
function textBox(text: string, at: P, sizeNm: number, justify: "left" | "center" | "right", angleDeg: number, baselineShift = 0): Box {
  const lines = text.split("\n");
  const w = Math.max(...lines.map((l) => measureStrokeText(l, sizeNm)), sizeNm * 0.5);
  const h = sizeNm * Math.max(1, lines.length) * 1.1;
  const x0 = justify === "left" ? 0 : justify === "center" ? -w / 2 : -w;
  // The glyphs sit above the baseline: a cap height of about 0.95 of the size.
  const corners: P[] = [
    [x0, baselineShift - sizeNm],
    [x0 + w, baselineShift - sizeNm],
    [x0 + w, baselineShift - sizeNm + h],
    [x0, baselineShift - sizeNm + h],
  ];
  const a = (angleDeg * Math.PI) / 180;
  const c = Math.cos(a);
  const s = Math.sin(a);
  return boxOfPoints(corners.map(([x, y]) => [at[0] + x * c - y * s, at[1] + x * s + y * c] as P))!;
}

/** The box a free text takes on the sheet: justified about its anchor as it is drawn (`EDA_TEXT::GetTextBox`). */
export function textItemBounds(t: SchematicText): Box {
  return schTextBox(t, measureStrokeText);
}

export function labelBounds(sch: Schematic, l: SchematicLabel): Box {
  const spin = l.spin ?? inferSpin(sch.wires, l.at);
  const size = l.size_nm && l.size_nm > 0 ? l.size_nm : LABEL_TEXT_SIZE_NM;
  if (l.scope !== "local" && l.shape) {
    const outline = l.scope === "global" ? globalLabelOutline(l.net, l.shape, spin, l.at, size) : hierLabelOutline(l.shape, spin, l.at, size);
    return boxOfPoints(outline as P[]) ?? { minX: l.at[0], minY: l.at[1], maxX: l.at[0], maxY: l.at[1] };
  }
  const { pos, justify } = localLabelTextPlacement(spin, l.at, size);
  const vertical = spin === "up" || spin === "bottom";
  return textBox(l.net, pos as P, size, justify, vertical ? -90 : 0);
}

function powerSymbolBounds(sch: Schematic, ps: Schematic["power_symbols"][number]): Box {
  const resolved = resolveLibSymbol({ id: ps.id, lib_id: ps.lib_id, at: ps.at, rot: ps.rot, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [ps.pin] }, sch.lib_symbols);
  if (resolved) return resolved.bbox;
  return { minX: ps.at[0] - 600_000, minY: ps.at[1] - 600_000, maxX: ps.at[0] + 600_000, maxY: ps.at[1] + 600_000 };
}

/** The sheet's fields by id, built once per sheet object (a marquee asks for every one of them). */
const fieldCache = new WeakMap<Schematic, Map<string, FieldItem>>();
function fieldIndex(sch: Schematic): Map<string, FieldItem> {
  let index = fieldCache.get(sch);
  if (!index) {
    index = new Map(fieldItems(sch).map((f) => [f.id, f]));
    fieldCache.set(sch, index);
  }
  return index;
}

/** Every item of the sheet, in painting order (later items sit on top of earlier ones). */
export function allItems(sch: Schematic): SchItemRef[] {
  const out: SchItemRef[] = [];
  for (const w of sch.wires) if (w.id) out.push({ id: w.id, kind: "wire" });
  for (const b of sch.bus_entries) if (b.id) out.push({ id: b.id, kind: "bus_entry" });
  for (const j of sch.junctions ?? []) if (j.id) out.push({ id: j.id, kind: "junction" });
  for (const l of sch.lines ?? []) if (l.id) out.push({ id: l.id, kind: "line" });
  for (const g of sch.graphics ?? []) if (g.id) out.push({ id: g.id, kind: "graphic" });
  // A table (`SCH_TABLE`) and, above it, its cells (`SCH_TABLECELL`): a click picks a cell, a window the table.
  for (const t of sch.tables ?? []) {
    if (!t.id) continue;
    out.push({ id: t.id, kind: "table" });
    for (const c of t.cells) if (isSelectableCell(c)) out.push({ id: c.id, kind: "tablecell" });
  }
  for (const s of sch.sheets) if (s.id) out.push({ id: s.id, kind: "sheet" });
  // The pins of those sheets (`SCH_SHEET_PIN_T`), above their sheet: each is selected, moved along the border, edited and deleted on its own.
  for (const p of sheetPinItems(sch)) out.push({ id: p.id, kind: "sheet_pin" });
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
  // The pins of the symbols (`SCH_PIN_T`): each is selected and acted on (Swap Pins) on its own.
  for (const p of pinItems(sch)) out.push({ id: p.id, kind: "pin" });
  // The shown fields (`SCH_FIELD`), on top of the item that has them: each is selected, moved and edited on its own.
  for (const f of fieldItems(sch)) if (isShownField(f.field)) out.push({ id: f.id, kind: "field" });
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
      return n ? { minX: n.at[0] - NOCONNECT_HALF_NM, minY: n.at[1] - NOCONNECT_HALF_NM, maxX: n.at[0] + NOCONNECT_HALF_NM, maxY: n.at[1] + NOCONNECT_HALF_NM } : null;
    }
    case "bus_entry": {
      const b = sch.bus_entries.find((x) => x.id === ref.id);
      return b ? boxOfPoints([b.at, [b.at[0] + b.size[0], b.at[1] + b.size[1]]]) : null;
    }
    case "junction": {
      const j = (sch.junctions ?? []).find((x) => x.id === ref.id);
      return j ? { minX: j.at[0] - JUNCTION_RADIUS_NM, minY: j.at[1] - JUNCTION_RADIUS_NM, maxX: j.at[0] + JUNCTION_RADIUS_NM, maxY: j.at[1] + JUNCTION_RADIUS_NM } : null;
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
    case "table": {
      const t = (sch.tables ?? []).find((x) => x.id === ref.id);
      return t ? { minX: t.bbox[0], minY: t.bbox[1], maxX: t.bbox[2], maxY: t.bbox[3] } : null;
    }
    case "tablecell": {
      const c = (sch.tables ?? []).flatMap((t) => t.cells).find((x) => x.id === ref.id);
      return c ? boxOfPoints(c.corners as unknown as P[]) : null;
    }
    case "field": {
      const f = fieldIndex(sch).get(ref.id);
      return f ? fieldBox(f.field, measureStrokeText) : null;
    }
    case "pin": {
      const pin = pinById(sch, ref.id);
      return pin ? pinBox(pin, measureStrokeText) : null;
    }
    case "sheet_pin": {
      const pin = sheetPinById(sch, ref.id);
      return pin ? sheetPinBox(pin, measureStrokeText) : null;
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
    // A table defers to its cells (`SCH_TABLE::HitTest` is for a window; a click is `SCH_TABLECELL::HitTest`).
    case "table":
      return false;
    case "tablecell": {
      const c = (sch.tables ?? []).flatMap((t) => t.cells).find((x) => x.id === ref.id);
      return c ? cellHitDistance(c, p[0], p[1]) <= tol : false;
    }
    case "pin": {
      const pin = pinById(sch, ref.id);
      return pin ? hitPin(pin, p, tol, measureStrokeText) : false;
    }
    default: {
      const b = itemBounds(sch, ref);
      return b ? pointInBox(p, inflateBox(b, tol)) : false;
    }
  }
}

/**
 * Every item under the point, topmost first. The pins are not among them: a pin is picked ahead of the symbol it belongs to when the point is on it
 * (`GuessSelectionCandidates`: "closest = pin"), by `pickPin`, and by nothing else.
 */
export function hitItems(sch: Schematic, x: number, y: number, tol: number): SchItemRef[] {
  const out: SchItemRef[] = [];
  const all = allItems(sch);
  for (let i = all.length - 1; i >= 0; i--) if (all[i]!.kind !== "pin" && hitItem(sch, all[i]!, [x, y], tol)) out.push(all[i]!);
  return out;
}

/**
 * The ids a marquee selects: items overlapping the box (`crossing`) or sitting wholly inside it. The pins are collected apart and taken only when nothing else is
 * (`SCH_SELECTION_TOOL::SelectMultiple`: "collector = pinsCollector" when the main collector is empty), so a box over a symbol selects the symbol and a box over just
 * some of its pins selects those pins. `pick` is the selection filter: what it keeps out does not count as found.
 */
export function boxItems(sch: Schematic, box: [number, number, number, number], crossing: boolean, pick: (id: string) => boolean = () => true): string[] {
  const b: Box = { minX: box[0], minY: box[1], maxX: box[2], maxY: box[3] };
  const out: string[] = [];
  const pins: string[] = [];
  for (const ref of allItems(sch)) {
    if (!pick(ref.id)) continue;
    // A window selects a table whole, not its cells (`SELECTION_TOOL::FilterCollectorForTableCells`).
    if (ref.kind === "tablecell") continue;
    const ib = itemBounds(sch, ref);
    if (!ib) continue;
    if (crossing ? boxesOverlap(ib, b) : boxEncloses(b, ib)) (ref.kind === "pin" ? pins : out).push(ref.id);
  }
  return out.length > 0 ? out : pins;
}
