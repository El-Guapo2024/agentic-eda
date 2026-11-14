// What the scroll wheel does on a simulator plot: `SIM_MOUSE_WHEEL_ACTION_SET` (eeschema/sim/sim_preferences.h at KiCad 8303b2ad), the settings of Preferences >
// Schematic Editor > Simulator (`PANEL_SIMULATOR_PREFERENCES`, eeschema/dialogs/panel_simulator_preferences.cpp; `simulator.mouse_wheel_actions.*` of eeschema.json),
// read by `mpWindow::onMouseWheel` / `PerformMouseWheelAction` (common/widgets/mathplot.cpp).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { pan, zoomIn, zoomOut, type PlotView } from "./mathPlot";

/** `SIM_MOUSE_WHEEL_ACTION`, in the enum's order (the choices' order of the vertical rows). */
export const SIM_WHEEL_ACTIONS = ["none", "pan_left_right", "pan_right_left", "pan_up_down", "zoom", "zoom_horizontally", "zoom_vertically"] as const;
export type SimWheelAction = (typeof SIM_WHEEL_ACTIONS)[number];

/** `verticalChoiceItems`, word for word. */
export const SIM_WHEEL_LABELS: Record<SimWheelAction, string> = {
  none: "No action",
  pan_left_right: "Pan left/right",
  pan_right_left: "Pan right/left",
  pan_up_down: "Pan up/down",
  zoom: "Zoom",
  zoom_horizontally: "Zoom horizontally",
  zoom_vertically: "Zoom vertically",
};

/** `horizontalChoiceItems`: the horizontal row offers three (`horizontalScrollSelectionToAction`). */
export const SIM_WHEEL_HORIZONTAL: readonly SimWheelAction[] = ["none", "pan_left_right", "zoom_horizontally"];

export interface SimWheelActions {
  verticalUnmodified: SimWheelAction;
  verticalWithCtrl: SimWheelAction;
  verticalWithShift: SimWheelAction;
  verticalWithAlt: SimWheelAction;
  horizontal: SimWheelAction;
}

/** `GetMouseDefaults()`: "equivalent to the global Mouse and Touchpad default settings" -- and the settings' own defaults (`PARAM_ENUM`'s). */
export const DEFAULT_SIM_WHEEL_ACTIONS: SimWheelActions = { verticalUnmodified: "zoom", verticalWithCtrl: "pan_left_right", verticalWithShift: "pan_up_down", verticalWithAlt: "none", horizontal: "none" };

/** `GetTrackpadDefaults()`. */
export const TRACKPAD_SIM_WHEEL_ACTIONS: SimWheelActions = { verticalUnmodified: "pan_up_down", verticalWithCtrl: "zoom", verticalWithShift: "pan_left_right", verticalWithAlt: "none", horizontal: "pan_left_right" };

const action = (v: unknown, fallback: SimWheelAction): SimWheelAction => ((SIM_WHEEL_ACTIONS as readonly unknown[]).includes(v) ? (v as SimWheelAction) : fallback);

/** What was stored, made valid: `PARAM_ENUM`'s range takes every action; the horizontal row's `actionToHorizontalScrollSelection` shows any other as "No action". */
export function sanitizeSimWheelActions(raw: unknown): SimWheelActions {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_SIM_WHEEL_ACTIONS;
  const horizontal = action(o.horizontal, d.horizontal);
  return {
    verticalUnmodified: action(o.verticalUnmodified, d.verticalUnmodified),
    verticalWithCtrl: action(o.verticalWithCtrl, d.verticalWithCtrl),
    verticalWithShift: action(o.verticalWithShift, d.verticalWithShift),
    verticalWithAlt: action(o.verticalWithAlt, d.verticalWithAlt),
    horizontal: SIM_WHEEL_HORIZONTAL.includes(horizontal) ? horizontal : "none",
  };
}

/** The wheel event as `onMouseWheel` reads it: the axis and the modifiers held (`GetModifiers()`: exactly one, or none). */
export interface SimWheelEvent {
  horizontal: boolean;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
}

/**
 * `mpWindow::onMouseWheel`: the action of the event -- the horizontal row for a horizontal wheel, else the row of the one modifier held -- or null when several
 * modifiers are held (`event.Skip()`, nothing happens).
 */
export function simWheelAction(actions: SimWheelActions, e: SimWheelEvent): SimWheelAction | null {
  if (e.horizontal) return actions.horizontal;
  const held = [e.ctrl, e.shift, e.alt].filter(Boolean).length;
  if (held === 0) return actions.verticalUnmodified;
  if (held > 1) return null;
  return e.ctrl ? actions.verticalWithCtrl : e.shift ? actions.verticalWithShift : actions.verticalWithAlt;
}

/**
 * `mpWindow::PerformMouseWheelAction`: `change` is the wheel's rotation (positive away from the user), in the plot's pixels; the zooms are about `at`. Pans move the
 * view by `change / scale` (`PAN_LEFT_RIGHT` `m_posX + changeUnitsX`, `PAN_RIGHT_LEFT` the other way, `PAN_UP_DOWN` `m_posY + changeUnitsY` unless the y axis is
 * locked); the zooms zoom in for a positive rotation, out for a negative one, in both directions or one (`zoomIncrementalFactor`).
 */
export function performSimWheelAction(view: PlotView, act: SimWheelAction, change: number, at: { x: number; y: number }): void {
  switch (act) {
    case "none":
      return;
    case "pan_left_right":
      pan(view, change, 0);
      return;
    case "pan_right_left":
      pan(view, -change, 0);
      return;
    case "pan_up_down":
      // `pan` takes pixels downwards; `SetYView( m_posY + changeUnitsY )` moves the view up the plot's axis
      pan(view, 0, -change);
      return;
    case "zoom":
      if (change > 0) zoomIn(view, "both", at);
      else if (change < 0) zoomOut(view, "both", at);
      return;
    case "zoom_horizontally":
      if (change > 0) zoomIn(view, "h", at);
      else if (change < 0) zoomOut(view, "h", at);
      return;
    case "zoom_vertically":
      if (change > 0) zoomIn(view, "v", at);
      else if (change < 0) zoomOut(view, "v", at);
      return;
  }
}
