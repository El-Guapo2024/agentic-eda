import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardDrillChart } from "../api/pcbItemTypes";
import type { BoardTable, BoardTableCell } from "../api/types";
import {
  chartRowCells,
  columnLabel,
  columnRows,
  defaultColumns,
  drillChartEditCmd,
  drillChartForm,
  groupCells,
  isDataRow,
  markEditCmd,
  moveColumnRow,
  onBorderChecked,
  readTemplate,
  shownColumns,
  templateJson,
  validateColumns,
  type DrillGroupRow,
} from "./drillChartDialog";

const stroke = (width: number) => ({ width, style: "solid" as const, color: null });

function cell(row: number, col: number): BoardTableCell {
  const [x0, y0] = [col * 10_000_000, row * 3_000_000];
  return {
    id: `tbl_a:${row}:${col}`,
    row,
    col,
    text: "",
    col_span: 1,
    row_span: 1,
    corners: [
      [x0, y0],
      [x0 + 10_000_000, y0],
      [x0 + 10_000_000, y0 + 3_000_000],
      [x0, y0 + 3_000_000],
    ],
  } as unknown as BoardTableCell;
}

/** A chart of two columns: the heading, two data rows and the totals. */
function chart(): { chart: BoardDrillChart; table: BoardTable } {
  const cells = [0, 1, 2, 3].flatMap((r) => [cell(r, 0), cell(r, 1)]);
  const table = { id: "tbl_a", layer: "Dwgs.User", rows: 4, cols: 2, cells, border: { external: true, header: true, stroke: stroke(100_000) }, separators: { rows: false, cols: false, stroke: stroke(-1) } } as unknown as BoardTable;
  const c: BoardDrillChart = {
    table_id: "tbl_a",
    units: "mils",
    precision: 2,
    columns: [
      { id: "drill_diameter", heading: "DIA", align: "right" },
      { id: "symbol", heading: "SYM", align: "center", width_nm: 5_000_000 },
    ],
    row_keys: { "1": "k1", "2": "k2" },
    show_totals: true,
  };
  return { chart: c, table };
}

test("the Columns page lists the chart's columns, then the schema's others hidden (setColumns), and moves a row by one", () => {
  const { chart: c } = chart();
  const rows = columnRows(c.columns.map((x) => ({ ...x, width_nm: x.width_nm ?? 0 })));
  assert.equal(rows.length, 12);
  assert.deepEqual(
    rows.slice(0, 3).map((r) => [r.column.id, r.shown]),
    [
      ["drill_diameter", true],
      ["symbol", true],
      ["slot_size", false],
    ]
  );
  assert.equal(rows[2]!.column.heading, "SLOT W x L", "a hidden column is the schema's default");
  assert.equal(columnLabel("drill_diameter"), "Drill diameter", "the name, whatever the heading");
  assert.equal(moveColumnRow(rows, 0, -1), null, "nothing above the first row");
  const moved = moveColumnRow(rows, 0, 1)!;
  assert.deepEqual([moved.rows[0]!.column.id, moved.rows[1]!.column.id, moved.row], ["symbol", "drill_diameter", 1]);
  assert.deepEqual(
    defaultColumns().map((x) => x.id),
    ["symbol", "drill_diameter", "slot_size", "plating", "operation_count", "layer_span"],
    "Reset to Default: the schema's Defaults()"
  );
});

test("OK sends one edit_drill_chart and the lock, or KiCad's message", () => {
  const { chart: c, table } = chart();
  const f = drillChartForm(c, table, false, ["size", "plating"]);
  assert.equal(f.units, "mils");
  assert.equal(f.separatorsWidthNm, -1);
  const out = drillChartEditCmd("tbl_a", { ...f, locked: true, groupBy: ["plating", "size", "span"] }, false);
  assert.ok("cmds" in out);
  const [edit, lock] = out.cmds as [{ chart: { columns: { id: string }[]; group_by: string[]; units: string } }, { op: string; locked: boolean }];
  assert.deepEqual(
    edit.chart.columns.map((x) => x.id),
    ["drill_diameter", "symbol"]
  );
  assert.deepEqual(edit.chart.group_by, ["size", "plating", "span"], "the checklist's order");
  assert.deepEqual([lock.op, lock.locked], ["set_locked", true]);
  const none = drillChartEditCmd("tbl_a", { ...f, rows: f.rows.map((r) => ({ ...r, shown: false })) }, false);
  assert.deepEqual(none, { error: "A table needs at least one column." });
  const wide = drillChartEditCmd("tbl_a", { ...f, rows: f.rows.map((r) => ({ shown: true, column: { ...r.column, width_nm: 5e9 } })) }, false);
  assert.deepEqual(wide, { error: "The columns are too wide in total." });
  assert.equal(shownColumns(f.rows)[1]!.width_nm, 5_000_000);
});

