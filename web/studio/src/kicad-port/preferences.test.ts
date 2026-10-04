import { test } from "node:test";
import assert from "node:assert/strict";
import {
  assignScrollModifier,
  DEFAULT_PREFERENCES,
  loadPreferences,
  MOUSE_SCROLL_DEFAULTS,
  PREFERENCES_STORAGE_KEY,
  sanitizePreferences,
  savePreferences,
  scrollModSetValid,
  toViewControlSettings,
  TRACKPAD_SCROLL_DEFAULTS,
  zoomControllerFor,
} from "./preferences";
import { AcceleratingZoomController } from "./zoomController";

test("the defaults are KiCad's: wheel zooms, Ctrl pans left/right, Shift pans up/down, auto pan off, automatic zoom speed", () => {
  const d = DEFAULT_PREFERENCES;
  assert.equal(d.scrollModifierZoom, "none");
  assert.equal(d.scrollModifierPanH, "ctrl");
  assert.equal(d.scrollModifierPanV, "shift");
  assert.equal(d.autoPan, false);
  assert.equal(d.autoPanAcceleration, 5);
  assert.equal(d.zoomAcceleration, false);
  assert.equal(d.zoomSpeedAuto, true);
  assert.equal(d.zoomSpeed, 5);
  assert.ok(scrollModSetValid(d));
  assert.deepEqual({ ...d, ...MOUSE_SCROLL_DEFAULTS }, d);
});

test("trackpad defaults: Ctrl+scroll zooms, Shift pans left/right, plain scroll pans up/down", () => {
  const p = { ...DEFAULT_PREFERENCES, ...TRACKPAD_SCROLL_DEFAULTS };
  assert.deepEqual([p.scrollModifierZoom, p.scrollModifierPanH, p.scrollModifierPanV], ["ctrl", "shift", "none"]);
  assert.ok(scrollModSetValid(p));
});

test("picking a key another action holds moves that action to the first free column (none, Ctrl, Shift, Alt)", () => {
  // up/down takes Ctrl from left/right: left/right moves to Shift, which up/down just gave up
  const a = assignScrollModifier(DEFAULT_PREFERENCES, "panV", "ctrl");
  assert.deepEqual([a.scrollModifierZoom, a.scrollModifierPanH, a.scrollModifierPanV], ["none", "shift", "ctrl"]);
  assert.ok(scrollModSetValid(a));
  // zoom takes Shift: up/down (which held it) falls to "none", free now that zoom has left it
  const b = assignScrollModifier(DEFAULT_PREFERENCES, "zoom", "shift");
  assert.deepEqual([b.scrollModifierZoom, b.scrollModifierPanH, b.scrollModifierPanV], ["shift", "ctrl", "none"]);
  assert.ok(scrollModSetValid(b));
  // zoom on Alt, left/right takes Shift from up/down, which falls to "none"
  const c = assignScrollModifier({ ...DEFAULT_PREFERENCES, scrollModifierZoom: "alt" }, "panH", "shift");
  assert.deepEqual([c.scrollModifierZoom, c.scrollModifierPanH, c.scrollModifierPanV], ["alt", "shift", "none"]);
});

test("picking none for a second action is allowed but makes the set invalid, as in the C++ warning", () => {
  const p = assignScrollModifier(DEFAULT_PREFERENCES, "panV", "none");
  assert.equal(p.scrollModifierPanV, "none");
  assert.equal(scrollModSetValid(p), false);
});

test("the view-control settings carry the wheel, reverse and auto-pan preferences", () => {
  const v = toViewControlSettings({ ...DEFAULT_PREFERENCES, autoPan: true, autoPanAcceleration: 9, scrollModifierZoom: "ctrl", scrollModifierPanH: "shift", reverseScrollZoom: true, reverseScrollPanH: true });
  assert.equal(v.autoPanEnabled, true);
  assert.equal(v.autoPanAcceleration, 9);
  assert.equal(v.scrollModifierZoom, "ctrl");
  assert.equal(v.scrollModifierPanH, "shift");
  assert.equal(v.reverseScrollZoom, true);
  assert.equal(v.reverseScrollPanH, true);
});

