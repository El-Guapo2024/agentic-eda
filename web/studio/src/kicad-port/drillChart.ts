// KiCad 10's drill charts and drill maps on the canvas and in the Properties panel (KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates them). The chart's
// table is a board table and is drawn as one; the server builds its rows (crates/ops/src/drill_chart.rs) and the marks of every hole (`ResolveDrillSymbolsByItem`). What is here:
//
//   common/plotters/drill_markers.cpp   DRILL_MARKERS::BuildMarker, CuratedShape        the strokes of a mark: [`buildMarker`], [`curatedShape`]
//   pcbnew/pcb_painter.cpp              PCB_PAINTER::drawChartSymbols                    the shape in each row's symbol cell: [`chartSymbols`]
//                                       PCB_PAINTER::draw( PCB_DRILL_MAP ), drawDrillMarks   the board outline slid by the offset, the marks at the holes: [`mapMarks`]
//   pcbnew/pcb_drill_map.cpp            PCB_DRILL_MAP::GetSymbolExtent, GetBoundingBox, HitTest   [`symbolExtent`], [`mapBounds`], [`mapHitDistance`]
//   pcbnew/widgets/pcb_properties_panel.cpp   the Hole Span choices                       [`spanChoices`]
import type { BoardDrillChart, BoardDrillMap, BoardDrillMark, BoardDrillProfile, BoardDrillSpan } from "../api/pcbItemTypes";
import type { BoardState, BoardTable } from "../api/types";

/** `marker_patterns`: bit order O Square Lozenge - | \ / (octal). */
const MARKER_PATTERNS: readonly number[] = [
  0o003, 0o100, 0o014, 0o040, 0o020, 0o103, 0o017, 0o043, 0o023, 0o114, 0o140, 0o120, 0o054, 0o034, 0o060, 0o117, 0o143, 0o123, 0o057, 0o037, 0o063, 0o154, 0o134, 0o074, 0o174, 0o163,
  0o157, 0o137, 0o077, 0o177, 0o110, 0o104, 0o101, 0o050, 0o044, 0o041, 0o030, 0o024, 0o021, 0o150, 0o144, 0o141, 0o130, 0o124, 0o121, 0o070, 0o064, 0o061, 0o170, 0o164, 0o161, 0o102,
  0o042, 0o022, 0o142, 0o122, 0o062, 0o162,
];

/** `curated`: the single- and two-part marks, the ones assignment works through first. */
const CURATED: readonly number[] = [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 16];

export type MarkerPart = { kind: "segment" | "polyline"; points: [number, number][] } | { kind: "circle"; r: number };

/** `CuratedShape`: the pattern of the curated mark `i` (0 for one out of range). */
export function curatedShape(i: number): number {
  return i >= 0 && i < CURATED.length ? CURATED[i]! : 0;
}

/** `BuildMarker`: the strokes of mark `shapeId` of radius `r` at (x, y), in the order they are drawn; a shape out of range is a plain circle. */
export function buildMarker(x: number, y: number, r: number, shapeId: number): MarkerPart[] {
  if (shapeId < 0 || shapeId >= MARKER_PATTERNS.length) return [{ kind: "circle", r }];
  const pat = MARKER_PATTERNS[shapeId]!;
  const out: MarkerPart[] = [];
  const seg = (a: [number, number], b: [number, number]) => out.push({ kind: "segment", points: [a, b] });
  if (pat & 0o001) seg([x - r, y - r], [x + r, y + r]);
  if (pat & 0o002) seg([x + r, y - r], [x - r, y + r]);
  if (pat & 0o004) seg([x, y - r], [x, y + r]);
  if (pat & 0o010) seg([x - r, y], [x + r, y]);
  if (pat & 0o020) out.push({ kind: "polyline", points: [[x, y + r], [x + r, y], [x, y - r], [x - r, y], [x, y + r]] });
  if (pat & 0o040) {
    // "The square is inscribed in the marker circle, so its half-side is r/sqrt(2)"
    const s = Math.round(r / 1.4142);
    out.push({ kind: "polyline", points: [[x + s, y + s], [x + s, y - s], [x - s, y - s], [x - s, y + s], [x + s, y + s]] });
  }
  if (pat & 0o100) out.push({ kind: "circle", r });
  return out;
}

