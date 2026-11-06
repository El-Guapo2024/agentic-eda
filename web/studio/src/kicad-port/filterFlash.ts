// The flash of the Selection Filter's rows that refused a click (pcbnew/widgets/panel_selection_filter.cpp at 8303b2ad, UI-LEFT-2 PP19):
//
//   PCB_SELECTION_TOOL::selectPoint       if( collector.GetCount() == 0 && preFilterCount > 0 ) editFrame->HighlightSelectionFilter( rejected );
//   PCB_BASE_EDIT_FRAME::HighlightSelectionFilter      posts EVT_PCB_SELECTION_FILTER_FLASH with the rows
//   PANEL_SELECTION_FILTER::OnFlashEvent  each row flagged (`aOptions.footprints` ...) -> flashCheckbox: its counter set to `m_flashSteps` (10), the timer started
//                                         at 50 ms (`m_flashTimer.Start( 50 )`); a row already flashing starts again from 10
//   onFlashTimer                          every 50 ms each counter goes down by one, a row at 0 stops; the timer stops with the last
//   onPanelPaint                          a row at step `s` is overlaid with the highlight colour blended into the panel's by s / 10, at half alpha (128)
//
// Plain module state with subscribers, like state/commonTool.ts, so the canvas (which knows what was refused) and the panel (which draws the rows) meet here;
// no React here (the panel subscribes with `useSyncExternalStore`), so the unit tests run it.

/** `m_flashSteps`. */
export const FLASH_STEPS = 10;
/** `m_flashTimer.Start( 50 )`: milliseconds a step. */
export const FLASH_INTERVAL_MS = 50;

/** The rows flashing and the step each is at (FLASH_STEPS down to 1). */
let counters: ReadonlyMap<string, number> = new Map();
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function publish(next: Map<string, number>): void {
  counters = next;
  for (const l of listeners) l();
}

/** `onFlashTimer`: every counter down by one, a row at 0 taken out; the timer stops with the last row. */
export function flashTick(): void {
  const next = new Map<string, number>();
  for (const [row, step] of counters) if (step - 1 > 0) next.set(row, step - 1);
  publish(next);
  if (next.size === 0 && timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

/** `OnFlashEvent`: each row given starts (or starts again) at FLASH_STEPS, and the timer runs while any row flashes. `withTimer` false leaves the ticking to the caller (tests). */
export function flashSelectionFilter(rows: Iterable<string>, withTimer = true): void {
  const next = new Map(counters);
  for (const row of rows) next.set(row, FLASH_STEPS);
  if (next.size === counters.size && [...next].every(([r, s]) => counters.get(r) === s)) return;
  publish(next);
  if (withTimer && next.size > 0 && timer === null) timer = setInterval(flashTick, FLASH_INTERVAL_MS);
}

export function getFilterFlash(): ReadonlyMap<string, number> {
  return counters;
}

/** For the panel's `useSyncExternalStore( subscribeFilterFlash, getFilterFlash )`. */
export function subscribeFilterFlash(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * `onPanelPaint`'s overlay of a row at `step`: `blend = ( highlight * step + defaultBg * ( m_flashSteps - step ) ) / m_flashSteps`, drawn at alpha 128 -- over the
 * panel's own colour that is the highlight at `step / m_flashSteps` of half alpha. `highlight` is `wxSYS_COLOUR_HIGHLIGHT` (the studio's selection blue).
 */
export function flashOverlay(step: number, highlight: readonly [number, number, number] = [53, 132, 228]): string {
  const a = (0.5 * Math.max(0, Math.min(step, FLASH_STEPS))) / FLASH_STEPS;
  return `rgba(${highlight[0]}, ${highlight[1]}, ${highlight[2]}, ${a.toFixed(3)})`;
}
