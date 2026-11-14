import { test } from "node:test";
import assert from "node:assert/strict";
import { isViaDrcMessage, viaDrcInfoBarText, VIA_VIOLATES_DRC } from "./viaDrcInfoBar";

test("Draw Via's DRC message is KiCad's info bar, the refused via's and the allowed one's alike (UI-LEFT-3 P3-32)", () => {
  assert.equal(VIA_VIOLATES_DRC, "Via location violates DRC.");
  assert.equal(isViaDrcMessage("place_via", "ops_via_drc: Via location violates DRC."), true);
  assert.equal(isViaDrcMessage("place_via", VIA_VIOLATES_DRC), true);
  assert.equal(isViaDrcMessage("place_via", "via drill and diameter must be positive, the drill smaller"), false);
  assert.equal(isViaDrcMessage("add_via", VIA_VIOLATES_DRC), false, "only Draw Via's");
  assert.equal(isViaDrcMessage("place_via", undefined), false);
});

test("the info bar says KiCad's words alone, not the server's error code before them", () => {
  assert.equal(viaDrcInfoBarText("ops_via_drc: Via location violates DRC."), "Via location violates DRC.");
  assert.equal(viaDrcInfoBarText("something else"), "something else");
});
