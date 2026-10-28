import { test } from "node:test";
import assert from "node:assert/strict";
import { nextUnitToPlace, referenceUnitSuffix, subpartIdsOf, unitCountOf } from "./schUnits";
import type { LibSymbol } from "../api/types";

const pin = (unit: number) => ({ number: "1", name: null, electrical_type: "passive", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit, body_style: 1, hidden: false }) as unknown as LibSymbol["pins"][number];
const box = (unit: number) => ({ kind: "rectangle", start: [0, 0], end: [1, 1], stroke_width: 0, fill: "none", unit, body_style: 1 }) as unknown as LibSymbol["graphics"][number];

test("a symbol's unit count is the highest unit any pin or graphic belongs to", () => {
  assert.equal(unitCountOf({ pins: [pin(1), pin(2), pin(3)], graphics: [] }), 3);
  assert.equal(unitCountOf({ pins: [pin(0)], graphics: [box(4), box(0)] }), 4);
  assert.equal(unitCountOf({ pins: [], graphics: [] }), 1);
  assert.equal(unitCountOf(undefined), 1);
});

test("the next unit is the lowest one not yet placed", () => {
  assert.deepEqual(nextUnitToPlace(4, [1]), { ok: true, unit: 2 });
  assert.deepEqual(nextUnitToPlace(4, [1, 3]), { ok: true, unit: 2 });
  assert.deepEqual(nextUnitToPlace(3, [2, 3]), { ok: true, unit: 1 });
});

test("a single-unit symbol and a fully placed one say why nothing is placed", () => {
  assert.deepEqual(nextUnitToPlace(1, [1]), { ok: false, message: "This symbol has only one unit." });
  assert.deepEqual(nextUnitToPlace(2, [1, 2]), { ok: false, message: "All units of this symbol are already placed." });
});

test("a placed unit's reference has its suffix whenever the symbol has more than one unit, in the project's naming (r7 M3, H5)", () => {
  const two = { pins: [pin(1), pin(2)], graphics: [] };
  // one placement of a two-unit symbol is still U1A (GetRef: GetUnitCount() > 1), a single-unit symbol never has one
  assert.equal(referenceUnitSuffix(two, 1), "A");
  assert.equal(referenceUnitSuffix({ pins: [pin(1)], graphics: [] }, 1), "");
  assert.equal(referenceUnitSuffix({ pins: [pin(1)], graphics: [], unit_count: 4 }, 3), "C", "the server's unit count wins");
  assert.equal(referenceUnitSuffix(undefined, 2), "");
  // "subpart_id_separator": 46, "subpart_first_id": 49 -- VD1.1, VD1.3
  const dotted = subpartIdsOf({ subpart_id_separator: 46, subpart_first_id: 49 });
  assert.equal(referenceUnitSuffix(two, 1, dotted), ".1");
  assert.equal(referenceUnitSuffix({ ...two, unit_count: 3 }, 3, dotted), ".3");
  // a display name follows the separator too (SCH_SYMBOL::SubReference)
  assert.equal(referenceUnitSuffix({ ...two, unit_names: { "2": "Pwr" } }, 2, dotted), ".Pwr");
  assert.equal(referenceUnitSuffix(two, 2, subpartIdsOf({ subpart_first_id: 97 })), "b", "letters from 'a'");
});
