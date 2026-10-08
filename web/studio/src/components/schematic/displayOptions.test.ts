import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_SCH_DISPLAY, ercSeverityShown, sanitizeDisplay } from "./displayOptions";

test("KiCad's defaults: exclusions and hidden pins off, everything else on", () => {
  assert.deepEqual(DEFAULT_SCH_DISPLAY, { showHiddenPins: false, showDirectiveLabels: true, showErcErrors: true, showErcWarnings: true, showErcExclusions: false, markSimExclusions: true });
});

test("saved values merge over the defaults and anything unknown is dropped", () => {
  assert.deepEqual(sanitizeDisplay({ showHiddenPins: true, showErcErrors: "yes", junk: 1 }), { ...DEFAULT_SCH_DISPLAY, showHiddenPins: true });
  assert.deepEqual(sanitizeDisplay(null), DEFAULT_SCH_DISPLAY);
  assert.deepEqual(sanitizeDisplay("x"), DEFAULT_SCH_DISPLAY);
});

test("each ERC severity follows its own toggle; an excluded marker follows the exclusions one", () => {
  const d = { showErcErrors: false, showErcWarnings: true, showErcExclusions: false };
  assert.equal(ercSeverityShown("error", d), false);
  assert.equal(ercSeverityShown("warning", d), true);
  assert.equal(ercSeverityShown("excluded", d), false);
  assert.equal(ercSeverityShown("excluded", { ...d, showErcExclusions: true }), true);
});
