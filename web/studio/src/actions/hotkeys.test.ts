import { test } from "node:test";
import assert from "node:assert/strict";
import { eventToHotkey, displayHotkey, effectiveHotkey } from "./hotkeys";

function key(partial: Partial<{ key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean }>) {
  return { key: "a", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...partial } as unknown as KeyboardEvent;
}

test("eventToHotkey: Ctrl is read from ctrlKey on non-Mac, metaKey on Mac", () => {
  assert.equal(eventToHotkey(key({ key: "z", ctrlKey: true }), false), "Ctrl+Z");
  // metaKey is simply not inspected on non-Mac, so a bare "z" with meta
  // held is just the plain "Z" hotkey, not "Ctrl+Z" -- meta isn't treated
  // as some OTHER modifier either, it's ignored entirely (same as source
  // only ever reading one Ctrl-like signal per platform).
  assert.equal(eventToHotkey(key({ key: "z", metaKey: true }), false), "Z", "metaKey alone shouldn't count as Ctrl on non-Mac");
  assert.equal(eventToHotkey(key({ key: "z", metaKey: true }), true), "Ctrl+Z", "Cmd stands in for Ctrl on Mac");
  assert.equal(eventToHotkey(key({ key: "z", ctrlKey: true }), true), "Z", "plain ctrlKey alone isn't Cmd on Mac (a real external-keyboard Ctrl), so it's likewise ignored");
});

test("eventToHotkey: modifier order is Ctrl, Alt, Shift", () => {
  assert.equal(eventToHotkey(key({ key: "z", ctrlKey: true, altKey: true, shiftKey: true }), false), "Ctrl+Alt+Shift+Z");
});

test("eventToHotkey: a bare modifier keypress is not itself a hotkey", () => {
  assert.equal(eventToHotkey(key({ key: "Shift", shiftKey: true }), false), null);
  assert.equal(eventToHotkey(key({ key: "Control", ctrlKey: true }), false), null);
});

test("eventToHotkey: named keys canonicalize (Delete->Del, space->Space, function keys pass through)", () => {
  assert.equal(eventToHotkey(key({ key: "Delete" }), false), "Del");
  assert.equal(eventToHotkey(key({ key: " " }), false), "Space");
  assert.equal(eventToHotkey(key({ key: "F1" }), false), "F1");
  assert.equal(eventToHotkey(key({ key: "Escape" }), false), "Esc");
});

test("displayHotkey: Ctrl/Alt/Shift become symbols on Mac, with '+' dropped between them", () => {
  assert.equal(displayHotkey("Ctrl+Shift+Z", true), "⌘⇧Z");
  assert.equal(displayHotkey("Ctrl+Home", true), "⌘Home");
  assert.equal(displayHotkey("F1", true), "F1");
});

test("displayHotkey: a literal '+'/'-' key survives even though the string also uses '+' as a separator (zoomIn/zoomOut's real macOS default)", () => {
  assert.equal(displayHotkey("Ctrl++", true), "⌘+", "zoomIn's macHotkey: Cmd held, '+' key pressed");
  assert.equal(displayHotkey("Ctrl+-", true), "⌘-", "zoomOut's macHotkey: Cmd held, '-' key pressed");
});

test("displayHotkey: unchanged on non-Mac", () => {
  assert.equal(displayHotkey("Ctrl+Shift+Z", false), "Ctrl+Shift+Z");
});

test("displayHotkey: null hotkey displays as empty string", () => {
  assert.equal(displayHotkey(null, true), "");
});

const baseAction = { hotkey: "F1", altHotkey: null, macHotkey: null, macAltHotkey: null };

test("effectiveHotkey: non-Mac always uses hotkey/altHotkey as extracted", () => {
  assert.deepEqual(effectiveHotkey(baseAction, false), { hotkey: "F1", altHotkey: null });
});

test("effectiveHotkey: Mac uses hotkey/altHotkey too when there's no mac-specific override", () => {
  assert.deepEqual(effectiveHotkey(baseAction, true), { hotkey: "F1", altHotkey: null });
});

test("effectiveHotkey: Mac prefers macHotkey/macAltHotkey when KiCad's source gives macOS a genuinely different default", () => {
  // zoomIn: F1 on Windows/Linux, Cmd+'+' on macOS (common/tool/actions.cpp).
  const zoomIn = { hotkey: "F1", altHotkey: null, macHotkey: "Ctrl++", macAltHotkey: null };
  assert.deepEqual(effectiveHotkey(zoomIn, false), { hotkey: "F1", altHotkey: null });
  assert.deepEqual(effectiveHotkey(zoomIn, true), { hotkey: "Ctrl++", altHotkey: null });
});

test("effectiveHotkey: common.Interactive.delete -- Del on Windows/Linux, Backspace on macOS", () => {
  const del = { hotkey: "Del", altHotkey: null, macHotkey: "Backspace", macAltHotkey: null };
  assert.deepEqual(effectiveHotkey(del, true), { hotkey: "Backspace", altHotkey: null });
});
