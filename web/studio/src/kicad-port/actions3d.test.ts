import { test } from "node:test";
import assert from "node:assert/strict";
import { resolve3DAction, ROTATE_SIGN, type KeyInput } from "./actions3d";

function key(k: string, mods: Partial<Omit<KeyInput, "key">> = {}): KeyInput {
  return { key: k, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...mods };
}

test("view presets: Y/Shift+Y, X/Shift+X, Z/Shift+Z", () => {
  assert.deepEqual(resolve3DAction(key("y"), false), { kind: "viewPreset", preset: "front" });
  assert.deepEqual(resolve3DAction(key("y", { shiftKey: true }), false), { kind: "viewPreset", preset: "back" });
  assert.deepEqual(resolve3DAction(key("x"), false), { kind: "viewPreset", preset: "right" });
  assert.deepEqual(resolve3DAction(key("x", { shiftKey: true }), false), { kind: "viewPreset", preset: "left" });
  assert.deepEqual(resolve3DAction(key("z"), false), { kind: "viewPreset", preset: "top" });
  assert.deepEqual(resolve3DAction(key("z", { shiftKey: true }), false), { kind: "viewPreset", preset: "bottom" });
});

test("Home resets (eda_3d_actions.cpp homeView / shared zoomFitScreen)", () => {
  assert.deepEqual(resolve3DAction(key("Home"), false), { kind: "reset" });
  assert.deepEqual(resolve3DAction(key("Home"), true), { kind: "reset" });
});

test("F flips, Space is the pivot action", () => {
  assert.deepEqual(resolve3DAction(key("f"), false), { kind: "flip" });
  assert.deepEqual(resolve3DAction(key("F"), false), { kind: "flip" });
  assert.deepEqual(resolve3DAction(key(" "), false), { kind: "pivot" });
});

test("R/Shift+R rotate Z (CCW/CW)", () => {
  assert.deepEqual(resolve3DAction(key("r"), false), { kind: "rotate", axis: "z", dir: "ccw" });
  assert.deepEqual(resolve3DAction(key("r", { shiftKey: true }), false), { kind: "rotate", axis: "z", dir: "cw" });
});

test("arrow keys pan", () => {
  assert.deepEqual(resolve3DAction(key("ArrowLeft"), false), { kind: "pan", direction: "left" });
  assert.deepEqual(resolve3DAction(key("ArrowRight"), false), { kind: "pan", direction: "right" });
  assert.deepEqual(resolve3DAction(key("ArrowUp"), false), { kind: "pan", direction: "up" });
  assert.deepEqual(resolve3DAction(key("ArrowDown"), false), { kind: "pan", direction: "down" });
});

test("F1/F2 zoom in/out on every platform", () => {
  assert.deepEqual(resolve3DAction(key("F1"), false), { kind: "zoomIn" });
  assert.deepEqual(resolve3DAction(key("F1"), true), { kind: "zoomIn" });
  assert.deepEqual(resolve3DAction(key("F2"), false), { kind: "zoomOut" });
});

test("F5 is a zoomRedraw no-op action, reachable on every platform", () => {
  assert.deepEqual(resolve3DAction(key("F5"), false), { kind: "zoomRedraw" });
});

test("mac Cmd-equivalents are additive (both the function key and Cmd+key work)", () => {
  assert.deepEqual(resolve3DAction(key("+", { metaKey: true }), true), { kind: "zoomIn" });
  assert.deepEqual(resolve3DAction(key("-", { metaKey: true }), true), { kind: "zoomOut" });
  assert.deepEqual(resolve3DAction(key("0", { metaKey: true }), true), { kind: "reset" });
  assert.deepEqual(resolve3DAction(key("r", { metaKey: true }), true), { kind: "zoomRedraw" });
  // On non-mac, metaKey held doesn't substitute for ctrlLike, so these fall through to null.
  assert.equal(resolve3DAction(key("+", { metaKey: true }), false), null);
});

test("Ctrl/Cmd/Alt held for any plain-letter action blocks it (don't steal Ctrl+R, Ctrl+Z, etc)", () => {
  assert.equal(resolve3DAction(key("r", { ctrlKey: true }), false), null);
  assert.equal(resolve3DAction(key("f", { ctrlKey: true }), false), null);
  assert.equal(resolve3DAction(key("y", { altKey: true }), false), null);
  assert.equal(resolve3DAction(key("z", { metaKey: true }), true), null);
});

test("unrelated keys resolve to null", () => {
  assert.equal(resolve3DAction(key("q"), false), null);
  assert.equal(resolve3DAction(key("Enter"), false), null);
  assert.equal(resolve3DAction(key("Escape"), false), null);
});

test("ROTATE_SIGN matches eda_3d_controller.cpp's RotateView table (Y is the odd one out)", () => {
  assert.equal(ROTATE_SIGN.x.cw, -1);
  assert.equal(ROTATE_SIGN.x.ccw, 1);
  assert.equal(ROTATE_SIGN.y.cw, 1);
  assert.equal(ROTATE_SIGN.y.ccw, -1);
  assert.equal(ROTATE_SIGN.z.cw, -1);
  assert.equal(ROTATE_SIGN.z.ccw, 1);
});
