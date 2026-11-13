import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardTable, BoardTableCell, Cmd, Schematic, SchLine, Sheet } from "../api/types";
import type { SchGraphic } from "../api/schEditTypes";
import { RECT_BOTRIGHT, RECT_LEFT, RECT_RIGHT, RECT_TOP, RECT_TOPLEFT } from "./eePointEdit";
import {
  bumpToNextGrid,
  cellDrag,
  cellEditPoints,
  COL_WIDTH,
  imageDrag,
  imageEditPoints,
  imageState,
  lineLinks,
  MIN_SHEET_WIDTH_NM,
  REFIMG_ORIGIN,
  ROW_HEIGHT,
  schItemPointEditor,
  sheetMinHeight,
  tableWithChange,
} from "./schItemPointEdit";

const MM = 1_000_000;
const MODE = "keep_center_adjust_angle_radius" as const;
const MIL = 25_400;
/** A text `0.6` of its size wide per character, for the pins' boxes. */
const measure = (text: string, size: number) => text.length * size * 0.6;

function sch(over: Partial<Schematic>): Schematic {
  return { lib_symbols: {}, symbols: [], power_symbols: [], wires: [], no_connects: [], labels: [], texts: [], title_block: null, bus_entries: [], sheets: [], sheet_path: [], ...over } as Schematic;
}

const at = (x: number, y: number): [number, number] => [x * MM, y * MM];

// ------------------------------------------------------------------------------------------------------------------------------------ sheet

const sheet: Sheet = {
  id: "sheet_sub",
  name: "Sub",
  file: "sub.kicad_sch",
  at: at(100, 50),
  size: at(40, 30),
  pins: [
    { id: "shpin_in", name: "IN", shape: "input", at: at(100, 60) },
    { id: "shpin_out", name: "OUT", shape: "output", at: at(140, 70) },
  ],
};
const sheetSch = sch({
  sheets: [sheet],
  wires: [
    { id: "wire_in", net: "SIG", pins: [], pts: [at(80, 60), at(100, 60)], bus: false },
    { id: "wire_out", net: "RES", pins: [], pts: [at(140, 70), at(160, 70)], bus: false },
  ],
  no_connects: [],
});

test("bumpToNextGrid pushes a value out to the next 50 mil line, away from it when it is on one", () => {
  const g = 50 * MIL;
  assert.equal(bumpToNextGrid(2 * g, 1), 3 * g);
  assert.equal(bumpToNextGrid(2 * g, -1), g);
  assert.equal(bumpToNextGrid(2 * g + 7, -1), 2 * g);
  assert.equal(bumpToNextGrid(2 * g + 7, 1), 3 * g);
  // C++'s truncation toward zero for a negative value
  assert.equal(bumpToNextGrid(-g - 7, -1), -g);
  assert.equal(bumpToNextGrid(-100, -1), 0);
});

test("a sheet's points are its corners and the middles of its sides; its right side drags its pins and their wires along", () => {
  const ed = schItemPointEditor(sheetSch, "sheet_sub", null, MODE, measure)!;
  assert.equal(ed.kind, "sheet");
  assert.deepEqual(ed.edit.points.map((p) => p.pos), [at(100, 50), at(140, 50), at(100, 80), at(140, 80)]);
  assert.equal(ed.edit.lines.length, 4);
  assert.deepEqual(ed.cmds, []);
  // the right side's middle to x = 150 mm
  const dragged = schItemPointEditor(sheetSch, "sheet_sub", { handle: { kind: "line", index: RECT_RIGHT }, to: at(150, 61) }, MODE, measure)!;
  assert.deepEqual(dragged.cmds, [{ op: "sch_edit", verb: "resize_sheet", id: "sheet_sub", at: { x: 100 * MM, y: 50 * MM }, width_nm: 50 * MM, height_nm: 30 * MM }]);
  const shown = dragged.shown(sheetSch);
  assert.deepEqual(shown.sheets[0]!.pins.map((p) => p.at), [at(100, 60), at(150, 70)]);
  assert.deepEqual(shown.wires.map((w) => w.pts), [[at(80, 60), at(100, 60)], [at(150, 70), at(160, 70)]]);
  assert.deepEqual(dragged.edit.points[RECT_BOTRIGHT]!.pos, at(150, 80));
});

