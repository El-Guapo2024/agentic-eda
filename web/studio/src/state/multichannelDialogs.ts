// Which of the multichannel tool's dialogs is open (GAPS.md items 24 and 32, package P24): Generate Placement Rule Areas and Repeat Layout. Plain module state with subscribers
// (like `state/pcbItemDialogs.ts`), so an action of the action runner can open a dialog without a place in the big reducer. `components/MultichannelDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { RepeatPlan } from "../api/multichannelClient";
import { afterGenerate, afterRepeat, DEFAULT_TOOL_MEMORY, generateFlagsOf, parseToolMemory, repeatOptionsOf, type GenerateRow, type McToolMemory, type RepeatOptions } from "../kicad-port/multichannel";

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

// What the tool keeps between its dialogs (`MULTICHANNEL_TOOL::m_areas`: `m_replaceExisting` and `m_options`, the Group box shared by both dialogs; kicad-port/multichannel.ts
// `McToolMemory`). KiCad keeps it in the tool, as long as the board editor is open, and in no settings file or project. The studio's board editor lives as long as the browser
// tab, so it is kept in the tab's sessionStorage: a reload keeps it, a new tab starts from KiCad's defaults. Without storage it lasts while the page does.
const STORAGE_KEY = "eda-studio.multichannel-tool.v1";

function session(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.sessionStorage : null;
  } catch {
    return null;
  }
}

function load(): McToolMemory {
  try {
    const raw = session()?.getItem(STORAGE_KEY);
    return raw ? parseToolMemory(JSON.parse(raw)) : { ...DEFAULT_TOOL_MEMORY, options: { ...DEFAULT_TOOL_MEMORY.options } };
  } catch {
    return { ...DEFAULT_TOOL_MEMORY, options: { ...DEFAULT_TOOL_MEMORY.options } };
  }
}

let memory: McToolMemory | null = null;

function currentMemory(): McToolMemory {
  if (!memory) memory = load();
  return memory;
}

function keep(next: McToolMemory): void {
  memory = next;
  try {
    session()?.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    /* no storage: the page's copy still works */
  }
}

export const lastRepeatOptions = (): RepeatOptions => repeatOptionsOf(currentMemory());
export const rememberRepeatOptions = (o: RepeatOptions): void => keep(afterRepeat(currentMemory(), o));
export const lastGenerateFlags = (): { replace: boolean; group: boolean } => generateFlagsOf(currentMemory());
export const rememberGenerateFlags = (f: { replace: boolean; group: boolean }, tab: GenerateRow["kind"]): void => keep(afterGenerate(currentMemory(), f, tab));
