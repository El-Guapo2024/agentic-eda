import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardDrillMap, BoardDrillMark } from "../api/pcbItemTypes";
import type { BoardTable } from "../api/types";
import { buildMarker, chartSymbols, curatedShape, DEFAULT_PROFILE, mapBounds, mapHitDistance, mapMarks, markText, pickDocumentationLayer, spanChoiceOf, spanChoices, withSpanChoice } from "./drillChart";

const mark = (x: number, y: number, extra: Partial<BoardDrillMark> = {}): BoardDrillMark => ({
  at: { x, y },
  span: { start: "F.Cu", end: "B.Cu" },
  mode: "shape",
  shape_index: 0,
  letter: "",
  diameter_nm: 300_000,
  size_nm: [300_000, 300_000],
  orientation_deg: 0,
  is_slot: false,
  ...extra,
});
const map = (extra: Partial<BoardDrillMap> = {}): BoardDrillMap => ({ id: "dmap_1", layer: "Dwgs.User", offset: { x: 50_000_000, y: 0 }, size_nm: 1_500_000, ...extra });

test("the marks are DRILL_MARKERS' strokes: an X, an O, a +, the inscribed square, the lozenge; out of range a circle", () => {
  assert.deepEqual(buildMarker(0, 0, 10, curatedShape(0)), [
    { kind: "segment", points: [[-10_000, -10_000], [10_000, 10_000]] },
    { kind: "segment", points: [[10_000, -10_000], [-10_000, 10_000]] },
  ]);
  assert.deepEqual(buildMarker(0, 0, 10, curatedShape(1)), [{ kind: "circle", r: 10 }]);
  assert.equal(buildMarker(0, 0, 10, curatedShape(3))[0]!.kind, "polyline");
  // `KiROUND( r / 1.4142 )`.
  assert.deepEqual((buildMarker(0, 0, 10, curatedShape(3))[0] as { points: number[][] }).points[0], [7000, 7000]);
  assert.equal(curatedShape(11), 16_000, "the last curated mark: X O +");
  assert.equal(curatedShape(12), 0, "out of the curated set");
  assert.deepEqual(buildMarker(0, 0, 5, 58_000), [{ kind: "circle", r: 5 }]);
});

test("a chart's shape sits in its symbol cell, fitted to it", () => {
  const cell = (row: number, col: number, x0: number, y0: number, x1: number, y1: number) => ({ id: `t:${row}:${col}`, row, col, corners: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]] }) as unknown as BoardTable["cells"][number];
  const table = { id: "tbl_1", cells: [cell(1, 0, 0, 0, 16_000_000, 2_100_000), cell(1, 1, 16_000_000, 0, 32_000_000, 2_100_000)] } as unknown as BoardTable;
  const s = chartSymbols({ table_id: "tbl_1", columns: [], symbol_column: 0, row_shapes: { "1": 2000 } }, table, DEFAULT_PROFILE);
  assert.deepEqual(s, [{ x: 8_000_000, y: 1_050_000, r: 700, shape: 2000 }], "the cell's third (700) under half the mark (750)");
  assert.deepEqual(chartSymbols({ table_id: "tbl_1", columns: [], symbol_column: -1, row_shapes: { "1": 2000 } }, table, DEFAULT_PROFILE), []);
});

test("a map slides its marks by its offset, shows one span or all, and is hit on a mark or its outline", () => {
  const marks = [mark(1_000_000, 1_000_000), mark(5_000_000, 1_000_000, { span: { start: "F.Cu", end: "In1.Cu" }, mode: "letter", letter: "A" })];
  assert.deepEqual(mapMarks(map(), marks).map((m) => [m.x, m.y]), [[51_000_000, 1_000_000], [55_000_000, 1_000_000]]);
  assert.equal(mapMarks(map({ span: { start: "F.Cu", end: "B.Cu" } }), marks).length, 1);
  assert.equal(markText(marks[1]!), "A");
  assert.equal(markText(mark(0, 0, { mode: "size_text", diameter_nm: 1_143_000 })), "1.14");
  // Reach: half of 3 x 1.5 mm + 2 x 0.15 mm.
  assert.equal(mapHitDistance(map(), marks, DEFAULT_PROFILE, [], 51_000_000, 1_000_000 + 2_400_000, 0), 2_400_000);
  assert.equal(mapHitDistance(map(), marks, DEFAULT_PROFILE, [], 51_000_000, 1_000_000 + 2_500_000, 0), Infinity);
  assert.equal(mapHitDistance(map(), [], DEFAULT_PROFILE, [[[0, 0], [10_000_000, 0], [10_000_000, 10_000_000]]], 55_000_000, 0, 10_000), 0, "on the outline, slid");
  assert.deepEqual(mapBounds(map(), marks, DEFAULT_PROFILE, [0, 0, 20_000_000, 10_000_000]), [46_200_000, -3_800_000, 70_000_000, 10_000_000], "the holes grown by the marks' extent, with the outline's box");
});

test("Hole Span lists every span after All spans, and a span the board lost reads as all", () => {
  const spans = [{ start: "F.Cu", end: "B.Cu" }, { start: "F.Cu", end: "In1.Cu" }];
  assert.deepEqual(spanChoices(spans).map((c) => c.label), ["All spans", "F.Cu - B.Cu", "F.Cu - In1.Cu"]);
  const one = withSpanChoice(map(), spans, 1);
  assert.equal(spanChoiceOf(one, spans), 1);
  assert.equal(spanChoiceOf(withSpanChoice(one, spans, -1), spans), -1);
  assert.equal(spanChoiceOf(map({ span: { start: "In1.Cu", end: "In2.Cu" } }), spans), -1);
});

test("the documentation layer is the active one when it can be, else the first that can", () => {
  const enabled = ["F.Cu", "B.Cu", "Dwgs.User", "Cmts.User", "F.Fab", "B.Fab"];
  assert.equal(pickDocumentationLayer("Cmts.User", enabled, () => true), "Cmts.User");
  assert.equal(pickDocumentationLayer("F.SilkS", enabled, () => true), "Dwgs.User");
  assert.equal(pickDocumentationLayer(null, enabled, (l) => l !== "Dwgs.User" && l !== "Cmts.User"), "B.Fab", "B.Fab comes before F.Fab");
  assert.equal(pickDocumentationLayer(null, ["F.Cu"], () => true), null);
});
