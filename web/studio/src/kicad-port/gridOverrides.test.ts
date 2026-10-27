import { test } from "node:test";
import assert from "node:assert/strict";
import {
  OVERRIDE_CATEGORIES,
  defaultGridOverrides,
  selectionGridSize,
  gridSizeFor,
  isOverridden,
  overrideRows,
  parseGridOverrides,
  pcbItemGrid,
  rebuildOverrides,
  safeOverrideIndex,
  sameOverrides,
  schItemGrid,
  selectionGrid,
  type GridCategory,
  type GridOverrides,
} from "./gridOverrides";
import { DEFAULT_PCB_GRIDS_NM } from "./grid";
import { EESCHEMA_GRIDS_NM } from "./gridSettings";

const same = (a: number, b: number) => Math.abs(a - b) < 1e-9;

test("the schematic's defaults: connected items and wires on 50 mil, text on 10 mil, graphics and vias off", () => {
  const d = defaultGridOverrides("schematic");
  assert.equal(d.enabled, true);
  assert.deepEqual(d.connectable, { on: true, index: 1 });
  assert.deepEqual(d.wires, { on: true, index: 1 });
  assert.deepEqual(d.text, { on: true, index: 3 });
  assert.deepEqual(d.graphics, { on: false, index: 2 });
  assert.deepEqual(d.vias, { on: false, index: 0 });
  // Indexes into the eeschema list 100, 50, 25, 10 mil.
  assert.equal(EESCHEMA_GRIDS_NM[d.connectable.index], 1_270_000);
  assert.equal(EESCHEMA_GRIDS_NM[d.text.index], 254_000);
  assert.equal(EESCHEMA_GRIDS_NM[d.graphics.index], 635_000);
});

test("the board editors start with every override off, pointing at KiCad's default entries", () => {
  const d = defaultGridOverrides("pcb");
  assert.equal(d.enabled, true);
  assert.ok(OVERRIDE_CATEGORIES.every((c) => d[c].on === false));
  assert.equal(DEFAULT_PCB_GRIDS_NM[d.connectable.index], 250_000, "0.25 mm");
  assert.equal(DEFAULT_PCB_GRIDS_NM[d.wires.index], 50_000, "0.05 mm");
  assert.equal(DEFAULT_PCB_GRIDS_NM[d.vias.index], 100_000, "0.1 mm");
  assert.equal(DEFAULT_PCB_GRIDS_NM[d.text.index], 100_000, "0.1 mm");
  assert.equal(DEFAULT_PCB_GRIDS_NM[d.graphics.index], 500_000, "0.5 mm");
});

test("GetGridSize: the category's grid when overrides are on and the category's is, else the current grid", () => {
  const grids = EESCHEMA_GRIDS_NM;
  const o = defaultGridOverrides("schematic");
  assert.equal(gridSizeFor("connectable", 635_000, grids, o), 1_270_000);
  assert.equal(gridSizeFor("wires", 635_000, grids, o), 1_270_000);
  assert.equal(gridSizeFor("text", 635_000, grids, o), 254_000);
  assert.equal(gridSizeFor("graphics", 635_000, grids, o), 635_000, "off: the current grid");
  assert.equal(gridSizeFor("vias", 635_000, grids, o), 635_000);
  assert.equal(gridSizeFor("current", 635_000, grids, o), 635_000);
  // The master switch (the toolbar button, Ctrl+Shift+G).
  assert.equal(gridSizeFor("connectable", 635_000, grids, { ...o, enabled: false }), 635_000);
  assert.equal(gridSizeFor("connectable", 635_000, grids, null), 635_000);
  assert.equal(isOverridden("connectable", grids, o), true);
  assert.equal(isOverridden("graphics", grids, o), false);
  assert.equal(isOverridden("connectable", grids, { ...o, enabled: false }), false);
});

test("an override whose index is not in the list means the current grid", () => {
  const o: GridOverrides = { ...defaultGridOverrides("schematic"), text: { on: true, index: 9 } };
  assert.equal(gridSizeFor("text", 635_000, EESCHEMA_GRIDS_NM, o), 635_000);
  assert.equal(gridSizeFor("text", 635_000, EESCHEMA_GRIDS_NM, { ...o, text: { on: true, index: -1 } }), 635_000);
  assert.equal(isOverridden("text", EESCHEMA_GRIDS_NM, o), false);
});

test("GetSelectionGrid: the coarsest grid of the selection's items, the first on a tie", () => {
  const sizes: Record<GridCategory, number> = { current: 635, connectable: 1270, wires: 1270, vias: 100, text: 254, graphics: 635 };
  const size = (c: GridCategory) => sizes[c];
  assert.equal(selectionGrid([], size), "current");
  assert.equal(selectionGrid(["text", "connectable", "graphics"], size), "connectable");
  assert.equal(selectionGrid(["text", "graphics"], size), "graphics");
  assert.equal(selectionGrid(["wires", "connectable"], size), "wires", "equal: the first");
  assert.equal(selectionGrid(["vias"], size), "vias");
});