export const DEFAULT_PROFILE: BoardDrillProfile = { size_um: 1500, width_um: 150, mark_policy: "shapes_then_letters", group_by: [], freeze_assignments: true };

/** `drawChartSymbols`: the mark of each row that has a shape, centred in its symbol cell and fitted to it ("so a tall symbol size cannot spill into the neighbouring row"). */
export function chartSymbols(chart: BoardDrillChart, table: BoardTable, profile: BoardDrillProfile): { x: number; y: number; r: number; shape: number }[] {
  const col = chart.symbol_column ?? -1;
  if (col < 0 || !chart.row_shapes) return [];
  const out: { x: number; y: number; r: number; shape: number }[] = [];
  for (const [rowKey, shapeIndex] of Object.entries(chart.row_shapes)) {
    const row = Number(rowKey);
    const cell = table.cells.find((c) => c.row === row && c.col === col);
    if (!cell) continue;
    const xs = cell.corners.map((p) => p[0]);
    const ys = cell.corners.map((p) => p[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const r = Math.min(Math.trunc(profile.size_um / 2), Math.trunc(Math.min(x1 - x0, y1 - y0) / 3));
    if (r <= 0) continue;
    out.push({ x: Math.trunc((x0 + x1) / 2), y: Math.trunc((y0 + y1) / 2), r, shape: curatedShape(shapeIndex) });
  }
  return out;
}

function sameSpan(a: BoardDrillSpan, b: BoardDrillSpan): boolean {
  return a.start === b.start && a.end === b.end && !!a.backdrill === !!b.backdrill && !!a.npth === !!b.npth;
}

/** `drawDrillMarks`' filter and place: the marks of the span the map shows (every span when it shows all), each slid by the map's offset. */
export function mapMarks(map: BoardDrillMap, marks: readonly BoardDrillMark[]): (BoardDrillMark & { x: number; y: number })[] {
  return marks.filter((m) => !map.span || sameSpan(m.span, map.span)).map((m) => ({ ...m, x: m.at.x + map.offset.x, y: m.at.y + map.offset.y }));
}

/** The text a letter or size mark draws: the letter, or the hole's own diameter in millimetres to two places ("The hole's own diameter, not the glyph size"). */
export function markText(m: BoardDrillMark): string {
  return m.mode === "letter" ? m.letter : (m.diameter_um / 1000).toFixed(2);
}

/** `GetSymbolExtent`: "Numeric marks can reach three character sizes either side of their hole." */
export function symbolExtent(map: BoardDrillMap, profile: BoardDrillProfile): number {
  return 3 * map.size_um + 2 * profile.width_um;
}

/** `GetBoundingBox`: the holes' extent grown by the marks' reach, merged with the board outline's box, slid by the offset. */
export function mapBounds(map: BoardDrillMap, marks: readonly BoardDrillMark[], profile: BoardDrillProfile, outlineBox: readonly [number, number, number, number] | null): [number, number, number, number] | null {
  let box: [number, number, number, number] | null = null;
  if (marks.length > 0) {
    const e = symbolExtent(map, profile);
    const xs = marks.map((m) => m.at.x);
    const ys = marks.map((m) => m.at.y);
    box = [Math.min(...xs) - e, Math.min(...ys) - e, Math.max(...xs) + e, Math.max(...ys) + e];
  }
  if (outlineBox) box = box ? [Math.min(box[0], outlineBox[0]), Math.min(box[1], outlineBox[1]), Math.max(box[2], outlineBox[2]), Math.max(box[3], outlineBox[3])] : [...outlineBox];
  if (!box) return null;
  return [box[0] + map.offset.x, box[1] + map.offset.y, box[2] + map.offset.x, box[3] + map.offset.y];
}

function segDist(px: number, py: number, a: readonly [number, number], b: readonly [number, number]): number {
  const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - a[0]) * dx + (py - a[1]) * dy) / len2));
  return Math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy));
}

