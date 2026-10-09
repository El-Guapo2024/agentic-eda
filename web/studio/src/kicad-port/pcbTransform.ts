// Move, Rotate and Flip of any selection of PCB items: which items a command works on, the point it turns or flips them about, and the
// one backend command that does it (`move_items`, `rotate_items`, `flip_items`, crates/ops/src/pcb_transform.rs).
//
// Ports the tool half of `EDIT_TOOL::Move` / `Rotate` / `Flip` (pcbnew/tools/edit_tool.cpp, edit_tool_move_fct.cpp at KiCad 8303b2ad); the
// item half -- what a track, a zone, a text, a dimension or a footprint does with the transform -- is the backend's.
//
//   RequestSelection + FilterCollectorForLockedItems   `editableSelection`: a locked item, a member of a locked group, a group with a
//                                                      locked member stays where it is
//   FilterCollectorForFreePads                         a selected pad stands for its footprint (pads are edited through it)
//   updateModificationPoint                            `modificationPoint`: one item turns about its own position, several about the
//                                                      centre of the selection's box, snapped to the grid
//   Rotate's `usePcbShapeCenter`                       a lone rectangle or polygon turns about its centre
//   Flip's reference point                             the centre of the selection's box (not snapped), or the lone item's position
//                                                      (a lone rectangle's centre)
//
// Pure: no store, no DOM. Unit-tested in pcbTransform.test.ts.

import type { BoardState, Cmd, PointXY } from "../api/types";
import { parseFieldId } from "./fpFields";
import { itemBounds, itemKind, itemPosition, padParent, unionBounds } from "./pcbItems";

export type FlipDirection = "left_right" | "top_bottom";
export type Pt = readonly [number, number];
export type Snap = (p: Pt) => [number, number];

const NO_SNAP: Snap = (p) => [p[0], p[1]];

const toXY = (p: Pt): PointXY => ({ x: Math.round(p[0]), y: Math.round(p[1]) });

// ------------------------------------------------------------------ locks

/** The group an id is a member of, or null. */
export function groupOf(board: BoardState, id: string): string | null {
  return board.drawings?.groups.find((g) => g.member_ids.includes(id))?.id ?? null;
}

/**
 * `BOARD_ITEM::IsLocked()` (an item's own lock, or its parent group's) joined with `FilterCollectorForLockedItems`' extra rule: a group
 * is as good as locked when any of its members is.
 */
export function isLocked(board: BoardState, id: string): boolean {
  const locked = new Set(board.locked ?? []);
  if (locked.has(id)) return true;
  // A footprint's field is as locked as the footprint (`PCB_TEXT::IsLocked`: "|| GetParentFootprint()->IsLocked()").
  const field = parseFieldId(id);
  if (field && locked.has(field.ref)) return true;
  const group = board.drawings?.groups.find((g) => g.id === id);
  if (group) return group.member_ids.some((m) => locked.has(m));
  const parent = groupOf(board, id);
  return parent != null && locked.has(parent);
}

// ------------------------------------------------------- the working selection

/**
 * What `RequestSelection` hands the edit tools: the selection with every pad promoted to its footprint (`FilterCollectorForFreePads`,
 * pads are edited through their footprint), ids the board no longer has dropped, and every locked item taken out
 * (`FilterCollectorForLockedItems`) -- so `lockedOut` is how the tool knows it left something where it was. Order is kept, repeats dropped.
 */
export function editableSelection(board: BoardState, ids: readonly string[], opts: { respectLocks?: boolean } = {}): { ids: string[]; lockedOut: boolean } {
  const respectLocks = opts.respectLocks ?? true;
  const out: string[] = [];
  const seen = new Set<string>();
  let lockedOut = false;
  for (const raw of ids) {
    const id = itemKind(board, raw) === "pad" ? padParent(board, raw) : raw;
    if (id == null || seen.has(id) || itemKind(board, id) == null) continue;
    seen.add(id);
    if (respectLocks && isLocked(board, id)) {
      lockedOut = true;
      continue;
    }
    out.push(id);
  }
  // `FilterCollectorForHierarchy`: a field travels with its footprint, so it is not moved on its own when the footprint is in the selection too.
  const kept = out.filter((id) => {
    const field = parseFieldId(id);
    return !(field && itemKind(board, id) === "field" && out.includes(field.ref));
  });
  return { ids: kept, lockedOut };
}

// ------------------------------------------------------------ reference points

