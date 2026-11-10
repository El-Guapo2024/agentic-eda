// Swap (Alt+S): `EDIT_TOOL::Swap` (pcbnew/tools/edit_tool_move_fct.cpp at 8303b2ad; 25ec2335 adds `ReportFilteredLockedItems`), the tool half -- which items, in
// which order -- for the board and the Footprint Editor. The item half (each class's `GetPosition` / `SetPosition`, the layer swap of two items of one type, two
// footprints' sides and orientations, two pads' `SwapShapePositions`) is the backend's: one `swap_items` (board) or `fp_item swap` (Footprint Editor), one undo
// step (crates/ops/src/pcb_swap.rs).
//
// The board's selection as `RequestSelection` filters it for Swap:
//   FilterCollectorForMarkers           a DRC marker is no item here (no `itemKind`)
//   FilterCollectorForHierarchy         an item of a group stands for the outermost group holding it inside the entered one (`TopLevelGroup`); a footprint's
//                                       field or text stands for its footprint's group when the footprint is in one, and is dropped when its footprint is selected
//   FilterCollectorForFreePads          a pad stands for its footprint (the studio has no "Allow free pads")
//   PCB_TRACE_T removed                 a straight track is left out; an arc (`PCB_ARC_T`) is swapped
//   FilterCollectorForLockedItems       a locked item, an item in a locked group, a group with a locked member is left out (the caller says so: 25ec2335's
//                                       `ReportFilteredLockedItems`)
// then the items in selection order (`GetItemsSortedBySelectionOrder`; a JS Set keeps insertion order). Fewer than two left: nothing to do.
//
// Left out here, which the backend does not swap: a table cell (KiCad would move the cell's box out of its table), a tuning pattern and a via stitching
// (`PCB_GENERATOR::SetPosition` sets their origin alone), KiCad 10's grid items and drill maps.
//
// A group's `GetPosition()` is the centre of its bounding box (`PCB_GROUP::GetPosition`, `GetBoundingBox().Centre()`), which `itemBounds` / `fpItemBounds` work out;
// it goes with the command (`centres`).
import type { BoardState, Cmd, PointXY } from "../api/types";
import { parseFieldId } from "./fpFields";
import { fpItemBounds, fpItemKind, fpSelectionTarget, type Fp } from "./fpItems";
import { topLevelGroup } from "./groupTree";
import { itemBounds, itemKind, padParent } from "./pcbItems";
import { isLocked } from "./pcbTransform";

const centreOf = (b: readonly [number, number, number, number] | null): PointXY | null => (b ? { x: Math.round((b[0] + b[2]) / 2), y: Math.round((b[1] + b[3]) / 2) } : null);

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
    // `FilterCollectorForFreePads`: a pad is edited through its footprint (and so through the footprint's group)
    if (kind === "pad") {
      const fp = padParent(board, raw);
      if (!fp) continue;
      const fpTop = topLevelGroup(groups, fp, enteredGroup);
      id = fpTop ?? fp;
      kind = fpTop ? "group" : "part";
    }
    // `if( item->Type() == PCB_TRACE_T ) aCollector.Remove( item )`: an arc stays
    if (kind === "track" && !board.routing?.tracks.find((t) => t.id === id)?.arc_mid) continue;
    if (kind === "tablecell" || kind === "via_stitch" || kind === "grid" || kind === "drill_map") continue;
    if (kind === "group" && board.drawings?.tuning_patterns?.some((p) => p.id === id)) continue;
    if (out.includes(id)) continue;
    if (isLocked(board, id)) {
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
    if (itemKind(board, id) !== "group") continue;
    const c = centreOf(itemBounds(board, id));
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
    const id = fpSelectionTarget(fp, raw);
    if (!ids.includes(id)) ids.push(id);
  }
  if (ids.length < 2) return null;
  const centres: Record<string, PointXY> = {};
  for (const id of ids) {
    if (fpItemKind(fp, id) !== "group") continue;
    const c = centreOf(fpItemBounds(fp, id));
    if (c) centres[id] = c;
  }
  return { op: "fp_item", verb: "swap", footprint, ids, ...(Object.keys(centres).length ? { centres } : {}) };
}
