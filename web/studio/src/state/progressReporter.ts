// The progress window a long job shows (`WX_PROGRESS_REPORTER`, KiCad 8303b2ad common/widgets/wx_progress_reporters.cpp: a `wxProgressDialog` with
// `wxPD_AUTO_HIDE | wxPD_APP_MODAL | wxPD_ELAPSED_TIME`, and `wxPD_CAN_ABORT` with `PR_CAN_ABORT`): a title, the job's message, a gauge of 1000 steps
// (`SetRange( 1000 )`), the elapsed time and, when the job can be aborted, a Cancel button whose press makes `KeepRefreshing()` false. Plain module state;
// components/ProgressReporterDialog.tsx draws it. Used by Place > Auto-Place Footprints (`AUTOPLACE_TOOL::autoplace`, UI-LEFT-2 PP20).
import { useSyncExternalStore } from "react";

export interface ProgressReport {
  title: string;
  message: string;
  /** 0 .. 1000 (`CurrentProgress()`). */
  progress: number;
  /** `Date.now()` when the window opened, for `wxPD_ELAPSED_TIME`. */
  startedAt: number;
  /** `PR_CAN_ABORT`: what the Cancel button does; absent, the window has none. */
  onAbort?: () => void;
  /** The Cancel button was pressed (`KeepRefreshing()` returns false from then on). */
  aborting?: boolean;
}

let current: ProgressReport | null = null;
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());

export function getProgressReport(): ProgressReport | null {
  return current;
}

/** Opens the window (or replaces what it shows). */
export function openProgressReport(report: ProgressReport): void {
  current = report;
  notify();
}

/** `Report()` / `SetCurrentProgress()`: what the open window shows now. */
export function updateProgressReport(change: Partial<Omit<ProgressReport, "startedAt">>): void {
  if (!current) return;
  current = { ...current, ...change };
  notify();
}

/** The job is over: the window goes (`wxPD_AUTO_HIDE`, the reporter's destructor). */
export function closeProgressReport(): void {
  if (!current) return;
  current = null;
  notify();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useProgressReport(): ProgressReport | null {
  return useSyncExternalStore(subscribe, () => current);
}

/** `wxPD_ELAPSED_TIME`'s "Elapsed time:" value, `H:MM:SS` as `wxGenericProgressDialog::SetTimeLabel` writes it. */
export function elapsedLabel(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const two = (n: number) => String(n).padStart(2, "0");
  return `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}`;
}