test("onBorderChecked: a width switched on starts at the layer's line thickness", () => {
  const { chart: c, table } = chart();
  const f = drillChartForm(c, table, false, []);
  const on = onBorderChecked({ ...f, rowSeparators: true }, 150_000);
  assert.deepEqual([on.borderWidthNm, on.separatorsWidthNm], [100_000, 150_000]);
  assert.equal(onBorderChecked(f, 150_000).separatorsWidthNm, -1, "still off: no width");
});

test("a template is written as SaveToFile writes it and read back as LoadFromFile reads it", () => {
  const text = templateJson({ name: "drill_chart", version: 1, units: "inch", precision: 4, showTotals: false, columns: [{ id: "symbol", heading: "S", align: "center", width_nm: 2_000_000 }] });
  assert.equal(
    text,
    '{\n  "columns": [\n    {\n      "align": "center",\n      "heading": "S",\n      "id": "symbol",\n      "width": 2000000\n    }\n  ],\n  "name": "drill_chart",\n  "precision": 4,\n  "show_totals": false,\n  "units": "inch",\n  "version": 1\n}\n'
  );
  const t = readTemplate(text);
  assert.ok(!("error" in t));
  assert.deepEqual([t.units, t.precision, t.showTotals, t.columns.length], ["inch", 4, false, 1]);
  assert.deepEqual(readTemplate("[1]"), { error: "Template is not a JSON object." });
  assert.deepEqual(readTemplate('{"columns": [{"id": "from_a_newer_kicad"}]}'), { error: "Template has no usable columns." }, "an unknown column is skipped, not defaulted");
  assert.ok("error" in readTemplate("{"), "the parser's own message");
  const twice = readTemplate('{"precision": 9, "columns": [{"id": "plating"}, {"id": "plating", "heading": "again"}]}');
  assert.ok(!("error" in twice) && twice.columns.length === 1 && twice.precision === 6, "a repeat dropped, the precision clamped to 6");
  assert.equal(validateColumns([{ id: "plating", heading: "", align: "left", width_nm: 2e9 }]).columns[0]!.width_nm, 1e9);
});

test("Drill Groups: a row's cells, and a mark committed only when it changed", () => {
  const g: DrillGroupRow = {
    key: "k",
    symbol: { mode: "shape", shape_index: 2 },
    mark_text: "Shape 3",
    diameter_nm: 300_000,
    is_slot: false,
    size_nm: [300_000, 300_000],
    not_plated: false,
    top: "F.Cu",
    bottom: "B.Cu",
    operations: 4,
    sites: 3,
    members: ["via_a"],
  };
  assert.deepEqual(groupCells(g), ["Shape 3", "0.300", "Yes", "F.Cu / B.Cu", "4", "3"]);
  assert.deepEqual(groupCells({ ...g, is_slot: true, size_nm: [600_000, 1_200_000], not_plated: true })[1], "0.600 x 1.200");
  const same = { mode: "shape" as const, shape: 2, letter: "", description: "" };
  assert.equal(markEditCmd("k", same, same), null);
  assert.deepEqual(markEditCmd("k", { ...same, mode: "letter", letter: "Z", shape: 99 }, same), {
    op: "pcb_item",
    verb: "set_drill_profile",
    profile: { assignment: { key: "k", mark: { mode: "letter", shape_index: 63, letter: "Z", description: "" } } },
  });
});

test("a double click on a data row selects the row's cells (selectChartRow); the heading and the totals do not", () => {
  const { chart: c, table } = chart();
  assert.deepEqual(chartRowCells(c, table, 15_000_000, 4_000_000), ["tbl_a:1:0", "tbl_a:1:1"]);
  assert.equal(chartRowCells(c, table, 5_000_000, 1_000_000), null, "the heading");
  assert.equal(chartRowCells(c, table, 5_000_000, 10_000_000), null, "the totals");
  // A chart written before the keys: the rows between the heading and the totals.
  assert.equal(isDataRow({ ...c, row_keys: {} }, table, 2), true);
  assert.equal(isDataRow({ ...c, row_keys: {} }, table, 3), false);
});
