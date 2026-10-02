import { test } from "node:test";
import assert from "node:assert/strict";
import { messageTextFromValue } from "./messageText";

// Same expectations as crates/cli/src/board_stats.rs's
// `message_text_from_value_matches_kicad_formats`, so the dialog and the
// backend report agree.

test("messageTextFromValue: long-form distances per unit", () => {
  assert.equal(messageTextFromValue(1600, "mm"), "1.6000 mm");
  assert.equal(messageTextFromValue(254, "mil"), "10.00 mils");
  assert.equal(messageTextFromValue(25_400, "in"), "1.0000 in");
});

test("messageTextFromValue: short-form areas trim one trailing zero in mm", () => {
  assert.equal(messageTextFromValue(5e9, "mm", true, "area"), "5000.00 mm²");
  assert.equal(messageTextFromValue(1234.5e6, "mm", true, "area"), "1234.50 mm²");
  assert.equal(messageTextFromValue(1234.567e6, "mm", true, "area"), "1234.567 mm²");
});

test("messageTextFromValue: a non-zero value that prints as zeros switches to %.3e", () => {
  assert.equal(messageTextFromValue(0.01, "mm", false), "1.000e-05");
  assert.equal(messageTextFromValue(0, "mm", false), "0.0000");
});

test("messageTextFromValue: INT_MAX nm (an unset minimum) prints like KiCad's dialog", () => {
  assert.equal(messageTextFromValue(2147483.647, "mm"), "2147.4836 mm");
});