/** `SELECTION::GetCenter()`: the centre of the box around every item of the selection. */
export function selectionCenter(board: BoardState, ids: readonly string[]): [number, number] | null {
  const b = unionBounds(ids.map((id) => itemBounds(board, id)));
  return b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : null;
}

/**
 * `EDIT_TOOL::updateModificationPoint`: a single item's reference point is its position (`BOARD_ITEM::GetPosition()`); with several it is
 * the centre of the selection's box moved to the nearest grid point (`PCB_GRID_HELPER::BestSnapAnchor`; here the grid point alone).
 */
export function modificationPoint(board: BoardState, ids: readonly string[], snap: Snap = NO_SNAP): [number, number] | null {
  if (ids.length === 0) return null;
  if (ids.length === 1) {
    const at = itemPosition(board, ids[0]!);
    return at ? [at[0], at[1]] : null;
  }
  const c = selectionCenter(board, ids);
  return c ? snap(c) : null;
}

/** A lone rectangle or polygon, the shapes whose own position is a corner: KiCad turns and flips them about their centre so they stay put. */
function isLoneRectOrPoly(board: BoardState, ids: readonly string[]): boolean {
  if (ids.length !== 1) return false;
  const s = board.drawings?.shapes.find((q) => q.id === ids[0]);
  return s != null && (s.kind === "rect" || s.kind === "polygon");
}

/** The point `EDIT_TOOL::Rotate` turns the selection about. */
export function rotationPivot(board: BoardState, ids: readonly string[], snap: Snap = NO_SNAP): [number, number] | null {
  if (isLoneRectOrPoly(board, ids)) return selectionCenter(board, ids);
  return modificationPoint(board, ids, snap);
}

/** The point `EDIT_TOOL::Flip` mirrors the selection about: the centre of its box, or a lone item's own position (a lone rectangle's centre). */
export function flipPivot(board: BoardState, ids: readonly string[]): [number, number] | null {
  if (ids.length === 1) {
    const s = board.drawings?.shapes.find((q) => q.id === ids[0]);
    if (!(s && s.kind === "rect")) return modificationPoint(board, ids);
  }
  return selectionCenter(board, ids);
}

// ---------------------------------------------------------------------- plans

export interface TransformPlan {
  /** The commands, ready to send as one batch (empty when there is nothing to do). */
  cmds: Cmd[];
  /** The items the commands act on. */
  ids: string[];
  /** Part of the selection was locked and so was left alone. */
  lockedOut: boolean;
}

const EMPTY: TransformPlan = { cmds: [], ids: [], lockedOut: false };

/**
 * `EDIT_TOOL::Move` of a selection by `(dx, dy)`: one `move_items`. Footprints, tracks (with the vias among them), zones, graphics, text,
 * dimensions and groups all move; locked items stay.
 */
export function planMove(board: BoardState, selection: readonly string[], dx: number, dy: number): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0 || (dx === 0 && dy === 0)) return { ...EMPTY, lockedOut };
  return { cmds: [{ op: "move_items", ids, dx: Math.round(dx), dy: Math.round(dy) }], ids, lockedOut };
}

/**
 * `EDIT_TOOL::Rotate`: `quarterTurnsCcw` quarter turns counter-clockwise on the screen (R is 1, Shift+R is -1; KiCad's rotation step, 90
 * degrees by default, is `stepDeg`). The backend's angle runs the other way (clockwise is positive), hence the negation.
 */
export function planRotate(board: BoardState, selection: readonly string[], quarterTurnsCcw: number, snap: Snap = NO_SNAP, stepDeg = 90): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0 || quarterTurnsCcw === 0) return { ...EMPTY, lockedOut };
  const pivot = rotationPivot(board, ids, snap);
  if (!pivot) return { ...EMPTY, lockedOut };
  return { cmds: [{ op: "rotate_items", ids, pivot: toXY(pivot), angle_millideg: Math.round(-quarterTurnsCcw * stepDeg * 1000) }], ids, lockedOut };
}

/** `EDIT_TOOL::Flip` ("Change Side / Flip", F): the selection mirrored about its flip point and put on the other side of the board. */
export function planFlip(board: BoardState, selection: readonly string[], direction: FlipDirection = "left_right"): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { ...EMPTY, lockedOut };
  const pivot = flipPivot(board, ids);
  if (!pivot) return { ...EMPTY, lockedOut };
  return { cmds: [{ op: "flip_items", ids, pivot: toXY(pivot), direction }], ids, lockedOut };
}

