import { test } from "node:test";
import assert from "node:assert/strict";
import { autoplacePins, constrainOnEdge, hasUndefinedPins, nextLabelToPlace, pinsToCleanUp, strNumCmp, syncRows, unplacedLabels, type HierLabel } from "./schSheetPins";

const sheet = { at: [10_000, 10_000] as const, size: [20_000, 10_000] as const };
const label = (name: string): HierLabel => ({ name, shape: "input" });

test("a pin goes on the nearest edge, level with the point, kept between the corners", () => {
  assert.deepEqual(constrainOnEdge(sheet, [15_000, 11_000]), { at: [15_000, 10_000], side: "top" });
  assert.deepEqual(constrainOnEdge(sheet, [29_000, 15_000]), { at: [30_000, 15_000], side: "right" });
  assert.deepEqual(constrainOnEdge(sheet, [20_000, 19_500]), { at: [20_000, 20_000], side: "bottom" });
  assert.deepEqual(constrainOnEdge(sheet, [9_000, 16_000]), { at: [10_000, 16_000], side: "left" });
});

test("a point past a corner is clamped onto the edge between the corners", () => {
  assert.deepEqual(constrainOnEdge(sheet, [5_000, 15_000]), { at: [10_000, 15_000], side: "left" });
  assert.deepEqual(constrainOnEdge(sheet, [15_000, 40_000]).at, [15_000, 20_000]);
});

test("a corner is on the top edge: the first of equally near edges wins", () => {
  assert.deepEqual(constrainOnEdge(sheet, [10_000, 10_000]), { at: [10_000, 10_000], side: "top" });
});

test("natural order puts A2 before A10 and ignores case when asked", () => {
  assert.ok(strNumCmp("A2", "A10", true) < 0);
  assert.ok(strNumCmp("a2", "A10", true) < 0);
  assert.ok(strNumCmp("a", "B", true) < 0);
  assert.ok(strNumCmp("a", "B", false) > 0, "case-sensitive: lower case sorts after upper case");
  assert.equal(strNumCmp("VCC", "vcc", true), 0);
  assert.ok(strNumCmp("A", "AB", true) < 0, "a prefix sorts first");
});

test("a pin is undefined when the file has no hierarchical label of exactly its name", () => {
  assert.equal(hasUndefinedPins([{ name: "A" }], [label("A")]), false);
  assert.equal(hasUndefinedPins([{ name: "A" }], [label("a")]), true);
  assert.equal(hasUndefinedPins([], []), false);
});

test("cleanup keeps a pin whose label differs only in case, as CleanupSheet does", () => {
  const pins = [{ id: "p1", name: "A" }, { id: "p2", name: "b" }, { id: "p3", name: "C" }];
  assert.deepEqual(pinsToCleanUp(pins, [label("a"), label("B")]).map((p) => p.id), ["p3"]);
});

test("labels with no pin yet are the ones to place, the next being the first in natural order", () => {
  const labels = [label("D10"), label("D2"), label("CLK")];
  assert.deepEqual(unplacedLabels([{ name: "CLK" }], labels).map((l) => l.name), ["D10", "D2"]);
  assert.equal(nextLabelToPlace([{ name: "CLK" }], labels)?.name, "D2");
  assert.equal(nextLabelToPlace([{ name: "CLK" }, { name: "D2" }, { name: "D10" }], labels), null);
});

test("the sync rows say which pins match, differ in shape, have no label, and which labels have no pin", () => {
  const pins = [
    { id: "p1", name: "A", shape: "input" as const },
    { id: "p2", name: "B", shape: "output" as const },
    { id: "p3", name: "GONE", shape: "passive" as const },
  ];
  const labels: HierLabel[] = [{ name: "A", shape: "input" }, { name: "B", shape: "input" }, { name: "NEW", shape: "output" }, { name: "NEW", shape: "output" }];
  assert.deepEqual(
    syncRows(pins, labels).map((r) => [r.kind, r.name]),
    [["ok", "A"], ["shape", "B"], ["pin_only", "GONE"], ["label_only", "NEW"]]
  );
});

test("autoplace lays the pins out along the top edge from the corner, then wraps onto the next edge", () => {
  const extent = () => ({ width: 6_000, height: 2_000 });
  const placed = autoplacePins(sheet, [], [label("A"), label("B"), label("C"), label("D")], extent);
  // The first at the corner, each next one a pin width further right; a fourth would need 24000 of a 20000-wide sheet, so it drops a row.
  assert.deepEqual(placed.slice(0, 3).map((p) => p.at), [[10_000, 10_000], [16_000, 10_000], [22_000, 10_000]]);
  assert.deepEqual(placed[3]!.at, [10_000, 12_000]);
});

test("autoplace continues after the pin that sorts last", () => {
  const extent = () => ({ width: 5_000, height: 2_000 });
  const placed = autoplacePins(sheet, [{ name: "X", at: [10_000, 10_000] }, { name: "Y", at: [15_000, 10_000] }], [label("X"), label("Y"), label("Z")], extent);
  assert.deepEqual(placed.map((p) => p.label.name), ["Z"]);
  assert.deepEqual(placed[0]!.at, [20_000, 10_000]);
});
