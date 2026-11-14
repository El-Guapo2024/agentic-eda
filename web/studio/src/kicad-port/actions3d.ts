// The 3D view's keys (3d-viewer/3d_viewer/tools/eda_3d_actions.cpp's `DefaultHotkey()`s and `EDA_3D_CONTROLLER::setTransitions`, KiCad 8303b2ad) and
// eda_3d_controller.cpp's RotateView/PanControl's axis/sign tables. The 3D tab dispatches its keys itself (a listener on the 3D canvas, Viewer3D.tsx, so they only
// fire while that canvas has focus, with `stopPropagation()` on every key it handles) rather than through the shared dispatcher (useGlobalHotkeys.ts): the 3D
// viewer's actions are not in src/kicad/actions.json. Their keys are the person's when Preferences > Hotkeys gave them some -- the page lists the 3D viewer's
// actions in a "3D Viewer" section (UI-LEFT-3 C3-4) -- else KiCad's defaults (src/kicad/actions3d.json, tools/extract-actions-3d.js):
//   pivotCenter Space; rotateZCW Shift+R, rotateZCCW R; moveLeft/Right/Up/Down the arrows; homeView Home; flipView F; viewFront Y, viewBack Shift+Y, viewRight X,
//   viewLeft Shift+X, viewTop Z, viewBottom Shift+Z; attributesTHT T, attributesSMD S, attributesOther V, attribute_not_in_posfile P, attribute_dnp D
// plus the common actions the controller takes (`ACTIONS::zoomIn` F1, `zoomOut` F2, `zoomFitScreen` Home, `zoomRedraw` F5 -- Cmd+'+', Cmd+'-', Cmd+0 and Cmd+R
// on macOS, each platform its own as KiCad's `#ifdef __WXMAC__` gives it -- and `panUp` .. `panRight`, Shift and an arrow).
import actionsData from "../kicad/actions.json";
import actions3dData from "../kicad/actions3d.json";
import type { ActionsFile, KicadAction } from "../kicad/types";
import { effectiveHotkey, eventToHotkeyCandidates } from "../actions/hotkeys";
import { GRID3D_ACTIONS, type Grid3DType } from "./grid3d";

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
  /** `KeyboardEvent.code`, for the number pad's own keys (actions/hotkeys.ts `eventToHotkeyCandidates`). */
  code?: string;
}

/** The Appearance rows the five model-attribute actions toggle (`EDA_3D_CONTROLLER::ToggleVisibility`'s `flipLayer( LAYER_3D_* )`). */
export type ModelRow3D = "th_models" | "smd_models" | "virtual_models" | "not_in_pos_models" | "dnp_models";

/**
 * What a key does in the 3D view: a camera action, a model row toggled, the projection toggled (`ToggleOrtho`), the board reloaded (`ReloadBoard`), the 3D grid
 * (`On3DGridSelection`), the material mode (`SetMaterial`), the navigator or the bounding boxes toggled (`ToggleVisibility`), the view copied or exported
 * (`ExportImage`), or the Appearance manager toggled (`ToggleLayersManager`).
 */
export type KeyTarget3D =
  | Action3D
  | { kind: "modelRow"; row: ModelRow3D }
  | { kind: "ortho" }
  | { kind: "reload" }
  | { kind: "grid"; grid: Grid3DType }
  | { kind: "material"; mode: "realistic" | "solid" | "cad" }
  | { kind: "row"; row: "navigator" | "bounding_boxes" }
  | { kind: "copy" }
  | { kind: "export" }
  | { kind: "layersManager" }
  | { kind: "missingModels" };

/**
 * `EDA_3D_CONTROLLER::setTransitions`: the actions a key reaches in the 3D view, by name -- the viewer's own (`EDA_3D_ACTIONS`, src/kicad/actions3d.json) first, then the
 * common ones it takes (`ACTIONS::panUp` .. `panRight`, `zoomRedraw`, `zoomInCenter`, `zoomOutCenter`, `zoomFitScreen`, and `zoomIn` / `zoomOut` -- "because F1 and F2
 * keys generate a zoomIn/zoomOut event, these events must be captured to use these hot keys"), as `ACTION_MANAGER::RunHotKey` prefers the active tool's actions to the
 * global ones (Home is both Home View and Zoom to Fit, the same view). Every other 3D action with a handler is here too, so a key a person gives it acts
 * (UI-LEFT-3 C3-4): the 3D grids, the materials, the navigator and the bounding boxes, Copy 3D Image to Clipboard, Export Image..., Show Appearance Manager.
 * Show parts without 3D model (`ToggleShowMissingModels`, `render.show_missing_models`) too (UI-LEFT-3 X3-2). Not here, as it does nothing in the studio: Use
 * raytracing (one render engine).
 */
