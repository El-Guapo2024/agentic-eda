// Which of the board items' dialogs is open (GAPS.md item 24, package P16): Text Box Properties today; Barcode Properties and Reference Image Properties as their steps land.
// Plain module state with subscribers (like `state/commonDialogs.ts`), so an action in the action runner can open a dialog without a place in the big reducer.
// `components/PcbItemDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { BoardTextBox } from "../api/pcbItemTypes";

/** `DIALOG_TEXTBOX_PROPERTIES`: the box it edits (`id` null for one the Draw Text Box tool just drew and the board does not have yet) and what to do when it closes. */
export interface TextBoxDialog {
  id: string | null;
  box: BoardTextBox;
  /** After OK or Cancel: the Draw Text Box tool starts its next box (`DrawRectangle`'s loop). */
  onClose?: () => void;
}

export interface PcbItemDialogsState {
  textBox: TextBoxDialog | null;
}

const CLOSED: PcbItemDialogsState = { textBox: null };

let current: PcbItemDialogsState = CLOSED;
const listeners = new Set<() => void>();

function set(next: PcbItemDialogsState): void {
  current = next;
  for (const l of listeners) l();
}

export function getPcbItemDialogs(): PcbItemDialogsState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePcbItemDialogs(): PcbItemDialogsState {
  return useSyncExternalStore(subscribe, getPcbItemDialogs, getPcbItemDialogs);
}

export function openTextBoxDialog(dialog: TextBoxDialog): void {
  set({ ...current, textBox: dialog });
}

export function closeTextBoxDialog(): void {
  const done = current.textBox?.onClose;
  set({ ...current, textBox: null });
  done?.();
}
