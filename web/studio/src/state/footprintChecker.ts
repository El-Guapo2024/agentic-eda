// The Footprint Editor's Footprint Checker (`FOOTPRINT_EDITOR_CONTROL::m_checkerDialog`, `DIALOG_FOOTPRINT_CHECKER`): the markers a run left, which one is selected, the "Show" boxes
// and whether the dialog is up. Like KiCad's the dialog is created when asked for and destroyed by Close, but the markers belong to the footprint on the editor's board, not to the
// dialog: they stay on the canvas after Close until "Delete All Markers" (or until another footprint is loaded, which clears the board), and the dialog comes back showing them.
// Plain module state with subscribers, as state/pcbFind.ts. What a run is -- kicad-cli's DRC on a scratch board that holds the footprint -- is crates/kicad-engine/src/footprint_check.rs;
// what the buttons do is actions/footprintCheckerActions.ts; the dialog is components/footprint/FootprintCheckerDialog.tsx and the markers are drawn by footprintPainter.ts.
import { useSyncExternalStore } from "react";
import type { FpCheckMarker } from "../api/types";
import { SHOW_ALL, type FpShow } from "../kicad-port/footprintChecker";

export interface FpCheckerState {
  open: boolean;
  /** Bumped each time the dialog is asked for, so it comes forward again. */
  shown: number;
  /** The footprint the markers are of (`g_lastFootprint`): another footprint opened clears them. */
  name: string | null;
  running: boolean;
  /** `m_checksRun`: the badges say "-" until a run (and again after Delete All Markers). */
  checksRun: boolean;
  engine: string | null;
  markers: FpCheckMarker[];
  /** The marker row selected (an index into `markers`), which the canvas draws highlighted. */
  selected: number | null;
  show: FpShow;
  /** A run that could not be made (kicad-cli missing, the footprint unknown): the message the dialog shows. */
  error: string | null;
}

const initial = (): FpCheckerState => ({ open: false, shown: 0, name: null, running: false, checksRun: false, engine: null, markers: [], selected: null, show: SHOW_ALL, error: null });

let state: FpCheckerState = initial();
const listeners = new Set<() => void>();

export function getFootprintChecker(): FpCheckerState {
  return state;
}

export function setFootprintChecker(patch: Partial<FpCheckerState>): void {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

export function resetFootprintChecker(): void {
  state = initial();
  for (const l of listeners) l();
}

const subscribe = (l: () => void): (() => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export function useFootprintChecker(): FpCheckerState {
  return useSyncExternalStore(subscribe, () => state);
}
