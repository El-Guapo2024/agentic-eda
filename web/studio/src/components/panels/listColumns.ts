// The headings of the Net Inspector's and the Search panel's lists as a person handles them: a heading's edge dragged to size its column, a heading dragged to
// another place among the others (`wxDATAVIEW_COL_RESIZABLE`, `wxDATAVIEW_COL_REORDERABLE` on the Net Inspector's `wxDataViewCtrl`; the report-mode `wxListView`'s
// own headers in the Search panel), and the width of a text in the list's font (`GetTextExtent`), which Auto-fit measures with. What the drags change is the panels'
// own (kicad-port/netInspector.ts, kicad-port/searchHandlers.ts); this file is only the pointer handling.

/** How far a heading must be dragged before it moves rather than sorts (`wxSYS_DRAG_X`). */
const MOVE_THRESHOLD_PX = 5;

/** A heading clicked right after a drag is not a sort: set while a drag ends, read (and cleared) by the heading's click. */
let draggedAt = 0;

/** Whether a click is the end of a heading's drag (and so not a sort). */
export function clickEndsDrag(): boolean {
  return Date.now() - draggedAt < 300;
}

/** A text's width in px in `el`'s font (`GetTextExtent( text ).x`): measured on a canvas, the font read from the element's computed style. */
export function textWidthIn(el: Element | null): (text: string) => number {
  const canvas = typeof document !== "undefined" ? document.createElement("canvas") : null;
  const ctx = canvas?.getContext("2d") ?? null;
  if (ctx && el) {
    const cs = getComputedStyle(el);
    ctx.font = cs.font && cs.font !== "" ? cs.font : `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  }
  return (text: string) => (ctx ? ctx.measureText(text).width : text.length * 7);
}

/**
 * A heading's edge pressed: the column follows the pointer (`onWidth` with each new width, never under `min`), `onEnd` with the last one when the pointer is let go.
 */
export function startColumnResize(e: { clientX: number; preventDefault(): void; stopPropagation(): void }, startWidth: number, min: number, onWidth: (w: number) => void, onEnd: (w: number) => void): void {
  e.preventDefault();
  e.stopPropagation();
  const x0 = e.clientX;
  let last = startWidth;
  const move = (ev: PointerEvent) => {
    last = Math.max(min, Math.round(startWidth + ev.clientX - x0));
    onWidth(last);
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    draggedAt = Date.now();
    onEnd(last);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

/**
 * The display position a heading dropped at `x` goes to among `headers` (the headings' elements in the order shown): before the first heading whose middle is right
 * of `x`, else at the end.
 */
export function dropIndexAt(headers: readonly Element[], x: number): number {
  for (let i = 0; i < headers.length; i++) {
    const r = headers[i]!.getBoundingClientRect();
    if (x < r.left + r.width / 2) return i;
  }
  return headers.length;
}

/**
 * A heading pressed: once dragged past the threshold it is moving, `onMarker` showing where it would go (a display position, or null when the drag ends), and when let
 * go `onDrop` puts it there. A press that never moves is left to the heading's click (the sort).
 */
export function startColumnMove(e: { clientX: number; clientY: number; button: number }, headers: () => readonly Element[], onMarker: (to: number | null) => void, onDrop: (to: number) => void): void {
  if (e.button !== 0) return;
  const x0 = e.clientX;
  const y0 = e.clientY;
  let moving = false;
  const move = (ev: PointerEvent) => {
    if (!moving && Math.hypot(ev.clientX - x0, ev.clientY - y0) < MOVE_THRESHOLD_PX) return;
    moving = true;
    ev.preventDefault();
    onMarker(dropIndexAt(headers(), ev.clientX));
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (!moving) return;
    draggedAt = Date.now();
    onMarker(null);
    onDrop(dropIndexAt(headers(), ev.clientX));
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}
