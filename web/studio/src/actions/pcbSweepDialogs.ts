// The dialogs the pcbnew edit-tool actions (actions/pcbEditSweep.ts) open.
// Each action asks for its parameters and then performs the edit when the
// dialog is accepted, so the dialog carries its own `onOk` closure; that is
// why this is a tiny module-level store (useSyncExternalStore) rather than
// reducer state -- the shared store file stays untouched. One dialog at a
// time, like KiCad's modal dialogs. The components are in
// components/PcbSweepDialogs.tsx.

import { useSyncExternalStore, type ReactNode } from "react";
import type { FilterOptions } from "../kicad-port/pcbSelectionOps";

/** `WX_UNIT_ENTRY_DIALOG`: one length (Fillet Lines' radius, Chamfer's setback, a tolerance...). `onOk` gets micrometres. */
export interface UnitEntryDialog {
  kind: "unit_entry";
  title: string;
  label: string;
  valueUm: number;
  /** The value may be zero (KiCad's own dialogs return "cancelled" for zero; this one just refuses it unless set). */
  allowZero?: boolean;
  onOk: (valueUm: number) => void;
}

/** `GetDogboneParams`' `WX_MULTI_ENTRY_DIALOG`: arc radius and "Add slots in acute corners". */
export interface DogboneDialog {
  kind: "dogbone";
  radiusUm: number;
  addSlots: boolean;
  onOk: (v: { radiusUm: number; addSlots: boolean }) => void;
}

/** `DIALOG_FILTER_SELECTION`. */
export interface FilterSelectionDialog {
  kind: "filter_selection";
  options: FilterOptions;
  onOk: (options: FilterOptions) => void;
}

/** Any other dialog: the action brings its own component (components/Pcb*Dialogs.tsx), which closes itself with `closeSweepDialog`. */
export interface ElementDialog {
  kind: "element";
  element: ReactNode;
}

export type SweepDialog = UnitEntryDialog | DogboneDialog | FilterSelectionDialog | ElementDialog;

let current: SweepDialog | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const l of listeners) l();
}

export function openSweepDialog(d: SweepDialog): void {
  current = d;
  emit();
}

export function closeSweepDialog(): void {
  if (current === null) return;
  current = null;
  emit();
}

export function useSweepDialog(): SweepDialog | null {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => current
  );
}
