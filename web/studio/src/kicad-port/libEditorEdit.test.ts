import { test } from "node:test";
import assert from "node:assert/strict";
import { footprintPropertiesTarget, symbolPropertiesTarget } from "./libEditorEdit";

test("footprintPropertiesTarget: EDIT_TOOL::Properties in the Footprint Editor -- one pad is its Pad Properties, nothing selected is Footprint Properties", () => {
  assert.deepEqual(footprintPropertiesTarget([{ id: "pad_1", kind: "pad" }]), { dialog: "pad", id: "pad_1" });
  assert.deepEqual(footprintPropertiesTarget([]), { dialog: "footprint" });
  // two items: KiCad opens nothing (`selection.Size() == 1`)
  assert.equal(footprintPropertiesTarget([{ id: "pad_1", kind: "pad" }, { id: "pad_2", kind: "pad" }]), null);
});

test("symbolPropertiesTarget: SYMBOL_EDITOR_EDIT_TOOL::Properties -- a pin is Pin Properties, a shape its properties, nothing selected is Symbol Properties", () => {
  assert.deepEqual(symbolPropertiesTarget([{ id: "p1", kind: "pin" }]), { dialog: "pin", id: "p1" });
  assert.deepEqual(symbolPropertiesTarget([{ id: "g1", kind: "graphic" }]), { dialog: "graphic", id: "g1" });
  assert.deepEqual(symbolPropertiesTarget([{ id: "fld", kind: "field" }]), { dialog: "symbol", field: "fld" });
  assert.deepEqual(symbolPropertiesTarget([]), { dialog: "symbol" });
  assert.equal(symbolPropertiesTarget([{ id: "p1", kind: "pin" }, { id: "p2", kind: "pin" }]), null);
});
