import { test } from "node:test";
import assert from "node:assert/strict";
import { keepOnSheet, sheetItemIds } from "./schSelectionPrune";
import type { Schematic } from "../api/types";

const sheet = (over: Partial<Record<keyof Schematic, unknown>>): Schematic =>
  ({ symbols: [], wires: [], labels: [], texts: [], power_symbols: [], no_connects: [], bus_entries: [], sheets: [], junctions: [], lines: [], graphics: [], ...over }) as unknown as Schematic;

test("every kind of sheet item counts as on the sheet", () => {
  const sch = sheet({
    symbols: [{ id: "C2" }, { id: "C2" }],
    wires: [{ id: "w1" }],
    labels: [{ id: "lbl_1" }],
    texts: [{ id: "txt_1" }],
    power_symbols: [{ id: "#PWR01" }],
    no_connects: [{ id: "nc_1" }],
    bus_entries: [{ id: "be_1" }],
    sheets: [{ id: "sh_1" }],
    junctions: [{ id: "j_1" }],
    lines: [{ id: "ln_1" }],
    graphics: [{ id: "shp_1" }, { id: "tbox_1" }],
  });
  assert.deepEqual([...sheetItemIds(sch)].sort(), ["#PWR01", "C2", "be_1", "j_1", "lbl_1", "ln_1", "nc_1", "sh_1", "shp_1", "tbox_1", "txt_1", "w1"]);
});

test("a selection loses only the ids that left the sheet", () => {
  const sch = sheet({ wires: [{ id: "w1" }], labels: [{ id: "lbl_new" }] });
  assert.deepEqual([...keepOnSheet(new Set(["w1", "lbl_old", "lbl_new"]), sch)].sort(), ["lbl_new", "w1"]);
});

test("a selection that lost nothing comes back as the same set, an empty one too", () => {
  const sch = sheet({ wires: [{ id: "w1" }] });
  const same = new Set(["w1"]);
  assert.equal(keepOnSheet(same, sch), same);
  const none = new Set<string>();
  assert.equal(keepOnSheet(none, sch), none);
});

test("an older backend's sheet without junctions, lines or graphics still works", () => {
  const sch = { symbols: [], wires: [{ id: "w1" }], labels: [], texts: [], power_symbols: [], no_connects: [], bus_entries: [], sheets: [] } as unknown as Schematic;
  assert.deepEqual([...keepOnSheet(new Set(["w1", "x"]), sch)], ["w1"]);
});
