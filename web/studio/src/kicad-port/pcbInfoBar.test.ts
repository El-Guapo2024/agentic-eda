import { test } from "node:test";
import assert from "node:assert/strict";
import { infoBarFor, withoutErrorCode } from "./pcbInfoBar";

test("the board editor's info-bar messages: KiCad's words, icon, time and close button; any other message is no bar's", () => {
  assert.deepEqual(infoBarFor("ops_via_drc: Via location violates DRC."), { message: "Via location violates DRC.", kind: "error", ms: 8000, close: true });
  assert.deepEqual(infoBarFor("Select a track to tune first."), { message: "Select a track to tune first.", kind: "warning", ms: 6000, close: false });
  const z = infoBarFor("Not all zones are filled. Use Edit > Fill All Zones (B) if you wish to see all fills.");
  assert.deepEqual(z && [z.kind, z.ms], ["warning", 5000]);
  assert.equal(infoBarFor("Could not find design block Lib:Thing.")?.kind, "error");
  assert.equal(infoBarFor("File 'design.json' saved."), null);
  assert.equal(withoutErrorCode("board_worse: that move"), "that move");
});
