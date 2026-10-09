// The board editor's Find dialog and the list it walks (`PCB_EDIT_FRAME::m_findDialog`: `DIALOG_FIND`'s search text and checkboxes, `m_hitList`, `m_it`, `m_upToDate`, and
// the frame's find history). The dialog is created once and only hidden, so Find Next and Find Previous (F3, Shift+F3) go on through the same list after it is closed:
// this is that object, as plain module state with subscribers, the way state/gridSettings.ts keeps the grids. The rules for it are kicad-port/pcbFind.ts, what a press
// does is actions/pcbFindActions.ts.
import { useSyncExternalStore } from "react";
import { defaultFindOptions, type PcbFindOptions, type PcbHit } from "../kicad-port/pcbFind";

export interface PcbFindState {
  /** The dialog is on screen. */
  open: boolean;
  options: PcbFindOptions;
  /** `m_hitList`, and `m_it`: the position in it of the hit last shown (the list's length is "end"). */
  hits: PcbHit[];
  cursor: number;
  /** What `hits` was built for (the options and the revision of the board and of the DRC run): the list is rebuilt when it no longer matches, as `m_upToDate` is cleared. */
  builtFor: string | null;
  /** `m_status`: "Hit(s): 2 / 5", "No hits", "'text' not found". */
  status: string;
  /** `GetFindHistoryList()`: the last ten texts searched for, the newest first. */
  history: string[];
  /** Bumped each time the dialog is asked to show, so it takes the focus again and selects the text. */
  shown: number;
  /** `m_findDialog` exists: it was shown at least once (`PCB_EDIT_FRAME::FindNext` shows it first when it was not). */
  created: boolean;
}

const initial = (): PcbFindState => ({ open: false, options: defaultFindOptions(), hits: [], cursor: 0, builtFor: null, status: "", history: [], shown: 0, created: false });

let current: PcbFindState = initial();
const listeners = new Set<() => void>();

export function getPcbFind(): PcbFindState {
  return current;
}

export function setPcbFind(patch: Partial<PcbFindState>): void {
  current = { ...current, ...patch };
  for (const l of listeners) l();
}

/** An option changed (`onOptionChanged`: `m_upToDate = false`): the list is built again at the next press. */
export function setFindOptions(patch: Partial<PcbFindOptions>): void {
  setPcbFind({ options: { ...current.options, ...patch }, builtFor: null, status: "" });
}

/** Back to a fresh dialog (tests, and a new design). */
export function resetPcbFind(): void {
  current = initial();
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePcbFind(): PcbFindState {
  return useSyncExternalStore(subscribe, () => current, () => current);
}

/** `m_frame->GetFindHistoryList().Insert( searchString, 0 )`: the text goes to the top of the list, once, and the list keeps ten. */
export function withHistory(history: readonly string[], text: string): string[] {
  return [text, ...history.filter((h) => h !== text)].slice(0, 10);
}
