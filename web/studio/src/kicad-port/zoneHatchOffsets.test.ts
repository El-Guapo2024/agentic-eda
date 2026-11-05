import { test } from "node:test";
import assert from "node:assert/strict";
import { hatchOffsetRows, hatchOffsetsOf, nextOffsetLayer, sameHatchOffsets } from "./zoneHatchOffsets";

test("the grid lists the zone's copper offsets in the layer list's order", () => {
  const rows = hatchOffsetRows({ "B.Cu": { x_nm: 1, y_nm: 2 }, "F.Cu": { x_nm: 0, y_nm: 0 }, "In2.Cu": { x_nm: 3, y_nm: 4 }, "F.SilkS": { x_nm: 9, y_nm: 9 } });
  assert.deepEqual(rows.map((r) => r.layer), ["F.Cu", "In2.Cu", "B.Cu"]);
  assert.deepEqual(hatchOffsetRows(undefined), []);
});

test("Add takes the next layer of the zone without a row; none left is KiCad's warning case", () => {
  assert.equal(nextOffsetLayer([], ["B.Cu", "F.Cu"]), "F.Cu");
  assert.equal(nextOffsetLayer([{ layer: "F.Cu", x_nm: 0, y_nm: 0 }], ["F.Cu", "B.Cu"]), "B.Cu");
  assert.equal(nextOffsetLayer([{ layer: "F.Cu", x_nm: 0, y_nm: 0 }], ["F.Cu"]), null);
});

test("OK gives the zone one offset per row, on the layers it is on (an offset of 0, 0 is an override too)", () => {
  const rows = [
    { layer: "F.Cu", x_nm: 0, y_nm: 0 },
    { layer: "In1.Cu", x_nm: 5, y_nm: 5 },
  ];
  assert.deepEqual(hatchOffsetsOf(rows, ["F.Cu", "B.Cu"]), { "F.Cu": { x_nm: 0, y_nm: 0 } });
  assert.equal(sameHatchOffsets(rows, rows.map((r) => ({ ...r }))), true);
  assert.equal(sameHatchOffsets(rows, [rows[0]!]), false);
});
