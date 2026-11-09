// The library editors' file watch (UI-LEFT-2 LL3): while an editor shows an item of a library of the tables, its library file is asked about every second
// (`GET /api/library/watch`, crates/cli/src/library_watch.rs) and kicad-port/libraryWatch.ts decides: an item that is not modified is reloaded at once when
// the file changed, a modified one asks KiCad's question first ("The library containing the current symbol has changed.\nDo you want to reload the library?",
// Yes / No), and an open dialog puts it off. KiCad: `SCH_BASE_FRAME::setSymWatcher` / `OnSymChangeDebounceTimer`, `PCB_BASE_FRAME::setFPWatcher` /
// `OnFpChangeDebounceTimer`. The reload is `actions/librarySaveOps.ts` `reloadFromLibrary`, one undoable step.
import { useEffect, useRef } from "react";
import { fetchWatch } from "../api/librarySaveClient";
import { reloadFromLibrary } from "../actions/librarySaveOps";
import { askConfirm } from "../components/library/libraryDialogs";
import { FOOTPRINT_LIBRARY_CHANGED, SYMBOL_LIBRARY_CHANGED, WATCH_IDLE, watchStep, type WatchState } from "../kicad-port/libraryWatch";

/** How often the watched file's time is asked (KiCad's debounce timer: one second). */
export const WATCH_PERIOD_MS = 1000;

/** `!IsEnabled()`: a dialog is open on the editor (a modal backdrop, or a dialog of the page). */
function dialogOpen(): boolean {
  return typeof document !== "undefined" && document.querySelector(".dialog-backdrop, [role='dialog'], [role='alertdialog']") !== null;
}

/**
 * Watch the library of `id` (`Lib:Name`, the item the editor shows; null: nothing) while `active` (the editor's tab is shown). `shown` runs after a reload: with
 * `gone` when the library no longer has the item (the canvas is emptied).
 */
export function useLibraryWatch(kind: "symbol" | "footprint", id: string | null, active: boolean, shown: (gone: boolean) => Promise<void>, toast: (message: string, kind?: "info" | "error") => void): void {
  const state = useRef<WatchState>(WATCH_IDLE);
  const busy = useRef(false);
  const shownRef = useRef(shown);
  shownRef.current = shown;
  const toastRef = useRef(toast);
  toastRef.current = toast;
  useEffect(() => {
    if (!active || !id) {
      state.current = WATCH_IDLE;
      return;
    }
    let stopped = false;
    const tick = async () => {
      if (busy.current || stopped) return;
      busy.current = true;
      try {
        const answer = await fetchWatch(kind, id).catch(() => null);
        if (stopped || answer === null) return;
        const { state: next, action } = watchStep(state.current, id, answer, dialogOpen());
        state.current = next;
        if (action === "none") return;
        if (action === "ask" && !(await askConfirm({ title: "Confirmation", message: kind === "symbol" ? SYMBOL_LIBRARY_CHANGED : FOOTPRINT_LIBRARY_CHANGED, okLabel: "Yes", cancelLabel: "No" }))) return;
        await reloadFromLibrary(kind, id, (gone) => shownRef.current(gone), (m, k) => toastRef.current(m, k));
      } finally {
        busy.current = false;
      }
    };
    void tick();
    const timer = setInterval(() => void tick(), WATCH_PERIOD_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [kind, id, active]);
}
