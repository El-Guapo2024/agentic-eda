import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PCB_GRIDS_NM } from "./grid";
import {
  EESCHEMA_GRIDS_NM,
  defaultGridSettings,
  gridEditorOfTab,
  gridListError,
  insertGrid,
  isDefaultGridSettings,
  moveGrid,
  normalizeGridSettings,
  parseGridSettings,
  parseGridSize,
  parseStoredGridSettings,
  removeGrid,
  replaceGrid,
  resetGrids,
  stepGrid,
  type GridEdit,
  type GridSettings,
} from "./gridSettings";

const ok = (e: GridEdit) => {
  assert.equal(e.ok, true);
  return e as Extract<GridEdit, { ok: true }>;
};

test("the board and footprint editors start with KiCad's PCB list and fast grids 0.5 mm and 0.25 mm; the symbol editor with 100/50/25/10 mil", () => {
  const pcb = defaultGridSettings("pcb");
  assert.equal(pcb.grids.length, 22);
  assert.deepEqual([pcb.grids[pcb.fast1], pcb.grids[pcb.fast2]], [500, 250]);
  assert.deepEqual(defaultGridSettings("footprint"), pcb);
  const sym = defaultGridSettings("symbol");
  assert.deepEqual(sym.grids, EESCHEMA_GRIDS_NM);
  assert.deepEqual([sym.grids[sym.fast1], sym.grids[sym.fast2]], [1270, 635], "the default grid (50 mil) and the one after it");
});

test("the board, schematic, footprint and symbol editors have a grid list", () => {
  assert.equal(gridEditorOfTab("pcb"), "pcb");
  assert.equal(gridEditorOfTab("footprint"), "footprint");
  assert.equal(gridEditorOfTab("symbol"), "symbol");
  assert.equal(gridEditorOfTab("schematic"), "schematic");
  assert.equal(gridEditorOfTab("viewer3d"), null);
});

test("the schematic starts with eeschema's list and fast grids 50 and 25 mil, like the symbol editor", () => {
  const sch = defaultGridSettings("schematic");
  assert.deepEqual(sch, defaultGridSettings("symbol"));
  assert.deepEqual(sch.grids, EESCHEMA_GRIDS_NM);
  assert.deepEqual([sch.grids[sch.fast1], sch.grids[sch.fast2]], [1270, 635]);
});

test("a fast grid outside the list is the first grid, and the list is never empty", () => {
  assert.deepEqual(normalizeGridSettings({ grids: [1_000_000, 500_000], fast1: 9, fast2: -3 }), { grids: [1_000_000, 500_000], fast1: 0, fast2: 0 });
  assert.deepEqual(normalizeGridSettings({ grids: [1_000_000, 500_000], fast1: 1, fast2: 0 }), { grids: [1_000_000, 500_000], fast1: 1, fast2: 0 });
  assert.deepEqual(normalizeGridSettings({ grids: [], fast1: 0, fast2: 1 }).grids, DEFAULT_PCB_GRIDS_NM);
});

test("a saved list needs a grid and every size above zero", () => {
  assert.equal(gridListError([1_000_000, 250_000]), null);
  assert.match(gridListError([])!, /at least one/);
  assert.match(gridListError([1_000_000, 0])!, /above zero/);
  assert.match(gridListError([Number.NaN])!, /above zero/);
});

const base: GridSettings = { grids: [1_000_000, 500_000, 250_000, 100_000], fast1: 1, fast2: 2 };

test("a new grid goes in before the selected row and becomes the selected one", () => {
  const e = ok(insertGrid(base, 2, 650_000));
  assert.deepEqual(e.settings.grids, [1_000_000, 500_000, 650_000, 250_000, 100_000]);
  assert.equal(e.row, 2);
  assert.deepEqual([e.settings.fast1, e.settings.fast2], [1, 3], "the fast grids stay on the grids they named");
  assert.deepEqual(ok(insertGrid(base, 0, 2_000_000)).settings.grids, [2_000_000, 1_000_000, 500_000, 250_000, 100_000]);
  assert.deepEqual(ok(insertGrid(base, 9, 50_000)).settings.grids, [1_000_000, 500_000, 250_000, 100_000, 50_000], "past the end it is the last");
});