export const KEYED_ACTIONS_3D: ReadonlyArray<readonly [string, KeyTarget3D]> = [
  ["3DViewer.Control.pivotCenter", { kind: "pivot" }],
  ["3DViewer.Control.rotateXclockwise", { kind: "rotate", axis: "x", dir: "cw" }],
  ["3DViewer.Control.rotateXcounterclockwise", { kind: "rotate", axis: "x", dir: "ccw" }],
  ["3DViewer.Control.rotateYclockwise", { kind: "rotate", axis: "y", dir: "cw" }],
  ["3DViewer.Control.rotateYcounterclockwise", { kind: "rotate", axis: "y", dir: "ccw" }],
  ["3DViewer.Control.rotateZclockwise", { kind: "rotate", axis: "z", dir: "cw" }],
  ["3DViewer.Control.rotateZcounterclockwise", { kind: "rotate", axis: "z", dir: "ccw" }],
  ["3DViewer.Control.moveLeft", { kind: "pan", direction: "left" }],
  ["3DViewer.Control.moveRight", { kind: "pan", direction: "right" }],
  ["3DViewer.Control.moveUp", { kind: "pan", direction: "up" }],
  ["3DViewer.Control.moveDown", { kind: "pan", direction: "down" }],
  ["3DViewer.Control.homeView", { kind: "reset" }],
  ["3DViewer.Control.flipView", { kind: "flip" }],
  ["3DViewer.Control.viewFront", { kind: "viewPreset", preset: "front" }],
  ["3DViewer.Control.viewBack", { kind: "viewPreset", preset: "back" }],
  ["3DViewer.Control.viewLeft", { kind: "viewPreset", preset: "left" }],
  ["3DViewer.Control.viewRight", { kind: "viewPreset", preset: "right" }],
  ["3DViewer.Control.viewTop", { kind: "viewPreset", preset: "top" }],
  ["3DViewer.Control.viewBottom", { kind: "viewPreset", preset: "bottom" }],
  ["3DViewer.Control.attributesTHT", { kind: "modelRow", row: "th_models" }],
  ["3DViewer.Control.attributesSMD", { kind: "modelRow", row: "smd_models" }],
  ["3DViewer.Control.attributesOther", { kind: "modelRow", row: "virtual_models" }],
  ["3DViewer.Control.attribute_not_in_posfile", { kind: "modelRow", row: "not_in_pos_models" }],
  ["3DViewer.Control.attribute_dnp", { kind: "modelRow", row: "dnp_models" }],
  ["3DViewer.Control.toggleOrtho", { kind: "ortho" }],
  ["3DViewer.Control.reloadBoard", { kind: "reload" }],
  ...Object.entries(GRID3D_ACTIONS).map(([name, grid]): readonly [string, KeyTarget3D] => [name, { kind: "grid", grid }]),
  ["3DViewer.Control.materialNormal", { kind: "material", mode: "realistic" }],
  ["3DViewer.Control.materialDiffuse", { kind: "material", mode: "solid" }],
  ["3DViewer.Control.materialCAD", { kind: "material", mode: "cad" }],
  ["3DViewer.Control.showNavigator", { kind: "row", row: "navigator" }],
  ["3DViewer.Control.showBoundingBoxes", { kind: "row", row: "bounding_boxes" }],
  ["3DViewer.Control.copyToClipboard", { kind: "copy" }],
  ["3DViewer.Control.exportImage", { kind: "export" }],
  ["3DViewer.Control.showLayersManager", { kind: "layersManager" }],
  // `EDA_3D_CONTROLLER::ToggleShowMissingModels` (UI-LEFT-3 X3-2)
  ["3DViewer.Control.toggleShowMissingModels", { kind: "missingModels" }],
  ["common.Control.panUp", { kind: "pan", direction: "up" }],
  ["common.Control.panDown", { kind: "pan", direction: "down" }],
  ["common.Control.panLeft", { kind: "pan", direction: "left" }],
  ["common.Control.panRight", { kind: "pan", direction: "right" }],
  ["common.Control.zoomRedraw", { kind: "zoomRedraw" }],
  ["common.Control.zoomInCenter", { kind: "zoomIn" }],
  ["common.Control.zoomOutCenter", { kind: "zoomOut" }],
  ["common.Control.zoomFitScreen", { kind: "reset" }],
  ["common.Control.zoomIn", { kind: "zoomIn" }],
  ["common.Control.zoomOut", { kind: "zoomOut" }],
];

const tables: ReadonlyMap<string, KicadAction> = new Map([...(actionsData as unknown as ActionsFile).actions, ...(actions3dData as unknown as { actions: KicadAction[] }).actions].map((a) => [a.name, a]));

