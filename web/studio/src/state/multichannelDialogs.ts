// Which of the multichannel tool's dialogs is open (GAPS.md items 24 and 32, package P24): Generate Placement Rule Areas and Repeat Layout. Plain module state with subscribers
// (like `state/pcbItemDialogs.ts`), so an action of the action runner can open a dialog without a place in the big reducer. `components/MultichannelDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { RepeatPlan } from "../api/multichannelClient";
import { DEFAULT_REPEAT_OPTIONS, type GenerateRow, type RepeatOptions } from "../kicad-port/multichannel";

/** `DIALOG_MULTICHANNEL_GENERATE_RULE_AREAS`: the rows `GeneratePotentialRuleAreas` found, none ticked. */
export interface GenerateDialog {
  rows: GenerateRow[];
}

/** `DIALOG_MULTICHANNEL_REPEAT_LAYOUT`: the reference area and what it matched against. */
export interface RepeatDialog {
  plan: RepeatPlan;
}

export interface MultichannelDialogsState {
  generate: GenerateDialog | null;
  repeat: RepeatDialog | null;
}

let current: MultichannelDialogsState = { generate: null, repeat: null };
const listeners = new Set<() => void>();

function set(next: MultichannelDialogsState): void {
  current = next;
  for (const l of listeners) l();
}

export function getMultichannelDialogs(): MultichannelDialogsState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useMultichannelDialogs(): MultichannelDialogsState {
  return useSyncExternalStore(subscribe, getMultichannelDialogs, getMultichannelDialogs);
}

export function openGenerateDialog(dialog: GenerateDialog): void {
  set({ ...current, generate: dialog });
}

export function closeGenerateDialog(): void {
  set({ ...current, generate: null });
}

export function openRepeatDialog(dialog: RepeatDialog): void {
  set({ ...current, repeat: dialog });
}

export function closeRepeatDialog(): void {
  set({ ...current, repeat: null });
}

// The tool keeps its options between uses (`m_areas.m_options`), within a session: the checkboxes of the Generate dialog (the Replace box and the Group box) and the Repeat Layout's.
let repeatOptions: RepeatOptions = { ...DEFAULT_REPEAT_OPTIONS };
let generateFlags = { replace: false, group: false };

export const lastRepeatOptions = (): RepeatOptions => ({ ...repeatOptions });
export const rememberRepeatOptions = (o: RepeatOptions): void => {
  repeatOptions = { ...o, anchor: null };
};
export const lastGenerateFlags = (): { replace: boolean; group: boolean } => ({ ...generateFlags });
export const rememberGenerateFlags = (f: { replace: boolean; group: boolean }): void => {
  generateFlags = { ...f };
};
