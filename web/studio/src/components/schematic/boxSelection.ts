// Box-select hit collection for the schematic canvas -- sch_selection_
// tool.cpp's own "crossing" (any overlap counts) vs "enclosed" (must sit
// fully inside) marquee rule, the same convention Canvas.tsx's PCB-side
// `collectBoxSelection` (components/canvas/selectionCandidates.ts)
// already applies to parts/tracks/vias/zones/shapes/texts -- this is that
// convention's schematic-side equivalent, extracted out of
// SchematicView.tsx (which used to inline the symbols-only version of
// this directly) so it is unit-testable the same way every other pure
// geometry module in this app's own "kicad-port" style already is.
//
// Symbols only, until this session: a wire drawn across (or inside) the
// box was never collected, so a box-select could never hand a wire id to
// `common.Interactive.delete` even though that handler's schematic branch
// already knows how to delete one by id (`api.wireById`/`delete_wire`) --
// a single modified click directly on a wire already reached that same
// code path (`SchematicView.tsx`'s own `hitWire` + `applySingleClickModifier`),
// box-select just never did. Added as a second loop here rather than
// generalizing symbols' own loop, since a wire's bounds are its own
// polyline extent, not `symbolBounds`'s lib-symbol-aware box.
import type { Schematic } from "../../api/types";
import { symbolBounds } from "./libSymbol";

interface Box {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

function wireBounds(pts: readonly (readonly [number, number])[]): Box | null {
  if (pts.length === 0) return null;
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const [x, y] of pts) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { minX, minY, maxX, maxY };
}

/** `item` overlaps `box` at all -- true for so much as touching an edge from inside, same loose test the original symbols-only version already used. */
function overlapsBox(item: Box, box: Box): boolean {
  return item.minX < box.maxX && item.maxX > box.minX && item.minY < box.maxY && item.maxY > box.minY;
}

/** `item` sits entirely inside `box`. */
function enclosedByBox(item: Box, box: Box): boolean {
  return item.minX >= box.minX && item.maxX <= box.maxX && item.minY >= box.minY && item.maxY <= box.maxY;
}

/**
 * `box` is `[minX, minY, maxX, maxY]` -- SchematicView.tsx's existing
 * marquee-to-world-space conversion already produces exactly this tuple
 * shape, kept as-is here rather than wrapped in an object at the call
 * site. `crossing`: KiCad's own left-to-right-drag-is-enclosed, right-to-
 * left-drag-is-crossing convention, decided by the caller (unchanged from
 * before this file existed).
 */
export function collectBoxSelection(sch: Schematic, box: [number, number, number, number], crossing: boolean): string[] {
  const [minX, minY, maxX, maxY] = box;
  const selBox: Box = { minX, minY, maxX, maxY };
  const hits: string[] = [];

  for (const s of sch.symbols) {
    const b = symbolBounds(s, sch.lib_symbols);
    if (crossing ? overlapsBox(b, selBox) : enclosedByBox(b, selBox)) hits.push(s.id);
  }
  for (const w of sch.wires) {
    const b = wireBounds(w.pts);
    if (b && (crossing ? overlapsBox(b, selBox) : enclosedByBox(b, selBox))) hits.push(w.id);
  }
  return hits;
}
