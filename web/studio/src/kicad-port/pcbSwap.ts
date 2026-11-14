// Swap (Alt+S): `EDIT_TOOL::Swap` (pcbnew/tools/edit_tool_move_fct.cpp at 8303b2ad; 25ec2335 adds `ReportFilteredLockedItems`), the tool half -- which items, in
// which order -- for the board and the Footprint Editor. The item half (each class's `GetPosition` / `SetPosition`, the layer swap of two items of one type, two
// footprints' sides and orientations, two pads' `SwapShapePositions`) is the backend's: one `swap_items` (board) or `fp_item swap` (Footprint Editor), one undo
// step (crates/ops/src/pcb_swap.rs).
//
// The board's selection as `RequestSelection` filters it for Swap:
//   FilterCollectorForMarkers           a DRC marker is no item here (no `itemKind`)
//   FilterCollectorForHierarchy         an item of a group stands for the outermost group holding it inside the entered one (`TopLevelGroup`); a footprint's
//                                       field or text stands for its footprint's group when the footprint is in one, and is dropped when its footprint is selected
//   FilterCollectorForFreePads          a pad stands for its footprint, unless "Allow free pads" is on (kicad-port/freePads.ts)
//   PCB_TRACE_T removed                 a straight track is left out; an arc (`PCB_ARC_T`) is swapped
//   FilterCollectorForLockedItems       a locked item, an item in a locked group, a group with a locked member is left out (the caller says so: 25ec2335's
//                                       `ReportFilteredLockedItems`)
// then the items in selection order (`GetItemsSortedBySelectionOrder`; a JS Set keeps insertion order). Fewer than two left: nothing to do.
//
// A tuning pattern and a via stitching (`PCB_GENERATOR`: `SetPosition` sets `m_origin` alone), KiCad 10's grid items, drill maps and drill charts are swapped as
// KiCad swaps them (UI-LEFT-3 P3-33, the backend's crates/ops/src/pcb_swap.rs). Left out here, which the backend does not swap: a table cell (KiCad would move the
// cell's box out of its table; the studio's table keeps its cells on its grid).
//
// A group's `GetPosition()` is the centre of its bounding box (`PCB_GROUP::GetPosition`: `GetBoundingBox().Centre()`), `PCB_GROUP::GetBoundingBox` the union of its
// members' boxes (`FOOTPRINT::GetBoundingBox( true )` for a footprint) inflated by 0.25 mm, `BOX2I::Centre` `m_Pos + m_Size / 2` in integers: `groupSwapCentre`. The
// members' boxes are the painter's ports (`itemBounds`, `footprintBox`; the glyph metrics live in the browser), so the centre goes with the command (`centres`). A
// tuning pattern's position is its origin, which the backend reads.
import type { BoardState, Cmd, PointXY } from "../api/types";
import { parseFieldId } from "./fpFields";
import { fpItemBounds, fpItemKind, fpSelectionTarget, type Fp } from "./fpItems";
import { topLevelGroup } from "./groupTree";
import { itemBounds, itemKind, padParent } from "./pcbItems";
import { lockedForTools } from "./pcbTransform";
import { padStaysPad } from "./freePads";


/** `PCB_GROUP::GetBoundingBox().Centre()` from the union `b` of its members' boxes: `bbox.Inflate( pcbIUScale.mmToIU( 0.25 ) )` (no change to the centre), then
 *  `BOX2I::Centre`, `m_Pos + m_Size / 2` with C++'s integer division. */
export function groupSwapCentre(b: readonly [number, number, number, number] | null): PointXY | null {
  if (!b) return null;
  const inflate = 250_000;
  const [x0, y0, x1, y1] = [Math.round(b[0]) - inflate, Math.round(b[1]) - inflate, Math.round(b[2]) + inflate, Math.round(b[3]) + inflate];
  return { x: x0 + Math.trunc((x1 - x0) / 2), y: y0 + Math.trunc((y1 - y0) / 2) };
}

/** The footprint a field (`REF:Name`) or a footprint's own text (`REF~t<k>`) belongs to, else null. */
function footprintOfChild(id: string, kind: string | null): string | null {
  if (kind === "field") return parseFieldId(id)?.ref ?? null;
  if (kind === "fptext") {
    const t = id.lastIndexOf("~t");
    return t > 0 ? id.slice(0, t) : null;
  }
  return null;
}

