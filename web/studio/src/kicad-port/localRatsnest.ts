// "Local ratsnest" during a move: pcbnew's real, non-router behavior
// when you drag a footprint is NOT to drag its connected tracks along
// (that needs the PNS router -- see edit_tool.cpp's Move/doMoveSelection,
// which just does `item->Move(movement)` on the selection itself and
// then `PCB_ACTIONS::updateLocalRatsnest`/CONNECTIVITY_DATA's dynamic
// ratsnest to redraw airwires live from the moving item's NEW position
// to its still-stationary neighbours). A previous description of this
// task's item 2 ("connected track ends following") doesn't match source
// for a plain Move -- verified directly against edit_tool_move_fct.cpp,
// see PARITY-pcb.md -- so this ports the thing source actually does
// instead: offset the ratsnest edges touching a moving part's pads by
// the live preview delta, purely for display, before the move commits
// and the backend recomputes the real thing.
export interface RatsnestEdgeLike {
  net: string;
  from: readonly [number, number];
  to: readonly [number, number];
}

export interface RatsnestBoardLike {
  parts: ReadonlyArray<{
    ref: string;
    pads?: ReadonlyArray<{ x: number; y: number }>;
    at?: readonly [number, number];
  }>;
}

export interface MovePreviewLike {
  refs: readonly string[];
  kind?: "part" | "via" | "shape" | "text";
  dxUm: number;
  dyUm: number;
}

/**
 * Shift every ratsnest edge endpoint that coincides with one of the
 * moving part(s)' (pre-move) pad positions by the preview's live delta.
 * Only meaningful for `kind === "part"` (a via/shape/text move has no
 * ratsnest of its own in this app's model); returns `edges` unchanged
 * (new array, same edge objects) when there's no part move in progress.
 */
export function offsetRatsnestForPreview<E extends RatsnestEdgeLike>(edges: readonly E[], board: RatsnestBoardLike, preview: MovePreviewLike | null): E[] {
  if (!preview || (preview.kind ?? "part") !== "part" || (preview.dxUm === 0 && preview.dyUm === 0)) return [...edges];

  const movingPoints = new Set<string>();
  for (const ref of preview.refs) {
    const part = board.parts.find((p) => p.ref === ref);
    if (!part) continue;
    for (const pad of part.pads ?? []) movingPoints.add(`${pad.x},${pad.y}`);
    if (part.at) movingPoints.add(`${part.at[0]},${part.at[1]}`);
  }
  if (movingPoints.size === 0) return [...edges];

  const shift = (pt: readonly [number, number]): [number, number] => (movingPoints.has(`${pt[0]},${pt[1]}`) ? [pt[0] + preview.dxUm, pt[1] + preview.dyUm] : [pt[0], pt[1]]);

  return edges.map((e) => ({ ...e, from: shift(e.from), to: shift(e.to) }));
}
