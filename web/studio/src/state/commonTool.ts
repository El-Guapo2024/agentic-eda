// The shared interactive tools that run over any editor's canvas, and the one running now.
//
// KiCad's `PICKER_TOOL` (common/tool/picker_tool.cpp) is a tool other tools borrow: it waits for the user to click a point,
// calls the click handler of whoever started it (`SetClickHandler`) with the picked position -- the handler returns whether to go
// on picking -- and the motion, cancel and finalize handlers on the way. The interactive delete tool, "place the grid origin" and
// "pick a new group member" are all built on it. The selection tool's lasso (`selectLasso`) is the other tool here: a polygon the
// user draws that selects what it touches.
//
// The store holds the running tool; `components/CommonToolHost.tsx` owns the pointer and draws the preview. Handlers are plain
// functions kept here (not in a React reducer), so a tool started by an action can read the editor's state when its click arrives.
import { useSyncExternalStore } from "react";

export interface PickPoint {
  x: number;
  y: number;
}

/** `PICKER_TOOL::Main`'s `finalize_state`: why the tool ended. */
export type PickerEnd = "click" | "cancel" | "activate";

export interface PickerRequest {
  /** What the status bar says while it runs (`m_frame->PushTool( sourceEvent )` shows the starter's name). */
  message: string;
  /** `PICKER_TOOL_BASE::m_snap` (default true): hand the handlers the grid-snapped cursor, not the raw one. */
  snap: boolean;
  /** `SetCursor`: the pointer shown over the canvas (`ARROW`, `PLACE`, `REMOVE`). */
  cursor: "default" | "place" | "remove";
  /** `pickerSubTool`: runs inside another tool without ending it. */
  sub?: boolean;
  /** `SetClickHandler`: return true to keep picking (`getNext`), false (or nothing) to end after this click. */
  onClick?: (pt: PickPoint) => boolean | void;
  /** `SetMotionHandler`. */
  onMotion?: (pt: PickPoint) => void;
  /** `SetCancelHandler`: Escape or a right click. */
  onCancel?: () => void;
  /** `SetFinalizeHandler`: always, with why it ended. */
  onFinalize?: (end: PickerEnd) => void;
}

/** The lasso being drawn (`SelectPolyArea` / `selectLasso`'s `points`). */
export interface LassoSession {
  pts: [number, number][];
  /** True while the button is down, i.e. while drag events add points. */
  dragging: boolean;
  /** Held modifiers decide how the result changes the selection. */
  additive: boolean;
  subtractive: boolean;
}

export interface CommonToolState {
  picker: PickerRequest | null;
  /** The id the picker's motion handler last found under the cursor, drawn highlighted (`BrightenItem`). */
  hover: string | null;
  lasso: LassoSession | null;
}

const IDLE: CommonToolState = { picker: null, hover: null, lasso: null };

let current: CommonToolState = IDLE;
const listeners = new Set<() => void>();

function set(next: CommonToolState): void {
  current = next;
  for (const l of listeners) l();
}

export function getCommonTool(): CommonToolState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useCommonTool(): CommonToolState {
  return useSyncExternalStore(subscribe, getCommonTool, getCommonTool);
}

/**
 * `ACTIONS::pickerTool` / `pickerSubTool`: start picking. Another picker that was running ends first ("Deactivate other tools;
 * particularly important if another PICKER is currently running" -- `Activate()` ends it, finalizing with `END_ACTIVATE`).
 */
export function startPicker(request: PickerRequest): void {
  endPicker("activate");
  set({ ...current, picker: request, hover: null, lasso: null });
}

/** Ends the running picker, running its finalize handler. */
export function endPicker(end: PickerEnd): void {
  const p = current.picker;
  if (!p) return;
  set({ ...current, picker: null, hover: null });
  try {
    p.onFinalize?.(end);
  } catch {
    /* PICKER_TOOL::Main swallows a finalize handler's exception */
  }
}

/** `evt->IsCancelInteractive()`: Escape or a right click cancels the picker, running its cancel handler first. */
export function cancelPicker(): boolean {
  const p = current.picker;
  if (!p) return false;
  try {
    p.onCancel?.();
  } catch {
    /* swallowed, as in PICKER_TOOL::Main */
  }
  endPicker("cancel");
  return true;
}

export function setPickerHover(id: string | null): void {
  if (current.hover !== id) set({ ...current, hover: id });
}

export function setLasso(lasso: LassoSession | null): void {
  set({ ...current, lasso });
}

/** Escape: cancels whichever shared tool is running (the picker, or a lasso in the making). True when one was. */
export function cancelCommonTool(): boolean {
  if (current.lasso) {
    set({ ...current, lasso: null });
    return true;
  }
  return cancelPicker();
}
