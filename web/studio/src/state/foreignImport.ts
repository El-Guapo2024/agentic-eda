// The two dialogs of File > Import > Non-KiCad Board / Schematic: "Edit Mapping of Imported Layers" (DIALOG_MAP_LAYERS) and the report of the import's errors and warnings
// (the DIALOG_HTML_REPORTER KiCad shows when "Show import issues" is on). Plain module state with subscribers, like `state/importDialogs.ts`, so an action registered in the
// action runner can open one and wait for the answer without a place in the big reducer; `components/ForeignImportDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { ImportFinding, ImportProjectDesc, SourceLayer } from "../kicad-port/foreignImport";

/** What the layer dialog answers: the layers moved off the importer's guess (kicad-cli's `--layer-map`), the layers left unmatched to leave out, and the "Keep KiCad layer names" box. */
export interface LayerMapAnswer {
  map: Record<string, string>;
  /** `DIALOG_MAP_LAYERS`: a layer left in the Unmatched list is `UNDEFINED_LAYER`, and what is on it is left out. */
  drop: string[];
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

/** A question before an import (`HandleUnsavedChanges`' Save / Discard Changes / Cancel, `IsOK`'s Yes / No): the button pressed, or `null` for the way out. */
export interface ImportQuestion {
  title: string;
  message: string;
  detail?: string;
  /** The buttons, in order; the last is the way out (Cancel, No). */
  buttons: string[];
  resolve: (button: string | null) => void;
}

/** `DIALOG_IMPORT_CHOOSE_PROJECT`: the rows, and the index of the one chosen (`null`: closed, which imports nothing). */
export interface ChooseProjectDialog {
  rows: ImportProjectDesc[];
  resolve: (row: number | null) => void;
}

interface Dialogs {
  layers: LayerMapDialog | null;
  report: ImportReport | null;
  question: ImportQuestion | null;
  choose: ChooseProjectDialog | null;
}

let current: Dialogs = { layers: null, report: null, question: null, choose: null };
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

/** Ask a question before an import and wait for the button pressed (`null` when the dialog is closed). One at a time: a second ask answers the first with its way out. */
export function askImportQuestion(args: Omit<ImportQuestion, "resolve">): Promise<string | null> {
  current.question?.resolve(null);
  return new Promise((resolve) => set({ ...current, question: { ...args, resolve } }));
}

export function answerImportQuestion(button: string | null): void {
  const open = current.question;
  set({ ...current, question: null });
  open?.resolve(button);
}

/** `DIALOG_IMPORT_CHOOSE_PROJECT::RunModal`: the rows shown, the first selected; resolves with the row chosen or `null`. */
export function askChooseProject(rows: ImportProjectDesc[]): Promise<number | null> {
  current.choose?.resolve(null);
  return new Promise((resolve) => set({ ...current, choose: { rows, resolve } }));
}

export function answerChooseProject(row: number | null): void {
  const open = current.choose;
  set({ ...current, choose: null });
  open?.resolve(row);
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
