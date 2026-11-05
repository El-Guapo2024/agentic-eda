// The board's Swap (Alt+S): `EDIT_TOOL::Swap` (pcbnew/tools/edit_tool_move_fct.cpp at 8303b2ad), planned as the `/api/cmd` verbs of one undo step.
//
// - The selection as `RequestSelection` filters it: a pad stands for its footprint (`FilterCollectorForFreePads`), a straight track is left out
//   (`if( item->Type() == PCB_TRACE_T ) aCollector.Remove( item )`), and so is a locked item -- its own lock, a locked group's above it, or a group with a
//   locked member (`FilterCollectorForLockedItems`; round 6 E6-3). Fewer than two left: nothing to do.
// - Then, in selection order, `for( i = 0; i < n - 1; i++ ) swap( sorted[i], sorted[i + 1] )`: the positions (`GetPosition` / `SetPosition`), and for two
//   items of the same type the layers ("We can also do a layer swap safely for two objects of the same type, except groups"). Two footprints also swap
//   their orientation and side; a selection of footprints only is `swap_chain`, which does that (round 6 E6-2 for every other kind).
//
// - A zone (round 7 R7-2): `ZONE::SetPosition` is a no-op (zone.h:116, `void SetPosition( const VECTOR2I& aPos ) override {}`), so a zone never moves, while
//   the item it is paired with still moves to the zone's `GetPosition`, its first corner. Two zones are "two objects of the same type", so their layers swap
//   through `ZONE::GetLayer` (zone.cpp:501: the layer of a set of exactly one, else `UNDEFINED_LAYER`) and `ZONE::SetLayer` (zone.cpp:550, `SetLayerSet(
//   LSET( { aLayer } ) )`). LSET's list constructor skips a negative layer (common/lset.cpp:44) and `SetLayerSet` returns on an empty set (zone.cpp:556), so
//   a zone on several layers given a single-layer zone's layer becomes a zone on that layer alone, and the single-layer zone keeps its own.
//
// Not swapped here (left out of the selection): arc tracks, dimensions, groups, tables, text boxes, barcodes and points, whose `SetPosition` this model
// has no verb for (an arc's moves only its start, a dimension's only its start, a group's is its box's centre). Two footprints in a selection that has other
// kinds too swap their positions but not their orientation or side. Two pads are not swapped as pads (`PAD::SwapShapePositions`): a pad is its footprint.
import type { BoardState, Cmd } from "../api/types";
import { shapePosition } from "./pcbConvert";
import { itemKind, padParent } from "./pcbItems";
import { isLocked } from "./pcbTransform";
import { zoneEditCmd } from "./zoneCmd";
import { zoneLayers } from "./zoneLayers";

type Kind = "part" | "via" | "shape" | "text" | "zone";

interface SwapItem {
  id: string;
  kind: Kind;
  pos: readonly [number, number];
  /** `GetLayer()`, for the kinds whose layers swap; null for the others, and for a zone on several layers (`UNDEFINED_LAYER`). */
  layer: string | null;
}

function swapItem(board: BoardState, id: string): SwapItem | null {
  const kind = itemKind(board, id);
  switch (kind) {
    case "part": {
      const p = board.parts.find((x) => x.ref === id && x.placed && x.at);
      return p?.at ? { id, kind, pos: [p.at[0], p.at[1]], layer: null } : null;
    }
    case "via": {
      const v = board.routing?.vias.find((x) => x.id === id);
      return v ? { id, kind, pos: [v.x, v.y], layer: null } : null;
    }
    case "shape": {
      const s = board.drawings?.shapes.find((x) => x.id === id);
      return s ? { id, kind, pos: shapePosition(s), layer: s.layer } : null;
    }
    case "text": {
      const t = board.drawings?.texts.find((x) => x.id === id);
      return t ? { id, kind, pos: [t.x, t.y], layer: t.layer } : null;
    }
    case "zone": {
      // `ZONE::GetPosition`: its first corner. `ZONE::GetLayer`: the layer of a zone on one, else `UNDEFINED_LAYER`.
      const z = board.routing?.zones.find((x) => x.id === id);
      const c = z?.outline[0];
      const layers = z ? zoneLayers(z) : [];
      return c ? { id, kind, pos: [c[0], c[1]], layer: layers.length === 1 ? layers[0]! : null } : null;
    }
    default:
      return null;
  }
}

/** What Swap does to `selection` (in selection order): `swap_chain` for footprints only, else the moves and layer changes, or null when fewer than two items are left. */
export function planSwap(board: BoardState, selection: readonly string[]): Cmd[] | null {
  const ids: string[] = [];
  for (const raw of selection) {
    const id = padParent(board, raw) ?? raw;
    if (!ids.includes(id)) ids.push(id);
  }
  const items = ids.filter((id) => !isLocked(board, id)).map((id) => swapItem(board, id)).filter((x): x is SwapItem => x !== null);
  if (items.length < 2) return null;
  if (items.every((x) => x.kind === "part")) return [{ op: "swap_chain", parts: items.map((x) => x.id) }];
  const pos = items.map((x) => x.pos);
  const layer = items.map((x) => x.layer);
  for (let i = 0; i < items.length - 1; i++) {
    const [a, b] = [items[i]!, items[i + 1]!];
    // `a->SetPosition( bPos ); b->SetPosition( aPos );` of the positions as the previous pair left them; `ZONE::SetPosition` does nothing.
    const [aPos, bPos] = [pos[i]!, pos[i + 1]!];
    if (a.kind !== "zone") pos[i] = bPos;
    if (b.kind !== "zone") pos[i + 1] = aPos;
    if (a.kind !== b.kind || a.kind === "part") continue;
    const [aLayer, bLayer] = [layer[i] ?? null, layer[i + 1] ?? null];
    if (a.kind === "zone") {
      // `SetLayer( UNDEFINED_LAYER )` leaves a zone as it is (an empty `LSET`).
      if (bLayer != null) layer[i] = bLayer;
      if (aLayer != null) layer[i + 1] = aLayer;
    } else {
      [layer[i], layer[i + 1]] = [bLayer, aLayer];
    }
  }
  const cmds: Cmd[] = [];
  items.forEach((x, i) => {
    const [dx, dy] = [Math.round(pos[i]![0] - x.pos[0]), Math.round(pos[i]![1] - x.pos[1])];
    if (dx !== 0 || dy !== 0) cmds.push({ op: "move_items", ids: [x.id], dx, dy });
    const to = layer[i];
    if (to == null || to === x.layer) return;
    if (x.kind === "shape") {
      const s = board.drawings!.shapes.find((q) => q.id === x.id)!;
      cmds.push({ op: "edit_shape", id: x.id, layer: to, stroke_width: s.stroke_width, filled: s.filled });
    } else if (x.kind === "text") {
      const t = board.drawings!.texts.find((q) => q.id === x.id)!;
      cmds.push({ op: "edit_text", id: x.id, content: t.content, angle: t.angle, layer: to, size_nm: t.size, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror });
    } else if (x.kind === "zone") {
      // `SetLayerSet( LSET( { aLayer } ) )`: the zone is on that one layer now, every other setting as it was.
      const z = board.routing!.zones.find((q) => q.id === x.id)!;
      cmds.push(zoneEditCmd(z, { layer: to, layers: [to] }));
    }
  });
  return cmds.length > 0 ? cmds : null;
}
