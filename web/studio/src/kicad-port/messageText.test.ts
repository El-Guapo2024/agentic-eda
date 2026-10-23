import { test } from "node:test";
import assert from "node:assert/strict";
import { messageTextFromValue } from "./messageText";

// Same expectations as crates/cli/src/board_stats.rs's
// `message_text_from_value_matches_kicad_formats`, so the dialog and the
// backend report agree.

test("messageTextFromValue: long-form distances per unit", () => {
  assert.equal(messageTextFromValue(1_600_000, "mm"), "1.6000 mm");
  assert.equal(messageTextFromValue(254_000, "mil"), "10.00 mils");
  assert.equal(messageTextFromValue(25_400_000, "in"), "1.0000 in");
});

test("messageTextFromValue: short-form areas trim one trailing zero in mm", () => {
  assert.equal(messageTextFromValue(5e15, "mm", true, "area"), "5000.00 mm²");
  assert.equal(messageTextFromValue(1234.5e12, "mm", true, "area"), "1234.50 mm²");
  assert.equal(messageTextFromValue(1234.567e12, "mm", true, "area"), "1234.567 mm²");
});

test("messageTextFromValue: a non-zero value that prints as zeros switches to %.3e", () => {
  assert.equal(messageTextFromValue(10, "mm", false), "1.000e-05");
  assert.equal(messageTextFromValue(0, "mm", false), "0.0000");
});

test("messageTextFromValue: INT_MAX nm (an unset minimum) prints like KiCad's dialog", () => {
  assert.equal(messageTextFromValue(2147483647, "mm"), "2147.4836 mm");
});