/**
 * `ACTION_MANAGER::RunHotKey` in the 3D view: the first of `KEYED_ACTIONS_3D` whose hotkey or alternate -- the person's (Preferences > Hotkeys' `user.hotkeys`), else
 * KiCad's default for the platform (`TOOL_ACTION::GetHotKey`, actions/hotkeys.ts `effectiveHotkey`) -- is the key pressed, with exactly its modifiers (Shift, Ctrl
 * or Alt held with a bare key is another key). Null for any other key: the caller does nothing with it (not even preventDefault), so browser shortcuts and the
 * studio's shared hotkeys keep working.
 */
export function resolve3DKey(e: KeyInput, isMac: boolean): KeyTarget3D | null {
  const candidates = eventToHotkeyCandidates(e as unknown as KeyboardEvent, isMac);
  if (candidates.length === 0) return null;
  for (const [name, target] of KEYED_ACTIONS_3D) {
    const a = tables.get(name);
    if (!a) continue;
    const { hotkey, altHotkey } = effectiveHotkey(a, isMac);
    if ((hotkey && candidates.includes(hotkey)) || (altHotkey && candidates.includes(altHotkey))) return target;
  }
  return null;
}

/** `resolve3DKey` for the camera's actions only (the view presets, rotations, moves, zooms, pivot, flip and home); null for the rest. */
export function resolve3DAction(e: KeyInput, isMac: boolean): Action3D | null {
  const t = resolve3DKey(e, isMac);
  const camera: ReadonlySet<string> = new Set(["viewPreset", "reset", "flip", "pivot", "zoomIn", "zoomOut", "zoomRedraw", "rotate", "pan"]);
  return t && camera.has(t.kind) ? (t as Action3D) : null;
}

/** One entry of the 3D view's right-click menu: an action by its KiCad name, a separator, or a submenu with its title and icon. */
export type ContextItem3D = { action: string } | { separator: true } | { submenu: string; icon: string; items: ContextItem3D[] };

/**
 * `EDA_3D_CONTROLLER::Init`: the menu a right click on the view opens (`m_menu`, a click that was not a drag): Zoom In and Zoom Out (`ACTIONS::zoomInCenter`,
 * `zoomOutCenter`), the six views (top, bottom, right, left, front, back, in that order), the "Rotate Board" submenu (X, Y and Z, clockwise then counterclockwise, a
 * separator between the axes), Flip Board and the "Move Board" submenu (left, right, up, down).
 */
export const CONTEXT_MENU_3D: readonly ContextItem3D[] = [
  { action: "common.Control.zoomInCenter" },
  { action: "common.Control.zoomOutCenter" },
  { separator: true },
  { action: "3DViewer.Control.viewTop" },
  { action: "3DViewer.Control.viewBottom" },
  { action: "3DViewer.Control.viewRight" },
  { action: "3DViewer.Control.viewLeft" },
  { action: "3DViewer.Control.viewFront" },
  { action: "3DViewer.Control.viewBack" },
  { separator: true },
  {
    submenu: "Rotate Board",
    icon: "rotate_cw",
    items: [
      { action: "3DViewer.Control.rotateXclockwise" },
      { action: "3DViewer.Control.rotateXcounterclockwise" },
      { separator: true },
      { action: "3DViewer.Control.rotateYclockwise" },
      { action: "3DViewer.Control.rotateYcounterclockwise" },
      { separator: true },
      { action: "3DViewer.Control.rotateZclockwise" },
      { action: "3DViewer.Control.rotateZcounterclockwise" },
    ],
  },
  { action: "3DViewer.Control.flipView" },
  { submenu: "Move Board", icon: "move", items: [{ action: "3DViewer.Control.moveLeft" }, { action: "3DViewer.Control.moveRight" }, { action: "3DViewer.Control.moveUp" }, { action: "3DViewer.Control.moveDown" }] },
];

/** What a menu action of the 3D view does, as the toolbar and the keys do it; `flip` is Flip Board (the viewer's flip switch), null for a name that is none of them. */
export function contextAction3D(name: string): Action3D | { kind: "flipSwitch" } | null {
  const view = /^3DViewer\.Control\.view(Top|Bottom|Front|Back|Left|Right)$/.exec(name);
  if (view) return { kind: "viewPreset", preset: view[1]!.toLowerCase() as ViewFacePreset };
  const rot = /^3DViewer\.Control\.rotate([XYZ])(clockwise|counterclockwise)$/.exec(name);
  if (rot) return { kind: "rotate", axis: rot[1]!.toLowerCase() as RotateAxis, dir: rot[2] === "clockwise" ? "cw" : "ccw" };
  const move = /^3DViewer\.Control\.move(Left|Right|Up|Down)$/.exec(name);
  if (move) return { kind: "pan", direction: move[1]!.toLowerCase() as PanDirection };
  if (name === "common.Control.zoomInCenter") return { kind: "zoomIn" };
  if (name === "common.Control.zoomOutCenter") return { kind: "zoomOut" };
  if (name === "3DViewer.Control.flipView") return { kind: "flipSwitch" };
  return null;
}
