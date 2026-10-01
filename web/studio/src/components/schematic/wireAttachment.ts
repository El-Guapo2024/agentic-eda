// `G`: sch_move_tool.cpp::getConnectedDragItems, reduced to this app's
// simpler schematic model. Shared between SchematicView.tsx (the direct
// click-and-drag gesture, and the live rubber-band preview) and
// useActionRunner.ts (arming `G` itself, before any pointer movement has
// happened to resolve attachment some other way).

import type { Schematic } from "../../api/types";
import { resolveLibSymbol } from "./libSymbol";

/**
 * The wire endpoints "attached" to one of `symbolId`'s own pins -- see
 * `Cmd::DragSymbol`'s own doc for the `(wire index, point index)` pair
 * shape this feeds straight into. Ported from
 * `sch_move_tool.cpp::getConnectedDragItems`, reduced to what this app's
 * simpler IR actually models:
 *
 *  - A `Wire` here is already a whole polyline (every bend drawn in one
 *    continuous `W` session), not a separate `SCH_LINE` per segment the
 *    way real eeschema's own connectivity graph does -- so only the
 *    polyline's own two true ends (`pts[0]`/`pts[last]`) are tested for
 *    attachment, the same "wire endpoint" convention `pinSnapPoints`'s
 *    "landing on a pin auto-finishes the wire" check already uses. An
 *    interior bend that happens to coincide with a pin (the wire tool
 *    itself never produces one) is not treated as attached.
 *  - Every matching endpoint is collected, not just one -- a T/+ junction
 *    where 2+ wires land on the same pin keeps every one of them glued to
 *    the symbol and moving together. Source's own `ptHasUnselectedJunction`
 *    branch (insert a new zero-length stub wire rather than drag a wire
 *    away from a junction some of whose other wires are NOT moving) has no
 *    counterpart here: that branch only matters when a caller can select
 *    one wire at a junction independent of the symbol being dragged, which
 *    this app's `G` binding doesn't offer yet (no wire box-select --
 *    PARITY-sch.md item 8), so every wire at a junction a dragged symbol's
 *    pin sits on is always either fully attached (this function) or fully
 *    unrelated -- never the partial case that would leave a gap needing a
 *    stub.
 *  - No labels/sheet-pins/bus-entries: this app's IR has no sheets, and a
 *    label's position is independent of any wire/symbol (`SchematicLabel`
 *    has no "attached to" concept), so there's nothing analogous to carry
 *    along.
 *  - A symbol with no resolved `lib_symbols` graphics (a generic-box
 *    fallback, see `resolveLibSymbol`) has no world-space pin geometry on
 *    this side, same documented gap `pinSnapPoints` already has for the
 *    wire tool -- nothing attaches to it, so dragging it behaves like a
 *    plain Move.
 */
export function attachedWireEndpoints(sch: Schematic, symbolId: string): [number, number][] {
  const sym = sch.symbols.find((s) => s.id === symbolId);
  if (!sym) return [];
  const real = resolveLibSymbol(sym, sch.lib_symbols);
  if (!real) return [];
  const pinPts = real.pins.map((p) => p.tip);
  const pairs: [number, number][] = [];
  sch.wires.forEach((w, wi) => {
    if (w.pts.length < 2) return;
    for (const pi of [0, w.pts.length - 1]) {
      const [x, y] = w.pts[pi]!;
      if (pinPts.some(([px, py]) => px === x && py === y)) pairs.push([wi, pi]);
    }
  });
  return pairs;
}

/**
 * `attachedWireEndpoints` for every ref in a selection that's actually a
 * symbol, keyed by id -- resolved once, at drag-start (see
 * `state.dragAttach`'s own doc in state/store.tsx for why this must be
 * frozen rather than recomputed live: once the symbol has moved, it's no
 * longer "at" the wire's point, so re-resolving mid-drag would just find
 * nothing attached any more).
 */
export function computeDragAttachment(sch: Schematic, refs: readonly string[]): Record<string, [number, number][]> {
  const out: Record<string, [number, number][]> = {};
  for (const ref of refs) {
    if (sch.symbols.some((s) => s.id === ref)) out[ref] = attachedWireEndpoints(sch, ref);
  }
  return out;
}
