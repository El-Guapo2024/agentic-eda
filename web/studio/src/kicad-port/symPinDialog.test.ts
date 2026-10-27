import { test } from "node:test";
import assert from "node:assert/strict";
import { commonBodyStylesBox, commonUnitsBox, pinOffGridMessage, pinUnitsOnOk, storedPinNameOffsetMm, syncPinsMessage } from "./symPinDialog";

test("an inside offset of 0 is 1 IU from New Symbol and 20 IU from Symbol Properties (E6)", () => {
  assert.equal(storedPinNameOffsetMm(true, 0, "new_symbol"), 0.0001);
  assert.equal(storedPinNameOffsetMm(true, 0, "symbol_properties"), 0.002);
  assert.equal(storedPinNameOffsetMm(true, 0.508, "new_symbol"), 0.508);
  assert.equal(storedPinNameOffsetMm(false, 0.508, "symbol_properties"), 0, "outside");
});

test("the off-grid question of TransferDataFromWindow, word for word, only for a moved pin (E7)", () => {
  const msg = "This pin is not on a 50 mils grid which will make it difficult to connect to in the schematic.\nDo you wish to continue?";
  assert.equal(pinOffGridMessage({ x: 0, y: 0 }, { x: 1.0001, y: 0 }), msg);
  assert.equal(pinOffGridMessage({ x: 1.0001, y: 0 }, { x: 1.0001, y: 0 }), null, "not moved: no question");
  // KiCad's test is `GetValue() % 50` in internal units (100 nm), so 1 mm (10000 IU) does not ask
  assert.equal(pinOffGridMessage({ x: 0, y: 0 }, { x: 1, y: -2.54 }), null);
});

test("the Common boxes are enabled only where they mean something; a single-unit pin gets the unit on OK (E8)", () => {
  assert.deepEqual(commonUnitsBox({ unit: 0 }, 1, false), { enabled: false, checked: false, tooltip: "This symbol only has one unit. This control has no effect." });
  assert.deepEqual(commonUnitsBox({ unit: 0 }, 2, true), { enabled: true, checked: true, tooltip: "Synchronized pins mode is enabled.\nSimilar pins will be edited regardless of this option." });
  assert.equal(commonUnitsBox({ unit: 1 }, 2, false).tooltip, "If checked, this pin will exist in all units.");
  assert.deepEqual(commonBodyStylesBox({ body_style: 0 }, false), { enabled: false, checked: true, tooltip: null });
  assert.deepEqual(pinUnitsOnOk(false, true, 1, 1), { unit: 1, body_style: 0 });
  assert.equal(syncPinsMessage(false, true), "Synchronized Pins Mode.  New pin will be added to all units.");
  assert.equal(syncPinsMessage(false, false), "Synchronized Pins Mode.  Matching pins in other units will be updated.");
  assert.equal(syncPinsMessage(true, false), "Synchronized Pins Mode.");
});
