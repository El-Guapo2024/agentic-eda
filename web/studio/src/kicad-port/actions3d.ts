// Port of 3d-viewer/3d_viewer/tools/eda_3d_actions.cpp's DefaultHotkey()
// bindings and eda_3d_controller.cpp's RotateView/PanControl's axis/sign
// tables -- the 3D tab's OWN keyboard dispatch, deliberately independent
// of this app's shared src/actions/* hotkey system (useGlobalHotkeys.ts,
// actions.json): another session is actively editing those shared files
// (and the Schematic tab), so the 3D tab's key handling lives entirely in
// viewer3d/ instead of adding new entries there. See Viewer3D.tsx for how
// this gets attached (a listener on the 3D canvas itself, not `window`,
// so it only ever fires while that canvas has focus, and
// e.stopPropagation() on every *handled* key so it can never also
// trigger an unrelated same-key shared action while bubbling).
//
// Visibility-toggle hotkeys (T/S/V/P/D -- show TH/SMD/virtual/
// not-in-pos/DNP models) are intentionally NOT here: the task's own
// phase split puts those under "Appearance" (visibility toggles), not
// "Actions and hotkeys" -- see PARITY-3d.md.
//
// eda_3d_actions.cpp's own DefaultHotkey() calls, verbatim:
//   pivotCenter    Space
//   rotateZCW      Shift+R      rotateZCCW   R
//   rotateXCW/CCW, rotateYCW/CCW: no default hotkey (toolbar/menu only)
//   moveLeft/Right/Up/Down: the 4 arrow keys
//   homeView       Home  (VIEW3D_FIT_SCREEN -- observably == Top, camera3d.ts's reset())
//   flipView       F
//   viewFront Y   viewBack Shift+Y   viewRight X   viewLeft Shift+X
//   viewTop   Z   viewBottom Shift+Z
//   toggleOrtho: no default hotkey (toolbar/menu only)
// Plus 4 actions EDA_3D_ACTIONS reuses unchanged from common/tool/
// actions.cpp (ACTIONS::zoomIn/zoomOut/zoomFitScreen/zoomRedraw):
// F1/F2/Home/F5, each with a genuine macOS-only override in source
// (Cmd+'+'/Cmd+'-'/Cmd+0/Cmd+R -- same ones PARITY-pcb.md's "Hotkey
// extraction fix" documents for the 2D app). Ported here as an ADDITIVE
// superset instead (both the function key AND the Cmd-equivalent work on
// every platform) rather than a true per-platform substitution -- simpler
// and strictly more capable, not a fidelity loss; noted in PARITY-3d.md.

export type ViewFacePreset = "top" | "bottom" | "front" | "back" | "left" | "right";
export type RotateAxis = "x" | "y" | "z";
export type RotateDir = "cw" | "ccw";
export type PanDirection = "left" | "right" | "up" | "down";

export type Action3D =
  | { kind: "viewPreset"; preset: ViewFacePreset }
  | { kind: "reset" }
  | { kind: "flip" }
  | { kind: "pivot" }
  | { kind: "zoomIn" }
  | { kind: "zoomOut" }
  | { kind: "zoomRedraw" }
  | { kind: "rotate"; axis: RotateAxis; dir: RotateDir }
  | { kind: "pan"; direction: PanDirection };

/**
 * eda_3d_controller.cpp RotateView's per-axis sign table, ported
 * verbatim including its own "Y rotations are backward b/c the RHR has Y
 * pointing into the screen" comment -- X_CW:RotateX(-inc)/X_CCW:(+inc),
 * Y_CW:RotateY(+inc)/Y_CCW:(-inc), Z_CW:RotateZ(-inc)/Z_CCW:(+inc).
 */
export const ROTATE_SIGN: Record<RotateAxis, Record<RotateDir, 1 | -1>> = {
  x: { cw: -1, ccw: 1 },
  y: { cw: 1, ccw: -1 },
  z: { cw: -1, ccw: 1 },
};

export interface KeyInput {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

/**
 * `e.key` for the 4 arrow keys, matched case-sensitively like everything
 * else here -- DOM's own names, not wx's WXK_LEFT/etc spelling.
 */
const ARROW_TO_PAN: Record<string, PanDirection> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

/**
 * Resolves a keydown event to one of this file's Action3D variants, or
 * null if it isn't one of eda_3d_actions.cpp's own bindings (or the 4
 * reused common actions) -- the caller should do nothing with a null
 * result (not even preventDefault), so every other key keeps working
 * normally (browser shortcuts, this app's own shared hotkey system).
 *
 * `ctrlLike` mirrors actions/hotkeys.ts's own eventToHotkey convention
 * (Cmd stands in for Ctrl on macOS) for the 4 reused zoom actions only --
 * every *real* eda_3d_actions.cpp hotkey below uses Shift or nothing, so
 * Ctrl/Cmd/Alt held down for any of those simply won't match (the exact
 * key string has to match with no extra modifier), the same way source's
 * own exact-hotkey-string matching works.
 */
export function resolve3DAction(e: KeyInput, isMac: boolean): Action3D | null {
  const ctrlLike = isMac ? e.metaKey : e.ctrlKey;

  // The 4 reused common/tool/actions.cpp actions -- checked first since
  // their Mac form uses a modifier the rest of this table never does.
  if (e.key === "F1" || (isMac && ctrlLike && e.key === "+")) return { kind: "zoomIn" };
  if (e.key === "F2" || (isMac && ctrlLike && e.key === "-")) return { kind: "zoomOut" };
  if ((e.key === "Home" && !ctrlLike) || (isMac && ctrlLike && e.key === "0")) return { kind: "reset" };
  if (e.key === "F5" || (isMac && ctrlLike && (e.key === "r" || e.key === "R"))) return { kind: "zoomRedraw" };

  // Nothing below ever uses Ctrl/Cmd or Alt -- bail out so e.g. Ctrl+R
  // (browser reload) or an unrelated Alt-combo never gets misread as one
  // of these plain/Shift-only keys.
  if (ctrlLike || e.altKey) return null;

  const pan = ARROW_TO_PAN[e.key];
  if (pan) return { kind: "pan", direction: pan };

  if (e.key === " " || e.key === "Spacebar") return { kind: "pivot" };
  if (e.key === "f" || e.key === "F") return { kind: "flip" };

  // R/Shift+R: eda_3d_actions.cpp binds plain 'R' to rotateZCCW and
  // Shift+'R' to rotateZCW -- source's DefaultHotkey('R') / DefaultHotkey(
  // MD_SHIFT+'R') don't care about the Shift key's effect on case, only
  // whether the modifier was held, so both e.key spellings ('r'/'R') are
  // accepted per branch rather than relying on JS's own shift-changes-the-
  // letter behavior.
  if ((e.key === "r" || e.key === "R") && e.shiftKey) return { kind: "rotate", axis: "z", dir: "cw" };
  if (e.key === "r" || e.key === "R") return { kind: "rotate", axis: "z", dir: "ccw" };

  switch (e.key) {
    case "y":
    case "Y":
      return { kind: "viewPreset", preset: e.shiftKey ? "back" : "front" };
    case "x":
    case "X":
      return { kind: "viewPreset", preset: e.shiftKey ? "left" : "right" };
    case "z":
    case "Z":
      return { kind: "viewPreset", preset: e.shiftKey ? "bottom" : "top" };
    default:
      return null;
  }
}