/** What `EDIT_TOOL::Swap` works on, of the board selection `selection` (in selection order), and whether a lock left something out. */
export function swapSelection(board: BoardState, selection: readonly string[], enteredGroup: string | null = null): { ids: string[]; lockedOut: boolean } {
  const groups = board.drawings?.groups ?? [];
  const selectedParts = new Set(selection.filter((id) => itemKind(board, id) === "part"));
  const out: string[] = [];
  let lockedOut = false;
  for (const raw of selection) {
    let kind = itemKind(board, raw);
    if (kind == null) continue;
    let id = raw;
    // `FilterCollectorForHierarchy`: a footprint's child starts from its footprint (`if( !m_isFootprintEditor && fp ) start = fp`), and goes when it is selected
    // ("Remove children of selected items")
    const parent = footprintOfChild(raw, kind);
    if (parent && selectedParts.has(parent)) continue;
    const top = topLevelGroup(groups, parent ?? raw, enteredGroup);
    if (top && top !== raw) {
      id = top;
      kind = "group";
    }
    // `FilterCollectorForFreePads`: a pad is edited through its footprint (and so through the footprint's group), unless "Allow free pads" is on: then two pads
    // swap their copper's places (`PAD::SwapShapePositions`, UI-LEFT-3 R3-7)
    if (kind === "pad" && !padStaysPad()) {
      const fp = padParent(board, raw);
      if (!fp) continue;
      const fpTop = topLevelGroup(groups, fp, enteredGroup);
      id = fpTop ?? fp;
      kind = fpTop ? "group" : "part";
    }
    // `if( item->Type() == PCB_TRACE_T ) aCollector.Remove( item )`: an arc stays
    if (kind === "track" && !board.routing?.tracks.find((t) => t.id === id)?.arc_mid) continue;
    // a table cell: KiCad moves its box out of its table, which the studio's table cannot hold
    if (kind === "tablecell") continue;
    if (out.includes(id)) continue;
    if (lockedForTools(board, id)) {
      lockedOut = true;
      continue;
    }
    out.push(id);
  }
  return { ids: out, lockedOut };
}

/** Swap of the board selection: one `swap_items`, or none when fewer than two items are left (`if( selection.Size() < 2 ) return 0;`). */
export function planSwap(board: BoardState, selection: readonly string[], enteredGroup: string | null = null): { cmd: Cmd | null; lockedOut: boolean } {
  const { ids, lockedOut } = swapSelection(board, selection, enteredGroup);
  if (ids.length < 2) return { cmd: null, lockedOut };
  const centres: Record<string, PointXY> = {};
  for (const id of ids) {
    // a tuning pattern and a microvia stack are groups here, but `PCB_GENERATOR`s whose position is their origin (the backend's)
    if (itemKind(board, id) !== "group" || board.drawings?.tuning_patterns?.some((p) => p.id === id) || board.drawings?.via_stacks?.some((v) => v.id === id)) continue;
    const c = groupSwapCentre(itemBounds(board, id));
    if (c) centres[id] = c;
  }
  return { cmd: { op: "swap_items", ids, ...(Object.keys(centres).length ? { centres } : {}) }, lockedOut };
}

/**
 * Swap in the Footprint Editor (KiCad's runs the same `EDIT_TOOL::Swap` with `m_isFootprintEditor`): a pad stays a pad, so two pads swap their copper's places; an
 * item of a group stands for its outermost group (no group is entered in this editor); nothing is locked out (`FilterCollectorForLockedItems` only works in the board
 * editor). One `fp_item swap`, or null with fewer than two items.
 */
export function fpSwapCmd(fp: Fp, footprint: string, selection: readonly string[]): Cmd | null {
  const ids: string[] = [];
  for (const raw of selection) {
    if (fpItemKind(fp, raw) == null) continue;
    const id = fpSelectionTarget(fp, raw) ?? raw;
    if (!ids.includes(id)) ids.push(id);
  }
  if (ids.length < 2) return null;
  const centres: Record<string, PointXY> = {};
  for (const id of ids) {
    if (fpItemKind(fp, id) !== "group") continue;
    const c = groupSwapCentre(fpItemBounds(fp, id));
    if (c) centres[id] = c;
  }
  return { op: "fp_item", verb: "swap", footprint, ids, ...(Object.keys(centres).length ? { centres } : {}) };
}
