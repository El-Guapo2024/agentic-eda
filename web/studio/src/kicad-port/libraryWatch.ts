// The library editors' watch on the library of the item they show (UI-LEFT-2 LL3): KiCad's `SCH_BASE_FRAME::setSymWatcher` / `OnSymChange` /
// `OnSymChangeDebounceTimer` (eeschema/sch_base_frame.cpp) and `PCB_BASE_FRAME::setFPWatcher` / `OnFPChange` / `OnFpChangeDebounceTimer`
// (pcbnew/pcb_base_frame.cpp), commit 8303b2ad. Pure: the server says what the watched file's time is now (`GET /api/library/watch`,
// crates/cli/src/library_watch.rs) and this decides, tick by tick, whether the editor reloads, asks, or waits; state/libraryWatch.ts runs it.
//
// KiCad keeps the time the file had when the watch was set (`m_watcherTimestamp`, `m_watcherLastModified`); an event starts a one-second debounce timer
// (restarted by every later event), and when it fires a changed, valid time is taken as the new one and the editor reloads -- at once when its content is
// not modified, else after `IsOK( this, ... )` -- while a dialog open on the frame puts the reload off (`!IsEnabled()`: "restarting debounce timer"). Here the
// page asks every second: a new time is acted on when the next answer still has it (the debounce), and a time the studio wrote itself (`own`: Save, Save As)
// is taken as the new one without a word, as KiCad's own save resets its watch.

/** What the server answers for the watched item (crates/cli/src/library_watch.rs `watch`). */
export interface WatchAnswer {
  /** The watched file or folder; null: the item has no library file (the project library, a footprint from the board). */
  path: string | null;
  /** Its modification time; null when there is no file (KiCad: `!lastModified.IsValid()`: nothing is reloaded). */
  stamp?: string | null;
  /** The studio wrote it last. */
  own?: boolean;
  /** The time did not move while the answer was made: `modified` was read against the file `stamp` stands for. */
  steady?: boolean;
  /** The item is modified (its flag, or its copy differs from the file). */
  modified?: boolean;
  /** The item's modified flag alone (`IsContentModified()`). */
  dirty?: boolean;
}

export interface WatchState {
  /** The item the watch was set for (`setSymWatcher( &libId )`, `setFPWatcher( aFootprint )`). */
  id: string | null;
  path: string | null;
  /** The time kept (`m_watcherTimestamp`); undefined before the first answer. */
  stamp: string | null | undefined;
  /** A new time seen once, waiting for the next answer (the debounce). */
  pending: string | null;
  /** Whether the item was modified while the file still had the kept time. */
  modifiedBefore: boolean;
}

export const WATCH_IDLE: WatchState = { id: null, path: null, stamp: undefined, pending: null, modifiedBefore: false };

export type WatchAction = "none" | "reload" | "ask";

/** `OnSymChangeDebounceTimer`'s question, word for word (`IsOK`: Confirmation, Yes / No). */
export const SYMBOL_LIBRARY_CHANGED = "The library containing the current symbol has changed.\nDo you want to reload the library?";
/** `OnFpChangeDebounceTimer`'s question. */
export const FOOTPRINT_LIBRARY_CHANGED = "The library containing the current footprint has changed.\nDo you want to reload the footprint?";

/**
 * One answer of the server for the item `id`: the watch's next state, and what the editor does now. `dialogOpen`: a dialog is open on the editor, so a
 * reload waits (`!IsEnabled()`).
 */
export function watchStep(st: WatchState, id: string | null, answer: WatchAnswer | null, dialogOpen: boolean): { state: WatchState; action: WatchAction } {
  const none = (state: WatchState) => ({ state, action: "none" as const });
  if (!id || !answer || answer.path === null) return none({ ...WATCH_IDLE, id });
  const modified = answer.modified === true;
  // a new item, or the same id in another library file (the tables changed): the watch is set again on what the file has now
  if (st.id !== id || st.path !== answer.path || st.stamp === undefined) {
    return none({ id, path: answer.path, stamp: answer.stamp ?? null, pending: null, modifiedBefore: answer.steady !== false && modified });
  }
  const stamp = answer.stamp ?? null;
  // no file now: nothing to reload (`if( !lastModified.IsValid() ) return;`)
  if (stamp === null) return none({ ...st, pending: null });
  if (stamp === st.stamp) return none({ ...st, pending: null, modifiedBefore: answer.steady === false ? st.modifiedBefore : modified });
  // the studio's own write: the watch moves on with it
  if (answer.own) return none({ ...st, stamp, pending: null, modifiedBefore: answer.steady === false ? st.modifiedBefore : modified });
  // a change of someone else's: wait for the next answer to have the same time (the one-second debounce, restarted by every event)
  if (st.pending !== stamp) return none({ ...st, pending: stamp });
  if (dialogOpen) return none(st);
  const ask = answer.dirty === true || st.modifiedBefore;
  return { state: { ...st, stamp, pending: null, modifiedBefore: false }, action: ask ? "ask" : "reload" };
}
