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
import { ancestors, groupAndDescendants, groupLeaves, isGroup, parentGroup } from "./groupTree";
import { cellById } from "./tables";
import type { CarryOp } from "./pcbCarry";

export type FlipDirection = "left_right" | "top_bottom";
export type Pt = readonly [number, number];
export type Snap = (p: Pt) => [number, number];

const NO_SNAP: Snap = (p) => [p[0], p[1]];

const toXY = (p: Pt): PointXY => ({ x: Math.round(p[0]), y: Math.round(p[1]) });

// ------------------------------------------------------------------ locks

/** The group an id is a member of, or null. */
export function groupOf(board: BoardState, id: string): string | null {
  return parentGroup(board.drawings?.groups ?? [], id)?.id ?? null;
}

/**
 * `BOARD_ITEM::IsLocked()` (an item's own lock, or that of any group above it) joined with `FilterCollectorForLockedItems`' extra rule: an item
 * is as good as locked when anything below it is -- a group with a locked member, however deep.
 */
export function isLocked(board: BoardState, id: string): boolean {
  // a tuning pattern (a `PCB_GENERATOR`) and its tracks are changed through the pattern -- its properties and its edit points -- not moved, turned or flipped
  const locked = new Set([...(board.locked ?? []), ...(board.drawings?.groups ?? []).filter((g) => g.generator).map((g) => g.id)]);
  if (locked.has(id)) return true;
  // A footprint's field is as locked as the footprint (`PCB_TEXT::IsLocked`: "|| GetParentFootprint()->IsLocked()").
  const field = parseFieldId(id);
  if (field && locked.has(field.ref)) return true;
  const groups = board.drawings?.groups ?? [];
  if (groups.length === 0) return false;
  if (isGroup(groups, id) && [...groupAndDescendants(groups, id), ...groupLeaves(groups, id)].some((m) => locked.has(m))) return true;
  return ancestors(groups, id).some((g) => locked.has(g.id));
}

// ------------------------------------------------------- the working selection

/**
 * What `RequestSelection` hands the edit tools: the selection with every pad promoted to its footprint (`FilterCollectorForFreePads`,
 * pads are edited through their footprint), ids the board no longer has dropped, and every locked item taken out
 * (`FilterCollectorForLockedItems`) -- so `lockedOut` is how the tool knows it left something where it was. Order is kept, repeats dropped.
 */
