// Which of the board items' dialogs is open (GAPS.md item 24, package P16): Text Box Properties, Reference Image Properties and Barcode Properties.
// Plain module state with subscribers (like `state/commonDialogs.ts`), so an action in the action runner can open a dialog without a place in the big reducer.
// `components/PcbItemDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { BoardTextBox, CmdBarcode } from "../api/pcbItemTypes";

/** `DIALOG_TEXTBOX_PROPERTIES`: the box it edits (`id` null for one the Draw Text Box tool just drew and the board does not have yet) and what to do when it closes. */
export interface TextBoxDialog {
  id: string | null;
  box: BoardTextBox;
  /** After OK or Cancel: the Draw Text Box tool starts its next box (`DrawRectangle`'s loop). */
  onClose?: () => void;
}

/** `DIALOG_REFERENCE_IMAGE_PROPERTIES`: the image it edits (always one the board has). */
export interface RefImageDialog {
  id: string;
}

/** `DIALOG_BARCODE_PROPERTIES`: the barcode it edits (`id` null for one the Add Barcode tool just made and the board does not have yet) as the verbs take it, and what to do when it closes. */
export interface BarcodeDialog {
  id: string | null;
  barcode: CmdBarcode;
  /** After OK or Cancel: the Add Barcode tool waits for its next click (`DrawBarcode`'s loop). */
  onClose?: () => void;
}

export interface PcbItemDialogsState {
  textBox: TextBoxDialog | null;
  refImage: RefImageDialog | null;
  barcode: BarcodeDialog | null;
}

const CLOSED: PcbItemDialogsState = { textBox: null, refImage: null, barcode: null };

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

export function openRefImageDialog(dialog: RefImageDialog): void {
  set({ ...current, refImage: dialog });
}

export function closeRefImageDialog(): void {
  set({ ...current, refImage: null });
}

export function openBarcodeDialog(dialog: BarcodeDialog): void {
  set({ ...current, barcode: dialog });
}

export function closeBarcodeDialog(): void {
  const done = current.barcode?.onClose;
  set({ ...current, barcode: null });
  done?.();
}
