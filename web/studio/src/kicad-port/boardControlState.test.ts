import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_BOARD_CONTROL, toggledHighlight, withHighlight } from "./boardControlState";

test("every highlight that is turned on is remembered, and clearing it forgets it (`m_currentlyHighlighted`)", () => {
  let bcx = withHighlight(DEFAULT_BOARD_CONTROL, null, ["GND"]);
  assert.deepEqual(bcx.currentHighlight, ["GND"]);
  bcx = withHighlight(bcx, "GND", ["VCC", "3V3"]);
  assert.deepEqual(bcx.currentHighlight, ["VCC", "3V3"]);
  assert.deepEqual(bcx.lastHighlight, ["GND"], "`m_lastHighlighted` took what was on show");
  bcx = withHighlight(bcx, "VCC", []);
  assert.deepEqual(bcx.currentHighlight, [], "ClearHighlight / toggling the same net off empties it");
});

test("Alt+` switches the highlight off and brings the last highlighted nets back, whatever the selection is", () => {
  // nothing highlighted and nothing remembered: nothing to bring back
  assert.deepEqual(toggledHighlight([], []), []);
  // highlighted now: off
  assert.deepEqual(toggledHighlight(["GND"], ["GND"]), []);
  // off, but GND is remembered: it comes back
  assert.deepEqual(toggledHighlight([], ["GND", "VCC"]), ["GND", "VCC"]);
  // and the remembered nets are not changed by the toggle: off and on again gives the same
  const remembered = ["GND"];
  const off = toggledHighlight(["GND"], remembered);
  assert.deepEqual(toggledHighlight(off, remembered), ["GND"]);
});
