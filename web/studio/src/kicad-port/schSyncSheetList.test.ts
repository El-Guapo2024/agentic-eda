import { test } from "node:test";
import assert from "node:assert/strict";
import { syncSheetPaths } from "./schSyncSheetList";

test("Sync All Sheet Pins lists every sheet path of the hierarchy, each before its children", () => {
  const tree = [
    { path: [], file: "" },
    { path: ["a"], file: "a.kicad_sch" },
    { path: ["a", "a1"], file: "leaf.kicad_sch" },
    { path: ["b"], file: "b.kicad_sch" },
  ];
  assert.deepEqual(syncSheetPaths(tree, false), [["a"], ["a", "a1"], ["b"]]);
});

test("a screen shown by several sheets lists its own sheets under the first path only (aVisited)", () => {
  const tree = [
    { path: [], file: "" },
    { path: ["a"], file: "same.kicad_sch" },
    { path: ["a", "x"], file: "leaf.kicad_sch" },
    { path: ["b"], file: "same.kicad_sch" },
    { path: ["b", "x"], file: "leaf.kicad_sch" },
  ];
  assert.deepEqual(syncSheetPaths(tree, false), [["a"], ["a", "x"], ["b"]]);
});

test("with KiCad 10's virtual root the top-level sheets are not listed, their children are", () => {
  const tree = [
    { path: [], file: "" },
    { path: ["t1"], file: "t1.kicad_sch" },
    { path: ["t1", "c"], file: "c.kicad_sch" },
    { path: ["t2"], file: "t2.kicad_sch" },
  ];
  assert.deepEqual(syncSheetPaths(tree, true), [["t1", "c"]]);
});

test("no sub-sheet anywhere is an empty list (No sub schematic found in the current project)", () => {
  assert.deepEqual(syncSheetPaths([{ path: [], file: "" }], false), []);
});
