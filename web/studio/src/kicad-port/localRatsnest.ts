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
  /** Pack and Move (`P`): each ref's own extra shift, on top of the shared `dxUm`/`dyUm`. */
  perRefOffsetUm?: Readonly<Record<string, readonly [number, number]>>;
}

/**
 * Shift every ratsnest edge endpoint that coincides with one of the
 * moving part(s)' (pre-move) pad positions by the preview's live delta.
 * Only meaningful for `kind === "part"` (a via/shape/text move has no
 * ratsnest of its own in this app's model); returns `edges` unchanged
 * (new array, same edge objects) when there's no part move in progress.
 */
export function offsetRatsnestForPreview<E extends RatsnestEdgeLike>(edges: readonly E[], board: RatsnestBoardLike, preview: MovePreviewLike | null): E[] {
  if (!preview || (preview.kind ?? "part") !== "part" || (preview.dxUm === 0 && preview.dyUm === 0 && !preview.perRefOffsetUm)) return [...edges];

  // Point -> the live delta of the part that owns it (every part shares dxUm/dyUm; a Pack and Move adds its own per-part shift).
  const movingPoints = new Map<string, [number, number]>();
  for (const ref of preview.refs) {
    const part = board.parts.find((p) => p.ref === ref);
    if (!part) continue;
    const own = preview.perRefOffsetUm?.[ref];
    const delta: [number, number] = [preview.dxUm + (own?.[0] ?? 0), preview.dyUm + (own?.[1] ?? 0)];
    for (const pad of part.pads ?? []) movingPoints.set(`${pad.x},${pad.y}`, delta);
    if (part.at) movingPoints.set(`${part.at[0]},${part.at[1]}`, delta);
  }
  if (movingPoints.size === 0) return [...edges];

  const shift = (pt: readonly [number, number]): [number, number] => {
    const d = movingPoints.get(`${pt[0]},${pt[1]}`);
    return d ? [pt[0] + d[0], pt[1] + d[1]] : [pt[0], pt[1]];
  };

  return edges.map((e) => ({ ...e, from: shift(e.from), to: shift(e.to) }));
}
