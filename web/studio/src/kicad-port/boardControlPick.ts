// Point picking for the board-control tools (`pcbnew.Control.localRatsnestTool`'s `selectionCursor` with `EDIT_TOOL::PadFilter`):
// which pad is under a point. Pure geometry, no React.

export interface PickPad {
  num: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PickPart {
  ref: string;
  placed: boolean;
  pads?: readonly PickPad[];
}

/**
 * The pad under (`x`, `y`): a pad's `x`/`y`/`w`/`h` are its box in board coordinates (the footprint's rotation is already in it), so a
 * hit is a point inside the box. Where boxes overlap the pad whose centre is nearest wins. `id` is `REF.NUMBER`, how the ratsnest names a pad.
 */
export function padAt(parts: readonly PickPart[], x: number, y: number): { ref: string; id: string; num: string } | null {
  let best: { ref: string; id: string; num: string; d: number } | null = null;
  for (const part of parts) {
    if (!part.placed) continue;
    for (const pad of part.pads ?? []) {
      const dx = Math.abs(x - pad.x);
      const dy = Math.abs(y - pad.y);
      if (dx > pad.w / 2 || dy > pad.h / 2) continue;
      const d = dx * dx + dy * dy;
      if (!best || d < best.d) best = { ref: part.ref, id: `${part.ref}.${pad.num}`, num: pad.num, d };
    }
  }
  return best ? { ref: best.ref, id: best.id, num: best.num } : null;
}