test("PCB_GRID_HELPER::GetItemGrid", () => {
  assert.equal(pcbItemGrid("footprint"), "connectable");
  assert.equal(pcbItemGrid("pad"), "connectable");
  assert.equal(pcbItemGrid("track"), "wires");
  assert.equal(pcbItemGrid("via"), "vias");
  assert.equal(pcbItemGrid("text"), "text");
  assert.equal(pcbItemGrid("field"), "text", "a footprint's field is text (`PCB_FIELD_T`)");
  assert.equal(pcbItemGrid("shape"), "graphics");
  assert.equal(pcbItemGrid("dimension"), "graphics");
  assert.equal(pcbItemGrid("zone"), "current");
  assert.equal(pcbItemGrid("group"), "current");
  assert.equal(pcbItemGrid(null), "current");
});

test("EE_GRID_HELPER::GetItemGrid", () => {
  for (const k of ["symbol", "pin", "sheet", "sheet_pin", "no_connect", "label", "global_label", "hier_label", "directive_label", "rule_area"] as const) assert.equal(schItemGrid(k), "connectable", k);
  for (const k of ["field", "text"] as const) assert.equal(schItemGrid(k), "text", k);
  for (const k of ["shape", "text_box", "bitmap", "graphic_line"] as const) assert.equal(schItemGrid(k), "graphics", k);
  for (const k of ["junction", "wire", "bus", "bus_entry"] as const) assert.equal(schItemGrid(k), "wires", k);
  assert.equal(schItemGrid("other"), "current");
  assert.equal(schItemGrid(undefined), "current");
});

test("the Grids page shows the rows each editor has, named as KiCad names them", () => {
  assert.deepEqual(
    overrideRows("pcb").map((r) => r.label),
    ["Footprints/pads", "Tracks", "Vias", "Text", "Graphics"]
  );
  assert.deepEqual(
    overrideRows("footprint").map((r) => r.category),
    ["connectable", "text", "graphics"]
  );
  assert.deepEqual(
    overrideRows("schematic").map((r) => r.category),
    ["connectable", "wires", "text", "graphics"]
  );
  assert.deepEqual(overrideRows("symbol"), overrideRows("schematic"));
});

test("editing the grid list keeps each override on its grid (found again by size), or sends it to the first entry", () => {
  const oldGrids = [2540, 1270, 635, 254];
  const o = defaultGridOverrides("schematic"); // connected 1, wires 1, text 3, graphics 2 (off)
  // Insert 500 at the front: 50 mil is now index 2.
  const inserted = rebuildOverrides(o, oldGrids, [500, 2540, 1270, 635, 254], same);
  assert.equal(inserted.connectable.index, 2);
  assert.equal(inserted.wires.index, 2);
  assert.equal(inserted.text.index, 4);
  assert.equal(inserted.graphics.index, 3);
  assert.equal(inserted.connectable.on, true);
  // Remove 10 mil: the text override loses its grid and goes to the first entry.
  const removed = rebuildOverrides(o, oldGrids, [2540, 1270, 635], same);
  assert.equal(removed.text.index, 0);
  assert.equal(removed.connectable.index, 1);
});

test("a stored index outside the list is the first grid (safeGrid)", () => {
  assert.equal(safeOverrideIndex(3, 4), 3);
  assert.equal(safeOverrideIndex(4, 4), 0);
  assert.equal(safeOverrideIndex(-1, 4), 0);
  assert.equal(safeOverrideIndex(Number.NaN, 4), 0);
});

test("overrides are read back from storage field by field, anything damaged taking the default", () => {
  const d = defaultGridOverrides("schematic");
  assert.deepEqual(parseGridOverrides(null, "schematic", 4), d);
  assert.deepEqual(parseGridOverrides("junk", "schematic", 4), d);
  const read = parseGridOverrides({ enabled: false, wires: { on: false, index: 2 }, text: { on: "yes", index: 99 }, graphics: 3 }, "schematic", 4);
  assert.equal(read.enabled, false);
  assert.deepEqual(read.wires, { on: false, index: 2 });
  assert.deepEqual(read.text, { on: d.text.on, index: 0 }, "a bad flag is the default; an index outside the list is the first grid");
  assert.deepEqual(read.graphics, d.graphics);
  assert.deepEqual(read.connectable, d.connectable);
  assert.equal(sameOverrides(d, defaultGridOverrides("schematic")), true);
  assert.equal(sameOverrides(d, { ...d, enabled: false }), false);
  assert.equal(sameOverrides(d, { ...d, wires: { on: true, index: 0 } }), false);
});

// r4 C4-M1 (`SCH_MOVE_TOOL::AlignToGrid`: `grid.GetSelectionGrid( selection )`): a text alone aligns to the text override (10 mil), a symbol to 50 mil, both to the
// coarser; graphics with their override off to the current grid.
test("Align to Grid aligns to the selection's grid, not always 50 mil", () => {
  const MIL = 25_400;
  const grids = [100 * MIL, 50 * MIL, 25 * MIL, 10 * MIL];
  const o = defaultGridOverrides("schematic");
  assert.equal(selectionGridSize(["text"], 25 * MIL, grids, o), 10 * MIL);
  assert.equal(selectionGridSize(["symbol"], 25 * MIL, grids, o), 50 * MIL);
  assert.equal(selectionGridSize(["symbol", "text"], 25 * MIL, grids, o), 50 * MIL);
  assert.equal(selectionGridSize(["graphic_line"], 25 * MIL, grids, o), 25 * MIL);
});
