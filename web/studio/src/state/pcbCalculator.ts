// The Calculator Tools window (`PCB_CALCULATOR_FRAME`, components/calculator/CalculatorWindow.tsx): whether it is open, and its settings
// (`PCB_CALCULATOR_SETTINGS`, kicad-port/calc/calcSettings.ts). KiCad keeps the settings in `pcb_calculator.json` in the user's settings folder; the
// studio keeps the same object in this browser's local storage (the person's, not the project's, as state/viewer3dUserSettings.ts keeps the 3D
// viewer's). Plain module state with subscribers, so the action that opens the window needs no place in the big reducer.
import { useSyncExternalStore } from "react";
import { defaultPcbCalculatorSettings, sanitizePcbCalculatorSettings, type PcbCalculatorSettings } from "../kicad-port/calc/calcSettings";

const STORAGE_KEY = "eda-studio.pcb_calculator";

export interface PcbCalculatorUi {
  open: boolean;
  /** Bumped by every `openPcbCalculator( page )` that asks for a page (KIWAY mail `MAIL_CALC_SHOW`), so the window selects it even when it is open. */
  showRequest: number;
  settings: PcbCalculatorSettings;
}

function load(): PcbCalculatorSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? sanitizePcbCalculatorSettings(JSON.parse(raw)) : defaultPcbCalculatorSettings();
  } catch {
    // no storage (private window, blocked site data) or a broken entry: the defaults
    return defaultPcbCalculatorSettings();
  }
}

let current: PcbCalculatorUi = { open: false, showRequest: 0, settings: load() };
const listeners = new Set<() => void>();

function set(next: PcbCalculatorUi): void {
  current = next;
  for (const l of [...listeners]) l();
}

function save(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(current.settings));
  } catch {
    // ignore: the settings stay for this session
  }
}

export function getPcbCalculator(): PcbCalculatorUi {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePcbCalculator(): PcbCalculatorUi {
  return useSyncExternalStore(subscribe, getPcbCalculator, getPcbCalculator);
}

export function usePcbCalculatorSettings(): PcbCalculatorSettings {
  return usePcbCalculator().settings;
}

/**
 * `COMMON_CONTROL::Execute( showCalculatorTools )` starts pcb_calculator (or raises it); `showCalculatorPage` (dialog_pad_properties.cpp) also mails it
 * the page to show. `page` is the tree page's index (`last_page`'s numbering).
 */
export function openPcbCalculator(page?: number): void {
  if (page !== undefined) {
    set({ open: true, showRequest: current.showRequest + 1, settings: { ...current.settings, last_page: page } });
    save();
  } else if (!current.open) set({ ...current, open: true });
}

/** `OnClosePcbCalc`'s question to the regulator panel (unsaved list changes): false keeps the window open. Set by the panel while it is mounted. */
let closeGuard: (() => boolean) | null = null;

export function setPcbCalculatorCloseGuard(guard: (() => boolean) | null): void {
  closeGuard = guard;
}

/** File > Close and the title bar's close button (`OnClosePcbCalc`): the regulator panel may keep the window open. */
export function closePcbCalculator(): void {
  if (closeGuard && !closeGuard()) return;
  set({ ...current, open: false });
}

/**
 * A panel's `SaveSettings`: the settings changed by `fn` (which returns the new object, or mutates the copy it is given and returns nothing), stored
 * at once. KiCad stores them when the frame closes; storing each change keeps them when the browser tab is closed instead.
 */
export function updatePcbCalculatorSettings(fn: (s: PcbCalculatorSettings) => PcbCalculatorSettings | void): void {
  const copy = structuredClone(current.settings);
  const next = fn(copy) ?? copy;
  set({ ...current, settings: next });
  save();
}

/** Every setting back to KiCad's defaults (a fresh `pcb_calculator.json`); for the browser checks. */
export function resetPcbCalculatorSettings(): void {
  set({ ...current, settings: defaultPcbCalculatorSettings() });
  save();
}