test("the left and top sides move the corner without the pins, then each pin is on its own side again", () => {
  const left = schItemPointEditor(sheetSch, "sheet_sub", { handle: { kind: "line", index: RECT_LEFT }, to: at(90, 70) }, MODE, measure)!;
  assert.deepEqual(left.cmds, [{ op: "sch_edit", verb: "resize_sheet", id: "sheet_sub", at: { x: 90 * MM, y: 50 * MM }, width_nm: 50 * MM, height_nm: 30 * MM }]);
  const s = left.shown(sheetSch);
  assert.deepEqual(s.sheets[0]!.pins.map((p) => p.at), [at(90, 60), at(140, 70)]);
  assert.deepEqual(s.wires[0]!.pts, [at(80, 60), at(90, 60)]);
  // a no-connect on a pin goes with it (`m_noConnects`)
  const nc: Schematic = { ...sheetSch, no_connects: [{ id: "nc", at: at(100, 60) }] };
  assert.deepEqual(schItemPointEditor(nc, "sheet_sub", { handle: { kind: "line", index: RECT_LEFT }, to: at(90, 70) }, MODE, measure)!.shown(nc).no_connects.map((n) => n.at), [at(90, 60)]);
  // the top side down: the corner's y, the bottom stays
  const top = schItemPointEditor(sheetSch, "sheet_sub", { handle: { kind: "line", index: RECT_TOP }, to: at(110, 55) }, MODE, measure)!;
  assert.deepEqual(top.cmds, [{ op: "sch_edit", verb: "resize_sheet", id: "sheet_sub", at: { x: 100 * MM, y: 55 * MM }, width_nm: 40 * MM, height_nm: 25 * MM }]);
});

test("a corner stops where the pins with their text still fit, on the 50 mil grid, and never under 500 x 150 mils", () => {
  // the bottom-right corner dragged up and left past everything
  const ed = schItemPointEditor(sheetSch, "sheet_sub", { handle: { kind: "point", index: RECT_BOTRIGHT }, to: at(101, 51) }, MODE, measure)!;
  const cmd = ed.cmds[0] as Extract<Cmd, { op: "sch_edit" }> & { width_nm: number; height_nm: number };
  // no pin on the top or bottom side: the width is `MIN_SHEET_WIDTH`
  assert.equal(cmd.width_nm, MIN_SHEET_WIDTH_NM);
  // the height is from the top to the grid line past the lowest pin's box (OUT at 70 mm)
  assert.equal(cmd.height_nm, sheetMinHeight(sheetSch, sheet, true, measure));
  assert.ok(50 * MM + cmd.height_nm > 70 * MM);
  assert.equal((50 * MM + cmd.height_nm) % (50 * MIL), 0);
  // from the bottom (the top-left corner dragged): from the grid line above the highest pin's box (IN at 60 mm) to the bottom
  const fromBottom = sheetMinHeight(sheetSch, sheet, false, measure);
  assert.ok(80 * MM - fromBottom < 60 * MM);
  const tl = schItemPointEditor(sheetSch, "sheet_sub", { handle: { kind: "point", index: RECT_TOPLEFT }, to: at(139, 79) }, MODE, measure)!;
  const c = tl.cmds[0] as unknown as { at: { x: number; y: number }; height_nm: number };
  assert.equal(c.height_nm, fromBottom);
  assert.equal(c.at.y, 80 * MM - fromBottom);
  // the pins stay on the sides, clamped to their ends
  assert.ok(tl.shown(sheetSch).sheets[0]!.pins.every((p) => p.at[1] >= c.at.y && p.at[1] <= 80 * MM));
});

// ------------------------------------------------------------------------------------------------------------------------------------ image

const image = (offset?: { x: number; y: number }) => {
  const g: SchGraphic = { id: "img", shape: { type: "image", at: { x: 50 * MM, y: 50 * MM }, scale_ppm: 1_000_000, px: [100, 50], ppi: 100 } };
  return sch({ graphics: [g], ...(offset ? { image_offsets: { img: offset } } : {}) });
};

test("an image's points are its corners and its transform origin", () => {
  const s = image();
  const im = imageState(s.graphics![0]!, s)!;
  assert.deepEqual(im.size, [25_400_000, 12_700_000]);
  assert.deepEqual(imageEditPoints(im).points.map((p) => p.pos), [[37_300_000, 43_650_000], [62_700_000, 43_650_000], [37_300_000, 56_350_000], [62_700_000, 56_350_000], [50 * MM, 50 * MM]]);
});

