// Which of the shared tools' dialogs is open, and what they are editing: Group Properties, About, Page Settings, Embedded Files.
// Plain module state with subscribers (like `state/commonOptions.ts` and `state/commonTool.ts`), so an action registered in the
// action runner can open a dialog without a place in the big reducer. `components/CommonDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { GridEditor } from "../kicad-port/gridSettings";

/** `DIALOG_GROUP_PROPERTIES`: the group being edited, its name and member list as the dialog has them so far. */
export interface GroupDialog {
  id: string;
  name: string;
  members: string[];
  /** `m_propertiesDialog->Show( false )` while the user picks a new member on the canvas (`PickNewMember`). */
  hidden: boolean;
}

export interface CommonDialogsState {
  group: GroupDialog | null;
  about: boolean;
  /** `DIALOG_PAGES_SETTINGS`: which document's page is being edited, or null when the dialog is closed. */
  page: "pcb" | "schematic" | null;
  /** `COMMON_TOOLS::GridOrigin`'s X / Y entry dialog is open. */
  gridOrigin: boolean;
  /** `PANEL_GRID_SETTINGS` ("Edit Grids..."): the editor whose Grids page of the Preferences is showing, or null when the dialog is closed. */
  grids: GridEditor | null;
}

const CLOSED: CommonDialogsState = { group: null, about: false, page: null, gridOrigin: false, grids: null };

let current: CommonDialogsState = CLOSED;
const listeners = new Set<() => void>();

function set(next: CommonDialogsState): void {
  current = next;
  for (const l of listeners) l();
}

export function getCommonDialogs(): CommonDialogsState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useCommonDialogs(): CommonDialogsState {
  return useSyncExternalStore(subscribe, getCommonDialogs, getCommonDialogs);
}

export function openGroupDialog(group: Omit<GroupDialog, "hidden">): void {
  set({ ...current, group: { ...group, hidden: false } });
}

export function updateGroupDialog(patch: Partial<GroupDialog>): void {
  if (current.group) set({ ...current, group: { ...current.group, ...patch } });
}

export function closeGroupDialog(): void {
  set({ ...current, group: null });
}

export function setAboutOpen(open: boolean): void {
  set({ ...current, about: open });
}

export function setPageSettingsOpen(target: "pcb" | "schematic" | null): void {
  set({ ...current, page: target });
}

export function setGridOriginDialogOpen(open: boolean): void {
  set({ ...current, gridOrigin: open });
}

export function setGridsDialogOpen(editor: GridEditor | null): void {
  set({ ...current, grids: editor });
}