/**
 * `PCB_DRILL_MAP::HitTest`: "Only the marks are clickable" -- a click within half the marks' extent of one, square, hits -- and the outline it draws (`PointOnEdge`). The distance
 * to the nearest of them; `Infinity` when nothing is near.
 */
export function mapHitDistance(map: BoardDrillMap, marks: readonly BoardDrillMark[], profile: BoardDrillProfile, rings: readonly (readonly (readonly [number, number])[])[], x: number, y: number, accuracy: number): number {
  const reach = symbolExtent(map, profile) / 2 + accuracy;
  let best = Infinity;
  for (const m of mapMarks(map, marks)) {
    const d = Math.max(Math.abs(m.x - x), Math.abs(m.y - y));
    if (d <= reach) best = Math.min(best, Math.hypot(m.x - x, m.y - y));
  }
  for (const outline of rings) {
    for (let i = 0; i < outline.length; i++) {
      const a = outline[i]!;
      const b = outline[(i + 1) % outline.length]!;
      const d = segDist(x, y, [a[0] + map.offset.x, a[1] + map.offset.y], [b[0] + map.offset.x, b[1] + map.offset.y]);
      if (d <= accuracy) best = Math.min(best, d);
    }
  }
  return best;
}

/** The Hole Span choices: "All spans" (-1), then each span the board has, `top - bottom`. */
export function spanChoices(spans: readonly BoardDrillSpan[]): { label: string; value: number }[] {
  return [{ label: "All spans", value: -1 }, ...spans.map((s, i) => ({ label: `${s.start} - ${s.end}${s.backdrill ? " (backdrill)" : ""}`, value: i }))];
}

/** `GetSpanChoice`: -1 for every span, or one the board no longer has ("A span the stackup no longer has reads as every span"). */
export function spanChoiceOf(map: BoardDrillMap, spans: readonly BoardDrillSpan[]): number {
  if (!map.span) return -1;
  return spans.findIndex((s) => sameSpan(s, map.span!));
}

/** `SetSpanChoice`. */
export function withSpanChoice(map: BoardDrillMap, spans: readonly BoardDrillSpan[], index: number): BoardDrillMap {
  const s = index >= 0 ? spans[index] : undefined;
  return { ...map, span: s ? { ...s } : null };
}

/** `GetBoardOutlines`: the board's outlines and their holes as KiCad builds them from Edge.Cuts (`GetBoardPolygonOutlines`), or the outline polygon of an older state. */
export function outlineRings(board: Partial<Pick<BoardState, "outline" | "outline_polys">>): [number, number][][] {
  if (board.outline_polys && board.outline_polys.length > 0) return board.outline_polys.flatMap((p) => [p.outer, ...p.holes]);
  return board.outline ? [board.outline] : [];
}

export function outlineBox(board: Partial<Pick<BoardState, "outline" | "outline_polys">>): [number, number, number, number] | null {
  const o = outlineRings(board).flat();
  if (o.length === 0) return null;
  const xs = o.map((p) => p[0]);
  const ys = o.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/** `pickDocumentationLayer`'s candidates (`DocumentationLayers().Seq()`), in layer order. */
export const DOCUMENTATION_LAYERS: readonly string[] = ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "B.Fab", "F.Fab", ...Array.from({ length: 45 }, (_, i) => `User.${i + 1}`)];

export function isDocumentationLayer(layer: string): boolean {
  return DOCUMENTATION_LAYERS.includes(layer);
}

/** `pickDocumentationLayer`: the active layer when it is available, else the first documentation layer that is; null when none is. */
export function pickDocumentationLayer(active: string | null, enabled: readonly string[], available: (layer: string) => boolean): string | null {
  const ok = (l: string) => isDocumentationLayer(l) && enabled.includes(l) && available(l);
  if (active && ok(active)) return active;
  return DOCUMENTATION_LAYERS.find(ok) ?? null;
}
