import { test } from "node:test";
import assert from "node:assert/strict";
import { resolve3DAction, resolve3DKey, ROTATE_SIGN, type KeyInput } from "./actions3d";
import { setUserHotkeyOverrides } from "../actions/hotkeys";

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

test("F1/F2 zoom in/out and F5 redraws off macOS (ACTIONS::zoomIn / zoomOut / zoomRedraw)", () => {
  assert.deepEqual(resolve3DAction(key("F1"), false), { kind: "zoomIn" });
  assert.deepEqual(resolve3DAction(key("F2"), false), { kind: "zoomOut" });
  assert.deepEqual(resolve3DAction(key("F5"), false), { kind: "zoomRedraw" });
});

test("macOS has its own defaults (`#ifdef __WXMAC__`): Cmd+'+', Cmd+'-', Cmd+0, Cmd+R -- and not the function keys", () => {
  assert.deepEqual(resolve3DAction(key("+", { metaKey: true }), true), { kind: "zoomIn" });
  assert.deepEqual(resolve3DAction(key("-", { metaKey: true }), true), { kind: "zoomOut" });
  assert.deepEqual(resolve3DAction(key("0", { metaKey: true }), true), { kind: "reset" });
  assert.deepEqual(resolve3DAction(key("r", { metaKey: true }), true), { kind: "zoomRedraw" });
  assert.equal(resolve3DAction(key("F1"), true), null);
  // off macOS the Meta key is no Ctrl
  assert.equal(resolve3DAction(key("+", { metaKey: true }), false), null);
});

test("C3-4: Shift and an arrow pan too (ACTIONS::panLeft .. panDown, which EDA_3D_CONTROLLER takes)", () => {
  assert.deepEqual(resolve3DAction(key("ArrowLeft", { shiftKey: true }), false), { kind: "pan", direction: "left" });
  assert.deepEqual(resolve3DAction(key("ArrowUp", { shiftKey: true }), false), { kind: "pan", direction: "up" });
});

test("C3-4: the model rows, the projection and Reload board are keys of the 3D view too", () => {
  assert.deepEqual(resolve3DKey(key("t"), false), { kind: "modelRow", row: "th_models" });
  assert.deepEqual(resolve3DKey(key("d"), false), { kind: "modelRow", row: "dnp_models" });
  assert.equal(resolve3DKey(key("d", { shiftKey: true }), false), null, "KiCad binds no Shift+D");
  assert.equal(resolve3DAction(key("t"), false), null, "not a camera action");
});

test("C3-4: a key the person gave a 3D action in Preferences > Hotkeys (user.hotkeys) replaces KiCad's", () => {
  try {
    setUserHotkeyOverrides(
      new Map([
        ["3DViewer.Control.viewTop", { hotkey: "Shift+T", altHotkey: null }],
        ["3DViewer.Control.toggleOrtho", { hotkey: "O", altHotkey: null }],
        ["3DViewer.Control.attributesTHT", { hotkey: null, altHotkey: null }],
      ])
    );
    assert.deepEqual(resolve3DAction(key("T", { shiftKey: true }), false), { kind: "viewPreset", preset: "top" });
    assert.equal(resolve3DAction(key("z"), false), null, "Z is no longer View Top's");
    assert.deepEqual(resolve3DKey(key("o"), false), { kind: "ortho" });
    assert.equal(resolve3DKey(key("t"), false), null, "THT's key cleared");
  } finally {
    setUserHotkeyOverrides(null);
  }
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

// V6-8 (round 6): eda_3d_actions.cpp binds 'F', ' ', WXK_LEFT .. WXK_DOWN and WXK_HOME with no modifier, and common/tool/actions.cpp binds
// WXK_F1 / WXK_F2 / WXK_F5 alone; a hotkey matches its exact modifiers, so Shift (or Ctrl, Alt) with them is no action.
test("F, Space, the arrows, Home and F1/F2/F5 do nothing with a modifier held (Shift and an arrow is Pan)", () => {
  for (const k of ["F", "f", " ", "ArrowLeft", "ArrowUp", "Home", "F1", "F2", "F5"]) {
    for (const mod of [{ shiftKey: true }, { ctrlKey: true }, { altKey: true }]) {
      if (mod.shiftKey && k.startsWith("Arrow")) continue;
      assert.equal(resolve3DAction(key(k, mod), false), null, `${JSON.stringify(k)} ${JSON.stringify(mod)}`);
    }
  }
  assert.equal(resolve3DAction(key("F1", { metaKey: true }), true), null);
  // Plain, they still act; the R / Y / X / Z Shift forms are their own bindings.
  assert.deepEqual(resolve3DAction(key("F"), false), { kind: "flip" });
  assert.deepEqual(resolve3DAction(key("ArrowLeft"), false), { kind: "pan", direction: "left" });
  assert.deepEqual(resolve3DAction(key("F1"), false), { kind: "zoomIn" });
});