/**
 * A Move that was turned or flipped on the way (R / F pressed while the selection was carried): `EDIT_TOOL::Rotate` and `::Flip` act
 * about the selection's reference point, which travels with the cursor, so the net effect is the same as turning it about where it was picked
 * up and then moving it -- one batch, in that order. `turnsCcw` counts the R presses (Shift+R takes one away).
 */
export function planCarry(board: BoardState, selection: readonly string[], dx: number, dy: number, turnsCcw = 0, flipped = false, snap: Snap = NO_SNAP): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { ...EMPTY, lockedOut };
  const cmds: Cmd[] = [];
  const turns = ((turnsCcw % 4) + 4) % 4;
  if (turns !== 0) {
    const pivot = rotationPivot(board, ids, snap);
    if (pivot) cmds.push({ op: "rotate_items", ids, pivot: toXY(pivot), angle_millideg: -turns * 90_000 });
  }
  if (flipped) {
    const pivot = flipPivot(board, ids);
    if (pivot) cmds.push({ op: "flip_items", ids, pivot: toXY(pivot), direction: "left_right" });
  }
  if (dx !== 0 || dy !== 0) cmds.push({ op: "move_items", ids, dx: Math.round(dx), dy: Math.round(dy) });
  return { cmds, ids, lockedOut };
}

/** `ROTATION_ANCHOR` of `DIALOG_MOVE_EXACT`: each item about its own anchor, the selection's centre (after the move), or the local coordinates origin. */
export type MoveExactAnchor = "item" | "center" | "origin";

/**
 * `EDIT_TOOL::MoveExact` ("Move Exactly...", Shift+M): every item of the selection is moved by `(dx, dy)` and then turned by `angleDegCcw` (the dialog's
 * angle, counter-clockwise on the screen) about its own anchor, the centre of the selection's box (measured before the move, then moved with it: "make sure
 * the rotation is from the right reference point"), or the local origin -- the whole thing one batch, so one undo step. The same items as any Move
 * (locked ones stay, a pad stands for its footprint), whatever their kind.
 */
export function planMoveExact(board: BoardState, selection: readonly string[], dx: number, dy: number, angleDegCcw: number, anchor: MoveExactAnchor, localOrigin: Pt = [0, 0]): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { ...EMPTY, lockedOut };
  const cmds: Cmd[] = [];
  const [mx, my] = [Math.round(dx), Math.round(dy)];
  if (mx !== 0 || my !== 0) cmds.push({ op: "move_items", ids, dx: mx, dy: my });
  const angle = Math.round(-angleDegCcw * 1000); // the backend turns clockwise for a positive angle
  if (angle !== 0) {
    if (anchor === "item") {
      // `boardItem->Rotate( boardItem->GetPosition(), angle )` after the item was moved
      for (const id of ids) {
        const at = itemPosition(board, id);
        if (at) cmds.push({ op: "rotate_items", ids: [id], pivot: toXY([at[0] + mx, at[1] + my]), angle_millideg: angle });
      }
    } else {
      const centre = anchor === "center" ? selectionCenter(board, ids) : [localOrigin[0], localOrigin[1]];
      if (centre) {
        const [sx, sy] = anchor === "center" ? [mx, my] : [0, 0];
        cmds.push({ op: "rotate_items", ids, pivot: toXY([centre[0]! + sx!, centre[1]! + sy!]), angle_millideg: angle });
      }
    }
  }
  return { cmds, ids, lockedOut };
}

// ------------------------------------------------------------- picking up

export interface CarryStart {
  /** What the Move (or drag) takes in the hand: the selection `RequestSelection` hands it, locked items out, pads as their footprints. */
  refs: string[];
  /** The point R turns the selection about, and the point F mirrors it about -- measured where it was picked up. */
  pivotUm?: [number, number];
  flipPivotUm?: [number, number];
  /** Part of the selection was locked and stayed where it was. */
  lockedOut: boolean;
}

/** `EDIT_TOOL::Move` picking a selection up: the items it works on and the points a turn or a flip during the move acts about. */
export function carryStart(board: BoardState, selection: readonly string[], snap: Snap = NO_SNAP): CarryStart {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { refs: [], lockedOut };
  const pivot = rotationPivot(board, ids, snap);
  const flip = flipPivot(board, ids);
  return { refs: ids, pivotUm: pivot ?? undefined, flipPivotUm: flip ?? undefined, lockedOut };
}
