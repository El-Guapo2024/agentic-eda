import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_BOARD_CONTROL, localRatsnestPadsAfter, toggledHighlight, withHighlight } from "./boardControlState";

test("every highlight that is turned on is remembered, and clearing it forgets it (`m_currentlyHighlighted`)", () => {
  let bcx = withHighlight(DEFAULT_BOARD_CONTROL, null, ["GND"]);
  assert.deepEqual(bcx.currentHighlight, ["GND"]);
  bcx = withHighlight(bcx, "GND", ["VCC"]);
  assert.deepEqual(bcx.currentHighlight, ["VCC"]);
  assert.deepEqual(bcx.lastHighlight, ["GND"], "`m_lastHighlighted` took what was on show");
  bcx = withHighlight(bcx, "VCC", ["3V3", "5V"]);
  assert.deepEqual(bcx.currentHighlight, ["3V3", "5V"]);
  assert.deepEqual(bcx.lastHighlight, ["GND"], "r7 C7-4: the multi-net branch returns before `m_lastHighlighted = netcodes`");
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

test("r7 C7-4: a highlight of several nets from nothing leaves the memory empty", () => {
  assert.deepEqual(withHighlight(DEFAULT_BOARD_CONTROL, "A", ["B", "C"]).lastHighlight, []);
});

test("r7 C7-3: every ratsnest display change, All to Visible layers included, forgets the Local Ratsnest tool's pads", () => {
  const pads = ["U1.1"];
  assert.deepEqual(localRatsnestPadsAfter(pads, { show: true, mode: "all" }, { show: true, mode: "visible" }), []);
  assert.deepEqual(localRatsnestPadsAfter(pads, { show: true, mode: "visible" }, { show: false, mode: "visible" }), []);
  assert.deepEqual(localRatsnestPadsAfter(pads, { show: true, mode: "all" }, { show: true, mode: "all" }), ["U1.1"], "no change, nothing forgotten");
});