test("a size outside 0.001 .. 1000 mm or in the list already is refused", () => {
  assert.deepEqual(insertGrid(base, 0, 0), { ok: false, error: "range" });
  assert.deepEqual(insertGrid(base, 0, 0.5), { ok: false, error: "range" });
  assert.deepEqual(insertGrid(base, 0, 1_000_001_000), { ok: false, error: "range" });
  assert.deepEqual(insertGrid(base, 0, Number.NaN), { ok: false, error: "range" });
  assert.equal(ok(insertGrid(base, 0, 1)).settings.grids[0], 1, "0.001 mm is the smallest");
  assert.equal(ok(insertGrid(base, 0, 1_000_000_000)).settings.grids[0], 1_000_000_000, "1000 mm is the largest");
  assert.deepEqual(insertGrid(base, 0, 250_000), { ok: false, error: "duplicate" });
});

test("editing a grid changes its size in place; one that was a fast grid goes back to the first / last entry", () => {
  const e = ok(replaceGrid(base, 1, 400_000));
  assert.deepEqual(e.settings.grids, [1_000_000, 400_000, 250_000, 100_000]);
  assert.equal(e.row, 1);
  assert.deepEqual([e.settings.fast1, e.settings.fast2], [0, 2], "fast grid 1 named the old size, so it falls back to the first entry; fast grid 2 stays");
  const last = ok(replaceGrid(base, 2, 300_000));
  assert.deepEqual([last.settings.fast1, last.settings.fast2], [1, 3], "fast grid 2 falls back to the last entry");
  assert.equal(ok(replaceGrid(base, 1, 500_000)).settings, base, "unchanged is not an error and changes nothing");
  assert.deepEqual(replaceGrid(base, 1, 250_000), { ok: false, error: "duplicate" });
  assert.deepEqual(replaceGrid(base, 1, 0), { ok: false, error: "range" });
  assert.deepEqual(replaceGrid(base, 7, 400_000), { ok: false, error: "range" });
});

test("removing a grid keeps the last one, selects the row above and keeps the fast grids on their grids", () => {
  const a = removeGrid(base, 0);
  assert.deepEqual(a.settings, { grids: [500_000, 250_000, 100_000], fast1: 0, fast2: 1 });
  assert.equal(a.row, 0, "the first row stays selected");
  const b = removeGrid(base, 3);
  assert.deepEqual(b.settings, { grids: [1_000_000, 500_000, 250_000], fast1: 1, fast2: 2 });
  assert.equal(b.row, 2);
  const c = removeGrid(base, 1);
  assert.deepEqual(c.settings, { grids: [1_000_000, 250_000, 100_000], fast1: 0, fast2: 1 }, "the removed grid was fast grid 1: the first entry; fast grid 2 moves back with the list");
  assert.equal(c.row, 0);
  const d = removeGrid(base, 2);
  assert.deepEqual(d.settings, { grids: [1_000_000, 500_000, 100_000], fast1: 1, fast2: 2 }, "the removed grid was fast grid 2: the last entry");
  assert.deepEqual(removeGrid({ grids: [1000], fast1: 0, fast2: 0 }, 0).settings.grids, [1000]);
  assert.equal(removeGrid(base, 9).settings, base);
});

test("moving a grid swaps it with its neighbour and the selection follows", () => {
  const up = moveGrid(base, 2, -1);
  assert.deepEqual(up.settings, { grids: [1_000_000, 250_000, 500_000, 100_000], fast1: 2, fast2: 1 });
  assert.equal(up.row, 1);
  const down = moveGrid(base, 0, 1);
  assert.deepEqual(down.settings.grids, [500_000, 1_000_000, 250_000, 100_000]);
  assert.equal(down.row, 1);
  assert.equal(moveGrid(base, 0, -1).settings, base, "the first cannot go up");
  assert.equal(moveGrid(base, 3, 1).settings, base, "the last cannot go down");
  assert.equal(moveGrid({ grids: [1000], fast1: 0, fast2: 0 }, 0, 1).row, 0);
});

