// The two dialogs of File > Import > Non-KiCad Board / Schematic: "Edit Mapping of Imported Layers" (DIALOG_MAP_LAYERS) and the report of the import's errors and warnings
// (the DIALOG_HTML_REPORTER KiCad shows when "Show import issues" is on). Plain module state with subscribers, like `state/importDialogs.ts`, so an action registered in the
// action runner can open one and wait for the answer without a place in the big reducer; `components/ForeignImportDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { ImportFinding, SourceLayer } from "../kicad-port/foreignImport";

/** What the layer dialog answers: the layers moved off the importer's guess (kicad-cli's `--layer-map`) and the "Keep KiCad layer names" box. */
export interface LayerMapAnswer {
  map: Record<string, string>;
  keepNames: boolean;
}

export interface LayerMapDialog {
  name: string;
  format: string;
  layers: SourceLayer[];
  keepNames: boolean;
  /** Resolves the promise `askLayerMapping` returned: `null` is Cancel, which here abandons the import. */
  resolve: (answer: LayerMapAnswer | null) => void;
}

export interface ImportReport {
  name: string;
  format: string;
  /** "Imported x.brd (Eagle): 19 footprints, ..." */
  summary: string;
  findings: ImportFinding[];
  notes: string[];
}

interface Dialogs {
  layers: LayerMapDialog | null;
  report: ImportReport | null;
}

let current: Dialogs = { layers: null, report: null };
const listeners = new Set<() => void>();

function set(next: Dialogs): void {
  current = next;
  for (const l of listeners) l();
}

export function getForeignImportDialogs(): Dialogs {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useForeignImportDialogs(): Dialogs {
  return useSyncExternalStore(subscribe, getForeignImportDialogs, getForeignImportDialogs);
}

/** `RunModal`: opens the layer dialog and waits for OK (the answer) or Cancel (`null`). One dialog at a time: a second ask cancels the first. */
export function askLayerMapping(args: Omit<LayerMapDialog, "resolve">): Promise<LayerMapAnswer | null> {
  current.layers?.resolve(null);
  return new Promise((resolve) => {
    set({ ...current, layers: { ...args, resolve } });
  });
}

/** Called by the dialog when it closes; also what ends the wait. */
export function answerLayerMapping(answer: LayerMapAnswer | null): void {
  const open = current.layers;
  set({ ...current, layers: null });
  open?.resolve(answer);
}

export function showImportReport(report: ImportReport): void {
  set({ ...current, report });
}

export function closeImportReport(): void {
  set({ ...current, report: null });
}

// ---------------------------------------------------------------------------------------------------------------- the dialogs' own settings

/** `PCBNEW_SETTINGS::m_ImportKeepKiCadLayerNames` and `m_System.show_import_issues`, kept in the browser (a per-viewer convenience: it is only ever a default for a dialog). */
const KEEP_NAMES_KEY = "eda.studio.importKeepKiCadLayerNames";
const SHOW_ISSUES_KEY = "eda.studio.importShowIssues";

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const v = window.localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

function writeFlag(key: string, value: boolean): void {
  try {
    window.localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // private window, storage blocked: the box just starts at its default next time
  }
}

export const importKeepKiCadLayerNames = (): boolean => readFlag(KEEP_NAMES_KEY, false);
export const setImportKeepKiCadLayerNames = (v: boolean): void => writeFlag(KEEP_NAMES_KEY, v);
/** "Show import issues" (the file dialog's check box in KiCad; its default is on). */
export const importShowIssues = (): boolean => readFlag(SHOW_ISSUES_KEY, true);
export const setImportShowIssues = (v: boolean): void => writeFlag(SHOW_ISSUES_KEY, v);
