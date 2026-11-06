// Point picking for the board-control tools (`pcbnew.Control.localRatsnestTool`'s `selectionCursor` with `EDIT_TOOL::PadFilter`):
// which pad is under a point. Pure geometry, no React.
import type { Pad } from "../api/types";
import { padHitDistance } from "../components/canvas/selectionCandidates";

export interface PickPad {
  num: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** A circle or an oval (a stadium): `PAD::HitTest` asks the round shape, not its box. */
  round?: boolean;
  /** The copper's contours of a trapezoid, a chamfered rectangle or a custom pad. */
  outline?: Pad["outline"];
}

export interface PickPart {
  ref: string;
  placed: boolean;
  pads?: readonly PickPad[];
}

/**
 * The pad under (`x`, `y`): `selectionCursor` with `EDIT_TOOL::PadFilter` hits a pad by its shape (`PAD::HitTest`: a round pad's circle or
 * stadium, a polygon pad's contours, else its box -- [`padHitDistance`], the selection tool's), so a click in the corner of a round pad's box
 * falls through to the footprint (r7 C7-10). Where pads overlap the one whose centre is nearest wins. `id` is `REF.NUMBER`, how the ratsnest
 * names a pad.
 */
export function padAt(parts: readonly PickPart[], x: number, y: number): { ref: string; id: string; num: string } | null {
  let best: { ref: string; id: string; num: string; d: number } | null = null;
  for (const part of parts) {
    if (!part.placed) continue;
    for (const pad of part.pads ?? []) {
      if (padHitDistance({ ...pad, round: pad.round ?? false }, x, y) > 0) continue;
      const dx = Math.abs(x - pad.x);
      const dy = Math.abs(y - pad.y);
      const d = dx * dx + dy * dy;
      if (!best || d < best.d) best = { ref: part.ref, id: `${part.ref}.${pad.num}`, num: pad.num, d };
    }
  }
  return best ? { ref: best.ref, id: best.id, num: best.num } : null;
}
