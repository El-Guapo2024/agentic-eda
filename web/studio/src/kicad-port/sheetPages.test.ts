import { test } from "node:test";
import assert from "node:assert/strict";
import { comparePageNum, neighbourSheet, sortByPageNumbers, strNumCmp, toLong, type HierarchySheet } from "./sheetPages";

const sheet = (path: string[], page = "", name = path.join("/") || "root"): HierarchySheet => ({ path, name, file: `${name}.kicad_sch`, page });

test("toLong takes a whole integer only", () => {
  assert.equal(toLong("12"), 12);
  assert.equal(toLong("+3"), 3);
  assert.equal(toLong("12a"), null);
  assert.equal(toLong(""), null);
  assert.equal(toLong("A1"), null);
});

test("a natural compare reads digit runs as numbers", () => {
  assert.equal(strNumCmp("A2", "A10"), -1);
  assert.equal(strNumCmp("A10", "A2"), 1);
  assert.equal(strNumCmp("A10", "A10"), 0);
  assert.equal(strNumCmp("A", "AB"), -1);
  assert.equal(strNumCmp("AB", "A"), 1);
  assert.equal(strNumCmp("a", "b"), -1);
  assert.equal(strNumCmp("S1x", "S1y"), -1);
});

test("integers come first in numeric order, then the other page names naturally", () => {
  const order = ["10", "2", "B", "A3", "A12", "1", "A"].sort(comparePageNum);
  assert.deepEqual(order, ["1", "2", "10", "A", "A3", "A12", "B"]);
  assert.equal(comparePageNum("3", "3"), 0);
  assert.equal(comparePageNum("03", "3"), 0);
});

test("a sheet without a page is numbered by its place in the walk, and the walk order breaks ties", () => {
  // walk: root, A (page 3), B (none -> 3), C (none -> 4)
  const walk = [sheet([]), sheet(["a"], "3", "A"), sheet(["b"], "", "B"), sheet(["c"], "", "C")];
  assert.deepEqual(sortByPageNumbers(walk).map((s) => s.name), ["root", "A", "B", "C"], "A and B are both page 3: A comes first in the walk");
  const moved = [sheet([]), sheet(["a"], "9", "A"), sheet(["b"], "", "B"), sheet(["c"], "2", "C")];
  assert.deepEqual(sortByPageNumbers(moved).map((s) => s.name), ["root", "C", "B", "A"], "C is page 2, B is its place (3), A is page 9");
});

test("Next and Previous step along the page order and stop at its ends", () => {
  const walk = [sheet([]), sheet(["a"], "3", "A"), sheet(["b"], "2", "B"), sheet(["c"], "", "C")];
  // page order: root(1), B(2), A(3), C(4)
  assert.equal(neighbourSheet(walk, [], 1)?.name, "B");
  assert.equal(neighbourSheet(walk, ["b"], 1)?.name, "A");
  assert.equal(neighbourSheet(walk, ["a"], 1)?.name, "C");
  assert.equal(neighbourSheet(walk, ["c"], 1), null, "the last page has no next");
  assert.equal(neighbourSheet(walk, ["c"], -1)?.name, "A");
  assert.equal(neighbourSheet(walk, [], -1), null, "the first page has no previous");
  assert.equal(neighbourSheet(walk, ["nope"], 1), null, "a sheet that is not in the list has no neighbours");
});