test("Reset to Defaults puts the default list back; the fast grids stay on their size when the list has it", () => {
  const edited = ok(insertGrid(defaultGridSettings("pcb"), 0, 650_000)).settings;
  assert.deepEqual(edited.grids.length, 23);
  assert.deepEqual(resetGrids(edited, "pcb"), defaultGridSettings("pcb"));
  const custom: GridSettings = { grids: [650_000, 500_000, 250_000], fast1: 0, fast2: 2 };
  const reset = resetGrids(custom, "pcb");
  assert.deepEqual(reset.grids, DEFAULT_PCB_GRIDS_NM);
  assert.deepEqual([reset.grids[reset.fast1], reset.grids[reset.fast2]], [DEFAULT_PCB_GRIDS_NM[0], 250], "650 um is not a default grid, so fast grid 1 is the first entry; 250 um is");
  const sym = resetGrids({ grids: [1_270_000, 100_000], fast1: 1, fast2: 0 }, "symbol");
  assert.deepEqual(sym, { grids: EESCHEMA_GRIDS_NM, fast1: 0, fast2: 1 });
});

test("a typed size is a number in the display unit unless it carries its own", () => {
  assert.equal(parseGridSize("0.25", "mm"), 250_000);
  assert.equal(parseGridSize("0,65", "mm"), 650_000);
  assert.equal(parseGridSize("50", "mil"), 1_270_000);
  assert.equal(parseGridSize("0.1", "in"), 2_540_000);
  assert.equal(parseGridSize("10 mil", "mm"), 254_000, "its own unit wins");
  assert.equal(parseGridSize("0.5mm", "mil"), 500_000);
  assert.equal(parseGridSize("1 in", "mm"), 25_400_000);
  assert.equal(parseGridSize("0", "mm"), 0, "a number; whether the size is allowed is for insertGrid to say");
  assert.equal(parseGridSize("", "mm"), null);
  assert.equal(parseGridSize("-1", "mm"), null);
  assert.equal(parseGridSize("abc", "mm"), null);
  assert.equal(parseGridSize("1 parsec", "mm"), null);
});

test("saved settings are read back when sound and the defaults used when not", () => {
  const good = { grids: [1_000_000, 650_000], fast1: 1, fast2: 0 };
  assert.deepEqual(parseGridSettings(good, "pcb"), good);
  assert.deepEqual(parseGridSettings(null, "pcb"), defaultGridSettings("pcb"));
  assert.deepEqual(parseGridSettings({ grids: [] }, "symbol"), defaultGridSettings("symbol"));
  assert.deepEqual(parseGridSettings({ grids: [10_000, "x"] }, "symbol").grids, [10], "a non-number is dropped");
  assert.deepEqual(parseGridSettings({ grids: [1_000_000, 500_000], fast1: 7, fast2: 8 }, "pcb"), { grids: [1_000_000, 500_000], fast1: 0, fast2: 0 }, "fast grids out of range are the first grid");
});

test("the settings of the four editors come from one saved text", () => {
  const all = parseStoredGridSettings(JSON.stringify({ footprint: { grids: [250_000, 100_000], fast1: 0, fast2: 1 } }));
  assert.deepEqual(all.footprint, { grids: [250_000, 100_000], fast1: 0, fast2: 1 });
  assert.deepEqual(all.pcb, defaultGridSettings("pcb"));
  assert.deepEqual(all.symbol, defaultGridSettings("symbol"));
  assert.deepEqual(all.schematic, defaultGridSettings("schematic"));
  assert.deepEqual(parseStoredGridSettings("{not json").pcb, defaultGridSettings("pcb"));
  assert.deepEqual(parseStoredGridSettings(null).symbol, defaultGridSettings("symbol"));
});

test("the defaults are recognised", () => {
  assert.equal(isDefaultGridSettings(defaultGridSettings("pcb"), "pcb"), true);
  assert.equal(isDefaultGridSettings(defaultGridSettings("pcb"), "symbol"), false);
  assert.equal(isDefaultGridSettings(ok(insertGrid(defaultGridSettings("pcb"), 0, 650_000)).settings, "pcb"), false);
});

test("Next Grid and Previous Grid walk the list and wrap round", () => {
  const grids = [1_000_000, 500_000, 250_000];
  assert.equal(stepGrid(grids, 1_000_000, 1), 500_000);
  assert.equal(stepGrid(grids, 250_000, 1), 1_000_000, "after the last is the first");
  assert.equal(stepGrid(grids, 1_000_000, -1), 250_000, "before the first is the last");
  assert.equal(stepGrid(grids, 123_000, 1), 1_000_000, "a grid that is not in the list: next is the first");
  assert.equal(stepGrid(grids, 123_000, -1), 250_000, "and previous the last");
});