test("a corner scales the image about its origin by how far it is from it, and stops on the origin when pulled across", () => {
  const s = image();
  const ed = schItemPointEditor(s, "img", { handle: { kind: "point", index: RECT_BOTRIGHT }, to: [75_400_000, 62_700_000] }, MODE, measure)!;
  assert.deepEqual(ed.cmds, [{ op: "sch_edit", verb: "edit_graphic", id: "img", graphic: { shape: { type: "image", at: { x: 50 * MM, y: 50 * MM }, scale_ppm: 2_000_000 } } }]);
  // across the origin: the corner is clamped onto it, the size to 50 mils (the smaller ratio of the two sides wins)
  const im = imageState(s.graphics![0]!, s)!;
  const crossed = imageDrag(im, { kind: "point", index: RECT_BOTRIGHT }, [40 * MM, 40 * MM])!;
  assert.equal(crossed.scalePpm, 50_000);
});

test("with the origin at a corner, the opposite corner scales about it; the origin's drag moves it alone", () => {
  const s = image({ x: -12_700_000, y: -6_350_000 });
  const ed = schItemPointEditor(s, "img", { handle: { kind: "point", index: RECT_BOTRIGHT }, to: [75_400_000, 62_700_000] }, MODE, measure)!;
  assert.deepEqual(ed.cmds, [
    { op: "sch_edit", verb: "edit_graphic", id: "img", graphic: { shape: { type: "image", at: { x: 56_350_000, y: 53_175_000 }, scale_ppm: 1_500_000 } } },
    { op: "sch_edit", verb: "set_image_offset", id: "img", offset: { x: -19_050_000, y: -9_525_000 } },
  ]);
  const origin = schItemPointEditor(s, "img", { handle: { kind: "point", index: REFIMG_ORIGIN }, to: [55 * MM, 52 * MM] }, MODE, measure)!;
  assert.deepEqual(origin.cmds, [{ op: "sch_edit", verb: "set_image_offset", id: "img", offset: { x: 5 * MM, y: 2 * MM } }]);
});

// ------------------------------------------------------------------------------------------------------------------------------------ graphic line

const ln = (id: string, a: [number, number], b: [number, number]): SchLine => ({ id, pts: [a, b], width_nm: 0 });

test("a graphic line's ends are its points; another line's end on one of them follows it", () => {
  const lines = [ln("a", at(0, 0), at(10, 0)), ln("b", at(10, 0), at(10, 10)), ln("c", at(0, 5), at(0, 0))];
  const s = sch({ lines });
  assert.deepEqual(lineLinks(lines, lines[0]!), [{ id: "c", end: "end" }, { id: "b", end: "start" }]);
  const end = schItemPointEditor(s, "a", { handle: { kind: "point", index: 1 }, to: at(12, 0) }, MODE, measure)!;
  assert.equal(end.kind, "line");
  assert.deepEqual(end.cmds, [
    { op: "sch_edit", verb: "set_line_ends", id: "a", end: { x: 12 * MM, y: 0 } },
    { op: "sch_edit", verb: "set_line_ends", id: "b", start: { x: 12 * MM, y: 0 } },
  ]);
  assert.deepEqual(end.shown(s).lines!.map((l) => l.pts), [[at(0, 0), at(12, 0)], [at(12, 0), at(10, 10)], [at(0, 5), at(0, 0)]]);
  const start = schItemPointEditor(s, "a", { handle: { kind: "point", index: 0 }, to: at(-2, 0) }, MODE, measure)!;
  assert.deepEqual(start.cmds, [
    { op: "sch_edit", verb: "set_line_ends", id: "a", start: { x: -2 * MM, y: 0 } },
    { op: "sch_edit", verb: "set_line_ends", id: "c", end: { x: -2 * MM, y: 0 } },
  ]);
  // a line on both ends of this one counts for its start only (the `else if` chain)
  assert.deepEqual(lineLinks([ln("a", at(0, 0), at(10, 0)), ln("d", at(0, 0), at(10, 0))], ln("a", at(0, 0), at(10, 0))), [{ id: "d", end: "start" }, null]);
  // its end onto its start is committed as KiCad commits it: a zero-length graphic line
  assert.deepEqual(schItemPointEditor(sch({ lines: [ln("a", at(0, 0), at(10, 0))] }), "a", { handle: { kind: "point", index: 1 }, to: at(0, 0) }, MODE, measure)!.cmds, [
    { op: "sch_edit", verb: "set_line_ends", id: "a", end: { x: 0, y: 0 } },
  ]);
});

