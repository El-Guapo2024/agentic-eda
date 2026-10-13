// The two report dialogs of the Inspect menu (`PCB_BASE_FRAME::GetInspectClearanceDialog` / `GetInspectConstraintsDialog`, both a `DIALOG_BOOK_REPORTER`): like KiCad's each
// is created once and only hidden, modeless, and a new report replaces the pages of the last (`DeleteAllPages`). Plain module state with subscribers, as state/pcbFind.ts. What the
// pages say is made by the server (crates/drc/src/inspect.rs); what the commands do is actions/inspectActions.ts; the dialog is components/InspectReportDialog.tsx.
import { useSyncExternalStore } from "react";
import type { InspectReply } from "../api/types";

export type InspectKind = "clearance" | "constraints";

export interface InspectReportState {
  open: boolean;
  /** Bumped each time a report is shown, so the dialog comes forward again. */
  shown: number;
  reply: InspectReply | null;
  /** The notebook page in front. */
  page: number;
  /** The entry of the "Layer:" choice of each page that has one (the first layer to begin with). */
  layerChoice: number[];
}

const initial = (): InspectReportState => ({ open: false, shown: 0, reply: null, page: 0, layerChoice: [] });

let states: Record<InspectKind, InspectReportState> = { clearance: initial(), constraints: initial() };
const listeners = new Set<() => void>();

export function getInspectReport(kind: InspectKind): InspectReportState {
  return states[kind];
}

export function setInspectReport(kind: InspectKind, patch: Partial<InspectReportState>): void {
  states = { ...states, [kind]: { ...states[kind], ...patch } };
  for (const l of listeners) l();
}

/** `dialog->DeleteAllPages()`, the new pages, `Raise()` and `Show( true )`. */
export function showInspectReport(kind: InspectKind, reply: InspectReply): void {
  setInspectReport(kind, { open: true, shown: states[kind].shown + 1, reply, page: 0, layerChoice: reply.pages.map(() => 0) });
}

export function resetInspectReports(): void {
  states = { clearance: initial(), constraints: initial() };
  for (const l of listeners) l();
}

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export function useInspectReport(kind: InspectKind): InspectReportState {
  return useSyncExternalStore(subscribe, () => states[kind]);
}
