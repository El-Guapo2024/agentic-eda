// What the shared pointer tools show over any editor's canvas while they run: the lasso being drawn
// (`SELECTION_TOOL::selectLasso` / `PCB_SELECTION_TOOL::SelectPolyArea`), the item the picker's motion handler highlights
// (`PICKER_TOOL` motion handler -> `BrightenItem`, as the interactive delete tool does) and, in the library editors, the rectangle of the
// zoom tool (`ZOOM_TOOL::selectRegion`; the board and the schematic have components/ZoomAreaOverlay.tsx).
//
// The picker itself is `actions/pcbPicker.ts` (`PickerHost`); `components/CommonToolHost.tsx` owns the pointer for the editors
// whose canvases do not answer a picker themselves, and draws these two. Plain module state with subscribers, like
// `state/commonOptions.ts`, so an action started from a menu and the canvas see the same tool.
import { useSyncExternalStore } from "react";

/** The lasso being drawn (`points` of `SelectPolyArea`): world points in the editor's own space. */
export interface LassoSession {
  pts: readonly (readonly [number, number])[];
  /** True while the button is down, i.e. while drag events add points (a click-by-click lasso goes on after the release). */
  dragging: boolean;
  /** Shift held at the start: the hits are added to the selection (`m_drag_additive`). */
  additive: boolean;
  /** Ctrl+Shift held at the start: the hits are removed from it (`m_drag_subtractive`). */
  subtractive: boolean;
}

/** The clarification menu (`SELECTION_TOOL::doSelectionMenu`): the items a click could mean, listed by description, where they were clicked. */
/** The zoom tool while it is armed in a library editor (`ZOOM_TOOL::Main`): the box being dragged, in world points, and the button that drags it (left zooms in, right out). */
export interface ZoomAreaSession {
  drag: { a: readonly [number, number]; b: readonly [number, number]; button: number } | null;
}

export interface SelectionMenu {
  at: { x: number; y: number };
  items: { id: string; label: string }[];
  /** What the choice does: `ids` is the one item picked, or all of them for "Select All" (null: the menu was dismissed). */
  onChoose: (ids: string[] | null) => void;
}

export interface CommonToolState {
  /** The id of the item under the cursor that the running picker highlights, or null. */
  hover: string | null;
  lasso: LassoSession | null;
  /** The zoom tool is armed (`common.Control.zoomTool` in the Footprint or Symbol Editor); null when it is not. */
  zoomArea: ZoomAreaSession | null;
  menu: SelectionMenu | null;
  /** The items every match of the open Find dialog's search text sits on, brightened (`SCH_FIND_REPLACE_TOOL::UpdateFind`). */
  findHighlights: readonly string[];
}

const IDLE: CommonToolState = { hover: null, lasso: null, zoomArea: null, menu: null, findHighlights: [] };

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

/** `BrightenItem` / `UnbrightenItem` for the picker's candidate. */
export function setPickerHover(id: string | null): void {
  if (current.hover !== id) set({ ...current, hover: id });
}

export function setLasso(lasso: LassoSession | null): void {
  set({ ...current, lasso });
}

export function setZoomArea(zoomArea: ZoomAreaSession | null): void {
  set({ ...current, zoomArea });
}

export function setFindHighlights(ids: readonly string[]): void {
  const same = ids.length === current.findHighlights.length && ids.every((id, i) => id === current.findHighlights[i]);
  if (!same) set({ ...current, findHighlights: ids });
}

/** Where the pointer last was, in client pixels (`KIPLATFORM::UI::GetMousePosition()`): a menu opened from an action appears there. */
let lastPointer = { x: 0, y: 0 };

export function setLastPointer(x: number, y: number): void {
  lastPointer = { x, y };
}

export function getLastPointer(): { x: number; y: number } {
  return lastPointer;
}

/** `ACTIONS::selectionMenu`: show the clarification menu; a menu already open is dismissed first. */
export function showSelectionMenu(menu: SelectionMenu): void {
  closeSelectionMenu();
  set({ ...current, menu });
}

/** The menu is gone (a choice was made, or it was dismissed): runs `onChoose` with the choice, or null. */
export function closeSelectionMenu(choice: string[] | null = null): void {
  const menu = current.menu;
  if (!menu) return;
  set({ ...current, menu: null, hover: null });
  menu.onChoose(choice);
}

/**
 * Escape while a lasso is being drawn (`evt->IsCancelInteractive()` in `SelectPolyArea`): nothing is selected; or while the zoom tool is armed
 * (`ZOOM_TOOL::Main`'s `evt->IsCancelInteractive()`): the view stays. True when there was one of them.
 */
export function cancelAreaTool(): boolean {
  if (!current.lasso && !current.zoomArea) return false;
  set({ ...current, lasso: null, zoomArea: null });
  return true;
}