// ------------------------------------------------------------------------------------------------------------------------------------ table cell

function table(angle: number, spans: Array<[number, number]> = []): BoardTable {
  const widths = [10 * MM, 20 * MM];
  const heights = [5 * MM, 5 * MM];
  const cells: BoardTableCell[] = [];
  for (let r = 0; r < 2; r++)
    for (let c = 0; c < 2; c++) {
      const span = spans.find(([rr, cc]) => rr === r && cc === c);
      cells.push({ id: `tbl:${r}:${c}`, row: r, col: c, text: "", col_span: span ? 2 : 1, row_span: 1, corners: [[0, 0], [0, 0], [0, 0], [0, 0]] } as unknown as BoardTableCell);
    }
  return {
    id: "tbl",
    layer: "",
    x: 0,
    y: 0,
    angle,
    rows: 2,
    cols: 2,
    col_widths: widths,
    row_heights: heights,
    bbox: [0, 0, 30 * MM, 10 * MM],
    borders: [
      { a: [10 * MM, 0], b: [10 * MM, 10 * MM], width: 0, style: "solid", color: null },
      { a: [30 * MM, 0], b: [30 * MM, 10 * MM], width: 0, style: "solid", color: null },
    ],
    cells,
  } as unknown as BoardTable;
}

test("a table cell's points are the middles of its right side and its bottom; they set its column's width and its row's height", () => {
  const t = table(0);
  const cell = t.cells[0]!;
  assert.deepEqual(cellEditPoints(t, cell).points.map((p) => p.pos), [at(10, 2.5), at(5, 5)]);
  assert.deepEqual(cellDrag(t, cell, { kind: "point", index: COL_WIDTH }, at(15, 3)), { col: 0, width: 15 * MM });
  assert.deepEqual(cellDrag(t, cell, { kind: "point", index: ROW_HEIGHT }, at(4, 8)), { row: 0, height: 8 * MM });
  const s = sch({ tables: [t] });
  const ed = schItemPointEditor(s, "tbl:0:0", { handle: { kind: "point", index: COL_WIDTH }, to: at(15, 3) }, MODE, measure)!;
  assert.deepEqual(ed.cmds, [{ op: "sch_table", verb: "edit_cells", id: "tbl", cells: [[0, 0]], edit: { col_width_nm: 15 * MM } }]);
  // the preview: the lines past the column move with its far side
  const shown = ed.shown(s).tables![0]!;
  assert.deepEqual(shown.borders.map((b) => b.a[0]), [15 * MM, 35 * MM]);
  assert.equal(shown.bbox[2], 35 * MM);
  assert.deepEqual(tableWithChange(t, { col: 0, width: 10 * MM }), t);
});

test("a merged cell sets the last column it spans, the others kept; a turned table's cell reads its points in its own frame", () => {
  const t = table(0, [[0, 0]]);
  const cell = t.cells[0]!;
  assert.deepEqual(cellEditPoints(t, cell).points[COL_WIDTH]!.pos, at(30, 2.5));
  assert.deepEqual(cellDrag(t, cell, { kind: "point", index: COL_WIDTH }, at(40, 2.5)), { col: 1, width: 30 * MM });
  const ed = schItemPointEditor(sch({ tables: [t] }), "tbl:0:0", { handle: { kind: "point", index: COL_WIDTH }, to: at(40, 2.5) }, MODE, measure)!;
  assert.deepEqual(ed.cmds, [{ op: "sch_table", verb: "edit_cells", id: "tbl", cells: [[0, 1]], edit: { col_width_nm: 30 * MM } }]);
  // turned a quarter: (x, y) -> (y, -x), the columns run up the sheet and the rows to the right
  const r = table(90_000);
  const rc = r.cells[0]!;
  assert.deepEqual(cellEditPoints(r, rc).points.map((p) => p.pos), [at(5, -5), at(2.5, -10)]);
  assert.deepEqual(cellDrag(r, rc, { kind: "point", index: ROW_HEIGHT }, at(2.5, -14)), { col: 0, width: 14 * MM });
  assert.deepEqual(cellDrag(r, rc, { kind: "point", index: COL_WIDTH }, at(7, -5)), { row: 0, height: 7 * MM });
});

test("an item of no behaviour has no point editor", () => {
  assert.equal(schItemPointEditor(sheetSch, "wire_in", null, MODE, measure), null);
  assert.equal(schItemPointEditor(sheetSch, "nope", null, MODE, measure), null);
});
