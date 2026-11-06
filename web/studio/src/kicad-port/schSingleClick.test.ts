import { test } from "node:test";
import assert from "node:assert/strict";
import { existingSingleClickItem } from "./schSingleClick";

const sheet = {
  no_connects: [{ id: "nc_1", at: [2_540_000, 5_080_000] as [number, number] }],
  junctions: [{ id: "jct_1", at: [0, 0] as [number, number] }],
  bus_entries: [{ id: "be_1", at: [10_160_000, 0] as [number, number], size: [2_540_000, 2_540_000] as [number, number] }],
};

test("a click on a no-connect flag finds it (the tool then removes it); a click beside it finds none", () => {
  assert.equal(existingSingleClickItem(sheet, "no_connect", [2_540_000, 5_080_000]), "nc_1");
  assert.equal(existingSingleClickItem(sheet, "no_connect", [2_540_000, 7_620_000]), null);
  assert.equal(existingSingleClickItem({}, "no_connect", [0, 0]), null);
});

test("a bus entry is found anywhere on its line, a junction at its point", () => {
  assert.equal(existingSingleClickItem(sheet, "bus_entry", [10_160_000, 0]), "be_1");
  assert.equal(existingSingleClickItem(sheet, "bus_entry", [11_430_000, 1_270_000]), "be_1", "the middle of the entry");
  assert.equal(existingSingleClickItem(sheet, "bus_entry", [12_700_000, 2_540_000]), "be_1", "its other end");
  assert.equal(existingSingleClickItem(sheet, "bus_entry", [12_700_000, 0]), null);
  assert.equal(existingSingleClickItem(sheet, "junction", [0, 0]), "jct_1");
});