test("zoom controller: automatic speed is the platform's, manual speed scales a constant controller or accelerates", () => {
  const near = (a: number, b: number) => assert.ok(Math.abs(a - b) < 1e-9, `${a} vs ${b}`);
  near(zoomControllerFor(DEFAULT_PREFERENCES, true).getScaleForRotation(10), 1.1); // macOS: 0.01 per unit
  near(zoomControllerFor(DEFAULT_PREFERENCES, false).getScaleForRotation(10), 1.05); // elsewhere: 0.005
  near(zoomControllerFor({ ...DEFAULT_PREFERENCES, zoomSpeedAuto: false, zoomSpeed: 10 }, true).getScaleForRotation(10), 1.1); // 10 * 0.001 per unit
  near(zoomControllerFor({ ...DEFAULT_PREFERENCES, zoomSpeedAuto: false, zoomSpeed: 1 }, true).getScaleForRotation(10), 1.01);
  assert.ok(zoomControllerFor({ ...DEFAULT_PREFERENCES, zoomAcceleration: true }, false) instanceof AcceleratingZoomController);
  assert.ok(zoomControllerFor({ ...DEFAULT_PREFERENCES, zoomAcceleration: true, zoomSpeedAuto: false }, true) instanceof AcceleratingZoomController);
  assert.ok(!(zoomControllerFor({ ...DEFAULT_PREFERENCES, zoomAcceleration: true }, true) instanceof AcceleratingZoomController), "macOS ignores zoom acceleration with automatic speed");
});

test("sanitizing: junk falls back to defaults, numbers are clamped, a clashing wheel assignment resets", () => {
  assert.deepEqual(sanitizePreferences(null), DEFAULT_PREFERENCES);
  assert.deepEqual(sanitizePreferences("x"), DEFAULT_PREFERENCES);
  const p = sanitizePreferences({ scrollModifierZoom: "meta", autoPan: "yes", autoPanAcceleration: 99, zoomSpeed: -3, zoomSpeedAuto: false, reverseScrollZoom: true });
  assert.equal(p.scrollModifierZoom, "none");
  assert.equal(p.autoPan, false);
  assert.equal(p.autoPanAcceleration, 10);
  assert.equal(p.zoomSpeed, 1);
  assert.equal(p.zoomSpeedAuto, false);
  assert.equal(p.reverseScrollZoom, true);
  const clash = sanitizePreferences({ scrollModifierZoom: "ctrl", scrollModifierPanH: "ctrl", scrollModifierPanV: "shift", autoPan: true });
  assert.deepEqual([clash.scrollModifierZoom, clash.scrollModifierPanH, clash.scrollModifierPanV], ["none", "ctrl", "shift"]);
  assert.equal(clash.autoPan, true, "only the wheel assignment resets");
});

test("preferences round-trip through storage and survive missing or throwing storage", () => {
  const mem = new Map<string, string>();
  const storage = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
  assert.deepEqual(loadPreferences(storage), DEFAULT_PREFERENCES);
  const mine = { ...DEFAULT_PREFERENCES, autoPan: true, scrollModifierZoom: "ctrl" as const, scrollModifierPanH: "shift" as const, scrollModifierPanV: "none" as const };
  savePreferences(storage, mine);
  assert.deepEqual(loadPreferences(storage), mine);
  mem.set(PREFERENCES_STORAGE_KEY, "{not json");
  assert.deepEqual(loadPreferences(storage), DEFAULT_PREFERENCES);
  assert.deepEqual(loadPreferences(null), DEFAULT_PREFERENCES);
  const throwing = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
  };
  assert.deepEqual(loadPreferences(throwing), DEFAULT_PREFERENCES);
  savePreferences(throwing, mine); // must not throw
  savePreferences(undefined, mine);
});
