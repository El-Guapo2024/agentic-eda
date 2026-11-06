import { test } from "node:test";
import assert from "node:assert/strict";
import { eventToHotkey, eventToHotkeyCandidates, displayHotkey, effectiveHotkey, defaultHotkey, setUserHotkeyOverrides } from "./hotkeys";

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

function keyc(partial: Partial<{ key: string; code: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean }>) {
  return { key: "a", code: "KeyA", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...partial } as unknown as KeyboardEvent;
}

test("eventToHotkey: Insert is a named key", () => {
  assert.equal(eventToHotkey(keyc({ key: "Insert" }), false), "Insert");
});

test("eventToHotkeyCandidates: letters, digits and named keys have exactly one spelling", () => {
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "X", shiftKey: true }), false), ["Shift+X"]);
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "4" }), false), ["4"]);
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "Delete", shiftKey: true }), false), ["Shift+Del"]);
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "Shift", shiftKey: true }), false), []);
});

test("eventToHotkeyCandidates: Ctrl+'+' typed with Shift also matches KiCad's 'Ctrl++'", () => {
  // changeTrackLayerNext / zoomIn (macOS) are spelled "Ctrl++": the typed "+" already carries the Shift.
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "+", code: "Equal", ctrlKey: true, shiftKey: true }), false), ["Ctrl+Shift++", "Ctrl++", "Ctrl+Shift+="]);
  // no Shift (numpad '+'): the plain reading, then the number pad's own name (`WXK_NUMPAD_ADD`, "Num Pad +")
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "+", code: "NumpadAdd", ctrlKey: true }), false), ["Ctrl++", "Ctrl+Num Pad +"]);
});

test("eventToHotkeyCandidates: Ctrl+Shift+'.' (typed as '>') matches similarZone's 'Ctrl+Shift+.'", () => {
  const c = eventToHotkeyCandidates(keyc({ key: ">", code: "Period", ctrlKey: true, shiftKey: true }), false);
  assert.ok(c.includes("Ctrl+Shift+."), c.join(" | "));
  assert.equal(c[0], "Ctrl+Shift+>");
});

test("eventToHotkeyCandidates: Cmd stands in for Ctrl on Mac in every alternate", () => {
  const c = eventToHotkeyCandidates(keyc({ key: "+", code: "Equal", metaKey: true, shiftKey: true }), true);
  assert.deepEqual(c, ["Ctrl+Shift++", "Ctrl++", "Ctrl+Shift+="]);
});

test("eventToHotkeyCandidates: a number pad key also reads as KiCad's Num Pad name; Page Up/Down are PgUp/PgDn", () => {
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "7", code: "Numpad7" }), false), ["7", "Num Pad 7"]);
  assert.deepEqual(eventToHotkeyCandidates(keyc({ key: "Enter", code: "NumpadEnter", shiftKey: true }), false), ["Shift+Enter", "Shift+Num Pad Enter"]);
  assert.equal(eventToHotkey(keyc({ key: "PageUp", code: "PageUp" }), false), "PgUp");
  assert.equal(eventToHotkey(keyc({ key: "PageDown", code: "PageDown", ctrlKey: true }), false), "Ctrl+PgDn");
});

test("defaultHotkey: the table's wrong defaults, read again from the C++ (`#ifdef __WXMAC__` arms, KiCad 10's Cycle Arc Drawing Mode)", () => {
  const f1 = { hotkey: "F1", altHotkey: null, macHotkey: null, macAltHotkey: null };
  assert.deepEqual(defaultHotkey({ ...f1, name: "pcbnew.PointEditor.addCorner" }, false), { hotkey: "Insert", altHotkey: null });
  assert.deepEqual(defaultHotkey({ ...f1, name: "pcbnew.PointEditor.addCorner" }, true), { hotkey: "F1", altHotkey: null });
  assert.deepEqual(defaultHotkey({ ...f1, name: "eeschema.InteractiveEdit.repeatDrawItem" }, false), { hotkey: "Insert", altHotkey: null });
  const zone = { name: "pcbnew.InteractiveDrawing.zone", hotkey: "Ctrl+Alt+Shift+Z", altHotkey: null, macHotkey: null, macAltHotkey: null };
  assert.deepEqual(defaultHotkey(zone, false), { hotkey: "Ctrl+Shift+Z", altHotkey: null });
  assert.deepEqual(defaultHotkey(zone, true), { hotkey: "Alt+Z", altHotkey: null });
  assert.equal(defaultHotkey({ name: "common.InteractiveDrawing.cycleArcDrawMode", hotkey: null, altHotkey: null, macHotkey: null, macAltHotkey: null }, false).hotkey, "Alt+/");
  // no name: the table as it is
  assert.deepEqual(defaultHotkey(f1, false), { hotkey: "F1", altHotkey: null });
});

test("effectiveHotkey: the person's hotkeys (user.hotkeys) first, then the default", () => {
  const route = { name: "pcbnew.InteractiveRouter.SingleTrack", hotkey: "X", altHotkey: null, macHotkey: null, macAltHotkey: null };
  try {
    setUserHotkeyOverrides(new Map([["pcbnew.InteractiveRouter.SingleTrack", { hotkey: "Shift+K", altHotkey: "F9" }]]));
    assert.deepEqual(effectiveHotkey(route, false), { hotkey: "Shift+K", altHotkey: "F9" });
    assert.deepEqual(effectiveHotkey(route, true), { hotkey: "Shift+K", altHotkey: "F9" });
    assert.deepEqual(effectiveHotkey({ ...route, name: "pcbnew.Other.action" }, false), { hotkey: "X", altHotkey: null }, "an action the file does not name keeps its default");
    setUserHotkeyOverrides(new Map([["pcbnew.InteractiveRouter.SingleTrack", { hotkey: null, altHotkey: null }]]));
    assert.deepEqual(effectiveHotkey(route, false), { hotkey: null, altHotkey: null }, "a cleared hotkey stays cleared");
  } finally {
    setUserHotkeyOverrides(null);
  }
  assert.deepEqual(effectiveHotkey(route, false), { hotkey: "X", altHotkey: null });
});