export function editableSelection(board: BoardState, ids: readonly string[], opts: { respectLocks?: boolean; forClipboard?: boolean } = {}): { ids: string[]; lockedOut: boolean } {
  const respectLocks = opts.respectLocks ?? true;
  const out: string[] = [];
  const seen = new Set<string>();
  let lockedOut = false;
  for (const raw of ids) {
    // (`FilterCollectorForTableCells`: a table cell stands for its table in a move, a turn, a flip, a duplicate and a delete of the whole item)
    const kind = itemKind(board, raw);
    // (`forClipboard`: Copy and Cut take a pad and a table cell as themselves -- `copyToClipboard`'s filter has neither `FilterCollectorForFreePads` nor
    // `FilterCollectorForTableCells`: `SaveSelection` copies a cell's table cut down to the selected cells, and Cut's `DeleteItems` clears the cell)
    const id = kind === "pad" ? (opts.forClipboard ? raw : padParent(board, raw)) : kind === "tablecell" ? (opts.forClipboard ? raw : (cellById(board, raw)?.table.id ?? null)) : raw;
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

/**
 * The point `EDIT_TOOL::Rotate` turns the selection about: a lone text box about its centre (`PCB_TEXTBOX::GetCenter()`), a lone rectangle, polygon or
 * table about its centre (`usePcbShapeCenter`), any other lone item about its position, several about the snapped centre of their box.
 */
export function rotationPivot(board: BoardState, ids: readonly string[], snap: Snap = NO_SNAP): [number, number] | null {
  if (isLoneRectOrPoly(board, ids)) return selectionCenter(board, ids);
  if (ids.length === 1) {
    const kind = itemKind(board, ids[0]!);
    if (kind === "textbox" || kind === "table") return selectionCenter(board, ids);
  }
  return modificationPoint(board, ids, snap);
}

/**
 * The point `EDIT_TOOL::Flip` mirrors the selection about: the centre of its box, or a lone item's own position -- but a lone rectangle and a lone table
 * flip about their centre ("Tables are a bunch of rectangles, so exclude them too").
 */
export function flipPivot(board: BoardState, ids: readonly string[]): [number, number] | null {
  if (ids.length === 1) {
    const s = board.drawings?.shapes.find((q) => q.id === ids[0]);
    if (!(s && s.kind === "rect") && itemKind(board, ids[0]!) !== "table") return modificationPoint(board, ids);
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

/**
 * `EDIT_TOOL::Flip` ("Change Side / Flip", F): the selection mirrored about its flip point and put on the other side of the board, across the axis
 * `PCBNEW_SETTINGS::m_FlipDirection` says (`direction`, left/right by default).
 */
export function planFlip(board: BoardState, selection: readonly string[], direction: FlipDirection = "left_right"): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { ...EMPTY, lockedOut };
  const pivot = flipPivot(board, ids);
  if (!pivot) return { ...EMPTY, lockedOut };
  return { cmds: [{ op: "flip_items", ids, pivot: toXY(pivot), direction }], ids, lockedOut };
}

/**
 * The angle `turnsCcw` presses of R (Shift+R takes one away) add up to, counter-clockwise degrees in [0, 360): `rotateAngle` of `EDIT_TOOL::Rotate` once per press,
 * `PCBNEW_SETTINGS::m_RotationAngle` (`stepDeg`) each. A whole number of turns is nothing.
 */
export function carryAngleDeg(turnsCcw: number, stepDeg = 90): number {
  const raw = turnsCcw * stepDeg;
  const norm = ((raw % 360) + 360) % 360;
  return norm < 1e-9 || 360 - norm < 1e-9 ? 0 : Math.round(norm * 1000) / 1000;
}

/**
 * The count of R presses kept within one turn when whole steps make a turn (a 90 degree step: 0 to 3, a 15 degree one: 0 to 23); a step that does not divide a
 * turn keeps counting, the angle being worked out from the count (`carryAngleDeg`).
 */
export function wrapTurns(count: number, stepDeg = 90): number {
  const per = 360 / stepDeg;
  const whole = Math.round(per);
  return whole > 0 && Math.abs(per - whole) < 1e-9 ? ((count % whole) + whole) % whole : count;
}

/**
 * A Move that was turned or flipped on the way (R / F pressed while the selection was carried): `EDIT_TOOL::Rotate` and `::Flip` act
 * about the selection's reference point, which travels with the cursor, so the net effect is the same as turning it about where it was picked
 * up and then moving it -- one batch, in that order. `ops` are the presses in the order they were made, each about its own point
 * (kicad-port/pcbCarry.ts `carryPress`); without them, `turnsCcw` counts the R presses (Shift+R takes one away) and `flipped` follows them.
 */
export function planCarry(board: BoardState, selection: readonly string[], dx: number, dy: number, turnsCcw = 0, flipped = false, snap: Snap = NO_SNAP, stepDeg = 90, direction: FlipDirection = "left_right", ops?: readonly CarryOp[]): TransformPlan {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { ...EMPTY, lockedOut };
  const cmds: Cmd[] = [];
  if (ops) {
    // The presses in the order they were made, each about its own point (kicad-port/pcbCarry.ts `carryPress`); turns in a row about one point are one turn.
    let k = 0;
    while (k < ops.length) {
      const op = ops[k]!;
      if (op.kind === "flip") {
        cmds.push({ op: "flip_items", ids, pivot: toXY(op.pivot), direction: op.direction });
        k++;
        continue;
      }
      let deg = 0;
      while (k < ops.length && ops[k]!.kind === "turn" && ops[k]!.pivot[0] === op.pivot[0] && ops[k]!.pivot[1] === op.pivot[1]) deg += (ops[k++] as { degCcw: number }).degCcw;
      const turned = carryAngleDeg(deg, 1);
      if (turned !== 0) cmds.push({ op: "rotate_items", ids, pivot: toXY(op.pivot), angle_millideg: -Math.round(turned * 1000) });
    }
    if (dx !== 0 || dy !== 0) cmds.push({ op: "move_items", ids, dx: Math.round(dx), dy: Math.round(dy) });
    return { cmds, ids, lockedOut };
  }
  const turned = carryAngleDeg(turnsCcw, stepDeg);
  if (turned !== 0) {
    const pivot = rotationPivot(board, ids, snap);
    if (pivot) cmds.push({ op: "rotate_items", ids, pivot: toXY(pivot), angle_millideg: -Math.round(turned * 1000) });
  }
  if (flipped) {
    const pivot = flipPivot(board, ids);
    if (pivot) cmds.push({ op: "flip_items", ids, pivot: toXY(pivot), direction });
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
  pivotNm?: [number, number];
  flipPivotNm?: [number, number];
  /** Part of the selection was locked and stayed where it was. */
  lockedOut: boolean;
}

/** `EDIT_TOOL::Move` picking a selection up: the items it works on and the points a turn or a flip during the move acts about. */
export function carryStart(board: BoardState, selection: readonly string[], snap: Snap = NO_SNAP): CarryStart {
  const { ids, lockedOut } = editableSelection(board, selection);
  if (ids.length === 0) return { refs: [], lockedOut };
  const pivot = rotationPivot(board, ids, snap);
  const flip = flipPivot(board, ids);
  return { refs: ids, pivotNm: pivot ?? undefined, flipPivotNm: flip ?? undefined, lockedOut };
}
